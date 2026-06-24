# Phase 6b — Sim & Replay Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `packages/coordify-sim` with two commands — `coordify simulate <script.json>` (drives real Core with a JSON event script) and `coordify replay <session-id>` (visual playback or state reconstruction from a past session).

**Architecture:** TypeScript/Node.js package. `CoreManager` detects or spawns the real `coordify-core` binary; `ScenarioRunner` validates + submits JSON script events via IPC; `Replayer` reads `events.log` and either renders via ink frame-by-frame or re-submits events to Core. Reuses IPC primitives from `coordify-cli`. No mock Core — real binary only.

**Tech Stack:** TypeScript 5, Node.js 20, ink 4, `ajv` for JSON Schema validation, `tsx` for tests, `node:test` + `node:assert`.

## Global Constraints

- No mock Core. `CoreManager` spawns the real `coordify-core` binary (auto-detected from `../coordify-core/target/` or `$COORDIFY_CORE_BIN` or PATH).
- `CoreManager` never stops a Core it did not spawn.
- `--dry-run` on `simulate`: validate + print steps, never connect to socket.
- Visual replay never requires a live Core.
- JSON scenario script validated against schema before any execution.
- `--json` flag on both commands dumps raw data and exits.
- ink version: `^4.4.1` (CJS-compatible, same as coordify-cli).
- TypeScript `"module": "commonjs"`, `"target": "ES2022"`. Entry: `dist/cli.js`.
- Core socket path: `.coordify/runtime/core.sock`; token: `.coordify/runtime/session.token`.
- Scenario script schema: `{ name: string, agents: string[], steps: [{delay_ms: number, event: object}][], finalize?: boolean }`.

---

### Task 1: Package scaffold + CoreManager

Scaffolds `coordify-sim` and implements `CoreManager` — detect existing Core or spawn the binary, wait for socket, return token.

**Files:**
- Create: `packages/coordify-sim/package.json`
- Create: `packages/coordify-sim/tsconfig.json`
- Create: `packages/coordify-sim/src/core-manager.ts`
- Create: `packages/coordify-sim/test/core-manager.test.ts`

**Interfaces:**
- Produces: `CoreManager` class with `ensure(): Promise<CoreHandle>`, `stop(): Promise<void>`. `CoreHandle = { socketPath: string; token: string; spawned: boolean }`.

- [ ] **Step 1: Write the failing tests**

Create `packages/coordify-sim/test/core-manager.test.ts`:

```typescript
import test from 'node:test';
import assert from 'node:assert';
import net from 'net';
import os from 'os';
import path from 'path';
import fs from 'fs';
import { CoreManager } from '../src/core-manager.js';

function fakeSocket(dir: string): net.Server {
  const sockPath = path.join(dir, '.coordify', 'runtime', 'core.sock');
  fs.mkdirSync(path.dirname(sockPath), { recursive: true });
  const server = net.createServer(conn => { conn.on('error', () => {}); });
  server.listen(sockPath);
  return server;
}

test('CoreManager.ensure detects existing socket as not-spawned', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'sim-cm-'));
  const tokenPath = path.join(root, '.coordify', 'runtime', 'session.token');
  const server = fakeSocket(root);
  fs.writeFileSync(tokenPath, 'tok-existing');

  const cm = new CoreManager(root);
  const handle = await cm.ensure();
  assert.equal(handle.spawned, false);
  assert.equal(handle.token, 'tok-existing');
  assert.ok(handle.socketPath.endsWith('core.sock'));

  server.close();
  fs.rmSync(root, { recursive: true });
});

test('CoreManager.ensure throws if binary not found and no socket', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'sim-cm2-'));
  const cm = new CoreManager(root, '/nonexistent/coordify-core');
  await assert.rejects(() => cm.ensure(), /binary not found|ENOENT|spawn/i);
  fs.rmSync(root, { recursive: true });
});

test('CoreManager.stop is no-op when nothing was spawned', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'sim-cm3-'));
  const cm = new CoreManager(root);
  await assert.doesNotReject(() => cm.stop());
  fs.rmSync(root, { recursive: true });
});
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd packages/coordify-sim && npx tsx --test test/core-manager.test.ts 2>&1 | tail -5
```

Expected: FAIL — module not found.

- [ ] **Step 3: Create `package.json`**

```json
{
  "name": "coordify-sim",
  "version": "0.1.0",
  "private": true,
  "description": "Coordify scenario runner and session replayer",
  "main": "dist/cli.js",
  "bin": { "coordify-sim": "dist/cli.js" },
  "scripts": {
    "build": "tsc",
    "test": "npx tsx --test 'test/**/*.test.ts' 'test/**/*.test.tsx'"
  },
  "dependencies": {
    "ajv": "^8.14.0",
    "ink": "^4.4.1",
    "react": "^18.2.0"
  },
  "devDependencies": {
    "@types/node": "^20.0.0",
    "@types/react": "^18.2.0",
    "ink-testing-library": "^3.0.0",
    "tsx": "^4.7.0",
    "typescript": "^5.4.0"
  }
}
```

- [ ] **Step 4: Create `tsconfig.json`**

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "module": "commonjs",
    "lib": ["ES2022"],
    "outDir": "dist",
    "rootDir": "src",
    "strict": true,
    "esModuleInterop": true,
    "skipLibCheck": true,
    "jsx": "react"
  },
  "include": ["src"]
}
```

- [ ] **Step 5: Create `src/core-manager.ts`**

```typescript
import net from 'net';
import fs from 'fs';
import path from 'path';
import { spawn, ChildProcess } from 'child_process';

export interface CoreHandle {
  socketPath: string;
  token: string;
  spawned: boolean;
}

function socketPath(root: string) { return path.join(root, '.coordify', 'runtime', 'core.sock'); }
function tokenPath(root: string)  { return path.join(root, '.coordify', 'runtime', 'session.token'); }

function readToken(root: string): string | null {
  try { return fs.readFileSync(tokenPath(root), 'utf8').trim(); } catch { return null; }
}

function resolveBin(override?: string): string | null {
  if (override) return fs.existsSync(override) ? override : null;
  if (process.env.COORDIFY_CORE_BIN) return process.env.COORDIFY_CORE_BIN;
  const base = path.resolve(__dirname, '..', '..', 'coordify-core', 'target');
  for (const p of [
    path.join(base, 'release', 'coordify-core'),
    path.join(base, 'debug', 'coordify-core'),
  ]) { if (fs.existsSync(p)) return p; }
  return 'coordify-core'; // PATH fallback
}

function waitForSocket(sockPath: string, timeoutMs: number): Promise<void> {
  return new Promise((resolve, reject) => {
    const deadline = Date.now() + timeoutMs;
    function check() {
      if (fs.existsSync(sockPath)) { resolve(); return; }
      if (Date.now() > deadline) { reject(new Error(`socket never appeared: ${sockPath}`)); return; }
      setTimeout(check, 100);
    }
    check();
  });
}

export class CoreManager {
  private spawned: ChildProcess | null = null;

  constructor(private readonly root: string, private readonly binOverride?: string) {}

  async ensure(): Promise<CoreHandle> {
    const sock = socketPath(this.root);
    if (fs.existsSync(sock)) {
      const tok = readToken(this.root) ?? '';
      return { socketPath: sock, token: tok, spawned: false };
    }
    const bin = resolveBin(this.binOverride);
    if (!bin) throw new Error(`coordify-core binary not found`);
    // Try spawning — if binary doesn't exist, spawn throws
    const child = spawn(bin, ['--root', this.root], {
      detached: false,
      stdio: 'ignore',
      env: { ...process.env },
    });
    child.on('error', err => { throw err; });
    this.spawned = child;
    await waitForSocket(sock, 5000);
    const tok = readToken(this.root) ?? '';
    return { socketPath: sock, token: tok, spawned: true };
  }

  async stop(): Promise<void> {
    if (!this.spawned) return;
    this.spawned.kill('SIGTERM');
    this.spawned = null;
    // wait for socket to disappear (up to 3s)
    const sock = socketPath(this.root);
    const deadline = Date.now() + 3000;
    while (fs.existsSync(sock) && Date.now() < deadline) {
      await new Promise(r => setTimeout(r, 100));
    }
  }
}
```

- [ ] **Step 6: Run tests and verify they pass**

```bash
cd packages/coordify-sim && npm install && npx tsx --test test/core-manager.test.ts 2>&1 | tail -8
```

Expected: `# tests 3`, `# pass 3`, `# fail 0`.

- [ ] **Step 7: Commit**

```bash
git add packages/coordify-sim/
git commit -m "feat(sim): package scaffold + CoreManager (detect/spawn real Core)"
```

---

### Task 2: JSON Schema validation + ScenarioRunner

Validates scenario scripts against a schema, then submits events to Core via IPC in order with delays.

**Files:**
- Create: `packages/coordify-sim/src/schema.ts`
- Create: `packages/coordify-sim/src/runner.ts`
- Create: `packages/coordify-sim/scenarios/two-agent-conflict.json`
- Create: `packages/coordify-sim/scenarios/deadlock-three-agents.json`
- Create: `packages/coordify-sim/test/schema.test.ts`
- Create: `packages/coordify-sim/test/runner.test.ts`

**Interfaces:**
- Consumes: `CoreManager`, `CoreHandle`
- Produces: `validateScript(raw: unknown): ScenarioScript | ValidationError[]`, `runScenario(handle: CoreHandle, script: ScenarioScript, opts: {dryRun?: boolean, noFinalize?: boolean}): Promise<void>`

- [ ] **Step 1: Write failing tests**

Create `packages/coordify-sim/test/schema.test.ts`:

```typescript
import test from 'node:test';
import assert from 'node:assert';
import { validateScript } from '../src/schema.js';

test('validates a correct script', () => {
  const result = validateScript({
    name: 'test',
    agents: ['a1'],
    steps: [{ delay_ms: 0, event: { type: 'AGENT_JOINED', agentId: 'a1' } }],
  });
  assert.ok(!Array.isArray(result));
  assert.equal((result as any).name, 'test');
});

test('rejects script missing name', () => {
  const result = validateScript({ agents: [], steps: [] });
  assert.ok(Array.isArray(result));
  assert.ok((result as string[]).some(e => e.includes('name')));
});

test('rejects script with bad step (missing event)', () => {
  const result = validateScript({ name: 'x', agents: [], steps: [{ delay_ms: 0 }] });
  assert.ok(Array.isArray(result));
});

test('rejects step with non-object event', () => {
  const result = validateScript({ name: 'x', agents: [], steps: [{ delay_ms: 0, event: 'bad' }] });
  assert.ok(Array.isArray(result));
});
```

Create `packages/coordify-sim/test/runner.test.ts`:

```typescript
import test from 'node:test';
import assert from 'node:assert';
import net from 'net';
import os from 'os';
import path from 'path';
import fs from 'fs';
import { runScenario } from '../src/runner.js';
import type { ScenarioScript } from '../src/schema.js';

function fakeCore(sockPath: string): { server: net.Server; received: any[] } {
  const received: any[] = [];
  const server = net.createServer(conn => {
    conn.setEncoding('utf8');
    let buf = '';
    conn.on('data', (d: string) => {
      buf += d;
      let i: number;
      while ((i = buf.indexOf('\n')) >= 0) {
        const line = buf.slice(0, i); buf = buf.slice(i + 1);
        if (!line.trim()) continue;
        const req = JSON.parse(line);
        received.push(req);
        const resp: any = { id: req.id, ok: true };
        if (req.action === 'register') resp.agent_id = 'agent-' + received.length;
        conn.write(JSON.stringify(resp) + '\n');
      }
    });
    conn.on('error', () => {});
  });
  server.listen(sockPath);
  return { server, received };
}

test('runScenario submits events in order', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'sim-runner-'));
  const sockPath = path.join(root, '.coordify', 'runtime', 'core.sock');
  fs.mkdirSync(path.dirname(sockPath), { recursive: true });
  fs.writeFileSync(path.join(root, '.coordify', 'runtime', 'session.token'), 'tok');
  const { server, received } = fakeCore(sockPath);

  const script: ScenarioScript = {
    name: 'test',
    agents: ['a1'],
    steps: [
      { delay_ms: 0, event: { type: 'AGENT_JOINED', agentId: 'a1' } },
      { delay_ms: 0, event: { type: 'CLAIM_PROPOSED', agentId: 'a1', intent: 'BUGFIX', confidence: 0.9, taskSummary: 't', domains: [], estimatedFiles: [] } },
    ],
    finalize: false,
  };

  await runScenario({ socketPath: sockPath, token: 'tok', spawned: false }, script, {});
  // register for a1 + 2 submit_events
  assert.ok(received.some(r => r.action === 'register'));
  assert.ok(received.some(r => r.action === 'submit_event'));

  server.close();
  fs.rmSync(root, { recursive: true });
});

test('runScenario --dry-run prints steps without connecting', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'sim-dry-'));
  // no socket — dry-run should not attempt connection
  const script: ScenarioScript = {
    name: 'dry',
    agents: ['a1'],
    steps: [{ delay_ms: 0, event: { type: 'AGENT_JOINED', agentId: 'a1' } }],
  };
  let out = '';
  const orig = process.stdout.write.bind(process.stdout);
  (process.stdout as any).write = (s: string) => { out += s; return true; };
  const { runScenario } = await import('../src/runner.js');
  await runScenario({ socketPath: '/nonexistent.sock', token: '', spawned: false }, script, { dryRun: true });
  (process.stdout as any).write = orig;
  assert.ok(out.includes('dry-run') || out.includes('AGENT_JOINED') || out.includes('step'));
  fs.rmSync(root, { recursive: true });
});
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd packages/coordify-sim && npx tsx --test test/schema.test.ts test/runner.test.ts 2>&1 | tail -5
```

Expected: FAIL — modules not found.

- [ ] **Step 3: Create `src/schema.ts`**

```typescript
import Ajv from 'ajv';

export interface ScenarioStep {
  delay_ms: number;
  event: Record<string, unknown>;
}

export interface ScenarioScript {
  name: string;
  agents: string[];
  steps: ScenarioStep[];
  finalize?: boolean;
}

const SCHEMA = {
  type: 'object',
  required: ['name', 'agents', 'steps'],
  additionalProperties: true,
  properties: {
    name: { type: 'string', minLength: 1 },
    agents: { type: 'array', items: { type: 'string' } },
    finalize: { type: 'boolean' },
    steps: {
      type: 'array',
      items: {
        type: 'object',
        required: ['delay_ms', 'event'],
        properties: {
          delay_ms: { type: 'number', minimum: 0 },
          event: { type: 'object', required: ['type'], properties: { type: { type: 'string' } } },
        },
      },
    },
  },
};

const ajv = new Ajv();
const validate = ajv.compile(SCHEMA);

export function validateScript(raw: unknown): ScenarioScript | string[] {
  const valid = validate(raw);
  if (!valid) return (validate.errors ?? []).map(e => `${e.instancePath || '/'} ${e.message}`);
  return raw as ScenarioScript;
}
```

- [ ] **Step 4: Create `src/runner.ts`**

```typescript
import net from 'net';
import type { CoreHandle } from './core-manager.js';
import type { ScenarioScript } from './schema.js';

class SimClient {
  private sock: net.Socket | null = null;
  private buf = '';
  private seq = 0;
  private pending = new Map<string, (r: any) => void>();
  private agentTokens = new Map<string, string>();

  constructor(private sockPath: string, private masterToken: string) {}

  connect(): Promise<void> {
    return new Promise((resolve, reject) => {
      const s = net.createConnection(this.sockPath);
      s.setEncoding('utf8');
      s.once('connect', () => { this.sock = s; resolve(); });
      s.once('error', reject);
      s.on('data', (chunk: string) => {
        this.buf += chunk;
        let i: number;
        while ((i = this.buf.indexOf('\n')) >= 0) {
          const line = this.buf.slice(0, i); this.buf = this.buf.slice(i + 1);
          if (!line.trim()) continue;
          let resp: any;
          try { resp = JSON.parse(line); } catch { continue; }
          const res = this.pending.get(resp.id);
          if (res) { this.pending.delete(resp.id); res(resp); }
        }
      });
    });
  }

  send(action: string, payload: Record<string, unknown> = {}): Promise<any> {
    return new Promise((resolve, reject) => {
      if (!this.sock) { reject(new Error('not connected')); return; }
      const id = 's' + (++this.seq);
      this.pending.set(id, resolve);
      this.sock.write(JSON.stringify({ id, token: this.masterToken, action, ...payload }) + '\n', err => {
        if (err) { this.pending.delete(id); reject(err); }
      });
    });
  }

  async registerAgent(agentId: string) {
    const resp = await this.send('register', { meta: { agentId } });
    if (resp.agent_id) this.agentTokens.set(agentId, resp.agent_id);
    return resp;
  }

  async submitEvent(event: Record<string, unknown>) {
    return this.send('submit_event', { capVersion: '0.1', event });
  }

  close() { try { this.sock?.end(); } catch (_) {} this.sock = null; }
}

export async function runScenario(
  handle: CoreHandle,
  script: ScenarioScript,
  opts: { dryRun?: boolean; noFinalize?: boolean }
): Promise<void> {
  if (opts.dryRun) {
    process.stdout.write(`[dry-run] ${script.name} — ${script.steps.length} steps\n`);
    for (let i = 0; i < script.steps.length; i++) {
      const s = script.steps[i];
      process.stdout.write(`  step ${i + 1}: ${(s.event as any).type ?? '?'} delay=${s.delay_ms}ms\n`);
    }
    return;
  }

  const client = new SimClient(handle.socketPath, handle.token);
  await client.connect();

  process.stdout.write(`Running: ${script.name}\n`);
  process.stdout.write(`  Registering agents...\n`);
  for (const agentId of script.agents) {
    await client.registerAgent(agentId);
  }

  for (let i = 0; i < script.steps.length; i++) {
    const step = script.steps[i];
    if (step.delay_ms > 0) await new Promise(r => setTimeout(r, step.delay_ms));
    process.stdout.write(`  Step ${i + 1}/${script.steps.length}  ${(step.event as any).type ?? '?'}\n`);
    await client.submitEvent(step.event as Record<string, unknown>);
  }

  if (script.finalize && !opts.noFinalize) {
    process.stdout.write(`  Finalizing...\n`);
    for (const agentId of script.agents) {
      await client.submitEvent({ type: 'AGENT_LEFT', agentId }).catch(() => {});
    }
  }

  client.close();
  process.stdout.write(`Done. Use 'coordify watch' or 'coordify stats' to inspect results.\n`);
}
```

- [ ] **Step 5: Create scenario fixtures**

Create `packages/coordify-sim/scenarios/two-agent-conflict.json`:

```json
{
  "name": "two-agent-conflict",
  "agents": ["agent-a", "agent-b"],
  "steps": [
    { "delay_ms": 0,   "event": { "type": "AGENT_JOINED", "agentId": "agent-a" } },
    { "delay_ms": 100, "event": { "type": "CLAIM_PROPOSED", "agentId": "agent-a", "taskSummary": "Fix auth bug", "intent": "BUGFIX", "domains": ["src"], "estimatedFiles": ["src/auth.rs"], "confidence": 0.9 } },
    { "delay_ms": 200, "event": { "type": "AGENT_JOINED", "agentId": "agent-b" } },
    { "delay_ms": 300, "event": { "type": "CLAIM_PROPOSED", "agentId": "agent-b", "taskSummary": "Add auth tests", "intent": "TESTING", "domains": ["src"], "estimatedFiles": ["src/auth.rs"], "confidence": 0.9 } },
    { "delay_ms": 500, "event": { "type": "FILE_TOUCHED", "agentId": "agent-a", "files": ["src/auth.rs"] } }
  ],
  "finalize": true
}
```

Create `packages/coordify-sim/scenarios/deadlock-three-agents.json`:

```json
{
  "name": "deadlock-three-agents",
  "agents": ["agent-a", "agent-b", "agent-c"],
  "steps": [
    { "delay_ms": 0,   "event": { "type": "AGENT_JOINED", "agentId": "agent-a" } },
    { "delay_ms": 0,   "event": { "type": "AGENT_JOINED", "agentId": "agent-b" } },
    { "delay_ms": 0,   "event": { "type": "AGENT_JOINED", "agentId": "agent-c" } },
    { "delay_ms": 100, "event": { "type": "CLAIM_PROPOSED", "agentId": "agent-a", "taskSummary": "Task A", "intent": "BUGFIX", "domains": ["src"], "estimatedFiles": ["src/x.rs"], "confidence": 0.9 } },
    { "delay_ms": 150, "event": { "type": "CLAIM_PROPOSED", "agentId": "agent-b", "taskSummary": "Task B", "intent": "BUGFIX", "domains": ["src"], "estimatedFiles": ["src/x.rs", "src/y.rs"], "confidence": 0.9 } },
    { "delay_ms": 200, "event": { "type": "CLAIM_PROPOSED", "agentId": "agent-c", "taskSummary": "Task C", "intent": "BUGFIX", "domains": ["src"], "estimatedFiles": ["src/y.rs"], "confidence": 0.9 } }
  ],
  "finalize": true
}
```

- [ ] **Step 6: Run tests and verify they pass**

```bash
cd packages/coordify-sim && npx tsx --test test/schema.test.ts test/runner.test.ts 2>&1 | tail -8
```

Expected: `# tests 6`, `# pass 6`, `# fail 0`.

- [ ] **Step 7: Commit**

```bash
git add packages/coordify-sim/src/schema.ts packages/coordify-sim/src/runner.ts packages/coordify-sim/scenarios/ packages/coordify-sim/test/schema.test.ts packages/coordify-sim/test/runner.test.ts
git commit -m "feat(sim): JSON schema validation + ScenarioRunner + example scenarios"
```

---

### Task 3: Replayer (visual + reconstruct)

Reads `events.log` from a past session. In `--visual` mode: renders events through ink TUI frame-by-frame with speed control. In `--reconstruct` mode: re-submits events to a live Core.

**Files:**
- Create: `packages/coordify-sim/src/replayer.ts`
- Create: `packages/coordify-sim/src/tui/replay-watch.tsx`
- Create: `packages/coordify-sim/test/replayer.test.ts`
- Create: `packages/coordify-sim/test/tui/replay-watch.test.tsx`

**Interfaces:**
- Produces: `replayVisual(root: string, sessionId: string, opts: {speed?: number}): Promise<void>`, `replayReconstruct(root: string, sessionId: string, opts: {stopAt?: number}): Promise<void>`

- [ ] **Step 1: Write failing tests**

Create `packages/coordify-sim/test/replayer.test.ts`:

```typescript
import test from 'node:test';
import assert from 'node:assert';
import os from 'os';
import path from 'path';
import fs from 'fs';

function makeSession(root: string, id: string, events: object[]): void {
  const sdir = path.join(root, '.coordify', 'sessions', id);
  fs.mkdirSync(sdir, { recursive: true });
  fs.writeFileSync(path.join(sdir, 'events.log'), events.map(e => JSON.stringify(e)).join('\n'));
}

test('replayVisual exits cleanly with no events', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'sim-rep-'));
  makeSession(root, 'sess-1', []);
  const { replayVisual } = await import('../src/replayer.js');
  // Should complete without throwing
  await assert.doesNotReject(() =>
    Promise.race([
      replayVisual(root, 'sess-1', { speed: 100 }),
      new Promise(r => setTimeout(r, 500)), // timeout so test doesn't hang
    ])
  );
  fs.rmSync(root, { recursive: true });
});

test('replayReconstruct --stop-at 1 submits only first event', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'sim-rec-'));
  const sockPath = path.join(root, '.coordify', 'runtime', 'core.sock');
  fs.mkdirSync(path.dirname(sockPath), { recursive: true });
  fs.writeFileSync(path.join(root, '.coordify', 'runtime', 'session.token'), 'tok');
  const net = require('net');
  const received: any[] = [];
  const server = net.createServer((conn: any) => {
    conn.setEncoding('utf8');
    let buf = '';
    conn.on('data', (d: string) => {
      buf += d;
      let i: number;
      while ((i = buf.indexOf('\n')) >= 0) {
        const line = buf.slice(0, i); buf = buf.slice(i + 1);
        if (line.trim()) { received.push(JSON.parse(line)); conn.write(JSON.stringify({ id: JSON.parse(line).id, ok: true }) + '\n'); }
      }
    });
    conn.on('error', () => {});
  });
  await new Promise<void>(r => server.listen(sockPath, r));
  makeSession(root, 'sess-1', [
    { type: 'AGENT_JOINED', agentId: 'a1', ts: '2026-06-23T00:00:00Z' },
    { type: 'CLAIM_PROPOSED', agentId: 'a1', ts: '2026-06-23T00:00:01Z' },
  ]);
  const { replayReconstruct } = await import('../src/replayer.js');
  await replayReconstruct(root, 'sess-1', { stopAt: 1 });
  // Only 1 event submitted (register doesn't count here — just the events)
  const events = received.filter(r => r.action === 'submit_event');
  assert.ok(events.length <= 1, `expected at most 1 submit_event, got ${events.length}`);
  server.close();
  fs.rmSync(root, { recursive: true });
});
```

Create `packages/coordify-sim/test/tui/replay-watch.test.tsx`:

```typescript
import test from 'node:test';
import assert from 'node:assert';
import React from 'react';
import { render } from 'ink-testing-library';
import ReplayFrame from '../../src/tui/replay-watch.js';

test('ReplayFrame renders event type and index', () => {
  const events = [
    { type: 'AGENT_JOINED', agentId: 'a1', ts: '2026-06-23T00:00:00Z' },
    { type: 'CLAIM_PROPOSED', agentId: 'a1', ts: '2026-06-23T00:00:01Z' },
  ];
  const { lastFrame } = render(React.createElement(ReplayFrame, {
    events, currentIndex: 0, total: 2, speed: 1, paused: false
  }));
  const frame = lastFrame() ?? '';
  assert.ok(frame.includes('AGENT_JOINED') || frame.includes('1/2'));
});
```

- [ ] **Step 2: Run to verify failure**

```bash
cd packages/coordify-sim && npx tsx --test test/replayer.test.ts test/tui/replay-watch.test.tsx 2>&1 | tail -5
```

Expected: FAIL — modules not found.

- [ ] **Step 3: Create `src/tui/replay-watch.tsx`**

```tsx
import React from 'react';
import { Box, Text } from 'ink';

interface Props {
  events: Record<string, unknown>[];
  currentIndex: number;
  total: number;
  speed: number;
  paused: boolean;
}

export default function ReplayFrame({ events, currentIndex, total, speed, paused }: Props) {
  const ev = events[currentIndex];
  const ts = ev ? String(ev.ts ?? '').replace('T', ' ').replace('Z', '') : '';
  return (
    <Box flexDirection="column">
      <Box borderStyle="single" borderColor="magenta" paddingX={1}>
        <Text bold color="magenta">Replay</Text>
        <Text color="gray">  {currentIndex + 1}/{total}  speed: {speed}x{paused ? '  [PAUSED]' : ''}</Text>
      </Box>
      {ev ? (
        <Box flexDirection="column" paddingX={1}>
          <Text color="cyan">[{ts}] <Text bold>{String(ev.type ?? '')}</Text></Text>
          {Object.entries(ev)
            .filter(([k]) => !['type', 'ts'].includes(k))
            .map(([k, v]) => <Text key={k} color="gray">  {k}: {JSON.stringify(v)}</Text>)
          }
        </Box>
      ) : <Text color="gray">end of replay</Text>}
      <Text color="gray">[q] quit  [space] pause  [←] -10  [→] +10  [+/-] speed</Text>
    </Box>
  );
}
```

- [ ] **Step 4: Create `src/replayer.ts`**

```typescript
import fs from 'fs';
import path from 'path';
import React, { useState, useEffect } from 'react';
import { render, useApp, useInput } from 'ink';
import ReplayFrame from './tui/replay-watch.js';
import net from 'net';

function readEvents(root: string, sessionId: string): Record<string, unknown>[] {
  const logPath = path.join(root, '.coordify', 'sessions', sessionId, 'events.log');
  try {
    return fs.readFileSync(logPath, 'utf8')
      .split('\n')
      .filter(l => l.trim())
      .map(l => { try { return JSON.parse(l); } catch { return null; } })
      .filter(Boolean) as Record<string, unknown>[];
  } catch { return []; }
}

function ReplayApp({ events, speed: initSpeed }: { events: Record<string, unknown>[]; speed: number }) {
  const { exit } = useApp();
  const [index, setIndex] = useState(0);
  const [paused, setPaused] = useState(false);
  const [speed, setSpeed] = useState(initSpeed);

  useInput((input, key) => {
    if (input === 'q' || (key.ctrl && input === 'c')) { exit(); return; }
    if (input === ' ') { setPaused(p => !p); return; }
    if (key.rightArrow) setIndex(i => Math.min(i + 10, events.length - 1));
    if (key.leftArrow)  setIndex(i => Math.max(i - 10, 0));
    if (input === '+') setSpeed(s => Math.min(s * 2, 4));
    if (input === '-') setSpeed(s => Math.max(s / 2, 0.5));
  });

  useEffect(() => {
    if (paused || events.length === 0) return;
    if (index >= events.length - 1) { exit(); return; }
    const delay = Math.max(50, 1000 / speed);
    const t = setTimeout(() => setIndex(i => i + 1), delay);
    return () => clearTimeout(t);
  }, [index, paused, speed, events.length]);

  return React.createElement(ReplayFrame, { events, currentIndex: index, total: events.length, speed, paused });
}

export async function replayVisual(root: string, sessionId: string, opts: { speed?: number }): Promise<void> {
  const events = readEvents(root, sessionId);
  if (events.length === 0) { process.stdout.write(`no events in session ${sessionId}\n`); return; }
  const speed = opts.speed ?? 1;
  const { waitUntilExit } = render(React.createElement(ReplayApp, { events, speed }));
  await waitUntilExit();
}

export async function replayReconstruct(root: string, sessionId: string, opts: { stopAt?: number }): Promise<void> {
  const events = readEvents(root, sessionId);
  const sock = path.join(root, '.coordify', 'runtime', 'core.sock');
  const tok = (() => { try { return fs.readFileSync(path.join(root, '.coordify', 'runtime', 'session.token'), 'utf8').trim(); } catch { return ''; } })();
  if (!tok) { process.stdout.write('error: no session token\n'); return; }

  const limit = opts.stopAt ?? events.length;
  const slice = events.slice(0, limit);

  process.stdout.write(`Reconstructing session ${sessionId}...\n`);
  process.stdout.write(`  Submitting ${slice.length}/${events.length} events\n`);

  for (let i = 0; i < slice.length; i++) {
    const ev = slice[i];
    await new Promise<void>((resolve, reject) => {
      const s = net.createConnection(sock);
      let buf = '';
      const id = 'r' + i;
      s.setEncoding('utf8');
      s.once('connect', () => {
        s.write(JSON.stringify({ id, token: tok, action: 'submit_event', capVersion: '0.1', event: ev }) + '\n');
      });
      s.on('data', (d: string) => {
        buf += d;
        let idx: number;
        while ((idx = buf.indexOf('\n')) >= 0) {
          const line = buf.slice(0, idx); buf = buf.slice(idx + 1);
          if (!line.trim()) continue;
          try { JSON.parse(line); } catch {}
          s.destroy(); resolve();
        }
      });
      s.on('error', reject);
    });
    process.stdout.write(`  Submitted event ${i + 1}/${slice.length}: ${String(ev.type ?? '?')}\n`);
  }

  if (opts.stopAt && opts.stopAt < events.length) {
    process.stdout.write(`Core is running. Use 'coordify watch' to inspect state.\n`);
  } else {
    process.stdout.write(`Reconstruction complete.\n`);
  }
}
```

- [ ] **Step 5: Run tests and verify they pass**

```bash
cd packages/coordify-sim && npx tsx --test test/replayer.test.ts test/tui/replay-watch.test.tsx 2>&1 | tail -8
```

Expected: `# tests 3`, `# pass 3`, `# fail 0`.

- [ ] **Step 6: Commit**

```bash
git add packages/coordify-sim/src/replayer.ts packages/coordify-sim/src/tui/ packages/coordify-sim/test/replayer.test.ts packages/coordify-sim/test/tui/
git commit -m "feat(sim): Replayer — visual ink playback + event reconstruction"
```

---

### Task 4: CLI entry point

Wires `simulate` and `replay` commands into a single binary. Handles `--dry-run`, `--no-finalize`, `--visual`, `--reconstruct`, `--stop-at`, `--speed`, `--core-bin`.

**Files:**
- Create: `packages/coordify-sim/src/cli.ts`
- Create: `packages/coordify-sim/test/cli.test.ts`

**Interfaces:**
- Consumes: `CoreManager`, `validateScript`, `runScenario`, `replayVisual`, `replayReconstruct`
- Produces: binary `coordify-sim` or subcommands callable via the main `coordify` CLI's `simulate`/`replay` dispatch

- [ ] **Step 1: Write the failing test**

Create `packages/coordify-sim/test/cli.test.ts`:

```typescript
import test from 'node:test';
import assert from 'node:assert';
import { execSync } from 'child_process';
import path from 'path';
import os from 'os';
import fs from 'fs';

const cli = path.resolve('src/cli.ts');
const run = (args: string) => {
  try { return execSync(`npx tsx ${cli} ${args}`, { encoding: 'utf8' }); }
  catch (e: any) { return e.stdout ?? e.stderr ?? ''; }
};

test('--help prints commands', () => {
  const out = run('--help');
  assert.ok(out.includes('simulate') || out.includes('replay') || out.includes('coordify'));
});

test('simulate --dry-run with valid script prints steps', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'sim-cli-'));
  const scriptPath = path.join(root, 'test.json');
  fs.writeFileSync(scriptPath, JSON.stringify({
    name: 'test', agents: ['a1'],
    steps: [{ delay_ms: 0, event: { type: 'AGENT_JOINED', agentId: 'a1' } }]
  }));
  const out = run(`simulate ${scriptPath} --dry-run --root ${root}`);
  assert.ok(out.includes('dry-run') || out.includes('AGENT_JOINED') || out.includes('step'));
  fs.rmSync(root, { recursive: true });
});

test('simulate with invalid script prints errors and exits', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'sim-cli2-'));
  const scriptPath = path.join(root, 'bad.json');
  fs.writeFileSync(scriptPath, JSON.stringify({ agents: [] })); // missing name
  const out = run(`simulate ${scriptPath} --dry-run --root ${root}`);
  assert.ok(out.includes('error') || out.includes('name') || out.includes('invalid'));
  fs.rmSync(root, { recursive: true });
});
```

- [ ] **Step 2: Run to verify failure**

```bash
cd packages/coordify-sim && npx tsx --test test/cli.test.ts 2>&1 | tail -5
```

Expected: FAIL — cli.ts not found.

- [ ] **Step 3: Create `src/cli.ts`**

```typescript
#!/usr/bin/env node
import path from 'path';
import fs from 'fs';
import { CoreManager } from './core-manager.js';
import { validateScript } from './schema.js';
import { runScenario } from './runner.js';
import { replayVisual, replayReconstruct } from './replayer.js';

const HELP = `coordify-sim <command> [options]

Commands:
  simulate <script.json>   Run a JSON scenario script against Core
  replay <session-id>      Replay a past session (visual or reconstruct)

Simulate options:
  --dry-run        Validate and print steps; do not connect to Core
  --no-finalize    Skip AGENT_LEFT events at end
  --core-bin <p>   Path to coordify-core binary
  --root <dir>     Project root (default: cwd)

Replay options:
  --visual         Visual ink playback (default)
  --reconstruct    Re-submit events to live Core
  --speed <Nx>     Playback speed (0.5|1|2|4), visual only (default: 1)
  --stop-at <N>    Stop after N events, reconstruct only
  --root <dir>     Project root
`;

const argv = process.argv.slice(2);
const root = (() => {
  const i = argv.indexOf('--root');
  if (i >= 0 && argv[i + 1]) return path.resolve(argv[i + 1]);
  return process.env.COORDIFY_ROOT ? path.resolve(process.env.COORDIFY_ROOT) : process.cwd();
})();
const cmd = argv.find(a => !a.startsWith('-'));

function flag(name: string): boolean { return argv.includes(name); }
function opt(name: string): string | undefined {
  const i = argv.indexOf(name);
  return i >= 0 && argv[i + 1] ? argv[i + 1] : undefined;
}

async function main() {
  switch (cmd) {
    case 'simulate': {
      const scriptPath = argv.find(a => !a.startsWith('-') && a !== 'simulate');
      if (!scriptPath) { process.stdout.write('usage: coordify-sim simulate <script.json>\n'); process.exit(1); }
      let raw: unknown;
      try { raw = JSON.parse(fs.readFileSync(scriptPath, 'utf8')); }
      catch { process.stdout.write(`error: cannot read ${scriptPath}\n`); process.exit(1); return; }
      const result = validateScript(raw);
      if (Array.isArray(result)) {
        process.stdout.write('invalid script:\n' + result.map(e => `  ${e}`).join('\n') + '\n');
        process.exit(1); return;
      }
      const dryRun = flag('--dry-run');
      if (dryRun) {
        await runScenario({ socketPath: '', token: '', spawned: false }, result, { dryRun: true });
        return;
      }
      const binOverride = opt('--core-bin');
      const cm = new CoreManager(root, binOverride);
      const handle = await cm.ensure();
      try {
        await runScenario(handle, result, { noFinalize: flag('--no-finalize') });
      } finally {
        if (handle.spawned) await cm.stop();
      }
      break;
    }
    case 'replay': {
      const sessionId = argv.find(a => !a.startsWith('-') && a !== 'replay');
      if (!sessionId) { process.stdout.write('usage: coordify-sim replay <session-id>\n'); process.exit(1); }
      if (flag('--reconstruct')) {
        const stopAt = opt('--stop-at') ? Number(opt('--stop-at')) : undefined;
        await replayReconstruct(root, sessionId, { stopAt });
      } else {
        const speedStr = opt('--speed') ?? '1';
        const speed = parseFloat(speedStr);
        await replayVisual(root, sessionId, { speed: isNaN(speed) ? 1 : speed });
      }
      break;
    }
    case '--help':
    case undefined: process.stdout.write(HELP); break;
    default: process.stdout.write(`unknown command: ${cmd}\n\n${HELP}`);
  }
}

main().catch(e => { process.stderr.write(String(e) + '\n'); process.exit(1); });
```

- [ ] **Step 4: Run all tests**

```bash
cd packages/coordify-sim && npx tsx --test 'test/**/*.test.ts' 'test/**/*.test.tsx' 2>&1 | tail -10
```

Expected: all pass.

- [ ] **Step 5: Build and verify**

```bash
cd packages/coordify-sim && npm run build 2>&1 | tail -5
node dist/cli.js --help
```

Expected: `tsc` succeeds; `--help` prints command list.

- [ ] **Step 6: Commit**

```bash
git add packages/coordify-sim/src/cli.ts packages/coordify-sim/test/cli.test.ts
git commit -m "feat(sim): CLI entry point — simulate + replay commands"
git push origin main
```
