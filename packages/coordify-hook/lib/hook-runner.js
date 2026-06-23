'use strict';

const fs = require('fs');
const path = require('path');
const { spawn } = require('child_process');
const paths = require('./paths');
const { emit } = require('./sidecar-client');

function readStdin() {
  return new Promise(resolve => {
    let raw = '';
    process.stdin.setEncoding('utf8');
    process.stdin.on('data', c => { raw += c; });
    process.stdin.on('end', () => resolve(raw));
    process.stdin.on('error', () => resolve(raw));
  });
}

function ensureSidecar(root, sessionId) {
  const sock = paths.sessionSock(root, sessionId);
  if (fs.existsSync(sock)) return; // best-effort; a dead socket just means emit() no-ops
  try {
    const child = spawn(
      process.execPath,
      [path.resolve(__dirname, '..', 'sidecar.js'), '--root', root, '--session', sessionId],
      { detached: true, stdio: 'ignore' }
    );
    child.unref();
  } catch (_) {}
}

// Crash-safe: always exits 0, never throws.
async function run(hook) {
  let payload = {};
  try {
    payload = JSON.parse(await readStdin());
  } catch (_) {}
  const root = payload.cwd || process.env.CLAUDE_PROJECT_DIR || process.cwd();
  const sessionId = payload.session_id || 'default';

  if (hook === 'SessionStart') ensureSidecar(root, sessionId);

  try {
    await emit(paths.sessionSock(root, sessionId), { hook, payload });
  } catch (_) {}
  process.exit(0);
}

module.exports = { run };
