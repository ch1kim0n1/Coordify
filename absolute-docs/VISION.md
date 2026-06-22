# Coordify — VISION.md

**Status:** Draft v0.1  
**Date:** 2026-06-22  
**Official product name:** Coordify  
**Protocol name:** Coordify Agent Protocol (CAP)  
**Core runtime:** Coordify Core  

---

## 1. One-Line Vision

**Coordify helps terminal-based Claude Code agents know what each other are doing, predict where they may collide, and coordinate before they damage the codebase.**

Coordify is a local, CLI-first coordination layer for multiple Claude Code terminal sessions working inside the same project root. It gives already-running agents shared awareness, structured ownership, deterministic conflict-risk scoring, structured handoffs, and persistent project-specific coordination memory.

---

## 2. Why Coordify Exists

Modern AI coding agents are powerful individually, but weak collectively.

A user can open five terminals, run `claude` in the same repository, and assign each terminal a task. Each Claude Code session may understand the codebase, but it does not naturally understand the other Claude Code sessions:

- One agent may refactor a file while another is patching it.
- One agent may silently drift from a bugfix into a broad refactor.
- One agent may hold ownership over a domain, then `/clear`, leaving stale mental state.
- One agent may delegate work without transferring enough context.
- Two agents may both block on each other without noticing the deadlock.
- Historical conflict patterns in the repo are lost between sessions.

Coordify fills that gap.

It does **not** try to replace Claude Code.  
It does **not** run a central AI.  
It does **not** become a master agent.  
It does **not** orchestrate work like a scheduler.

Coordify is the missing collaboration substrate between independent terminal agents.

---

## 3. Product Scope

### MVP Scope

The MVP is intentionally narrow:

- Claude Code CLI only.
- Local machine only.
- Same project root folder only.
- CLI/TUI-first.
- No web dashboard.
- No cloud service.
- No OpenClaw connection.
- No Codex/Devin/open-source agent support yet.
- No persistent live-agent network after the last Claude Code terminal closes.

### Post-MVP Scope

Deferred topics belong in `POST-MVP.md`, including:

- Codex CLI support.
- Devin support.
- open-source agent adapters.
- distributed multi-machine networks.
- multi-repo coordination.
- workspace-scoped monorepo networks.
- empirical heat calibration from Git merge conflicts.
- historical task decomposition advisor.
- advanced dashboards.
- OpenClaw integration.

---

## 4. What Coordify Is

Coordify is:

1. **A local coordination layer**  
   It runs on the user's machine and coordinates local Claude Code sessions.

2. **A protocol**  
   Agents communicate through validated Coordify Agent Protocol (CAP) events.

3. **A source of truth**  
   Coordify Core owns live state: agents, claims, heat, conflicts, handoffs, session artifacts, and project knowledge updates.

4. **A conflict-prevention system**  
   Its primary purpose is to expose intent, ownership, overlap, and historical risk early enough that agents coordinate before damaging the repo.

5. **A project intelligence layer**  
   Live networks die at session end, but project intelligence persists: hotzones, coupling graph, agent velocity profiles, heat history, and statistics.

---

## 5. What Coordify Is Not

Coordify is not:

- an orchestrator;
- a dashboard-first product;
- a cloud service;
- a Claude Code replacement;
- a wrapper that launches Claude Code;
- a scheduler that automatically spawns agents;
- a master-agent/worker-agent hierarchy;
- a hard-lock source control system;
- a general multi-agent framework in the MVP;
- a persistent agent memory that resumes old work sessions.

The user still opens terminals and runs Claude Code. Coordify only makes those Claude Code sessions aware of each other and safer to run in parallel.

---

## 6. Core Naming

| Concept | Name |
|---|---|
| Product | Coordify |
| Protocol | Coordify Agent Protocol (CAP) |
| Runtime/source of truth | Coordify Core |
| Claude Code hook adapter | Coordify Hooks |
| CLI | `coordify` |
| Optional terminal UI | Coordify TUI |

---

## 7. Core Principle

Coordify has two equally important pillars:

### 7.1 Ownership

Ownership answers:

> Who is responsible for what?

Ownership is explicit and structured. Agents do not passively become owners because they touched a file. They claim ownership through CAP.

### 7.2 Heat

Heat answers:

> How likely are two agents to collide?

Heat is deterministic. It is calculated by Coordify Core, not invented by an AI agent.

Ownership tracks responsibility.  
Heat tracks risk.

---

## 8. Agent Model

### 8.1 Definition of an Agent

In the MVP, an agent is:

> One Claude Code session running inside one terminal, rooted in one project folder.

Example:

```bash
cd ~/projects/kanshi
claude
```

That terminal is one Coordify agent.

If the user opens three terminals, runs `claude` in the same root folder, Coordify sees a three-agent network.

If the user opens a fourth terminal in a different root folder, that fourth Claude Code session belongs to a different network.

---

## 9. Network Model

### 9.1 Network Identity

A Coordify network is defined by the canonical project root folder.

```text
~/projects/kanshi        -> network:kanshi
~/projects/whirr         -> network:whirr
```

A Claude Code session belongs to the network for the root folder it is operating inside.

### 9.2 Network Lifecycle

Coordify networks are session-scoped.

```text
0 agents alive
  -> no live network

first Claude Code terminal starts in project root
  -> Coordify Core starts
  -> network is born
  -> agent joins

more Claude Code terminals start in same root
  -> agents join same network

a Claude Code terminal exits
  -> agent leaves

last Claude Code terminal exits
  -> network dies
  -> live state is deleted
  -> session artifacts are finalized
  -> project knowledge persists
```

### 9.3 Old Sessions Cannot Resume

Old live networks cannot be resumed. A historical session can only be reviewed.

Coordify has two different persistence layers:

| Layer | Survives after network dies? | Resumable? |
|---|---:|---:|
| Live network state | No | No |
| Session artifacts | Yes | Review only |
| Project intelligence | Yes | Used by future sessions |

This keeps Coordify from becoming a persistent orchestrator while allowing the codebase-specific intelligence to improve over time.

---

## 10. Agent States

Coordify requires first-class agent states.

| State | Meaning |
|---|---|
| `DISCOVERY` | Agent exists, but has no clear actionable task yet. |
| `IDLE` | No active user command, no active subagent, waiting for user input. |
| `ACTIVE` | Agent has an accepted task/intent/domain claim and is executing. |
| `SUBAGENT_WAITING` | Main agent is waiting, but a subagent/tool execution is still active. |
| `TESTING` | Agent is running or evaluating tests as part of active work. |
| `BLOCKED` | Agent cannot proceed due to dependency, claim, conflict, or user decision. |
| `NEGOTIATING` | Agent is participating in structured conflict resolution. |
| `WAITING_USER` | Agent requires a human decision. |
| `OFFLINE` | Agent has exited cleanly or disappeared. |

### 10.1 DISCOVERY

`DISCOVERY` is important.

If the user starts Claude Code but gives no clear task, the agent should not claim random domains or files.

If the user says:

```text
solve issues on GitHub
```

the agent remains in `DISCOVERY` until it discovers or selects a concrete issue. It may inspect GitHub using `gh issue list` or `gh issue view`, but it does not claim task/domain/file ownership until a clear task is known.

Rule:

> No clear task = no ownership claim.

### 10.2 IDLE

An agent is truly idle only when:

- it is waiting for user input;
- it has no active user command;
- it has no active subagent running in the background;
- it is not blocked, testing, negotiating, or waiting on a handoff.

A main Claude instance waiting for a subagent is **not** idle. It is `SUBAGENT_WAITING`.

---

## 11. Core Objects

Coordify locks in these objects:

| Object | Purpose |
|---|---|
| `Agent` | One Claude Code terminal session. |
| `Task` | The user-level work objective. |
| `Intent` | Why the agent is touching the code. |
| `Domain` | Functional/codebase area affected. |
| `Claim` | Structured ownership of task/domain/file/scope. |
| `Ownership` | Active responsibility model derived from claims. |
| `Heat` | Pairwise conflict-risk score between agents. |
| `Session` | One live network run and its artifacts. |
| `Knowledge` | Persistent project intelligence across sessions. |
| `Event` | Validated CAP state mutation. |
| `Handoff` | Rich transfer of task context from one agent to another. |
| `Conflict` | Structured coordination problem opened by heat or claim overlap. |

---

## 12. Intent Model

Intent must be formal and canonical.

Examples:

- `SECURITY`
- `QA`
- `TESTING`
- `PERFORMANCE`
- `REFACTOR`
- `DOCUMENTATION`
- `FEATURE`
- `BUGFIX`
- `ARCHITECTURE`
- `DEVOPS`
- `RESEARCH`
- `MIGRATION`

Intent heavily affects heat.

Same file, same domain, different intent may be safe:

```text
Agent A: SECURITY on src/auth/session.ts
Agent B: DOCUMENTATION on src/auth/session.ts
```

Same file, same domain, same intent is much riskier:

```text
Agent A: SECURITY on src/auth/session.ts
Agent B: SECURITY on src/auth/session.ts
```

Agents may propose intent, but Coordify Core validates the submitted intent against the canonical schema.

---

## 13. Domain Model

Domains are first-class objects.

Examples:

- `AUTHENTICATION`
- `PAYMENTS`
- `DATABASE`
- `API`
- `FRONTEND`
- `DEPLOYMENT`
- `OBSERVABILITY`
- `TESTING`
- `DOCUMENTATION`
- `INFRASTRUCTURE`
- `SECURITY`
- `CONFIGURATION`

A domain can map to path patterns:

```yaml
domains:
  AUTHENTICATION:
    paths:
      - "src/auth/**"
      - "tests/auth/**"
  PAYMENTS:
    paths:
      - "src/payments/**"
      - "tests/payments/**"
```

For MVP, domains can be generated by agents and normalized by Coordify Core. Later, projects can refine them through `coordify.yaml`.

---

## 14. Ownership Model

Ownership exists at exactly three levels for MVP:

1. **Task ownership**
2. **Domain ownership**
3. **File/path ownership**

Do not add more layers in MVP. Function-level or line-level ownership belongs in future work if proven necessary.

### 14.1 Explicit Claims

Agents claim ownership before meaningful work.

Example:

```json
{
  "type": "CLAIM_CREATED",
  "task": "Fix session expiry bug from GitHub issue #88",
  "intent": "BUGFIX",
  "domains": ["AUTHENTICATION"],
  "estimatedFiles": [
    "src/auth/session.ts",
    "tests/auth/session.test.ts"
  ]
}
```

Coordify Core validates the claim.

If valid, the network updates and heat recalculates.

### 14.2 Co-Ownership

Same file ownership is allowed when intent/scope differs.

Example:

```text
Agent A: SECURITY, src/auth/session.ts
Agent B: TESTING, src/auth/session.ts
```

This is co-ownership, not automatically a conflict.

Heat determines whether overlap is safe, monitorable, or dangerous.

### 14.3 Dynamic Reassignment

User prompts can change an agent's task.

If the user says:

```text
Actually forget auth. Fix deployment.
```

the agent must update CAP:

1. release old claims;
2. declare new task;
3. declare new intent;
4. declare new domains;
5. declare new estimated files;
6. recalculate heat.

Ownership is derived from the active task, not from historical conversation.

---

## 15. `/clear` Semantics

Claude Code `/clear` is a hard Coordify reset for that agent.

When `/clear` is detected:

- task ownership is released;
- domain ownership is released;
- file/path ownership is released;
- active intent is cleared;
- estimated files are cleared;
- pending delegation state is cleared;
- active conflict state is cleared;
- agent generation increments;
- agent returns to `DISCOVERY`;
- heat recalculates;
- `CLEAR_INVOKED` event is written.

This prevents ghost ownership.

---

## 16. Agent Identity Across `/clear`

Coordify separates identity into three layers:

| Identity | Meaning |
|---|---|
| `terminalInstanceId` | Stable terminal/process lineage. |
| `claudeSessionId` | Claude Code's session identifier, which may change. |
| `agentId` | Coordify's logical agent identity for the live terminal. |
| `generation` | Increments after `/clear` or reset events. |

If Claude Code changes session ID after `/clear`, Coordify should preserve terminal lineage and increment generation instead of treating it as a totally unrelated agent.

Example:

```json
{
  "terminalInstanceId": "terminal-abc",
  "agentId": "agent-123",
  "claudeSessionId": "session-789",
  "generation": 3
}
```

---

## 17. Heat Model

Heat is a deterministic pairwise score between two agents.

Coordify uses undirected pairwise heat:

```text
Agent A ↔ Agent B = 83%
```

For `n` agents, the number of heat edges is:

```text
n * (n - 1) / 2
```

Examples:

| Agents | Heat Edges |
|---:|---:|
| 2 | 1 |
| 5 | 10 |
| 20 | 190 |
| 50 | 1,225 |

### 17.1 Predicted Heat vs Current Heat

Coordify tracks two versions:

| Type | Meaning |
|---|---|
| Predicted Heat | Risk based on proposed task/intent/domain/estimated files + historical knowledge. |
| Current Heat | Risk based on actual claims, actual files touched, tool activity, branch/worktree, and live state. |

Predicted heat exists before coding starts.  
Current heat changes during execution.

### 17.2 Heat Formula

MVP heat formula:

```text
Heat(A, B) =
  10% Task Similarity
+ 15% Intent Similarity
+ 15% Domain Overlap
+ 20% File / Path Overlap
+ 10% Temporal Activity Overlap
+ 10% Branch / Worktree Proximity
+ 10% Historical Hotzone Risk
+ 10% Historical Coupling
```

Task similarity is intentionally weak because deterministic lexical similarity can miss semantic equivalence. Stronger signals come from intent, domain, file/path overlap, historical project intelligence, branch/worktree context, and actual activity.

### 17.3 Heat Bands

Default bands:

| Score | Band | Meaning |
|---:|---|---|
| 0–25 | Safe | No action. |
| 26–50 | Monitor | Include in context, no interruption. |
| 51–75 | Overlap | Warn and encourage coordination. |
| 76–100 | Conflict Candidate | Open structured conflict or require coordination. |

These defaults are configurable through `coordify.yaml`.

### 17.4 Directional Signals

MVP exposes heat as undirected, but the calculation may include directional signals:

- Agent A reading, Agent B writing.
- Agent A writing, Agent B writing.
- Agent A testing, Agent B editing implementation.
- Agent A waiting on Agent B.

Directional signals improve calculation without making the user-facing graph harder to read.

### 17.5 Incremental Heat Calculation

Coordify must not recalculate all heat edges on every event.

Rules:

- only recalculate edges involving the changed agent;
- debounce high-frequency file events;
- batch related file-touch updates;
- cache hotzone and coupling lookups;
- update heat history at bounded intervals.

---

## 18. Pre-Task Heat Forecast

Before accepting a claim, Coordify calculates speculative predicted heat against:

- active claims;
- active domains;
- active files;
- active branches/worktrees;
- hotzone map;
- coupling graph;
- current agent states.

Example:

```text
Proposed task: Fix auth token expiry

Predicted heat with Agent B: 74% OVERLAP

Reasons:
- Agent B owns AUTHENTICATION
- src/auth/session.ts is a historical hotzone
- src/auth/session.ts is coupled with tests/auth/session.test.ts
- both agents are active on same branch

Recommendation:
Split scope now or sequence after Agent B.
```

This is one of Coordify's core UX improvements: pre-conflict intelligence.

---

## 19. Conflict Handling

Coordify is not just a collision detector. It must define what happens after heat crosses a threshold.

### 19.1 Conflict Flow

1. Heat crosses configured threshold.
2. Coordify Core emits `HEAT_THRESHOLD_EXCEEDED`.
3. Affected agents enter `NEGOTIATING`.
4. Coordination escalation level is selected.
5. Agents submit structured proposals.
6. Coordify Core validates proposals.
7. Compatible proposals are applied.
8. Incompatible proposals escalate to user.
9. Both agents present the same user decision prompt.
10. User decides.
11. Claims update.
12. Heat recalculates.
13. Agents resume.

### 19.2 Supported Resolution Types

- `CO_OWN`
- `SPLIT_SCOPE`
- `YIELD_CLAIM`
- `TRANSFER_TASK`
- `QUEUE_TASK`
- `ASK_USER`
- `ABORT_TASK`

### 19.3 Coordination Escalation Levels

| Level | Name | Behavior |
|---:|---|---|
| 0 | Observe | No interruption, log only. |
| 1 | Warn | Agent sees overlap warning. |
| 2 | Coordinate | CAP coordination required before risky write. |
| 3 | Ask User | Human approval required. |
| 4 | Block | Strict mode only; protected write blocked until safe. |

Default MVP should use Levels 1–3. Level 4 exists for strict/protected paths only.

---

## 20. Deadlock Detection

Coordify Core tracks waiting relationships as a graph.

Example:

```text
Agent A waits for Agent B
Agent B waits for Agent A
```

This is a cycle and must trigger:

```text
DEADLOCK_DETECTED
```

Deadlocks escalate to user arbitration. Agents should not negotiate endlessly.

---

## 21. Claim Tombstones and Crash Handling

If an agent exits cleanly, claims are released.

If an agent disappears without clean shutdown, claims become `ORPHANED` instead of vanishing immediately.

Example:

```json
{
  "claimId": "claim-123",
  "owner": "agent-a",
  "status": "ORPHANED",
  "orphanedAt": "2026-06-22T18:42:00Z",
  "ttlSeconds": 300
}
```

Other agents can reclaim after TTL or user can force release:

```bash
coordify claim release --orphaned
```

This prevents unsafe silent cleanup after crashes.

---

## 22. Branch and Worktree Awareness

Heat must account for Git branch/worktree context.

Default branch multiplier:

| Context | Multiplier |
|---|---:|
| Same branch | 1.0 |
| Different branch, same base | 0.65 |
| Different worktree | 0.45 |
| Unknown | 0.85 |

Same file, same branch is much riskier than same file, different worktree.

---

## 23. Rich Agent Handoff

Delegation must transfer context, not just claims.

A handoff includes:

- task summary;
- intent;
- domains;
- files modified;
- files in progress;
- open questions;
- known risks;
- current failing tests;
- heat at handoff;
- claim transfer decision;
- whether sender releases ownership;
- whether receiver accepts ownership.

Example:

```json
{
  "type": "TASK_HANDOFF",
  "from": "agent-a",
  "to": "agent-b",
  "taskSummary": "Update token validation for 24h expiry.",
  "intent": "BUGFIX",
  "domains": ["AUTHENTICATION"],
  "filesModified": ["src/auth/session.ts"],
  "filesInProgress": ["src/auth/tokens.ts"],
  "openQuestions": ["Should refresh token grace period remain 7 days?"],
  "knownRisks": ["tests/auth/session.test.ts currently failing"],
  "heatAtHandoff": 34,
  "claimTransfer": {
    "releaseFromSender": true,
    "claimForReceiver": true
  }
}
```

Weak handoff causes confusion. Rich handoff creates continuity.

---

## 24. Persistent Project Intelligence

Coordify distinguishes live coordination from persistent project intelligence.

Live state dies when the last agent exits.  
Project intelligence persists under `.coordify/knowledge/`.

### 24.1 Hotzone Map

Tracks files/domains that historically generate heat or conflicts.

Stored at:

```text
.coordify/knowledge/hotzones.json
```

Example:

```json
{
  "path": "src/auth/session.ts",
  "heatEvents": 14,
  "conflicts": 6,
  "averageHeat": 78,
  "riskLevel": "HIGH"
}
```

### 24.2 Coupling Discovery Graph

Tracks files that are behaviorally touched together across sessions.

Stored at:

```text
.coordify/knowledge/coupling-graph.json
```

Example:

```json
{
  "src/api/users.ts": {
    "schema.prisma": 0.87,
    "tests/api/users.test.ts": 0.79
  }
}
```

This improves heat scoring beyond static import analysis.

### 24.3 Agent Velocity Profiles

Tracks agent workflow performance:

- prompt to claim time;
- claim to first read time;
- claim to first write time;
- tokens before first write;
- tasks with zero writes;
- tasks completed;
- tasks abandoned;
- blocked time;
- idle time;
- active time.

### 24.4 Coordination Overhead

Measures how much activity is spent coordinating vs coding.

```text
Coordination Overhead =
coordination time/tokens/events
divided by
total session activity
```

Default interpretation:

| Range | Meaning |
|---:|---|
| 0–10% | Lightweight |
| 10–25% | Healthy coordination |
| 25–40% | Heavy coordination |
| 40%+ | Over-coordinated or badly split work |

### 24.5 Intent Drift Detection

Agents declare an intent. Coordify observes behavior.

If declared intent and behavioral intent diverge, Coordify emits:

```text
INTENT_DRIFT_DETECTED
```

Example:

```text
Declared: BUGFIX
Observed: REFACTOR
```

In MVP, intent drift should warn, raise heat, and request claim update. It should not automatically block work unless configured.

### 24.6 Ghost Work Detection

Ghost work is a task/session where an agent claims meaningful ownership but produces no meaningful artifact before release, `/clear`, crash, or session end.

Types:

- valid investigation;
- ghost work;
- abandoned work;
- blocked work.

Ghost work is useful because it exposes wasted coordination slots and vague prompts that lead to token burn.

---

## 25. Logging

Coordify logs deeply. All logs for a session live in the same folder.

```text
.coordify/
  sessions/
    2026-06-22_18-42-11/
      events.log
      diagnostics.log
      trace.log
      stats.json
      heat-history.json
      network-final.json
      session-summary.json
```

### 25.1 Log Types

| File | Purpose |
|---|---|
| `events.log` | Structured CAP protocol events. |
| `diagnostics.log` | Coordify software errors, warnings, crashes, schema failures. |
| `trace.log` | Maximum-detail agent/tool/file activity. |

Trace should collect as much detail as possible. Do not trim detail to make logs pretty. Use rotation and compression.

### 25.2 Compression

During live session: logs remain expanded.

When last agent exits:

1. session finalizes;
2. final stats are written;
3. heat history is finalized;
4. logs rotate/compress;
5. session is marked closed.

If crash prevents clean finalization, next startup finalizes the previous session.

---

## 26. Statistics

Statistics are part of the product, not an afterthought.

### 26.1 Engineering Metrics

- tasks completed;
- tasks accepted;
- tasks rejected;
- tasks delegated;
- tasks transferred;
- average task duration;
- blocked time;
- waiting time;
- idle time;
- active time;
- testing time;
- subagent waiting time;
- conflicts triggered;
- conflicts resolved;
- conflicts escalated to user;
- ownership claims;
- ownership releases;
- domains touched;
- files touched;
- estimated vs actual file accuracy;
- average heat;
- peak heat;
- heat resolved;
- heat generated;
- schema validation failures;
- CAP messages sent;
- CAP messages received;
- tool calls;
- file reads;
- file writes;
- test runs;
- test passes;
- test failures;
- GitHub issues viewed;
- GitHub issues updated;
- GitHub issues closed.

### 26.2 Resource Metrics

- estimated token usage;
- input tokens;
- output tokens;
- tool-call count;
- tokens per completed task;
- tokens spent on abandoned tasks;
- tokens spent while blocked;
- tokens spent before first useful edit.

Some token metrics depend on what Claude Code exposes. The schema should support them even if values are unavailable at first.

### 26.3 Entertainment Metrics

Keep these in the vision. They make Coordify feel alive.

Examples:

- Top Contributor;
- Most Efficient Agent;
- Most Cooperative Agent;
- Fastest Finisher;
- Conflict Magnet;
- Heat Generator;
- Heat Diffuser;
- Most Reliable Agent;
- Most Expensive Agent;
- Most Idle Agent;
- Most Overloaded Agent;
- Best Delegator;
- Best Task Finisher;
- Most Accurate Planner;
- Most Chaotic Agent;
- Most Helpful Agent;
- Longest Active Streak;
- Most Domains Covered;
- Most Files Touched;
- Most Tests Written.

Entertainment metrics are not fluff. They increase user attachment, session review value, and shareability.

---

## 27. CLI and TUI

MVP is CLI-first.

Required commands:

```bash
coordify status
coordify heat
coordify agents
coordify claims
coordify logs
coordify stats
coordify session list
coordify session inspect
coordify graph
coordify watch
```

No web UI in MVP.

Optional TUI:

```bash
coordify watch
```

Can show:

- active agents;
- tasks;
- states;
- ownership;
- heat edges;
- conflicts;
- handoffs;
- stats.

When live network dies, TUI dies. Session artifacts remain.

---

## 28. Trust Model

Coordify is local-first. It is not designed to defend against a malicious process that already has full same-user filesystem access.

It should protect against:

- malformed CAP events;
- accidental corruption;
- casual local process spoofing;
- non-Coordify processes writing live state;
- bad schema data poisoning project knowledge.

It should not claim to defend against:

- malware running as the same OS user;
- a malicious developer intentionally editing `.coordify`;
- compromised dependencies with full filesystem access.

### 28.1 Required Trust Controls

- session-scoped auth token;
- local socket permissions;
- CAP handshake;
- schema validation;
- only Coordify Core writes canonical state;
- knowledge updates derived from accepted CAP events;
- atomic writes;
- corruption detection;
- quarantine for invalid knowledge files.

---

## 29. Configuration

Coordify ships with opinionated defaults but must be configurable.

Config file:

```text
coordify.yaml
```

Example:

```yaml
heat:
  safeMax: 25
  monitorMax: 50
  overlapMax: 75
  conflictMin: 76

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
  compressOnSessionEnd: true

knowledge:
  enabled: true
  hotzoneWeight: 0.10
  couplingWeight: 0.10
```

Hardcoded thresholds are not acceptable for teams with different risk tolerance.

---

## 30. Coordify Core Reliability

Coordify Core is mandatory in MVP.

It is not an orchestrator. It is the local source of truth.

If Core crashes, agents must not silently pretend coordination is still active.

Default principle:

> Fail safe, not invisible.

Degraded behavior depends on escalation level:

| Mode | Behavior if Core unavailable |
|---|---|
| Observe | Agent may continue, warning logged. |
| Warn | Agent may continue, warning shown. |
| Coordinate | Agent warns user before risky writes. |
| Ask User | Agent asks user whether to continue uncoordinated. |
| Strict | Protected writes block until Core recovers. |

Core recovery should attempt:

1. reconnect;
2. restart if stale lock;
3. reload live state snapshot;
4. mark session recovered;
5. emit `CORE_RECOVERED`.

---

## 31. Knowledge Write Safety

Project knowledge files must use atomic writes.

Pattern:

```text
write file.tmp
fsync
rename file.tmp -> file
keep file.prev
validate on startup
quarantine corrupt files
rebuild from event logs if possible
```

Knowledge files are derived indexes. The append-only event log is the recoverable source.

---

## 32. Startup Race Protection

If two Claude Code terminals start within milliseconds, only one Coordify Core should start.

Bootstrap must use:

- project-scoped lock file;
- local socket or named pipe ownership;
- PID/heartbeat validation;
- stale lock recovery.

Startup flow:

1. agent hook checks for Core socket;
2. if no socket, tries lock;
3. lock winner starts Core;
4. loser waits and connects;
5. stale lock is verified before breaking.

---

## 33. Simulation and Testability

CAP must be testable without Claude Code.

Coordify needs simulation mode:

```bash
coordify simulate
coordify replay
```

It should support:

- agent join;
- user prompt;
- CAP claim;
- file read/write;
- heat threshold;
- conflict;
- `/clear`;
- crash;
- orphaned claim;
- deadlock;
- Core restart.

This is required for CI and contributors.

---

## 34. Phase 0: Technical Validation

No Coordify Core implementation begins until Claude Code hook behavior is locally validated.

Phase 0 must prove:

- `PreToolUse` can intercept writes before mutation;
- `PreToolUse` can block risky writes reliably;
- `UserPromptSubmit` can inject live network context;
- `/clear` produces detectable hook events;
- `SubagentStart` / `SubagentStop` are granular enough;
- terminal close and hard crash are distinguishable enough through SessionEnd/heartbeat;
- hook latency is acceptable.

If Phase 0 fails, the architecture changes.

---

## 35. MVP Success Definition

Coordify MVP succeeds if:

1. user can open multiple Claude Code terminals in the same repo;
2. agents auto-join the same network;
3. agents see live peer summaries;
4. agents submit valid CAP claims;
5. Coordify calculates predicted and current heat;
6. agents coordinate before high-risk writes;
7. `/clear` resets ownership cleanly;
8. crashed agents produce orphaned claim tombstones;
9. handoffs transfer rich context;
10. logs/stats/session artifacts are saved;
11. hotzones and coupling graphs improve future sessions;
12. no web UI or cloud infrastructure is required.

---

## 36. Guiding Philosophy

Coordify exists to make parallel AI coding feel safe.

The best version of Coordify is almost invisible:

- when agents are not colliding, it stays quiet;
- when risk appears, it surfaces the reason clearly;
- when coordination is needed, it provides structure;
- when the user must decide, every affected agent asks the same question;
- when the session ends, it leaves behind useful artifacts and better project intelligence.

Coordify should not make the user babysit agents.

Coordify should make it possible to trust several Claude Code terminals working in the same codebase at once.
