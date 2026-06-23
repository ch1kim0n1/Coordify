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
