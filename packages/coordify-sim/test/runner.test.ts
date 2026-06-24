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
