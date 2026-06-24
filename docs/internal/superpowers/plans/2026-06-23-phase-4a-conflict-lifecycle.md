# Phase 4a — Conflict Lifecycle Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Open and close CONFLICT objects driven by current heat — open at the `CONFLICT_CANDIDATE` band, auto-resolve when heat drops, abort when a participant's claim goes away.

**Architecture:** A pure `conflict.rs` (ordered-pair `ConflictStore`). `server.rs` drives it entirely from the existing `recompute_current_heat`: after each heat edge is computed, open/resolve the pair's conflict by band; in the no-live-claim branch, abort the agent's open conflicts. Conflict events append under the existing log lock. `server.rs` stays the only IO+state site.

**Tech Stack:** Rust 2021, serde + serde_json + chrono (no new deps). Builds on merged Phase 3.

**Spec:** `docs/superpowers/specs/2026-06-23-phase-4a-conflict-lifecycle-design.md`; CAP_SPEC §17/§19.

## Global Constraints

- No new dependencies. Conflicts are deterministic from heat (no LLM). Core is the only writer.
- A conflict opens iff a pair's current-heat band is `CONFLICT_CANDIDATE` (heat ≥ 76, i.e. > `overlap_max` 75) and none is open for that pair. It resolves when the band drops below `CONFLICT_CANDIDATE`. It aborts when a participant loses its live claim.
- One open conflict per ordered agent pair. Conflict ids are sequential `conflict-N`.
- Lock discipline (carried from Phase 1–3): never hold two of {state, heat, conflict, log} simultaneously across a log append. Snapshot/mutate each under a short scope; append events last under the log lock.
- `ConflictState` enum carries the full §17 state set; Phase 4a uses only `Detected`, `Resolved`, `Aborted`.
- macOS/Linux only; std-only concurrency.

---

## File Structure

```text
packages/coordify-core/src/
  conflict.rs  NEW  Conflict, ConflictState, ConflictStore.
  server.rs    MOD  ConflictStore in Shared; open/resolve in recompute_current_heat;
                    abort in the no-live-claim branch; conflict events.
  lib.rs       MOD  pub mod conflict;
```

---

## Task 1: Conflict store (`conflict.rs`)

**Files:** Create `src/conflict.rs`; modify `src/lib.rs` (`pub mod conflict;`).

**Interfaces — Produces:**
- `conflict::ConflictState { Detected, Negotiating, AwaitingAgentResponse, AwaitingUserDecision, Resolved, Timeout, Aborted }` (serde SCREAMING_SNAKE; `as_str`).
- `conflict::Conflict { conflict_id: String, agents: (String, String), state: ConflictState, trigger_heat: u32, paths: Vec<String>, domains: Vec<String>, intents: Vec<String> }` (Clone).
- `conflict::ConflictStore` (Default): `new()`, `has_open(a,b)->bool`, `open(a,b,trigger_heat,paths,domains,intents)->Option<Conflict>` (None if already open), `resolve(a,b)->Option<Conflict>` (removes + marks Resolved), `abort_for_agent(agent)->Vec<Conflict>` (removes + marks Aborted), `open_count()->usize`.
- Edges keyed by ordered pair so direction does not matter.

- [ ] **Step 1: Add `pub mod conflict;` to `src/lib.rs`** (after `pub mod claim;` or alongside the other modules).

- [ ] **Step 2: Write `src/conflict.rs`**

```rust
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ConflictState {
    Detected,
    Negotiating,
    AwaitingAgentResponse,
    AwaitingUserDecision,
    Resolved,
    Timeout,
    Aborted,
}

impl ConflictState {
    pub fn as_str(&self) -> &'static str {
        match self {
            ConflictState::Detected => "DETECTED",
            ConflictState::Negotiating => "NEGOTIATING",
            ConflictState::AwaitingAgentResponse => "AWAITING_AGENT_RESPONSE",
            ConflictState::AwaitingUserDecision => "AWAITING_USER_DECISION",
            ConflictState::Resolved => "RESOLVED",
            ConflictState::Timeout => "TIMEOUT",
            ConflictState::Aborted => "ABORTED",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Conflict {
    pub conflict_id: String,
    pub agents: (String, String),
    pub state: ConflictState,
    pub trigger_heat: u32,
    pub paths: Vec<String>,
    pub domains: Vec<String>,
    pub intents: Vec<String>,
}

fn key(a: &str, b: &str) -> (String, String) {
    if a <= b {
        (a.to_string(), b.to_string())
    } else {
        (b.to_string(), a.to_string())
    }
}

/// Holds currently-open conflicts (one per ordered agent pair). Resolved/aborted
/// conflicts are removed; their lifecycle is recorded in the event log.
#[derive(Default)]
pub struct ConflictStore {
    open: HashMap<(String, String), Conflict>,
    next_id: u64,
}

impl ConflictStore {
    pub fn new() -> Self {
        Self { open: HashMap::new(), next_id: 0 }
    }

    pub fn has_open(&self, a: &str, b: &str) -> bool {
        self.open.contains_key(&key(a, b))
    }

    /// Open a conflict for the pair. Returns None if one is already open.
    pub fn open(
        &mut self,
        a: &str,
        b: &str,
        trigger_heat: u32,
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
        };
        self.open.insert(k, conflict.clone());
        Some(conflict)
    }

    /// Resolve (remove) the pair's open conflict. Returns it with state Resolved.
    pub fn resolve(&mut self, a: &str, b: &str) -> Option<Conflict> {
        let mut c = self.open.remove(&key(a, b))?;
        c.state = ConflictState::Resolved;
        Some(c)
    }

    /// Abort (remove) every open conflict involving `agent`. Returns them with
    /// state Aborted.
    pub fn abort_for_agent(&mut self, agent: &str) -> Vec<Conflict> {
        let keys: Vec<(String, String)> = self
            .open
            .keys()
            .filter(|(x, y)| x == agent || y == agent)
            .cloned()
            .collect();
        let mut out = Vec::new();
        for k in keys {
            let mut c = self.open.remove(&k).unwrap();
            c.state = ConflictState::Aborted;
            out.push(c);
        }
        out
    }

    pub fn open_count(&self) -> usize {
        self.open.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_as_str_values() {
        assert_eq!(ConflictState::Detected.as_str(), "DETECTED");
        assert_eq!(ConflictState::AwaitingUserDecision.as_str(), "AWAITING_USER_DECISION");
        assert_eq!(ConflictState::Aborted.as_str(), "ABORTED");
        assert_eq!(serde_json::to_value(ConflictState::Resolved).unwrap(), serde_json::json!("RESOLVED"));
    }

    #[test]
    fn open_is_idempotent_per_pair_and_direction_independent() {
        let mut s = ConflictStore::new();
        let c1 = s.open("agent-2", "agent-1", 80, vec!["f".into()], vec!["AUTH".into()], vec!["BUGFIX".into()]);
        assert!(c1.is_some());
        let c = c1.unwrap();
        assert_eq!(c.conflict_id, "conflict-1");
        assert_eq!(c.agents, ("agent-1".to_string(), "agent-2".to_string())); // ordered
        assert_eq!(c.state, ConflictState::Detected);
        // Same pair, reversed order: already open -> None.
        assert!(s.open("agent-1", "agent-2", 90, vec![], vec![], vec![]).is_none());
        assert_eq!(s.open_count(), 1);
        assert!(s.has_open("agent-1", "agent-2"));
    }

    #[test]
    fn resolve_removes_and_marks_resolved() {
        let mut s = ConflictStore::new();
        s.open("agent-1", "agent-2", 80, vec![], vec![], vec![]);
        let r = s.resolve("agent-2", "agent-1").unwrap();
        assert_eq!(r.state, ConflictState::Resolved);
        assert!(!s.has_open("agent-1", "agent-2"));
        assert!(s.resolve("agent-1", "agent-2").is_none());
    }

    #[test]
    fn abort_for_agent_removes_all_its_conflicts() {
        let mut s = ConflictStore::new();
        s.open("agent-1", "agent-2", 80, vec![], vec![], vec![]);
        s.open("agent-1", "agent-3", 80, vec![], vec![], vec![]);
        s.open("agent-2", "agent-3", 80, vec![], vec![], vec![]);
        let aborted = s.abort_for_agent("agent-1");
        assert_eq!(aborted.len(), 2);
        assert!(aborted.iter().all(|c| c.state == ConflictState::Aborted));
        assert_eq!(s.open_count(), 1);
        assert!(s.has_open("agent-2", "agent-3"));
    }

    #[test]
    fn ids_are_sequential_across_opens() {
        let mut s = ConflictStore::new();
        let a = s.open("agent-1", "agent-2", 80, vec![], vec![], vec![]).unwrap();
        let b = s.open("agent-3", "agent-4", 80, vec![], vec![], vec![]).unwrap();
        assert_eq!(a.conflict_id, "conflict-1");
        assert_eq!(b.conflict_id, "conflict-2");
    }
}
```

- [ ] **Step 3: Run tests** — `cd packages/coordify-core && cargo test conflict::` → 5 pass.
- [ ] **Step 4: Commit** — `git add packages/coordify-core/src/lib.rs packages/coordify-core/src/conflict.rs && git commit -m "feat(core): conflict store (open/resolve/abort, ordered-pair keyed)"`

---

## Task 2: Open/resolve conflicts in current-heat recompute (`server.rs`)

**Files:** Modify `src/server.rs`; modify `tests/integration.rs`.

**Interfaces:**
- `Shared` gains `pub conflicts: Mutex<ConflictStore>` (init in `run` and `shared_for_test`).
- In `recompute_current_heat`, after the heat edges are upserted, decide conflict actions per edge: open on `CONFLICT_CANDIDATE`, resolve on drop-below; append `CONFLICT_OPENED`/`CONFLICT_RESOLVED` under the existing log lock.

> **Behavior (per edge `(agent_id, other_id, result)`):**
> - `result.band == HeatBand::ConflictCandidate` AND `!conflicts.has_open(agent_id, other_id)` → `conflicts.open(...)` with `trigger_heat = result.heat`, `paths` = the two inputs' shared files (intersection; empty allowed), `domains` = union of the two inputs' domains, `intents` = `[mine.intent, other.intent]` → append `CONFLICT_OPENED`.
> - `result.band != HeatBand::ConflictCandidate` AND `conflicts.has_open(agent_id, other_id)` → `conflicts.resolve(...)` → append `CONFLICT_RESOLVED { resolution: "AUTO_RESOLVED_HEAT_DROPPED" }`.
> - else: nothing.
>
> The conflict metadata needs the two `HeatInputs`. `recompute_current_heat` already holds `mine` and the `others` vec; pair each `result` with its `other` input.

Lock order: state (snapshot, released) → heat lock (upsert, released) → conflict lock (decide open/resolve, collect events, released) → log lock (append heat + conflict events). Never nested.

- [ ] **Step 1: Imports + `Shared` field**

Add to server.rs imports:
```rust
use crate::conflict::ConflictStore;
```
Add to `Shared`:
```rust
    pub conflicts: Mutex<ConflictStore>,
```
Initialize in `run`'s `Shared { ... }` and in `shared_for_test`:
```rust
        conflicts: Mutex::new(ConflictStore::new()),
```

- [ ] **Step 2: Extend `recompute_current_heat` to drive conflicts**

The current function computes `updates: Vec<(String /*other_id*/, HeatResult)>`, upserts them into the heat store, then logs HEAT_UPDATED/THRESHOLD. Modify it so the `others` inputs are available alongside results, and add a conflict-decision pass.

Change the compute loop to keep the `other` input with each update:
```rust
    // Compute (pure). Keep each other's inputs for conflict metadata.
    let mut updates: Vec<(heat::HeatInputs, heat::HeatResult)> = Vec::new();
    for other in others {
        let result = heat::compute_heat(&mine, &other, &shared.knowledge, &shared.heat_cfg);
        updates.push((other, result));
    }
    {
        let mut store = shared.heat.lock().unwrap();
        for (other, result) in &updates {
            store.upsert(agent_id, &other.agent_id, result.clone());
        }
    }
    // Decide conflict open/resolve per edge (collect events to log after).
    let mut conflict_events: Vec<serde_json::Value> = Vec::new();
    {
        let mut cstore = shared.conflicts.lock().unwrap();
        for (other, result) in &updates {
            let other_id = &other.agent_id;
            if result.band == HeatBand::ConflictCandidate {
                if !cstore.has_open(agent_id, other_id) {
                    let paths: Vec<String> =
                        mine.files.intersection(&other.files).cloned().collect();
                    let domains: Vec<String> =
                        mine.domains.union(&other.domains).cloned().collect();
                    let intents = vec![mine.intent.clone(), other.intent.clone()];
                    if let Some(c) = cstore.open(agent_id, other_id, result.heat, paths, domains, intents) {
                        conflict_events.push(serde_json::json!({
                            "type": "CONFLICT_OPENED",
                            "conflictId": c.conflict_id,
                            "agents": [c.agents.0, c.agents.1],
                            "openedAt": crate::bootstrap::now_iso(),
                            "trigger": {"type": "HEAT_THRESHOLD", "heat": c.trigger_heat},
                            "paths": c.paths,
                            "domains": c.domains,
                            "intents": c.intents,
                            "requiredAction": "NEGOTIATE_OR_REASSIGN",
                            "ts": crate::bootstrap::now_iso(),
                        }));
                    }
                }
            } else if cstore.has_open(agent_id, other_id) {
                if let Some(c) = cstore.resolve(agent_id, other_id) {
                    conflict_events.push(serde_json::json!({
                        "type": "CONFLICT_RESOLVED",
                        "conflictId": c.conflict_id,
                        "resolution": "AUTO_RESOLVED_HEAT_DROPPED",
                        "ts": crate::bootstrap::now_iso(),
                    }));
                }
            }
        }
    }
    {
        let mut log = shared.log.lock().unwrap();
        for (other, result) in &updates {
            let components = serde_json::to_value(&result.components).unwrap_or(serde_json::Value::Null);
            let _ = log.append(&serde_json::json!({
                "type": "HEAT_UPDATED",
                "pair": [agent_id, other.agent_id],
                "heat": result.heat,
                "heatKind": "CURRENT",
                "band": result.band.as_str(),
                "components": components,
                "reasons": result.reasons,
                "ts": crate::bootstrap::now_iso(),
            }));
            if let Some((level, action)) = escalation(result.band) {
                let _ = log.append(&serde_json::json!({
                    "type": "HEAT_THRESHOLD_EXCEEDED",
                    "pair": [agent_id, other.agent_id],
                    "heat": result.heat,
                    "escalationLevel": level,
                    "requiredAction": action,
                    "ts": crate::bootstrap::now_iso(),
                }));
            }
        }
        for ev in &conflict_events {
            let _ = log.append(ev);
        }
    }
```

> NOTE: this changes `updates` from `(String, HeatResult)` to `(HeatInputs, HeatResult)`. `HeatInputs` is `Clone`; `others` is consumed by value in the loop (`for other in others`). Adjust the earlier `others` binding if it was borrowed — it is built fresh in the snapshot, so move it. The HEAT_UPDATED/THRESHOLD logging now reads `other.agent_id` instead of a bare `other_id`; behavior identical.

- [ ] **Step 3: Unit test** (append to `server::tests`)

```rust
    #[test]
    fn high_overlap_opens_exactly_one_conflict() {
        let s = shared_for_test("good");
        let a = handle_request(&s, &{ let mut r = req("good", "register"); r.meta = json!({"branch":"main"}); r }).agent_id.unwrap();
        let b = handle_request(&s, &{ let mut r = req("good", "register"); r.meta = json!({"branch":"main"}); r }).agent_id.unwrap();
        let mk = |agent: &str| json!({"type":"CLAIM_PROPOSED","agentId":agent,"intent":"BUGFIX","domains":["AUTH"],"estimatedFiles":["src/auth/session.ts"],"task":{"summary":"fix session expiry"},"confidence":0.9});
        assert!(handle_request(&s, &cap_req("good", mk(&a))).ok);
        assert!(handle_request(&s, &cap_req("good", mk(&b))).ok); // heat 80 -> ConflictCandidate
        assert_eq!(s.conflicts.lock().unwrap().open_count(), 1);
        assert!(s.conflicts.lock().unwrap().has_open(&a, &b));
        // Re-proposing/recompute must not open a duplicate.
        recompute_current_heat(&s, &a);
        assert_eq!(s.conflicts.lock().unwrap().open_count(), 1);
    }

    #[test]
    fn low_overlap_opens_no_conflict() {
        let s = shared_for_test("good");
        let a = handle_request(&s, &req("good", "register")).agent_id.unwrap();
        let b = handle_request(&s, &req("good", "register")).agent_id.unwrap();
        let ca = json!({"type":"CLAIM_PROPOSED","agentId":a,"intent":"BUGFIX","domains":["AUTH"],"estimatedFiles":["src/a.rs"],"task":{"summary":"alpha"},"confidence":0.9});
        let cb = json!({"type":"CLAIM_PROPOSED","agentId":b,"intent":"DOCUMENTATION","domains":["DOCS"],"estimatedFiles":["docs/b.md"],"task":{"summary":"beta"},"confidence":0.9});
        assert!(handle_request(&s, &cap_req("good", ca)).ok);
        assert!(handle_request(&s, &cap_req("good", cb)).ok);
        assert_eq!(s.conflicts.lock().unwrap().open_count(), 0);
    }
```

- [ ] **Step 4: Run unit tests** — `cargo test server::tests` → prior + two new pass.

- [ ] **Step 5: Integration test** (append to `tests/integration.rs`)

```rust
#[test]
fn high_overlap_claims_open_conflict() {
    let core = spawn_core("conflict");
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
    assert_eq!(send_line(&mut stream, &mk("4", &b))["ok"], true);

    drop(stream); // connection closed.

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
        if log_contents.contains("CONFLICT_OPENED") {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(log_contents.contains("CONFLICT_OPENED"), "no CONFLICT_OPENED logged for high-overlap pair");
}
```

- [ ] **Step 6: Full suite + multi-thread + clippy** — `cargo test && cargo test -- --test-threads=4 && cargo clippy --all-targets -- -D warnings`. Use `connect_retry`; if flaky, raise budgets.
- [ ] **Step 7: Commit** — `git add packages/coordify-core/src/server.rs packages/coordify-core/tests/integration.rs && git commit -m "feat(core): open/auto-resolve conflicts from current heat band"`

---

## Task 3: Abort conflicts when a participant's claim goes away (`server.rs`)

**Files:** Modify `src/server.rs`; modify `tests/integration.rs`.

**Interfaces:** extends the no-live-claim branch of `recompute_current_heat` — when an agent has no live claim (its heat edges are dropped), also abort any open conflict involving it and append `CONFLICT_ABORTED`.

> **Behavior:** In the `None` arm (after `heat.remove_agent(agent_id)`), take the conflict lock, `abort_for_agent(agent_id)`, then append one `CONFLICT_ABORTED { conflictId, reason: "PARTICIPANT_LEFT", ts }` per aborted conflict under the log lock. Lock order: heat (remove, released) → conflict (abort, released) → log (append). Never nested.

- [ ] **Step 1: Extend the no-live-claim branch**

Replace the current `None` arm of `recompute_current_heat`:
```rust
    let mine = match mine {
        Some(m) => m,
        None => {
            shared.heat.lock().unwrap().remove_agent(agent_id);
            return;
        }
    };
```
with:
```rust
    let mine = match mine {
        Some(m) => m,
        None => {
            shared.heat.lock().unwrap().remove_agent(agent_id);
            let aborted = shared.conflicts.lock().unwrap().abort_for_agent(agent_id);
            if !aborted.is_empty() {
                let mut log = shared.log.lock().unwrap();
                for c in &aborted {
                    let _ = log.append(&serde_json::json!({
                        "type": "CONFLICT_ABORTED",
                        "conflictId": c.conflict_id,
                        "reason": "PARTICIPANT_LEFT",
                        "ts": crate::bootstrap::now_iso(),
                    }));
                }
            }
            return;
        }
    };
```

- [ ] **Step 2: Unit test** (append to `server::tests`)

```rust
    #[test]
    fn releasing_a_participant_aborts_the_conflict() {
        let s = shared_for_test("good");
        let a = handle_request(&s, &{ let mut r = req("good", "register"); r.meta = json!({"branch":"main"}); r }).agent_id.unwrap();
        let b = handle_request(&s, &{ let mut r = req("good", "register"); r.meta = json!({"branch":"main"}); r }).agent_id.unwrap();
        let mk = |agent: &str| json!({"type":"CLAIM_PROPOSED","agentId":agent,"intent":"BUGFIX","domains":["AUTH"],"estimatedFiles":["src/auth/session.ts"],"task":{"summary":"fix session expiry"},"confidence":0.9});
        assert!(handle_request(&s, &cap_req("good", mk(&a))).ok);
        let rb = handle_request(&s, &cap_req("good", mk(&b)));
        let b_claim = rb.data.unwrap()["claimId"].as_str().unwrap().to_string();
        assert_eq!(s.conflicts.lock().unwrap().open_count(), 1);
        // Release b's claim -> b has no live claim -> conflict aborted.
        let release = json!({"type":"CLAIM_RELEASED","claimId":b_claim,"agentId":b,"reason":"TASK_COMPLETED"});
        assert!(handle_request(&s, &cap_req("good", release)).ok);
        assert_eq!(s.conflicts.lock().unwrap().open_count(), 0);
    }
```

- [ ] **Step 3: Run unit tests** — `cargo test server::tests` → passes incl new test.

- [ ] **Step 4: Integration test** (append to `tests/integration.rs`)

```rust
#[test]
fn releasing_participant_aborts_conflict_over_socket() {
    let core = spawn_core("cabort");
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
    let rb = send_line(&mut stream, &mk("4", &b));
    let b_claim = rb["data"]["claimId"].as_str().unwrap().to_string();

    // Release b's claim -> conflict aborted.
    let release = format!(
        r#"{{"id":"5","token":"{}","action":"submit_event","capVersion":"0.1","event":{{"type":"CLAIM_RELEASED","claimId":"{}","agentId":"{}","reason":"TASK_COMPLETED"}}}}"#,
        token, b_claim, b
    );
    assert_eq!(send_line(&mut stream, &release)["ok"], true);

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
        if log_contents.contains("CONFLICT_ABORTED") {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(log_contents.contains("CONFLICT_OPENED"), "expected CONFLICT_OPENED");
    assert!(log_contents.contains("CONFLICT_ABORTED"), "expected CONFLICT_ABORTED after release");
}
```

- [ ] **Step 5: Full suite (5x) + multi-thread + clippy + coverage**

```bash
cd packages/coordify-core
for i in 1 2 3 4 5; do cargo test --test integration >/dev/null 2>&1 && echo "run $i ok" || echo "run $i FAIL"; done
cargo test -- --test-threads=4
cargo clippy --all-targets -- -D warnings
cargo llvm-cov --summary-only -- --test-threads=4 | tail -1
```
Expected: 5/5 ok; multi-thread passes; clippy clean; TOTAL line coverage ≥ 90% (target ≥ 95%).

- [ ] **Step 6: Commit** — `git add packages/coordify-core/src/server.rs packages/coordify-core/tests/integration.rs && git commit -m "feat(core): abort open conflicts when a participant's claim is released/cleared"`

---

## Out of Scope (Phase 4b / later)

- Negotiation: `CONFLICT_PROPOSAL_SUBMITTED`, proposal kinds, Core auto-resolve-vs-escalate (§18.4), the `NEGOTIATING`/`AWAITING_AGENT_RESPONSE` states.
- User arbitration: `AWAITING_USER_DECISION`, arbitration timeouts.
- Deadlock detection / wait-graph (§20).
- Handoff (§21).
- `CONFLICT_TIMEOUT` and proposal/handoff timeouts (§18.6).
- Non-heat conflict triggers (protected path, explicit coordination request) — §17 lists more; Phase 4a triggers on the heat band only.
- Conflict persistence beyond the event log (open conflicts are in-memory; the log records open/resolve/abort).

## Self-Review Notes

- **Spec coverage:** conflict object + store (Task 1) ✓; open on threshold band (Task 2) ✓; auto-resolve on heat drop (Task 2) ✓; abort on participant exit (Task 3) ✓.
- **CAP_SPEC coverage:** §17 conflict schema/states (ConflictState full set, CONFLICT_OPENED shape) ✓; §17 trigger HEAT_THRESHOLD ✓; resolution AUTO_RESOLVED_HEAT_DROPPED (§18.4 heat-dropped) ✓.
- **Type consistency:** `Conflict`, `ConflictState`, `ConflictStore` (open/resolve/abort_for_agent/has_open/open_count), the `Shared.conflicts` field, and the `updates: Vec<(HeatInputs, HeatResult)>` shape are referenced identically across Tasks 1–3.
- **Lock discipline:** open/resolve decided under a short conflict lock; abort under a short conflict lock in the None branch; all events appended last under the log lock. Never two of {state, heat, conflict, log} held at once. Matches Phase 1–3 invariant.
- **Determinism:** conflicts open/resolve purely from the deterministic heat band; one conflict per pair via `has_open` guard.
