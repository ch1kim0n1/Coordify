# Coordify Phase 0 — Hook Validation Matrix

Generated: 2026-06-23T01:20:46.961Z

## Results

| ID | Assumption | Status | Evidence |
|----|-----------|--------|----------|
| H1 | `PreToolUse` fires before file mutation | **PASS** | 96 payload(s) captured, full payload structure confirmed |
| H2 | `PreToolUse` can block writes via exit code 1 | **PASS** | 8 Write tool call(s) to sentinel path intercepted — hook exits 1 on match |
| H3 | `UserPromptSubmit` can inject context into Claude input | **MANUAL** | 10 payload(s) — hook fires and writes {"context":"..."} to stdout; Claude Code consumption unconfirmed |
| H4 | `/clear` produces detectable SessionStart with source: clear | **PASS** | Confirmed — field: payload.source, startup value: "startup", clear value: "clear" (1 clear event(s) captured) |
| H5 | `SubagentStart` / `SubagentStop` fire at subagent boundaries | **PASS** | SubagentStart: 1, SubagentStop: 1 |
| H6 | Clean exit vs hard crash distinguishable via SessionEnd presence | **MANUAL** | 4 clean SessionEnd(s), 3 real SessionStart(s) — crash scenario not yet tested |
| H7 | PreToolUse latency p99 < 100ms | **PASS** | p50=3ms p95=25ms p99=30ms (34 samples) |

## Status Key

| Status | Meaning |
|--------|---------|
| PASS | Confirmed with captured payload evidence |
| PARTIAL | Hook fires but payload structure differs from assumption |
| FAIL | Does not fire or cannot achieve required behavior |
| MANUAL | Hook fired; requires manual observation to confirm |
| PENDING | Not yet tested |

## Coverage

- Total payload files: 204
- Hooks seen: BadHook, PostToolUse, PreToolUse, SessionEnd, SessionStart, SubagentStart, SubagentStop, TestHook, UserPromptSubmit
- Latency samples: 70
