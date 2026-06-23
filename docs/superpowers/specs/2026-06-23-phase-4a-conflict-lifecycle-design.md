# Coordify Phase 4a — Conflict Lifecycle Design

**Status:** Approved (scope decided: conflict lifecycle MVP)
**Date:** 2026-06-23
**Depends on:** Phase 3 Heat (merged), `absolute-docs/CAP_SPEC.md` §17/§19, `absolute-docs/ARCHITECTURE.md` §20/§27 Phase 4.

## Goal

Open and close CONFLICT objects driven by heat. When a pair's current heat reaches the conflict-candidate band, Core opens a conflict; when heat drops back, or a participant's claim goes away, Core closes it. This is the foundation the rest of Phase 4 (negotiation, deadlock, arbitration) builds on.

## Scope

In:
- **Conflict object + store** — pairwise, keyed by ordered agent pair (mirrors the heat edge store). Sequential `conflict-N` ids.
- **Open on threshold** — in the existing current-heat recompute, when a pair's edge band is `CONFLICT_CANDIDATE` (heat ≥ 76) and no conflict is open for the pair, open one → `CONFLICT_OPENED`.
- **Auto-resolve on heat drop** — when a pair with an open conflict recomputes to a band below `CONFLICT_CANDIDATE` (heat ≤ 75), resolve it → `CONFLICT_RESOLVED { resolution: "AUTO_RESOLVED_HEAT_DROPPED" }`.
- **Abort on participant exit** — when an agent's claim is released/cleared (its heat edges drop), abort any open conflict involving it → `CONFLICT_ABORTED { reason: "PARTICIPANT_LEFT" }`.

Out (Phase 4b / later):
- Negotiation: `CONFLICT_PROPOSAL_SUBMITTED`, proposal kinds, Core auto-resolve-vs-escalate (§18.4), `NEGOTIATING`/`AWAITING_AGENT_RESPONSE` states.
- User arbitration: `AWAITING_USER_DECISION`, arbitration events/timeouts.
- Deadlock detection / wait-graph (§20).
- Handoff (§21).
- Conflict timeouts (`CONFLICT_TIMEOUT`).
- Protected-path / intent-collision triggers (§17 lists more triggers; Phase 4a triggers on heat band only).

## Design Decisions

1. **Conflict opens at the `CONFLICT_CANDIDATE` band (heat ≥ 76), not merely heat > 50.** OVERLAP (51–75) keeps emitting `HEAT_THRESHOLD_EXCEEDED` (Phase 3, level 2 "coordinate") without opening a conflict object. The band is literally named `CONFLICT_CANDIDATE`; that is the conflict trigger. Resolve when the band falls below it.

2. **Conflict store mirrors the heat store** — `ConflictStore` keyed by ordered `(agentId, agentId)`, one open conflict per pair. `open`/`get`/`resolve`/`abort`/`remove_agent`. This keeps the conflict set consistent with the heat set (same pairs, same ordering).

3. **Driven entirely from `recompute_current_heat`.** Conflicts are a deterministic function of current heat, so they open/resolve in the same place heat edges are computed — no separate trigger path. The abort-on-exit case is the existing `heat.remove_agent` branch (no live claim): also abort that agent's conflicts.

4. **Full `ConflictState` enum defined now** (DETECTED, NEGOTIATING, AWAITING_AGENT_RESPONSE, AWAITING_USER_DECISION, RESOLVED, TIMEOUT, ABORTED per §17) so Phase 4b extends without a type change; Phase 4a only uses `Detected`, `Resolved`, `Aborted`.

5. **Lock discipline unchanged.** `ConflictStore` lives in `Shared` behind its own `Mutex`, locked in short scopes alongside `heat`/`log`, never nested with `state`/`log`. Conflict events append under the existing log lock in `recompute_current_heat`.

## Event Shapes (CAP_SPEC §17)

- `CONFLICT_OPENED { conflictId, agents:[a,b], openedAt, trigger:{type:"HEAT_THRESHOLD", heat}, paths, domains, intents, requiredAction:"NEGOTIATE_OR_REASSIGN", ts }`
- `CONFLICT_RESOLVED { conflictId, resolution:"AUTO_RESOLVED_HEAT_DROPPED", ts }`
- `CONFLICT_ABORTED { conflictId, reason:"PARTICIPANT_LEFT", ts }`

`paths`/`domains`/`intents` come from the two agents' live-claim inputs at open time (intersection where meaningful, union for context).

## Module Design

```text
packages/coordify-core/src/
  conflict.rs  NEW  Conflict, ConflictState, ConflictStore (open/get/resolve/abort/
                    remove_agent, sequential ids, has_open).
  server.rs    MOD  ConflictStore in Shared; in recompute_current_heat, after each
                    edge upsert: open on CONFLICT_CANDIDATE / resolve on drop; in the
                    no-live-claim branch, abort the agent's open conflicts. Conflict
                    events appended under the existing log lock.
  lib.rs       MOD  pub mod conflict;
```

`conflict.rs` is pure (no IO), fully unit-testable. `server.rs` stays the only IO+state site.

## Testing

- Unit: ConflictStore open/get/resolve/abort/remove_agent, sequential ids, has_open dedup (no double-open per pair), ordered-key direction independence.
- Integration (socket): two co-registered agents with high-overlap claims → `CONFLICT_OPENED` logged; (resolve/abort observed via the log). A low-overlap pair → no `CONFLICT_OPENED`.
- Unit (server): high-overlap pair opens exactly one conflict (no duplicate on recompute); releasing a participant aborts it.
- Coverage gate stays 90%; target ≥95%.

## Non-Negotiables Carried Forward

- Conflicts are deterministic from heat (no LLM). Core is the only writer.
- Lock discipline: never hold two of {state, heat, conflict, log} at once across a log append.
