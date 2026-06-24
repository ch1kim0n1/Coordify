# Coordify Quickstart

Get Coordify running in under 5 minutes. macOS or Linux. Node ≥ 18, Rust stable.

---

## 1. Install

### Option A — Published packages (fastest, 3 commands)

```bash
cargo install coordify-core
npm install -g coordify-hook coordify-cli coordify-sim
```

### Option B — From the GitHub repo (no publish needed)

```bash
git clone https://github.com/ch1kim0n1/Coordify.git
cd Coordify

# Core (Rust daemon)
cargo install --path packages/coordify-core

# Hook adapter (pure JS, no build)
npm install -g packages/coordify-hook

# Optional: CLI + sim (need build)
for p in coordify-cli coordify-sim; do
  (cd packages/$p && npm ci && npm run build && npm install -g .)
done
```

Verify the install:

```bash
coordify-core --version    # → coordify-core 0.1.0
coordify --version         # → coordify 0.1.0
coordify-sim --version     # → coordify-sim 0.1.0
```

---

## 2. Wire hooks into your project

Run this **once per project** from the project root:

```bash
node "$(npm root -g)/coordify-hook/install.js"
```

This adds Coordify hooks to `.claude/settings.json` and backs up the existing
file to `.claude/settings.json.backup`. It fails loudly if `coordify-core` is
not on PATH.

To uninstall the hooks later:

```bash
rm -rf .claude/settings.json.backup
# Restore your pre-Coordify settings:
mv .claude/settings.json.backup .claude/settings.json
```

---

## 3. Start coordinating

Open two or more Claude Code terminals in the **same project**:

```bash
cd my-project
claude    # terminal 1 — agent A
claude    # terminal 2 — agent B
```

The first session auto-starts Coordify Core in the background. Each session
registers as an agent and begins emitting ownership claims and heat.

---

## 4. Watch the network

From any terminal:

```bash
# Live overview
coordify status

# Agent list + states
coordify agents

# Heat between agent pairs
coordify heat

# Active claims
coordify claims

# Active conflicts
coordify conflicts

# Live TUI dashboard
coordify watch

# Event log (last 20 lines)
coordify logs
```

Example `coordify status` output with 2 agents:

```
status: live
agents: 2
claims: 2
conflicts: 0
peak heat: agent-1 ↔ agent-2 42 MONITOR
```

---

## 5. After the session

When all Claude Code sessions close, Core finalizes the session and writes
stats under `.coordify/sessions/`. Review past sessions:

```bash
coordify session list
coordify session inspect <session-id>
coordify stats
```

---

## Troubleshooting

**`coordify-core is not on PATH`** — Re-run `cargo install coordify-core` (or
`cargo install --path packages/coordify-core` from the repo). Verify with
`which coordify-core`.

**`coordify status` says "not running"** — Open a Claude Code session in the
project. Core starts automatically on the first `SessionStart` hook. If it
does not, check that `install.js` ran and `.claude/settings.json` has the
Coordify hooks.

**Socket path too long on macOS** — macOS limits Unix socket paths to 104
bytes. If your project path is deep, Coordify falls back to `$TMPDIR` for the
session socket. No action needed.

**Stale lock / "Core already running"** — If Core crashed without cleanup,
remove the stale lock: `rm -f .coordify/runtime/core.lock`. Then open a new
Claude Code session.

**Hook not firing** — Verify `.claude/settings.json` contains Coordify hook
entries. Re-run `install.js`. Check `node "$(npm root -g)/coordify-hook/sidecar.js"
--root . --session test` for errors.

---

## What Coordify does not do

- No cloud. No telemetry. No account. Zero outbound network calls.
- No billing. MIT-licensed, free forever.
- No Windows in 0.1.0 (macOS + Linux only).
- Does not read your file contents — only file paths from tool events.

See [README.md](README.md) for the full architecture and [SECURITY.md](SECURITY.md)
for the threat model.
