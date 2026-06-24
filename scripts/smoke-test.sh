#!/usr/bin/env bash
# Coordify clean-machine smoke test.
#
# Verifies the published install path works end-to-end on a fresh-ish machine.
# Run on macOS and Linux before each release (see RELEASE.md step 1).
#
# Usage: ./scripts/smoke-test.sh
# Exit code: 0 = pass, non-zero = fail.
set -euo pipefail

fail() { echo "FAIL: $*" >&2; exit 1; }
ok()   { echo "ok: $*"; }

echo "=== Coordify smoke test ==="

# 1. Toolchain present
command -v cargo   >/dev/null || fail "cargo not found; install Rust via rustup"
command -v node    >/dev/null || fail "node not found; install Node >=18"
command -v npm     >/dev/null || fail "npm not found"
node --version | grep -qE '^v(1[8-9]|2[0-9])\.' || fail "node >=18 required, got $(node --version)"
ok "toolchain present"

# 2. Uninstall any prior Coordify (idempotent)
cargo uninstall coordify-core 2>/dev/null || true
npm uninstall -g coordify-hook coordify-cli coordify-sim 2>/dev/null || true
ok "prior install removed (if any)"

# 3. Install from source (simulates `cargo install coordify-core` + `npm i -g`)
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
echo "--- building coordify-core ---"
cargo install --path "$ROOT/packages/coordify-core" --quiet
ok "coordify-core installed"

echo "--- building npm packages ---"
for p in coordify-hook coordify-cli coordify-sim; do
  echo "  $p: ci"
  (cd "$ROOT/packages/$p" && npm ci) || fail "$p ci failed"
  # coordify-hook has no build step (pure JS); cli + sim need tsc.
  if [ "$p" != "coordify-hook" ]; then
    echo "  $p: build"
    (cd "$ROOT/packages/$p" && npm run build) || fail "$p build failed"
  fi
  echo "  $p: install -g"
  npm install -g "$ROOT/packages/$p" || fail "$p install failed"
  ok "$p installed"
done

# 4. Binaries on PATH
command -v coordify-core >/dev/null || fail "coordify-core not on PATH after install"
command -v coordify      >/dev/null || fail "coordify not on PATH after install"
command -v coordify-sim  >/dev/null || fail "coordify-sim not on PATH after install"
ok "binaries on PATH"

# 5. Versions match
CORE_VER="$(coordify-core --version 2>&1 | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1)"
CLI_VER="$(coordify --version 2>&1 | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1)"
SIM_VER="$(coordify-sim --version 2>&1 | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1)"
[ -n "$CORE_VER" ] || fail "coordify-core --version empty"
[ "$CORE_VER" = "$CLI_VER" ] || fail "version mismatch: core=$CORE_VER cli=$CLI_VER"
[ "$CORE_VER" = "$SIM_VER" ] || fail "version mismatch: core=$CORE_VER sim=$SIM_VER"
ok "versions consistent: $CORE_VER"

# 6. CLI empty states (no Core running)
OUT="$(coordify status 2>&1)"
echo "$OUT" | grep -qi "not running\|open a Claude Code session" || fail "status empty state wrong: $OUT"
ok "status empty state"

OUT="$(coordify agents 2>&1)"
echo "$OUT" | grep -qi "not running\|open a Claude Code session" || fail "agents empty state wrong: $OUT"
ok "agents empty state"

# 7. Sim scenarios pass (each in a fresh temp root so Core starts clean)
echo "--- running sim scenarios ---"
for s in "$ROOT"/packages/coordify-sim/scenarios/*.json; do
  name="$(basename "$s")"
  simroot="$(mktemp -d /tmp/coordify-sim-XXXXXX)"
  if coordify-sim simulate "$s" --root "$simroot" >/tmp/coordify-smoke-"$name".log 2>&1; then
    ok "scenario $name"
  else
    fail "scenario $name failed (see /tmp/coordify-smoke-$name.log)"
  fi
  rm -rf "$simroot"
done

# 8. Core starts + responds
TMPROOT="$(mktemp -d /tmp/coordify-smoke-XXXXXX)"
trap 'rm -rf "$TMPROOT"' EXIT
echo "--- starting Core in $TMPROOT ---"
coordify-core --root "$TMPROOT" &
CORE_PID=$!
sleep 1
kill -0 "$CORE_PID" 2>/dev/null || fail "Core died on startup"
ok "Core started"

# Core should have created socket + token
[ -S "$TMPROOT/.coordify/runtime/core.sock" ] || fail "socket missing"
[ -f "$TMPROOT/.coordify/runtime/session.token" ] || fail "token missing"
ok "socket + token present"

# Status should now show live (no agents)
OUT="$(coordify --root "$TMPROOT" status 2>&1)"
echo "$OUT" | grep -qi "live" || fail "status should be live, got: $OUT"
ok "status live"

# Cleanup
kill "$CORE_PID" 2>/dev/null || true
wait "$CORE_PID" 2>/dev/null || true

echo ""
echo "=== ALL SMOKE TESTS PASSED ==="
