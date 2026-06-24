# Coordify — ARCHITECTURE.md

**Status:** Draft v0.1  
**Date:** 2026-06-22  
**Depends on:** `VISION.md`, `CAP_SPEC.md`, `TECHNICAL_VALIDATION.md`

---

## 1. Architecture Summary

Coordify is a local, CLI-first coordination layer for Claude Code terminal sessions working in the same project root.

The architecture has four main components:

1. **Coordify Core**  
   Mandatory local runtime and source of truth.

2. **Coordify Hooks**  
   Claude Code hook adapter that observes prompts, tool calls, session lifecycle, `/clear`, and subagent lifecycle.

3. **Coordify CLI/TUI**  
   User-facing command surface for status, heat, stats, logs, simulation, and session review.

4. **Coordify Storage**  
   Runtime files, append-only logs, session artifacts, and persistent project intelligence.

---

## 2. Recommended Technology Stack

### 2.1 Coordify Core

**Recommended language:** Rust

Reasons:

- single static binary;
- low runtime overhead;
- strong filesystem and socket primitives;
- safe concurrency;
- reliable file locking;
- good cross-platform support;
- good fit for local daemon behavior;
- good fit for atomic writes and append-only logs;
- robust schema validation options.

Coordify Core is infrastructure. It should be boring, fast, and hard to crash.

### 2.2 Coordify CLI and Hook Adapter

**Recommended language:** TypeScript / Node.js

Reasons:

- easy distribution through npm-style workflows;
- familiar to Claude Code/plugin users;
- fast contributor onboarding;
- simple shell integration;
- good JSON/CLI ergonomics;
- easy wrapper around Rust binary.

### 2.3 Schemas

**Recommended format:** JSON Schema

Used for:

- CAP event validation;
- claim validation;
- configuration validation;
- stats shape;
- knowledge files;
- simulation fixtures.

### 2.4 IPC

MVP IPC:

- Unix domain socket on macOS/Linux;
- Windows named pipe on Windows;
- JSONL or framed JSON messages;
- session-scoped auth token;
- request/response + event stream.

### 2.5 Storage

Use:

- append-only JSONL logs for events/traces;
- derived JSON indexes for knowledge;
- atomic write/rename for indexes;
- compressed session artifacts after finalization.

---

## 3. Repository Layout

Recommended monorepo layout:

```text
coordify/
  packages/
    coordify-core/             # Rust local runtime
    coordify-cli/              # TypeScript CLI wrapper
    coordify-hooks/            # Claude Code hook adapter
    coordify-schemas/          # JSON Schemas for CAP/config/storage
    coordify-tui/              # Optional terminal UI
    coordify-sim/              # Simulation runner / fixture utilities

  docs/
    VISION.md
    ARCHITECTURE.md
    CAP_SPEC.md
    TECHNICAL_VALIDATION.md
    POST-MVP.md

  fixtures/
    simple-conflict.json
    deadlock.json
    clear-reset.json
    orphaned-claim.json
    hotzone-learning.json
    core-recovery.json

  examples/
    basic-claude-code/
    auth-conflict-demo/
    handoff-demo/
```

---

## 4. Component Responsibilities

## 4.1 Coordify Core

Coordify Core is mandatory.

It is **not** an orchestrator and does **not** control agents as workers. It is the local source of truth for validated coordination state.

Responsibilities:

- network lifecycle;
- agent registration;
- heartbeat tracking;
- CAP event validation;
- CAP event ingestion;
- live state registry;
- claim lifecycle;
- orphaned claim tombstones;
- heat calculation;
- heat history;
- conflict lifecycle;
- deadlock detection;
- handoff routing;
- schema validation;
- trust/auth handshake;
- startup locking;
- atomic storage;
- session finalization;
- degraded mode;
- project knowledge updates;
- statistics aggregation.

Core should never execute arbitrary agent code.

Core should never ask an LLM to decide heat.

Core should never accept unvalidated state mutations.

---

## 4.2 Coordify Hooks

Coordify Hooks connect Claude Code to Coordify Core.

Responsibilities:

- start or connect to Core on Claude session start;
- register the agent;
- submit heartbeat;
- observe user prompts;
- inject network context into Claude when appropriate;
- ask Claude to produce CAP claim schema before work;
- intercept tool use before risky actions;
- log tool results after execution;
- detect `/clear`;
- detect session end;
- track subagent lifecycle;
- route CAP events to Core;
- receive Core responses and convert them into Claude-readable context.

The hook adapter should be thin. It should not calculate heat or own canonical state.

---

## 4.3 Coordify CLI

The CLI is the user's control surface.

Required commands:

```bash
coordify status
coordify agents
coordify heat
coordify claims
coordify conflicts
coordify logs
coordify stats
coordify graph
coordify watch
coordify session list
coordify session inspect
coordify simulate
coordify replay
```

CLI talks to Core when a live network exists.

CLI reads finalized session artifacts when no network exists.

---

## 4.4 Coordify TUI

MVP can include terminal rendering, not a web dashboard.

Optional commands:

```bash
coordify watch
coordify graph
```

Shows:

- agents;
- states;
- tasks;
- ownership claims;
- heat edges;
- current conflicts;
- handoffs;
- session metrics.

TUI is ephemeral. It dies with the live network. Historical data remains inspectable.

---

## 5. Claude Code Hook Integration

Coordify relies on Claude Code hooks. Phase 0 must validate behavior locally before Core implementation.

Required hook categories:

| Hook | Coordify Use |
|---|---|
| `SessionStart` | agent registration, context injection, `/clear` restart handling |
| `UserPromptSubmit` | classify prompt, inject network context, request CAP claim |
| `PreToolUse` | check heat/claims before read/write/bash/tool execution |
| `PostToolUse` | log successful actions, update actual files, update stats |
| `SubagentStart` | mark agent as `SUBAGENT_WAITING` |
| `SubagentStop` | return from `SUBAGENT_WAITING` when appropriate |
| `SessionEnd` | release/close agent, detect `/clear`, finalize clean exits |
| `CwdChanged` | validate network root membership |
| `Notification` / `Stop` | optional state refinement and user waiting detection |

References for validation:
- https://docs.anthropic.com/en/docs/claude-code/hooks
- https://docs.anthropic.com/en/docs/claude-code/hooks-guide

---

## 6. Hook-to-CAP Event Mapping

| Claude Hook | CAP Events |
|---|---|
| `SessionStart(startup)` | `AGENT_JOINED`, `SESSION_STARTED` |
| `SessionStart(clear)` | `CLEAR_INVOKED`, `AGENT_GENERATION_INCREMENTED` |
| `UserPromptSubmit` | `USER_PROMPT_OBSERVED`, `TASK_DECLARED`, `CLAIM_PROPOSED` |
| `PreToolUse(Read)` | `TOOL_PRECHECK`, possible `HEAT_CHECKED` |
| `PreToolUse(Edit/Write)` | `RISKY_WRITE_CHECKED`, possible `COORDINATION_REQUIRED` |
| `PostToolUse(Read)` | `FILE_READ` |
| `PostToolUse(Edit/Write)` | `FILE_TOUCHED`, `ACTUAL_FILES_UPDATED` |
| `PostToolUse(Bash)` | `COMMAND_EXECUTED`, possible `TEST_RUN` |
| `SubagentStart` | `SUBAGENT_STARTED` |
| `SubagentStop` | `SUBAGENT_STOPPED` |
| `SessionEnd` | `AGENT_LEFT`, `SESSION_END_OBSERVED` |
| heartbeat timeout | `AGENT_LOST`, `CLAIM_ORPHANED` |

---

## 7. Runtime Storage Layout

Per project root:

```text
.coordify/
  config/
    coordify.yaml

  runtime/
    core.sock
    core.lock
    session.token
    core.pid
    live-state.json
    heartbeat/
      agent-abc.json

  sessions/
    2026-06-22_18-42-11/
      events.log
      diagnostics.log
      trace.log
      stats.json
      heat-history.json
      network-final.json
      session-summary.json
      compressed/
        events.log.zst
        diagnostics.log.zst
        trace.log.zst

  knowledge/
    hotzones.json
    hotzones.json.prev
    coupling-graph.json
    coupling-graph.json.prev
    agent-profiles.json
    velocity-profiles.json
    coordination-overhead.json
    quarantine/

  schemas/
    cap-event.schema.json
    claim.schema.json
    config.schema.json
```

Runtime files are ephemeral.

Session and knowledge files persist.

---

## 8. Network Bootstrap

Coordify must prevent startup races.

### 8.1 Startup Flow

1. Hook starts inside Claude Code session.
2. Hook discovers project root.
3. Hook checks for `.coordify/runtime/core.sock`.
4. If socket responds, hook connects.
5. If no socket, hook tries to acquire `.coordify/runtime/core.lock`.
6. Lock winner starts Coordify Core.
7. Lock loser waits and retries socket.
8. If lock is stale, validate PID/heartbeat before breaking it.
9. Core creates session token.
10. Hook performs CAP handshake.
11. Agent registers.

### 8.2 Lock Requirements

Lock must include:

```json
{
  "pid": 18432,
  "startedAt": "2026-06-22T18:42:00Z",
  "projectRoot": "/abs/path/project",
  "coreVersion": "0.1.0"
}
```

Do not trust lock blindly. Verify PID and socket.

---

## 9. IPC Protocol

Use framed JSON over local IPC.

### 9.1 Message Types

- request;
- response;
- event;
- stream update;
- heartbeat;
- error.

Example request:

```json
{
  "kind": "request",
  "id": "req-123",
  "token": "session-token",
  "action": "submit_event",
  "event": {
    "type": "CLAIM_PROPOSED",
    "agentId": "agent-a",
    "payload": {}
  }
}
```

### 9.2 Auth

Every IPC message must include the session token after handshake.

Token is generated by Core and stored with restrictive permissions.

Unix permissions:

- `.coordify/runtime`: `0700`
- token file: `0600`
- socket: user-only access where possible

Windows equivalent: current-user ACL.

---

## 10. Trust Model

Coordify is local-first, not hostile-machine secure.

It protects against:

- malformed events;
- accidental corruption;
- casual spoofing;
- non-registered local processes;
- bad schema data;
- direct poisoning of live state through the public API.

It does not promise protection from:

- malicious same-user malware;
- developer manually editing `.coordify`;
- compromised dependency with full filesystem access.

Security principles:

- only Core mutates canonical state;
- hook adapter cannot bypass schema validation;
- CAP token required for IPC;
- knowledge is derived from accepted events;
- invalid knowledge is quarantined;
- all state mutations are logged.

---

## 11. Configuration Surface

Config file:

```text
.coordify/config/coordify.yaml
```

Also allow root-level convenience config:

```text
coordify.yaml
```

If both exist, define precedence in implementation spec.

Example:

```yaml
heat:
  safeMax: 25
  monitorMax: 50
  overlapMax: 75
  conflictMin: 76
  debounceMs: 500

claims:
  orphanTtlSeconds: 300
  lowConfidenceRejectBelow: 0.45
  provisionalBelow: 0.75

escalation:
  defaultMode: coordinate
  strictProtectedPaths:
    - "schema.prisma"
    - "src/auth/**"
    - "infra/**"

logging:
  traceLevel: verbose
  rotateSizeMb: 1024
  compressOnSessionEnd: true
  compression: zstd

knowledge:
  enabled: true
  hotzoneWeight: 0.10
  couplingWeight: 0.10

core:
  heartbeatIntervalMs: 2000
  heartbeatTimeoutMs: 10000
  degradedMode: fail-safe
```

Hardcoded defaults are allowed only as defaults.

---

## 12. State Model

Coordify Core owns live state.

High-level structure:

```json
{
  "session": {},
  "agents": {},
  "claims": {},
  "heat": {},
  "conflicts": {},
  "handoffs": {},
  "waitGraph": {},
  "knowledgeSnapshot": {}
}
```

Live state is periodically snapshotted to:

```text
.coordify/runtime/live-state.json
```

This snapshot is not the primary source of truth. It helps recovery.

Append-only event logs remain the recoverable source.

---

## 13. Atomic Storage

Knowledge indexes and final summaries must use atomic writes.

Pattern:

1. serialize new data to memory;
2. validate against schema;
3. write `file.tmp`;
4. fsync temp file;
5. rename old current to `.prev`;
6. rename temp to current;
7. fsync parent directory where supported;
8. on startup, validate current;
9. if corrupt, load `.prev`;
10. if both invalid, quarantine and rebuild from events.

Knowledge files are derived. They can be rebuilt from events if needed.

---

## 14. Logging Architecture

### 14.1 Event Log

`events.log`

Append-only JSONL CAP events.

### 14.2 Diagnostics Log

`diagnostics.log`

Core/hook/IPC/system errors.

### 14.3 Trace Log

`trace.log`

Maximum-detail activity:

- hook fired;
- CAP event submitted;
- precheck result;
- file read;
- file write;
- bash command;
- test run;
- subagent start/stop;
- conflict lifecycle;
- handoff lifecycle;
- degraded mode.

Trace should not remove detail. Use rotation/compression instead.

---

## 15. Session Finalization

When last agent exits:

1. mark all live agents offline;
2. release or close claims;
3. finalize orphan state where needed;
4. write network-final;
5. write stats;
6. write heat-history;
7. update knowledge;
8. compress logs;
9. mark session closed;
10. remove runtime files.

If Core crashes before finalization, next startup detects unfinalized session and finalizes/rebuilds.

---

## 16. Graceful Core Degradation

Core is a single local source of truth. Failure behavior must be explicit.

### 16.1 Hook Cannot Reach Core

Hook should:

1. retry connection briefly;
2. check if Core process exists;
3. if lock stale, try restart;
4. if restart succeeds, reconnect;
5. if not, enter degraded behavior.

### 16.2 Degraded Modes

| Mode | Behavior |
|---|---|
| Observe | Continue and log warning. |
| Warn | Continue but warn agent/user. |
| Coordinate | Warn before risky write. |
| Ask User | Ask before uncoordinated risky write. |
| Strict | Block protected writes until Core recovers. |

Default should be fail-safe for risky writes.

### 16.3 Recovery

On recovery:

- reload snapshot;
- replay events since snapshot;
- validate live claims;
- recalculate heat;
- mark `CORE_RECOVERED`;
- log recovery event.

---

## 17. Heat Calculation Architecture

### 17.1 Inputs

- task;
- intent;
- domains;
- estimated files;
- actual files;
- recent file operations;
- branch/worktree;
- hotzone map;
- coupling graph;
- claim conflicts;
- temporal overlap;
- agent states.

### 17.2 Incremental Recalculation

On event from Agent A, recalculate only edges involving Agent A.

For `n = 50`, this means 49 edges, not 1,225.

### 17.3 Debouncing

High-frequency events should be debounced:

- file reads: coalesce aggressively;
- file writes: lower debounce;
- claim changes: immediate;
- conflict events: immediate;
- heartbeat: no heat recalculation unless state changes.

### 17.4 Cache

Cache:

- glob/path overlap;
- hotzone lookups;
- coupling lookups;
- branch/worktree context;
- domain path mappings.

---

## 18. Persistent Knowledge Architecture

### 18.1 Hotzone Map

Updated from:

- high heat events;
- conflicts;
- repeated file overlap;
- risky writes;
- user arbitration;
- deadlocks;
- orphaned claims.

### 18.2 Coupling Graph

Updated from:

- files touched in same task;
- files touched in same handoff;
- files touched near same conflict;
- files repeatedly modified together across sessions.

### 18.3 Agent Profiles

Updated from:

- velocity metrics;
- task completion;
- ghost work;
- average heat generated;
- average heat resolved;
- coordination overhead.

### 18.4 Knowledge Update Timing

During session:

- update in memory;
- optionally write periodic snapshots.

At finalization:

- atomically write canonical knowledge files.

---

## 19. Claim Lifecycle

Claim states:

- `PROPOSED`
- `PROVISIONAL`
- `ACTIVE`
- `SHARED`
- `RELEASED`
- `ORPHANED`
- `RECLAIMABLE`
- `TRANSFERRED`
- `EXPIRED`

Low confidence claims may become provisional instead of active.

Orphaned claims occur after unclean agent disappearance.

---

## 20. Conflict Architecture

Conflict is opened when:

- heat crosses threshold;
- same file/same intent overlap occurs;
- protected path violation occurs;
- deadlock detected;
- agent requests coordination;
- Core detects high-risk claim.

Conflict lifecycle:

```text
DETECTED
NEGOTIATING
AWAITING_AGENT_RESPONSE
AWAITING_USER_DECISION
RESOLVED
TIMEOUT
ABORTED
```

Core never lets agents free-form negotiate as raw chat. They exchange structured CAP proposals.

---

## 21. Handoff Architecture

Handoff must be a structured package.

Core validates:

- sender owns or participates in task;
- receiver exists;
- receiver state allows handoff;
- files/claims referenced are real;
- sender release/receiver claim rules are valid;
- heat recalculates after acceptance.

Handoff does not silently reassign work. Receiver must accept unless configured otherwise.

---

## 22. Simulation Architecture

CAP must be testable without Claude Code.

Simulation adapter emits the same CAP events hooks would emit.

Simulation use cases:

```bash
coordify simulate fixtures/simple-conflict.json
coordify simulate fixtures/deadlock.json
coordify replay .coordify/sessions/2026-06-22_18-42-11/events.log
```

Simulation should test:

- agent join;
- prompt submit;
- claim;
- heat;
- conflict;
- handoff;
- clear;
- crash;
- orphan;
- deadlock;
- core restart.

This is mandatory for CI.

---

## 23. Technical Validation Gate

Before implementing Core:

1. create minimal hook scripts;
2. test actual Claude Code hook behavior;
3. verify blocking behavior;
4. verify `/clear`;
5. verify subagent lifecycle;
6. verify prompt context injection;
7. record results in `TECHNICAL_VALIDATION.md`.

If validation fails, update architecture before writing Core.

---

## 24. Cross-Platform Notes

MVP should aim for macOS/Linux first, but architecture should not prevent Windows.

### Unix/macOS

- Unix domain sockets;
- POSIX file locks;
- chmod permissions;
- fsync/rename atomicity.

### Windows

- named pipes;
- file locking via Win32 APIs or Rust crates;
- ACLs;
- rename semantics differ and must be tested.

If Windows support is deferred, say so explicitly in release scope.

---

## 25. Performance Goals

MVP should handle:

- 1–10 agents comfortably;
- 20 agents without degraded UX;
- 50 agents as an upper stress target.

Targets:

- claim validation: < 50ms;
- heat update for changed agent: < 100ms at 20 agents;
- PreToolUse risky-write decision: fast enough not to annoy user;
- session finalization: bounded, can happen async after final agent exits;
- log append: non-blocking or very low latency.

---

## 26. Failure Modes

Coordify must explicitly handle:

- Core startup race;
- stale lock;
- stale socket;
- Core crash;
- hook crash;
- malformed CAP event;
- invalid schema;
- corrupt knowledge file;
- partial write;
- agent hard kill;
- `/clear`;
- subagent state mismatch;
- deadlock;
- conflict timeout;
- handoff receiver disappears;
- session finalization interrupted.

Every failure should produce a diagnostic log entry.

---

## 27. Implementation Phases

### Phase 0 — Technical Validation

Prove hook assumptions.

### Phase 1 — Core Skeleton

- Core binary;
- socket/named pipe;
- lock;
- agent registration;
- heartbeat;
- session lifecycle;
- event log.

### Phase 2 — CAP Foundation

- schemas;
- event ingestion;
- agent states;
- claim lifecycle;
- `/clear`;
- orphaned claims.

### Phase 3 — Heat

- deterministic heat;
- predicted/current heat;
- heat history;
- incremental updates;
- branch/worktree awareness.

### Phase 4 — Conflict Handling

- heat thresholds;
- negotiation state machine;
- escalation levels;
- deadlock detection;
- user arbitration.

### Phase 5 — Knowledge and Stats

- hotzones;
- coupling graph;
- velocity profiles;
- coordination overhead;
- entertainment metrics;
- session summaries.

### Phase 6 — TUI and Polish

- `coordify watch`;
- graph rendering;
- richer stats display;
- docs/examples.

---

## 28. Architectural Non-Negotiables

- Coordify Core is the only writer of canonical live state.
- CAP events must be schema-validated.
- Heat is deterministic.
- Live network state dies when the last agent exits.
- Historical session artifacts are review-only.
- Project knowledge persists.
- `/clear` resets agent ownership.
- Crashed agents produce orphaned claim tombstones.
- CAP must be testable without Claude Code.
- Core failure must be visible and safe.
