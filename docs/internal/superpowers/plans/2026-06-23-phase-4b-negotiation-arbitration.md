# Phase 4b — Negotiation, Arbitration, Deadlock Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make open conflicts resolvable end-to-end — agents submit proposals, Core compares them deterministically and auto-resolves, escalates to user arbitration, or detects deadlock.

**Architecture:** Extend the existing conflict engine (`conflict.rs`, `server.rs`) from Phase 4a. Proposals arrive as new CAP events, are stored on the `Conflict`, and when both participants have proposed Core runs a pure `compare()` to decide auto-resolve vs escalate. A new pure `waitgraph.rs` detects deadlock (mutual `QUEUE_TASK` → cycle). The reaper escalates conflicts whose proposal never arrives. All conflict events are appended to `events.log`.

**Tech Stack:** Rust (edition 2021). Dependencies limited to `serde` + derive, `serde_json`, `chrono`. No new crates.

## Global Constraints

- No new dependencies; only `serde`, `serde_json`, `chrono`.
- Determinism: `compare()` and `find_cycle()` are pure total functions — same inputs → same output. No LLM, no clock/random inside them. Core is the only writer (CAP_SPEC §31).
- Lock discipline: never hold two of `{state, heat, conflict, waitgraph, log}` mutexes at once across a log append. Snapshot under a short lock, compute pure, then log in a separate scope. Same-thread re-lock of a `std::Mutex` deadlocks the suite.
- CAP enum strings are `SCREAMING_SNAKE_CASE`; event field names are `camelCase`; `capVersion` is `"0.1"`.
- Both agents in an arbitration must see identical framing (CAP_SPEC §18.5).
- Coverage gate stays `--fail-under-lines 90`; target ≥95%. Run `cargo test` and `cargo clippy -- -D warnings` clean before each commit.
- Agent FSM state is NOT mutated by Core on conflict events (agents self-report via `AGENT_STATE_CHANGED`). The `Conflict.state` field is negotiation's source of truth.

---

### Task 1: CAP proposal types + new events + ConflictNotFound

**Files:**
- Modify: `packages/coordify-core/src/cap.rs`

**Interfaces:**
- Consumes: existing `CapEvent` enum (`#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]`), `CapErrorCode`, `decode_event`.
- Produces:
  - `pub enum ProposalKind { CoOwn, SplitScope, YieldClaim, TransferTask, QueueTask, AskUser, AbortTask }` with `as_str()` and serde `SCREAMING_SNAKE_CASE` (`CoOwn`→`"CO_OWN"`, `SplitScope`→`"SPLIT_SCOPE"`, `YieldClaim`→`"YIELD_CLAIM"`, `TransferTask`→`"TRANSFER_TASK"`, `QueueTask`→`"QUEUE_TASK"`, `AskUser`→`"ASK_USER"`, `AbortTask`→`"ABORT_TASK"`). Derives `Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize`.
  - `pub struct ClaimChange { pub agent_id: String, pub keep: Option<Vec<String>>, pub take: Option<Vec<String>> }` — serde `camelCase`, `keep`/`take` default `None`.
  - `pub struct Proposal { pub kind: ProposalKind, pub summary: String, pub claim_changes: Vec<ClaimChange>, pub requires_user_approval: bool }` — serde `camelCase`; `summary` default `""`, `claim_changes` default `[]`, `requires_user_approval` default `false`.
  - `CapEvent::ConflictProposalSubmitted { conflict_id: String, from: String, proposal: Proposal }`.
  - `CapEvent::ConflictUserDecision { conflict_id: String, choice: String }`.
  - `CapErrorCode::ConflictNotFound` → `as_str()` `"CONFLICT_NOT_FOUND"`.

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `packages/coordify-core/src/cap.rs`:

```rust
    #[test]
    fn decodes_conflict_proposal_all_kinds() {
        use ProposalKind::*;
        let cases = [
            ("CO_OWN", CoOwn), ("SPLIT_SCOPE", SplitScope), ("YIELD_CLAIM", YieldClaim),
            ("TRANSFER_TASK", TransferTask), ("QUEUE_TASK", QueueTask),
            ("ASK_USER", AskUser), ("ABORT_TASK", AbortTask),
        ];
        for (s, k) in cases {
            let ev = json!({
                "type": "CONFLICT_PROPOSAL_SUBMITTED",
                "conflictId": "conflict-1",
                "from": "agent-1",
                "proposal": {
                    "kind": s,
                    "summary": "do the thing",
                    "claimChanges": [{"agentId":"agent-1","keep":["src/a.rs"]}],
                    "requiresUserApproval": false
                }
            });
            match decode_event(&ev).unwrap() {
                CapEvent::ConflictProposalSubmitted { conflict_id, from, proposal } => {
                    assert_eq!(conflict_id, "conflict-1");
                    assert_eq!(from, "agent-1");
                    assert_eq!(proposal.kind, k);
                    assert_eq!(proposal.summary, "do the thing");
                    assert_eq!(proposal.claim_changes[0].agent_id, "agent-1");
                    assert_eq!(proposal.claim_changes[0].keep.as_ref().unwrap(), &vec!["src/a.rs".to_string()]);
                    assert!(!proposal.requires_user_approval);
                }
                other => panic!("wrong variant: {other:?}"),
            }
        }
    }

    #[test]
    fn proposal_defaults_optional_fields() {
        let ev = json!({
            "type":"CONFLICT_PROPOSAL_SUBMITTED","conflictId":"c1","from":"a1",
            "proposal":{"kind":"CO_OWN"}
        });
        match decode_event(&ev).unwrap() {
            CapEvent::ConflictProposalSubmitted { proposal, .. } => {
                assert_eq!(proposal.summary, "");
                assert!(proposal.claim_changes.is_empty());
                assert!(!proposal.requires_user_approval);
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn decodes_conflict_user_decision() {
        let ev = json!({"type":"CONFLICT_USER_DECISION","conflictId":"c1","choice":"option-2"});
        match decode_event(&ev).unwrap() {
            CapEvent::ConflictUserDecision { conflict_id, choice } => {
                assert_eq!(conflict_id, "c1");
                assert_eq!(choice, "option-2");
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn rejects_bad_proposal_kind() {
        let ev = json!({"type":"CONFLICT_PROPOSAL_SUBMITTED","conflictId":"c1","from":"a1","proposal":{"kind":"NOPE"}});
        assert_eq!(decode_event(&ev).unwrap_err(), CapErrorCode::SchemaValidationFailed);
    }

    #[test]
    fn proposal_kind_as_str_and_conflict_not_found() {
        assert_eq!(ProposalKind::SplitScope.as_str(), "SPLIT_SCOPE");
        assert_eq!(ProposalKind::QueueTask.as_str(), "QUEUE_TASK");
        assert_eq!(serde_json::to_value(ProposalKind::CoOwn).unwrap(), json!("CO_OWN"));
        assert_eq!(CapErrorCode::ConflictNotFound.as_str(), "CONFLICT_NOT_FOUND");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p coordify-core --lib cap:: 2>&1 | tail -20`
Expected: FAIL — `ProposalKind` / `ConflictProposalSubmitted` / `ConflictNotFound` not found (compile errors).

- [ ] **Step 3: Add the types**

In `packages/coordify-core/src/cap.rs`, after the `Intent` impl block (before `ClaimStatus`), add:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProposalKind {
    CoOwn,
    SplitScope,
    YieldClaim,
    TransferTask,
    QueueTask,
    AskUser,
    AbortTask,
}

impl ProposalKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProposalKind::CoOwn => "CO_OWN",
            ProposalKind::SplitScope => "SPLIT_SCOPE",
            ProposalKind::YieldClaim => "YIELD_CLAIM",
            ProposalKind::TransferTask => "TRANSFER_TASK",
            ProposalKind::QueueTask => "QUEUE_TASK",
            ProposalKind::AskUser => "ASK_USER",
            ProposalKind::AbortTask => "ABORT_TASK",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaimChange {
    pub agent_id: String,
    #[serde(default)]
    pub keep: Option<Vec<String>>,
    #[serde(default)]
    pub take: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Proposal {
    pub kind: ProposalKind,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub claim_changes: Vec<ClaimChange>,
    #[serde(default)]
    pub requires_user_approval: bool,
}
```

Add the two variants to the `CapEvent` enum (after `ClearInvoked`):

```rust
    #[serde(rename_all = "camelCase")]
    ConflictProposalSubmitted {
        conflict_id: String,
        from: String,
        proposal: Proposal,
    },
    #[serde(rename_all = "camelCase")]
    ConflictUserDecision {
        conflict_id: String,
        choice: String,
    },
```

Add the error variant to `CapErrorCode` (after `ClaimNotFound`):

```rust
    ConflictNotFound,
```

And its arm in `CapErrorCode::as_str()` (after the `ClaimNotFound` arm):

```rust
            CapErrorCode::ConflictNotFound => "CONFLICT_NOT_FOUND",
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p coordify-core --lib cap:: 2>&1 | tail -20`
Expected: PASS (all `cap::tests` green, including the new tests).

- [ ] **Step 5: Commit**

```bash
git add packages/coordify-core/src/cap.rs
git commit -m "feat(core): CAP proposal types, conflict negotiation events, ConflictNotFound"
```

---

### Task 2: Conflict proposals, store methods, compare(), ConflictConfig

**Files:**
- Modify: `packages/coordify-core/src/conflict.rs`

**Interfaces:**
- Consumes: `crate::cap::{Proposal, ProposalKind}` (Task 1); existing `Conflict`, `ConflictState`, `ConflictStore`, `key()`.
- Produces:
  - `Conflict` gains `pub proposals: HashMap<String, Proposal>` and `pub opened_at_ms: u64`.
  - `ConflictStore::open(&mut self, a, b, trigger_heat, opened_at_ms: u64, paths, domains, intents) -> Option<Conflict>` (new `opened_at_ms` param, inserted after `trigger_heat`).
  - `ConflictStore::record_proposal(&mut self, conflict_id: &str, from: &str, proposal: Proposal) -> bool` — false if no open conflict with that id, or `from` is not a participant; on success stores the proposal and sets state `Negotiating`.
  - `ConflictStore::both_proposed(&self, conflict_id: &str) -> bool`.
  - `ConflictStore::get_by_id(&self, conflict_id: &str) -> Option<&Conflict>`.
  - `ConflictStore::set_state(&mut self, conflict_id: &str, state: ConflictState)`.
  - `ConflictStore::resolve_by_id(&mut self, conflict_id: &str) -> Option<Conflict>` — removes and marks `Resolved`.
  - `ConflictStore::timed_out(&self, now_ms: u64, timeout_ms: u64) -> Vec<String>` — ids of open conflicts where `now - opened_at_ms > timeout_ms`, not both-proposed, and state ∈ {`Detected`, `Negotiating`, `AwaitingAgentResponse`}.
  - `Conflict::proposals_sorted(&self) -> Vec<(&String, &Proposal)>` — entries sorted by agent id (deterministic arbitration framing).
  - `pub enum Decision { AutoResolve { resolution: &'static str }, Escalate { reason: &'static str } }` (derive `Debug, PartialEq, Eq`).
  - `pub fn compare(a: &Proposal, b: &Proposal, paths: &[String], cfg: &ConflictConfig) -> Decision`.
  - `pub struct ConflictConfig { pub protected_paths: Vec<String>, pub allow_co_own: bool, pub proposal_timeout_ms: u64 }` + `Default` (`protected_paths: vec![]`, `allow_co_own: true`, `proposal_timeout_ms: 60_000`).

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `packages/coordify-core/src/conflict.rs`:

```rust
    use crate::cap::{ClaimChange, Proposal, ProposalKind};

    fn prop(kind: ProposalKind, files: &[&str]) -> Proposal {
        Proposal {
            kind,
            summary: "s".into(),
            claim_changes: vec![ClaimChange {
                agent_id: "x".into(),
                keep: Some(files.iter().map(|s| s.to_string()).collect()),
                take: None,
            }],
            requires_user_approval: false,
        }
    }

    #[test]
    fn open_records_opened_at_and_empty_proposals() {
        let mut s = ConflictStore::new();
        let c = s.open("a", "b", 80, 12345, vec!["f".into()], vec![], vec![]).unwrap();
        assert_eq!(c.opened_at_ms, 12345);
        assert!(c.proposals.is_empty());
        assert_eq!(c.state, ConflictState::Detected);
    }

    #[test]
    fn record_proposal_sets_negotiating_and_validates_participant() {
        let mut s = ConflictStore::new();
        let c = s.open("a", "b", 80, 0, vec![], vec![], vec![]).unwrap();
        let id = c.conflict_id.clone();
        assert!(!s.record_proposal("conflict-404", "a", prop(ProposalKind::CoOwn, &[])));
        assert!(!s.record_proposal(&id, "stranger", prop(ProposalKind::CoOwn, &[])));
        assert!(s.record_proposal(&id, "a", prop(ProposalKind::CoOwn, &[])));
        assert_eq!(s.get_by_id(&id).unwrap().state, ConflictState::Negotiating);
        assert!(!s.both_proposed(&id));
        assert!(s.record_proposal(&id, "b", prop(ProposalKind::CoOwn, &[])));
        assert!(s.both_proposed(&id));
    }

    #[test]
    fn set_state_resolve_by_id_and_timed_out() {
        let mut s = ConflictStore::new();
        let id = s.open("a", "b", 80, 1000, vec![], vec![], vec![]).unwrap().conflict_id;
        // not timed out yet
        assert!(s.timed_out(1500, 1000).is_empty());
        // age 1001 > 1000 -> timed out (no proposals)
        assert_eq!(s.timed_out(2001, 1000), vec![id.clone()]);
        // once awaiting user, it is no longer swept
        s.set_state(&id, ConflictState::AwaitingUserDecision);
        assert!(s.timed_out(9999, 1000).is_empty());
        let r = s.resolve_by_id(&id).unwrap();
        assert_eq!(r.state, ConflictState::Resolved);
        assert!(s.resolve_by_id(&id).is_none());
    }

    #[test]
    fn compare_covers_every_branch() {
        let cfg = ConflictConfig::default();
        let no_paths: Vec<String> = vec![];
        // requiresUserApproval -> escalate
        let mut p = prop(ProposalKind::CoOwn, &[]); p.requires_user_approval = true;
        assert_eq!(compare(&p, &prop(ProposalKind::CoOwn, &[]), &no_paths, &cfg),
                   Decision::Escalate { reason: "USER_APPROVAL_REQUIRED" });
        // ASK_USER -> escalate
        assert_eq!(compare(&prop(ProposalKind::AskUser, &[]), &prop(ProposalKind::CoOwn, &[]), &no_paths, &cfg),
                   Decision::Escalate { reason: "AGENT_REQUESTED_USER" });
        // protected path -> escalate
        let pcfg = ConflictConfig { protected_paths: vec!["src/auth/".into()], ..ConflictConfig::default() };
        assert_eq!(compare(&prop(ProposalKind::CoOwn, &[]), &prop(ProposalKind::CoOwn, &[]),
                           &["src/auth/session.ts".to_string()], &pcfg),
                   Decision::Escalate { reason: "PROTECTED_PATH" });
        // one yields -> auto-resolve
        assert_eq!(compare(&prop(ProposalKind::YieldClaim, &[]), &prop(ProposalKind::CoOwn, &[]), &no_paths, &cfg),
                   Decision::AutoResolve { resolution: "PARTICIPANT_STEPPED_ASIDE" });
        // exactly one queue -> auto-resolve
        assert_eq!(compare(&prop(ProposalKind::QueueTask, &[]), &prop(ProposalKind::CoOwn, &[]), &no_paths, &cfg),
                   Decision::AutoResolve { resolution: "QUEUED" });
        // both split disjoint -> auto-resolve
        assert_eq!(compare(&prop(ProposalKind::SplitScope, &["a.rs"]), &prop(ProposalKind::SplitScope, &["b.rs"]), &no_paths, &cfg),
                   Decision::AutoResolve { resolution: "SCOPE_SPLIT" });
        // both split overlapping -> escalate
        assert_eq!(compare(&prop(ProposalKind::SplitScope, &["a.rs"]), &prop(ProposalKind::SplitScope, &["a.rs"]), &no_paths, &cfg),
                   Decision::Escalate { reason: "OVERLAPPING_SPLIT" });
        // both co-own, allowed -> auto-resolve
        assert_eq!(compare(&prop(ProposalKind::CoOwn, &[]), &prop(ProposalKind::CoOwn, &[]), &no_paths, &cfg),
                   Decision::AutoResolve { resolution: "CO_OWNERSHIP" });
        // both co-own, disallowed -> escalate
        let nocoown = ConflictConfig { allow_co_own: false, ..ConflictConfig::default() };
        assert_eq!(compare(&prop(ProposalKind::CoOwn, &[]), &prop(ProposalKind::CoOwn, &[]), &no_paths, &nocoown),
                   Decision::Escalate { reason: "CO_OWN_DISALLOWED" });
        // mixed incompatible (co-own vs split) -> escalate
        assert_eq!(compare(&prop(ProposalKind::CoOwn, &[]), &prop(ProposalKind::SplitScope, &["a.rs"]), &no_paths, &cfg),
                   Decision::Escalate { reason: "INCOMPATIBLE_PROPOSALS" });
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p coordify-core --lib conflict:: 2>&1 | tail -20`
Expected: FAIL — `compare` / `Decision` / `ConflictConfig` / new methods not found.

- [ ] **Step 3: Implement**

In `packages/coordify-core/src/conflict.rs`:

Update the imports at the top:

```rust
use crate::cap::{Proposal, ProposalKind};
use serde::Serialize;
use std::collections::{BTreeSet, HashMap};
```

Add the two new fields to `struct Conflict` (after `intents`):

```rust
    pub proposals: HashMap<String, Proposal>,
    pub opened_at_ms: u64,
```

Add `proposals_sorted` to a new `impl Conflict` block (place it right after the `struct Conflict { .. }` definition):

```rust
impl Conflict {
    /// Proposals ordered by agent id — deterministic arbitration framing (§18.5).
    pub fn proposals_sorted(&self) -> Vec<(&String, &Proposal)> {
        let mut v: Vec<(&String, &Proposal)> = self.proposals.iter().collect();
        v.sort_by(|x, y| x.0.cmp(y.0));
        v
    }
}
```

Change `ConflictStore::open` to accept `opened_at_ms` and initialise the new fields. Replace the existing `open` method body's signature line and `Conflict { .. }` literal:

```rust
    /// Open a conflict for the pair. Returns None if one is already open.
    pub fn open(
        &mut self,
        a: &str,
        b: &str,
        trigger_heat: u32,
        opened_at_ms: u64,
        paths: Vec<String>,
        domains: Vec<String>,
        intents: Vec<String>,
    ) -> Option<Conflict> {
        let k = key(a, b);
        if self.open.contains_key(&k) {
            return None;
        }
        self.next_id += 1;
        let conflict = Conflict {
            conflict_id: format!("conflict-{}", self.next_id),
            agents: k.clone(),
            state: ConflictState::Detected,
            trigger_heat,
            paths,
            domains,
            intents,
            proposals: HashMap::new(),
            opened_at_ms,
        };
        self.open.insert(k, conflict.clone());
        Some(conflict)
    }
```

Add these methods to `impl ConflictStore` (after `resolve`):

```rust
    /// Record a proposal from `from` for the conflict. Returns false if the
    /// conflict is unknown or `from` is not a participant. Moves the conflict
    /// to NEGOTIATING.
    pub fn record_proposal(&mut self, conflict_id: &str, from: &str, proposal: Proposal) -> bool {
        for c in self.open.values_mut() {
            if c.conflict_id == conflict_id {
                if c.agents.0 != from && c.agents.1 != from {
                    return false;
                }
                c.proposals.insert(from.to_string(), proposal);
                c.state = ConflictState::Negotiating;
                return true;
            }
        }
        false
    }

    pub fn both_proposed(&self, conflict_id: &str) -> bool {
        self.open
            .values()
            .find(|c| c.conflict_id == conflict_id)
            .map(|c| c.proposals.contains_key(&c.agents.0) && c.proposals.contains_key(&c.agents.1))
            .unwrap_or(false)
    }

    pub fn get_by_id(&self, conflict_id: &str) -> Option<&Conflict> {
        self.open.values().find(|c| c.conflict_id == conflict_id)
    }

    pub fn set_state(&mut self, conflict_id: &str, state: ConflictState) {
        for c in self.open.values_mut() {
            if c.conflict_id == conflict_id {
                c.state = state;
                return;
            }
        }
    }

    /// Resolve (remove) the conflict by id. Returns it with state Resolved.
    pub fn resolve_by_id(&mut self, conflict_id: &str) -> Option<Conflict> {
        let k = self
            .open
            .iter()
            .find(|(_, c)| c.conflict_id == conflict_id)
            .map(|(k, _)| k.clone())?;
        let mut c = self.open.remove(&k)?;
        c.state = ConflictState::Resolved;
        Some(c)
    }

    /// Ids of open conflicts that have aged past `timeout_ms` without both
    /// proposals and are still awaiting them (not yet escalated/resolved).
    pub fn timed_out(&self, now_ms: u64, timeout_ms: u64) -> Vec<String> {
        self.open
            .values()
            .filter(|c| {
                now_ms.saturating_sub(c.opened_at_ms) > timeout_ms
                    && !(c.proposals.contains_key(&c.agents.0) && c.proposals.contains_key(&c.agents.1))
                    && matches!(
                        c.state,
                        ConflictState::Detected
                            | ConflictState::Negotiating
                            | ConflictState::AwaitingAgentResponse
                    )
            })
            .map(|c| c.conflict_id.clone())
            .collect()
    }
```

Add the `Decision` enum, `compare`, and `ConflictConfig` at the end of the file (before `#[cfg(test)]`):

```rust
#[derive(Debug, Clone)]
pub struct ConflictConfig {
    pub protected_paths: Vec<String>,
    pub allow_co_own: bool,
    pub proposal_timeout_ms: u64,
}

impl Default for ConflictConfig {
    fn default() -> Self {
        Self { protected_paths: Vec::new(), allow_co_own: true, proposal_timeout_ms: 60_000 }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    AutoResolve { resolution: &'static str },
    Escalate { reason: &'static str },
}

fn proposal_files(p: &Proposal) -> BTreeSet<String> {
    let mut s = BTreeSet::new();
    for c in &p.claim_changes {
        if let Some(k) = &c.keep {
            s.extend(k.iter().cloned());
        }
        if let Some(t) = &c.take {
            s.extend(t.iter().cloned());
        }
    }
    s
}

fn protected(paths: &[String], cfg: &ConflictConfig) -> bool {
    paths.iter().any(|p| {
        cfg.protected_paths
            .iter()
            .any(|pp| !pp.is_empty() && p.starts_with(pp.as_str()))
    })
}

/// Pure deterministic comparison of two proposals (§18.4). Evaluated in order.
/// Note: mutual QUEUE_TASK is handled as a deadlock by the server before this
/// is called, so the single-queue branch here only ever sees one queue.
pub fn compare(a: &Proposal, b: &Proposal, paths: &[String], cfg: &ConflictConfig) -> Decision {
    use ProposalKind::*;
    if a.requires_user_approval || b.requires_user_approval {
        return Decision::Escalate { reason: "USER_APPROVAL_REQUIRED" };
    }
    if a.kind == AskUser || b.kind == AskUser {
        return Decision::Escalate { reason: "AGENT_REQUESTED_USER" };
    }
    if protected(paths, cfg) {
        return Decision::Escalate { reason: "PROTECTED_PATH" };
    }
    let steps_aside = |k: ProposalKind| matches!(k, YieldClaim | AbortTask | TransferTask);
    if steps_aside(a.kind) || steps_aside(b.kind) {
        return Decision::AutoResolve { resolution: "PARTICIPANT_STEPPED_ASIDE" };
    }
    if (a.kind == QueueTask) ^ (b.kind == QueueTask) {
        return Decision::AutoResolve { resolution: "QUEUED" };
    }
    if a.kind == SplitScope && b.kind == SplitScope {
        return if proposal_files(a).is_disjoint(&proposal_files(b)) {
            Decision::AutoResolve { resolution: "SCOPE_SPLIT" }
        } else {
            Decision::Escalate { reason: "OVERLAPPING_SPLIT" }
        };
    }
    if a.kind == CoOwn && b.kind == CoOwn {
        return if cfg.allow_co_own {
            Decision::AutoResolve { resolution: "CO_OWNERSHIP" }
        } else {
            Decision::Escalate { reason: "CO_OWN_DISALLOWED" }
        };
    }
    Decision::Escalate { reason: "INCOMPATIBLE_PROPOSALS" }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p coordify-core --lib conflict:: 2>&1 | tail -20`
Expected: PASS. (Note: `server.rs` still calls the old `open` signature — it will not compile yet. Confirm `conflict::tests` pass by checking the failure is only the unrelated `server.rs` call site. If the crate fails to compile, temporarily run only this file's tests is not possible; instead proceed — Task 4 fixes the call site. To keep this task self-contained, also apply the one-line call-site fix below so the crate compiles and commits green.)

Call-site fix in `packages/coordify-core/src/server.rs` `recompute_current_heat` — change the `open(...)` call to pass `now_ms()`. Replace the line that computes `paths`/`domains`/`intents` and calls `cstore.open(agent_id, other_id, result.heat, paths, domains, intents)` so it reads:

```rust
                    if let Some(c) = cstore.open(agent_id, other_id, result.heat, now_ms(), paths, domains, intents) {
```

(`now_ms` is already imported in `server.rs`.)

Re-run: `cargo test -p coordify-core --lib 2>&1 | tail -15`
Expected: PASS (whole lib compiles and is green).

- [ ] **Step 5: Commit**

```bash
git add packages/coordify-core/src/conflict.rs packages/coordify-core/src/server.rs
git commit -m "feat(core): conflict proposals, store methods, pure compare(), ConflictConfig"
```

---

### Task 3: Wait graph for deadlock detection

**Files:**
- Create: `packages/coordify-core/src/waitgraph.rs`
- Modify: `packages/coordify-core/src/lib.rs`

**Interfaces:**
- Produces:
  - `pub struct WaitEdge { pub from: String, pub to: String, pub resource: String }` — serde `camelCase`, derives `Debug, Clone, PartialEq, Eq, Serialize`.
  - `pub struct WaitGraph` (default-constructible; `new()`).
  - `WaitGraph::add_edge(&mut self, from: &str, to: &str, resource: &str)` — idempotent on `(from, to)`.
  - `WaitGraph::remove_agent(&mut self, agent: &str)` — drops every edge touching `agent`.
  - `WaitGraph::find_cycle(&self) -> Option<Vec<WaitEdge>>` — returns the edges of a directed cycle if one exists.

- [ ] **Step 1: Write the failing tests**

Create `packages/coordify-core/src/waitgraph.rs` with the test module first (implementation in Step 3):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_two_cycle() {
        let mut g = WaitGraph::new();
        g.add_edge("a", "b", "f1");
        g.add_edge("b", "a", "f2");
        let cyc = g.find_cycle().expect("expected a cycle");
        assert_eq!(cyc.len(), 2);
        // both agents appear as a `from`
        let froms: std::collections::BTreeSet<&str> = cyc.iter().map(|e| e.from.as_str()).collect();
        assert!(froms.contains("a") && froms.contains("b"));
    }

    #[test]
    fn no_cycle_in_dag() {
        let mut g = WaitGraph::new();
        g.add_edge("a", "b", "f1");
        g.add_edge("b", "c", "f2");
        assert!(g.find_cycle().is_none());
    }

    #[test]
    fn remove_agent_breaks_cycle_and_dedupes() {
        let mut g = WaitGraph::new();
        g.add_edge("a", "b", "f1");
        g.add_edge("a", "b", "f1"); // duplicate ignored
        g.add_edge("b", "a", "f2");
        assert!(g.find_cycle().is_some());
        g.remove_agent("a");
        assert!(g.find_cycle().is_none());
    }

    #[test]
    fn direction_matters() {
        let mut g = WaitGraph::new();
        // a waits on b twice in the SAME direction -> no cycle
        g.add_edge("a", "b", "f1");
        assert!(g.find_cycle().is_none());
    }

    #[test]
    fn detects_three_cycle() {
        let mut g = WaitGraph::new();
        g.add_edge("a", "b", "f1");
        g.add_edge("b", "c", "f2");
        g.add_edge("c", "a", "f3");
        assert_eq!(g.find_cycle().expect("cycle").len(), 3);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

First register the module: in `packages/coordify-core/src/lib.rs`, add `pub mod waitgraph;` (alongside the other `pub mod` lines).

Run: `cargo test -p coordify-core --lib waitgraph:: 2>&1 | tail -20`
Expected: FAIL — `WaitGraph` not found.

- [ ] **Step 3: Implement**

Prepend to `packages/coordify-core/src/waitgraph.rs` (above the test module):

```rust
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WaitEdge {
    pub from: String,
    pub to: String,
    pub resource: String,
}

/// Directed wait-for graph. An edge `from -> to` means agent `from` is waiting
/// on agent `to` for `resource`. A cycle is a deadlock (CAP_SPEC §20).
#[derive(Default)]
pub struct WaitGraph {
    edges: Vec<WaitEdge>,
}

impl WaitGraph {
    pub fn new() -> Self {
        Self { edges: Vec::new() }
    }

    /// Add `from -> to`; ignores an existing edge in the same direction.
    pub fn add_edge(&mut self, from: &str, to: &str, resource: &str) {
        if self
            .edges
            .iter()
            .any(|e| e.from.as_str() == from && e.to.as_str() == to)
        {
            return;
        }
        self.edges.push(WaitEdge {
            from: from.to_string(),
            to: to.to_string(),
            resource: resource.to_string(),
        });
    }

    pub fn remove_agent(&mut self, agent: &str) {
        self.edges
            .retain(|e| e.from.as_str() != agent && e.to.as_str() != agent);
    }

    /// Return the edges of a directed cycle if one exists (DFS, three-colour).
    pub fn find_cycle(&self) -> Option<Vec<WaitEdge>> {
        let mut adj: HashMap<&str, Vec<&WaitEdge>> = HashMap::new();
        for e in &self.edges {
            adj.entry(e.from.as_str()).or_default().push(e);
        }
        // 0 = unvisited, 1 = on current DFS stack, 2 = done
        let mut state: HashMap<&str, u8> = HashMap::new();
        let mut order: Vec<&str> = Vec::new();
        for e in &self.edges {
            for n in [e.from.as_str(), e.to.as_str()] {
                if !state.contains_key(n) {
                    state.insert(n, 0);
                    order.push(n);
                }
            }
        }
        for &start in &order {
            if state[start] == 0 {
                let mut path: Vec<&WaitEdge> = Vec::new();
                if let Some(c) = Self::dfs(start, &adj, &mut state, &mut path) {
                    return Some(c);
                }
            }
        }
        None
    }

    fn dfs<'a>(
        node: &'a str,
        adj: &HashMap<&'a str, Vec<&'a WaitEdge>>,
        state: &mut HashMap<&'a str, u8>,
        path: &mut Vec<&'a WaitEdge>,
    ) -> Option<Vec<WaitEdge>> {
        state.insert(node, 1);
        if let Some(outs) = adj.get(node) {
            for e in outs {
                let to = e.to.as_str();
                match state.get(to).copied().unwrap_or(0) {
                    1 => {
                        // Back-edge: cycle runs from where `to` entered the path.
                        let start_idx =
                            path.iter().position(|pe| pe.from.as_str() == to).unwrap_or(0);
                        let mut cyc: Vec<WaitEdge> =
                            path[start_idx..].iter().map(|pe| (*pe).clone()).collect();
                        cyc.push((*e).clone());
                        return Some(cyc);
                    }
                    0 => {
                        path.push(e);
                        if let Some(c) = Self::dfs(to, adj, state, path) {
                            return Some(c);
                        }
                        path.pop();
                    }
                    _ => {}
                }
            }
        }
        state.insert(node, 2);
        None
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p coordify-core --lib waitgraph:: 2>&1 | tail -20`
Expected: PASS (all 5 `waitgraph::tests` green).

- [ ] **Step 5: Commit**

```bash
git add packages/coordify-core/src/waitgraph.rs packages/coordify-core/src/lib.rs
git commit -m "feat(core): wait-for graph with cycle detection for deadlock"
```

---

### Task 4: Server — proposal ingestion, compare, deadlock, escalation

**Files:**
- Modify: `packages/coordify-core/src/server.rs`

**Interfaces:**
- Consumes: `crate::conflict::{compare, Decision, ConflictConfig, ConflictState, Conflict}`; `crate::waitgraph::WaitGraph`; `crate::cap::{ProposalKind}`; Task 1 `CapEvent::ConflictProposalSubmitted`.
- Produces: `Shared` gains `pub conflict_cfg: ConflictConfig` and `pub waitgraph: Mutex<WaitGraph>`; a free fn `build_arbitration(c: &Conflict) -> serde_json::Value`; negotiation handled inside `handle_cap_event`.

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `packages/coordify-core/src/server.rs` (helpers `shared_for_test`, `req`, `cap_req` already exist):

```rust
    fn open_conflict_between_two(s: &Arc<Shared>) -> (String, String, String) {
        let a = handle_request(s, &{ let mut r = req("good", "register"); r.meta = json!({"branch":"main"}); r }).agent_id.unwrap();
        let b = handle_request(s, &{ let mut r = req("good", "register"); r.meta = json!({"branch":"main"}); r }).agent_id.unwrap();
        let mk = |agent: &str| json!({"type":"CLAIM_PROPOSED","agentId":agent,"intent":"BUGFIX","domains":["AUTH"],"estimatedFiles":["src/auth/session.ts"],"task":{"summary":"fix session expiry"},"confidence":0.9});
        assert!(handle_request(s, &cap_req("good", mk(&a))).ok);
        assert!(handle_request(s, &cap_req("good", mk(&b))).ok);
        let id = s.conflicts.lock().unwrap().get_by_id("conflict-1").map(|c| c.conflict_id.clone())
            .or_else(|| { let cs = s.conflicts.lock().unwrap(); cs.has_open(&a, &b).then(|| "conflict-1".to_string()) })
            .expect("a conflict should be open");
        (a, b, id)
    }

    fn proposal_ev(conflict_id: &str, from: &str, kind: &str) -> serde_json::Value {
        json!({"type":"CONFLICT_PROPOSAL_SUBMITTED","conflictId":conflict_id,"from":from,
               "proposal":{"kind":kind,"summary":format!("{from} proposes {kind}"),"claimChanges":[],"requiresUserApproval":false}})
    }

    #[test]
    fn both_compatible_proposals_resolve_conflict() {
        let s = shared_for_test("good");
        let (a, b, id) = open_conflict_between_two(&s);
        assert_eq!(s.conflicts.lock().unwrap().open_count(), 1);
        assert!(handle_request(&s, &cap_req("good", proposal_ev(&id, &a, "YIELD_CLAIM"))).ok);
        assert!(handle_request(&s, &cap_req("good", proposal_ev(&id, &b, "CO_OWN"))).ok);
        // YIELD by one -> auto-resolved -> removed.
        assert_eq!(s.conflicts.lock().unwrap().open_count(), 0);
    }

    #[test]
    fn incompatible_proposals_escalate_to_user() {
        let s = shared_for_test("good");
        let (a, b, id) = open_conflict_between_two(&s);
        assert!(handle_request(&s, &cap_req("good", proposal_ev(&id, &a, "CO_OWN"))).ok);
        assert!(handle_request(&s, &cap_req("good", proposal_ev(&id, &b, "SPLIT_SCOPE"))).ok);
        // Mixed -> escalate. Conflict stays open, AwaitingUserDecision.
        let cs = s.conflicts.lock().unwrap();
        assert_eq!(cs.open_count(), 1);
        assert_eq!(cs.get_by_id(&id).unwrap().state, crate::conflict::ConflictState::AwaitingUserDecision);
    }

    #[test]
    fn mutual_queue_is_deadlock_and_escalates() {
        let s = shared_for_test("good");
        let (a, b, id) = open_conflict_between_two(&s);
        assert!(handle_request(&s, &cap_req("good", proposal_ev(&id, &a, "QUEUE_TASK"))).ok);
        assert!(handle_request(&s, &cap_req("good", proposal_ev(&id, &b, "QUEUE_TASK"))).ok);
        let cs = s.conflicts.lock().unwrap();
        assert_eq!(cs.get_by_id(&id).unwrap().state, crate::conflict::ConflictState::AwaitingUserDecision);
    }

    #[test]
    fn proposal_for_unknown_conflict_errors() {
        let s = shared_for_test("good");
        let (a, _b, _id) = open_conflict_between_two(&s);
        let resp = handle_request(&s, &cap_req("good", proposal_ev("conflict-999", &a, "CO_OWN")));
        assert!(!resp.ok);
        assert_eq!(resp.error.as_deref(), Some("CONFLICT_NOT_FOUND"));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p coordify-core --lib server:: 2>&1 | tail -25`
Expected: FAIL — the new `CONFLICT_PROPOSAL_SUBMITTED` event is decoded but unhandled (today's match would not compile because `CapEvent` is non-exhaustive after Task 1 added variants). Confirm the failures reference the new tests / a non-exhaustive `match`.

- [ ] **Step 3: Implement**

In `packages/coordify-core/src/server.rs`:

Update imports at the top:

```rust
use crate::conflict::{compare, Conflict, ConflictConfig, ConflictState, ConflictStore, Decision};
use crate::cap::{self, CapErrorCode, CapEvent, ClaimStatus, ProposalKind};
use crate::waitgraph::WaitGraph;
```

Add two fields to `struct Shared` (after `conflicts`):

```rust
    pub conflict_cfg: ConflictConfig,
    pub waitgraph: Mutex<WaitGraph>,
}
```

Initialise them in BOTH constructors — in `run()`'s `Shared { .. }` and in the test `shared_for_test`'s `Shared { .. }` — add:

```rust
            conflict_cfg: ConflictConfig::default(),
            waitgraph: Mutex::new(WaitGraph::new()),
```

In `run()`, after the `orphan_ttl_ms` env block, override the proposal timeout from the environment and fold it into the config used by `Shared`. Replace the `conflict_cfg: ConflictConfig::default(),` line in `run()`'s `Shared { .. }` with a value built beforehand:

```rust
    let proposal_timeout_ms = std::env::var("COORDIFY_PROPOSAL_TIMEOUT_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(60_000);
    let conflict_cfg = ConflictConfig { proposal_timeout_ms, ..ConflictConfig::default() };
```

and in `run()`'s `Shared { .. }` use `conflict_cfg,` (shorthand) instead of `ConflictConfig::default()`. Move this block above the `let shared = Arc::new(Shared { .. })` line.

Add the two new match arms inside `handle_cap_event`, after the `CapEvent::ClearInvoked { .. } => { .. }` arm (before the closing `}` of the `match event`):

```rust
        CapEvent::ConflictProposalSubmitted { conflict_id, from, proposal } => {
            let recorded = {
                let mut cs = shared.conflicts.lock().unwrap();
                cs.record_proposal(&conflict_id, &from, proposal.clone())
            };
            if !recorded {
                return cap_err(&req.id, CapErrorCode::ConflictNotFound);
            }
            {
                let _ = shared.log.lock().unwrap().append(&serde_json::json!({
                    "type": "CONFLICT_PROPOSAL_RECEIVED",
                    "conflictId": conflict_id,
                    "from": from,
                    "kind": proposal.kind.as_str(),
                    "summary": proposal.summary,
                    "ts": crate::bootstrap::now_iso(),
                }));
            }
            // When both have proposed, decide synchronously on a snapshot.
            let snapshot = {
                let cs = shared.conflicts.lock().unwrap();
                if cs.both_proposed(&conflict_id) {
                    cs.get_by_id(&conflict_id).cloned()
                } else {
                    None
                }
            };
            if let Some(c) = snapshot {
                finalize_negotiation(shared, &c);
            }
            Response::ok_for(&req.id)
        }
        CapEvent::ConflictUserDecision { conflict_id, choice } => {
            let resolved = {
                let mut cs = shared.conflicts.lock().unwrap();
                cs.resolve_by_id(&conflict_id)
            };
            match resolved {
                None => cap_err(&req.id, CapErrorCode::ConflictNotFound),
                Some(c) => {
                    {
                        let mut wg = shared.waitgraph.lock().unwrap();
                        wg.remove_agent(&c.agents.0);
                        wg.remove_agent(&c.agents.1);
                    }
                    let _ = shared.log.lock().unwrap().append(&serde_json::json!({
                        "type": "CONFLICT_RESOLVED",
                        "conflictId": c.conflict_id,
                        "resolution": "USER_ARBITRATED",
                        "choice": choice,
                        "ts": crate::bootstrap::now_iso(),
                    }));
                    Response::ok_for(&req.id)
                }
            }
        }
```

Add the negotiation helpers as free functions (place them after `recompute_current_heat`, before `handle_conn`):

```rust
/// Build the identical arbitration prompt shown to both agents (§18.5).
fn build_arbitration(c: &Conflict) -> serde_json::Value {
    let options: Vec<serde_json::Value> = c
        .proposals_sorted()
        .iter()
        .enumerate()
        .map(|(i, (agent, p))| {
            serde_json::json!({
                "id": format!("option-{}", i + 1),
                "from": agent,
                "summary": p.summary,
            })
        })
        .collect();
    serde_json::json!({
        "type": "USER_ARBITRATION_REQUIRED",
        "conflictId": c.conflict_id,
        "agents": [c.agents.0, c.agents.1],
        "prompt": "Coordify requires a user decision.",
        "options": options,
        "ts": crate::bootstrap::now_iso(),
    })
}

/// Move a conflict to AWAITING_USER_DECISION and emit escalation + arbitration
/// (plus a DEADLOCK_DETECTED record when caused by a wait cycle).
fn escalate_conflict(
    shared: &Shared,
    c: &Conflict,
    reason: &str,
    deadlock_edges: Option<Vec<crate::waitgraph::WaitEdge>>,
) {
    shared
        .conflicts
        .lock()
        .unwrap()
        .set_state(&c.conflict_id, ConflictState::AwaitingUserDecision);
    let mut log = shared.log.lock().unwrap();
    if let Some(edges) = deadlock_edges {
        let _ = log.append(&serde_json::json!({
            "type": "DEADLOCK_DETECTED",
            "agents": [c.agents.0, c.agents.1],
            "waitEdges": edges,
            "requiredAction": "USER_ARBITRATION",
            "ts": crate::bootstrap::now_iso(),
        }));
    }
    let _ = log.append(&serde_json::json!({
        "type": "CONFLICT_ESCALATED",
        "conflictId": c.conflict_id,
        "reason": reason,
        "ts": crate::bootstrap::now_iso(),
    }));
    let _ = log.append(&build_arbitration(c));
}

/// Both participants have proposed: add wait edges for queue proposals, detect
/// deadlock, otherwise compare and auto-resolve or escalate.
fn finalize_negotiation(shared: &Shared, c: &Conflict) {
    let a = &c.agents.0;
    let b = &c.agents.1;
    let pa = match c.proposals.get(a) {
        Some(p) => p,
        None => return,
    };
    let pb = match c.proposals.get(b) {
        Some(p) => p,
        None => return,
    };
    let resource = c.paths.join(",");
    let both_queue = pa.kind == ProposalKind::QueueTask && pb.kind == ProposalKind::QueueTask;
    let deadlock_edges = {
        let mut wg = shared.waitgraph.lock().unwrap();
        if pa.kind == ProposalKind::QueueTask {
            wg.add_edge(a, b, &resource);
        }
        if pb.kind == ProposalKind::QueueTask {
            wg.add_edge(b, a, &resource);
        }
        if both_queue {
            wg.find_cycle()
        } else {
            None
        }
    };
    if let Some(edges) = deadlock_edges {
        escalate_conflict(shared, c, "DEADLOCK", Some(edges));
        return;
    }
    match compare(pa, pb, &c.paths, &shared.conflict_cfg) {
        Decision::AutoResolve { resolution } => {
            let resolved = {
                let mut cs = shared.conflicts.lock().unwrap();
                cs.resolve_by_id(&c.conflict_id)
            };
            if resolved.is_some() {
                {
                    let mut wg = shared.waitgraph.lock().unwrap();
                    wg.remove_agent(a);
                    wg.remove_agent(b);
                }
                let _ = shared.log.lock().unwrap().append(&serde_json::json!({
                    "type": "CONFLICT_RESOLVED",
                    "conflictId": c.conflict_id,
                    "resolution": resolution,
                    "ts": crate::bootstrap::now_iso(),
                }));
            }
        }
        Decision::Escalate { reason } => escalate_conflict(shared, c, reason, None),
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p coordify-core --lib 2>&1 | tail -15`
Then: `cargo clippy -p coordify-core --all-targets -- -D warnings 2>&1 | tail -10`
Expected: PASS, clippy clean.

- [ ] **Step 5: Commit**

```bash
git add packages/coordify-core/src/server.rs
git commit -m "feat(core): proposal ingestion, compare, deadlock detection, user arbitration"
```

---

### Task 5: Reaper proposal-timeout sweep, waitgraph cleanup on abort, integration test

**Files:**
- Modify: `packages/coordify-core/src/server.rs`
- Modify: `packages/coordify-core/tests/integration.rs`

**Interfaces:**
- Consumes: `finalize_negotiation`, `escalate_conflict`, `build_arbitration` (Task 4); `ConflictStore::timed_out`; `shared.conflict_cfg.proposal_timeout_ms`.
- Produces: reaper escalates timed-out conflicts; `recompute_current_heat`'s no-claim branch also clears the agent from the wait graph; one socket-level integration test.

- [ ] **Step 1: Write the failing tests**

Add a unit test to the `tests` module in `packages/coordify-core/src/server.rs`:

```rust
    #[test]
    fn timed_out_conflict_is_escalated() {
        let s = shared_for_test("good");
        let (_a, _b, id) = open_conflict_between_two(&s);
        // Force the conflict's opened_at_ms far into the past so any timeout fires.
        {
            let mut cs = s.conflicts.lock().unwrap();
            let timed = cs.timed_out(now_ms() + 10_000_000, s.conflict_cfg.proposal_timeout_ms);
            assert_eq!(timed, vec![id.clone()]);
            for cid in &timed {
                cs.set_state(cid, crate::conflict::ConflictState::AwaitingUserDecision);
            }
        }
        assert_eq!(
            s.conflicts.lock().unwrap().get_by_id(&id).unwrap().state,
            crate::conflict::ConflictState::AwaitingUserDecision
        );
    }
```

Add an integration test to `packages/coordify-core/tests/integration.rs`. The real harness uses `spawn_core(tag) -> Spawned`, `read_token(&core.root)`, `connect_retry(&sock)`, and `send_line(&mut stream, &line) -> serde_json::Value`. Model the new test on the existing `high_overlap_claims_open_conflict` / `releasing_participant_aborts_conflict_over_socket` tests (raw JSON strings, two registers on one stream, poll `.coordify/sessions/*/events.log` after `drop(stream)`):

```rust
#[test]
fn negotiation_resolves_conflict_over_socket() {
    let core = spawn_core("negotiate");
    let token = read_token(&core.root);
    let sock = core.root.join(".coordify/runtime/core.sock");
    let mut stream = connect_retry(&sock);

    let reg_a = format!(r#"{{"id":"1","token":"{}","action":"register","meta":{{"branch":"main"}}}}"#, token);
    let a = send_line(&mut stream, &reg_a)["agent_id"].as_str().unwrap().to_string();
    let reg_b = format!(r#"{{"id":"2","token":"{}","action":"register","meta":{{"branch":"main"}}}}"#, token);
    let b = send_line(&mut stream, &reg_b)["agent_id"].as_str().unwrap().to_string();

    let mk = |id: &str, agent: &str| format!(
        r#"{{"id":"{}","token":"{}","action":"submit_event","capVersion":"0.1","event":{{"type":"CLAIM_PROPOSED","agentId":"{}","intent":"BUGFIX","domains":["AUTH"],"estimatedFiles":["src/auth/session.ts"],"task":{{"summary":"fix session expiry"}},"confidence":0.9}}}}"#,
        id, token, agent
    );
    assert_eq!(send_line(&mut stream, &mk("3", &a))["ok"], true);
    assert_eq!(send_line(&mut stream, &mk("4", &b))["ok"], true); // conflict-1 opens

    // One agent yields, the other co-owns -> Core auto-resolves (PARTICIPANT_STEPPED_ASIDE).
    let prop = |id: &str, agent: &str, kind: &str| format!(
        r#"{{"id":"{}","token":"{}","action":"submit_event","capVersion":"0.1","event":{{"type":"CONFLICT_PROPOSAL_SUBMITTED","conflictId":"conflict-1","from":"{}","proposal":{{"kind":"{}","summary":"{} proposes"}}}}}}"#,
        id, token, agent, kind, agent
    );
    assert_eq!(send_line(&mut stream, &prop("5", &a, "YIELD_CLAIM"))["ok"], true);
    assert_eq!(send_line(&mut stream, &prop("6", &b, "CO_OWN"))["ok"], true);

    drop(stream);

    let sessions = core.root.join(".coordify/sessions");
    let mut log_contents = String::new();
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(3) {
        if let Ok(entries) = std::fs::read_dir(&sessions) {
            for e in entries.flatten() {
                let log = e.path().join("events.log");
                if log.exists() {
                    log_contents = std::fs::read_to_string(log).unwrap();
                }
            }
        }
        if log_contents.contains("CONFLICT_RESOLVED") {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(log_contents.contains("CONFLICT_OPENED"), "expected CONFLICT_OPENED");
    assert!(log_contents.contains("CONFLICT_PROPOSAL_RECEIVED"), "expected proposals logged");
    assert!(log_contents.contains("PARTICIPANT_STEPPED_ASIDE"), "expected negotiated resolution");
}
```

NOTE TO IMPLEMENTER: match the file's established pattern exactly (raw JSON strings, the `Spawned` struct from `spawn_core`, polling the sessions dir). Do not invent a new harness or add `serde_json::json!` helpers if the file uses raw strings.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p coordify-core --lib server::tests::timed_out_conflict_is_escalated 2>&1 | tail -10`
Run: `cargo test -p coordify-core --test integration negotiation_resolves_conflict_over_socket 2>&1 | tail -20`
Expected: unit test PASS already (it only exercises store methods present from Task 2) — if so, keep it as a regression guard. Integration test FAIL until Step 3 wires the reaper sweep (the conflict resolution path itself works from Task 4, so the integration test may already pass; if it passes, that is acceptable — its purpose is end-to-end coverage). The reaper sweep is verified by the unit test plus the timeout-store unit test from Task 2.

- [ ] **Step 3: Implement the reaper sweep and abort cleanup**

In `packages/coordify-core/src/server.rs`, in `recompute_current_heat`, the no-live-claim branch currently calls `abort_for_agent`. Add wait-graph cleanup in its own scope right after `shared.heat.lock().unwrap().remove_agent(agent_id);`:

```rust
            shared.heat.lock().unwrap().remove_agent(agent_id);
            shared.waitgraph.lock().unwrap().remove_agent(agent_id);
```

In `spawn_reaper`, after the existing `lost/orphaned/reclaimable` log block and before the empty-network finalize block, add the conflict-timeout sweep:

```rust
        // Proposal-timeout sweep (§18.6): escalate conflicts that aged out
        // without both proposals. Snapshot under a short conflict lock, then log.
        let timed: Vec<Conflict> = {
            let mut cs = shared.conflicts.lock().unwrap();
            let ids = cs.timed_out(now, shared.conflict_cfg.proposal_timeout_ms);
            let mut snaps = Vec::new();
            for id in &ids {
                cs.set_state(id, ConflictState::AwaitingUserDecision);
                if let Some(c) = cs.get_by_id(id) {
                    snaps.push(c.clone());
                }
            }
            snaps
        };
        if !timed.is_empty() {
            let mut log = shared.log.lock().unwrap();
            for c in &timed {
                let _ = log.append(&serde_json::json!({
                    "type": "CONFLICT_TIMEOUT",
                    "conflictId": c.conflict_id,
                    "ts": crate::bootstrap::now_iso(),
                }));
                let _ = log.append(&build_arbitration(c));
            }
        }
```

(`Conflict` and `ConflictState` are imported from Task 4; `build_arbitration` is in this module.)

- [ ] **Step 4: Run the full suite + clippy**

Run: `cargo test -p coordify-core 2>&1 | tail -20`
Run: `cargo clippy -p coordify-core --all-targets -- -D warnings 2>&1 | tail -10`
Expected: ALL tests PASS (lib + integration), clippy clean.

- [ ] **Step 5: Commit**

```bash
git add packages/coordify-core/src/server.rs packages/coordify-core/tests/integration.rs
git commit -m "feat(core): reaper proposal-timeout sweep, waitgraph cleanup, negotiation e2e test"
```

---

## Notes for the Final Whole-Branch Review

- Verify lock discipline across `finalize_negotiation`, `escalate_conflict`, and the reaper sweep: no two of `{state, heat, conflict, waitgraph, log}` held simultaneously across a log append. Each `.lock()` scope should close before the next opens.
- Verify determinism: `compare()` and `find_cycle()` take no clock/random; arbitration options are emitted in agent-id-sorted order (`proposals_sorted`).
- Confirm `CONFLICT_RESOLVED` resolution strings match the spec set: `PARTICIPANT_STEPPED_ASIDE`, `QUEUED`, `SCOPE_SPLIT`, `CO_OWNERSHIP`, `USER_ARBITRATED`, and the Phase 4a `AUTO_RESOLVED_HEAT_DROPPED`.
- Confirm coverage ≥90% (target ≥95%) via `cargo llvm-cov --summary-only`.
