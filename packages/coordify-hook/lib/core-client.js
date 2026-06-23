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
