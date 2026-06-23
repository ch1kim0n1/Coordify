# Coordify Phase 4b — Negotiation, Arbitration, Deadlock Design

**Status:** Approved (scope: full Phase 4 remainder)
**Date:** 2026-06-23
**Depends on:** Phase 4a Conflict Lifecycle (merged), `absolute-docs/CAP_SPEC.md` §18/§19/§20, `absolute-docs/ARCHITECTURE.md` §20/§27 Phase 4.

## Goal

Make open conflicts resolvable end-to-end. Agents submit proposals; Core compares them deterministically and either auto-resolves the conflict, escalates it to the user for arbitration, or detects a deadlock and escalates. A proposal timeout escalates a conflict whose participant never proposes. This completes ARCHITECTURE §27 Phase 4 (heat thresholds + conflict lifecycle landed in Phases 3/4a).

## Scope

In:
- **Proposal ingestion** — new CAP event `CONFLICT_PROPOSAL_SUBMITTED`; per-agent proposals stored on the conflict.
- **Core comparison** (§18.4) — deterministic `compare()` deciding auto-resolve vs escalate from the two proposals.
- **User arbitration** (§18.5) — escalation moves the conflict to `AWAITING_USER_DECISION` and emits a single identical arbitration prompt; `CONFLICT_USER_DECISION` applies the user's choice and resolves the conflict.
- **Deadlock detection** (§20) — a wait graph; mutual `QUEUE_TASK` proposals form a cycle → `DEADLOCK_DETECTED` → escalate.
- **Proposal timeout** (§18.6) — reaper escalates a conflict still missing a proposal after `proposal_timeout_ms` (default 60_000).

Out (later / separate):
- Handoff execution / claim transfer (§21) — `TRANSFER_TASK` here means only "I step aside"; the conflict resolves, no claim is moved.
- Persistent config file — hardcoded defaults + env override only (config file is Phase 5/6).
- Heat debouncing/caching, protected-path globbing beyond prefix match.
- Core mutating agent FSM state on conflict (see Decision 6).

## Design Decisions

1. **Proposals arrive as a CAP event and are stored on the conflict.** `CONFLICT_PROPOSAL_SUBMITTED { conflictId, from, proposal }` is decoded like every other CAP event (deserialization is validation). The proposal is recorded in `Conflict.proposals[from]`. When both participants have proposed, Core runs `compare()`.

2. **`compare()` is a pure, total function** over `(Proposal, Proposal, paths, ConflictConfig) → Decision`, where `Decision = AutoResolve { resolution } | Escalate { reason }`. Rules, evaluated in order:
   - either proposal `requiresUserApproval == true` → `Escalate("USER_APPROVAL_REQUIRED")`.
   - either kind == `ASK_USER` → `Escalate("AGENT_REQUESTED_USER")`.
   - a protected path is involved → `Escalate("PROTECTED_PATH")`.
   - either kind ∈ {`YIELD_CLAIM`, `ABORT_TASK`, `TRANSFER_TASK`} → `AutoResolve("PARTICIPANT_STEPPED_ASIDE")`.
   - exactly one kind == `QUEUE_TASK` → `AutoResolve("QUEUED")`.
   - both kinds == `SPLIT_SCOPE` → disjoint claim-change file sets → `AutoResolve("SCOPE_SPLIT")`, else `Escalate("OVERLAPPING_SPLIT")`.
   - both kinds == `CO_OWN` → `cfg.allow_co_own` → `AutoResolve("CO_OWNERSHIP")`, else `Escalate("CO_OWN_DISALLOWED")`.
   - otherwise (mixed kinds, both keep) → `Escalate("INCOMPATIBLE_PROPOSALS")`.
   The both-`QUEUE_TASK` case never reaches rule 5: the server detects it first as a deadlock (Decision 4).

3. **Comparison is server-orchestrated; deadlock is layered on top.** When the second proposal lands, the server: (a) adds a wait edge for each `QUEUE_TASK` proposer; (b) if that produces a cycle → deadlock path (emit `DEADLOCK_DETECTED`, escalate); (c) otherwise call `compare()` and act on the `Decision`.

4. **Deadlock = a cycle in the wait graph.** `waitgraph.rs` holds directed edges `from → to` labelled with a `resource` string. A `QUEUE_TASK` proposal from agent X in conflict (X,Y) adds edge `X → Y` (resource = the conflict's joined paths). `find_cycle()` returns the edges of a cycle if one exists (the 2-cycle from mutual queueing is the minimum case). Deadlock always escalates (§20). `remove_agent` clears an agent's edges when its claim is released/cleared or its conflict resolves/aborts.

5. **Proposal timeout is reaper-driven.** Each `Conflict` records `opened_at_ms`. The existing reaper loop (2s) sweeps open conflicts; any conflict whose age exceeds `proposal_timeout_ms` and that still lacks a proposal from at least one participant is escalated (`CONFLICT_TIMEOUT` + arbitration). Already-escalated/awaiting conflicts are skipped. The sweep locks `conflicts` then `log` in short scopes, never nested with `state`.

6. **Core does not mutate agent FSM state on conflict events.** §18.1 says affected agents "enter NEGOTIATING" — that is the agent reacting to `CONFLICT_OPENED`, not Core forcing a transition. Agents self-report `NEGOTIATING` / `WAITING_USER` through the existing `AGENT_STATE_CHANGED` path (validated by the state machine). The `Conflict.state` field is the authoritative record of where negotiation stands; agent state stays the agent's own. This avoids coupling the conflict engine to transition legality.

7. **`ConflictConfig` carries the knobs**, hardcoded defaults + env override (mirrors `HeatConfig`): `protected_paths: Vec<String>` (default empty; prefix match), `allow_co_own: bool` (default true), `proposal_timeout_ms: u64` (default 60_000, env `COORDIFY_PROPOSAL_TIMEOUT_MS`). Lives in `Shared`.

8. **Conflict state transitions** (uses the existing `ConflictState` enum):
   - `Detected` (4a, on open) → `Negotiating` (first proposal recorded).
   - `Negotiating` → `Resolved` (auto-resolve, or `CONFLICT_USER_DECISION`), or → `AwaitingUserDecision` (escalate), or → `Timeout` then `AwaitingUserDecision` (reaper timeout), or → `Aborted` (participant left, 4a).
   - `AwaitingUserDecision` → `Resolved` (`CONFLICT_USER_DECISION`) or `Aborted` (participant left).

## Event Shapes

Inbound (decoded CAP events):
- `CONFLICT_PROPOSAL_SUBMITTED { conflictId, from, proposal:{ kind, summary, claimChanges:[{agentId, keep?:[..], take?:[..]}], requiresUserApproval } }`
- `CONFLICT_USER_DECISION { conflictId, choice }`  — `choice` is an opaque string (e.g. an option id or free text); Core records it and resolves.

Outbound (appended to `events.log`):
- `CONFLICT_PROPOSAL_RECEIVED { conflictId, from, kind, summary, ts }`
- `CONFLICT_RESOLVED { conflictId, resolution, ts }` — `resolution` ∈ {`PARTICIPANT_STEPPED_ASIDE`, `QUEUED`, `SCOPE_SPLIT`, `CO_OWNERSHIP`, `USER_ARBITRATED`} (4a's `AUTO_RESOLVED_HEAT_DROPPED` still applies on heat drop).
- `CONFLICT_ESCALATED { conflictId, reason, ts }`
- `USER_ARBITRATION_REQUIRED { conflictId, agents:[a,b], prompt, options:[{id, summary}], ts }` — options built from the two proposals; both agents see identical framing (§18.5).
- `DEADLOCK_DETECTED { agents:[..], waitEdges:[{from, to, resource}], requiredAction:"USER_ARBITRATION", ts }`
- `CONFLICT_TIMEOUT { conflictId, ts }`

## Module Design

```text
packages/coordify-core/src/
  cap.rs        MOD  ProposalKind enum; CONFLICT_PROPOSAL_SUBMITTED + CONFLICT_USER_DECISION
                     CapEvent variants; Proposal/ClaimChange structs; CapErrorCode::ConflictNotFound.
  conflict.rs   MOD  Conflict gains proposals: HashMap<String,Proposal>, opened_at_ms; ConflictStore
                     gains record_proposal, both_proposed, get_by_id, set_state, resolve_by_id,
                     timed_out(now, timeout_ms); open() gains opened_at_ms. compare() pure fn + Decision.
                     ConflictConfig struct (defaults + env).
  waitgraph.rs  NEW  WaitGraph: add_edge(from,to,resource), remove_agent(agent), find_cycle()
                     -> Option<Vec<WaitEdge>>. Pure, fully unit-testable.
  server.rs     MOD  ConflictConfig + WaitGraph in Shared; handle CONFLICT_PROPOSAL_SUBMITTED
                     (record -> on both: wait edges, deadlock check, compare, act) and
                     CONFLICT_USER_DECISION (resolve); pass opened_at_ms at open; reaper conflict-
                     timeout sweep; remove_agent from wait graph on resolve/abort/release.
  lib.rs        MOD  pub mod waitgraph;
```

`compare()`, `ProposalKind`, `waitgraph.rs`, and the `ConflictStore` methods are pure/no-IO and fully unit-testable. `server.rs` stays the only IO + state wiring site.

## Error Handling

- `CONFLICT_PROPOSAL_SUBMITTED` for an unknown/closed `conflictId`, or `from` not a participant → `ConflictNotFound`.
- `CONFLICT_USER_DECISION` for an unknown `conflictId` → `ConflictNotFound`; for any open conflict it resolves regardless of state (user override is always honored).
- Malformed proposal / bad `kind` → `SCHEMA_VALIDATION_FAILED` (deserialization).
- Wrong/missing `capVersion` → `UNSUPPORTED_CAP_VERSION` (existing gate).

## Testing

- Unit (`cap.rs`): decode `CONFLICT_PROPOSAL_SUBMITTED` (all proposal kinds round-trip), decode `CONFLICT_USER_DECISION`, reject bad kind, `ConflictNotFound.as_str()`.
- Unit (`conflict.rs`): `record_proposal` + `both_proposed`; `compare()` golden cases — one per rule branch (user-approval, ask-user, protected-path, step-aside, single-queue, split disjoint vs overlapping, co-own allowed vs disallowed, incompatible mixed); `timed_out` boundary; `open()` records `opened_at_ms`.
- Unit (`waitgraph.rs`): add_edge/remove_agent; `find_cycle` finds a 2-cycle, returns None for a DAG, returns None after `remove_agent` breaks it; direction matters.
- Unit (`server.rs`): both propose compatible → `CONFLICT_RESOLVED` + conflict removed; both propose incompatible → `CONFLICT_ESCALATED` + `USER_ARBITRATION_REQUIRED` + state `AwaitingUserDecision`; mutual `QUEUE_TASK` → `DEADLOCK_DETECTED` + escalate; `CONFLICT_USER_DECISION` resolves an escalated conflict; proposal for unknown conflict → `ConflictNotFound`; reaper timeout sweep escalates a stale single-proposal conflict.
- Integration (socket): two co-registered agents drive a high-overlap pair to `CONFLICT_OPENED`, each submits a proposal, observe `CONFLICT_RESOLVED` (compatible) or `USER_ARBITRATION_REQUIRED` then `CONFLICT_USER_DECISION` → `CONFLICT_RESOLVED` (escalated) in `events.log`.
- Coverage gate stays 90%; target ≥95%. Uncovered paths limited to fault-injection-only (IO errors), consistent with prior phases.

## Non-Negotiables Carried Forward

- Conflicts and their resolution decisions are deterministic; no LLM. Core is the only writer (CAP_SPEC §31).
- Lock discipline: never hold two of {state, heat, conflict, waitgraph, log} across a log append; snapshot under a short lock, compute pure, log in a separate scope.
- Same inputs → identical `compare()` decision and identical arbitration framing for both agents (§18.5).
