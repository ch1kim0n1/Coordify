# Coordify — POST-MVP.md

**Status:** Draft v0.1  
**Date:** 2026-06-22  
**Purpose:** Capture intentionally deferred ideas without polluting MVP scope.

---

## 1. Post-MVP Principle

The MVP is:

- Claude Code CLI only;
- local machine only;
- same root folder only;
- CLI/TUI only;
- Coordify Core + CAP + hooks + heat + ownership + conflict handling + project intelligence.

Everything in this file is valuable but deferred.

This keeps `VISION.md` sharp and implementation realistic.

---

## 2. Non-MVP Agent Runtime Support

### 2.1 Codex CLI Adapter

Support terminal-based Codex agents joining CAP.

Requirements:

- adapter for Codex lifecycle;
- prompt observation equivalent;
- tool/write interception equivalent;
- identity mapping;
- CAP schema compatibility;
- degraded behavior if hooks are weaker than Claude Code.

### 2.2 Devin Adapter

Support Devin-like agents if they expose a local/session API.

Likely requires:

- API bridge;
- task state mapping;
- file activity observation;
- claim/handoff compatibility.

### 2.3 Open-Source CLI Agent Adapters

Potential targets:

- Aider;
- OpenHands;
- custom LangGraph CLIs;
- custom agent shells.

Each adapter should implement CAP without changing Core.

### 2.4 Runtime-Agnostic Agent Model

Future schema extension:

```yaml
runtime:
  type: claude_code | codex | devin | aider | custom
  adapterVersion: 0.1.0
  capabilities:
    canBlockWrite: true
    canObservePrompt: true
    canTrackSubagents: false
```

---

## 3. Distributed Coordify

MVP is local-only.

Post-MVP could support:

- laptop + desktop;
- multiple developer machines;
- cloud VM agents;
- remote worktrees;
- distributed sockets via relay;
- secure LAN mode;
- encrypted agent communication.

Hard problems:

- authentication;
- authorization;
- clock skew;
- network partitions;
- conflict consistency;
- latency;
- cross-machine file path mapping;
- multi-user security.

Do not attempt in MVP.

---

## 4. Multi-Repo Coordination

MVP network = same project root.

Post-MVP:

- coordinate agents across service repos;
- backend/frontend/shared SDK repositories;
- monolith + infra repo;
- dependency-aware multi-repo heat.

Example:

```text
api-service
web-app
shared-types
infra
```

If agents touch coupled repos, Coordify can calculate cross-repo heat.

---

## 5. Workspace-Scoped Monorepo Networks

MVP same root can be too broad for large monorepos.

Post-MVP should support:

```bash
coordify init --scope packages/auth
```

or config:

```yaml
network:
  scope: packages/auth
```

Modes:

- root-wide network;
- package-level network;
- workspace-level network;
- custom domain-defined network.

Risk:

Too much scoping can hide real coupling. Use coupling graph to warn when scoped networks still overlap.

---

## 6. Empirical Heat Calibration

Feature formerly called Merge Conflict Correlation.

Coordify can compare historical heat with actual Git outcomes:

- merge conflicts;
- reverted commits;
- failed merges;
- repeated file edits;
- follow-up bugfixes;
- conflict-heavy PRs.

Goal:

```text
Heat above 70% in this repo has historically led to merge conflicts 81% of the time.
```

Why deferred:

- requires Git history analysis;
- requires branch/merge-base handling;
- correlation can be misleading;
- not needed for MVP heat utility.

---

## 7. Historical Task Decomposition Advisor

When the user gives a broad prompt:

```text
Refactor the auth system.
```

Coordify can use historical project intelligence to suggest task splits:

```text
Last sessions touching AUTHENTICATION split into:
- Token validation
- Session management
- Middleware updates
- Tests
```

Potential output:

```text
Suggested claim:
Start with Token Validation.
Predicted heat: low.
Estimated files:
- src/auth/tokens.ts
- tests/auth/tokens.test.ts
```

Why deferred:

- depends on mature hotzone/coupling/task history;
- risks fake-smart advice early;
- shifts Coordify toward planning assistant.

---

## 8. Work Stealing

When an agent becomes idle and another has backlog, Coordify could suggest transferable subtasks.

MVP supports explicit handoff only.

Post-MVP work stealing could:

- detect idle agent;
- detect overloaded agent;
- identify independent subtask;
- propose transfer;
- require acceptance.

Do not make agents silently steal work.

---

## 9. Autonomous Task Queue Optimization

Future Coordify could manage per-agent queues:

- current task;
- queued tasks;
- blocked tasks;
- delegated tasks;
- suggested next work.

This moves toward scheduling. Keep out of MVP.

---

## 10. Advanced Dashboard

MVP has CLI/TUI only.

Post-MVP may add:

- local web dashboard;
- session replay UI;
- heat timeline;
- agent graph;
- task graph;
- hotzone explorer;
- coupling graph viewer;
- stats leaderboard.

Risks:

- scope creep;
- front-end maintenance;
- distracts from CLI-first value.

---

## 11. Cross-Session Trend Analytics

Future analytics:

- weekly coordination overhead;
- recurring hotzones;
- agent effectiveness trends;
- prompt quality trends;
- domain conflict trends;
- “most conflict-prone files this month”;
- “most improved domain”;
- “token burn by prompt type”.

This extends persistent project intelligence.

---

## 12. Advanced Entertainment Metrics

MVP keeps entertainment metrics, but advanced ones can come later:

- badges;
- session recap cards;
- shareable terminal summaries;
- heat story visualization;
- “Agent of the Session”;
- achievement-style milestones;
- progress streaks across sessions.

Keep lightweight in MVP. Expand later if users love it.

---

## 13. OpenClaw Integration

Explicitly not MVP.

Post-MVP possibility:

- Coordify becomes a coordination substrate for OpenClaw agents;
- OpenClaw agents speak CAP;
- project-level intelligence feeds broader agent runtime;
- multi-tool agent mesh.

Do not mention OpenClaw in MVP docs except as deferred.

---

## 14. Cloud Sync

Potential future:

- sync `.coordify/knowledge` across machines;
- encrypted backup;
- team-shared project intelligence;
- opt-in remote storage.

This introduces security/privacy/product complexity. Not MVP.

---

## 15. Team/Multi-User Mode

MVP assumes one local user.

Post-MVP team mode requires:

- user identity;
- authorization;
- per-user agent attribution;
- shared trust model;
- audit trails;
- signed CAP events;
- team config;
- repo policy integration.

Not MVP.

---

## 16. Signed CAP Events

For stronger trust in distributed/team mode:

- public/private key identity;
- signed events;
- event verification;
- tamper-evident logs.

This is unnecessary for same-user local MVP but relevant for distributed/team support.

---

## 17. Function-Level or Line-Level Ownership

MVP ownership layers are:

- task;
- domain;
- file/path.

Post-MVP may add:

- symbol ownership;
- function ownership;
- line-range ownership;
- AST-aware conflict detection.

This requires language-aware analysis and should wait.

---

## 18. Static Analysis Integration

Future heat could use:

- import graphs;
- call graphs;
- dependency graphs;
- test coverage maps;
- ownership from CODEOWNERS;
- package manager workspaces.

Behavioral coupling remains core, but static analysis can improve predictions.

---

## 19. IDE Integrations

Potential integrations:

- VS Code extension;
- Cursor extension;
- JetBrains plugin;
- terminal panel;
- file gutter heat indicators.

Do not start here. CLI first.

---

## 20. Marketplace Strategy

Future packaging:

```bash
/plugin marketplace add owner/coordify
/plugin install coordify
```

or equivalent Claude Code plugin distribution if supported.

Requires:

- stable MVP;
- docs;
- examples;
- security review;
- clean install/uninstall;
- minimal config.

---

## 21. Advanced Replay

MVP can inspect/replay logs in CLI.

Post-MVP advanced replay:

- full timeline playback;
- heat animation;
- conflict reconstruction;
- handoff timeline;
- agent decision trails;
- diff overlays.

Useful for debugging and demos.

---

## 22. Privacy Controls

For persistent knowledge and stats, future versions may need:

- redact file content from logs;
- hash paths;
- disable token metrics;
- disable prompt recording;
- per-project privacy profiles;
- enterprise mode.

MVP should already avoid storing unnecessary raw secrets, but advanced privacy controls can expand later.

---

## 23. Summary of Deferred Features

| Feature | Reason Deferred |
|---|---|
| Codex/Devin adapters | MVP focuses on Claude Code. |
| Distributed mesh | Security/consistency complexity. |
| Multi-repo | Requires broader dependency modeling. |
| Workspace monorepo scope | Useful but not MVP-critical. |
| Merge conflict correlation | Requires Git outcome calibration. |
| Task decomposition advisor | Needs mature history first. |
| Work stealing | Moves toward scheduler. |
| Web dashboard | Scope creep. |
| OpenClaw integration | Not MVP goal. |
| Team mode | Requires identity/auth model. |
| Signed events | Needed later, not local MVP. |
| Function/line ownership | Requires language analysis. |

---

## 24. Rule for Moving Items Into MVP

A post-MVP feature can move into main scope only if it:

1. directly improves Claude Code local coordination;
2. does not require distributed identity;
3. does not require a web dashboard;
4. does not turn Coordify into an orchestrator;
5. can be tested through CAP simulation;
6. strengthens ownership, heat, conflict handling, handoff, logging, or project intelligence.

If it does not pass that test, keep it here.
