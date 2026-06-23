# Coordify Phase 2 — CAP Foundation Design

**Status:** Approved (schema approach decided)
**Date:** 2026-06-23
**Depends on:** Phase 1 Core skeleton (merged), `absolute-docs/CAP_SPEC.md`, `absolute-docs/ARCHITECTURE.md` §27 Phase 2.

## Goal

Turn the Phase 1 daemon — which logs events verbatim — into a CAP state machine: typed event validation, agent states, claim lifecycle, `/clear`, and orphaned-claim TTL.

## Scope (ARCHITECTURE §27 Phase 2)

In:
- **CAP event validation** — typed serde events; deserialization IS validation; bad canonical values → `CAP_ERROR`.
- **Event ingestion** — `submit_event` parses a typed `CapEvent` and dispatches by type, replacing Phase 1's verbatim append.
- **Agent states** — per CAP_SPEC §7 state set + transition rules; `AGENT_STATE_CHANGED`.
- **Claim lifecycle** — `CLAIM_PROPOSED` → `CLAIM_CREATED`/`CLAIM_REJECTED` (by confidence), `CLAIM_RELEASED`.
- **`/clear`** — `CLEAR_INVOKED`: release agent claims, state → `DISCOVERY`, generation++, `AGENT_GENERATION_INCREMENTED`.
- **Orphaned claims** — reaper marks a lost agent's claims `ORPHANED` (with TTL), then `RECLAIMABLE` after `orphanTtlSeconds`.

Out (Phase 3+):
- Heat calculation, `PREDICTED_HEAT_CALCULATED`, heat bands (Phase 3).
- Conflicts, negotiation, deadlock, handoff (Phase 4).
- Claim statuses that require those: `SHARED`, `TRANSFERRED`, `EXPIRED`, `RECLAIMED` by another agent (reclaim *action* deferred; Phase 2 only marks `RECLAIMABLE`).
- Tool/file/task events (`TOOL_PRECHECK`, `FILE_TOUCHED`, `TASK_DECLARED`, …) — not ingested yet; Core rejects unrecognized CAP types (strict per §31).
- Knowledge, stats, drift, ghost-work.
- Full CAP envelope migration (capVersion is checked; the rest of the §4 envelope stays Phase 3+).

## Design Decisions

1. **Validation = typed serde structs** (user-approved). `CapEvent` is a `#[serde(tag = "type")]` enum; canonical values (`Intent`, `AgentState`, claim status, release reason) are enums with `rename_all = "SCREAMING_SNAKE_CASE"`. A parse failure maps to `CAP_ERROR { code: SCHEMA_VALIDATION_FAILED }`. No `jsonschema` crate, no `.schema.json` files this phase.

2. **Strict ingestion.** Only CAP event types Phase 2 models are accepted. Unrecognized type → `CAP_ERROR`. No verbatim logging of unknown events (honors §31 "no untyped messages mutate state"). Phase 1's `submit_event` verbatim behavior is replaced.

3. **Transport unchanged.** Phase 1's `register`/`heartbeat` actions stay (they map to `AGENT_JOINED`/`HEARTBEAT`). CAP claim/state/clear events arrive via the existing `submit_event` action, now typed. `Request` gains an optional `cap_version` field; `submit_event` requires it to equal `"0.1"` (else `CAP_ERROR { UNSUPPORTED_CAP_VERSION }`).

4. **Confidence thresholds** (CAP_SPEC §9, config defaults from ARCHITECTURE §11): `>= 0.75` → `ACTIVE`; `0.45–0.749` → `PROVISIONAL`; `< 0.45` → rejected. Hardcoded defaults this phase (config wiring is later).

5. **Orphan TTL** default `300` s (ARCHITECTURE §11 `orphanTtlSeconds`), overridable via env `COORDIFY_ORPHAN_TTL_MS` for tests, mirroring the Phase 1 reaper env pattern.

6. **Core is the only state writer; the event log stays the recoverable source.** Every accepted event appends its canonical record(s) to `events.log` before the response is sent.

## Module Design

```text
packages/coordify-core/src/
  cap.rs       NEW  canonical enums (Intent, AgentState, ClaimStatus, ReleaseReason),
                    CapErrorCode, CapEvent enum (tag="type"), parse/validate helpers.
  claim.rs     NEW  Claim struct, confidence->status, ClaimStore (propose/release/
                    orphan/sweep-reclaimable) keyed by claimId, owner index by agentId.
  state.rs     MOD  Agent gains `state: AgentState` and `generation: u64`;
                    transition validation `can_transition(from,to)`; claims live in
                    ClaimStore held by State.
  server.rs    MOD  submit_event: parse CapEvent, dispatch claim/state/clear handlers,
                    emit derived events; reaper orphans claims + sweeps RECLAIMABLE.
  ipc.rs       MOD  Request gains `cap_version: Option<String>`.
```

`cap.rs` is pure types + parsing (no IO, unit-testable). `claim.rs` is pure logic (no IO). `state.rs` stays IO-free. `server.rs` remains the only IO+state wiring point — same separation as Phase 1, so the lock-ordering audit stays valid.

## Error Handling

`submit_event` responses reuse the Phase 1 `Response`: on a CAP failure, `ok=false` with `error` set to the `CapErrorCode` string (e.g. `"SCHEMA_VALIDATION_FAILED"`, `"INVALID_STATE_TRANSITION"`, `"CLAIM_NOT_FOUND"`, `"AGENT_NOT_FOUND"`, `"UNSUPPORTED_CAP_VERSION"`). No state mutation and no log append on a rejected event.

## Testing

- Unit: serde round-trip + bad-value rejection for every canonical enum; confidence→status; state-transition matrix (valid + invalid); orphan→reclaimable sweep.
- Integration (real socket + spawned binary): propose claim (active + provisional + rejected), release, state change (valid + rejected transition), `/clear` releasing claims and bumping generation, reaper orphaning a claim then marking it reclaimable under a fast TTL.
- Coverage gate stays at 90% (CI); target keeping ≥95%.

## Non-Negotiables Carried Forward (CAP_SPEC §31)

- No untyped message mutates state.
- No claim exists without schema validation.
- No `/clear` leaves ownership behind.
- No unclean crash silently deletes claims (they orphan with a tombstone TTL).
