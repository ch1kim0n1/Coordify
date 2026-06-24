'use strict';
const test = require('node:test');
const assert = require('node:assert');
const os = require('os');
const fs = require('fs');
const path = require('path');
const { spawn } = require('child_process');
const paths = require('../lib/paths');
const { emit } = require('../lib/sidecar-client');

const sleep = ms => new Promise(r => setTimeout(r, ms));

function coreBin() {
  if (process.env.COORDIFY_CORE_BIN) return process.env.COORDIFY_CORE_BIN;
  const base = path.resolve(__dirname, '..', '..', 'coordify-core', 'target');
  for (const p of [path.join(base, 'debug', 'coordify-core'), path.join(base, 'release', 'coordify-core')]) {
    if (fs.existsSync(p)) return p;
  }
  return null;
}

async function waitFor(predicate, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (predicate()) return true;
    await sleep(50);
  }
  return false;
}

function readEventsLog(root) {
  const sessions = paths.coordify(root) + '/sessions';
  let out = '';
  try {
    for (const d of fs.readdirSync(sessions)) {
      const f = path.join(sessions, d, 'events.log');
      if (fs.existsSync(f)) out = fs.readFileSync(f, 'utf8');
    }
  } catch (_) {}
  return out;
}

test('full hook pipe registers, claims, state-changes, releases', async (t) => {
  const bin = coreBin();
  if (!bin) { t.skip('coordify-core binary not built (run: cd packages/coordify-core && cargo build)'); return; }

  const root = fs.mkdtempSync('/tmp/cc-int-');
  const SESSION = 's1';
  const sockPath = paths.sessionSock(root, SESSION);

  // Spawn the sidecar; it boots Core itself via COORDIFY_CORE_BIN.
  const side = spawn(process.execPath,
    [path.resolve(__dirname, '..', 'sidecar.js'), '--root', root, '--session', SESSION],
    { env: Object.assign({}, process.env, { COORDIFY_CORE_BIN: bin }), stdio: 'ignore' });

  try {
    const up = await waitFor(() => fs.existsSync(sockPath), 8000);
    assert.ok(up, 'sidecar session socket should appear');

    await emit(sockPath, { hook: 'SessionStart', payload: { source: 'startup', cwd: root, session_id: SESSION } });
    await emit(sockPath, { hook: 'UserPromptSubmit', payload: { prompt: 'fix the session expiry bug', cwd: root, session_id: SESSION } });
    await emit(sockPath, { hook: 'PostToolUse', payload: { tool_name: 'Write', tool_input: { file_path: 'src/auth/session.ts' }, cwd: root, session_id: SESSION } });
    await emit(sockPath, { hook: 'PostToolUse', payload: { tool_name: 'Read', tool_input: { file_path: 'src/auth/tokens.ts' }, cwd: root, session_id: SESSION } });
    await emit(sockPath, { hook: 'SubagentStart', payload: { cwd: root, session_id: SESSION } });
    await emit(sockPath, { hook: 'SubagentStop', payload: { cwd: root, session_id: SESSION } });

    const claimed = await waitFor(() => readEventsLog(root).includes('CLAIM_CREATED'), 5000);
    assert.ok(claimed, 'CLAIM_CREATED should be logged:\n' + readEventsLog(root));

    const log = readEventsLog(root);
    assert.ok(log.includes('AGENT_JOINED'), 'agent registered');
    assert.ok(log.includes('AGENT_STATE_CHANGED'), 'subagent state change forwarded');

    // PostToolUse(Write) now forwards FILE_TOUCHED to Core (was recorded-only).
    const fwdLog = await waitFor(() => readEventsLog(root).includes('FILE_TOUCHED'), 5000);
    assert.ok(fwdLog, 'FILE_TOUCHED forwarded to Core log:\n' + readEventsLog(root));
    assert.ok(readEventsLog(root).includes('src/auth/session.ts'), 'touched file in Core log');

    // PostToolUse(Read) stays recorded-only: it lands in the hooktrace, NOT Core's log.
    const traceFiles = fs.readdirSync(paths.agentsDir(root)).filter(f => f.endsWith('.hooktrace.jsonl'));
    assert.ok(traceFiles.length >= 1, 'a hooktrace file exists (from the recorded-only Read)');
    const trace = fs.readFileSync(path.join(paths.agentsDir(root), traceFiles[0]), 'utf8');
    assert.ok(trace.includes('FILE_READ'), 'recorded-only FILE_READ in trace');
    assert.ok(!readEventsLog(root).includes('SCHEMA_VALIDATION_FAILED'), 'no rejected events in Core log');

    // SessionEnd releases + disconnects; with the last agent gone Core finalizes.
    await emit(sockPath, { hook: 'SessionEnd', payload: { reason: 'other', cwd: root, session_id: SESSION } });
    const released = await waitFor(() => readEventsLog(root).includes('AGENT_LEFT'), 5000);
    assert.ok(released, 'AGENT_LEFT after SessionEnd:\n' + readEventsLog(root));
  } finally {
    try { side.kill(); } catch (_) {}
    // Core exits on its own when the last agent leaves; best-effort cleanup.
    try { fs.rmSync(root, { recursive: true, force: true }); } catch (_) {}
  }
});
