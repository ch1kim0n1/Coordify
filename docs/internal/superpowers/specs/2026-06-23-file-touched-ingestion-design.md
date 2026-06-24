# Coordify — FILE_TOUCHED Ingestion Design

**Status:** Approved
**Date:** 2026-06-23
**Depends on:** Core Phases 1–5a (merged) — heat (`estimated_files` → `HeatInputs.files`), knowledge engine (coupling accrual), conflict lifecycle; the Node hook adapter (`packages/coordify-hook/`, which currently records `FILE_TOUCHED` locally instead of forwarding it). `absolute-docs/ARCHITECTURE.md` §6 (hook→CAP mapping), §18.2 (coupling sources), CAP_SPEC file-event notes.

## Goal

Let Core ingest the actual files an agent touches (`PostToolUse(Edit/Write)`), fold them into heat's file-overlap **and** the knowledge coupling graph, and flip the adapter's `FILE_TOUCHED` from recorded-only to forwarded. This turns the adapter's real-session data live: heat and conflicts begin reflecting what agents actually edit, not just declared `estimatedFiles` (empty from the adapter today), and coupling accrues from files touched together.

## Scope

In:
- **`FILE_TOUCHED` CAP event** ingested by Core: `{ agentId, files: [..] }`.
- **`actual_files` on the claim** — a per-claim observed-file set, distinct from `estimated_files`.
- **Heat uses the union** `estimated_files ∪ actual_files` for file-overlap.
- **Coupling accrual** from co-touched files (incremental, new × existing pairs).
- **Adapter flip**: `PostToolUse(Edit/Write/MultiEdit)` → forward `FILE_TOUCHED` (one-line mapping change).

Out:
- Separate `ACTUAL_FILES_UPDATED` event — `FILE_TOUCHED` covers it.
- Hotzone accrual on plain touched files — hotzone stays conflict-driven (see Decision 5).
- `FILE_READ` / `Bash` forwarding — stay recorded-only (reads/commands do not claim files).
- Per-file risky-write weighting — a future tunable.

## Design Decisions

1. **`FILE_TOUCHED { agentId, files: Vec<String> }`.** A list (the adapter sends one file per Edit/Write, but the event accepts a batch). Decoded via the existing serde `CapEvent` enum (deserialization is validation).

2. **`actual_files: BTreeSet<String>` on `Claim`**, separate from `estimated_files`. Keeps "declared at claim time" vs "observed during work" distinct and honest; `BTreeSet` gives dedup + deterministic order. `ClaimStore::record_touched(agent_id, files) -> Option<Vec<String>>` adds the files to the agent's live claim's `actual_files` and returns the subset that was **newly inserted** (empty Vec if all were already present; `None` if the agent has no live claim).

3. **Heat uses `estimated_files ∪ actual_files`.** `state::heat_inputs_for` builds `HeatInputs.files` from the union. The heat formula is unchanged; its file-overlap Jaccard now reflects real edits, so conflicts open on real contention and the 5a hotzone loop fires with real data.

4. **Coupling accrues incrementally on co-touch (§18.2).** After a `FILE_TOUCHED` adds new files, for each newly-inserted file `f` and each other file `g` already in the claim's `actual_files`, accrue +1 coupling for the unordered pair `(f, g)`. Only new files accrue (the set dedups), so re-touching a file never re-accrues — bounded and deterministic, and it captures "files touched together in the same task." Uses the existing `KnowledgeStore` coupling map.

5. **Hotzone stays conflict-driven (deliberate).** A plain touched file does NOT get hotzone +1. Hotzone means *contended* — a file raises its hotzone count only when it causes a conflict (now more likely, since overlap reflects real files). Auto-incrementing hotzone on every edit would make it an edit-frequency counter, not a risk signal. `// ponytail: hotzone = contention, not edit count; risky-write weighting is a future tunable.` (§18.1 lists "risky writes"; deferred as a knob.)

6. **Handler mirrors the claim path** (`server.rs`): under a short state lock, add files + capture the newly-inserted ones; `None` → `AGENT_NOT_FOUND`/`CLAIM_NOT_FOUND`; then `recompute_current_heat(agent)`; then accrue coupling under the knowledge lock alone; then log `FILE_TOUCHED`. Lock discipline unchanged — never two of `{state, heat, conflict, waitgraph, knowledge, log}` across a log append.

7. **Adapter flip.** In `coordify-hook/lib/mapping.js`, the `PostToolUse` branch for `Edit`/`Write`/`MultiEdit` returns `{ kind: 'forward', event: { type: 'FILE_TOUCHED', files: [file] } }` instead of `{ kind: 'record', ... }` — the file comes from `tool_input.file_path || tool_input.path`. The sidecar already injects `agentId` into forwarded events; no sidecar change. `FILE_READ` (Read) and `COMMAND_EXECUTED`/`TEST_RUN` (Bash) stay `record`.

## Event Shape

- Inbound CAP: `FILE_TOUCHED { agentId, files: [ "<path>", ... ] }`.
- Outbound log: `FILE_TOUCHED { agentId, files, newFiles, ts }` (echo with the newly-tracked subset, for observability).

## Module Design

```text
packages/coordify-core/src/
  cap.rs     MOD  CapEvent::FileTouched { agent_id, files } (camelCase agentId/files).
  claim.rs   MOD  Claim gains actual_files: BTreeSet<String>; ClaimStore::record_touched(
                  agent_id, &[String]) -> Option<Vec<String>> (new-files subset / None).
  state.rs   MOD  heat_inputs_for builds HeatInputs.files from estimated_files ∪ actual_files.
  server.rs  MOD  handle FILE_TOUCHED: record_touched -> recompute_current_heat -> coupling
                  accrual (new×existing pairs) -> log. Errors map to AGENT/CLAIM_NOT_FOUND.
packages/coordify-hook/
  lib/mapping.js  MOD  PostToolUse Edit/Write/MultiEdit -> forward FILE_TOUCHED.
  test/mapping.test.js  MOD  update the recorded-only assertions to expect forward.
```

`claim.rs` / `state.rs` changes are pure and unit-testable; `server.rs` stays the IO+state wiring site.

## Error Handling

- `FILE_TOUCHED` for an agent with no live claim → `CLAIM_NOT_FOUND` (the agent must have an active claim to attribute files to). Unknown agent → `AGENT_NOT_FOUND`.
- Empty `files` list → accepted, no-op (logs with `newFiles: []`).
- Malformed event / wrong `capVersion` → existing `SCHEMA_VALIDATION_FAILED` / `UNSUPPORTED_CAP_VERSION`.

## Testing

- Unit (`claim.rs`): `record_touched` adds files, dedups, returns only newly-inserted; `None` when the agent has no live claim.
- Unit (`state.rs`): `heat_inputs_for` returns the union of estimated and actual files.
- Unit (`server.rs`): two agents claim with disjoint `estimatedFiles` (low heat); one `FILE_TOUCHED` makes them share a file → recomputed heat rises into the overlap/conflict band; co-touched files accrue coupling (count ≥1); `FILE_TOUCHED` for an unknown/claimless agent errors.
- Unit (`mapping.js`, `node:test`): `PostToolUse` Edit/Write/MultiEdit now `{kind:'forward', event:{type:'FILE_TOUCHED', files:[...]}}`; Read still `FILE_READ` record; Bash still record.
- Integration (Core socket): two co-registered agents, each `FILE_TOUCHED` the same file → `HEAT_UPDATED` reflects the shared file / a conflict opens; `events.log` shows `FILE_TOUCHED`.
- Integration (adapter, `coordify-hook`): a `PostToolUse(Write)` hook now lands `FILE_TOUCHED` in Core's `events.log` (no longer only in the hooktrace); no `SCHEMA_VALIDATION_FAILED`.
- Lock discipline preserved (suite would hang on a deadlock). Coverage gate stays 90% / target ≥95% (Core); adapter suite all-green.

## Non-Negotiables Carried Forward

- Deterministic; no LLM. Core is the only writer.
- Lock discipline: never hold two of `{state, heat, conflict, waitgraph, knowledge, log}` across a log append.
- Adapter hooks stay emit-only and crash-safe (the flip changes only the mapping result, not the hook contract).
- Coupling accrues on transitions (new co-touch pairs), never per-recompute — no count inflation.
