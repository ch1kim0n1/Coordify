# Coordify Node Hook Adapter — v1 Design

**Status:** Approved (sidecar model; emit-only; full §6 hook coverage; Core untouched)
**Date:** 2026-06-23
**Depends on:** Coordify Core (Phases 1–4b, merged) and its socket protocol; Phase 0 hook validation (`phase-0/`, `phase-0/results/hook-matrix.md`); `absolute-docs/ARCHITECTURE.md` §6 (hook→CAP mapping), §7 (storage layout), §8 (bootstrap), §27 (the CLI/hook adapter is the missing live-client layer).

## Goal

Make Coordify Core usable from a live Claude Code session. A per-session **sidecar** owns the persistent Core socket connection, the agent identity, and the heartbeat. Ephemeral Claude Code hooks are thin clients that push their payloads to the sidecar over a per-session local socket; the sidecar translates hook payloads into CAP events and forwards the ones Core ingests, records the rest for forward-compat. v1 is **emit-only** — no hook ever blocks.

## Why a sidecar (the core constraint)

Claude Code hooks are ephemeral: each hook invocation is a fresh `node` process that exits immediately. Core, as built through Phase 4b, binds an agent to **one persistent socket connection**, **finalizes + exits when its last connection drops**, and **serializes connections** (handles one fully before accepting the next). A naive "each hook connects, sends, disconnects" would make Core treat the agent as having left and shut the daemon down after the first hook. The sidecar is the long-lived holder of that one connection, outliving individual hooks. Core is not modified.

## Scope

In:
- **Per-session sidecar** — one long-lived Node process per Claude `session_id`; owns the Core connection, registers once, heartbeats, listens for hook events on a per-session Unix socket.
- **7 hook clients** wired into `.claude/settings.json` (full §6 set): SessionStart, UserPromptSubmit, PreToolUse, PostToolUse, SubagentStart, SubagentStop, SessionEnd. Each is a thin emit-only client.
- **Translation layer** — pure hook-payload→CAP-event mapping + a coarse claim heuristic.
- **Bootstrap** per ARCHITECTURE §8: discover/own `core.sock`, acquire `core.lock` / spawn the `coordify-core` binary if absent, read `session.token`, register.
- **Installer** — writes the hook block into `.claude/settings.json` (evolves `phase-0/install.js`).

Out (v1):
- **Blocking / PreToolUse enforcement** — emit-only; deferred.
- **Actual-file→heat** — Core does not ingest `FILE_TOUCHED`/`ACTUAL_FILES_UPDATED` yet (Phase 3 deferred). The sidecar records them; wiring them into heat is a small Core change, the recommended immediate follow-up.
- **Concurrent agents inside one Core** — Core's serialized accept loop stands. Multiple *sessions* each get their own sidecar + connection (used serially); true concurrency is the deferred Core work.
- **Config file**, the `coordify watch` TUI (Phase 6).

## The mapping (honest split vs Core's current decode set)

Core's `decode_event` ingests exactly four CAP events today — `CLAIM_PROPOSED`, `CLAIM_RELEASED`, `AGENT_STATE_CHANGED`, `CLEAR_INVOKED` — plus the `register` and `heartbeat` actions. The §6 table lists more events Core does not decode yet. So the sidecar splits hooks into **forwarded** (state-changing in Core) and **recorded-only** (written to a sidecar-local NDJSON trace for forward-compat, never sent to Core so Core never returns `SCHEMA_VALIDATION_FAILED`).

| Claude hook | Maps to | v1 fate |
|---|---|---|
| `SessionStart` source=`startup`/`resume` | ensure sidecar exists; **register happens once at sidecar startup** (Core logs `AGENT_JOINED`) | **sidecar bootstrap** (the hook event itself forwards nothing; if the sidecar is already up it is a no-op) |
| `SessionStart` source=`clear` | `CLEAR_INVOKED` | **forwarded** (re-uses the existing agent; Core bumps generation) |
| `UserPromptSubmit` | `CLAIM_PROPOSED` (heuristic claim) | **forwarded** → Core logs `CLAIM_CREATED` |
| `SubagentStart` | `AGENT_STATE_CHANGED` → `SUBAGENT_WAITING` | **forwarded** |
| `SubagentStop` | `AGENT_STATE_CHANGED` → `ACTIVE` | **forwarded** |
| `SessionEnd` | `CLAIM_RELEASED`(reason `SESSION_END`) for live claims, then disconnect | **forwarded** → Core logs `AGENT_LEFT` |
| `PreToolUse` | `TOOL_PRECHECK` / `RISKY_WRITE_CHECKED` | **recorded-only** |
| `PostToolUse`(Edit/Write) | `FILE_TOUCHED` / `ACTUAL_FILES_UPDATED` | **recorded-only** |
| `PostToolUse`(Read) | `FILE_READ` | **recorded-only** |
| `PostToolUse`(Bash) | `COMMAND_EXECUTED` (`TEST_RUN` if a test command) | **recorded-only** |

When Core later ingests the recorded-only events, each flips to forwarded with a one-line change in `mapping.js` (a `forward: true` flag per mapping entry).

Notes on the forwarded set:
- `AGENT_STATE_CHANGED` must respect Core's state machine. SubagentStart from `ACTIVE` → `SUBAGENT_WAITING` is legal; SubagentStop → `ACTIVE`. If the agent is not in a state that legally transitions, Core returns `INVALID_STATE_TRANSITION`; the sidecar logs and ignores it (best-effort, never throws into the hook).
- The sidecar tracks live claim ids (from `CLAIM_CREATED` responses) so SessionEnd can release them. If none, SessionEnd just disconnects.

## Claim derivation (ponytail heuristic)

`CLAIM_PROPOSED` requires `intent` (enum) and `confidence`; `UserPromptSubmit` carries only the raw prompt. v1 derives:
- `task.summary` = prompt text, trimmed and truncated to 200 chars.
- `intent` = first keyword match (case-insensitive) on the prompt: `fix|bug` → `BUGFIX`; `test` → `TESTING`; `doc` → `DOCUMENTATION`; `refactor` → `REFACTOR`; `perf|optimi` → `PERFORMANCE`; `secur` → `SECURITY`; else `FEATURE`.
- `domains` = `[]`, `estimatedFiles` = `[]` (no reliable signal pre-tool-use).
- `confidence` = `0.7` (fixed; above Core's low-confidence reject threshold).

Marked `// ponytail: coarse claim; real intent/domain/file extraction is a later enrichment.`

## Known limitation (stated plainly)

Emit-only + empty `estimatedFiles` + Core not ingesting actual files ⇒ pairwise **heat stays near-inert** in v1 (only intent/temporal/branch components can fire), so conflicts rarely open. v1's deliverable is the **integration backbone** — sidecar lifecycle + the event pipe + agent lifecycle end-to-end. The highest-value immediate follow-up is a small Core change to ingest `FILE_TOUCHED` and fold actual files into heat inputs, which turns the recorded-only PostToolUse data into live coordination.

## Sidecar lifecycle

1. **SessionStart hook** ensures the sidecar exists: check for the per-session socket / a sidecar pidfile; if absent, spawn `node sidecar.js --session <id> --root <cwd>` **detached** (`spawn` with `detached:true, stdio:'ignore'`, `unref()`), then forward the SessionStart event as usual.
2. **Sidecar startup:** Core discovery (§8) — if `core.sock` is missing or dead, acquire `core.lock` / spawn the `coordify-core` binary and poll for the socket (bounded retry); read `session.token`; open the persistent connection; `register` with `meta` carrying `branch` (from `git rev-parse --abbrev-ref HEAD`, best-effort) and `session_id`; store `agent_id`; bind the per-session listener socket; start the heartbeat interval.
3. **Per hook event:** sidecar reads one JSON line `{hook, payload}`, maps it, forwards or records, replies with a tiny `{ok:true}` ack (hooks ignore it — emit-only). A mapping or forward error is logged to the sidecar's diagnostics, never propagated.
4. **Heartbeat:** `action:heartbeat` to Core every 3s (configurable via env), independent of hook traffic, so the reaper does not reap the agent during idle gaps.
5. **SessionEnd:** release live claims (`CLAIM_RELEASED` reason `SESSION_END`), close the Core connection (Core logs `AGENT_LEFT`, finalizes if last), remove the per-session socket, exit.

## Components / files

`packages/coordify-hook/` — plain Node (CommonJS like `phase-0/`), **zero runtime dependencies** (`net`, `child_process`, `fs`, `path`, `crypto` only):

```text
packages/coordify-hook/
  sidecar.js            Daemon: Core bootstrap+discovery, register, heartbeat,
                        per-session listener, forward/record dispatch, SessionEnd shutdown.
  lib/
    core-client.js      Core socket framing: newline-JSON request/response, token auth,
                        request-id correlation, register/heartbeat/submit_event helpers.
    mapping.js          PURE: mapEvent(hook, payload) -> {forward:bool, request?} ; claim heuristic.
    paths.js            Mirror of Core's path layout (socket/token/lock + per-session sock + hooktrace).
    sidecar-client.js   Thin connect-write-one-line-exit used by every hook.
  hooks/
    session-start.js  user-prompt-submit.js  pre-tool-use.js  post-tool-use.js
    subagent-start.js subagent-stop.js  session-end.js
  install.js            Write the 7-hook block into .claude/settings.json (backup first).
  test/
    mapping.test.js     Unit: every hook payload -> expected CAP/recorded (node:test).
    integration.test.js Spawn real coordify-core + sidecar; drive hooks; assert events.log.
```

`mapping.js` is pure and the heart of the unit tests. `core-client.js` and `sidecar-client.js` isolate all socket IO. `sidecar.js` is the only stateful long-lived piece.

## Error handling

- Hooks **never throw and always exit 0** (emit-only; a hook failure must never break the user's Claude session). Connect failures to the sidecar are swallowed (best-effort) — the worst case is a missed event, not a broken session.
- Sidecar Core-bootstrap failure (cannot spawn/connect within the retry budget): the sidecar logs to its diagnostics file and exits; hooks then no-op (sidecar absent → connect fails → swallowed). Claude is unaffected.
- Core returning a CAP error (`INVALID_STATE_TRANSITION`, `AGENT_NOT_FOUND`, etc.): sidecar logs and continues. No retries in v1.
- Malformed hook stdin: hook parses defensively, exits 0 (mirrors `phase-0/` discipline).

## Testing

- **Unit (`mapping.test.js`, `node:test`):** `mapEvent` for each of the 7 hooks → exact `{forward, request}`; the claim heuristic for each intent keyword + the default; `SessionStart` source split (`startup` vs `clear`); Bash test-command → `TEST_RUN`. Pure, no IO.
- **Integration (`integration.test.js`):** spawn the real `coordify-core` binary (built via `cargo build`; the test resolves the binary path, skips with a clear message if it is not built) into a temp root; start the sidecar; fire a scripted hook sequence (SessionStart → UserPromptSubmit → SubagentStart → SubagentStop → SessionEnd) through the per-session socket; poll the session `events.log` and assert it contains `AGENT_JOINED`, `CLAIM_CREATED`, `AGENT_STATE_CHANGED`, `CLAIM_RELEASED`, `AGENT_LEFT`. Verify the recorded-only trace file gets a PostToolUse entry and Core's log does **not** (no `SCHEMA_VALIDATION_FAILED`). Poll on the last-written marker to avoid mid-write snapshot races (lesson carried from Core's integration tests).
- A `package.json` with `"test": "node --test"` and `"private": true`; no dependencies.

## Non-negotiables

- Core is not modified by this phase.
- Hooks are emit-only and crash-safe: never block, never throw, always exit 0.
- `mapping.js` is pure and deterministic (same payload → same CAP event); the keyword heuristic is fixed-order and case-insensitive.
- No new runtime dependencies (Node stdlib only).
- Recorded-only events are never sent to Core (no rejected-event noise); flipping one to forwarded is a one-line `mapping.js` change once Core ingests it.
