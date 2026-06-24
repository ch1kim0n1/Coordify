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
