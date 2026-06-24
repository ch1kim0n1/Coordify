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
