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
