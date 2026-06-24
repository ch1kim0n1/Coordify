# Coordify Phase 5b — Stats, Profiles & Entertainment Design

**Status:** Approved (one phase; entertainment maxed; no emojis — color-coded facts)
**Date:** 2026-06-23
**Depends on:** Core Phases 1–5a + FILE_TOUCHED (merged); `absolute-docs/ARCHITECTURE.md` §7 (storage layout), §16 (live-state high-level structure), §18.3 (agent profiles), §27 Phase 5; the existing `events.log` (the recoverable event source) and `KnowledgeStore` (for the knowledge snapshot + atomic-write pattern).

## Goal

A reporting layer: at session finalize, derive session statistics, a session summary, heat history, per-agent profiles, coordination overhead, and a rich set of (deterministic) entertainment metrics. Pure reporting — it does NOT touch live heat or any hot path.

## Core architectural decision — derive at finalize from `events.log`

Events are already the recoverable source of truth (§"append-only event logs remain the recoverable source"). So 5b is built around a **pure `summarize(events: &[Value]) -> SessionStats`** plus a pure entertainment builder, invoked once at finalize against the session's `events.log`. No counters threaded through handlers, no new mutex on a hot path, no live-heat coupling. The whole aggregation is unit-testable by feeding a `Vec<serde_json::Value>` of events and asserting the reports.

## Scope (one phase)

In:
- **`stats.rs`** — pure `summarize(events)` producing session counts, peak heat, conflict outcomes, per-agent tallies, duration.
- **`entertainment.rs`** — pure builder over the same events + stats producing leaderboards, per-agent badges, drama/superlatives, streaks, and a plain-text narrative. **No emojis; facts are color-coded** via a named `color` field.
- **Cross-session profiles** — `agent-profiles.json`, `velocity-profiles.json`, `coordination-overhead.json`: load existing, merge this session's per-agent tallies (cumulative), write back with `.prev` rotation.
- **Per-session outputs** — `stats.json`, `session-summary.json` (§16 high-level rollup + narrative + knowledge snapshot), `heat-history.json` (the `HEAT_UPDATED` time series), `entertainment.json`.
- **`persist_stats` wiring** — called after `persist_knowledge` at both finalize sites; reads `events.log`, summarizes, writes all files atomically; best-effort.

Out (YAGNI): live periodic stats snapshots (finalize-only); handoff metrics (handoff unimplemented); wall-clock time-in-state durations (overhead is a participation count); a stats query API (the Phase 6 TUI reads these files).

## Data available in `events.log`

`AGENT_JOINED`, `CLAIM_CREATED`, `CLAIM_REJECTED`, `CLAIM_RELEASED{reason}`, `CLAIM_ORPHANED`, `HEAT_UPDATED{pair,heat,band,ts}`, `HEAT_THRESHOLD_EXCEEDED`, `PREDICTED_HEAT_CALCULATED`, `CONFLICT_OPENED{conflictId,agents,paths,trigger}`, `CONFLICT_PROPOSAL_RECEIVED{from,kind}`, `CONFLICT_RESOLVED{resolution}`, `CONFLICT_ESCALATED{reason}`, `USER_ARBITRATION_REQUIRED`, `CONFLICT_TIMEOUT`, `CONFLICT_ABORTED{reason}`, `DEADLOCK_DETECTED{agents}`, `AGENT_STATE_CHANGED`, `AGENT_LOST`, `AGENT_LEFT`, `FILE_TOUCHED{agentId,files}`, `CLEAR_INVOKED`, `KNOWLEDGE_QUARANTINED`. Every event carries `ts` (ISO). Aggregation keys on `type` and the relevant fields.

## Components

### `stats.rs` — `SessionStats` + `summarize`

`summarize(events: &[Value]) -> SessionStats` computes (all deterministic):
- **Session counts:** `agentsSeen` (distinct `AGENT_JOINED` agentIds), `claimsCreated`, `claimsReleased`, `claimsRejected`, `filesTouched` (distinct paths across `FILE_TOUCHED.files`), `heatUpdates`, conflicts by outcome — `conflictsOpened`, `autoResolvedHeatDropped`, `negotiatedResolved` (resolution ∈ {PARTICIPANT_STEPPED_ASIDE, QUEUED, SCOPE_SPLIT, CO_OWNERSHIP}), `userArbitrated`, `escalated`, `timedOut`, `aborted`, `deadlocks`, `arbitrationsRequested`.
- **`peakHeat`** = `{ heat, pair:[a,b], ts }` over all `HEAT_UPDATED` (max heat; tie → earliest ts).
- **`durationMs`** = last ts − first ts (parsed from ISO; 0 if <2 events).
- **Per-agent tally** `AgentTally { claimsMade, tasksCompleted (CLAIM_RELEASED reason=TASK_COMPLETED), ghostClaims (CLAIM_ORPHANED + CONFLICT_ABORTED involving agent), conflictsInvolved (CONFLICT_OPENED with agent in agents), heatGeneratedSum + heatGeneratedCount (for mean of HEAT_UPDATED.heat where agent ∈ pair), arbitrationsInvolved, deadlocksInvolved }`, keyed by agentId (BTreeMap → deterministic order).

`SessionStats` derives `Serialize`. A `to_summary(&self, knowledge_snapshot: Value) -> Value` builds the §16 `session-summary.json` shape `{ session, agents, claims, heat, conflicts, knowledgeSnapshot, narrative }`.

### `entertainment.rs` — color-coded, no emojis

`build_entertainment(events: &[Value], stats: &SessionStats) -> Entertainment`. A fixed **named color palette** (semantic, deterministic), rendered by consumers (Phase 6 TUI); JSON carries color *names*, never raw ANSI:

| color | meaning | used by |
|---|---|---|
| `red` | heat / danger | Firestarter, peakHeat, battleground |
| `cyan` | calm / cool | Coolest Operator, Pacifist |
| `blue` | cooperative | Diplomat |
| `yellow` | drama / caution | Conflict Magnet, deadlocks, arbitrations |
| `green` | success | Sprinter, Sniper, Speed Demon, completed streaks, auto-resolved |
| `gray` | neutral / absent | Ghost, Lone Wolf, quiet session |
| `magenta` | special / superlative | biggestSpike, longestNegotiation |

- **Leaderboards** — `Vec<Leaderboard { metric, color, entries: Vec<{agent, value}> }>` (entries sorted desc by value, ties by agent-id asc): mostClaims, mostTasksCompleted, mostFilesTouched, mostHeatGenerated (red), lowestAvgHeat (cyan), mostConflictsInvolved (yellow), mostAutoResolved (green).
- **Badges** — `Vec<Badge { id, label, color, agent }>` (plain-text labels, no emoji), awarded from event patterns: `Firestarter` (max total heat generated, red), `Diplomat` (most non-escalated conflict resolutions / YIELD+CO_OWN proposals, blue), `Ghost` (max ghostClaims, gray), `Hotzone Hero` (touched the most-contested file, red), `Lone Wolf` (zero conflictsInvolved, gray), `Conflict Magnet` (max conflictsInvolved, yellow), `Sprinter` (max tasksCompleted, green), `Pacifist` (involved in conflicts but never escalated, cyan), `Speed Demon` (shortest mean claim→release, green), `Sniper` (≥1 claim, all completed, zero ghost, green). A badge is emitted only when there is a non-trivial winner (e.g. Firestarter needs heatGeneratedSum > 0); ties → lowest agent-id.
- **Superlatives** — `{ key, value, color, ... }` list: `theBattleground` (most-contested file = max hotzone occurrences in CONFLICT_OPENED.paths + FILE_TOUCHED; red), `peakHeatMoment` (from stats.peakHeat; red), `biggestSpike` (largest heat delta between consecutive HEAT_UPDATED on the same pair; magenta), `mexicanStandoffs` (deadlock count; yellow), `courtCases` (arbitrationsRequested; yellow), `longestNegotiation` (max span CONFLICT_OPENED→matching CONFLICT_RESOLVED/ESCALATED by conflictId; magenta), `bloodiestMinute` (count of events in the busiest 60s window; magenta).
- **Streaks** — `{ longestAutoResolveStreak, longestCompletionStreak }` (max run of consecutive auto-resolved conflicts / consecutive non-ghost completed claims, in event order).
- **`narrative`** — a deterministic plain-text, multi-line recap built from the above (no emojis). Each highlighted fact is also present as a structured colored item, so a renderer can colorize; the narrative string itself is plain. Empty/quiet session → graceful defaults ("Quiet session — no conflicts, no drama."), never panics or empty-unwraps.

`Entertainment` derives `Serialize` → `entertainment.json`; `narrative` is also embedded in `session-summary.json`.

### Cross-session profiles — `ProfileStore`

`agent-profiles.json` accumulates per-agent cumulative totals across sessions. `ProfileStore { agents: BTreeMap<String, AgentProfile> }` where `AgentProfile { sessions, claimsMade, tasksCompleted, ghostClaims, conflictsInvolved, heatGeneratedSum, heatGeneratedCount, arbitrationsInvolved, deadlocksInvolved }`. `load(dir)`, `merge_session(&[ (agentId, AgentTally) ])` (adds totals + `sessions += 1`), `save_atomic(dir)` (`.prev` rotation). Derived views written alongside: `velocity-profiles.json` (`{agent: {tasksPerSession, meanHeatGenerated}}`) and `coordination-overhead.json` (`{agent: {conflictsInvolved, arbitrationsInvolved, deadlocksInvolved, overheadScore = conflicts + arbitrations + deadlocks}}`), both derived from the merged `ProfileStore`. Corrupt profile file → quarantine (reuse the knowledge quarantine pattern) + start empty.

## Wiring (`server.rs`)

`persist_stats(shared: &Shared, session: &Session, paths: &Paths)`:
1. Read the session's `events.log` to a `String`, parse each non-empty line as `Value` (skip parse failures) → `Vec<Value>`.
2. `let stats = stats::summarize(&events);`
3. `let ent = entertainment::build_entertainment(&events, &stats);`
4. knowledge snapshot for the summary: `let snap = { shared.knowledge.lock().unwrap().snapshot(shared.knowledge_k) };` (short lock, dropped) → serialize to a `Value`.
5. Write per-session `stats.json`, `session-summary.json` (`stats.to_summary(snap)` + narrative), `heat-history.json`, `entertainment.json` (atomic).
6. Load → `merge_session` → save `agent-profiles.json` + derived `velocity-profiles.json` + `coordination-overhead.json` (knowledge dir, `.prev`).
- Best-effort: any failure logs `STATS_PERSIST_FAILED { error, ts }`; never crashes finalize. Called immediately after `persist_knowledge(&shared, &paths)` in BOTH finalize branches (run-loop + reaper), inside the `finalized` CAS-won branch (exactly once).

**Lock discipline:** `persist_stats` runs at shutdown (network empty). It locks `knowledge` once (snapshot) in a closed scope, then does file IO; the `STATS_PERSIST_FAILED` log lock is a separate scope. Never two of `{state, heat, conflict, waitgraph, knowledge, log}` co-held.

## Module Design

```text
packages/coordify-core/src/
  stats.rs          NEW  SessionStats + AgentTally + summarize(events) + to_summary();
                         ProfileStore (load/merge_session/save_atomic) + derived velocity/
                         overhead. Pure aggregation fully unit-tested; IO thin.
  entertainment.rs  NEW  Entertainment + build_entertainment(events, stats); leaderboards,
                         badges, superlatives, streaks, narrative; named color palette.
                         Pure, fully unit-tested.
  server.rs         MOD  persist_stats(shared, session, paths) after persist_knowledge at
                         both finalize sites; reads events.log, summarizes, writes outputs.
  paths.rs          MOD  stats_file/summary_file/heat_history_file/entertainment_file
                         (session dir) + agent_profiles/velocity/overhead file paths (knowledge dir).
  lib.rs            MOD  pub mod stats; pub mod entertainment;
```

## Error Handling

- `events.log` missing/empty at finalize → summarize over `[]` → all-zero stats + "quiet session" entertainment; still writes the files. Never aborts finalize.
- Unparseable event line → skipped (the log is otherwise valid JSONL).
- Corrupt cross-session profile file → quarantined + empty start (knowledge pattern).
- Atomic-write failure → `STATS_PERSIST_FAILED` logged; `.prev` (last good) intact.
- ISO timestamp parse failure → that event contributes 0 to duration/windows; no panic.

## Testing

- Unit (`stats.rs`): `summarize` over a crafted event list asserts each count, conflict-outcome buckets, peakHeat (incl. tie→earliest), per-agent tallies, duration; empty events → zeroed stats. `ProfileStore` merge accumulates across two sessions, derived velocity/overhead correct, `.prev` rotation, corrupt → quarantine.
- Unit (`entertainment.rs`): each leaderboard ordering + tie-break (agent-id asc); each badge awarded to the right agent for crafted events and NOT awarded when trivial; superlatives (battleground, biggestSpike, longestNegotiation, bloodiestMinute); streaks; narrative is non-empty plain text with no emoji (assert it contains no chars > U+007E in the badge labels / palette is from the fixed set); quiet-session graceful defaults (no panic).
- Server/integration (socket): drive a session (two agents, overlapping claims, a conflict, file touches), finalize, assert `stats.json` / `session-summary.json` / `heat-history.json` / `entertainment.json` exist with expected fields, and `agent-profiles.json` has the agents with claim counts. Poll on the last-written file.
- Lock discipline preserved (suite would hang on a deadlock). Coverage gate 90% / target ≥95%; uncovered limited to IO-fault paths.

## Non-Negotiables

- Deterministic; no LLM, no randomness, no clock inside `summarize`/`build_entertainment` (timestamps come from events). Same events → identical reports (golden tests pin key values).
- No emojis anywhere in output; facts are highlighted via the named `color` field only.
- Core is the only writer; reports are derived from accepted events.
- Atomic writes with `.prev` rotation for cross-session profiles.
- Lock discipline: never hold two of `{state, heat, conflict, waitgraph, knowledge, log}` across a log append; `persist_stats` locks knowledge briefly, never nested.
- 5b is pure reporting — live heat, conflicts, and the hot path are unchanged (regression-safe by construction).
