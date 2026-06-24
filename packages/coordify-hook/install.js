'use strict';

const fs = require('fs');
const path = require('path');
const { execFileSync } = require('child_process');

const SETTINGS_PATH = path.resolve(process.cwd(), '.claude', 'settings.json');
const BACKUP_PATH = SETTINGS_PATH + '.backup';
// Absolute path so hooks work regardless of CWD at invocation time.
const HOOKS_DIR = path.resolve(__dirname, 'hooks');
const NODE_BIN = process.execPath;

// The seven Claude Code hook events Coordify wires. Re-running install only
// rewrites these keys; any other hook keys the user configured are preserved.
const COORDIFY_HOOK_EVENTS = [
  'SessionStart',
  'UserPromptSubmit',
  'PreToolUse',
  'PostToolUse',
  'SubagentStart',
  'SubagentStop',
  'SessionEnd',
];

// Marker present in every command Coordify writes, so we can recognize (and
// replace) our own entries without touching foreign hook entries.
function isCoordifyEntry(entry) {
  const cmd = entry && entry.hooks && entry.hooks[0] && entry.hooks[0].command;
  return typeof cmd === 'string' && cmd.includes('coordify-hook/hooks/');
}

function hookEntry(file) {
  return {
    matcher: '',
    hooks: [{ type: 'command', command: JSON.stringify(NODE_BIN) + ' ' + JSON.stringify(path.join(HOOKS_DIR, file)) }],
  };
}

// Fail loudly if coordify-core is not on PATH instead of silently wiring a
// broken hook. `COORDIFY_SKIP_CORE_CHECK=1` exists only for the install test
// harness; real installs always perform the check.
function ensureCoreOnPath() {
  if (process.env.COORDIFY_SKIP_CORE_CHECK === '1') return;
  let missing = false;
  try {
    if (process.platform === 'win32') {
      execFileSync('where', ['coordify-core'], { stdio: 'ignore' });
    } else {
      execFileSync('command', ['-v', 'coordify-core'], { stdio: 'ignore', shell: true });
    }
  } catch (_) {
    missing = true;
  }
  if (missing) {
    console.error('coordify-core is not on PATH. Install it first with `cargo install coordify-core`, then re-run install.js.');
    process.exit(1);
  }
}

function run() {
  ensureCoreOnPath();

  // Back up the user's pre-Coordify settings exactly once. We never overwrite a
  // backup that already exists, so re-running install does not clobber the
  // original with our own output.
  if (fs.existsSync(SETTINGS_PATH) && !fs.existsSync(BACKUP_PATH)) {
    fs.copyFileSync(SETTINGS_PATH, BACKUP_PATH);
    console.log('Backed up existing settings to ' + BACKUP_PATH);
  }

  let settings = {};
  if (fs.existsSync(SETTINGS_PATH)) {
    try { settings = JSON.parse(fs.readFileSync(SETTINGS_PATH, 'utf8')); }
    catch (_) { console.warn('Could not parse existing settings.json — starting fresh'); }
  }

  // Merge: preserve foreign hook keys + foreign entries under shared event keys,
  // replace only Coordify's own entries. Idempotent — a second run produces the
  // same bytes because we drop prior Coordify entries before re-adding them.
  const existingHooks = settings.hooks && typeof settings.hooks === 'object' ? settings.hooks : {};
  const merged = {};
  // Keep foreign entries for every event the user already configured.
  for (const event of Object.keys(existingHooks)) {
    const entries = Array.isArray(existingHooks[event]) ? existingHooks[event] : [];
    const kept = entries.filter(e => !isCoordifyEntry(e));
    if (kept.length) merged[event] = kept;
  }
  // Append Coordify's entry to its seven events (after any foreign entries).
  for (const event of COORDIFY_HOOK_EVENTS) {
    const file = event.replace(/([a-z])([A-Z])/g, (_, a, b) => a + '-' + b.toLowerCase()).toLowerCase() + '.js';
    const prior = merged[event] || [];
    merged[event] = prior.concat(hookEntry(file));
  }
  settings.hooks = merged;

  fs.mkdirSync(path.dirname(SETTINGS_PATH), { recursive: true });
  fs.writeFileSync(SETTINGS_PATH, JSON.stringify(settings, null, 2));
  console.log('Coordify hook configuration written to ' + SETTINGS_PATH);
}

run();
