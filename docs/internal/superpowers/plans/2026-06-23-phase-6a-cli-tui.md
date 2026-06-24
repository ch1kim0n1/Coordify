# Phase 6a — CLI + TUI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a `coordify` CLI binary (TypeScript/Node.js) with all query commands, a live `watch` TUI, and a `graph` TUI — talking to Core over socket when live, falling back to JSON files when offline.

**Architecture:** A single `packages/coordify-cli` package with three layers: IPC client (socket request/response), file reader (JSON artifacts), and commands (query + ink TUI). Core is extended with a `get_state` action so the CLI can read live state. `coordify watch` polls `get_state` every 500ms; `coordify graph` reads the same data plus knowledge files.

**Tech Stack:** TypeScript 5, Node.js 20, ink 4 (React for terminals), `tsx` for tests (no compile step), `node:test` + `node:assert`.

## Global Constraints

- No emoji in any output. Colors via named palette: red=heat/danger, yellow=conflict, green=resolved/ok, gray=offline/empty, cyan=cool, blue=cooperative.
- `--json` flag on every command: dump raw data as JSON and exit (no ink rendering).
- Offline mode never throws: if socket absent or artifact missing, print one-line message and exit 0 (except malformed JSON → exit 1).
- `q` or Ctrl-C always exits cleanly (ink cleanup, socket close).
- Coverage gate: ≥ 90% lines on `src/ipc.ts`, `src/files.ts`, `src/commands/`.
- Core token file path: `.coordify/runtime/session.token` (from paths.rs).
- Core socket path: `.coordify/runtime/core.sock` (from paths.rs).
- Session dirs: `.coordify/sessions/<id>/` containing `stats.json`, `session-summary.json`, `heat-history.json`, `entertainment.json`, `events.log`.
- Knowledge dir: `.coordify/knowledge/` containing `hotzones.json`, `coupling-graph.json`, `agent-profiles.json`, `velocity-profiles.json`, `coordination-overhead.json`.
- ink version: `^4.4.1` (CJS-compatible; do NOT use v5 which is ESM-only).
- TypeScript `"module": "commonjs"`, `"target": "ES2022"`, `outDir: "dist"`. Entry: `dist/cli.js`.
- `tsx` version: `^4.7.0` for running tests without pre-compilation.

---

### Task 1: Core — `get_state` IPC action

Extend `coordify-core` so the CLI can read live state without extra complexity. Adds three helper methods and one new IPC action. All within `packages/coordify-core/src/`.

**Files:**
- Modify: `packages/coordify-core/src/claim.rs` — add `pub fn all_active()`
- Modify: `packages/coordify-core/src/heatstore.rs` — add `pub fn snapshot()`
- Modify: `packages/coordify-core/src/conflict.rs` — add `pub fn all_open()`
- Modify: `packages/coordify-core/src/server.rs` — add `"get_state"` match arm

**Interfaces:**
- Produces: IPC action `"get_state"` (no extra fields needed in request) → `Response::ok_with_data(id, json!({ "agents": [...], "claims": [...], "heat": [...], "conflicts": [...] }))`

- [ ] **Step 1: Write the failing test for `get_state`**

Add to `packages/coordify-core/src/server.rs` inside `mod tests`:

```rust
#[test]
fn get_state_returns_live_snapshot() {
    let s = Arc::new(make_shared());
    // Register agent and give it a claim
    let reg = handle_request(&s, &{ let mut r = req("good", "register"); r.meta = serde_json::json!({}); r });
    let agent_id = reg.agent_id.clone().unwrap();
    let mut claim_req = req("good", "submit_event");
    claim_req.cap_version = Some("0.1".into());
    claim_req.agent_id = Some(agent_id.clone());
    claim_req.event = serde_json::json!({
        "type": "CLAIM_PROPOSED",
        "agentId": agent_id,
        "taskSummary": "test task",
        "intent": "BUGFIX",
        "domains": ["src"],
        "estimatedFiles": ["src/x.rs"],
        "confidence": 0.9
    });
    handle_request(&s, &claim_req);

    let state_req = req("good", "get_state");
    let resp = handle_request(&s, &state_req);
    assert!(resp.ok);
    let data = resp.data.unwrap();
    let agents = data["agents"].as_array().unwrap();
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0]["agentId"], agent_id);
    assert!(data["claims"].as_array().unwrap().len() >= 1 || data["agents"][0]["claimId"].is_string());
    let heat = data["heat"].as_array().unwrap();
    assert!(heat.is_empty()); // no heat yet with one agent
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cd packages/coordify-core && cargo test get_state 2>&1 | tail -5
```

Expected: FAIL — `"get_state"` hits the `_ => Response::err("unknown action")` arm.

- [ ] **Step 3: Add `all_active()` to ClaimStore (`claim.rs`)**

```rust
// In impl ClaimStore:
pub fn all_active(&self) -> impl Iterator<Item = &Claim> {
    self.claims.values().filter(|c| c.orphaned_at_ms.is_none())
}
```

- [ ] **Step 4: Add `snapshot()` to HeatStore (`heatstore.rs`)**

```rust
// In impl HeatStore:
pub fn snapshot(&self) -> Vec<serde_json::Value> {
    self.edges
        .iter()
        .map(|((a, b), r)| serde_json::json!({
            "pair": [a, b],
            "heat": r.heat,
            "band": r.band.as_str()
        }))
        .collect()
}
```

- [ ] **Step 5: Add `all_open()` to ConflictStore (`conflict.rs`)**

```rust
// In impl ConflictStore:
pub fn all_open(&self) -> Vec<&Conflict> {
    self.conflicts.values().collect()
}
```

- [ ] **Step 6: Add `"get_state"` arm in `server.rs` `handle_request`**

Add before the `_ =>` catch-all:

```rust
"get_state" => {
    let (agents_json, claims_json) = {
        let st = shared.state.lock().unwrap();
        let agents = st.agent_ids().into_iter().map(|id| {
            let state_val = st.agent_state(&id)
                .map(|s| serde_json::to_value(s).unwrap_or(serde_json::Value::Null))
                .unwrap_or(serde_json::Value::Null);
            let claim_id = st.claims.live_claim_for(&id).map(|c| c.claim_id.clone());
            serde_json::json!({ "agentId": id, "state": state_val, "claimId": claim_id })
        }).collect::<Vec<_>>();
        let claims = st.claims.all_active().map(|c| serde_json::json!({
            "claimId": c.claim_id,
            "agentId": c.agent_id,
            "files": c.actual_files.iter().chain(c.estimated_files.iter())
                        .collect::<std::collections::BTreeSet<_>>()
        })).collect::<Vec<_>>();
        (agents, claims)
    };
    let heat_json = shared.heat.lock().unwrap().snapshot();
    let conflicts_json = {
        shared.conflicts.lock().unwrap().all_open().iter().map(|c| {
            let age_ms = now_ms().saturating_sub(c.opened_at_ms);
            serde_json::json!({
                "conflictId": c.conflict_id,
                "agents": [c.agents.0, c.agents.1],
                "paths": c.paths,
                "state": c.state.as_str(),
                "ageMs": age_ms
            })
        }).collect::<Vec<_>>()
    };
    Response::ok_with_data(&req.id, serde_json::json!({
        "agents": agents_json,
        "claims": claims_json,
        "heat": heat_json,
        "conflicts": conflicts_json
    }))
}
```

- [ ] **Step 7: Run tests and verify they pass**

```bash
cd packages/coordify-core && cargo test 2>&1 | tail -5
```

Expected: `test result: ok. N passed; 0 failed`

- [ ] **Step 8: Commit**

```bash
git add packages/coordify-core/src/claim.rs packages/coordify-core/src/heatstore.rs packages/coordify-core/src/conflict.rs packages/coordify-core/src/server.rs
git commit -m "feat(core): get_state IPC action + helper iterators for CLI"
```

---

### Task 2: CLI package scaffold + IPC client

Creates the `coordify-cli` package with TypeScript config, path helpers, and the IPC client (CJS-style CoreClient ported to TypeScript).

**Files:**
- Create: `packages/coordify-cli/package.json`
- Create: `packages/coordify-cli/tsconfig.json`
- Create: `packages/coordify-cli/src/paths.ts`
- Create: `packages/coordify-cli/src/ipc.ts`
- Create: `packages/coordify-cli/test/ipc.test.ts`

**Interfaces:**
- Produces: `ipc.isLive(root)`, `ipc.query(root, action, payload?)`, `ipc.CoreClient` class with `connect()`, `query(action, payload?)`, `close()`

- [ ] **Step 1: Write the failing test**

Create `packages/coordify-cli/test/ipc.test.ts`:

```typescript
import test from 'node:test';
import assert from 'node:assert';
import net from 'net';
import os from 'os';
import path from 'path';
import fs from 'fs';
import { CoreClient, isLive, query } from '../src/ipc.js';

function tmpSock(): string {
  const d = fs.mkdtempSync(path.join(os.tmpdir(), 'cli-ipc-'));
  return path.join(d, 's.sock');
}

function fakeCore(sockPath: string, handler: (req: Record<string, unknown>) => Record<string, unknown>) {
  const server = net.createServer(conn => {
    conn.setEncoding('utf8');
    let buf = '';
    conn.on('data', (d: string) => {
      buf += d;
      let i: number;
      while ((i = buf.indexOf('\n')) >= 0) {
        const line = buf.slice(0, i); buf = buf.slice(i + 1);
        if (!line.trim()) continue;
        const resp = handler(JSON.parse(line));
        conn.write(JSON.stringify(resp) + '\n');
      }
    });
    conn.on('error', () => {});
  });
  return new Promise<net.Server>(resolve => server.listen(sockPath, () => resolve(server)));
}

test('isLive returns false when socket absent', () => {
  assert.equal(isLive('/nonexistent/root'), false);
});

test('CoreClient.query sends request and resolves response', async () => {
  const sock = tmpSock();
  const server = await fakeCore(sock, req => ({
    id: (req as any).id, ok: true, data: { agents: [] }
  }));
  const client = new CoreClient(sock, 'tok');
  await client.connect();
  const resp = await client.query('get_state');
  assert.equal(resp.ok, true);
  assert.deepEqual(resp.data, { agents: [] });
  client.close();
  server.close();
});

test('query() helper opens, requests, closes', async () => {
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'cli-q-'));
  const sockPath = path.join(tmpDir, '.coordify', 'runtime', 'core.sock');
  fs.mkdirSync(path.dirname(sockPath), { recursive: true });
  fs.writeFileSync(path.join(tmpDir, '.coordify', 'runtime', 'session.token'), 'tok-abc');
  const server = await fakeCore(sockPath, req => ({ id: (req as any).id, ok: true, data: { msg: 'hi' } }));
  const resp = await query(tmpDir, 'get_state');
  assert.equal(resp.ok, true);
  assert.deepEqual(resp.data, { msg: 'hi' });
  server.close();
  fs.rmSync(tmpDir, { recursive: true });
});
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cd packages/coordify-cli && npx tsx --test test/ipc.test.ts 2>&1 | tail -5
```

Expected: FAIL — module not found.

- [ ] **Step 3: Create `package.json`**

```json
{
  "name": "coordify-cli",
  "version": "0.1.0",
  "private": true,
  "description": "Coordify CLI — query, watch, graph",
  "main": "dist/cli.js",
  "bin": { "coordify": "dist/cli.js" },
  "scripts": {
    "build": "tsc",
    "test": "npx tsx --test test/**/*.test.ts"
  },
  "dependencies": {
    "ink": "^4.4.1",
    "react": "^18.2.0"
  },
  "devDependencies": {
    "@types/node": "^20.0.0",
    "@types/react": "^18.2.0",
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

- [ ] **Step 5: Create `src/paths.ts`**

```typescript
import path from 'path';
import fs from 'fs';

export const coordify = (root: string) => path.join(root, '.coordify');
export const runtime = (root: string) => path.join(coordify(root), 'runtime');
export const socket = (root: string) => path.join(runtime(root), 'core.sock');
export const token = (root: string) => path.join(runtime(root), 'session.token');
export const sessions = (root: string) => path.join(coordify(root), 'sessions');
export const sessionDir = (root: string, id: string) => path.join(sessions(root), id);
export const knowledgeDir = (root: string) => path.join(coordify(root), 'knowledge');

export function readToken(root: string): string | null {
  try { return fs.readFileSync(token(root), 'utf8').trim(); } catch { return null; }
}
```

- [ ] **Step 6: Create `src/ipc.ts`**

```typescript
import net from 'net';
import fs from 'fs';
import { socket as socketPath, readToken } from './paths.js';

export interface IpcResponse {
  id: string;
  ok: boolean;
  agent_id?: string;
  error?: string;
  data?: unknown;
}

export function isLive(root: string): boolean {
  return fs.existsSync(socketPath(root));
}

export class CoreClient {
  private sock: net.Socket | null = null;
  private buf = '';
  private seq = 0;
  private pending = new Map<string, (r: IpcResponse) => void>();

  constructor(private readonly sockPath: string, private readonly tok: string) {}

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
          let resp: IpcResponse;
          try { resp = JSON.parse(line); } catch { continue; }
          const resolve = this.pending.get(resp.id);
          if (resolve) { this.pending.delete(resp.id); resolve(resp); }
        }
      });
    });
  }

  query(action: string, payload: Record<string, unknown> = {}): Promise<IpcResponse> {
    return new Promise((resolve, reject) => {
      if (!this.sock) { reject(new Error('not connected')); return; }
      const id = 'q' + (++this.seq);
      this.pending.set(id, resolve);
      const msg = JSON.stringify({ id, token: this.tok, action, ...payload }) + '\n';
      this.sock.write(msg, err => { if (err) { this.pending.delete(id); reject(err); } });
    });
  }

  close(): void {
    try { this.sock?.end(); } catch (_) {}
    this.sock = null;
  }
}

export async function query(root: string, action: string, payload: Record<string, unknown> = {}): Promise<IpcResponse> {
  const tok = readToken(root);
  if (!tok) return { id: '?', ok: false, error: 'no token' };
  const sock = socketPath(root);
  const client = new CoreClient(sock, tok);
  try {
    await client.connect();
    const resp = await client.query(action, payload);
    return resp;
  } finally {
    client.close();
  }
}
```

- [ ] **Step 7: Run tests and verify they pass**

```bash
cd packages/coordify-cli && npm install && npx tsx --test test/ipc.test.ts 2>&1 | tail -8
```

Expected: `# tests 3`, `# pass 3`, `# fail 0`.

- [ ] **Step 8: Commit**

```bash
git add packages/coordify-cli/
git commit -m "feat(cli): package scaffold, IPC client, path helpers"
```

---

### Task 3: File reader

Reads `.coordify` JSON artifacts for offline commands. Session discovery, stats, summary, heat-history, entertainment, knowledge files.

**Files:**
- Create: `packages/coordify-cli/src/files.ts`
- Create: `packages/coordify-cli/test/files.test.ts`

**Interfaces:**
- Produces: `files.latestSession(root)`, `files.readStats(root, id)`, `files.readSummary(root, id)`, `files.readHeatHistory(root, id)`, `files.readEntertainment(root, id)`, `files.readEventLog(root, id)`, `files.readKnowledge(root)`, `files.listSessions(root)`

- [ ] **Step 1: Write the failing tests**

Create `packages/coordify-cli/test/files.test.ts`:

```typescript
import test from 'node:test';
import assert from 'node:assert';
import os from 'os';
import path from 'path';
import fs from 'fs';
import { listSessions, latestSession, readStats, readKnowledge } from '../src/files.js';

function tmpRoot(): string {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'cli-files-'));
  // scaffold .coordify/sessions/2026-06-23_12-00-00/stats.json
  const sid = '2026-06-23_12-00-00';
  const sdir = path.join(dir, '.coordify', 'sessions', sid);
  fs.mkdirSync(sdir, { recursive: true });
  fs.writeFileSync(path.join(sdir, 'stats.json'), JSON.stringify({
    agentsSeen: 2, claimsCreated: 3, peakHeat: { heat: 82, pair: ['a', 'b'] }
  }));
  // knowledge
  const kdir = path.join(dir, '.coordify', 'knowledge');
  fs.mkdirSync(kdir, { recursive: true });
  fs.writeFileSync(path.join(kdir, 'hotzones.json'), JSON.stringify({ 'src/x.rs': 3 }));
  return dir;
}

test('listSessions returns sorted session ids', () => {
  const root = tmpRoot();
  const sessions = listSessions(root);
  assert.equal(sessions.length, 1);
  assert.equal(sessions[0], '2026-06-23_12-00-00');
  fs.rmSync(root, { recursive: true });
});

test('latestSession returns last session id', () => {
  const root = tmpRoot();
  const id = latestSession(root);
  assert.equal(id, '2026-06-23_12-00-00');
  fs.rmSync(root, { recursive: true });
});

test('latestSession returns null when no sessions', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'cli-empty-'));
  assert.equal(latestSession(root), null);
  fs.rmSync(root, { recursive: true });
});

test('readStats returns parsed JSON', () => {
  const root = tmpRoot();
  const stats = readStats(root, '2026-06-23_12-00-00');
  assert.equal(stats?.agentsSeen, 2);
  fs.rmSync(root, { recursive: true });
});

test('readStats returns null for missing session', () => {
  const root = tmpRoot();
  assert.equal(readStats(root, 'nonexistent'), null);
  fs.rmSync(root, { recursive: true });
});

test('readKnowledge returns hotzones', () => {
  const root = tmpRoot();
  const k = readKnowledge(root);
  assert.equal(k.hotzones?.['src/x.rs'], 3);
  fs.rmSync(root, { recursive: true });
});
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd packages/coordify-cli && npx tsx --test test/files.test.ts 2>&1 | tail -5
```

Expected: FAIL — module not found.

- [ ] **Step 3: Create `src/files.ts`**

```typescript
import fs from 'fs';
import path from 'path';
import { sessionDir, sessions, knowledgeDir } from './paths.js';

function readJson<T>(filePath: string): T | null {
  try { return JSON.parse(fs.readFileSync(filePath, 'utf8')) as T; } catch { return null; }
}

export function listSessions(root: string): string[] {
  const dir = sessions(root);
  try { return fs.readdirSync(dir).sort(); } catch { return []; }
}

export function latestSession(root: string): string | null {
  const s = listSessions(root);
  return s.length > 0 ? s[s.length - 1] : null;
}

export function readStats(root: string, id: string): Record<string, unknown> | null {
  return readJson(path.join(sessionDir(root, id), 'stats.json'));
}

export function readSummary(root: string, id: string): Record<string, unknown> | null {
  return readJson(path.join(sessionDir(root, id), 'session-summary.json'));
}

export function readHeatHistory(root: string, id: string): unknown[] | null {
  return readJson(path.join(sessionDir(root, id), 'heat-history.json'));
}

export function readEntertainment(root: string, id: string): Record<string, unknown> | null {
  return readJson(path.join(sessionDir(root, id), 'entertainment.json'));
}

export function readEventLog(root: string, id: string): string[] {
  const p = path.join(sessionDir(root, id), 'events.log');
  try { return fs.readFileSync(p, 'utf8').split('\n').filter(l => l.trim()); } catch { return []; }
}

export function readKnowledge(root: string): Record<string, unknown> {
  const dir = knowledgeDir(root);
  return {
    hotzones: readJson(path.join(dir, 'hotzones.json')),
    coupling: readJson(path.join(dir, 'coupling-graph.json')),
    profiles: readJson(path.join(dir, 'agent-profiles.json')),
    velocity: readJson(path.join(dir, 'velocity-profiles.json')),
    overhead: readJson(path.join(dir, 'coordination-overhead.json')),
  };
}
```

- [ ] **Step 4: Run tests and verify they pass**

```bash
cd packages/coordify-cli && npx tsx --test test/files.test.ts 2>&1 | tail -5
```

Expected: `# tests 6`, `# pass 6`, `# fail 0`.

- [ ] **Step 5: Commit**

```bash
git add packages/coordify-cli/src/files.ts packages/coordify-cli/test/files.test.ts
git commit -m "feat(cli): file reader — session list, stats, knowledge, event log"
```

---

### Task 4: Query commands — status, agents, heat, claims, conflicts, logs

One function per command; each resolves data (live or offline) and prints a formatted table or list. `--json` flag dumps raw JSON.

**Files:**
- Create: `packages/coordify-cli/src/commands/status.ts`
- Create: `packages/coordify-cli/src/commands/agents.ts`
- Create: `packages/coordify-cli/src/commands/heat.ts`
- Create: `packages/coordify-cli/src/commands/claims.ts`
- Create: `packages/coordify-cli/src/commands/conflicts.ts`
- Create: `packages/coordify-cli/src/commands/logs.ts`
- Create: `packages/coordify-cli/test/commands/query.test.ts`

**Interfaces:**
- Consumes: `ipc.query(root, action)`, `ipc.isLive(root)`, `files.latestSession(root)`, `files.readStats(root, id)`, `files.readEventLog(root, id)`
- Produces: `runStatus(root, opts)`, `runAgents(root, opts)`, `runHeat(root, opts)`, `runClaims(root, opts)`, `runConflicts(root, opts)`, `runLogs(root, opts)` — all `async (root: string, opts: {json?: boolean, tail?: number, follow?: boolean}) => Promise<void>`

- [ ] **Step 1: Write the failing tests**

Create `packages/coordify-cli/test/commands/query.test.ts`:

```typescript
import test from 'node:test';
import assert from 'node:assert';
import net from 'net';
import os from 'os';
import path from 'path';
import fs from 'fs';

// Helper: scaffold a fake root with socket + token + session artifacts
function fakeRoot(handler: (req: any) => any): { root: string; close: () => void } {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'cli-cmd-'));
  const sockPath = path.join(root, '.coordify', 'runtime', 'core.sock');
  fs.mkdirSync(path.dirname(sockPath), { recursive: true });
  fs.writeFileSync(path.join(root, '.coordify', 'runtime', 'session.token'), 'tok');
  const server = net.createServer(conn => {
    conn.setEncoding('utf8');
    let buf = '';
    conn.on('data', (d: string) => {
      buf += d;
      let i: number;
      while ((i = buf.indexOf('\n')) >= 0) {
        const line = buf.slice(0, i); buf = buf.slice(i + 1);
        if (!line.trim()) continue;
        conn.write(JSON.stringify(handler(JSON.parse(line))) + '\n');
      }
    });
  });
  server.listen(sockPath);
  return { root, close: () => { server.close(); fs.rmSync(root, { recursive: true }); } };
}

test('runStatus live: prints socket status and agent count', async () => {
  const { root, close } = fakeRoot(req => ({
    id: req.id, ok: true,
    data: { agents: [{ agentId: 'a1', state: 'ACTIVE' }], claims: [], heat: [], conflicts: [] }
  }));
  const { runStatus } = await import('../../src/commands/status.js');
  let out = '';
  const orig = process.stdout.write.bind(process.stdout);
  (process.stdout as any).write = (s: string) => { out += s; return true; };
  await runStatus(root, {});
  (process.stdout as any).write = orig;
  assert.ok(out.includes('1') || out.includes('agent'), `output: ${out}`);
  close();
});

test('runStatus offline: falls back to last session stats', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'cli-off-'));
  const sdir = path.join(root, '.coordify', 'sessions', '2026-06-23_00-00-00');
  fs.mkdirSync(sdir, { recursive: true });
  fs.writeFileSync(path.join(sdir, 'stats.json'), JSON.stringify({ agentsSeen: 3, claimsCreated: 5, peakHeat: { heat: 50 }, conflictsOpened: 1 }));
  const { runStatus } = await import('../../src/commands/status.js');
  let out = '';
  const orig = process.stdout.write.bind(process.stdout);
  (process.stdout as any).write = (s: string) => { out += s; return true; };
  await runStatus(root, {});
  (process.stdout as any).write = orig;
  assert.ok(out.includes('offline') || out.includes('3') || out.includes('session'), `output: ${out}`);
  fs.rmSync(root, { recursive: true });
});

test('runLogs prints events from log file', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'cli-logs-'));
  const sdir = path.join(root, '.coordify', 'sessions', '2026-06-23_00-00-00');
  fs.mkdirSync(sdir, { recursive: true });
  fs.writeFileSync(path.join(sdir, 'events.log'), [
    JSON.stringify({ type: 'AGENT_JOINED', agentId: 'a1', ts: '2026-06-23T00:00:00Z' }),
    JSON.stringify({ type: 'CLAIM_CREATED', agentId: 'a1', ts: '2026-06-23T00:00:01Z' }),
  ].join('\n'));
  const { runLogs } = await import('../../src/commands/logs.js');
  let out = '';
  const orig = process.stdout.write.bind(process.stdout);
  (process.stdout as any).write = (s: string) => { out += s; return true; };
  await runLogs(root, { tail: 5 });
  (process.stdout as any).write = orig;
  assert.ok(out.includes('AGENT_JOINED'));
  assert.ok(out.includes('CLAIM_CREATED'));
  fs.rmSync(root, { recursive: true });
});
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd packages/coordify-cli && npx tsx --test test/commands/query.test.ts 2>&1 | tail -5
```

Expected: FAIL — modules not found.

- [ ] **Step 3: Create `src/commands/status.ts`**

```typescript
import { isLive, query } from '../ipc.js';
import { latestSession, readStats } from '../files.js';

export async function runStatus(root: string, opts: { json?: boolean }): Promise<void> {
  if (isLive(root)) {
    const resp = await query(root, 'get_state');
    if (!resp.ok) { process.stdout.write(`error: ${resp.error}\n`); return; }
    const d = resp.data as any;
    if (opts.json) { process.stdout.write(JSON.stringify(d, null, 2) + '\n'); return; }
    process.stdout.write(`status: live\nagents: ${d.agents?.length ?? 0}\nclaims: ${d.claims?.length ?? 0}\nconflicts: ${d.conflicts?.length ?? 0}\npeak heat: ${d.heat?.map((h: any) => `${h.pair?.join('↔')} ${h.heat}`).join(', ') || 'none'}\n`);
  } else {
    const id = latestSession(root);
    if (!id) { process.stdout.write('status: offline (no sessions found)\n'); return; }
    const stats = readStats(root, id) as any;
    if (!stats) { process.stdout.write(`status: offline (no stats for ${id})\n`); return; }
    if (opts.json) { process.stdout.write(JSON.stringify(stats, null, 2) + '\n'); return; }
    process.stdout.write(`status: offline (last session: ${id})\nagents seen: ${stats.agentsSeen ?? 0}\nclaims: ${stats.claimsCreated ?? 0}\nconflicts: ${stats.conflictsOpened ?? 0}\npeak heat: ${stats.peakHeat?.heat ?? 0}\n`);
  }
}
```

- [ ] **Step 4: Create `src/commands/agents.ts`**

```typescript
import { isLive, query } from '../ipc.js';
import { latestSession, readStats } from '../files.js';

export async function runAgents(root: string, opts: { json?: boolean }): Promise<void> {
  if (isLive(root)) {
    const resp = await query(root, 'get_state');
    if (!resp.ok) { process.stdout.write(`error: ${resp.error}\n`); return; }
    const agents = (resp.data as any)?.agents ?? [];
    if (opts.json) { process.stdout.write(JSON.stringify(agents, null, 2) + '\n'); return; }
    if (agents.length === 0) { process.stdout.write('no agents\n'); return; }
    process.stdout.write('AGENT ID        STATE       CLAIM\n');
    for (const a of agents) {
      process.stdout.write(`${String(a.agentId).padEnd(16)}${String(a.state).padEnd(12)}${a.claimId ?? '-'}\n`);
    }
  } else {
    const id = latestSession(root);
    const stats = id ? readStats(root, id) as any : null;
    if (opts.json) { process.stdout.write(JSON.stringify(stats?.agents ?? {}, null, 2) + '\n'); return; }
    process.stdout.write('offline — showing last session per-agent tallies\n');
    const agents = Object.entries(stats?.agents ?? {});
    for (const [aid, t] of agents) {
      process.stdout.write(`${String(aid).padEnd(16)}sessions: ${(t as any).sessions ?? 0}\n`);
    }
  }
}
```

- [ ] **Step 5: Create `src/commands/heat.ts`**

```typescript
import { isLive, query } from '../ipc.js';
import { latestSession, readHeatHistory } from '../files.js';

export async function runHeat(root: string, opts: { json?: boolean }): Promise<void> {
  let edges: any[] = [];
  if (isLive(root)) {
    const resp = await query(root, 'get_state');
    if (!resp.ok) { process.stdout.write(`error: ${resp.error}\n`); return; }
    edges = ((resp.data as any)?.heat ?? []).sort((a: any, b: any) => b.heat - a.heat);
  } else {
    const id = latestSession(root);
    const history = id ? readHeatHistory(root, id) as any[] : [];
    // Last entry per pair
    const byPair = new Map<string, any>();
    for (const e of (history ?? [])) { byPair.set((e.pair ?? []).join('↔'), e); }
    edges = [...byPair.values()].sort((a, b) => b.heat - a.heat);
  }
  if (opts.json) { process.stdout.write(JSON.stringify(edges, null, 2) + '\n'); return; }
  if (edges.length === 0) { process.stdout.write('no heat data\n'); return; }
  process.stdout.write('PAIR                          HEAT   BAND\n');
  for (const e of edges) {
    const pair = (e.pair ?? []).join(' ↔ ');
    process.stdout.write(`${String(pair).padEnd(30)}${String(e.heat).padEnd(7)}${e.band ?? ''}\n`);
  }
}
```

- [ ] **Step 6: Create `src/commands/claims.ts`**

```typescript
import { isLive, query } from '../ipc.js';

export async function runClaims(root: string, opts: { json?: boolean }): Promise<void> {
  if (!isLive(root)) { process.stdout.write('claims: no live network\n'); return; }
  const resp = await query(root, 'get_state');
  if (!resp.ok) { process.stdout.write(`error: ${resp.error}\n`); return; }
  const claims = (resp.data as any)?.claims ?? [];
  if (opts.json) { process.stdout.write(JSON.stringify(claims, null, 2) + '\n'); return; }
  if (claims.length === 0) { process.stdout.write('no active claims\n'); return; }
  process.stdout.write('CLAIM ID        AGENT           FILES\n');
  for (const c of claims) {
    const files = (c.files ?? []).slice(0, 3).join(', ') + ((c.files ?? []).length > 3 ? '...' : '');
    process.stdout.write(`${String(c.claimId).padEnd(16)}${String(c.agentId).padEnd(16)}${files}\n`);
  }
}
```

- [ ] **Step 7: Create `src/commands/conflicts.ts`**

```typescript
import { isLive, query } from '../ipc.js';

export async function runConflicts(root: string, opts: { json?: boolean }): Promise<void> {
  if (!isLive(root)) { process.stdout.write('conflicts: no live network\n'); return; }
  const resp = await query(root, 'get_state');
  if (!resp.ok) { process.stdout.write(`error: ${resp.error}\n`); return; }
  const conflicts = (resp.data as any)?.conflicts ?? [];
  if (opts.json) { process.stdout.write(JSON.stringify(conflicts, null, 2) + '\n'); return; }
  if (conflicts.length === 0) { process.stdout.write('no active conflicts\n'); return; }
  process.stdout.write('CONFLICT ID     AGENTS                STATE               AGE\n');
  for (const c of conflicts) {
    const agents = (c.agents ?? []).join(',');
    const age = c.ageMs ? `${Math.round(c.ageMs / 1000)}s` : '?';
    process.stdout.write(`${String(c.conflictId).padEnd(16)}${String(agents).padEnd(22)}${String(c.state).padEnd(20)}${age}\n`);
  }
}
```

- [ ] **Step 8: Create `src/commands/logs.ts`**

```typescript
import { isLive } from '../ipc.js';
import { latestSession, readEventLog } from '../files.js';

export async function runLogs(root: string, opts: { tail?: number; follow?: boolean; json?: boolean }): Promise<void> {
  const id = latestSession(root);
  if (!id) { process.stdout.write('no sessions found\n'); return; }
  const lines = readEventLog(root, id);
  const tail = opts.tail ?? 20;
  const slice = lines.slice(-tail);
  if (opts.json) { process.stdout.write(JSON.stringify(slice.map(l => { try { return JSON.parse(l); } catch { return l; } }), null, 2) + '\n'); return; }
  for (const line of slice) {
    try {
      const e = JSON.parse(line);
      const ts = e.ts ? e.ts.replace('T', ' ').replace('Z', '') : '?';
      const fields = Object.entries(e).filter(([k]) => !['type', 'ts'].includes(k)).map(([k, v]) => `${k}=${JSON.stringify(v)}`).join(' ');
      process.stdout.write(`[${ts}] ${e.type} ${fields}\n`);
    } catch {
      process.stdout.write(line + '\n');
    }
  }
  if (opts.follow && isLive(root)) {
    // tail -f: re-check every 500ms, print new lines
    let known = lines.length;
    const interval = setInterval(() => {
      const fresh = readEventLog(root, id);
      if (fresh.length > known) {
        fresh.slice(known).forEach(l => process.stdout.write(l + '\n'));
        known = fresh.length;
      }
    }, 500);
    process.on('SIGINT', () => { clearInterval(interval); process.exit(0); });
    await new Promise(() => {}); // wait for SIGINT
  }
}
```

- [ ] **Step 9: Run tests and verify they pass**

```bash
cd packages/coordify-cli && npx tsx --test test/commands/query.test.ts 2>&1 | tail -8
```

Expected: `# tests 3`, `# pass 3`, `# fail 0`.

- [ ] **Step 10: Commit**

```bash
git add packages/coordify-cli/src/commands/
git add packages/coordify-cli/test/commands/query.test.ts
git commit -m "feat(cli): query commands — status, agents, heat, claims, conflicts, logs"
```

---

### Task 5: Session + stats commands

**Files:**
- Create: `packages/coordify-cli/src/commands/stats.ts`
- Create: `packages/coordify-cli/src/commands/session.ts`
- Create: `packages/coordify-cli/test/commands/session.test.ts`

**Interfaces:**
- Produces: `runStats(root, opts)`, `runSessionList(root, opts)`, `runSessionInspect(root, id, opts)` — same signature pattern.

- [ ] **Step 1: Write the failing tests**

Create `packages/coordify-cli/test/commands/session.test.ts`:

```typescript
import test from 'node:test';
import assert from 'node:assert';
import os from 'os';
import path from 'path';
import fs from 'fs';

function makeSession(root: string, id: string) {
  const sdir = path.join(root, '.coordify', 'sessions', id);
  fs.mkdirSync(sdir, { recursive: true });
  fs.writeFileSync(path.join(sdir, 'stats.json'), JSON.stringify({ agentsSeen: 2, claimsCreated: 3, conflictsOpened: 1, durationMs: 9000, peakHeat: { heat: 82 } }));
  fs.writeFileSync(path.join(sdir, 'session-summary.json'), JSON.stringify({ narrative: 'Good session.' }));
  fs.writeFileSync(path.join(sdir, 'entertainment.json'), JSON.stringify({ badges: [], leaderboards: [], narrative: 'Good session.' }));
}

test('runSessionList prints session ids', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'cli-sess-'));
  makeSession(root, '2026-06-23_10-00-00');
  makeSession(root, '2026-06-23_11-00-00');
  const { runSessionList } = await import('../../src/commands/session.js');
  let out = '';
  const orig = process.stdout.write.bind(process.stdout);
  (process.stdout as any).write = (s: string) => { out += s; return true; };
  await runSessionList(root, {});
  (process.stdout as any).write = orig;
  assert.ok(out.includes('2026-06-23_10-00-00'));
  assert.ok(out.includes('2026-06-23_11-00-00'));
  fs.rmSync(root, { recursive: true });
});

test('runSessionInspect prints stats and narrative', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'cli-inspect-'));
  makeSession(root, '2026-06-23_10-00-00');
  const { runSessionInspect } = await import('../../src/commands/session.js');
  let out = '';
  const orig = process.stdout.write.bind(process.stdout);
  (process.stdout as any).write = (s: string) => { out += s; return true; };
  await runSessionInspect(root, '2026-06-23_10-00-00', {});
  (process.stdout as any).write = orig;
  assert.ok(out.includes('Good session.') || out.includes('82') || out.includes('agents'));
  fs.rmSync(root, { recursive: true });
});

test('runStats prints latest session stats', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'cli-stats-'));
  makeSession(root, '2026-06-23_10-00-00');
  const { runStats } = await import('../../src/commands/stats.js');
  let out = '';
  const orig = process.stdout.write.bind(process.stdout);
  (process.stdout as any).write = (s: string) => { out += s; return true; };
  await runStats(root, {});
  (process.stdout as any).write = orig;
  assert.ok(out.includes('2') || out.includes('82') || out.includes('agent'));
  fs.rmSync(root, { recursive: true });
});
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd packages/coordify-cli && npx tsx --test test/commands/session.test.ts 2>&1 | tail -5
```

Expected: FAIL — modules not found.

- [ ] **Step 3: Create `src/commands/stats.ts`**

```typescript
import { latestSession, readStats } from '../files.js';

export async function runStats(root: string, opts: { json?: boolean }): Promise<void> {
  const id = latestSession(root);
  if (!id) { process.stdout.write('no sessions found\n'); return; }
  const stats = readStats(root, id) as any;
  if (!stats) { process.stdout.write(`no stats.json for session ${id}\n`); return; }
  if (opts.json) { process.stdout.write(JSON.stringify(stats, null, 2) + '\n'); return; }
  process.stdout.write([
    `session:    ${id}`,
    `agents:     ${stats.agentsSeen ?? 0}`,
    `claims:     ${stats.claimsCreated ?? 0}`,
    `conflicts:  ${stats.conflictsOpened ?? 0}`,
    `peak heat:  ${stats.peakHeat?.heat ?? 0} (${(stats.peakHeat?.pair ?? []).join('↔')})`,
    `duration:   ${Math.round((stats.durationMs ?? 0) / 1000)}s`,
  ].join('\n') + '\n');
}
```

- [ ] **Step 4: Create `src/commands/session.ts`**

```typescript
import { listSessions, readStats, readSummary, readEntertainment } from '../files.js';

export async function runSessionList(root: string, opts: { json?: boolean }): Promise<void> {
  const sessions = listSessions(root);
  if (opts.json) { process.stdout.write(JSON.stringify(sessions, null, 2) + '\n'); return; }
  if (sessions.length === 0) { process.stdout.write('no sessions\n'); return; }
  process.stdout.write('SESSION ID               \n');
  for (const s of sessions) process.stdout.write(s + '\n');
}

export async function runSessionInspect(root: string, id: string, opts: { json?: boolean }): Promise<void> {
  const stats = readStats(root, id) as any;
  const summary = readSummary(root, id) as any;
  const ent = readEntertainment(root, id) as any;
  if (!stats) { process.stdout.write(`session not found: ${id}\n`); return; }
  if (opts.json) {
    process.stdout.write(JSON.stringify({ stats, summary, entertainment: ent }, null, 2) + '\n');
    return;
  }
  process.stdout.write(`=== Session ${id} ===\n`);
  process.stdout.write(`agents: ${stats.agentsSeen ?? 0}  claims: ${stats.claimsCreated ?? 0}  conflicts: ${stats.conflictsOpened ?? 0}\n`);
  process.stdout.write(`peak heat: ${stats.peakHeat?.heat ?? 0}  duration: ${Math.round((stats.durationMs ?? 0) / 1000)}s\n`);
  if (summary?.narrative) process.stdout.write(`\nNarrative: ${summary.narrative}\n`);
  if (ent?.badges?.length) {
    process.stdout.write('\nBadges:\n');
    for (const b of ent.badges) process.stdout.write(`  ${b.label}: ${b.agent}\n`);
  }
}
```

- [ ] **Step 5: Run tests and verify they pass**

```bash
cd packages/coordify-cli && npx tsx --test test/commands/session.test.ts 2>&1 | tail -5
```

Expected: `# tests 3`, `# pass 3`, `# fail 0`.

- [ ] **Step 6: Commit**

```bash
git add packages/coordify-cli/src/commands/stats.ts packages/coordify-cli/src/commands/session.ts packages/coordify-cli/test/commands/session.test.ts
git commit -m "feat(cli): session list/inspect + stats commands"
```

---

### Task 6: CLI entry point

Parses `process.argv`, dispatches to command functions. Binary shebang. Tests via subprocess.

**Files:**
- Create: `packages/coordify-cli/src/cli.ts`
- Create: `packages/coordify-cli/test/cli.test.ts`

**Interfaces:**
- Consumes: all `run*` functions from commands/
- Produces: binary at `dist/cli.js` (after `tsc`) or `src/cli.ts` (via `tsx`)

- [ ] **Step 1: Write the failing test**

Create `packages/coordify-cli/test/cli.test.ts`:

```typescript
import test from 'node:test';
import assert from 'node:assert';
import { execSync } from 'child_process';
import path from 'path';

const cli = path.resolve('src/cli.ts');
const run = (args: string) => {
  try {
    return execSync(`npx tsx ${cli} ${args}`, { encoding: 'utf8', env: { ...process.env, COORDIFY_ROOT: '/tmp/nonexistent-root-xyz' } });
  } catch (e: any) { return e.stdout ?? ''; }
};

test('unknown command prints usage', () => {
  const out = run('badcommand');
  assert.ok(out.includes('usage') || out.includes('Usage') || out.includes('coordify'));
});

test('status offline: no sessions found', () => {
  const out = run('status');
  assert.ok(out.includes('offline') || out.includes('no') || out.includes('session'));
});

test('--help prints command list', () => {
  const out = run('--help');
  assert.ok(out.includes('watch') || out.includes('status') || out.includes('coordify'));
});
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd packages/coordify-cli && npx tsx --test test/cli.test.ts 2>&1 | tail -5
```

Expected: FAIL — cli.ts not found.

- [ ] **Step 3: Create `src/cli.ts`**

```typescript
#!/usr/bin/env node
import path from 'path';
import { runStatus } from './commands/status.js';
import { runAgents } from './commands/agents.js';
import { runHeat } from './commands/heat.js';
import { runClaims } from './commands/claims.js';
import { runConflicts } from './commands/conflicts.js';
import { runLogs } from './commands/logs.js';
import { runStats } from './commands/stats.js';
import { runSessionList, runSessionInspect } from './commands/session.js';

const HELP = `coordify <command> [options]

Commands:
  status                  Live or offline overview
  agents                  List agents and their state
  heat                    Heat edges between agent pairs
  claims                  Active claims (live only)
  conflicts               Active conflicts (live only)
  logs [--tail N] [--follow]  Print event log
  stats                   Last session statistics
  session list            List finalized sessions
  session inspect <id>    Inspect a session
  watch                   Live terminal dashboard
  graph --coupling|--heat Graph view

Options:
  --json    Output raw JSON
  --root    Project root (default: cwd)
`;

const argv = process.argv.slice(2);
const root = (() => {
  const i = argv.indexOf('--root');
  if (i >= 0 && argv[i + 1]) return path.resolve(argv[i + 1]);
  return process.env.COORDIFY_ROOT ? path.resolve(process.env.COORDIFY_ROOT) : process.cwd();
})();
const json = argv.includes('--json');
const cmd = argv.find(a => !a.startsWith('-'));
const rest = argv.filter(a => !a.startsWith('-') && a !== cmd);

async function main() {
  switch (cmd) {
    case 'status': await runStatus(root, { json }); break;
    case 'agents': await runAgents(root, { json }); break;
    case 'heat':   await runHeat(root, { json }); break;
    case 'claims': await runClaims(root, { json }); break;
    case 'conflicts': await runConflicts(root, { json }); break;
    case 'logs': {
      const tail = Number(argv[argv.indexOf('--tail') + 1] ?? 20);
      await runLogs(root, { json, tail, follow: argv.includes('--follow') });
      break;
    }
    case 'stats':   await runStats(root, { json }); break;
    case 'session': {
      if (rest[0] === 'list' || argv.includes('list')) await runSessionList(root, { json });
      else if (rest[0] === 'inspect' || argv.includes('inspect')) {
        const id = rest[1] ?? argv[argv.indexOf('inspect') + 1];
        if (!id) { process.stdout.write('usage: coordify session inspect <id>\n'); process.exit(1); }
        await runSessionInspect(root, id, { json });
      } else { process.stdout.write(HELP); }
      break;
    }
    case 'watch': {
      const { renderWatch } = await import('./tui/watch.js');
      await renderWatch(root);
      break;
    }
    case 'graph': {
      const { renderGraph } = await import('./tui/graph.js');
      const mode = argv.includes('--heat') ? 'heat' : 'coupling';
      const top = Number(argv[argv.indexOf('--top') + 1] ?? 20);
      await renderGraph(root, mode, top);
      break;
    }
    case '--help':
    case undefined: process.stdout.write(HELP); break;
    default: process.stdout.write(`unknown command: ${cmd}\n\n${HELP}`);
  }
}

main().catch(e => { process.stderr.write(String(e) + '\n'); process.exit(1); });
```

- [ ] **Step 4: Run tests and verify they pass**

```bash
cd packages/coordify-cli && npx tsx --test test/cli.test.ts 2>&1 | tail -5
```

Expected: `# tests 3`, `# pass 3`, `# fail 0`.

- [ ] **Step 5: Commit**

```bash
git add packages/coordify-cli/src/cli.ts packages/coordify-cli/test/cli.test.ts
git commit -m "feat(cli): entry point — argv parsing, command dispatch, --help"
```

---

### Task 7: `coordify watch` TUI

Four-panel ink app: Agents, Heat, Conflicts, Session. Polls `get_state` every 500ms.

**Files:**
- Create: `packages/coordify-cli/src/tui/watch.tsx`
- Create: `packages/coordify-cli/src/tui/components/AgentPanel.tsx`
- Create: `packages/coordify-cli/src/tui/components/HeatPanel.tsx`
- Create: `packages/coordify-cli/src/tui/components/ConflictPanel.tsx`
- Create: `packages/coordify-cli/src/tui/components/SessionPanel.tsx`
- Create: `packages/coordify-cli/test/tui/watch.test.tsx`

**Interfaces:**
- Consumes: `ipc.query(root, 'get_state')`, `ipc.isLive(root)`, `files.readStats(root, id)`, `files.latestSession(root)`
- Produces: `renderWatch(root: string): Promise<void>` — mounts ink app, exits on `q`/Ctrl-C

- [ ] **Step 1: Write the failing test**

Create `packages/coordify-cli/test/tui/watch.test.tsx`:

```typescript
import test from 'node:test';
import assert from 'node:assert';
import React from 'react';
import { render } from 'ink-testing-library';
import AgentPanel from '../../src/tui/components/AgentPanel.js';
import HeatPanel from '../../src/tui/components/HeatPanel.js';
import ConflictPanel from '../../src/tui/components/ConflictPanel.js';

test('AgentPanel renders agents table', () => {
  const agents = [
    { agentId: 'agent-1', state: 'ACTIVE', claimId: 'claim-1' },
    { agentId: 'agent-2', state: 'IDLE', claimId: null },
  ];
  const { lastFrame } = render(React.createElement(AgentPanel, { agents }));
  assert.ok(lastFrame()?.includes('agent-1'));
  assert.ok(lastFrame()?.includes('ACTIVE'));
  assert.ok(lastFrame()?.includes('agent-2'));
});

test('AgentPanel renders empty state', () => {
  const { lastFrame } = render(React.createElement(AgentPanel, { agents: [] }));
  assert.ok(lastFrame()?.includes('no agents') || lastFrame()?.includes('Agents'));
});

test('HeatPanel renders heat edges', () => {
  const heat = [{ pair: ['a', 'b'], heat: 82, band: 'CONFLICT_CANDIDATE' }];
  const { lastFrame } = render(React.createElement(HeatPanel, { heat }));
  assert.ok(lastFrame()?.includes('82') || lastFrame()?.includes('a'));
});

test('ConflictPanel renders conflict list', () => {
  const conflicts = [{ conflictId: 'c-1', agents: ['a', 'b'], paths: ['x.rs'], state: 'NEGOTIATING', ageMs: 5000 }];
  const { lastFrame } = render(React.createElement(ConflictPanel, { conflicts }));
  assert.ok(lastFrame()?.includes('c-1') || lastFrame()?.includes('NEGOTIATING'));
});
```

- [ ] **Step 2: Install ink-testing-library and run to verify failure**

```bash
cd packages/coordify-cli && npm install --save-dev ink-testing-library
npx tsx --test test/tui/watch.test.tsx 2>&1 | tail -5
```

Expected: FAIL — components not found.

- [ ] **Step 3: Create `src/tui/components/AgentPanel.tsx`**

```tsx
import React from 'react';
import { Box, Text } from 'ink';

interface Agent { agentId: string; state: string; claimId?: string | null; }
interface Props { agents: Agent[]; }

export default function AgentPanel({ agents }: Props) {
  return (
    <Box flexDirection="column" borderStyle="single" borderColor="gray" paddingX={1}>
      <Text bold>Agents</Text>
      {agents.length === 0
        ? <Text color="gray">no agents</Text>
        : agents.map(a => (
            <Box key={a.agentId}>
              <Text color={a.state === 'ACTIVE' ? 'green' : 'gray'} wrap="truncate-end">{String(a.agentId).padEnd(16)}</Text>
              <Text color={a.state === 'ACTIVE' ? 'green' : 'gray'}>{String(a.state).padEnd(10)}</Text>
              <Text color="gray">{a.claimId ?? '-'}</Text>
            </Box>
          ))
      }
    </Box>
  );
}
```

- [ ] **Step 4: Create `src/tui/components/HeatPanel.tsx`**

```tsx
import React from 'react';
import { Box, Text } from 'ink';

interface HeatEdge { pair: string[]; heat: number; band: string; }
interface Props { heat: HeatEdge[]; }

function bandColor(band: string): string {
  if (band.includes('CONFLICT')) return 'red';
  if (band.includes('OVERLAP')) return 'yellow';
  if (band.includes('MONITOR')) return 'cyan';
  return 'gray';
}

export default function HeatPanel({ heat }: Props) {
  return (
    <Box flexDirection="column" borderStyle="single" borderColor="gray" paddingX={1}>
      <Text bold>Heat</Text>
      {heat.length === 0
        ? <Text color="gray">no heat</Text>
        : heat.slice(0, 8).map((e, i) => (
            <Box key={i}>
              <Text color={bandColor(e.band)} wrap="truncate-end">{(e.pair ?? []).join('↔').padEnd(30)}</Text>
              <Text color={bandColor(e.band)}>{String(e.heat).padEnd(5)}</Text>
              <Text color="gray">{e.band}</Text>
            </Box>
          ))
      }
    </Box>
  );
}
```

- [ ] **Step 5: Create `src/tui/components/ConflictPanel.tsx`**

```tsx
import React from 'react';
import { Box, Text } from 'ink';

interface Conflict { conflictId: string; agents: string[]; paths: string[]; state: string; ageMs?: number; }
interface Props { conflicts: Conflict[]; }

export default function ConflictPanel({ conflicts }: Props) {
  return (
    <Box flexDirection="column" borderStyle="single" borderColor="yellow" paddingX={1}>
      <Text bold color="yellow">Conflicts</Text>
      {conflicts.length === 0
        ? <Text color="gray">none</Text>
        : conflicts.map(c => (
            <Box key={c.conflictId} flexDirection="column">
              <Text color="yellow">{c.conflictId} <Text color="gray">({(c.agents ?? []).join(',')})</Text></Text>
              <Text color="gray">{(c.paths ?? []).slice(0, 2).join(', ')} — <Text color="yellow">{c.state}</Text>{c.ageMs ? ` ${Math.round(c.ageMs / 1000)}s` : ''}</Text>
            </Box>
          ))
      }
    </Box>
  );
}
```

- [ ] **Step 6: Create `src/tui/components/SessionPanel.tsx`**

```tsx
import React from 'react';
import { Box, Text } from 'ink';

interface Props { agents: number; claims: number; conflicts: number; peakHeat: number; }

export default function SessionPanel({ agents, claims, conflicts, peakHeat }: Props) {
  return (
    <Box borderStyle="single" borderColor="gray" paddingX={1} gap={3}>
      <Text bold>Session</Text>
      <Text>agents: <Text color="green">{agents}</Text></Text>
      <Text>claims: <Text color="cyan">{claims}</Text></Text>
      <Text>conflicts: <Text color={conflicts > 0 ? 'yellow' : 'gray'}>{conflicts}</Text></Text>
      <Text>peak heat: <Text color={peakHeat >= 80 ? 'red' : 'gray'}>{peakHeat}</Text></Text>
    </Box>
  );
}
```

- [ ] **Step 7: Create `src/tui/watch.tsx`**

```tsx
import React, { useState, useEffect } from 'react';
import { render, useApp, useInput } from 'ink';
import { Box, Text } from 'ink';
import AgentPanel from './components/AgentPanel.js';
import HeatPanel from './components/HeatPanel.js';
import ConflictPanel from './components/ConflictPanel.js';
import SessionPanel from './components/SessionPanel.js';
import { query, isLive } from '../ipc.js';
import { latestSession, readStats } from '../files.js';

interface State { agents: any[]; claims: any[]; heat: any[]; conflicts: any[]; error?: string; }

function WatchApp({ root }: { root: string }) {
  const { exit } = useApp();
  const [state, setState] = useState<State>({ agents: [], claims: [], heat: [], conflicts: [] });

  useInput((input, key) => {
    if (input === 'q' || key.ctrl && input === 'c') exit();
  });

  useEffect(() => {
    let alive = true;
    async function poll() {
      while (alive) {
        if (isLive(root)) {
          const resp = await query(root, 'get_state').catch(() => null);
          if (resp?.ok && alive) {
            const d = resp.data as any;
            setState({ agents: d.agents ?? [], claims: d.claims ?? [], heat: (d.heat ?? []).sort((a: any, b: any) => b.heat - a.heat), conflicts: d.conflicts ?? [] });
          }
        } else {
          const id = latestSession(root);
          const stats = id ? readStats(root, id) as any : null;
          if (alive) setState({ agents: [], claims: [], heat: [], conflicts: [], error: `offline${id ? ` (last: ${id})` : ''}` });
        }
        await new Promise(r => setTimeout(r, 500));
      }
    }
    poll();
    return () => { alive = false; };
  }, [root]);

  const peakHeat = Math.max(0, ...state.heat.map((h: any) => h.heat ?? 0));

  return (
    <Box flexDirection="column" width="100%">
      {state.error && <Text color="yellow">{state.error}</Text>}
      <Box gap={1}>
        <Box flexDirection="column" flexGrow={1}>
          <AgentPanel agents={state.agents} />
          <HeatPanel heat={state.heat} />
        </Box>
        <Box flexDirection="column" flexGrow={1}>
          <ConflictPanel conflicts={state.conflicts} />
          <SessionPanel agents={state.agents.length} claims={state.claims.length} conflicts={state.conflicts.length} peakHeat={peakHeat} />
        </Box>
      </Box>
      <Text color="gray">[q] quit</Text>
    </Box>
  );
}

export async function renderWatch(root: string): Promise<void> {
  const { waitUntilExit } = render(React.createElement(WatchApp, { root }));
  await waitUntilExit();
}
```

- [ ] **Step 8: Run tests and verify they pass**

```bash
cd packages/coordify-cli && npx tsx --test test/tui/watch.test.tsx 2>&1 | tail -8
```

Expected: `# tests 4`, `# pass 4`, `# fail 0`.

- [ ] **Step 9: Commit**

```bash
git add packages/coordify-cli/src/tui/watch.tsx packages/coordify-cli/src/tui/components/
git add packages/coordify-cli/test/tui/watch.test.tsx
git commit -m "feat(cli): coordify watch TUI — four-panel ink dashboard"
```

---

### Task 8: `coordify graph` TUI

Coupling graph and heat matrix views, switchable via `--coupling`/`--heat`. Reads knowledge files + `get_state`.

**Files:**
- Create: `packages/coordify-cli/src/tui/graph.tsx`
- Create: `packages/coordify-cli/src/tui/components/CouplingGraph.tsx`
- Create: `packages/coordify-cli/src/tui/components/HeatMatrix.tsx`
- Create: `packages/coordify-cli/test/tui/graph.test.tsx`

**Interfaces:**
- Consumes: `files.readKnowledge(root)`, `ipc.query(root, 'get_state')`
- Produces: `renderGraph(root: string, mode: 'coupling' | 'heat', top: number): Promise<void>`

- [ ] **Step 1: Write the failing tests**

Create `packages/coordify-cli/test/tui/graph.test.tsx`:

```typescript
import test from 'node:test';
import assert from 'node:assert';
import React from 'react';
import { render } from 'ink-testing-library';
import CouplingGraph from '../../src/tui/components/CouplingGraph.js';
import HeatMatrix from '../../src/tui/components/HeatMatrix.js';

test('CouplingGraph renders edge list sorted by count', () => {
  const edges = [
    { a: 'src/x.rs', b: 'src/y.rs', count: 5 },
    { a: 'src/a.rs', b: 'src/b.rs', count: 10 },
  ];
  const { lastFrame } = render(React.createElement(CouplingGraph, { edges, top: 20 }));
  const frame = lastFrame() ?? '';
  assert.ok(frame.includes('src/a.rs') || frame.includes('10'));
  // higher count appears first (or both appear)
  assert.ok(frame.includes('src/x.rs') || frame.includes('5'));
});

test('CouplingGraph renders empty state', () => {
  const { lastFrame } = render(React.createElement(CouplingGraph, { edges: [], top: 20 }));
  assert.ok(lastFrame()?.includes('no coupling') || lastFrame()?.includes('Coupling'));
});

test('HeatMatrix renders agent pair grid', () => {
  const heat = [
    { pair: ['agent-1', 'agent-2'], heat: 82, band: 'CONFLICT_CANDIDATE' },
    { pair: ['agent-1', 'agent-3'], heat: 30, band: 'SAFE' },
  ];
  const { lastFrame } = render(React.createElement(HeatMatrix, { heat }));
  const frame = lastFrame() ?? '';
  assert.ok(frame.includes('agent-1') || frame.includes('82'));
});
```

- [ ] **Step 2: Run to verify failure**

```bash
cd packages/coordify-cli && npx tsx --test test/tui/graph.test.tsx 2>&1 | tail -5
```

Expected: FAIL — components not found.

- [ ] **Step 3: Create `src/tui/components/CouplingGraph.tsx`**

```tsx
import React from 'react';
import { Box, Text } from 'ink';

interface Edge { a: string; b: string; count: number; }
interface Props { edges: Edge[]; top: number; }

export default function CouplingGraph({ edges, top }: Props) {
  const sorted = [...edges].sort((x, y) => y.count - x.count).slice(0, top);
  return (
    <Box flexDirection="column" borderStyle="single" borderColor="blue" paddingX={1}>
      <Text bold color="blue">Coupling Graph (top {top})</Text>
      {sorted.length === 0
        ? <Text color="gray">no coupling data</Text>
        : sorted.map((e, i) => (
            <Box key={i}>
              <Text color="cyan" wrap="truncate-end">{e.a}</Text>
              <Text color="gray"> ↔ </Text>
              <Text color="cyan" wrap="truncate-end">{e.b}</Text>
              <Text color="blue">  count: {e.count}</Text>
            </Box>
          ))
      }
    </Box>
  );
}
```

- [ ] **Step 4: Create `src/tui/components/HeatMatrix.tsx`**

```tsx
import React from 'react';
import { Box, Text } from 'ink';

interface HeatEdge { pair: string[]; heat: number; band: string; }
interface Props { heat: HeatEdge[]; }

function cell(heat: number, band: string): { label: string; color: string } {
  if (band.includes('CONFLICT')) return { label: String(heat).padStart(4), color: 'red' };
  if (band.includes('OVERLAP')) return { label: String(heat).padStart(4), color: 'yellow' };
  if (band.includes('MONITOR')) return { label: String(heat).padStart(4), color: 'cyan' };
  return { label: String(heat).padStart(4), color: 'gray' };
}

export default function HeatMatrix({ heat }: Props) {
  const agents = [...new Set(heat.flatMap(e => e.pair ?? []))].sort();
  const lookup = new Map(heat.map(e => [(e.pair ?? []).join('↔'), e]));
  const getEdge = (a: string, b: string) => lookup.get(`${a}↔${b}`) ?? lookup.get(`${b}↔${a}`);

  return (
    <Box flexDirection="column" borderStyle="single" borderColor="red" paddingX={1}>
      <Text bold color="red">Heat Matrix</Text>
      {agents.length === 0
        ? <Text color="gray">no heat data</Text>
        : (
          <>
            <Box>
              <Text color="gray">{''.padEnd(12)}</Text>
              {agents.map(a => <Text key={a} color="gray">{String(a).slice(0, 8).padStart(9)}</Text>)}
            </Box>
            {agents.map(row => (
              <Box key={row}>
                <Text color="gray">{String(row).slice(0, 10).padEnd(12)}</Text>
                {agents.map(col => {
                  if (row === col) return <Text key={col} color="gray">{'  --'.padStart(9)}</Text>;
                  const e = getEdge(row, col);
                  const { label, color } = e ? cell(e.heat, e.band) : { label: '   0', color: 'gray' };
                  return <Text key={col} color={color as any}>{label.padStart(9)}</Text>;
                })}
              </Box>
            ))}
          </>
        )
      }
    </Box>
  );
}
```

- [ ] **Step 5: Create `src/tui/graph.tsx`**

```tsx
import React, { useState, useEffect } from 'react';
import { render, useApp, useInput } from 'ink';
import { Box, Text } from 'ink';
import CouplingGraph from './components/CouplingGraph.js';
import HeatMatrix from './components/HeatMatrix.js';
import { readKnowledge } from '../files.js';
import { query, isLive } from '../ipc.js';

function GraphApp({ root, mode, top }: { root: string; mode: 'coupling' | 'heat'; top: number }) {
  const { exit } = useApp();
  const [edges, setEdges] = useState<any[]>([]);
  const [heat, setHeat] = useState<any[]>([]);

  useInput((input, key) => {
    if (input === 'q' || (key.ctrl && input === 'c')) exit();
  });

  useEffect(() => {
    let alive = true;
    async function refresh() {
      while (alive) {
        const k = readKnowledge(root);
        if (mode === 'coupling') setEdges((k.coupling as any[]) ?? []);
        if (isLive(root)) {
          const resp = await query(root, 'get_state').catch(() => null);
          if (resp?.ok && alive) setHeat((resp.data as any)?.heat ?? []);
        }
        await new Promise(r => setTimeout(r, 2000));
      }
    }
    refresh();
    return () => { alive = false; };
  }, [root, mode]);

  return (
    <Box flexDirection="column">
      {mode === 'coupling' && <CouplingGraph edges={edges} top={top} />}
      {mode === 'heat' && <HeatMatrix heat={heat} />}
      <Text color="gray">[q] quit</Text>
    </Box>
  );
}

export async function renderGraph(root: string, mode: 'coupling' | 'heat', top: number): Promise<void> {
  const { waitUntilExit } = render(React.createElement(GraphApp, { root, mode, top }));
  await waitUntilExit();
}
```

- [ ] **Step 6: Run all tests**

```bash
cd packages/coordify-cli && npx tsx --test 'test/**/*.test.ts' 'test/**/*.test.tsx' 2>&1 | tail -10
```

Expected: all pass.

- [ ] **Step 7: Build and verify binary**

```bash
cd packages/coordify-cli && npm run build 2>&1 | tail -5
node dist/cli.js --help
```

Expected: `tsc` succeeds with no errors; `--help` prints command list.

- [ ] **Step 8: Commit**

```bash
git add packages/coordify-cli/src/tui/graph.tsx packages/coordify-cli/src/tui/components/CouplingGraph.tsx packages/coordify-cli/src/tui/components/HeatMatrix.tsx packages/coordify-cli/test/tui/graph.test.tsx
git commit -m "feat(cli): coordify graph TUI — coupling edge list + heat agent matrix"
```
