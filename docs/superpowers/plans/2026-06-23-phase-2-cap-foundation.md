# Phase 2 — CAP Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the Phase 1 daemon into a CAP state machine: typed CAP event validation, agent states, claim lifecycle, `/clear`, and orphaned-claim TTL.

**Architecture:** Two new pure modules — `cap.rs` (typed event enum + canonical enums, deserialization-is-validation) and `claim.rs` (claim store + confidence rules) — plus extensions to `state.rs` (agent state/generation, transition rules, embedded claim store) and `server.rs` (typed `submit_event` dispatch + reaper orphan lifecycle). `server.rs` stays the only IO+state wiring point, preserving the Phase 1 state-before-log lock ordering.

**Tech Stack:** Rust edition 2021, serde + serde_json + chrono (no new deps). Builds on the merged Phase 1 crate at `packages/coordify-core/`.

**Spec:** `docs/superpowers/specs/2026-06-23-phase-2-cap-foundation-design.md`; authoritative event shapes in `absolute-docs/CAP_SPEC.md` (§7 states, §9/§13 claims, §12 `/clear`, §14 orphans, §28 errors).

## Global Constraints

- No new dependencies. serde (derive), serde_json, chrono only.
- Validation = typed serde. A `CapEvent` parse failure → `CAP_ERROR` with code `SCHEMA_VALIDATION_FAILED`. No `jsonschema` crate, no `.schema.json` files.
- Canonical enums serialize to the EXACT CAP_SPEC strings (SCREAMING_SNAKE_CASE): agent states (§7), intents (§8), claim statuses (§9), release reasons (§13.3), error codes (§28).
- Confidence thresholds (CAP_SPEC §9): `>= 0.75` → `ACTIVE`; `0.45 <= c < 0.75` → `PROVISIONAL`; `< 0.45` → rejected. Constants `ACTIVE_MIN = 0.75`, `PROVISIONAL_MIN = 0.45`.
- Claim ids are Core-assigned sequential: `claim-1`, `claim-2`, … . Agent ids stay `agent-N` (Phase 1).
- `submit_event` requires `cap_version == "0.1"` else `CAP_ERROR { UNSUPPORTED_CAP_VERSION }`.
- Orphan TTL default `300_000` ms (300 s), overridable via env `COORDIFY_ORPHAN_TTL_MS`.
- Core is the only state writer; every accepted event appends its canonical record(s) to `events.log` BEFORE responding. A rejected event mutates nothing and logs nothing.
- Lock discipline (carried from Phase 1): never hold the `state` mutex while locking `log`. Lock `state` in a short scope, release, then lock `log`.
- Phase 2 models ONLY: `CLAIM_PROPOSED`, `CLAIM_RELEASED`, `AGENT_STATE_CHANGED`, `CLEAR_INVOKED`. Any other CAP event type → `CAP_ERROR { SCHEMA_VALIDATION_FAILED }` (strict per CAP_SPEC §31).
- macOS/Linux only; std-only concurrency.

---

## File Structure

```text
packages/coordify-core/src/
  cap.rs       NEW  AgentState, Intent, ClaimStatus, ReleaseReason, CapErrorCode enums;
                    CapEvent enum (tag="type"); decode_event().
  claim.rs     NEW  Claim, status_for_confidence(), ClaimStore.
  state.rs     MOD  Agent { + state, + generation }; can_transition(); State { + claims };
                    set_state(), clear(), promote_active(), agent_state().
  ipc.rs       MOD  Request { + cap_version: Option<String> }; Response { + data: Option<Value> }.
  server.rs    MOD  handle_cap_event() dispatch; reaper orphan + sweep-reclaimable.
  lib.rs       MOD  pub mod cap; pub mod claim;
```

---

## Task 1: CAP canonical enums + event type

**Files:**
- Create: `packages/coordify-core/src/cap.rs`
- Modify: `packages/coordify-core/src/lib.rs` (add `pub mod cap;`)

**Interfaces:**
- Produces:
  - `cap::AgentState` (Discovery, Idle, Active, SubagentWaiting, Testing, Blocked, Negotiating, WaitingUser, Offline) — Copy, serde SCREAMING_SNAKE_CASE.
  - `cap::Intent` (14 variants per CAP_SPEC §8) — Copy, serde SCREAMING_SNAKE_CASE; `Intent::as_str(&self) -> &'static str`.
  - `cap::ClaimStatus` (Proposed, Provisional, Active, Released, Orphaned, Reclaimable, Rejected) — Copy, serde SCREAMING_SNAKE_CASE; `ClaimStatus::as_str`.
  - `cap::ReleaseReason` (TaskCompleted, TaskAborted, UserChangedTask, ClearInvoked, HandoffTransfer, ManualRelease, SessionEnd) — serde SCREAMING_SNAKE_CASE.
  - `cap::CapErrorCode` (SchemaValidationFailed, InvalidStateTransition, AuthFailed, ClaimConflict, AgentNotFound, ClaimNotFound, CoreDegraded, Timeout, UnsupportedCapVersion); `CapErrorCode::as_str(&self) -> &'static str`.
  - `cap::CapEvent` enum (`#[serde(tag="type", rename_all="SCREAMING_SNAKE_CASE")]`) with variants `ClaimProposed`, `ClaimReleased`, `AgentStateChanged`, `ClearInvoked` (fields camelCase).
  - `cap::decode_event(event: &serde_json::Value) -> Result<CapEvent, CapErrorCode>`.

- [ ] **Step 1: Add module to `src/lib.rs`** (insert `pub mod cap;` after `pub mod ipc;`)

- [ ] **Step 2: Write `src/cap.rs` with enums, CapEvent, decode, and tests**

```rust
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AgentState {
    Discovery,
    Idle,
    Active,
    SubagentWaiting,
    Testing,
    Blocked,
    Negotiating,
    WaitingUser,
    Offline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Intent {
    Security,
    Qa,
    Testing,
    Performance,
    Refactor,
    Documentation,
    Feature,
    Bugfix,
    Architecture,
    Devops,
    Research,
    Migration,
    Configuration,
    Observability,
}

impl Intent {
    pub fn as_str(&self) -> &'static str {
        match self {
            Intent::Security => "SECURITY",
            Intent::Qa => "QA",
            Intent::Testing => "TESTING",
            Intent::Performance => "PERFORMANCE",
            Intent::Refactor => "REFACTOR",
            Intent::Documentation => "DOCUMENTATION",
            Intent::Feature => "FEATURE",
            Intent::Bugfix => "BUGFIX",
            Intent::Architecture => "ARCHITECTURE",
            Intent::Devops => "DEVOPS",
            Intent::Research => "RESEARCH",
            Intent::Migration => "MIGRATION",
            Intent::Configuration => "CONFIGURATION",
            Intent::Observability => "OBSERVABILITY",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ClaimStatus {
    Proposed,
    Provisional,
    Active,
    Released,
    Orphaned,
    Reclaimable,
    Rejected,
}

impl ClaimStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ClaimStatus::Proposed => "PROPOSED",
            ClaimStatus::Provisional => "PROVISIONAL",
            ClaimStatus::Active => "ACTIVE",
            ClaimStatus::Released => "RELEASED",
            ClaimStatus::Orphaned => "ORPHANED",
            ClaimStatus::Reclaimable => "RECLAIMABLE",
            ClaimStatus::Rejected => "REJECTED",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReleaseReason {
    TaskCompleted,
    TaskAborted,
    UserChangedTask,
    ClearInvoked,
    HandoffTransfer,
    ManualRelease,
    SessionEnd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapErrorCode {
    SchemaValidationFailed,
    InvalidStateTransition,
    AuthFailed,
    ClaimConflict,
    AgentNotFound,
    ClaimNotFound,
    CoreDegraded,
    Timeout,
    UnsupportedCapVersion,
}

impl CapErrorCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            CapErrorCode::SchemaValidationFailed => "SCHEMA_VALIDATION_FAILED",
            CapErrorCode::InvalidStateTransition => "INVALID_STATE_TRANSITION",
            CapErrorCode::AuthFailed => "AUTH_FAILED",
            CapErrorCode::ClaimConflict => "CLAIM_CONFLICT",
            CapErrorCode::AgentNotFound => "AGENT_NOT_FOUND",
            CapErrorCode::ClaimNotFound => "CLAIM_NOT_FOUND",
            CapErrorCode::CoreDegraded => "CORE_DEGRADED",
            CapErrorCode::Timeout => "TIMEOUT",
            CapErrorCode::UnsupportedCapVersion => "UNSUPPORTED_CAP_VERSION",
        }
    }
}

/// CAP events Phase 2 ingests. `type` selects the variant; fields are camelCase.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CapEvent {
    #[serde(rename_all = "camelCase")]
    ClaimProposed {
        agent_id: String,
        #[serde(default)]
        task: Value,
        intent: Intent,
        #[serde(default)]
        domains: Vec<String>,
        #[serde(default)]
        estimated_files: Vec<String>,
        confidence: f64,
    },
    #[serde(rename_all = "camelCase")]
    ClaimReleased {
        claim_id: String,
        agent_id: String,
        reason: ReleaseReason,
    },
    #[serde(rename_all = "camelCase")]
    AgentStateChanged {
        agent_id: String,
        state: AgentState,
    },
    #[serde(rename_all = "camelCase")]
    ClearInvoked {
        agent_id: String,
    },
}

/// Deserialize-is-validation: any shape/value error → SCHEMA_VALIDATION_FAILED.
pub fn decode_event(event: &Value) -> Result<CapEvent, CapErrorCode> {
    serde_json::from_value(event.clone()).map_err(|_| CapErrorCode::SchemaValidationFailed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn canonical_enum_strings_match_cap_spec() {
        assert_eq!(serde_json::to_value(AgentState::SubagentWaiting).unwrap(), json!("SUBAGENT_WAITING"));
        assert_eq!(serde_json::to_value(AgentState::WaitingUser).unwrap(), json!("WAITING_USER"));
        assert_eq!(serde_json::to_value(AgentState::Discovery).unwrap(), json!("DISCOVERY"));
        assert_eq!(serde_json::to_value(Intent::Qa).unwrap(), json!("QA"));
        assert_eq!(serde_json::to_value(Intent::Devops).unwrap(), json!("DEVOPS"));
        assert_eq!(serde_json::to_value(Intent::Bugfix).unwrap(), json!("BUGFIX"));
        assert_eq!(Intent::Qa.as_str(), "QA");
        assert_eq!(ClaimStatus::Reclaimable.as_str(), "RECLAIMABLE");
        assert_eq!(serde_json::to_value(ReleaseReason::ClearInvoked).unwrap(), json!("CLEAR_INVOKED"));
        assert_eq!(CapErrorCode::SchemaValidationFailed.as_str(), "SCHEMA_VALIDATION_FAILED");
        assert_eq!(CapErrorCode::UnsupportedCapVersion.as_str(), "UNSUPPORTED_CAP_VERSION");
    }

    #[test]
    fn decodes_claim_proposed_with_camel_case_fields() {
        let ev = json!({
            "type": "CLAIM_PROPOSED",
            "agentId": "agent-1",
            "intent": "BUGFIX",
            "domains": ["AUTHENTICATION"],
            "estimatedFiles": ["src/auth/session.ts"],
            "confidence": 0.86
        });
        match decode_event(&ev).unwrap() {
            CapEvent::ClaimProposed { agent_id, intent, domains, estimated_files, confidence, .. } => {
                assert_eq!(agent_id, "agent-1");
                assert_eq!(intent, Intent::Bugfix);
                assert_eq!(domains, vec!["AUTHENTICATION"]);
                assert_eq!(estimated_files, vec!["src/auth/session.ts"]);
                assert!((confidence - 0.86).abs() < 1e-9);
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn decodes_other_phase2_variants() {
        let rel = json!({"type":"CLAIM_RELEASED","claimId":"claim-1","agentId":"agent-1","reason":"TASK_COMPLETED"});
        assert!(matches!(decode_event(&rel).unwrap(), CapEvent::ClaimReleased { .. }));
        let st = json!({"type":"AGENT_STATE_CHANGED","agentId":"agent-1","state":"TESTING"});
        assert!(matches!(decode_event(&st).unwrap(), CapEvent::AgentStateChanged { state: AgentState::Testing, .. }));
        let clr = json!({"type":"CLEAR_INVOKED","agentId":"agent-1"});
        assert!(matches!(decode_event(&clr).unwrap(), CapEvent::ClearInvoked { .. }));
    }

    #[test]
    fn rejects_bad_intent_and_unknown_type_and_missing_field() {
        let bad_intent = json!({"type":"CLAIM_PROPOSED","agentId":"a","intent":"NOPE","confidence":0.9});
        assert_eq!(decode_event(&bad_intent).unwrap_err(), CapErrorCode::SchemaValidationFailed);
        let unknown = json!({"type":"TOOL_PRECHECK","agentId":"a"});
        assert_eq!(decode_event(&unknown).unwrap_err(), CapErrorCode::SchemaValidationFailed);
        let missing_conf = json!({"type":"CLAIM_PROPOSED","agentId":"a","intent":"BUGFIX"});
        assert_eq!(decode_event(&missing_conf).unwrap_err(), CapErrorCode::SchemaValidationFailed);
    }
}
```

- [ ] **Step 3: Run the tests — expect PASS**

Run: `cd packages/coordify-core && cargo test cap::`
Expected: 4 `cap::tests::*` pass. If `canonical_enum_strings_match_cap_spec` fails on any value, add a per-variant `#[serde(rename = "...")]` to fix that exact string, then re-run.

- [ ] **Step 4: Commit**

```bash
git add packages/coordify-core/src/lib.rs packages/coordify-core/src/cap.rs
git commit -m "feat(core): CAP canonical enums + typed CapEvent (parse=validate)"
```

---

## Task 2: Claim model + store

**Files:**
- Create: `packages/coordify-core/src/claim.rs`
- Modify: `packages/coordify-core/src/lib.rs` (add `pub mod claim;`)

**Interfaces:**
- Consumes: `crate::cap::ClaimStatus`.
- Produces:
  - `claim::Claim { claim_id: String, agent_id: String, status: ClaimStatus, intent: String, domains: Vec<String>, estimated_files: Vec<String>, confidence: f64, orphaned_at_ms: Option<u64> }` (Clone).
  - `claim::ACTIVE_MIN: f64 = 0.75`, `claim::PROVISIONAL_MIN: f64 = 0.45`.
  - `claim::status_for_confidence(c: f64) -> Option<ClaimStatus>` — `Active`/`Provisional`/`None` (rejected).
  - `claim::ClaimStore` with `new()`, `propose(agent_id, intent, domains, estimated_files, confidence) -> Option<Claim>` (None = rejected), `release(claim_id) -> bool`, `release_for_agent(agent_id) -> Vec<String>`, `orphan_for_agent(agent_id, now_ms) -> Vec<String>`, `sweep_reclaimable(now_ms, ttl_ms) -> Vec<String>`, `get(claim_id) -> Option<&Claim>`, `len() -> usize`, `is_empty() -> bool`.

- [ ] **Step 1: Add module to `src/lib.rs`** (add `pub mod claim;` after `pub mod cap;`)

- [ ] **Step 2: Write `src/claim.rs` with the store and tests**

```rust
use crate::cap::ClaimStatus;
use std::collections::HashMap;

pub const ACTIVE_MIN: f64 = 0.75;
pub const PROVISIONAL_MIN: f64 = 0.45;

#[derive(Debug, Clone)]
pub struct Claim {
    pub claim_id: String,
    pub agent_id: String,
    pub status: ClaimStatus,
    pub intent: String,
    pub domains: Vec<String>,
    pub estimated_files: Vec<String>,
    pub confidence: f64,
    pub orphaned_at_ms: Option<u64>,
}

/// Map confidence to the initial claim status, or None if it must be rejected.
pub fn status_for_confidence(c: f64) -> Option<ClaimStatus> {
    if c >= ACTIVE_MIN {
        Some(ClaimStatus::Active)
    } else if c >= PROVISIONAL_MIN {
        Some(ClaimStatus::Provisional)
    } else {
        None
    }
}

#[derive(Default)]
pub struct ClaimStore {
    claims: HashMap<String, Claim>,
    next_id: u64,
}

impl ClaimStore {
    pub fn new() -> Self {
        Self { claims: HashMap::new(), next_id: 1 }
    }

    /// Create a claim from a proposal. Returns None if confidence is too low
    /// (the caller emits CLAIM_REJECTED).
    pub fn propose(
        &mut self,
        agent_id: &str,
        intent: String,
        domains: Vec<String>,
        estimated_files: Vec<String>,
        confidence: f64,
    ) -> Option<Claim> {
        let status = status_for_confidence(confidence)?;
        let claim_id = format!("claim-{}", self.next_id);
        self.next_id += 1;
        let claim = Claim {
            claim_id: claim_id.clone(),
            agent_id: agent_id.to_string(),
            status,
            intent,
            domains,
            estimated_files,
            confidence,
            orphaned_at_ms: None,
        };
        self.claims.insert(claim_id, claim.clone());
        Some(claim)
    }

    /// Mark a single claim RELEASED. Returns false if the claim does not exist.
    pub fn release(&mut self, claim_id: &str) -> bool {
        match self.claims.get_mut(claim_id) {
            Some(c) => {
                c.status = ClaimStatus::Released;
                true
            }
            None => false,
        }
    }

    /// Release every live (Proposed/Provisional/Active) claim owned by an agent;
    /// returns the released claim ids. Used by /clear.
    pub fn release_for_agent(&mut self, agent_id: &str) -> Vec<String> {
        let ids: Vec<String> = self
            .claims
            .values()
            .filter(|c| {
                c.agent_id == agent_id
                    && matches!(
                        c.status,
                        ClaimStatus::Proposed | ClaimStatus::Provisional | ClaimStatus::Active
                    )
            })
            .map(|c| c.claim_id.clone())
            .collect();
        for id in &ids {
            self.claims.get_mut(id).unwrap().status = ClaimStatus::Released;
        }
        ids
    }

    /// Orphan every live (Provisional/Active) claim owned by an agent that was
    /// lost uncleanly; stamps orphaned_at_ms. Returns the orphaned claim ids.
    pub fn orphan_for_agent(&mut self, agent_id: &str, now_ms: u64) -> Vec<String> {
        let ids: Vec<String> = self
            .claims
            .values()
            .filter(|c| {
                c.agent_id == agent_id
                    && matches!(c.status, ClaimStatus::Provisional | ClaimStatus::Active)
            })
            .map(|c| c.claim_id.clone())
            .collect();
        for id in &ids {
            let c = self.claims.get_mut(id).unwrap();
            c.status = ClaimStatus::Orphaned;
            c.orphaned_at_ms = Some(now_ms);
        }
        ids
    }

    /// Promote ORPHANED claims past their TTL to RECLAIMABLE. Returns the ids.
    pub fn sweep_reclaimable(&mut self, now_ms: u64, ttl_ms: u64) -> Vec<String> {
        let ids: Vec<String> = self
            .claims
            .values()
            .filter(|c| {
                c.status == ClaimStatus::Orphaned
                    && c.orphaned_at_ms
                        .is_some_and(|t| now_ms.saturating_sub(t) >= ttl_ms)
            })
            .map(|c| c.claim_id.clone())
            .collect();
        for id in &ids {
            self.claims.get_mut(id).unwrap().status = ClaimStatus::Reclaimable;
        }
        ids
    }

    pub fn get(&self, claim_id: &str) -> Option<&Claim> {
        self.claims.get(claim_id)
    }

    pub fn len(&self) -> usize {
        self.claims.len()
    }

    pub fn is_empty(&self) -> bool {
        self.claims.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confidence_maps_to_status() {
        assert_eq!(status_for_confidence(0.9), Some(ClaimStatus::Active));
        assert_eq!(status_for_confidence(0.75), Some(ClaimStatus::Active));
        assert_eq!(status_for_confidence(0.749), Some(ClaimStatus::Provisional));
        assert_eq!(status_for_confidence(0.45), Some(ClaimStatus::Provisional));
        assert_eq!(status_for_confidence(0.44), None);
    }

    #[test]
    fn propose_assigns_sequential_ids_and_rejects_low_confidence() {
        let mut s = ClaimStore::new();
        let c1 = s.propose("agent-1", "BUGFIX".into(), vec![], vec![], 0.9).unwrap();
        assert_eq!(c1.claim_id, "claim-1");
        assert_eq!(c1.status, ClaimStatus::Active);
        let c2 = s.propose("agent-1", "QA".into(), vec![], vec![], 0.5).unwrap();
        assert_eq!(c2.claim_id, "claim-2");
        assert_eq!(c2.status, ClaimStatus::Provisional);
        assert!(s.propose("agent-1", "QA".into(), vec![], vec![], 0.1).is_none());
        assert_eq!(s.len(), 2);
    }

    #[test]
    fn release_and_release_for_agent() {
        let mut s = ClaimStore::new();
        let c = s.propose("agent-1", "BUGFIX".into(), vec![], vec![], 0.9).unwrap();
        assert!(s.release(&c.claim_id));
        assert_eq!(s.get(&c.claim_id).unwrap().status, ClaimStatus::Released);
        assert!(!s.release("claim-999"));

        let a = s.propose("agent-2", "QA".into(), vec![], vec![], 0.9).unwrap();
        let _b = s.propose("agent-2", "FEATURE".into(), vec![], vec![], 0.5).unwrap();
        let released = s.release_for_agent("agent-2");
        assert_eq!(released.len(), 2);
        assert_eq!(s.get(&a.claim_id).unwrap().status, ClaimStatus::Released);
    }

    #[test]
    fn orphan_then_sweep_reclaimable_respects_ttl() {
        let mut s = ClaimStore::new();
        let c = s.propose("agent-1", "BUGFIX".into(), vec![], vec![], 0.9).unwrap();
        let orphaned = s.orphan_for_agent("agent-1", 1_000);
        assert_eq!(orphaned, vec![c.claim_id.clone()]);
        assert_eq!(s.get(&c.claim_id).unwrap().status, ClaimStatus::Orphaned);

        // Not yet past TTL (idle 500 < 1000): no sweep.
        assert!(s.sweep_reclaimable(1_500, 1_000).is_empty());
        assert_eq!(s.get(&c.claim_id).unwrap().status, ClaimStatus::Orphaned);
        // Past TTL (idle 1000 >= 1000): swept.
        let swept = s.sweep_reclaimable(2_000, 1_000);
        assert_eq!(swept, vec![c.claim_id.clone()]);
        assert_eq!(s.get(&c.claim_id).unwrap().status, ClaimStatus::Reclaimable);
    }
}
```

- [ ] **Step 3: Run the tests — expect PASS**

Run: `cd packages/coordify-core && cargo test claim::`
Expected: 4 `claim::tests::*` pass.

- [ ] **Step 4: Commit**

```bash
git add packages/coordify-core/src/lib.rs packages/coordify-core/src/claim.rs
git commit -m "feat(core): claim store with confidence rules + orphan TTL sweep"
```

---

## Task 3: Agent state/generation, transitions, IPC fields

**Files:**
- Modify: `packages/coordify-core/src/state.rs` (Agent fields, `can_transition`, `set_state`, `clear`, `promote_active`, `agent_state`, embed `ClaimStore`)
- Modify: `packages/coordify-core/src/ipc.rs` (Request `cap_version`, Response `data`)

**Interfaces:**
- Consumes: `crate::cap::AgentState`, `crate::claim::ClaimStore`.
- Produces:
  - `state::Agent` gains `pub state: AgentState` and `pub generation: u64`. `register` sets `state: AgentState::Discovery`, `generation: 1`.
  - `state::can_transition(from: AgentState, to: AgentState) -> bool`.
  - `state::StateError` enum: `AgentNotFound`, `InvalidTransition`.
  - `State` gains `pub claims: ClaimStore`.
  - `State::agent_state(&self, id: &str) -> Option<AgentState>`.
  - `State::set_state(&mut self, id: &str, to: AgentState) -> Result<(), StateError>`.
  - `State::clear(&mut self, id: &str) -> Option<u64>` (sets Discovery, increments generation, returns new generation).
  - `State::promote_active(&mut self, id: &str)` (Discovery → Active; no-op otherwise).
  - `ipc::Request` gains `#[serde(default)] pub cap_version: Option<String>`.
  - `ipc::Response` gains `#[serde(default, skip_serializing_if = "Option::is_none")] pub data: Option<serde_json::Value>`; existing constructors set `data: None`.

- [ ] **Step 1: Extend `Request` and `Response` in `src/ipc.rs`**

Add to `Request` (after `event`). The wire envelope uses camelCase `capVersion`, so rename explicitly (the rest of `Request` stays snake_case):
```rust
    #[serde(default, rename = "capVersion")]
    pub cap_version: Option<String>,
```
Add to `Response` (after `error`):
```rust
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
```
Update the three constructors to include `data: None`:
```rust
    pub fn ok_for(id: &str) -> Self {
        Self { id: id.to_string(), ok: true, agent_id: None, error: None, data: None }
    }
    pub fn ok_with_agent(id: &str, agent_id: &str) -> Self {
        Self { id: id.to_string(), ok: true, agent_id: Some(agent_id.to_string()), error: None, data: None }
    }
    pub fn err(id: &str, msg: &str) -> Self {
        Self { id: id.to_string(), ok: false, agent_id: None, error: Some(msg.to_string()), data: None }
    }
```
Add a constructor for data-carrying ok responses:
```rust
    pub fn ok_with_data(id: &str, data: Value) -> Self {
        Self { id: id.to_string(), ok: true, agent_id: None, error: None, data: Some(data) }
    }
```
(`Value` is already imported in ipc.rs as `serde_json::Value`.)

- [ ] **Step 2: Add an ipc test for the new fields**

Add to `ipc::tests`:
```rust
    #[test]
    fn request_defaults_cap_version_to_none_and_response_omits_data() {
        let line = r#"{"id":"r1","token":"t","action":"submit_event","event":{"type":"X"}}"#;
        let req = decode_request(line).unwrap();
        assert_eq!(req.cap_version, None);
        // ok_for omits data
        assert_eq!(encode_response(&Response::ok_for("r1")), r#"{"id":"r1","ok":true}"#);
        // ok_with_data includes it
        let r = Response::ok_with_data("r1", serde_json::json!({"claimId":"claim-1"}));
        assert!(encode_response(&r).contains(r#""data":{"claimId":"claim-1"}"#));
    }
```

- [ ] **Step 3: Run ipc tests — expect PASS**

Run: `cd packages/coordify-core && cargo test ipc::`
Expected: existing ipc tests + the new one pass.

- [ ] **Step 4: Extend `Agent`, `State`, and add transition logic in `src/state.rs`**

At the top, add imports:
```rust
use crate::cap::AgentState;
use crate::claim::ClaimStore;
```
Change `Agent`:
```rust
#[derive(Debug, Clone)]
pub struct Agent {
    pub id: String,
    pub last_seen_ms: u64,
    pub meta: serde_json::Value,
    pub state: AgentState,
    pub generation: u64,
}
```
Change `State` to hold claims:
```rust
pub struct State {
    agents: HashMap<String, Agent>,
    pub claims: ClaimStore,
    next_id: u64,
}
```
Update `State::new`:
```rust
    pub fn new() -> Self {
        Self { agents: HashMap::new(), claims: ClaimStore::new(), next_id: 1 }
    }
```
Update `register` to set state/generation:
```rust
    pub fn register(&mut self, meta: serde_json::Value, now_ms: u64) -> String {
        let id = format!("agent-{}", self.next_id);
        self.next_id += 1;
        self.agents.insert(
            id.clone(),
            Agent {
                id: id.clone(),
                last_seen_ms: now_ms,
                meta,
                state: AgentState::Discovery,
                generation: 1,
            },
        );
        id
    }
```
Add the error type, transition rule, and new methods (place after the existing impl methods, inside `impl State` for the methods):
```rust
#[derive(Debug, PartialEq, Eq)]
pub enum StateError {
    AgentNotFound,
    InvalidTransition,
}

/// Allowed AGENT_STATE_CHANGED transitions (CAP_SPEC §7). Offline is reachable
/// from any live state; a same-state report is a no-op (allowed). Discovery is
/// re-entered only via /clear (State::clear), not via set_state.
pub fn can_transition(from: AgentState, to: AgentState) -> bool {
    use AgentState::*;
    if from == to {
        return true;
    }
    if to == Offline {
        return true;
    }
    matches!(
        (from, to),
        (Discovery, Active)
            | (Discovery, Idle)
            | (Active, Idle)
            | (Active, SubagentWaiting)
            | (Active, Testing)
            | (Active, Negotiating)
            | (Active, Blocked)
            | (Idle, Active)
            | (SubagentWaiting, Active)
            | (SubagentWaiting, Idle)
            | (Testing, Active)
            | (Testing, Idle)
            | (Negotiating, WaitingUser)
            | (Negotiating, Active)
            | (Blocked, Active)
            | (Blocked, WaitingUser)
            | (WaitingUser, Active)
            | (WaitingUser, Idle)
    )
}
```
Add these methods inside `impl State`:
```rust
    pub fn agent_state(&self, id: &str) -> Option<AgentState> {
        self.agents.get(id).map(|a| a.state)
    }

    pub fn set_state(&mut self, id: &str, to: AgentState) -> Result<(), StateError> {
        let agent = self.agents.get_mut(id).ok_or(StateError::AgentNotFound)?;
        if !can_transition(agent.state, to) {
            return Err(StateError::InvalidTransition);
        }
        agent.state = to;
        Ok(())
    }

    /// /clear: reset to DISCOVERY and bump generation. Returns the new
    /// generation, or None if the agent is unknown.
    pub fn clear(&mut self, id: &str) -> Option<u64> {
        let agent = self.agents.get_mut(id)?;
        agent.state = AgentState::Discovery;
        agent.generation += 1;
        Some(agent.generation)
    }

    /// Promote a DISCOVERY agent to ACTIVE after an accepted claim (CAP_SPEC §7).
    pub fn promote_active(&mut self, id: &str) {
        if let Some(agent) = self.agents.get_mut(id) {
            if agent.state == AgentState::Discovery {
                agent.state = AgentState::Active;
            }
        }
    }
```

- [ ] **Step 5: Add state tests** (append to `state::tests`)

```rust
    #[test]
    fn register_starts_in_discovery_generation_one() {
        let mut s = State::new();
        let id = s.register(serde_json::json!({}), 1000);
        assert_eq!(s.agent_state(&id), Some(crate::cap::AgentState::Discovery));
    }

    #[test]
    fn set_state_enforces_transition_rules() {
        use crate::cap::AgentState::*;
        let mut s = State::new();
        let id = s.register(serde_json::json!({}), 1000);
        // Discovery -> Active is allowed; Discovery -> Testing is not.
        assert!(s.set_state(&id, Active).is_ok());
        assert!(s.set_state(&id, Testing).is_ok()); // Active -> Testing ok
        assert_eq!(s.set_state(&id, SubagentWaiting), Err(super::StateError::InvalidTransition)); // Testing -> SubagentWaiting not allowed
        assert_eq!(s.set_state("agent-999", Idle), Err(super::StateError::AgentNotFound));
        // Any -> Offline allowed
        assert!(s.set_state(&id, Offline).is_ok());
    }

    #[test]
    fn clear_resets_to_discovery_and_bumps_generation() {
        use crate::cap::AgentState::*;
        let mut s = State::new();
        let id = s.register(serde_json::json!({}), 1000);
        s.set_state(&id, Active).unwrap();
        let gen = s.clear(&id).unwrap();
        assert_eq!(gen, 2);
        assert_eq!(s.agent_state(&id), Some(Discovery));
        assert_eq!(s.clear("agent-999"), None);
    }

    #[test]
    fn promote_active_only_from_discovery() {
        use crate::cap::AgentState::*;
        let mut s = State::new();
        let id = s.register(serde_json::json!({}), 1000);
        s.promote_active(&id);
        assert_eq!(s.agent_state(&id), Some(Active));
        // From Active, promote is a no-op (stays Active).
        s.promote_active(&id);
        assert_eq!(s.agent_state(&id), Some(Active));
    }
```

- [ ] **Step 6: Run tests — expect PASS**

Run: `cd packages/coordify-core && cargo test state:: && cargo test ipc::`
Expected: all state + ipc tests pass. (Note: `server.rs` constructs `Shared` and may reference `State`; if `cargo test` for the whole crate fails to compile because `server.rs` builds `Response` without `data`, that is fixed in Task 4 — for THIS task run only the `state::` and `ipc::` filters, which compile the lib. If the lib fails to compile due to server.rs using the old `Response` literal, note it; server.rs uses the `Response::*` constructors which now set `data` internally, so it should still compile.)

- [ ] **Step 7: Verify the whole crate still compiles**

Run: `cd packages/coordify-core && cargo build`
Expected: compiles. If `server.rs` or `main.rs` break due to the `Agent` struct now requiring `state`/`generation`, that is only constructed inside `State::register` (not elsewhere), so no breakage is expected. Fix any compile error by routing through the new constructors.

- [ ] **Step 8: Commit**

```bash
git add packages/coordify-core/src/state.rs packages/coordify-core/src/ipc.rs
git commit -m "feat(core): agent state + generation, transition rules, claim store in State"
```

---

## Task 4: Server dispatch — CLAIM_PROPOSED + CLAIM_RELEASED

**Files:**
- Modify: `packages/coordify-core/src/server.rs` (add `handle_cap_event`, route `submit_event` to it, `cap_err` helper)
- Modify: `packages/coordify-core/tests/integration.rs` (claim integration tests)

**Interfaces:**
- Consumes: `crate::cap::{self, CapEvent, CapErrorCode, ClaimStatus}`, `crate::ipc::Response`.
- Produces:
  - `server::handle_cap_event(shared: &Arc<Shared>, req: &Request) -> Response` — validates cap_version, decodes the event, dispatches the claim variants (state + clear are added in Task 5).
  - `server::cap_err(id: &str, code: CapErrorCode) -> Response` (helper: `Response::err(id, code.as_str())`).

> **Dispatch rules for this task (CAP_SPEC §13):**
> - `submit_event` with `cap_version != Some("0.1")` → `cap_err(UnsupportedCapVersion)`.
> - decode failure → `cap_err(SchemaValidationFailed)`.
> - `ClaimProposed`: if the agent is unknown → `cap_err(AgentNotFound)`. Else `state.claims.propose(...)`:
>   - `Some(claim)` → if `claim.status == Active`, `state.promote_active(agent_id)`; append `{type:"CLAIM_CREATED", claimId, agentId, status, ts}`; respond `ok_with_data({claimId, status})`.
>   - `None` (rejected) → append `{type:"CLAIM_REJECTED", agentId, reason:"LOW_CONFIDENCE", ts}`; respond `ok_with_data({status:"REJECTED", reason:"LOW_CONFIDENCE"})`.
> - `ClaimReleased`: if the claim does not exist → `cap_err(ClaimNotFound)`. Else `state.claims.release(claim_id)`; append `{type:"CLAIM_RELEASED", claimId, agentId, reason, ts}`; respond `ok_for`.
> - `AgentStateChanged` / `ClearInvoked` arms: leave a `todo!()`-free placeholder that returns `cap_err(SchemaValidationFailed)` is NOT acceptable — instead, this task adds the two claim arms and a catch-all `_ => cap_err(&req.id, CapErrorCode::SchemaValidationFailed)` so the function compiles; Task 5 replaces the catch-all with the state/clear arms. (Do not use `todo!()`/`unimplemented!()`.)

Lock discipline: lock `state` in a short scope to mutate, release it, then lock `log` to append. Never hold both.

- [ ] **Step 1: Add imports to `src/server.rs`**

Add near the existing `use` lines:
```rust
use crate::cap::{self, CapErrorCode, CapEvent, ClaimStatus};
use crate::ipc::Request;
```
(If `Request` is already imported, do not duplicate it.)

- [ ] **Step 2: Route `submit_event` to the new handler**

In `handle_request`, replace the existing `"submit_event"` arm:
```rust
        "submit_event" => handle_cap_event(shared, req),
```
Note: `handle_request`'s signature is `handle_request(shared: &Shared, req: &Request)`. `handle_cap_event` needs `&Arc<Shared>` only if it spawns; here it does not — make `handle_cap_event(shared: &Shared, req: &Request) -> Response` to match. Adjust the import/signature accordingly (use `&Shared`).

- [ ] **Step 3: Add `cap_err` and `handle_cap_event` (claim arms) to `src/server.rs`**

```rust
fn cap_err(id: &str, code: CapErrorCode) -> Response {
    Response::err(id, code.as_str())
}

fn handle_cap_event(shared: &Shared, req: &Request) -> Response {
    if req.cap_version.as_deref() != Some("0.1") {
        return cap_err(&req.id, CapErrorCode::UnsupportedCapVersion);
    }
    let event = match cap::decode_event(&req.event) {
        Ok(e) => e,
        Err(code) => return cap_err(&req.id, code),
    };
    match event {
        CapEvent::ClaimProposed {
            agent_id,
            intent,
            domains,
            estimated_files,
            confidence,
            ..
        } => {
            // Agent must exist.
            {
                let st = shared.state.lock().unwrap();
                if st.agent_state(&agent_id).is_none() {
                    return cap_err(&req.id, CapErrorCode::AgentNotFound);
                }
            }
            let created = {
                let mut st = shared.state.lock().unwrap();
                st.claims.propose(
                    &agent_id,
                    intent.as_str().to_string(),
                    domains,
                    estimated_files,
                    confidence,
                )
            };
            match created {
                Some(claim) => {
                    if claim.status == ClaimStatus::Active {
                        shared.state.lock().unwrap().promote_active(&agent_id);
                    }
                    let event = serde_json::json!({
                        "type": "CLAIM_CREATED",
                        "claimId": claim.claim_id,
                        "agentId": agent_id,
                        "status": claim.status.as_str(),
                        "ts": crate::bootstrap::now_iso(),
                    });
                    let _ = shared.log.lock().unwrap().append(&event);
                    Response::ok_with_data(
                        &req.id,
                        serde_json::json!({"claimId": claim.claim_id, "status": claim.status.as_str()}),
                    )
                }
                None => {
                    let event = serde_json::json!({
                        "type": "CLAIM_REJECTED",
                        "agentId": agent_id,
                        "reason": "LOW_CONFIDENCE",
                        "ts": crate::bootstrap::now_iso(),
                    });
                    let _ = shared.log.lock().unwrap().append(&event);
                    Response::ok_with_data(
                        &req.id,
                        serde_json::json!({"status": "REJECTED", "reason": "LOW_CONFIDENCE"}),
                    )
                }
            }
        }
        CapEvent::ClaimReleased { claim_id, agent_id, reason } => {
            let released = {
                let mut st = shared.state.lock().unwrap();
                st.claims.release(&claim_id)
            };
            if !released {
                return cap_err(&req.id, CapErrorCode::ClaimNotFound);
            }
            let event = serde_json::json!({
                "type": "CLAIM_RELEASED",
                "claimId": claim_id,
                "agentId": agent_id,
                "reason": serde_json::to_value(reason).unwrap(),
                "ts": crate::bootstrap::now_iso(),
            });
            let _ = shared.log.lock().unwrap().append(&event);
            Response::ok_for(&req.id)
        }
        // Task 5 replaces this catch-all with AgentStateChanged + ClearInvoked.
        _ => cap_err(&req.id, CapErrorCode::SchemaValidationFailed),
    }
}
```

- [ ] **Step 4: Add unit tests for claim dispatch** (append to `server::tests`)

```rust
    fn cap_req(token: &str, event: serde_json::Value) -> Request {
        Request {
            id: "r1".to_string(),
            token: token.to_string(),
            action: "submit_event".to_string(),
            agent_id: None,
            meta: json!({}),
            event,
            cap_version: Some("0.1".to_string()),
        }
    }

    #[test]
    fn claim_proposed_active_creates_claim_and_activates_agent() {
        let s = shared_for_test("good");
        let reg = handle_request(&s, &req("good", "register"));
        let agent = reg.agent_id.unwrap();
        let resp = handle_request(
            &s,
            &cap_req("good", json!({"type":"CLAIM_PROPOSED","agentId":agent,"intent":"BUGFIX","confidence":0.9})),
        );
        assert!(resp.ok);
        let data = resp.data.unwrap();
        assert_eq!(data["status"], "ACTIVE");
        assert!(data["claimId"].as_str().unwrap().starts_with("claim-"));
        assert_eq!(s.state.lock().unwrap().agent_state(&agent), Some(crate::cap::AgentState::Active));
    }

    #[test]
    fn claim_proposed_low_confidence_is_rejected() {
        let s = shared_for_test("good");
        let reg = handle_request(&s, &req("good", "register"));
        let agent = reg.agent_id.unwrap();
        let resp = handle_request(
            &s,
            &cap_req("good", json!({"type":"CLAIM_PROPOSED","agentId":agent,"intent":"BUGFIX","confidence":0.1})),
        );
        assert!(resp.ok);
        assert_eq!(resp.data.unwrap()["status"], "REJECTED");
        // Agent stays DISCOVERY.
        assert_eq!(s.state.lock().unwrap().agent_state(&agent), Some(crate::cap::AgentState::Discovery));
    }

    #[test]
    fn claim_proposed_unknown_agent_errors() {
        let s = shared_for_test("good");
        let resp = handle_request(
            &s,
            &cap_req("good", json!({"type":"CLAIM_PROPOSED","agentId":"agent-404","intent":"BUGFIX","confidence":0.9})),
        );
        assert!(!resp.ok);
        assert_eq!(resp.error.as_deref(), Some("AGENT_NOT_FOUND"));
    }

    #[test]
    fn bad_cap_version_and_unknown_event_and_release_missing() {
        let s = shared_for_test("good");
        // wrong cap version
        let mut r = cap_req("good", json!({"type":"CLAIM_RELEASED","claimId":"claim-1","agentId":"a","reason":"TASK_COMPLETED"}));
        r.cap_version = Some("9.9".to_string());
        assert_eq!(handle_request(&s, &r).error.as_deref(), Some("UNSUPPORTED_CAP_VERSION"));
        // unknown event type
        let resp = handle_request(&s, &cap_req("good", json!({"type":"NONSENSE"})));
        assert_eq!(resp.error.as_deref(), Some("SCHEMA_VALIDATION_FAILED"));
        // release a claim that doesn't exist
        let resp = handle_request(&s, &cap_req("good", json!({"type":"CLAIM_RELEASED","claimId":"claim-404","agentId":"a","reason":"TASK_COMPLETED"})));
        assert_eq!(resp.error.as_deref(), Some("CLAIM_NOT_FOUND"));
    }
```

> Note: the existing `req(...)` helper in `server::tests` builds a `Request`; it must be updated to include `cap_version: None` now that the struct has the field. Update the helper's literal accordingly (add `cap_version: None,`).

- [ ] **Step 5: Run unit tests — expect PASS**

Run: `cd packages/coordify-core && cargo test server::tests`
Expected: prior server tests + the four new claim tests pass.

- [ ] **Step 6: Add a claim integration test** (append to `tests/integration.rs`)

```rust
#[test]
fn claim_proposed_and_released_over_socket() {
    let core = spawn_core("claim");
    let token = read_token(&core.root);
    let sock = core.root.join(".coordify/runtime/core.sock");
    let mut stream = connect_retry(&sock);

    let reg = format!(r#"{{"id":"1","token":"{}","action":"register","meta":{{}}}}"#, token);
    let agent = send_line(&mut stream, &reg)["agent_id"].as_str().unwrap().to_string();

    let propose = format!(
        r#"{{"id":"2","token":"{}","action":"submit_event","capVersion":"0.1","event":{{"type":"CLAIM_PROPOSED","agentId":"{}","intent":"BUGFIX","confidence":0.9}}}}"#,
        token, agent
    );
    let resp = send_line(&mut stream, &propose);
    assert_eq!(resp["ok"], true);
    let claim_id = resp["data"]["claimId"].as_str().unwrap().to_string();
    assert_eq!(resp["data"]["status"], "ACTIVE");

    let release = format!(
        r#"{{"id":"3","token":"{}","action":"submit_event","capVersion":"0.1","event":{{"type":"CLAIM_RELEASED","claimId":"{}","agentId":"{}","reason":"TASK_COMPLETED"}}}}"#,
        token, claim_id, agent
    );
    assert_eq!(send_line(&mut stream, &release)["ok"], true);
}
```

> Note: the request envelope sent over the socket uses `"capVersion"` (camelCase). Task 3 Step 1 already renames `cap_version` to `capVersion` via `#[serde(rename = "capVersion")]`, so this deserializes correctly. Other `Request` fields stay snake_case (Phase 1 already sends `"agent_id"` in heartbeat requests and it works).

- [ ] **Step 7: Run integration tests — expect PASS**

Run: `cd packages/coordify-core && cargo test --test integration claim_proposed_and_released_over_socket`
Expected: passes.

- [ ] **Step 8: Full suite + clippy**

Run: `cd packages/coordify-core && cargo test && cargo clippy --all-targets -- -D warnings`
Expected: all pass; clippy clean.

- [ ] **Step 9: Commit**

```bash
git add packages/coordify-core/src/server.rs packages/coordify-core/src/ipc.rs packages/coordify-core/tests/integration.rs
git commit -m "feat(core): CAP dispatch for CLAIM_PROPOSED + CLAIM_RELEASED"
```

---

## Task 5: Server dispatch — AGENT_STATE_CHANGED + CLEAR_INVOKED

**Files:**
- Modify: `packages/coordify-core/src/server.rs` (replace the catch-all with the two arms)
- Modify: `packages/coordify-core/tests/integration.rs` (state-change + `/clear` integration tests)

**Interfaces:**
- Consumes: `crate::state::StateError`.
- Produces: extends `handle_cap_event` with `AgentStateChanged` and `ClearInvoked` arms (no new public functions).

> **Dispatch rules (CAP_SPEC §7, §12):**
> - `AgentStateChanged`: `state.set_state(agent_id, state)`:
>   - `Ok(())` → append `{type:"AGENT_STATE_CHANGED", agentId, state, ts}`; respond `ok_for`.
>   - `Err(AgentNotFound)` → `cap_err(AgentNotFound)`.
>   - `Err(InvalidTransition)` → `cap_err(InvalidStateTransition)`.
> - `ClearInvoked`: if agent unknown → `cap_err(AgentNotFound)`. Else: release all the agent's live claims (`state.claims.release_for_agent`), then `state.clear(agent_id)` for the new generation. Append, in order: one `CLAIM_RELEASED` (reason `CLEAR_INVOKED`) per released claim, then `CLEAR_INVOKED {agentId, newGeneration}`, then `AGENT_GENERATION_INCREMENTED {agentId, generation}`. Respond `ok_with_data({generation})`.

Lock discipline: gather `(released_ids, new_generation)` under one short `state` lock scope, release it, then append all events under the `log` lock.

- [ ] **Step 1: Add the import** in `src/server.rs`:
```rust
use crate::state::StateError;
```

- [ ] **Step 2: Replace the catch-all arm in `handle_cap_event`**

Replace:
```rust
        // Task 5 replaces this catch-all with AgentStateChanged + ClearInvoked.
        _ => cap_err(&req.id, CapErrorCode::SchemaValidationFailed),
```
with:
```rust
        CapEvent::AgentStateChanged { agent_id, state } => {
            let result = {
                let mut st = shared.state.lock().unwrap();
                st.set_state(&agent_id, state)
            };
            match result {
                Ok(()) => {
                    let event = serde_json::json!({
                        "type": "AGENT_STATE_CHANGED",
                        "agentId": agent_id,
                        "state": serde_json::to_value(state).unwrap(),
                        "ts": crate::bootstrap::now_iso(),
                    });
                    let _ = shared.log.lock().unwrap().append(&event);
                    Response::ok_for(&req.id)
                }
                Err(StateError::AgentNotFound) => cap_err(&req.id, CapErrorCode::AgentNotFound),
                Err(StateError::InvalidTransition) => {
                    cap_err(&req.id, CapErrorCode::InvalidStateTransition)
                }
            }
        }
        CapEvent::ClearInvoked { agent_id } => {
            let cleared = {
                let mut st = shared.state.lock().unwrap();
                if st.agent_state(&agent_id).is_none() {
                    None
                } else {
                    let released = st.claims.release_for_agent(&agent_id);
                    let generation = st.clear(&agent_id).unwrap();
                    Some((released, generation))
                }
            };
            let (released, generation) = match cleared {
                Some(v) => v,
                None => return cap_err(&req.id, CapErrorCode::AgentNotFound),
            };
            {
                let mut log = shared.log.lock().unwrap();
                for claim_id in &released {
                    let _ = log.append(&serde_json::json!({
                        "type": "CLAIM_RELEASED",
                        "claimId": claim_id,
                        "agentId": agent_id,
                        "reason": "CLEAR_INVOKED",
                        "ts": crate::bootstrap::now_iso(),
                    }));
                }
                let _ = log.append(&serde_json::json!({
                    "type": "CLEAR_INVOKED",
                    "agentId": agent_id,
                    "newGeneration": generation,
                    "ts": crate::bootstrap::now_iso(),
                }));
                let _ = log.append(&serde_json::json!({
                    "type": "AGENT_GENERATION_INCREMENTED",
                    "agentId": agent_id,
                    "generation": generation,
                    "ts": crate::bootstrap::now_iso(),
                }));
            }
            Response::ok_with_data(&req.id, serde_json::json!({"generation": generation}))
        }
```

- [ ] **Step 3: Add unit tests** (append to `server::tests`)

```rust
    #[test]
    fn agent_state_changed_valid_and_invalid() {
        let s = shared_for_test("good");
        let agent = handle_request(&s, &req("good", "register")).agent_id.unwrap();
        // Discovery -> Active ok
        let ok = handle_request(&s, &cap_req("good", json!({"type":"AGENT_STATE_CHANGED","agentId":agent,"state":"ACTIVE"})));
        assert!(ok.ok);
        // Active -> SUBAGENT_WAITING ok; then SUBAGENT_WAITING -> TESTING is invalid
        assert!(handle_request(&s, &cap_req("good", json!({"type":"AGENT_STATE_CHANGED","agentId":agent,"state":"SUBAGENT_WAITING"}))).ok);
        let bad = handle_request(&s, &cap_req("good", json!({"type":"AGENT_STATE_CHANGED","agentId":agent,"state":"TESTING"})));
        assert_eq!(bad.error.as_deref(), Some("INVALID_STATE_TRANSITION"));
        // unknown agent
        let nf = handle_request(&s, &cap_req("good", json!({"type":"AGENT_STATE_CHANGED","agentId":"agent-404","state":"IDLE"})));
        assert_eq!(nf.error.as_deref(), Some("AGENT_NOT_FOUND"));
    }

    #[test]
    fn clear_invoked_releases_claims_and_bumps_generation() {
        let s = shared_for_test("good");
        let agent = handle_request(&s, &req("good", "register")).agent_id.unwrap();
        let propose = handle_request(&s, &cap_req("good", json!({"type":"CLAIM_PROPOSED","agentId":agent,"intent":"BUGFIX","confidence":0.9})));
        let claim_id = propose.data.unwrap()["claimId"].as_str().unwrap().to_string();

        let resp = handle_request(&s, &cap_req("good", json!({"type":"CLEAR_INVOKED","agentId":agent})));
        assert!(resp.ok);
        assert_eq!(resp.data.unwrap()["generation"], 2);
        let st = s.state.lock().unwrap();
        assert_eq!(st.agent_state(&agent), Some(crate::cap::AgentState::Discovery));
        assert_eq!(st.claims.get(&claim_id).unwrap().status, crate::cap::ClaimStatus::Released);
    }

    #[test]
    fn clear_invoked_unknown_agent_errors() {
        let s = shared_for_test("good");
        let resp = handle_request(&s, &cap_req("good", json!({"type":"CLEAR_INVOKED","agentId":"agent-404"})));
        assert_eq!(resp.error.as_deref(), Some("AGENT_NOT_FOUND"));
    }
```

- [ ] **Step 4: Run unit tests — expect PASS**

Run: `cd packages/coordify-core && cargo test server::tests`
Expected: all prior + three new tests pass.

- [ ] **Step 5: Add integration tests** (append to `tests/integration.rs`)

```rust
#[test]
fn agent_state_change_over_socket() {
    let core = spawn_core("state");
    let token = read_token(&core.root);
    let sock = core.root.join(".coordify/runtime/core.sock");
    let mut stream = connect_retry(&sock);
    let reg = format!(r#"{{"id":"1","token":"{}","action":"register","meta":{{}}}}"#, token);
    let agent = send_line(&mut stream, &reg)["agent_id"].as_str().unwrap().to_string();

    let good = format!(
        r#"{{"id":"2","token":"{}","action":"submit_event","capVersion":"0.1","event":{{"type":"AGENT_STATE_CHANGED","agentId":"{}","state":"ACTIVE"}}}}"#,
        token, agent
    );
    assert_eq!(send_line(&mut stream, &good)["ok"], true);

    // Active -> WAITING_USER is not a legal direct transition.
    let bad = format!(
        r#"{{"id":"3","token":"{}","action":"submit_event","capVersion":"0.1","event":{{"type":"AGENT_STATE_CHANGED","agentId":"{}","state":"WAITING_USER"}}}}"#,
        token, agent
    );
    let resp = send_line(&mut stream, &bad);
    assert_eq!(resp["ok"], false);
    assert_eq!(resp["error"], "INVALID_STATE_TRANSITION");
}

#[test]
fn clear_invoked_over_socket_releases_and_bumps_generation() {
    let core = spawn_core("clear");
    let token = read_token(&core.root);
    let sock = core.root.join(".coordify/runtime/core.sock");
    let mut stream = connect_retry(&sock);
    let reg = format!(r#"{{"id":"1","token":"{}","action":"register","meta":{{}}}}"#, token);
    let agent = send_line(&mut stream, &reg)["agent_id"].as_str().unwrap().to_string();

    let propose = format!(
        r#"{{"id":"2","token":"{}","action":"submit_event","capVersion":"0.1","event":{{"type":"CLAIM_PROPOSED","agentId":"{}","intent":"BUGFIX","confidence":0.9}}}}"#,
        token, agent
    );
    assert_eq!(send_line(&mut stream, &propose)["data"]["status"], "ACTIVE");

    let clear = format!(
        r#"{{"id":"3","token":"{}","action":"submit_event","capVersion":"0.1","event":{{"type":"CLEAR_INVOKED","agentId":"{}"}}}}"#,
        token, agent
    );
    let resp = send_line(&mut stream, &clear);
    assert_eq!(resp["ok"], true);
    assert_eq!(resp["data"]["generation"], 2);
}
```

- [ ] **Step 6: Run integration tests — expect PASS**

Run: `cd packages/coordify-core && cargo test --test integration agent_state_change_over_socket && cargo test --test integration clear_invoked_over_socket_releases_and_bumps_generation`
Expected: both pass.

- [ ] **Step 7: Full suite + clippy + multi-thread**

Run: `cd packages/coordify-core && cargo test && cargo test -- --test-threads=4 && cargo clippy --all-targets -- -D warnings`
Expected: all pass; clippy clean.

- [ ] **Step 8: Commit**

```bash
git add packages/coordify-core/src/server.rs packages/coordify-core/tests/integration.rs
git commit -m "feat(core): CAP dispatch for AGENT_STATE_CHANGED + CLEAR_INVOKED"
```

---

## Task 6: Reaper orphan lifecycle + reclaimable sweep

**Files:**
- Modify: `packages/coordify-core/src/server.rs` (reaper orphans real claims + sweeps reclaimable; orphan-TTL param)
- Modify: `packages/coordify-core/tests/integration.rs` (orphan→reclaimable test; update the existing reaper test)

**Interfaces:**
- Consumes: `state.claims.orphan_for_agent`, `state.claims.sweep_reclaimable`.
- Produces: `spawn_reaper` gains an `orphan_ttl_ms: u64` parameter; `run` reads `COORDIFY_ORPHAN_TTL_MS` (default `300_000`).

> **Reaper behavior (CAP_SPEC §14):**
> - On each tick: reap timed-out agents (existing). For each lost agent, append `AGENT_LOST`, then orphan its live claims (`orphan_for_agent`) and append one `{type:"CLAIM_ORPHANED", claimId, previousOwner: agentId, ttlSeconds: orphan_ttl_ms/1000}` per orphaned claim. (Phase 1 emitted a placeholder `CLAIM_ORPHANED` per lost agent with no claim id — this REPLACES that: emit only for real claims.)
> - After handling lost agents, `sweep_reclaimable(now, orphan_ttl_ms)` and append `{type:"CLAIM_RECLAIMABLE", claimId}` per swept claim.
> - The empty-network finalize logic (Phase 1) is unchanged.

Lock discipline: collect lost ids under a short `state` lock; collect orphaned/reclaimable ids each under their own short `state` lock; then append events under the `log` lock. Never hold `state` while locking `log`.

- [ ] **Step 1: Update `spawn_reaper` in `src/server.rs`**

Change the signature to add `orphan_ttl_ms: u64` and rewrite the body so it orphans real claims and sweeps reclaimable. Replace the existing `spawn_reaper` with:
```rust
pub fn spawn_reaper(
    shared: Arc<Shared>,
    session: Session,
    paths: Paths,
    interval_ms: u64,
    timeout_ms: u64,
    orphan_ttl_ms: u64,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_millis(interval_ms));
        let now = now_ms();
        let lost = {
            let mut st = shared.state.lock().unwrap();
            st.reap(now, timeout_ms)
        };
        // Orphan each lost agent's live claims (collect under a short lock).
        let mut orphaned: Vec<(String, String)> = Vec::new(); // (claimId, previousOwner)
        if !lost.is_empty() {
            let mut st = shared.state.lock().unwrap();
            for id in &lost {
                for claim_id in st.claims.orphan_for_agent(id, now) {
                    orphaned.push((claim_id, id.clone()));
                }
            }
        }
        let reclaimable = {
            let mut st = shared.state.lock().unwrap();
            st.claims.sweep_reclaimable(now, orphan_ttl_ms)
        };

        if !lost.is_empty() || !orphaned.is_empty() || !reclaimable.is_empty() {
            let ttl_seconds = orphan_ttl_ms / 1000;
            let mut log = shared.log.lock().unwrap();
            for id in &lost {
                let _ = log.append(&serde_json::json!({
                    "type": "AGENT_LOST",
                    "agentId": id,
                    "ts": crate::bootstrap::now_iso(),
                }));
            }
            for (claim_id, previous_owner) in &orphaned {
                let _ = log.append(&serde_json::json!({
                    "type": "CLAIM_ORPHANED",
                    "claimId": claim_id,
                    "previousOwner": previous_owner,
                    "ttlSeconds": ttl_seconds,
                    "ts": crate::bootstrap::now_iso(),
                }));
            }
            for claim_id in &reclaimable {
                let _ = log.append(&serde_json::json!({
                    "type": "CLAIM_RECLAIMABLE",
                    "claimId": claim_id,
                    "ts": crate::bootstrap::now_iso(),
                }));
            }
        }

        // Empty-network finalize (unchanged from Phase 1).
        let empty = shared.state.lock().unwrap().agent_count() == 0;
        let seen = *shared.agents_seen.lock().unwrap();
        if empty
            && seen > 0
            && shared.finalized.compare_exchange(false, true, SeqCst, SeqCst).is_ok()
        {
            let _ = finalize(&session, &paths, seen);
            std::process::exit(0);
        }
    })
}
```

- [ ] **Step 2: Pass the TTL from `run`**

In `run`, after reading `timeout_ms`, add:
```rust
    let orphan_ttl_ms = std::env::var("COORDIFY_ORPHAN_TTL_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(300_000);
```
and update the `spawn_reaper(...)` call to pass `orphan_ttl_ms` as the last argument.

- [ ] **Step 3: Update the unit test that called `spawn_reaper` indirectly**

The existing `reaper_emits_lost_and_orphaned_events` server unit test calls `state.reap` directly (not `spawn_reaper`), so it still compiles. Confirm it still asserts only `reap` behavior. No change needed unless it referenced the old reaper signature.

- [ ] **Step 4: Update the existing integration test `reaper_logs_agent_lost_for_silent_agent`**

This Phase 1 test registered an agent (no claim) and asserted both `AGENT_LOST` and `CLAIM_ORPHANED` appear. Under Phase 2, an agent with NO claim produces `AGENT_LOST` but NO `CLAIM_ORPHANED`. Update the test to propose a claim before going silent, so `CLAIM_ORPHANED` is expected:

Replace its body's register step with register + propose, then keep the open-socket/no-heartbeat wait, and assert the log contains `AGENT_LOST` and `CLAIM_ORPHANED`:
```rust
#[test]
fn reaper_logs_agent_lost_and_orphans_claim() {
    let core = spawn_core_fast_reaper("reap");
    let token = read_token(&core.root);
    let sock = core.root.join(".coordify/runtime/core.sock");

    let mut stream = connect_retry(&sock);
    let reg = format!(r#"{{"id":"1","token":"{}","action":"register","meta":{{}}}}"#, token);
    let agent = send_line(&mut stream, &reg)["agent_id"].as_str().unwrap().to_string();
    let propose = format!(
        r#"{{"id":"2","token":"{}","action":"submit_event","capVersion":"0.1","event":{{"type":"CLAIM_PROPOSED","agentId":"{}","intent":"BUGFIX","confidence":0.9}}}}"#,
        token, agent
    );
    assert_eq!(send_line(&mut stream, &propose)["ok"], true);

    // Keep the connection open, send no heartbeats; reaper times the agent out.
    std::thread::sleep(Duration::from_millis(700));

    let sessions = core.root.join(".coordify/sessions");
    let mut log_contents = String::new();
    if let Ok(entries) = std::fs::read_dir(&sessions) {
        for e in entries.flatten() {
            let log = e.path().join("events.log");
            if log.exists() {
                log_contents = std::fs::read_to_string(log).unwrap();
            }
        }
    }
    assert!(log_contents.contains("AGENT_LOST"), "no AGENT_LOST logged");
    assert!(log_contents.contains("CLAIM_ORPHANED"), "no CLAIM_ORPHANED logged");
    drop(stream);
}
```
Delete the old `reaper_logs_agent_lost_for_silent_agent` test (replaced by the above). If a now-silent agent with no claim is still wanted as a separate case, it is covered by `reaper_finalizes_when_last_silent_agent_times_out` (which registers no claim and still finalizes).

- [ ] **Step 5: Add an orphan→reclaimable integration test** (append to `tests/integration.rs`)

Add a spawn helper with a fast orphan TTL and short reaper timing:
```rust
fn spawn_core_fast_orphan(tag: &str) -> Spawned {
    let root = temp_root(tag);
    let child = Command::new(env!("CARGO_BIN_EXE_coordify-core"))
        .arg("--root")
        .arg(&root)
        .env("COORDIFY_REAPER_INTERVAL_MS", "100")
        .env("COORDIFY_REAPER_TIMEOUT_MS", "300")
        .env("COORDIFY_ORPHAN_TTL_MS", "300")
        .spawn()
        .expect("failed to spawn coordify-core");
    let sock = root.join(".coordify/runtime/core.sock");
    assert!(wait_for(&sock, Duration::from_secs(5)), "socket never appeared");
    Spawned { child, root }
}

#[test]
fn orphaned_claim_becomes_reclaimable_after_ttl() {
    let core = spawn_core_fast_orphan("orph");
    let token = read_token(&core.root);
    let sock = core.root.join(".coordify/runtime/core.sock");
    let mut stream = connect_retry(&sock);
    let reg = format!(r#"{{"id":"1","token":"{}","action":"register","meta":{{}}}}"#, token);
    let agent = send_line(&mut stream, &reg)["agent_id"].as_str().unwrap().to_string();
    let propose = format!(
        r#"{{"id":"2","token":"{}","action":"submit_event","capVersion":"0.1","event":{{"type":"CLAIM_PROPOSED","agentId":"{}","intent":"BUGFIX","confidence":0.9}}}}"#,
        token, agent
    );
    assert_eq!(send_line(&mut stream, &propose)["ok"], true);

    // Keep socket open, no heartbeat: reaped at ~300ms (CLAIM_ORPHANED), then
    // swept to RECLAIMABLE once orphaned >= 300ms TTL on a later tick.
    std::thread::sleep(Duration::from_millis(1200));

    let sessions = core.root.join(".coordify/sessions");
    let mut log_contents = String::new();
    if let Ok(entries) = std::fs::read_dir(&sessions) {
        for e in entries.flatten() {
            let log = e.path().join("events.log");
            if log.exists() {
                log_contents = std::fs::read_to_string(log).unwrap();
            }
        }
    }
    assert!(log_contents.contains("CLAIM_ORPHANED"), "no CLAIM_ORPHANED");
    assert!(log_contents.contains("CLAIM_RECLAIMABLE"), "claim never became RECLAIMABLE");
    drop(stream);
}
```

- [ ] **Step 6: Run integration tests — expect PASS**

Run: `cd packages/coordify-core && cargo test --test integration reaper_logs_agent_lost_and_orphans_claim && cargo test --test integration orphaned_claim_becomes_reclaimable_after_ttl`
Expected: both pass. If timing-flaky, increase the sleeps (do not weaken assertions).

- [ ] **Step 7: Full suite (multiple runs) + multi-thread + clippy + coverage**

Run:
```bash
cd packages/coordify-core
for i in 1 2 3 4 5; do cargo test --test integration >/dev/null 2>&1 && echo "run $i ok" || echo "run $i FAIL"; done
cargo test -- --test-threads=4
cargo clippy --all-targets -- -D warnings
cargo llvm-cov --summary-only -- --test-threads=4 | tail -3
```
Expected: 5/5 integration runs ok (no flakiness); full suite passes under `--test-threads=4`; clippy clean; TOTAL line coverage ≥ 90% (target ≥ 95%).

- [ ] **Step 8: Commit**

```bash
git add packages/coordify-core/src/server.rs packages/coordify-core/tests/integration.rs
git commit -m "feat(core): reaper orphans claims with TTL, sweeps to RECLAIMABLE"
```

---

## Out of Scope (Phase 3+)

Recorded so reviewers do not flag these as gaps:
- Heat calculation, predicted heat, heat bands/thresholds — Phase 3.
- Conflicts, negotiation, deadlock, handoff, and the claim statuses they need (`SHARED`, `TRANSFERRED`, `EXPIRED`) — Phase 4.
- `CLAIM_RECLAIMED` by a new owner (reclaim action) — Phase 4 (Phase 2 only marks `RECLAIMABLE`).
- Tool/file/task CAP events (`TOOL_PRECHECK`, `FILE_TOUCHED`, `TASK_DECLARED`, …) — rejected for now; ingested in their phases.
- Full CAP envelope (§4: messageId, messageKind, projectRoot, sessionId, timestamp) — only `capVersion` is checked here.
- Config-file tuning of confidence thresholds / TTL — hardcoded defaults + env overrides this phase.
- JSON Schema files (`.schema.json`) for cross-language validation — deferred; typed serde is the Phase 2 validator.
- **Claim ownership enforcement on `CLAIM_RELEASED`** — Phase 2 releases by `claimId` without checking the requester owns the claim (the event's `agentId` is logged, not enforced). Ownership/authorization checks are Phase 4 (conflict/handoff).
- **PROVISIONAL claim does not promote the agent to ACTIVE** — deliberate. Per CAP_SPEC §9 a PROVISIONAL claim (confidence 0.45–0.75) is a lower-commitment state ("requires recheck before risky writes", "should be upgraded or released once clearer"). Only an ACTIVE-status claim promotes DISCOVERY→ACTIVE. PROVISIONAL→ACTIVE upgrade is Phase 3.

## Self-Review Notes

- **Spec coverage (ARCHITECTURE §27 Phase 2):** schemas (Task 1) ✓; event ingestion (Task 4,5) ✓; agent states (Task 3,5) ✓; claim lifecycle (Task 2,4) ✓; `/clear` (Task 5) ✓; orphaned claims (Task 2,6) ✓.
- **CAP_SPEC coverage:** canonical enums/strings (Task 1) ✓; claim confidence rules §9 (Task 2) ✓; state transitions §7 (Task 3) ✓; `/clear` effects §12 (Task 5) ✓; orphan→reclaimable §14 (Task 6) ✓; error codes §28 (Task 1,4,5) ✓.
- **Type consistency:** `AgentState`, `Intent`, `ClaimStatus`, `ReleaseReason`, `CapErrorCode`, `CapEvent`, `Claim`, `ClaimStore`, `StateError`, the new `Request.cap_version`/`Response.data` fields, and `spawn_reaper`'s `orphan_ttl_ms` param are referenced identically across Tasks 1-6.
- **Lock discipline:** every server handler and the reaper lock `state` in short scopes and release before locking `log` — preserving the Phase 1 invariant.
- **Strictness:** unknown CAP types and bad canonical values are rejected (Task 1 decode + Task 4 catch-all replaced in Task 5), honoring CAP_SPEC §31.
