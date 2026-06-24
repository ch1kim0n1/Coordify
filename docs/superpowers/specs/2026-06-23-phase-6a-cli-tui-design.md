# Coordify Phase 6a — CLI + TUI Design

**Status:** Approved  
**Date:** 2026-06-23  
**Package:** `packages/coordify-cli` (TypeScript/Node.js)  
**Depends on:** Phases 0–5b; `coordify-core` binary on PATH or in `packages/coordify-core/target/`

---

## Goal

A single `coordify` binary (TypeScript/Node.js, one npm package) providing all user-facing read commands plus two live terminal views (`watch`, `graph`). Talks to Core over Unix socket when live; falls back to reading JSON artifacts on disk when no network exists.

---

## Architecture

```
coordify <command>
    │
    ├── IPC Client (src/ipc.ts)
    │     connect to .coordify/runtime/core.sock
    │     → streaming mode: subscribe to event stream
    │     → request/response mode: send query, read response
    │
    ├── File Reader (src/files.ts)
    │     read .coordify/state/*.json, sessions/<id>/*.json
    │     used when socket absent, or for session/stats commands
    │
    ├── Commands (src/commands/*.ts)
    │     one file per command group
    │     each resolves data via ipc or files, formats, prints
    │
    └── TUI (src/tui/*.tsx)
          ink components for `watch` and `graph`
          rendered only when command requires live view
```

**Socket detection:** On startup, CLI checks for `.coordify/runtime/core.sock`. If present and connectable → live mode. Otherwise → offline mode. Commands that require live state (`watch` streaming) degrade gracefully with a "no live network" message; static commands just read files.

---

## IPC Client (`src/ipc.ts`)

Wraps the Unix socket in two modes:

**Request/response** — send `{ kind: "request", id, token, action, ... }`, await single `{ kind: "response", ... }`. Used by static query commands.

**Event stream** — send `{ kind: "subscribe", token }`, then receive a continuous stream of `{ kind: "event", ... }` JSONL frames. Used by `watch` and `graph` in live mode.

**Token:** read from `.coordify/runtime/token` (0600). If file absent → offline mode.

---

## Commands

### `coordify status`

Single-screen summary: socket presence, agent count, active claims, peak heat pair, conflict count. Live: one request/response. Offline: read `stats.json` from latest session.

### `coordify agents`

Table: agentId | state | claims | heat score. Live: query Core state. Offline: read `agent-profiles.json`.

### `coordify heat`

Table: pair | heat | band | last-updated. Sorted by heat desc. Live: query Core heatstore. Offline: read `heat-history.json` from latest session (last entry per pair).

### `coordify claims`

Table: claimId | agentId | files | age. Live only (no offline; claims are ephemeral). Prints "no live network" when offline.

### `coordify conflicts`

Table: conflictId | agents | paths | state | age. Live only.

### `coordify logs [--tail N] [--follow]`

Print `events.log` JSONL lines, formatted as `[ts] TYPE field=value ...`. `--tail N` (default 20). `--follow` streams new lines (live only). Offline: reads file directly.

### `coordify stats`

Pretty-print `stats.json` from latest finalized session: agent counts, claim counts, conflict outcomes, peak heat, duration. Offline only (stats are finalized artifacts).

### `coordify session list`

Table of finalized sessions: id | start-ts | agents | duration | conflicts. Reads session dirs under `.coordify/sessions/`.

### `coordify session inspect <id>`

Prints `stats.json` + `session-summary.json` + `entertainment.json` (leaderboards, badges, narrative) for the given session id.

### `coordify watch`

Full-screen ink TUI. Refreshes on socket event (live) or every 500 ms (polling fallback).

**Layout (four panels):**

```
┌─ Agents ──────────────────┐ ┌─ Heat ──────────────────────┐
│ agent-1  ACTIVE  2 claims │ │ agent-1↔agent-2  82  CONFLICT│
│ agent-2  IDLE    0 claims │ │ agent-1↔agent-3  34  MONITOR │
└───────────────────────────┘ └─────────────────────────────┘
┌─ Conflicts ───────────────────────────────────────────────┐
│ conflict-1  agent-1,2  src/x.rs  NEGOTIATING  12s         │
└───────────────────────────────────────────────────────────┘
┌─ Session ─────────────────────────────────────────────────┐
│ Claims: 4  Resolved: 2  Escalated: 0  Peak heat: 82       │
└───────────────────────────────────────────────────────────┘
```

Colors use the Phase 5b named palette (red=heat/danger, yellow=conflict, green=resolved, gray=idle). No emoji. `q` or Ctrl-C to quit.

### `coordify graph --coupling | --heat`

Full-screen ink TUI, static snapshot (refreshes every 2s in live mode).

**`--coupling`:** Edge list sorted by co-touch count desc. Each row: `file-a ↔ file-b  count: N`. Top 20 by default; `--top N` overrides.

**`--heat`:** Agent pair matrix. Rows = agents (sorted by id), columns = agents, cell = heat value colored by band (red ≥ 80, yellow ≥ 50, green < 50, gray = no data). For N agents: N×N grid; works well up to ~10 agents before wrapping.

Both views: `q` or Ctrl-C to quit. `--json` flag dumps raw data as JSON and exits (no ink rendering).

---

## File Layout

```
packages/coordify-cli/
  package.json          bin: { coordify: "./dist/cli.js" }
  tsconfig.json
  src/
    cli.ts              entry point; parses argv, dispatches command
    ipc.ts              Unix socket client (request + stream)
    files.ts            JSON artifact reader (paths mirror paths.rs)
    commands/
      status.ts
      agents.ts
      heat.ts
      claims.ts
      conflicts.ts
      logs.ts
      stats.ts
      session.ts        list + inspect
    tui/
      watch.tsx         ink app for coordify watch
      graph.tsx         ink app for coordify graph
      components/
        AgentPanel.tsx
        HeatPanel.tsx
        ConflictPanel.tsx
        SessionPanel.tsx
        CouplingGraph.tsx
        HeatMatrix.tsx
  tests/
    ipc.test.ts         mock socket; request/response + stream
    files.test.ts       fixture JSON; path resolution
    commands/           one test file per command; fixture-driven
```

---

## Error Handling

- Socket connect fails → offline mode; commands that need live state print one-line warning.
- JSON artifact missing → command prints "no data" and exits 0.
- Malformed JSON artifact → print error, exit 1.
- `watch`/`graph` socket drops mid-stream → print "connection lost, switching to poll" and continue polling.

---

## Testing

- **IPC client:** mock Unix socket server; assert request/response + stream frame parsing.
- **File reader:** fixture JSON files; assert correct field extraction and path resolution.
- **Commands:** fixture-driven; for each command, provide a fixture (socket mock or JSON file), assert stdout output matches expected table/format.
- **TUI components:** ink `render()` snapshot tests for each panel with fixture data.
- No integration tests in this package; real Core integration is in `coordify-sim`.

---

## Non-Negotiables

- No emoji in output; colors via named palette only (consistent with Phase 5b).
- Offline mode never crashes; degrades to "no data" messages.
- `--json` flag on every command dumps raw data as JSON (machine-readable).
- `q` or Ctrl-C always exits cleanly (ink cleanup, socket close).
- Token read failure → offline mode, never crashes.
- Coverage gate: ≥ 90% lines on src/ipc.ts, src/files.ts, src/commands/*.
