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

### Changed

- README "Configuration" section rewritten: `coordify.yaml` is not implemented
  in 0.1.0. Defaults (heat bands, claim thresholds, orphan TTL, knowledge
  weights) are now documented as compiled-in, with the `COORDIFY_ORPHAN_TTL_MS`
  env override called out. A `coordify.yaml` config file is on the roadmap for 0.2.
- README "Crash Handling" and "Troubleshooting" no longer reference
  `coordify claim release --orphaned` (no `claim` subcommand exists in 0.1.0).
  Orphan reclaim is TTL-only; a manual release command is planned for 0.2.
- Storage layout no longer lists a `config/coordify.yaml` file.
- Badge URLs normalized to `ch1kim0n1/Coordify` (matching the repo name).
- README "Install" section now documents that CI is ubuntu-only and macOS is
  verified manually before each release.

### Added (docs + community)

- `CONTRIBUTING.md` — dev setup, test/lint/coverage requirements, PR checklist,
  commit style, scope guardrails (no telemetry/cloud/new deps without issue).
- `CODE_OF_CONDUCT.md` — Contributor Covenant 2.1.
- `.github/ISSUE_TEMPLATE/` — bug report, feature request, and config.yml
  (redirects security reports to private advisories, questions to Discussions).
- `.github/PULL_REQUEST_TEMPLATE.md` — type/scope/checklist template.
- `README.md` "Roadmap" section listing post-0.1.0 priorities.

### Changed (docs + community)

- `SECURITY.md` "Reporting a vulnerability" section rewritten: the only
  private channel is a draft GitHub Security Advisory. No email address is
  published. Added required report contents and coordinated-disclosure note.
- `phase-0/`, `absolute-docs/`, `docs/superpowers/`, and root
  `TECHNICAL_VALIDATION.md` moved under `docs/internal/` to keep the repo root
  focused on user-facing docs. History preserved via `git mv`.
