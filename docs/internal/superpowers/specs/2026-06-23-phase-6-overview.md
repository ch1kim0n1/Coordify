# Phase 6 — TUI and Polish: Sub-Phase Decomposition

**Status:** Approved decomposition; specs follow  
**Date:** 2026-06-23  
**Depends on:** All Phases 0–5b merged and green

---

## Why Split

Phase 6 covers three independent deliverables that can be specced, planned, and merged separately without blocking each other:

| Sub-phase | Package | Primary concern |
|---|---|---|
| 6a | `coordify-cli` | All user-facing commands + live TUI + graph |
| 6b | `coordify-sim` | Scenario runner + replay |

`coordify-cli` has no dependency on `coordify-sim`. `coordify-sim` depends on `coordify-cli` being present (for IPC client utilities) but can be built in parallel.

---

## Sub-Phase 6a — `coordify-cli`

**One npm package** (`packages/coordify-cli`), TypeScript/Node.js.  
**Entry point:** `coordify` binary registered in `package.json#bin`.

### Commands in scope

| Command | Live (socket) | Offline (files) |
|---|---|---|
| `coordify status` | ✅ | ✅ |
| `coordify agents` | ✅ | ✅ |
| `coordify heat` | ✅ | ✅ |
| `coordify claims` | ✅ | ✅ |
| `coordify conflicts` | ✅ | ✅ |
| `coordify logs` | ✅ | ✅ |
| `coordify stats` | — | ✅ |
| `coordify session list` | — | ✅ |
| `coordify session inspect <id>` | — | ✅ |
| `coordify watch` | ✅ stream | polling fallback |
| `coordify graph --coupling\|--heat` | ✅ | ✅ |

### Key design decisions

- **Socket streaming when live, file polling fallback when offline.** CLI detects socket presence; if found, opens persistent connection and subscribes to event stream. If not found, reads JSON artifacts on disk.
- **`coordify watch`** rendered via **ink** (React for terminals). Live state panel: agents, states, claims, heat edges, active conflicts, session metrics. Refreshes on every incoming socket event (streaming) or every 500 ms (polling).
- **`coordify graph`** rendered via ink. `--coupling` shows file coupling edges (weight = co-touch count); `--heat` shows agent pair heat matrix. Both default to top-20 entries by weight/heat. Switchable with flags.
- **No mocks.** All data comes from real Core state or real finalized artifacts.

---

## Sub-Phase 6b — `coordify-sim`

**One npm package** (`packages/coordify-sim`), TypeScript/Node.js.  
Spawns real `coordify-core` binary processes; no fakes.

### Commands in scope

| Command | Description |
|---|---|
| `coordify simulate <script.json>` | Feed JSON scenario script as CAP events into a real running Core |
| `coordify replay <session-id>` | Visual playback (ink, no Core) OR state reconstruction (re-submit events to a live Core) |

### Key design decisions

- **Scenario scripts are JSON.** Schema: `{ "name": string, "steps": [{ "delay_ms": number, "event": CAP_EVENT }] }`. Steps are replayed in order with delays. Multiple agents supported by setting `agentId` per event.
- **`coordify simulate`** starts a fresh Core if none is running, registers agents from the script, then submits events in order. On completion, leaves Core running so `coordify watch` can observe the result.
- **`coordify replay`** has two modes:
  - `--visual` (default): reads `events.log` from the session artifact dir, renders each event through the ink TUI frame-by-frame with configurable speed (`--speed 1x|2x|0.5x`).
  - `--reconstruct`: re-submits events to a live Core to rebuild state; used for debugging.

---

## Build Order

1. Spec + plan + implement **6a** (`coordify-cli`) — no dependencies on 6b.
2. Spec + plan + implement **6b** (`coordify-sim`) — uses IPC client from 6a.

Both specs follow in separate documents.
