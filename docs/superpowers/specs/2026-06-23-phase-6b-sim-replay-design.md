# Coordify Phase 6b — Sim & Replay Design

**Status:** Approved  
**Date:** 2026-06-23  
**Package:** `packages/coordify-sim` (TypeScript/Node.js)  
**Depends on:** Phase 6a (`coordify-cli` IPC client + file reader); `coordify-core` binary

---

## Goal

Two commands that drive real `coordify-core` instances with real events — no mocks, no fakes:

- `coordify simulate <script.json>` — replay a JSON scenario script as live CAP events into a real Core
- `coordify replay <session-id>` — visual playback or state reconstruction from a past session's `events.log`

---

## Architecture

```
coordify-sim
    │
    ├── ScenarioRunner (src/runner.ts)
    │     parse scenario JSON, start Core if needed,
    │     register agents, submit events in order with delays
    │
    ├── Replayer (src/replayer.ts)
    │     read events.log from session artifact dir,
    │     drive visual playback (ink) OR re-submit to live Core
    │
    ├── CoreManager (src/core-manager.ts)
    │     spawn / detect / stop coordify-core process,
    │     wait for socket ready, obtain token
    │
    └── IPC Client
          re-exported from coordify-cli/src/ipc.ts
```

**Real Core only.** `CoreManager` either detects an already-running Core (socket present) or spawns a new one from the binary path. No in-process Core simulation.

---

## Scenario Script Format

JSON file. Schema:

```json
{
  "name": "two-agent-conflict",
  "agents": ["agent-a", "agent-b"],
  "steps": [
    { "delay_ms": 0,    "event": { "type": "AGENT_JOINED", "agentId": "agent-a" } },
    { "delay_ms": 100,  "event": { "type": "CLAIM_CREATED", "agentId": "agent-a", "claimId": "c-1", "files": ["src/x.rs"] } },
    { "delay_ms": 200,  "event": { "type": "AGENT_JOINED", "agentId": "agent-b" } },
    { "delay_ms": 300,  "event": { "type": "CLAIM_CREATED", "agentId": "agent-b", "claimId": "c-2", "files": ["src/x.rs"] } }
  ],
  "finalize": true
}
```

**Fields:**

| Field | Type | Description |
|---|---|---|
| `name` | string | Human label; printed during run |
| `agents` | string[] | Agent IDs to register before steps begin |
| `steps` | Step[] | Ordered list of events with delays |
| `steps[].delay_ms` | number | Wait before submitting this event (0 = immediate) |
| `steps[].event` | CAP event | Any valid CAP event object (must include `type`) |
| `finalize` | boolean | If true, send AGENT_LEFT for all agents after last step |

Events are submitted via the IPC socket (`submit_event` action). The script's `agentId` fields determine which agent token is used per event — each agent gets its own session token from Core registration.

**Validation:** Script is validated against JSON Schema before execution. Invalid script → print errors, exit 1.

---

## `coordify simulate <script.json>`

```
$ coordify simulate scenarios/two-agent-conflict.json

Running: two-agent-conflict
  Starting coordify-core...   ✓ (pid 12345)
  Registering agents...       ✓ agent-a, agent-b
  Step 1/4  AGENT_JOINED      agent-a          0ms
  Step 2/4  CLAIM_CREATED     agent-a  c-1     100ms
  Step 3/4  AGENT_JOINED      agent-b          200ms
  Step 4/4  CLAIM_CREATED     agent-b  c-2     300ms
  Finalizing...               ✓
Done. Use `coordify watch` or `coordify stats` to inspect results.
```

**Behavior:**

1. Parse + validate script.
2. `CoreManager.ensure()` — if socket present, use existing Core; otherwise spawn binary, wait for socket (timeout 5s).
3. Register each agent in `agents[]` via `AGENT_JOINED` IPC (obtain per-agent token).
4. Submit steps in order; `setTimeout(delay_ms)` between each.
5. If `finalize: true`, send `AGENT_LEFT` for all agents.
6. Print summary line; exit 0.

**Flags:**

| Flag | Description |
|---|---|
| `--dry-run` | Parse + validate script, print steps, do not connect to Core |
| `--no-finalize` | Override `finalize: true` in script; leave Core running |
| `--core-bin <path>` | Path to `coordify-core` binary (default: auto-detect) |

---

## `coordify replay <session-id>`

Two modes selected by flag:

### `--visual` (default)

Reads `events.log` from `.coordify/sessions/<id>/events.log`. Renders each event through the ink TUI (same panels as `coordify watch`) with configurable playback speed. No Core required.

```
$ coordify replay 2026-06-23-abc --visual --speed 2x

Replaying session 2026-06-23-abc (47 events, ~12s compressed to ~6s)
[ink watch panels animate as events play]
[q] quit  [space] pause  [←→] rewind/forward 10 events
```

**Controls:** `q` quit, space pause/resume, `←` back 10 events, `→` forward 10 events, `+`/`-` adjust speed.

**Speed:** `--speed 0.5x|1x|2x|4x` (default `1x`). Delays between events are scaled accordingly; minimum 50ms between frames regardless of speed.

### `--reconstruct`

Re-submits events from `events.log` to a live Core in order, rebuilding session state. Useful for debugging: pauses at a specific event with `--stop-at <event-index>` and leaves Core running for inspection.

```
$ coordify replay 2026-06-23-abc --reconstruct --stop-at 23

Reconstructing session 2026-06-23-abc...
  Starting coordify-core...  ✓
  Submitting event 1/47...
  ...
  Submitting event 23/47...  [stopped at --stop-at]
Core is running. Use `coordify watch` to inspect state.
```

**Flags:**

| Flag | Description |
|---|---|
| `--visual` | Visual playback (default) |
| `--reconstruct` | State reconstruction mode |
| `--speed <Nx>` | Playback speed multiplier (visual only) |
| `--stop-at <N>` | Stop after N events (reconstruct only) |
| `--core-bin <path>` | Binary path (reconstruct only) |

---

## `CoreManager` (`src/core-manager.ts`)

```typescript
interface CoreManager {
  ensure(): Promise<{ pid: number; socketPath: string; token: string }>;
  stop(): Promise<void>;
}
```

**`ensure()`:**
1. Check for existing socket + valid PID in `.coordify/runtime/core.lock`.
2. If alive → return existing socket + read token.
3. If not alive → spawn `coordify-core` with `--session-dir .coordify`, wait up to 5s for socket to appear, read token.
4. Throws if binary not found or socket never appears.

**`stop()`:** send `SIGTERM` to the spawned PID; wait for socket to disappear (up to 3s).

CoreManager only stops a Core it spawned — never stops a pre-existing one.

---

## File Layout

```
packages/coordify-sim/
  package.json
  tsconfig.json
  src/
    cli.ts              entry point; dispatches simulate | replay
    runner.ts           ScenarioRunner
    replayer.ts         Replayer (visual + reconstruct)
    core-manager.ts     CoreManager
    schema.ts           JSON Schema validation for scenario scripts
    tui/
      replay-watch.tsx  ink TUI for visual replay (reuses watch panels)
  scenarios/
    two-agent-conflict.json    example scenario
    deadlock-three-agents.json example scenario
  tests/
    runner.test.ts      mock IPC; assert event submission order + timing
    replayer.test.ts    fixture events.log; assert visual frame sequence
    core-manager.test.ts spawn real binary; assert socket appears
    schema.test.ts      valid + invalid scripts; assert validation output
```

---

## Error Handling

- Script validation fails → print each schema error, exit 1.
- Core binary not found → clear message with install hint, exit 1.
- Core fails to start within 5s → error + exit 1.
- IPC submission fails mid-script → print failed step, exit 1 (partial runs are logged).
- `events.log` missing → "session not found", exit 1.
- `--reconstruct` with no live Core + spawn fails → error, exit 1.

---

## Testing

- **ScenarioRunner:** mock IPC socket; assert events submitted in order with correct delays.
- **Schema validation:** valid + invalid fixtures; assert error messages.
- **Replayer visual:** fixture `events.log`; assert ink render sequence (snapshot).
- **CoreManager:** spawn real `coordify-core` binary; assert socket appears within 5s, token readable. (Integration test, runs only when binary present.)
- No end-to-end sim tests in this package; those live in a future `coordify-sim/e2e/` suite.

---

## Non-Negotiables

- No mocks of Core internals — `CoreManager` spawns the real binary.
- `--dry-run` on simulate never connects to Core or socket.
- CoreManager never stops a pre-existing Core.
- Visual replay never requires a live Core.
- `--json` flag on both commands dumps raw event list / replay metadata and exits.
- Coverage gate: ≥ 90% lines on `src/runner.ts`, `src/replayer.ts`, `src/schema.ts`.
