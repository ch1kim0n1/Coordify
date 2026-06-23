# Node Hook Adapter Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A per-session Node sidecar + thin hook clients that connect a live Claude Code session to Coordify Core, emit-only, translating §6 hooks into CAP events.

**Architecture:** Ephemeral Claude Code hooks each push one JSON line to a long-lived per-session sidecar over a Unix socket. The sidecar owns the single persistent Core connection, registers the agent once, heartbeats, and forwards the CAP events Core ingests (records the rest locally). Core is not modified.

**Tech Stack:** Node.js (CommonJS), standard library only (`net`, `child_process`, `fs`, `path`, `crypto`). Tests use `node:test` + `node:assert`. No runtime or dev dependencies.

## Global Constraints

- New package lives at `packages/coordify-hook/`. CommonJS (`require`/`module.exports`), matching `phase-0/`.
- ZERO dependencies — Node stdlib only. `package.json` has no `dependencies`/`devDependencies`.
- Core is NOT modified by this work.
- Hooks are emit-only and crash-safe: never block, never throw, ALWAYS `process.exit(0)`. A hook failure must never break the user's Claude session.
- `lib/mapping.js` is pure and deterministic (same payload → same result); no IO, no clock, no randomness inside it.
- Core's socket protocol: newline-delimited JSON. Request `{id, token, action, agent_id?, meta?, event?, capVersion?}`; response `{id, ok, agent_id?, error?, data?}`. Actions: `register`, `heartbeat`, `submit_event`. Core ingests only these CAP event types: `CLAIM_PROPOSED`, `CLAIM_RELEASED`, `AGENT_STATE_CHANGED`, `CLEAR_INVOKED`. `capVersion` must be `"0.1"` on `submit_event`.
- Per-session socket filename uses a short hash of `session_id` (macOS caps Unix socket paths at ~104 bytes).
- Run all commands from the repo root unless noted. Node test command: `node --test packages/coordify-hook/`.

---

### Task 1: Pure foundation — paths + mapping + unit tests

**Files:**
- Create: `packages/coordify-hook/package.json`
- Create: `packages/coordify-hook/lib/paths.js`
- Create: `packages/coordify-hook/lib/mapping.js`
- Test: `packages/coordify-hook/test/mapping.test.js`

**Interfaces:**
- Produces:
  - `paths`: `socket(root)`, `lock(root)`, `token(root)`, `agentsDir(root)`, `sessionSock(root, sessionId)` (short-hashed filename), `hooktrace(root, agentId)`, `sidecarLog(root, sessionId)` — all return absolute path strings.
  - `mapping.mapEvent(hook, payload) -> result` where result is one of `{kind:'bootstrap'}`, `{kind:'forward', event:{type, ...}}` (no `agentId` — caller injects), `{kind:'release'}`, `{kind:'record', record:{type, ...}}`.
  - `mapping.classifyIntent(prompt) -> string` (CAP intent enum).
  - `mapping.isTestCommand(cmd) -> bool`.

- [ ] **Step 1: Write the failing tests**

Create `packages/coordify-hook/test/mapping.test.js`:

```js
'use strict';
const test = require('node:test');
const assert = require('node:assert');
const { mapEvent, classifyIntent, isTestCommand } = require('../lib/mapping');

test('classifyIntent keyword rules and default', () => {
  assert.equal(classifyIntent('fix the login bug'), 'BUGFIX');
  assert.equal(classifyIntent('write tests for auth'), 'TESTING');
  assert.equal(classifyIntent('update the docs'), 'DOCUMENTATION');
  assert.equal(classifyIntent('refactor the parser'), 'REFACTOR');
  assert.equal(classifyIntent('optimize perf of query'), 'PERFORMANCE');
  assert.equal(classifyIntent('security review of tokens'), 'SECURITY');
  assert.equal(classifyIntent('add a new dashboard'), 'FEATURE');
  assert.equal(classifyIntent(''), 'FEATURE');
  assert.equal(classifyIntent(undefined), 'FEATURE');
});

test('isTestCommand', () => {
  assert.equal(isTestCommand('cargo test'), true);
  assert.equal(isTestCommand('npm test'), true);
  assert.equal(isTestCommand('pytest -k foo'), true);
  assert.equal(isTestCommand('ls -la'), false);
});

test('SessionStart clear vs startup', () => {
  assert.deepEqual(mapEvent('SessionStart', { source: 'clear' }), { kind: 'forward', event: { type: 'CLEAR_INVOKED' } });
  assert.deepEqual(mapEvent('SessionStart', { source: 'startup' }), { kind: 'bootstrap' });
  assert.deepEqual(mapEvent('SessionStart', { source: 'resume' }), { kind: 'bootstrap' });
});

test('UserPromptSubmit -> CLAIM_PROPOSED with heuristic claim', () => {
  const r = mapEvent('UserPromptSubmit', { prompt: 'fix the bug in session expiry' });
  assert.equal(r.kind, 'forward');
  assert.equal(r.event.type, 'CLAIM_PROPOSED');
  assert.equal(r.event.intent, 'BUGFIX');
  assert.deepEqual(r.event.domains, []);
  assert.deepEqual(r.event.estimatedFiles, []);
  assert.equal(r.event.confidence, 0.7);
  assert.equal(r.event.task.summary, 'fix the bug in session expiry');
});

test('UserPromptSubmit truncates summary to 200 chars', () => {
  const long = 'x'.repeat(500);
  const r = mapEvent('UserPromptSubmit', { prompt: long });
  assert.equal(r.event.task.summary.length, 200);
});

test('Subagent start/stop -> AGENT_STATE_CHANGED', () => {
  assert.deepEqual(mapEvent('SubagentStart', {}), { kind: 'forward', event: { type: 'AGENT_STATE_CHANGED', state: 'SUBAGENT_WAITING' } });
  assert.deepEqual(mapEvent('SubagentStop', {}), { kind: 'forward', event: { type: 'AGENT_STATE_CHANGED', state: 'ACTIVE' } });
});

test('SessionEnd -> release', () => {
  assert.deepEqual(mapEvent('SessionEnd', { reason: 'other' }), { kind: 'release' });
});

test('PreToolUse -> recorded TOOL_PRECHECK / RISKY_WRITE_CHECKED', () => {
  assert.equal(mapEvent('PreToolUse', { tool_name: 'Read', tool_input: { file_path: 'a' } }).record.type, 'TOOL_PRECHECK');
  assert.equal(mapEvent('PreToolUse', { tool_name: 'Edit', tool_input: { file_path: 'a' } }).record.type, 'RISKY_WRITE_CHECKED');
});

test('PostToolUse -> recorded file/read/command', () => {
  assert.equal(mapEvent('PostToolUse', { tool_name: 'Write', tool_input: { file_path: 'src/x.rs' } }).record.type, 'FILE_TOUCHED');
  assert.equal(mapEvent('PostToolUse', { tool_name: 'Read', tool_input: { file_path: 'src/x.rs' } }).record.type, 'FILE_READ');
  assert.equal(mapEvent('PostToolUse', { tool_name: 'Bash', tool_input: { command: 'cargo test' } }).record.type, 'TEST_RUN');
  assert.equal(mapEvent('PostToolUse', { tool_name: 'Bash', tool_input: { command: 'ls' } }).record.type, 'COMMAND_EXECUTED');
});

test('all recorded events carry kind:record', () => {
  for (const t of ['PreToolUse', 'PostToolUse']) {
    assert.equal(mapEvent(t, { tool_name: 'X' }).kind, 'record');
  }
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `node --test packages/coordify-hook/`
Expected: FAIL — `Cannot find module '../lib/mapping'`.

- [ ] **Step 3: Implement `package.json`, `lib/paths.js`, `lib/mapping.js`**

`packages/coordify-hook/package.json`:

```json
{
  "name": "coordify-hook",
  "version": "0.1.0",
  "private": true,
  "description": "Coordify Node hook adapter — per-session sidecar bridging Claude Code hooks to Coordify Core.",
  "scripts": {
    "test": "node --test"
  }
}
```

`packages/coordify-hook/lib/paths.js`:

```js
'use strict';

const path = require('path');
const crypto = require('crypto');

function coordify(root) { return path.join(root, '.coordify'); }
function runtime(root) { return path.join(coordify(root), 'runtime'); }
function agentsDir(root) { return path.join(runtime(root), 'agents'); }

// Short, stable id for filenames — keeps the per-session socket path well under
// the ~104-byte macOS Unix-socket limit even for long session UUIDs / deep roots.
function shortId(sessionId) {
  return crypto.createHash('sha1').update(String(sessionId)).digest('hex').slice(0, 12);
}

module.exports = {
  coordify,
  runtime,
  agentsDir,
  shortId,
  socket: root => path.join(runtime(root), 'core.sock'),
  lock: root => path.join(runtime(root), 'core.lock'),
  token: root => path.join(runtime(root), 'session.token'),
  sessionSock: (root, sessionId) => path.join(agentsDir(root), shortId(sessionId) + '.sock'),
  hooktrace: (root, agentId) => path.join(agentsDir(root), 'agent-' + agentId + '.hooktrace.jsonl'),
  sidecarLog: (root, sessionId) => path.join(agentsDir(root), shortId(sessionId) + '.log'),
};
```

`packages/coordify-hook/lib/mapping.js`:

```js
'use strict';

const MAX_SUMMARY = 200;

// Fixed-order, case-insensitive keyword rules. First match wins.
const INTENT_RULES = [
  [/secur/i, 'SECURITY'],
  [/\btest/i, 'TESTING'],
  [/\bdoc/i, 'DOCUMENTATION'],
  [/refactor/i, 'REFACTOR'],
  [/perf|optimi/i, 'PERFORMANCE'],
  [/fix|bug/i, 'BUGFIX'],
];

function classifyIntent(prompt) {
  const p = String(prompt == null ? '' : prompt);
  for (const [re, intent] of INTENT_RULES) {
    if (re.test(p)) return intent;
  }
  return 'FEATURE';
}

function isTestCommand(cmd) {
  return /\b(test|jest|pytest|cargo test|go test|npm test|vitest|mocha)\b/i.test(String(cmd || ''));
}

// Pure translation of one hook payload to an adapter action.
//   {kind:'bootstrap'}                  ensure the sidecar exists; no Core traffic
//   {kind:'forward', event:{type,...}}  CAP event for Core (caller injects agentId)
//   {kind:'release'}                    SessionEnd: release live claims, then disconnect
//   {kind:'record', record:{type,...}}  recorded-only (local trace, not sent to Core)
function mapEvent(hook, payload) {
  payload = payload || {};
  switch (hook) {
    case 'SessionStart':
      return payload.source === 'clear'
        ? { kind: 'forward', event: { type: 'CLEAR_INVOKED' } }
        : { kind: 'bootstrap' };

    case 'UserPromptSubmit': {
      const summary = String(payload.prompt || '').trim().slice(0, MAX_SUMMARY);
      return {
        kind: 'forward',
        event: {
          type: 'CLAIM_PROPOSED',
          intent: classifyIntent(payload.prompt),
          domains: [],
          estimatedFiles: [],
          confidence: 0.7,
          task: { summary },
        },
      };
    }

    case 'SubagentStart':
      return { kind: 'forward', event: { type: 'AGENT_STATE_CHANGED', state: 'SUBAGENT_WAITING' } };
    case 'SubagentStop':
      return { kind: 'forward', event: { type: 'AGENT_STATE_CHANGED', state: 'ACTIVE' } };

    case 'SessionEnd':
      return { kind: 'release' };

    case 'PreToolUse': {
      const tool = payload.tool_name || '';
      const type = tool === 'Edit' || tool === 'Write' || tool === 'MultiEdit'
        ? 'RISKY_WRITE_CHECKED'
        : 'TOOL_PRECHECK';
      return { kind: 'record', record: { type, tool, input: payload.tool_input || {} } };
    }

    case 'PostToolUse': {
      const tool = payload.tool_name || '';
      const ti = payload.tool_input || {};
      if (tool === 'Edit' || tool === 'Write' || tool === 'MultiEdit') {
        return { kind: 'record', record: { type: 'FILE_TOUCHED', tool, file: ti.file_path || ti.path || null } };
      }
      if (tool === 'Read') {
        return { kind: 'record', record: { type: 'FILE_READ', tool, file: ti.file_path || ti.path || null } };
      }
      if (tool === 'Bash') {
        return { kind: 'record', record: { type: isTestCommand(ti.command) ? 'TEST_RUN' : 'COMMAND_EXECUTED', tool, command: ti.command || '' } };
      }
      return { kind: 'record', record: { type: 'TOOL_USED', tool } };
    }

    default:
      return { kind: 'record', record: { type: 'UNKNOWN_HOOK', hook } };
  }
}

module.exports = { mapEvent, classifyIntent, isTestCommand, MAX_SUMMARY };
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `node --test packages/coordify-hook/`
Expected: PASS (all `mapping.test.js` tests green).

- [ ] **Step 5: Commit**

```bash
git add packages/coordify-hook/package.json packages/coordify-hook/lib/paths.js packages/coordify-hook/lib/mapping.js packages/coordify-hook/test/mapping.test.js
git commit -m "feat(hook): pure hook->CAP mapping + path layout"
```

---

### Task 2: Socket clients — Core client + sidecar client

**Files:**
- Create: `packages/coordify-hook/lib/core-client.js`
- Create: `packages/coordify-hook/lib/sidecar-client.js`
- Test: `packages/coordify-hook/test/core-client.test.js`

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces:
  - `CoreClient(sockPath, token)` with async `connect()`, `register(meta) -> resp`, `heartbeat(agentId) -> resp`, `submitEvent(event) -> resp`, and `close()`. Each request sets a unique `id` and the `token`; responses are correlated by `id`.
  - `sidecar-client.emit(sockPath, message, timeoutMs=1000) -> Promise<void>` — connect, write one JSON line, end; resolve on close/error/timeout; NEVER reject (emit-only, crash-safe).

- [ ] **Step 1: Write the failing test**

Create `packages/coordify-hook/test/core-client.test.js` — drives `CoreClient` against an in-process fake Core that echoes framed responses:

```js
'use strict';
const test = require('node:test');
const assert = require('node:assert');
const net = require('net');
const os = require('os');
const path = require('path');
const fs = require('fs');
const { CoreClient } = require('../lib/core-client');

function tmpSock() {
  const d = fs.mkdtempSync(path.join(os.tmpdir(), 'cc-test-'));
  return path.join(d, 's.sock');
}

// Fake Core: reads newline JSON requests, replies per-request with a canned response.
function fakeCore(sockPath, handler) {
  const server = net.createServer(conn => {
    conn.setEncoding('utf8');
    let buf = '';
    conn.on('data', d => {
      buf += d;
      let i;
      while ((i = buf.indexOf('\n')) >= 0) {
        const line = buf.slice(0, i); buf = buf.slice(i + 1);
        if (!line.trim()) continue;
        const req = JSON.parse(line);
        const resp = handler(req);
        if (resp) conn.write(JSON.stringify(resp) + '\n');
      }
    });
    conn.on('error', () => {});
  });
  return new Promise(resolve => server.listen(sockPath, () => resolve(server)));
}

test('register and submitEvent correlate responses by id and carry token', async () => {
  const sock = tmpSock();
  const seen = [];
  const server = await fakeCore(sock, req => {
    seen.push(req);
    if (req.action === 'register') return { id: req.id, ok: true, agent_id: 'agent-1' };
    if (req.action === 'submit_event') return { id: req.id, ok: true, data: { claimId: 'claim-9', status: 'ACTIVE' } };
    return { id: req.id, ok: true };
  });

  const c = new CoreClient(sock, 'tok-abc');
  await c.connect();
  const reg = await c.register({ branch: 'main' });
  assert.equal(reg.ok, true);
  assert.equal(reg.agent_id, 'agent-1');

  const resp = await c.submitEvent({ type: 'CLAIM_PROPOSED', agentId: 'agent-1', intent: 'BUGFIX', confidence: 0.7 });
  assert.equal(resp.data.claimId, 'claim-9');

  // token + capVersion present on the wire
  assert.equal(seen[0].token, 'tok-abc');
  assert.equal(seen[0].action, 'register');
  assert.equal(seen[1].capVersion, '0.1');
  assert.equal(seen[1].event.type, 'CLAIM_PROPOSED');

  c.close();
  server.close();
});

test('concurrent requests resolve to their own responses', async () => {
  const sock = tmpSock();
  const server = await fakeCore(sock, req => ({ id: req.id, ok: true, data: { echo: req.action } }));
  const c = new CoreClient(sock, 't');
  await c.connect();
  const [a, b] = await Promise.all([c.heartbeat('agent-1'), c.submitEvent({ type: 'X' })]);
  assert.equal(a.data.echo, 'heartbeat');
  assert.equal(b.data.echo, 'submit_event');
  c.close();
  server.close();
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `node --test packages/coordify-hook/test/core-client.test.js`
Expected: FAIL — `Cannot find module '../lib/core-client'`.

- [ ] **Step 3: Implement**

`packages/coordify-hook/lib/core-client.js`:

```js
'use strict';

const net = require('net');

// Owns one persistent connection to coordify-core. Requests are newline-delimited
// JSON; responses are correlated to requests by a per-client sequence id.
class CoreClient {
  constructor(sockPath, token) {
    this.sockPath = sockPath;
    this.token = token;
    this.sock = null;
    this.buf = '';
    this.pending = new Map();
    this.seq = 0;
  }

  connect() {
    return new Promise((resolve, reject) => {
      const s = net.createConnection(this.sockPath);
      s.setEncoding('utf8');
      s.once('connect', () => { this.sock = s; resolve(); });
      s.once('error', reject);
      s.on('data', chunk => this._onData(chunk));
    });
  }

  _onData(chunk) {
    this.buf += chunk;
    let i;
    while ((i = this.buf.indexOf('\n')) >= 0) {
      const line = this.buf.slice(0, i);
      this.buf = this.buf.slice(i + 1);
      if (!line.trim()) continue;
      let resp;
      try { resp = JSON.parse(line); } catch { continue; }
      const resolve = this.pending.get(resp.id);
      if (resolve) { this.pending.delete(resp.id); resolve(resp); }
    }
  }

  _send(req) {
    return new Promise((resolve, reject) => {
      if (!this.sock) return reject(new Error('not connected'));
      const id = 'h' + (++this.seq);
      req.id = id;
      req.token = this.token;
      this.pending.set(id, resolve);
      this.sock.write(JSON.stringify(req) + '\n', err => {
        if (err) { this.pending.delete(id); reject(err); }
      });
    });
  }

  register(meta) { return this._send({ action: 'register', meta: meta || {} }); }
  heartbeat(agentId) { return this._send({ action: 'heartbeat', agent_id: agentId }); }
  submitEvent(event) { return this._send({ action: 'submit_event', capVersion: '0.1', event }); }

  close() {
    if (this.sock) { try { this.sock.end(); } catch (_) {} this.sock = null; }
  }
}

module.exports = { CoreClient };
```

`packages/coordify-hook/lib/sidecar-client.js`:

```js
'use strict';

const net = require('net');

// Fire-and-forget: connect to the per-session sidecar, write one JSON line, end.
// Resolves on close/error/timeout and NEVER rejects — a hook must not break the
// user's session if the sidecar is absent or slow.
function emit(sockPath, message, timeoutMs = 1000) {
  return new Promise(resolve => {
    let done = false;
    const finish = () => { if (!done) { done = true; resolve(); } };
    let s;
    const timer = setTimeout(() => { try { if (s) s.destroy(); } catch (_) {} finish(); }, timeoutMs);
    try {
      s = net.createConnection(sockPath);
    } catch (_) {
      clearTimeout(timer); return finish();
    }
    s.setEncoding('utf8');
    s.once('error', () => { clearTimeout(timer); finish(); });
    s.once('connect', () => { s.write(JSON.stringify(message) + '\n'); s.end(); });
    s.once('close', () => { clearTimeout(timer); finish(); });
  });
}

module.exports = { emit };
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `node --test packages/coordify-hook/`
Expected: PASS (mapping + core-client tests green).

- [ ] **Step 5: Commit**

```bash
git add packages/coordify-hook/lib/core-client.js packages/coordify-hook/lib/sidecar-client.js packages/coordify-hook/test/core-client.test.js
git commit -m "feat(hook): Core socket client + crash-safe sidecar emit client"
```

---

### Task 3: The sidecar daemon

**Files:**
- Create: `packages/coordify-hook/sidecar.js`

**Interfaces:**
- Consumes: `lib/paths.js`, `lib/core-client.js` (`CoreClient`), `lib/mapping.js` (`mapEvent`).
- Produces: an executable daemon `node sidecar.js --root <path> --session <id>` that bootstraps Core, registers, heartbeats, listens on the per-session socket, dispatches hook messages (forward / record / release), and shuts down on SessionEnd or SIGTERM/SIGINT.

- [ ] **Step 1: Implement the daemon**

There is no isolated unit test for `sidecar.js` (it is the long-lived IO orchestrator; it is covered end-to-end by Task 5's integration test). Create `packages/coordify-hook/sidecar.js`:

```js
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
```

- [ ] **Step 2: Smoke-check it parses and starts (no Core needed to fail-fast)**

Run: `node -c packages/coordify-hook/sidecar.js && echo "syntax ok"`
Expected: prints `syntax ok` (syntax check only; full behavior is exercised in Task 5).

- [ ] **Step 3: Commit**

```bash
git add packages/coordify-hook/sidecar.js
git commit -m "feat(hook): per-session sidecar daemon (bootstrap, register, heartbeat, dispatch)"
```

---

### Task 4: Hook clients + installer

**Files:**
- Create: `packages/coordify-hook/lib/hook-runner.js`
- Create: `packages/coordify-hook/hooks/session-start.js`
- Create: `packages/coordify-hook/hooks/user-prompt-submit.js`
- Create: `packages/coordify-hook/hooks/pre-tool-use.js`
- Create: `packages/coordify-hook/hooks/post-tool-use.js`
- Create: `packages/coordify-hook/hooks/subagent-start.js`
- Create: `packages/coordify-hook/hooks/subagent-stop.js`
- Create: `packages/coordify-hook/hooks/session-end.js`
- Create: `packages/coordify-hook/install.js`
- Test: `packages/coordify-hook/test/install.test.js`

**Interfaces:**
- Consumes: `lib/paths.js`, `lib/sidecar-client.js` (`emit`).
- Produces: `lib/hook-runner.js` exporting `run(hook)` (read stdin, ensure sidecar on SessionStart, emit to the session socket, exit 0); 7 hook entrypoints; `install.js` writing the hook block to `.claude/settings.json`.

- [ ] **Step 1: Write the failing test**

Create `packages/coordify-hook/test/install.test.js` — verifies the installer writes a hook block referencing all 7 hooks and backs up an existing file:

```js
'use strict';
const test = require('node:test');
const assert = require('node:assert');
const os = require('os');
const fs = require('fs');
const path = require('path');
const { execFileSync } = require('child_process');

test('install.js writes 7 hooks and backs up existing settings', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'cc-install-'));
  fs.mkdirSync(path.join(dir, '.claude'), { recursive: true });
  fs.writeFileSync(path.join(dir, '.claude', 'settings.json'), JSON.stringify({ existing: true }));

  execFileSync(process.execPath, [path.resolve(__dirname, '..', 'install.js')], { cwd: dir });

  const settings = JSON.parse(fs.readFileSync(path.join(dir, '.claude', 'settings.json'), 'utf8'));
  assert.equal(settings.existing, true, 'preserves unrelated keys');
  const names = Object.keys(settings.hooks);
  for (const h of ['SessionStart', 'UserPromptSubmit', 'PreToolUse', 'PostToolUse', 'SubagentStart', 'SubagentStop', 'SessionEnd']) {
    assert.ok(names.includes(h), 'has hook ' + h);
  }
  assert.match(settings.hooks.SessionStart[0].command, /coordify-hook\/hooks\/session-start\.js/);
  assert.ok(fs.existsSync(path.join(dir, '.claude', 'settings.json.backup')), 'backed up');
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `node --test packages/coordify-hook/test/install.test.js`
Expected: FAIL — `install.js` does not exist (ENOENT from execFileSync).

- [ ] **Step 3: Implement the runner, hooks, and installer**

`packages/coordify-hook/lib/hook-runner.js`:

```js
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
```

Each hook file is a 2-line entrypoint. Create them with these exact contents:

`hooks/session-start.js`:
```js
'use strict';
require('../lib/hook-runner').run('SessionStart');
```
`hooks/user-prompt-submit.js`:
```js
'use strict';
require('../lib/hook-runner').run('UserPromptSubmit');
```
`hooks/pre-tool-use.js`:
```js
'use strict';
require('../lib/hook-runner').run('PreToolUse');
```
`hooks/post-tool-use.js`:
```js
'use strict';
require('../lib/hook-runner').run('PostToolUse');
```
`hooks/subagent-start.js`:
```js
'use strict';
require('../lib/hook-runner').run('SubagentStart');
```
`hooks/subagent-stop.js`:
```js
'use strict';
require('../lib/hook-runner').run('SubagentStop');
```
`hooks/session-end.js`:
```js
'use strict';
require('../lib/hook-runner').run('SessionEnd');
```

`packages/coordify-hook/install.js` (mirrors the proven `phase-0/install.js` settings shape):

```js
'use strict';

const fs = require('fs');
const path = require('path');

const SETTINGS_PATH = path.resolve(process.cwd(), '.claude', 'settings.json');
const BACKUP_PATH = SETTINGS_PATH + '.backup';
const HOOKS_DIR = 'packages/coordify-hook/hooks';

function run() {
  if (fs.existsSync(SETTINGS_PATH)) {
    fs.copyFileSync(SETTINGS_PATH, BACKUP_PATH);
    console.log('Backed up existing settings to ' + BACKUP_PATH);
  }

  let settings = {};
  if (fs.existsSync(SETTINGS_PATH)) {
    try { settings = JSON.parse(fs.readFileSync(SETTINGS_PATH, 'utf8')); }
    catch (_) { console.warn('Could not parse existing settings.json — starting fresh'); }
  }

  settings.hooks = {
    SessionStart:     [{ command: 'node ' + HOOKS_DIR + '/session-start.js' }],
    UserPromptSubmit: [{ command: 'node ' + HOOKS_DIR + '/user-prompt-submit.js' }],
    PreToolUse:       [{ command: 'node ' + HOOKS_DIR + '/pre-tool-use.js' }],
    PostToolUse:      [{ command: 'node ' + HOOKS_DIR + '/post-tool-use.js' }],
    SubagentStart:    [{ command: 'node ' + HOOKS_DIR + '/subagent-start.js' }],
    SubagentStop:     [{ command: 'node ' + HOOKS_DIR + '/subagent-stop.js' }],
    SessionEnd:       [{ command: 'node ' + HOOKS_DIR + '/session-end.js' }],
  };

  fs.mkdirSync(path.dirname(SETTINGS_PATH), { recursive: true });
  fs.writeFileSync(SETTINGS_PATH, JSON.stringify(settings, null, 2));
  console.log('Coordify hook configuration written to ' + SETTINGS_PATH);
}

run();
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `node --test packages/coordify-hook/`
Expected: PASS (mapping + core-client + install tests green).

- [ ] **Step 5: Commit**

```bash
git add packages/coordify-hook/lib/hook-runner.js packages/coordify-hook/hooks packages/coordify-hook/install.js packages/coordify-hook/test/install.test.js
git commit -m "feat(hook): 7 thin hook clients, shared runner, settings installer"
```

---

### Task 5: End-to-end integration test against real Core

**Files:**
- Create: `packages/coordify-hook/test/integration.test.js`

**Interfaces:**
- Consumes: the built `coordify-core` binary, `sidecar.js`, `lib/paths.js`, `lib/sidecar-client.js`.
- Produces: one end-to-end test driving the full pipe and asserting Core's `events.log`.

- [ ] **Step 1: Write the test**

Create `packages/coordify-hook/test/integration.test.js`. It resolves the Core binary, skips with a clear message if it is not built, spawns the sidecar, drives a hook sequence through the per-session socket, and polls Core's `events.log`:

```js
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

  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'cc-int-'));
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
    await emit(sockPath, { hook: 'SubagentStart', payload: { cwd: root, session_id: SESSION } });
    await emit(sockPath, { hook: 'SubagentStop', payload: { cwd: root, session_id: SESSION } });

    const claimed = await waitFor(() => readEventsLog(root).includes('CLAIM_CREATED'), 5000);
    assert.ok(claimed, 'CLAIM_CREATED should be logged:\n' + readEventsLog(root));

    const log = readEventsLog(root);
    assert.ok(log.includes('AGENT_JOINED'), 'agent registered');
    assert.ok(log.includes('AGENT_STATE_CHANGED'), 'subagent state change forwarded');

    // PostToolUse(Write) is recorded-only: it lands in the hooktrace, NOT Core's log.
    const traceFiles = fs.readdirSync(paths.agentsDir(root)).filter(f => f.endsWith('.hooktrace.jsonl'));
    assert.ok(traceFiles.length >= 1, 'a hooktrace file exists');
    const trace = fs.readFileSync(path.join(paths.agentsDir(root), traceFiles[0]), 'utf8');
    assert.ok(trace.includes('FILE_TOUCHED'), 'recorded-only FILE_TOUCHED in trace');
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
```

- [ ] **Step 2: Build Core and run the test**

Run:
```bash
(cd packages/coordify-core && cargo build) && node --test packages/coordify-hook/test/integration.test.js
```
Expected: PASS — the test drives the full pipe and asserts `AGENT_JOINED`, `CLAIM_CREATED`, `AGENT_STATE_CHANGED`, `AGENT_LEFT` in Core's log, the recorded-only `FILE_TOUCHED` in the trace, and no `SCHEMA_VALIDATION_FAILED`.

- [ ] **Step 3: Run the whole adapter suite**

Run: `node --test packages/coordify-hook/`
Expected: PASS (mapping + core-client + install + integration).

- [ ] **Step 4: Commit**

```bash
git add packages/coordify-hook/test/integration.test.js
git commit -m "test(hook): end-to-end pipe against real coordify-core"
```

---

## Notes for the Final Whole-Branch Review

- Crash-safety: confirm every hook path ends in `process.exit(0)` and swallows all errors (a thrown hook would break the user's Claude session).
- Confirm `mapping.js` stays pure (no `require` of IO modules, no `Date`/`Math.random` affecting output).
- Confirm only the four Core-ingestable CAP types are ever sent to Core; recorded-only types go to the hooktrace, never `submit_event` (no `SCHEMA_VALIDATION_FAILED`).
- Confirm the per-session socket path uses the short hash (macOS 104-byte limit).
- Confirm Core is unmodified (no changes under `packages/coordify-core/`).
- Integration test must skip cleanly (not fail) when the Core binary is not built.
- Known v1 limitation (by design, not a defect): heat stays near-inert because claims carry no files and Core does not yet ingest `FILE_TOUCHED`.
