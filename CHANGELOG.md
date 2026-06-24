# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-06-24

### Added

- **coordify-core** — Rust daemon implementing the CAP (Coordinated Agent Protocol) over Unix sockets. Manages agent registration, ownership claims, heat scoring, conflict resolution, knowledge graph, and session persistence.
- **coordify-cli** — TypeScript CLI (`coordify`) with commands: `status`, `agents`, `heat`, `claims`, `conflicts`, `graph`, `watch` (live TUI), `logs`, `stats`, `session list`, `session inspect`.
- **coordify-sim** — Scenario runner (`coordify-sim simulate`) and session replayer (`coordify-sim replay`) for testing and debugging multi-agent coordination without live Claude Code sessions.
- **coordify-hook** — Node.js hook adapter that bridges Claude Code hooks (SessionStart, UserPromptSubmit, PreToolUse, PostToolUse, SubagentStart, SubagentStop, SessionEnd) to coordify-core via a per-session sidecar process.
- `install.js` — one-command hook wiring for Claude Code projects.
- FILE_TOUCHED ingestion via PostToolUse(Write/Edit) hook.
- macOS Unix socket path length fix — session sockets moved to `$TMPDIR` to stay under the 104-byte limit.
