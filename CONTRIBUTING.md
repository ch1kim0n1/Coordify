# Contributing to Coordify

Thanks for considering a contribution. Coordify is a small, local-first tool
and we want to keep it that way — minimal dependencies, no telemetry, no
cloud. The bar for new code is: tested, linted, and does not break the
local-first contract.

## Project layout

```
packages/
  coordify-core/   # Rust daemon — the source of truth (CAP, heat, claims, knowledge)
  coordify-hook/   # Node.js hook adapter bridging Claude Code hooks to Core
  coordify-cli/    # TypeScript CLI + live TUI
  coordify-sim/    # Simulation runner + replay TUI

docs/internal/     # Historical design docs, phase-0 research, SDD scratch (read-only reference)
scripts/           # Release + smoke-test helpers
```

`docs/internal/` is archived material from the build. It is useful background
but is not authoritative — the code is. Do not edit it as part of a change.

## Prerequisites

- Rust stable (latest stable toolchain)
- Node ≥ 18, npm
- macOS or Linux. Windows is not supported in 0.1.0.

## Development setup

```bash
git clone https://github.com/ch1kim0n1/Coordify.git
cd Coordify

# Core (Rust)
cd packages/coordify-core
cargo build
cargo test

# Hook (pure JS, no build step)
cd ../coordify-hook
npm ci
node --test test/

# CLI + Sim (TypeScript, build step required)
cd ../coordify-cli
npm ci
npm run build
npm test

cd ../coordify-sim
npm ci
npm run build
npm test
```

## Before you submit a change

### Core (Rust)

All of these must pass. CI enforces them; do not push code that fails any.

```bash
cd packages/coordify-core
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo llvm-cov --fail-under-lines 90 -- --test-threads=4   # coverage gate
cargo audit --deny warnings                                  # advisories
cargo deny check                                             # advisories + licenses + bans
```

Coverage floor is currently 90%. Do not lower it to make a build pass — add
tests instead.

### Hook / CLI / Sim (TypeScript)

```bash
cd packages/<pkg>
npm ci
npm run build    # cli + sim only; hook has no build step
npm test
npm audit --omit=dev --audit-level=high
```

### Simulation scenarios (Core changes only)

If you touch Core, run the full sim suite. All scenarios must pass:

```bash
cd packages/coordify-sim
npm run build
for s in scenarios/*.json; do
  echo "=== $s ==="
  node dist/cli.js simulate "$s" || { echo "FAIL: $s"; exit 1; }
done
```

## What we welcome

- **Bug fixes** with a failing fixture or test that reproduces the issue, plus
  the fix that turns it green.
- **New sim scenarios** for edge cases not currently covered.
- **Codex CLI adapter** — the highest-priority post-MVP item. Open an issue
  first to coordinate design.
- **Documentation improvements** — accuracy matters more than volume. If the
  README or QUICKSTART claims something the code does not do, fixing the doc
  (or the code) is a valuable contribution.

## What needs discussion first

Open an issue **before** starting work on:

- Changes to Core architecture or the CAP protocol schema
- New runtime dependencies (Rust or npm). The dependency tree is intentionally
  minimal; additions need a reason.
- Anything that introduces an outbound network call. Coordify is local-first
  and zero-outbound by contract. This is non-negotiable.
- Anything that adds telemetry, analytics, or an account requirement.

## Pull request checklist

- [ ] Branch is up to date with `main`
- [ ] `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test` pass (Core)
- [ ] `npm run build` + `npm test` pass (TS packages)
- [ ] Coverage did not drop below 90% (Core)
- [ ] No new advisories from `cargo audit` / `npm audit`
- [ ] No new outbound network calls, telemetry, or account requirements
- [ ] No secrets, tokens, or personal paths committed
- [ ] README / QUICKSTART / CHANGELOG updated if user-visible behavior changed
- [ ] CHANGELOG entry under `## [Unreleased]` (or the next version section)

## Commit messages

Follow the existing style — `type(scope): summary`:

```
feat(heat): add branch proximity weight
fix(server): reject absolute paths in claim file lists
docs(readme): correct install commands
chore(deps): bump serde to 1.0.210
```

Keep the summary under 72 characters. Body explains *why*, not *what*.

## Versioning

Coordify follows [Semantic Versioning](https://semver.org/). All four packages
(`coordify-core`, `coordify-hook`, `coordify-cli`, `coordify-sim`) version in
lockstep. See [RELEASE.md](RELEASE.md) for the release runbook.

## Licensing

By contributing, you agree that your contributions are licensed under the MIT
License that covers this project. No CLA is required.

## Questions

Open a GitHub issue with the `question` label. For security reports, see
[SECURITY.md](SECURITY.md) — do **not** open a public issue for security
vulnerabilities.
