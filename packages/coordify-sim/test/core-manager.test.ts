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
