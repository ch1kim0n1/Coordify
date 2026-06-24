# Coordify Phase 3 — Heat Design

**Status:** Approved (architecture decided; weights/bands fixed by spec)
**Date:** 2026-06-23
**Depends on:** Phase 2 (merged), `absolute-docs/CAP_SPEC.md` §15/§16/§19, `absolute-docs/ARCHITECTURE.md` §11/§17/§27 Phase 3, README heat weights.

## Goal

Add deterministic pairwise heat scoring to Core: predicted heat (before a claim is accepted) and current heat (after claims change), incrementally recomputed per changed agent, with bands and threshold escalation.

## Scope (ARCHITECTURE §27 Phase 3)

In:
- **Deterministic heat** — pure function of 8 components with fixed weights; same inputs → same output, no LLM (CAP_SPEC principle 4).
- **Predicted heat** — on `CLAIM_PROPOSED`, score the proposed claim against existing active claims before creating it (`PREDICTED_HEAT_CALCULATED` + recommendation).
- **Current heat** — after `CLAIM_CREATED`/`CLAIM_RELEASED`/`CLEAR_INVOKED`, recompute edges involving the changed agent (`HEAT_UPDATED`, `HEAT_THRESHOLD_EXCEEDED`).
- **Incremental updates** — recompute only the n−1 edges touching the changed agent, never the full n² (CAP_SPEC §17.2).
- **Heat history** — the sequence of `HEAT_UPDATED` events in `events.log` (the recoverable source). A separate persistent `heat-history.json` is Phase 5.
- **Branch/worktree awareness** — agent register metadata may carry `branch`; the branch/worktree component uses it.

Out (later phases):
- Persistent knowledge: hotzone map + coupling graph (Phase 5). In Phase 3 these inputs are empty, so `historicalHotzoneRisk` and `historicalCoupling` always score 0 — the formula is complete, the inputs grow later.
- Conflict objects, negotiation, deadlock, user arbitration (Phase 4) — Phase 3 only emits `HEAT_THRESHOLD_EXCEEDED`; opening a conflict is Phase 4.
- Actual-file tracking (`FILE_TOUCHED`/`ACTUAL_FILES_UPDATED`) — Phase 3 file overlap uses a claim's `estimatedFiles`; actual files are added when tool events are ingested.
- Heat debouncing/caching (§17.3/§17.4) — correctness first; optimization later.
- Config-file tuning — hardcoded defaults + the existing env-override pattern.

## Design Decisions

1. **Total heat function, growing inputs.** `compute_heat` takes the full input set for both agents plus a `Knowledge` reference. Missing inputs (empty hotzone/coupling, `None` branch) score their component 0. This lets Phase 3 ship the complete CAP_SPEC §15 formula; Phases 4–5 only populate inputs.

2. **Fixed weights (README, sum = 1.0):** task 0.10, intent 0.15, domain 0.15, filePath 0.20, temporal 0.10, branchWorktree 0.10, hotzone 0.10, coupling 0.10. `heat = round(100 · Σ wᵢ·rawᵢ)`, integer 0..100.

3. **Component formulas (raw ∈ [0,1], deterministic):**
   - `taskSimilarity` = Jaccard of task-summary token sets (lowercased alphanumeric words).
   - `intentSimilarity` = 1.0 if same intent else 0.0.
   - `domainOverlap` = Jaccard of domain sets.
   - `filePathOverlap` = Jaccard of estimated-file path sets.
   - `temporalActivity` = `1 − min(1, |lastSeenA − lastSeenB| / windowMs)`, window default 60_000 ms.
   - `branchWorktreeProximity` = 1.0 if both branches present and equal, else 0.0.
   - `historicalHotzoneRisk` = max hotzone risk over shared files (empty knowledge → 0).
   - `historicalCoupling` = max coupling score over cross pairs of the two file sets (empty → 0).

4. **Bands (ARCHITECTURE §11):** `≤25 SAFE`, `26–50 MONITOR`, `51–75 OVERLAP`, `≥76 CONFLICT_CANDIDATE`. Recommendation: SAFE→`PROCEED`, MONITOR→`MONITOR`, OVERLAP→`SPLIT_SCOPE_OR_SEQUENCE`, CONFLICT_CANDIDATE→`NEGOTIATE_BEFORE_CLAIM`.

5. **Threshold escalation (CAP_SPEC §19):** emit `HEAT_THRESHOLD_EXCEEDED` when `heat > 50` (OVERLAP or above). OVERLAP → level 2 / `COORDINATE_BEFORE_WRITE`; CONFLICT_CANDIDATE → level 3 / `ASK_USER`. (Opening a conflict object is Phase 4.)

6. **Incremental edges.** A `HeatStore` keeps pairwise edges keyed by an ordered `(agentId, agentId)`. On a change to agent X, recompute only edges (X, Y) for each other agent Y holding a live claim. Released/cleared agents have their edges dropped.

7. **Heat inputs derive from an agent's live claim.** Only agents with a live (ACTIVE or PROVISIONAL) claim participate in heat. An agent with no claim has no edges.

## Module Design

```text
packages/coordify-core/src/
  heat.rs    NEW  HeatConfig (weights, thresholds, window), HeatBand, HeatComponents,
                  HeatResult, Knowledge (empty in P3), HeatInputs, compute_heat(),
                  band_for()/recommendation_for(), tokens(), jaccard().
  heatstore.rs NEW HeatStore: ordered-pair edge map; upsert_edge, edges_for(agent),
                  remove_agent, recompute helpers.
  state.rs   MOD  Agent gains `branch: Option<String>` (from register meta); helper to
                  build HeatInputs for an agent from its live claim.
  server.rs  MOD  predicted heat on CLAIM_PROPOSED; current-heat recompute on
                  CLAIM_CREATED/RELEASED/CLEAR; HEAT_UPDATED / PREDICTED_HEAT_CALCULATED /
                  HEAT_THRESHOLD_EXCEEDED events; HeatStore in Shared.
  lib.rs     MOD  pub mod heat; pub mod heatstore;
```

`heat.rs` and `heatstore.rs` are pure (no IO), fully unit-testable. `server.rs` stays the only IO+state wiring point — the state-before-log lock discipline is preserved; the `HeatStore` lives in `Shared` behind its own `Mutex`, locked in short scopes like `state`/`log`, never nested.

## Event Shapes (CAP_SPEC §15/§16/§19)

- `HEAT_UPDATED { pair:[a,b], heat, heatKind:"CURRENT", band, components{8}, reasons[] }`
- `PREDICTED_HEAT_CALCULATED { agentId, edges:[{pair, heat, band, reasons}], recommendation }`
- `HEAT_THRESHOLD_EXCEEDED { pair:[a,b], heat, escalationLevel, requiredAction }`

## Testing

- Unit: each component fn on crafted inputs (exact raw values); `compute_heat` golden cases (e.g. same-intent + shared-file → known integer heat); band boundaries (25/26, 50/51, 75/76); empty-knowledge → hotzone/coupling 0; HeatStore ordered-key dedup + remove_agent.
- Integration (socket): two agents propose overlapping claims → predicted heat in response + `PREDICTED_HEAT_CALCULATED` logged; after both created → `HEAT_UPDATED` logged with a band; a high-overlap pair → `HEAT_THRESHOLD_EXCEEDED`; release/clear drops edges.
- Concurrency lock: HeatStore never held while log locked. Coverage gate stays 90%; target ≥95%.

Note: integration tests need two agents' claims to coexist. Phase 2's serialized accept loop blocks a second open connection, but heat only needs both claims to EXIST — a test can register+propose for agent A, let A disconnect cleanly (its claim persists; clean disconnect does not release claims), then connect agent B and propose, observing heat A↔B. (Clean-disconnect claim persistence is current behavior; it is not orphaned, so the edge is valid for the test.)

## Non-Negotiables Carried Forward

- Heat is deterministic; no agent computes canonical heat (CAP_SPEC §31). Core is the only writer.
- Same inputs → identical integer heat (golden tests pin this).
