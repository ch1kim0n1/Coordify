# Phase 0 Technical Validation — Design Spec

**Date:** 2026-06-22
**Status:** Approved
**Scope:** Prove Claude Code hook assumptions before Coordify Core implementation begins.

---

## Goal

Validate 7 Claude Code hook assumptions using real hook scripts installed into this project's `.claude/settings.json`. No Coordify Core work begins until all assumptions are classified as PASS, PARTIAL, or FAIL with raw evidence.

---

## Assumptions Under Test

| ID | Assumption |
|----|-----------|
| H1 | `PreToolUse` fires before file mutation |
| H2 | `PreToolUse` can block a write by returning exit code 1 + JSON |
| H3 | `UserPromptSubmit` can inject context into Claude's input |
| H4 | `/clear` produces a detectable `SessionStart` hook event with `reason: clear` |
| H5 | `SubagentStart` / `SubagentStop` fire at subagent boundaries |
| H6 | Clean session exit vs hard crash are distinguishable via `SessionEnd` / heartbeat timeout |
| H7 | Hook latency is acceptable (PreToolUse blocking under 100ms p99) |

---

## Folder Structure

```
phase-0/
  hooks/
    logger.js              # shared: payload capture, latency, JSONL output
    session-start.js       # H4: /clear detection, startup classification
    user-prompt-submit.js  # H3: context injection
    pre-tool-use.js        # H1, H2, H7: intercept + sentinel blocking + timing
    post-tool-use.js       # supporting: file read/write/bash logging
    subagent-start.js      # H5: subagent start granularity
    subagent-stop.js       # H5: subagent stop granularity
    session-end.js         # H6: clean exit detection
  results/
    payloads/              # raw JSON per hook invocation
    latency.jsonl          # { hook, startedAt, durationMs } per call
    hook-matrix.md         # generated pass/fail table
  sentinel/
    BLOCK_TARGET           # PreToolUse blocks writes to this path only
  install.js               # writes hook entries to .claude/settings.json
  report.js                # reads results/, writes hook-matrix.md
```

---

## Component Design

### `logger.js`

Shared module. All hook scripts require it.

Responsibilities:
- Accept hook name + raw payload
- Record `startedAt` timestamp on entry, `durationMs` on exit
- Write raw payload as JSONL to `results/payloads/<hook>-<iso-timestamp>.jsonl`
- Append latency record to `results/latency.jsonl`
- Never throw — Phase 0 scripts must not crash Claude Code

Exports:
```js
logger.capture(hookName, payload)   // call at hook entry
logger.finish(hookName, startedAt)  // call before exit, records latency
```

---

### `pre-tool-use.js`

Most critical script. Validates H1, H2, H7.

Logic:
1. Call `logger.capture('PreToolUse', payload)` — records raw payload and start time
2. Check if `payload.tool_input.path` matches `phase-0/sentinel/BLOCK_TARGET`
3. If match: write block response to stdout, exit 1 — proves H2
4. If no match: exit 0 — passes through, proves H1 without disrupting work
5. Call `logger.finish(...)` before either exit — records latency for H7

Block response format (stdout on exit 1):
```json
{
  "decision": "block",
  "reason": "Coordify Phase 0: sentinel path blocked for PreToolUse validation"
}
```

---

### `session-start.js`

Validates H4.

Reads `payload.hook_event_name` and `payload.session_info.init_hook_args.type` (or equivalent field indicating clear vs startup). Logs the raw payload so the field structure can be confirmed manually.

---

### `user-prompt-submit.js`

Validates H3.

Injects a small static context string into Claude's prompt context by writing to stdout:
```json
{
  "context": "[Coordify Phase 0] Hook injection active."
}
```

Logs whether injection was accepted (exit 0) and captures the full payload for field analysis.

---

### `subagent-start.js` / `subagent-stop.js`

Validates H5.

Both scripts log full payload and timestamp. Analysis confirms whether events fire once per subagent boundary, and whether they carry enough identity to match start/stop pairs.

---

### `session-end.js`

Validates H6.

Logs payload with timestamp. Compared against heartbeat timeout behavior (simulated by hard-killing Claude Code process) to determine if the two cases are distinguishable.

---

### `post-tool-use.js`

Supporting validation. Not a primary assumption, but captures file read/write/bash results for context on how PostToolUse payloads are structured.

---

### `install.js`

Writes hook configuration to `.claude/settings.json` in this repo:

```json
{
  "hooks": {
    "SessionStart":       [{ "command": "node phase-0/hooks/session-start.js" }],
    "UserPromptSubmit":   [{ "command": "node phase-0/hooks/user-prompt-submit.js" }],
    "PreToolUse":         [{ "command": "node phase-0/hooks/pre-tool-use.js" }],
    "PostToolUse":        [{ "command": "node phase-0/hooks/post-tool-use.js" }],
    "SubagentStart":      [{ "command": "node phase-0/hooks/subagent-start.js" }],
    "SubagentStop":       [{ "command": "node phase-0/hooks/subagent-stop.js" }],
    "SessionEnd":         [{ "command": "node phase-0/hooks/session-end.js" }]
  }
}
```

Backs up existing `.claude/settings.json` before writing.

---

### `report.js`

Reads all files under `results/payloads/` and `results/latency.jsonl`. Generates `results/hook-matrix.md` with:
- One row per assumption (H1–H7)
- Status: PASS / PARTIAL / FAIL / PENDING
- Evidence: payload file reference + key field observed
- p50/p95/p99 latency from `latency.jsonl`

---

## Validation Procedure

1. Run `node phase-0/install.js` to install hooks
2. Open Claude Code in this repo: `claude`
3. Submit a few prompts — captures `UserPromptSubmit` payloads (H3)
4. Ask Claude to read a file — captures `PreToolUse` read payload (H1)
5. Ask Claude to write to `phase-0/sentinel/BLOCK_TARGET` — confirms blocking (H2)
6. Ask Claude to write to any other file — confirms pass-through (H1)
7. Run `/clear` — captures `SessionStart` with clear reason (H4)
8. Trigger a subagent (e.g., ask Claude to use Agent tool) — captures H5
9. Close Claude Code cleanly — captures `SessionEnd` (H6)
10. Hard kill the Claude Code process — observe missing `SessionEnd` for H6 comparison
11. Run `node phase-0/report.js` to generate `results/hook-matrix.md`

---

## Output

`results/hook-matrix.md` classifies each assumption:
- `PASS` — confirmed with raw payload evidence
- `PARTIAL` — fires but payload structure differs from assumption
- `FAIL` — does not fire or cannot achieve the required behavior
- `PENDING` — not yet tested

If any assumption is FAIL, architecture must be revised before Core implementation.

---

## Constraints

- Hook scripts must never throw unhandled exceptions
- `PreToolUse` blocking only applies to `phase-0/sentinel/BLOCK_TARGET`
- No network calls from hook scripts
- Results folder is gitignored (raw payloads may contain sensitive prompt content)
- `install.js` backs up existing settings before overwriting

---

## Success Criteria

All 7 assumptions reach PASS or PARTIAL with documented evidence. Any PARTIAL result includes a specific note on what differs and whether it affects Coordify Core design.
