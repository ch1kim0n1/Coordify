# Coordify Agent Protocol — CAP_SPEC.md

**Status:** Draft v0.1  
**Date:** 2026-06-22  
**Protocol name:** Coordify Agent Protocol (CAP)  
**Product:** Coordify  
**Runtime authority:** Coordify Core  

---

## 1. Purpose

CAP is the protocol that allows Claude Code terminal agents to coordinate through Coordify Core.

CAP defines:

- event schemas;
- agent identity;
- state transitions;
- claims;
- ownership;
- heat;
- handoffs;
- conflicts;
- negotiation;
- deadlock detection;
- `/clear` behavior;
- crash/orphan behavior;
- trust handshake;
- timeout behavior.

Coordify Core must not accept ad-hoc messages. Every state mutation must be a validated CAP event.

---

## 2. Protocol Principles

1. **Schema first**  
   Every event must validate before it mutates state.

2. **Core owns truth**  
   Agents propose. Coordify Core validates and commits.

3. **No free-form agent chat for coordination**  
   Coordination happens through typed events.

4. **Heat is deterministic**  
   Agents provide inputs. Core calculates heat.

5. **Ownership is explicit**  
   Claims are created, updated, released, transferred, orphaned, or reclaimed.

6. **User decides unresolved ambiguity**  
   Agents can negotiate structured proposals, but unresolved strategic decisions escalate to the user.

7. **Old sessions are review-only**  
   CAP does not resume dead networks.

8. **Simulation is a first-class adapter**  
   CAP must work without Claude Code for tests.

---

## 3. Transport

MVP transport:

- local Unix socket on macOS/Linux;
- Windows named pipe on Windows;
- framed JSON messages;
- session-scoped auth token;
- request/response and event stream.

CAP does not require network transport in MVP.

---

## 4. Envelope

Every CAP message uses a standard envelope.

```json
{
  "capVersion": "0.1",
  "messageId": "msg_01H...",
  "messageKind": "event",
  "timestamp": "2026-06-22T18:42:00.000Z",
  "projectRoot": "/abs/path/project",
  "sessionId": "coordify-session-abc",
  "agentId": "agent-a",
  "token": "session-token",
  "event": {}
}
```

### Fields

| Field | Required | Meaning |
|---|---:|---|
| `capVersion` | yes | Protocol version. |
| `messageId` | yes | Unique message ID. |
| `messageKind` | yes | `event`, `request`, `response`, `heartbeat`, `error`. |
| `timestamp` | yes | ISO timestamp. |
| `projectRoot` | yes | Canonical root folder. |
| `sessionId` | yes | Live Coordify session ID. |
| `agentId` | usually | Required for agent-originated messages. |
| `token` | yes after handshake | Session auth token. |
| `event` | for events | CAP event payload. |

---

## 5. Handshake

### 5.1 Agent Connects

```json
{
  "type": "CAP_HANDSHAKE",
  "capVersion": "0.1",
  "projectRoot": "/abs/path/project",
  "terminalInstanceId": "terminal-abc",
  "claudeSessionId": "claude-session-xyz",
  "process": {
    "pid": 18432,
    "ppid": 18400
  },
  "adapter": {
    "name": "coordify-hooks",
    "version": "0.1.0",
    "runtime": "claude-code"
  }
}
```

### 5.2 Core Responds

```json
{
  "type": "CAP_HANDSHAKE_ACCEPTED",
  "sessionId": "coordify-session-abc",
  "agentId": "agent-a",
  "generation": 1,
  "token": "session-token",
  "coreVersion": "0.1.0",
  "configHash": "sha256:..."
}
```

### 5.3 Rejection

```json
{
  "type": "CAP_HANDSHAKE_REJECTED",
  "reason": "PROJECT_ROOT_MISMATCH"
}
```

Rejection reasons:

- `INVALID_CAP_VERSION`
- `PROJECT_ROOT_MISMATCH`
- `INVALID_ADAPTER`
- `CORE_DEGRADED`
- `SCHEMA_MISMATCH`
- `TOKEN_REQUIRED`
- `LOCK_CONFLICT`

---

## 6. Agent Identity Schema

```json
{
  "agentId": "agent-a",
  "terminalInstanceId": "terminal-abc",
  "claudeSessionId": "claude-session-xyz",
  "generation": 1,
  "pid": 18432,
  "projectRoot": "/abs/path/project",
  "state": "DISCOVERY",
  "joinedAt": "2026-06-22T18:42:00.000Z",
  "lastSeen": "2026-06-22T18:42:02.000Z"
}
```

`generation` increments after `/clear`.

---

## 7. Agent States

Allowed states:

```text
DISCOVERY
IDLE
ACTIVE
SUBAGENT_WAITING
TESTING
BLOCKED
NEGOTIATING
WAITING_USER
OFFLINE
```

### State Transition Rules

```text
new agent -> DISCOVERY
DISCOVERY + accepted claim -> ACTIVE
ACTIVE + no command/subagent -> IDLE
ACTIVE + subagent start -> SUBAGENT_WAITING
SUBAGENT_WAITING + subagent stop -> ACTIVE or IDLE
ACTIVE + tests running -> TESTING
ACTIVE + conflict -> NEGOTIATING
NEGOTIATING + unresolved -> WAITING_USER
any live state + clean exit -> OFFLINE
any live state + heartbeat timeout -> OFFLINE + claims ORPHANED
any live state + /clear -> DISCOVERY + generation increment
```

Invalid transitions must be rejected or normalized by Core.

---

## 8. Canonical Intents

Allowed initial intents:

```text
SECURITY
QA
TESTING
PERFORMANCE
REFACTOR
DOCUMENTATION
FEATURE
BUGFIX
ARCHITECTURE
DEVOPS
RESEARCH
MIGRATION
CONFIGURATION
OBSERVABILITY
```

Projects may extend the list through config, but extensions must remain canonical after validation.

---

## 9. Claim Schema

```json
{
  "claimId": "claim-123",
  "agentId": "agent-a",
  "status": "PROPOSED",
  "task": {
    "summary": "Fix session expiry bug from GitHub issue #88",
    "source": {
      "type": "github_issue",
      "repo": "owner/repo",
      "issueNumber": 88
    }
  },
  "intent": "BUGFIX",
  "domains": ["AUTHENTICATION"],
  "scope": {
    "estimatedFiles": [
      "src/auth/session.ts",
      "tests/auth/session.test.ts"
    ],
    "actualFiles": []
  },
  "confidence": 0.86,
  "createdAt": "2026-06-22T18:42:00.000Z",
  "updatedAt": "2026-06-22T18:42:00.000Z"
}
```

### Claim Statuses

```text
PROPOSED
PROVISIONAL
ACTIVE
SHARED
RELEASED
ORPHANED
RECLAIMABLE
TRANSFERRED
EXPIRED
REJECTED
```

### Confidence Behavior

| Confidence | Result |
|---:|---|
| `>= 0.75` | Claim can become `ACTIVE`. |
| `0.45 - 0.749` | Claim becomes `PROVISIONAL`. |
| `< 0.45` | Claim rejected; agent remains `DISCOVERY`. |

Config can tune thresholds.

### Provisional Claim Rules

Provisional claims:

- count for predicted heat;
- appear in network context;
- allow reads/discovery;
- require recheck before risky writes;
- should be upgraded or released once clearer.

---

## 10. Core Event Types

### Agent Lifecycle

- `AGENT_JOINED`
- `AGENT_LEFT`
- `AGENT_LOST`
- `HEARTBEAT`
- `AGENT_STATE_CHANGED`
- `AGENT_GENERATION_INCREMENTED`

### Session Lifecycle

- `SESSION_STARTED`
- `SESSION_FINALIZED`
- `CORE_DEGRADED`
- `CORE_RECOVERED`

### Prompt/Task

- `USER_PROMPT_OBSERVED`
- `TASK_DECLARED`
- `TASK_UPDATED`
- `TASK_COMPLETED`
- `TASK_ABORTED`

### Claim/Ownership

- `CLAIM_PROPOSED`
- `CLAIM_CREATED`
- `CLAIM_UPDATED`
- `CLAIM_RELEASED`
- `CLAIM_ORPHANED`
- `CLAIM_RECLAIMABLE`
- `CLAIM_RECLAIMED`
- `CLAIM_TRANSFERRED`
- `CLAIM_REJECTED`

### Tool/File Activity

- `TOOL_PRECHECK`
- `TOOL_EXECUTED`
- `FILE_READ`
- `FILE_TOUCHED`
- `ACTUAL_FILES_UPDATED`
- `TEST_RUN`

### Heat

- `HEAT_UPDATED`
- `HEAT_THRESHOLD_EXCEEDED`
- `PREDICTED_HEAT_CALCULATED`
- `CURRENT_HEAT_CALCULATED`

### Conflict

- `CONFLICT_OPENED`
- `CONFLICT_PROPOSAL_SUBMITTED`
- `CONFLICT_PROPOSAL_ACCEPTED`
- `CONFLICT_PROPOSAL_REJECTED`
- `CONFLICT_RESOLVED`
- `CONFLICT_TIMEOUT`
- `CONFLICT_ABORTED`
- `DEADLOCK_DETECTED`

### Handoff

- `TASK_HANDOFF_PROPOSED`
- `TASK_HANDOFF_ACCEPTED`
- `TASK_HANDOFF_REJECTED`
- `TASK_HANDOFF_COMPLETED`

### Drift/Ghost/Knowledge

- `INTENT_DRIFT_DETECTED`
- `GHOST_WORK_DETECTED`
- `HOTZONE_UPDATED`
- `COUPLING_UPDATED`
- `AGENT_PROFILE_UPDATED`

### Reset

- `CLEAR_INVOKED`

---

## 11. Agent Lifecycle Events

### 11.1 AGENT_JOINED

```json
{
  "type": "AGENT_JOINED",
  "agent": {
    "agentId": "agent-a",
    "terminalInstanceId": "terminal-abc",
    "claudeSessionId": "claude-session-xyz",
    "generation": 1,
    "pid": 18432,
    "state": "DISCOVERY"
  }
}
```

### 11.2 HEARTBEAT

```json
{
  "type": "HEARTBEAT",
  "agentId": "agent-a",
  "state": "ACTIVE",
  "activeSubagents": 0,
  "lastToolActivityAt": "2026-06-22T18:42:00.000Z"
}
```

### 11.3 AGENT_LOST

Core emits after heartbeat timeout.

```json
{
  "type": "AGENT_LOST",
  "agentId": "agent-a",
  "reason": "HEARTBEAT_TIMEOUT"
}
```

---

## 12. `/clear` Event

```json
{
  "type": "CLEAR_INVOKED",
  "agentId": "agent-a",
  "terminalInstanceId": "terminal-abc",
  "previousClaudeSessionId": "claude-session-old",
  "newClaudeSessionId": "claude-session-new",
  "previousGeneration": 2,
  "newGeneration": 3,
  "reset": {
    "releaseClaims": true,
    "clearIntent": true,
    "clearTask": true,
    "clearEstimatedFiles": true,
    "clearConflicts": true,
    "clearPendingHandoffs": true
  }
}
```

Core effects:

- release active claims;
- close conflicts involving agent;
- cancel pending handoffs involving agent unless accepted by another agent;
- set state to `DISCOVERY`;
- increment generation;
- recalculate heat.

---

## 13. Claim Events

### 13.1 CLAIM_PROPOSED

```json
{
  "type": "CLAIM_PROPOSED",
  "agentId": "agent-a",
  "task": {
    "summary": "Fix session expiry bug from GitHub issue #88",
    "source": {
      "type": "github_issue",
      "issueNumber": 88
    }
  },
  "intent": "BUGFIX",
  "domains": ["AUTHENTICATION"],
  "estimatedFiles": [
    "src/auth/session.ts",
    "tests/auth/session.test.ts"
  ],
  "confidence": 0.86
}
```

Core validates and either emits:

- `CLAIM_CREATED`;
- `CLAIM_REJECTED`;
- `PREDICTED_HEAT_CALCULATED`;
- `HEAT_THRESHOLD_EXCEEDED`.

### 13.2 CLAIM_CREATED

```json
{
  "type": "CLAIM_CREATED",
  "claimId": "claim-123",
  "agentId": "agent-a",
  "status": "ACTIVE"
}
```

### 13.3 CLAIM_RELEASED

```json
{
  "type": "CLAIM_RELEASED",
  "claimId": "claim-123",
  "agentId": "agent-a",
  "reason": "TASK_COMPLETED"
}
```

Release reasons:

- `TASK_COMPLETED`
- `TASK_ABORTED`
- `USER_CHANGED_TASK`
- `CLEAR_INVOKED`
- `HANDOFF_TRANSFER`
- `MANUAL_RELEASE`
- `SESSION_END`

---

## 14. Orphaned Claims

When an agent disappears uncleanly, claims become `ORPHANED`.

```json
{
  "type": "CLAIM_ORPHANED",
  "claimId": "claim-123",
  "previousOwner": "agent-a",
  "orphanedAt": "2026-06-22T18:42:00.000Z",
  "ttlSeconds": 300
}
```

After TTL:

```json
{
  "type": "CLAIM_RECLAIMABLE",
  "claimId": "claim-123"
}
```

Reclaim:

```json
{
  "type": "CLAIM_RECLAIMED",
  "claimId": "claim-123",
  "previousOwner": "agent-a",
  "newOwner": "agent-b"
}
```

Manual force release:

```json
{
  "type": "CLAIM_RELEASED",
  "claimId": "claim-123",
  "reason": "USER_FORCE_RELEASE_ORPHAN"
}
```

---

## 15. Heat Schema

```json
{
  "type": "HEAT_UPDATED",
  "pair": ["agent-a", "agent-b"],
  "heat": 82,
  "heatKind": "CURRENT",
  "band": "CONFLICT_CANDIDATE",
  "components": {
    "taskSimilarity": 0.08,
    "intentSimilarity": 0.15,
    "domainOverlap": 0.15,
    "filePathOverlap": 0.18,
    "temporalActivity": 0.08,
    "branchWorktreeProximity": 0.10,
    "historicalHotzoneRisk": 0.09,
    "historicalCoupling": 0.09
  },
  "reasons": [
    "same domain: AUTHENTICATION",
    "same branch: main",
    "same file: src/auth/session.ts",
    "historical hotzone: src/auth/session.ts"
  ]
}
```

### Heat Kinds

- `PREDICTED`
- `CURRENT`

### Heat Bands

- `SAFE`
- `MONITOR`
- `OVERLAP`
- `CONFLICT_CANDIDATE`

---

## 16. Pre-Task Heat Forecast

When a claim is proposed, Core calculates predicted heat before accepting.

```json
{
  "type": "PREDICTED_HEAT_CALCULATED",
  "agentId": "agent-a",
  "proposedClaimId": "claim-proposed-123",
  "edges": [
    {
      "pair": ["agent-a", "agent-b"],
      "heat": 74,
      "band": "OVERLAP",
      "reasons": [
        "Agent B owns AUTHENTICATION",
        "src/auth/session.ts is historical hotzone",
        "same branch"
      ]
    }
  ],
  "recommendation": "SPLIT_SCOPE_OR_SEQUENCE"
}
```

Recommendations:

- `PROCEED`
- `MONITOR`
- `SPLIT_SCOPE_OR_SEQUENCE`
- `NEGOTIATE_BEFORE_CLAIM`
- `ASK_USER`

---

## 17. Conflict Schema

```json
{
  "conflictId": "conflict-123",
  "status": "DETECTED",
  "agents": ["agent-a", "agent-b"],
  "openedAt": "2026-06-22T18:42:00.000Z",
  "trigger": {
    "type": "HEAT_THRESHOLD",
    "heat": 82
  },
  "claims": ["claim-a", "claim-b"],
  "paths": ["src/auth/session.ts"],
  "domains": ["AUTHENTICATION"],
  "intents": ["BUGFIX", "REFACTOR"],
  "requiredAction": "NEGOTIATE_OR_REASSIGN"
}
```

### Conflict States

```text
NONE
DETECTED
NEGOTIATING
AWAITING_AGENT_RESPONSE
AWAITING_USER_DECISION
RESOLVED
TIMEOUT
ABORTED
```

---

## 18. Negotiation State Machine

### 18.1 Opening

When heat crosses threshold:

1. Core emits `CONFLICT_OPENED`.
2. Affected agents enter `NEGOTIATING`.
3. Core requests proposals.

### 18.2 Proposal Submission

```json
{
  "type": "CONFLICT_PROPOSAL_SUBMITTED",
  "conflictId": "conflict-123",
  "from": "agent-a",
  "proposal": {
    "kind": "SPLIT_SCOPE",
    "summary": "Agent A keeps implementation, Agent B takes tests.",
    "claimChanges": [
      {
        "agentId": "agent-a",
        "keep": ["src/auth/session.ts"]
      },
      {
        "agentId": "agent-b",
        "take": ["tests/auth/session.test.ts"]
      }
    ],
    "requiresUserApproval": false
  }
}
```

### 18.3 Proposal Kinds

- `CO_OWN`
- `SPLIT_SCOPE`
- `YIELD_CLAIM`
- `TRANSFER_TASK`
- `QUEUE_TASK`
- `ASK_USER`
- `ABORT_TASK`

### 18.4 Core Comparison

Core can auto-resolve when:

- both proposals are compatible;
- no protected path is involved;
- no user-required decision exists;
- heat after proposed change falls below threshold;
- config allows auto-resolution.

Core must escalate when:

- proposals conflict;
- both agents claim same intent/file/path;
- protected path involved;
- agents propose incompatible architecture choices;
- timeout occurs;
- deadlock exists.

### 18.5 User Arbitration

If user arbitration is required, both agents receive the same prompt:

```text
Coordify requires a user decision.

Conflict:
Agent A and Agent B both want to modify src/auth/session.ts.

Option 1:
Agent A keeps implementation, Agent B handles tests.

Option 2:
Agent B proceeds first, Agent A waits.

Option 3:
Allow co-ownership.

Choose one.
```

Both agents must present the same decision request to avoid divergent framing.

### 18.6 Timeout Behavior

Default timeouts:

| Situation | Default |
|---|---:|
| Agent proposal timeout | 60s |
| Handoff acceptance timeout | 120s |
| User arbitration timeout | no automatic resolution |
| Core degraded timeout | config-driven |
| Deadlock timeout | immediate escalation |

If an agent ignores proposal request:

- mark agent `BLOCKED` or `WAITING_USER`;
- escalate to user if other agent is waiting;
- do not silently auto-resolve high-risk conflict.

If an agent crashes during conflict:

- its claims become orphaned;
- conflict updates;
- remaining agent may proceed only after tombstone/TTL or user action.

If heat falls below threshold during negotiation:

- Core may resolve conflict as `AUTO_RESOLVED_HEAT_DROPPED`;
- agents return to prior states.

---

## 19. Coordination Escalation

```json
{
  "type": "HEAT_THRESHOLD_EXCEEDED",
  "pair": ["agent-a", "agent-b"],
  "heat": 82,
  "escalationLevel": 2,
  "requiredAction": "COORDINATE_BEFORE_WRITE"
}
```

Levels:

| Level | Name | CAP Action |
|---:|---|---|
| 0 | Observe | log only |
| 1 | Warn | emit warning context |
| 2 | Coordinate | require proposal/ack before risky write |
| 3 | Ask User | require user arbitration |
| 4 | Block | deny protected write until resolved |

---

## 20. Deadlock Detection

Core maintains wait graph.

```json
{
  "type": "DEADLOCK_DETECTED",
  "agents": ["agent-a", "agent-b"],
  "waitEdges": [
    { "from": "agent-a", "to": "agent-b", "resource": "src/auth/session.ts" },
    { "from": "agent-b", "to": "agent-a", "resource": "src/auth/tokens.ts" }
  ],
  "requiredAction": "USER_ARBITRATION"
}
```

Deadlock always escalates. Agents should not resolve cycles alone unless a clear configured rule exists.

---

## 21. Handoff Protocol

### 21.1 Handoff Proposal

```json
{
  "type": "TASK_HANDOFF_PROPOSED",
  "handoffId": "handoff-123",
  "from": "agent-a",
  "to": "agent-b",
  "taskSummary": "Update token validation for 24h expiry.",
  "intent": "BUGFIX",
  "domains": ["AUTHENTICATION"],
  "filesModified": ["src/auth/session.ts"],
  "filesInProgress": ["src/auth/tokens.ts"],
  "openQuestions": [
    "Should refresh token grace period remain 7 days?"
  ],
  "knownRisks": [
    "tests/auth/session.test.ts currently failing"
  ],
  "heatAtHandoff": 34,
  "claimTransfer": {
    "releaseFromSender": true,
    "claimForReceiver": true
  },
  "timeoutSeconds": 120
}
```

### 21.2 Handoff Acceptance

```json
{
  "type": "TASK_HANDOFF_ACCEPTED",
  "handoffId": "handoff-123",
  "to": "agent-b",
  "acceptedAt": "2026-06-22T18:42:00.000Z"
}
```

### 21.3 Handoff Rejection

```json
{
  "type": "TASK_HANDOFF_REJECTED",
  "handoffId": "handoff-123",
  "to": "agent-b",
  "reason": "AGENT_NOT_IDLE"
}
```

Rejection reasons:

- `AGENT_NOT_IDLE`
- `LOW_CONTEXT`
- `CONFLICTING_CLAIM`
- `USER_DECISION_REQUIRED`
- `TIMEOUT`
- `AGENT_OFFLINE`

---

## 22. Intent Drift Detection

```json
{
  "type": "INTENT_DRIFT_DETECTED",
  "agentId": "agent-a",
  "declaredIntent": "BUGFIX",
  "observedIntent": "REFACTOR",
  "confidence": 0.78,
  "signals": [
    "large diff size",
    "many files touched",
    "new abstraction created"
  ],
  "recommendedAction": "UPDATE_CLAIM"
}
```

MVP behavior:

- warn;
- increase heat;
- request claim update;
- do not block unless config says so.

---

## 23. Ghost Work Detection

```json
{
  "type": "GHOST_WORK_DETECTED",
  "agentId": "agent-a",
  "claimId": "claim-123",
  "durationSeconds": 1800,
  "tokensEstimated": 23000,
  "filesWritten": 0,
  "classification": "GHOST_WORK",
  "likelyCause": "VAGUE_PROMPT"
}
```

Classifications:

- `VALID_INVESTIGATION`
- `GHOST_WORK`
- `ABANDONED_WORK`
- `BLOCKED_WORK`

---

## 24. Knowledge Events

### 24.1 HOTZONE_UPDATED

```json
{
  "type": "HOTZONE_UPDATED",
  "path": "src/auth/session.ts",
  "heatEvents": 14,
  "conflicts": 6,
  "averageHeat": 78,
  "riskLevel": "HIGH"
}
```

### 24.2 COUPLING_UPDATED

```json
{
  "type": "COUPLING_UPDATED",
  "pathA": "src/api/users.ts",
  "pathB": "schema.prisma",
  "score": 0.87,
  "observations": 12
}
```

---

## 25. Tool Precheck

Before risky tool use:

```json
{
  "type": "TOOL_PRECHECK",
  "agentId": "agent-a",
  "tool": "Edit",
  "targetPaths": ["src/auth/session.ts"],
  "operationKind": "WRITE"
}
```

Core response:

```json
{
  "decision": "allow",
  "context": {
    "heat": 42,
    "warnings": []
  }
}
```

Possible decisions:

- `allow`
- `warn`
- `coordinate`
- `ask_user`
- `block`
- `core_unavailable`

---

## 26. Core Degradation Events

### 26.1 CORE_DEGRADED

```json
{
  "type": "CORE_DEGRADED",
  "reason": "IPC_UNAVAILABLE",
  "mode": "fail-safe",
  "affectedAgents": ["agent-a", "agent-b"]
}
```

### 26.2 CORE_RECOVERED

```json
{
  "type": "CORE_RECOVERED",
  "recoveredAt": "2026-06-22T18:42:00.000Z",
  "stateRebuiltFrom": "event_log"
}
```

---

## 27. Session Finalization

```json
{
  "type": "SESSION_FINALIZED",
  "sessionId": "coordify-session-abc",
  "endedAt": "2026-06-22T23:10:00.000Z",
  "agentsTotal": 5,
  "tasksCompleted": 12,
  "conflictsResolved": 4,
  "logsCompressed": true,
  "knowledgeUpdated": true
}
```

---

## 28. Error Schema

```json
{
  "type": "CAP_ERROR",
  "code": "SCHEMA_VALIDATION_FAILED",
  "message": "intent must be one of canonical values",
  "eventId": "msg-123",
  "recoverable": true
}
```

Error codes:

- `SCHEMA_VALIDATION_FAILED`
- `INVALID_STATE_TRANSITION`
- `AUTH_FAILED`
- `CLAIM_CONFLICT`
- `AGENT_NOT_FOUND`
- `CLAIM_NOT_FOUND`
- `CONFLICT_NOT_FOUND`
- `CORE_DEGRADED`
- `TIMEOUT`
- `UNSUPPORTED_CAP_VERSION`

---

## 29. Simulation Event Format

Simulation fixtures should use CAP envelopes with deterministic timestamps.

```json
{
  "fixture": "simple-conflict",
  "events": [
    {
      "at": 0,
      "event": { "type": "AGENT_JOINED", "agentId": "agent-a" }
    },
    {
      "at": 1000,
      "event": { "type": "CLAIM_PROPOSED", "agentId": "agent-a" }
    }
  ]
}
```

Simulation must produce the same state transitions as live hooks.

---

## 30. Compatibility Rules

CAP versioning:

- minor version may add optional fields;
- major version may change required fields;
- Core rejects unsupported major versions;
- adapters declare supported versions during handshake.

---

## 31. Non-Negotiable CAP Rules

- No untyped messages mutate state.
- No free-form chat mutates state.
- No agent calculates canonical heat.
- No agent writes canonical project knowledge.
- No claim exists without schema validation.
- No `/clear` leaves ownership behind.
- No unclean crash silently deletes claims.
- No negotiation can run forever without timeout or user escalation.
