# Coordify — TECHNICAL_VALIDATION.md

**Status:** Phase 0 Complete  
**Date:** 2026-06-23  
**Purpose:** Prove the Claude Code integration assumptions before building Coordify Core.

---

## Phase 0 Results

**Validation date:** 2026-06-23  
**OS:** macOS Darwin 24.6.0  
**Node version:** v25.2.0  
**Claude Code model:** claude-sonnet-4-6  
**Repo path:** /Users/pomoika/Documents/GitHub_repo/Coordify

### Hook Validation Matrix

| ID | Assumption | Status | Evidence |
|----|-----------|--------|----------|
| H1 | `PreToolUse` fires before file mutation | **PASS** | 96 payloads captured across multiple real sessions |
| H2 | `PreToolUse` can block writes via exit code 1 | **PASS** | 8 Write tool calls to sentinel path intercepted, hook exits 1 on match |
| H3 | `UserPromptSubmit` can inject context via stdout | **FAIL** | Hook fires. `{"context":"..."}` stdout key not consumed by Claude Code. Confirmed via live session test — no injected text visible to model. |
| H4 | `/clear` produces detectable `SessionStart` with `source: clear` | **PASS** | Field confirmed: `payload.source` = `"startup"` on launch, `"clear"` after `/clear` |
| H5 | `SubagentStart` / `SubagentStop` fire at subagent boundaries | **PASS** | Both hooks fired in real session |
| H6 | Clean exit produces `SessionEnd`; hard crash does not | **PASS** | 3 clean exits captured with `SessionEnd`; crash = no SessionEnd (architectural guarantee) |
| H7 | PreToolUse latency p99 < 100ms | **PASS** | p50=3ms, p95=25ms, p99=30ms across 34 real samples |

### Key Field Discoveries

| Hook | Key Fields |
|------|-----------|
| `SessionStart` | `session_id`, `cwd`, `source` (`startup` or `clear`), `model` |
| `UserPromptSubmit` | `session_id`, `prompt`, `cwd`, `permission_mode` |
| `PreToolUse` | `session_id`, `tool_name`, `tool_input` (path via `path` or `file_path`), `tool_use_id` |
| `PostToolUse` | `session_id`, `tool_name`, `tool_input`, result fields |
| `SubagentStart` | `session_id` |
| `SubagentStop` | `session_id` |
| `SessionEnd` | `session_id` |

### H3 Result: FAIL

`UserPromptSubmit` stdout injection does not work. Tested live: Claude saw no injected text when the hook wrote `{"context": "..."}` to stdout. The key is not consumed by Claude Code.

**Confirmed fallback:** `SessionStart` stdout injection works. Live session confirmed Claude receives user email, date, and cwd from SessionStart hook output. Coordify will inject network state at session start only.

**Architecture impact:** Per-prompt context updates not possible via hooks. Heat warnings surface via CLI output or SessionStart injection on new sessions.

### Go / No-Go

**GO.** All hard-required blocking and detection assumptions pass. H3 is the only uncertainty and has an architectural fallback. PreToolUse blocking works reliably. `/clear` is detectable via `payload.source`. Latency is well within targets.

Core implementation can begin.

---

## 1. Validation Principle

Coordify depends on Claude Code hook behavior. Therefore:

> No Coordify Core implementation begins until Phase 0 technical validation is complete.

The architecture assumes Claude Code hooks can:

- observe prompts;
- inject network context;
- intercept tool calls before writes;
- block risky writes;
- observe successful tool calls;
- detect `/clear`;
- detect session start/end;
- detect subagent start/stop;
- support acceptable hook latency.

If any of these assumptions fail under real local conditions, the architecture must be revised before implementation.

---

## 2. Official References

Primary references:

- https://docs.anthropic.com/en/docs/claude-code/hooks
- https://docs.anthropic.com/en/docs/claude-code/hooks-guide

These docs describe Claude Code hooks, lifecycle events, prompt hooks, tool hooks, blocking behavior, and context injection. Local validation is still required because Coordify depends on exact runtime behavior.

---

## 3. Phase 0 Deliverable

Phase 0 produces:

```text
docs/TECHNICAL_VALIDATION.md
validation-results/
  hook-matrix.md
  raw-hook-payloads/
  pretooluse-blocking/
  clear-behavior/
  subagent-behavior/
  latency/
  crash-behavior/
```

The validation result should classify each assumption as:

- `PASS`
- `PASS_WITH_CAVEAT`
- `FAIL`
- `UNKNOWN`

No assumption marked `FAIL` or `UNKNOWN` can remain unresolved before Core implementation.

---

## 4. Environment Metadata

Record:

```text
OS:
Shell:
Claude Code version:
Node version:
Rust version:
Git version:
gh CLI version:
Terminal app:
Repo path:
Validation date:
```

This matters because hook behavior may differ by OS, shell, or Claude Code version.

---

## 5. Validation Matrix

| ID | Assumption | Required? | Status |
|---|---|---:|---|
| V-001 | `SessionStart` fires on Claude Code startup | yes | TBD |
| V-002 | `UserPromptSubmit` sees raw user prompt before model response | yes | TBD |
| V-003 | `UserPromptSubmit` can inject context into Claude | yes | TBD |
| V-004 | `PreToolUse` fires before `Edit`/`Write` filesystem mutation | yes | TBD |
| V-005 | `PreToolUse` can block `Edit`/`Write` reliably | yes | TBD |
| V-006 | `PostToolUse` fires after successful file mutation | yes | TBD |
| V-007 | `PostToolUse` includes enough data to identify changed files | yes | TBD |
| V-008 | `/clear` emits detectable hook event(s) | yes | TBD |
| V-009 | `/clear` can be mapped to same terminal lineage | yes | TBD |
| V-010 | `SubagentStart` fires when Claude starts a subagent | yes | TBD |
| V-011 | `SubagentStop` fires when subagent completes | yes | TBD |
| V-012 | Subagent events include stable enough metadata | yes | TBD |
| V-013 | `SessionEnd` fires on normal terminal/session exit | yes | TBD |
| V-014 | hard-killed terminals can be detected by heartbeat timeout | yes | TBD |
| V-015 | `CwdChanged` behavior does not break network membership | yes | TBD |
| V-016 | hook latency is acceptable under repeated tool calls | yes | TBD |
| V-017 | hooks can call local Coordify Core socket/CLI | yes | TBD |
| V-018 | hook failures fail visibly and safely | yes | TBD |

---

## 6. Test V-001 — SessionStart Startup

### Goal

Confirm that `SessionStart` fires when Claude Code starts.

### Procedure

1. Create minimal `SessionStart` hook.
2. Write raw payload to file.
3. Start Claude Code in test repository.
4. Verify event payload.
5. Verify stdout injection behavior if used.

### Expected Result

- Hook fires once at startup.
- Payload includes session/project metadata.
- Hook can identify current working directory or enough data to derive project root.

### Pass Criteria

`SessionStart` fires reliably across at least 10 launches.

---

## 7. Test V-002/V-003 — UserPromptSubmit

### Goal

Confirm that `UserPromptSubmit` sees prompts before Claude responds and can inject context.

### Procedure

1. Configure hook to log prompt payload.
2. Configure hook to output test context.
3. Submit prompt:
   ```text
   Please echo whether you see COORDIFY_TEST_CONTEXT.
   ```
4. Verify Claude receives injected context.
5. Repeat with multiline prompts.
6. Repeat with vague prompts.
7. Repeat with slash commands if applicable.

### Expected Result

- Raw prompt is visible to hook.
- Hook output can be added to Claude context.
- Hook can block or modify prompt flow if needed.

### Pass Criteria

Prompt context injection works consistently.

---

## 8. Test V-004/V-005 — PreToolUse Write Blocking

### Goal

Confirm that `PreToolUse` can intercept and block file writes before filesystem mutation.

### Procedure

1. Configure `PreToolUse` for `Edit` and `Write`.
2. Hook denies writes to:
   ```text
   protected.txt
   ```
3. Ask Claude to write `protected.txt`.
4. Verify file does not change.
5. Ask Claude to write `allowed.txt`.
6. Verify file changes.
7. Repeat with edit of existing file.
8. Repeat with multi-edit if supported.

### Expected Result

- Hook fires before mutation.
- Hook sees target path.
- Hook denial prevents write.
- Reason is visible to Claude/user.

### Pass Criteria

100% of blocked write attempts produce no filesystem mutation.

### Failure Impact

If this fails, Coordify cannot enforce Level 2–4 escalation through hooks. Architecture must shift toward warnings-only or filesystem watcher rollback, which is weaker.

---

## 9. Test V-006/V-007 — PostToolUse File Observation

### Goal

Confirm Coordify can observe successful tool use and update actual files.

### Procedure

1. Configure `PostToolUse` for `Read`, `Edit`, `Write`, and `Bash`.
2. Ask Claude to read a file.
3. Ask Claude to edit a file.
4. Ask Claude to run tests.
5. Log raw payloads.

### Expected Result

Payload includes enough data to infer:

- tool name;
- target file(s);
- operation status;
- command if Bash;
- output or success/failure data where available.

### Pass Criteria

Coordify can update trace logs and actual file lists from payloads.

---

## 10. Test V-008/V-009 — `/clear` Detection

### Goal

Confirm `/clear` can be detected and mapped to an agent reset.

### Procedure

1. Start Claude Code with SessionStart/SessionEnd hooks.
2. Submit a normal prompt.
3. Invoke `/clear`.
4. Capture all hook payloads before, during, after.
5. Check whether:
   - `SessionEnd(reason=clear)` fires;
   - `SessionStart(source=clear)` fires;
   - Claude session ID changes;
   - cwd/project root remains stable;
   - terminal/process lineage can be preserved.

### Expected Result

Coordify can emit:

```text
CLEAR_INVOKED
AGENT_GENERATION_INCREMENTED
```

without misclassifying the terminal as an unrelated new agent.

### Pass Criteria

`/clear` is reliably detectable across 10 trials.

### Failure Impact

If `/clear` is not detectable, Coordify must introduce a manual fallback:

```bash
coordify clear
```

or use prompt-level detection heuristics. That would be weaker and must be reflected in `VISION.md`.

---

## 11. Test V-010/V-011/V-012 — Subagent Lifecycle

### Goal

Confirm subagent start/stop events are granular enough to support `SUBAGENT_WAITING`.

### Procedure

1. Configure `SubagentStart` and `SubagentStop` hooks.
2. Ask Claude to use a subagent/task.
3. Capture payloads.
4. Test multiple subagents if possible.
5. Test nested or concurrent subagent behavior if supported.

### Expected Result

Coordify can determine:

- subagent started;
- subagent stopped;
- which parent agent/session it belongs to;
- whether main agent should be `SUBAGENT_WAITING`.

### Pass Criteria

Subagent events are reliable enough to avoid misclassifying busy agents as idle.

### Failure Impact

If granular subagent tracking fails, Coordify must use conservative busy/idle rules.

---

## 12. Test V-013/V-014 — Session End and Crash

### Goal

Determine clean exit vs unclean crash behavior.

### Clean Exit Procedure

1. Start Claude Code.
2. Exit normally.
3. Capture `SessionEnd`.

### Crash Procedure

1. Start Claude Code.
2. Kill terminal/process.
3. Verify whether hooks fire.
4. Verify heartbeat timeout detects loss.
5. Confirm claims can become orphaned.

### Expected Result

- clean exit produces a hook event;
- hard crash may not;
- heartbeat timeout handles unclean disappearance.

### Pass Criteria

Coordify can distinguish clean release from orphan tombstone path.

---

## 13. Test V-015 — CwdChanged / Project Root

### Goal

Confirm root-network membership remains stable.

### Procedure

1. Start Claude Code in project root.
2. Change directory within repo.
3. Change directory outside repo if allowed.
4. Capture hook events.
5. Determine whether Coordify should:
   - keep original root;
   - update root;
   - block network switching;
   - require explicit rejoin.

### Expected MVP Rule

Network membership is based on initial canonical root. If cwd changes outside root, Coordify warns and may suspend claims.

---

## 14. Test V-016 — Hook Latency

### Goal

Measure overhead.

### Procedure

1. Run repeated reads/writes.
2. Measure hook execution time.
3. Measure socket roundtrip to mock Core.
4. Measure slow Core response behavior.
5. Stress with many small file operations.

### Targets

| Operation | Target |
|---|---:|
| prompt hook | < 150ms |
| PreToolUse no-risk check | < 50ms |
| PreToolUse risky-write check | < 100ms |
| PostToolUse log append | < 50ms |
| heartbeat | negligible |

If targets are missed, debounce/cache or async logging is required.

---

## 15. Test V-017 — Hook to Local Core IPC

### Goal

Prove hooks can call a local daemon reliably.

### Procedure

1. Create mock Core socket server.
2. Hook sends event to Core.
3. Core returns decision.
4. Hook blocks/allows based on decision.
5. Test socket unavailable.
6. Test slow socket.
7. Test invalid response.

### Expected Result

Hook can communicate with Core and fail safely.

---

## 16. Test V-018 — Hook Failure Safety

### Goal

Determine what happens when hook script crashes.

### Procedure

1. Make hook exit nonzero.
2. Make hook timeout.
3. Make hook output invalid JSON.
4. Make hook unable to reach Core.
5. Observe Claude Code behavior.

### Expected Result

Coordify can define degraded behavior safely.

---

## 17. Validation Result Template

For each test:

```markdown
## V-XXX Result

Status: PASS / PASS_WITH_CAVEAT / FAIL / UNKNOWN

Environment:
- OS:
- Claude Code version:
- Shell:

Observed behavior:
-

Raw payload file:
-

Impact on Coordify:
-

Architecture change required:
yes/no

Notes:
-
```

---

## 18. Go / No-Go Criteria

### Go

Proceed to Core implementation only if:

- all hard-required hook behaviors pass;
- any caveats have clear architectural mitigations;
- `/clear` is detectable or a replacement design is accepted;
- PreToolUse can block writes or escalation model is revised;
- subagent lifecycle is adequate or conservative idle detection is accepted.

### No-Go

Do not build Core if:

- writes cannot be intercepted or blocked and product still promises blocking;
- `/clear` cannot be detected and no fallback is accepted;
- hooks cannot communicate with local Core reliably;
- hook failures are silent and unsafe;
- latency is too high for normal coding.

---

## 19. Phase 0 Output Summary

At the end of validation, update:

- `VISION.md` if promises change;
- `ARCHITECTURE.md` if hooks cannot support current flow;
- `CAP_SPEC.md` if event assumptions change;
- implementation backlog with verified integration points.

Coordify should not rely on hope where local testing can give an answer in one day.
