# Coordify Phase 5a — Knowledge Engine Design

**Status:** Approved (scope: knowledge engine first; stats/profiles/summaries → 5b)
**Date:** 2026-06-23
**Depends on:** Core Phases 1–4b (merged); `absolute-docs/ARCHITECTURE.md` §7 (storage layout), §18 (persistent knowledge), §27 Phase 5; the existing `heat::Knowledge` input type and the heat components `historicalHotzoneRisk` / `historicalCoupling` (Phase 3, currently always 0 because `Shared.knowledge` is an empty immutable `Knowledge`).

## Goal

Derive a hotzone map and a coupling graph from accepted events, accumulate them in memory (and across sessions via persisted counts), and feed them back into **live heat** so the two currently-dead heat components fire. This closes the Phase 3 loop: Core learns which files are risky (hotzones) and which files move together (coupling), and that knowledge raises heat on future overlapping claims.

## Scope

In:
- **`KnowledgeStore`** — integer counts for hotzones (per file) and coupling (per ordered file pair), accrued from accepted events.
- **Score derivation** — a deterministic saturating curve `score = n / (n + K)` turning counts into the `[0,1]` values `heat::Knowledge` consumes.
- **Live-heat feedback** — `Shared.knowledge` becomes a `Mutex<KnowledgeStore>`; `recompute_current_heat` snapshots it for the (pure) heat compute and updates counts from this round's events.
- **Cross-session persistence** — load `hotzones.json` / `coupling-graph.json` at startup; atomic-write them (with `.prev` rotation) at finalize.
- **Corrupt-file handling** — an unparseable knowledge file is quarantined; the store starts empty and logs.

Out (later / separate):
- Agent/velocity profiles, coordination overhead, `stats.json` / `session-summary.json` / `heat-history.json`, entertainment metrics → Phase 5b.
- `FILE_TOUCHED` ingestion (actual touched files) → separate adapter follow-up; the engine enriches automatically once it lands.
- Automated rebuild-from-events recovery (counts are rebuildable in principle; not automated here).
- Knowledge time-decay / staleness models.

## Design Decisions

1. **Two layers: counts (persisted) vs scores (consumed).** `KnowledgeStore` holds raw `u64` counts; `heat::Knowledge` holds `f64` scores in `[0,1]`. Counts persist and accumulate exactly across sessions; scores are derived on demand. This keeps accumulation lossless and the scoring curve swappable.

2. **Saturating score `n / (n + K)`, K = 5 default (env `COORDIFY_KNOWLEDGE_K`).** Deterministic, monotonic, bounded in `[0,1)`. n=0→0, n=5→0.5, n→∞→1. `// ponytail: saturating count score, not a tuned time-decay model; add decay if staleness matters.`

3. **Accrual sources (deterministic, bounded — accrue on transitions, never per recompute):**
   - **On a NEW conflict open** (`ConflictStore::open` returns `Some`): `record_conflict(paths)` → +1 hotzone per path **and** +1 coupling per unordered path-pair. Accruing only on the open transition (not on every `ConflictCandidate` recompute) keeps counts bounded and deterministic. The §18.1 "high heat" and "conflict" hotzone sources coincide here (a `ConflictCandidate` edge is exactly what opens a conflict), so they fold into this one accrual — no double counting.
   - **On `CLAIM_CREATED`**: `record_claim_files(estimated_files)` → +1 coupling per unordered file-pair within the claim (intra-claim coupling).
   - Risky-writes and actual touched files require `FILE_TOUCHED` (out of scope); with the adapter sending empty `estimatedFiles`, real-session signal is thin today, but conflict paths and Core/test clients exercise the engine fully.
   - Orphaned-claim hotzone accrual (§18.1) is **deferred**: the reaper's orphan path surfaces only `claimId`/`previousOwner`, not the claim's files, so accruing there would entangle the reaper with a claim-files-by-id lookup for a marginal source. Add when the reaper carries claim files.

4. **Live-heat feedback with one-event lag.** In `recompute_current_heat`: snapshot the store into a `Knowledge` under a short lock (`let k = { shared.knowledge.lock().unwrap().snapshot() };`), run `compute_heat(&mine, &other, &k, &cfg)` (pure; signature unchanged), then update counts from this round's conflicts/high-heat in a separate short lock scope. Heat therefore reads the pre-update knowledge — knowledge lags by exactly one event. This avoids an intra-compute feedback loop and stays deterministic.

5. **Lock discipline (new lock).** Locks are now `{state, heat, conflict, waitgraph, knowledge, log}`. The invariant is unchanged: never hold two across a log append. `knowledge` is locked alone and briefly — once to snapshot for compute, once to update — never nested with `heat`/`conflict`/`log`.

6. **Cross-session persistence (§18.4).** At Core startup, load `knowledge/hotzones.json` and `coupling-graph.json` into counts. A file that does not parse is quarantined (renamed into `knowledge/quarantine/`), the store starts that map empty, and a `KNOWLEDGE_QUARANTINED` event is logged. At finalize: rotate the existing canonical file to `.prev`, then atomic-write (temp file + rename) the current counts. Writes happen at both finalize sites (the run-loop path and the reaper path) via a single `persist_knowledge` helper called immediately before `finalize`.

7. **Persistence format (counts, not scores):**
   - `knowledge/hotzones.json`: object map `{ "<path>": <count>, ... }`.
   - `knowledge/coupling-graph.json`: array `[ { "a": "<path>", "b": "<path>", "count": <n> }, ... ]`, `a <= b` (ordered pair).

8. **`Shared.knowledge` type change.** Was `pub knowledge: Knowledge` (immutable, empty). Becomes `pub knowledge: Mutex<KnowledgeStore>`. The only consumer (`recompute_current_heat` / `predicted_heat`) switches from `&shared.knowledge` to a snapshotted `Knowledge`. `predicted_heat` also snapshots once at its start.

## Module Design

```text
packages/coordify-core/src/
  knowledge.rs  NEW  KnowledgeStore { hotzone_counts: HashMap<String,u64>,
                     coupling_counts: HashMap<(String,String),u64> }.
                     - record_conflict(paths) (+1 hotzone/path, +1 coupling/pair),
                       record_claim_files(files) (+1 coupling/pair).
                     - snapshot(k_const) -> heat::Knowledge  (score = n/(n+K)).
                     - load(dir) -> (KnowledgeStore, Vec<quarantined>)  (corrupt -> quarantine).
                     - save_atomic(dir)  (rotate .prev, temp+rename).
                     Pure count/score logic unit-tested; IO (load/save) thin + tested via tmp dir.
  heat.rs       —    Knowledge struct + accessors unchanged (input type).
  server.rs     MOD  Shared.knowledge: Mutex<KnowledgeStore>; KnowledgeConfig (K) ;
                     load at startup (run()); snapshot for compute in recompute_current_heat
                     and predicted_heat; update counts from conflicts/high-heat/claims/orphans;
                     persist_knowledge(shared, paths) before both finalize calls.
  session.rs    —    finalize() unchanged; knowledge persistence is a sibling step in server.
  paths.rs      MOD  knowledge_dir(), hotzones_file(), coupling_file(), knowledge_quarantine().
```

`knowledge.rs` count/score logic is pure and fully unit-testable. `server.rs` stays the only state+IO wiring point; the state-before-log lock discipline is preserved.

## Event Shapes

- `KNOWLEDGE_QUARANTINED { file, reason, ts }` — emitted at startup when a knowledge file fails to parse.
- (No new CAP ingest events; knowledge is derived from events Core already accepts.)

The canonical knowledge files (`hotzones.json`, `coupling-graph.json`) and their `.prev` are written at finalize, not streamed as events.

## Error Handling

- Corrupt knowledge file at load → move to `knowledge/quarantine/<name>.<ts>`, start that map empty, log `KNOWLEDGE_QUARANTINED`. Never abort startup.
- Atomic write failure at finalize → log to diagnostics; do not crash finalize (best-effort, mirrors existing finalize tolerance). The `.prev` (last good) remains intact because rotation precedes the temp write and rename is atomic.
- Missing knowledge files at startup → start empty (first run).

## Testing

- Unit (`knowledge.rs`): each accrual method increments the right counts (hotzone per file; coupling per unordered pair, deduped/symmetric); `snapshot` produces exact `n/(n+K)` values incl. n=0→0 and the K boundary; `save_atomic` then `load` round-trips counts; `.prev` rotation leaves the prior file recoverable; a corrupt file is quarantined and yields an empty map.
- Unit (`server.rs`): after a conflict opens on a file, the store's hotzone count for that file is ≥1, and a subsequent `compute_heat` for a pair sharing that file returns `historical_hotzone_risk > 0` (proves the live feedback loop end to end, in-process).
- Integration (socket): two agents propose overlapping claims (with `estimatedFiles`) → conflict opens → last agent leaves → finalize → assert `knowledge/hotzones.json` exists and contains the shared file with count ≥1, and `coupling-graph.json` contains the file pair. Poll on the last-written file to avoid mid-write snapshots.
- Lock discipline: the suite would hang on a deadlock; passing ≥4-thread runs is the evidence (carried-forward practice).
- Coverage gate stays 90%; target ≥95%. Uncovered limited to fault-injection IO paths (consistent with prior phases).

## Non-Negotiables Carried Forward

- Knowledge is deterministic and derived from accepted events; no LLM. Core is the only writer (CAP_SPEC §31).
- Lock discipline: never hold two of `{state, heat, conflict, waitgraph, knowledge, log}` across a log append; snapshot under a short lock, compute pure, persist/update in separate scopes.
- Same counts → identical scores (`n/(n+K)` pinned by golden tests).
- Knowledge files use atomic writes with `.prev` rotation (§"Knowledge indexes ... must use atomic writes").
