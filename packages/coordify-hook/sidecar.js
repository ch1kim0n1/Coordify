'use strict';

const net = require('net');
const fs = require('fs');
const fsp = require('fs/promises');
const path = require('path');
const { spawn, execSync } = require('child_process');

const paths = require('./lib/paths');
const { CoreClient } = require('./lib/core-client');
const { mapEvent } = require('./lib/mapping');

function arg(name, def) {
  const i = process.argv.indexOf(name);
  return i >= 0 && process.argv[i + 1] ? process.argv[i + 1] : def;
}

const ROOT = path.resolve(arg('--root', process.cwd()));
const SESSION_ID = arg('--session', 'unknown');
const HEARTBEAT_MS = parseInt(process.env.COORDIFY_HEARTBEAT_MS || '3000', 10);
const BOOT_TIMEOUT_MS = parseInt(process.env.COORDIFY_BOOT_TIMEOUT_MS || '5000', 10);

const sleep = ms => new Promise(r => setTimeout(r, ms));

function diag(msg) {
  try {
    fs.appendFileSync(paths.sidecarLog(ROOT, SESSION_ID), new Date().toISOString() + ' ' + msg + '\n');
  } catch (_) {}
}

function coreBin() {
  if (process.env.COORDIFY_CORE_BIN) return process.env.COORDIFY_CORE_BIN;
  const base = path.resolve(__dirname, '..', 'coordify-core', 'target');
  for (const p of [path.join(base, 'release', 'coordify-core'), path.join(base, 'debug', 'coordify-core')]) {
    if (fs.existsSync(p)) return p;
  }
  return 'coordify-core'; // PATH fallback
}

// §8 discovery: if no live socket+token, spawn the binary (it self-arbitrates via
// its own lock — a loser exits 0) and poll until both appear.
async function ensureCore() {
  if (fs.existsSync(paths.socket(ROOT)) && fs.existsSync(paths.token(ROOT))) return;
  try {
    const child = spawn(coreBin(), ['--root', ROOT], { detached: true, stdio: 'ignore' });
    child.unref();
  } catch (e) {
    diag('spawn core failed: ' + e.message);
  }
  const deadline = Date.now() + BOOT_TIMEOUT_MS;
  while (Date.now() < deadline) {
    if (fs.existsSync(paths.socket(ROOT)) && fs.existsSync(paths.token(ROOT))) return;
    await sleep(100);
  }
  throw new Error('core socket/token did not appear within ' + BOOT_TIMEOUT_MS + 'ms');
}

function gitBranch() {
  try {
    return execSync('git rev-parse --abbrev-ref HEAD', { cwd: ROOT, stdio: ['ignore', 'pipe', 'ignore'] })
      .toString().trim() || null;
  } catch (_) { return null; }
}

async function main() {
  fs.mkdirSync(paths.agentsDir(ROOT), { recursive: true });
  await ensureCore();

  const token = (await fsp.readFile(paths.token(ROOT), 'utf8')).trim();
  const core = new CoreClient(paths.socket(ROOT), token);
  await core.connect();

  const reg = await core.register({ branch: gitBranch(), sessionId: SESSION_ID });
  if (!reg.ok) throw new Error('register failed: ' + (reg.error || '?'));
  const agentId = reg.agent_id;
  diag('registered ' + agentId);

  const liveClaims = new Set();
  const hb = setInterval(() => { core.heartbeat(agentId).catch(() => {}); }, HEARTBEAT_MS);

  const sockPath = paths.sessionSock(ROOT, SESSION_ID);
  try { fs.unlinkSync(sockPath); } catch (_) {}

  let server;
  async function shutdown() {
    clearInterval(hb);
    core.close();
    try { if (server) server.close(); } catch (_) {}
    try { fs.unlinkSync(sockPath); } catch (_) {}
    diag('shutdown');
    process.exit(0);
  }

  async function handle(msg) {
    const res = mapEvent(msg.hook, msg.payload);
    if (res.kind === 'forward') {
      const event = Object.assign({ agentId }, res.event);
      const resp = await core.submitEvent(event).catch(e => ({ ok: false, error: e.message }));
      if (resp && resp.ok && resp.data && resp.data.claimId) liveClaims.add(resp.data.claimId);
      if (resp && !resp.ok) diag('core rejected ' + res.event.type + ': ' + resp.error);
    } else if (res.kind === 'release') {
      for (const claimId of liveClaims) {
        await core.submitEvent({ type: 'CLAIM_RELEASED', claimId, agentId, reason: 'SESSION_END' }).catch(() => {});
      }
      liveClaims.clear();
      await shutdown();
    } else if (res.kind === 'record') {
      try {
        fs.appendFileSync(paths.hooktrace(ROOT, agentId),
          JSON.stringify(Object.assign({ ts: new Date().toISOString(), agentId }, res.record)) + '\n');
      } catch (_) {}
    }
    // 'bootstrap' -> nothing; the sidecar is already up.
  }

  server = net.createServer(conn => {
    conn.setEncoding('utf8');
    let buf = '';
    conn.on('data', d => {
      buf += d;
      let i;
      while ((i = buf.indexOf('\n')) >= 0) {
        const line = buf.slice(0, i); buf = buf.slice(i + 1);
        if (!line.trim()) continue;
        let msg;
        try { msg = JSON.parse(line); } catch { continue; }
        conn.write('{"ok":true}\n'); // emit-only ack; hooks ignore it
        handle(msg).catch(e => diag('handle error: ' + e.message));
      }
    });
    conn.on('error', () => {});
  });
  server.listen(sockPath, () => diag('listening ' + sockPath));

  process.on('SIGTERM', shutdown);
  process.on('SIGINT', shutdown);
}

main().catch(e => { diag('fatal: ' + e.message); process.exit(1); });
