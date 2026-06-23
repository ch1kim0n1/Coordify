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
