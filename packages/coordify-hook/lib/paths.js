'use strict';

const os = require('os');
const path = require('path');
const crypto = require('crypto');

function coordify(root) { return path.join(root, '.coordify'); }
function runtime(root) { return path.join(coordify(root), 'runtime'); }
function agentsDir(root) { return path.join(runtime(root), 'agents'); }

function shortId(sessionId) {
  return crypto.createHash('sha1').update(String(sessionId)).digest('hex').slice(0, 12);
}

// Per-project hash keeps sockets isolated even when multiple projects use coordify.
function rootHash(root) {
  return crypto.createHash('sha1').update(root).digest('hex').slice(0, 8);
}

// Session sockets go to $TMPDIR to stay well under the 104-byte macOS
// Unix-socket path limit (deep project roots push in-tree paths over the cap).
function sessionSockDir(root) {
  return path.join(os.tmpdir(), 'coordify-' + rootHash(root));
}

module.exports = {
  coordify,
  runtime,
  agentsDir,
  shortId,
  socket: root => path.join(runtime(root), 'core.sock'),
  lock: root => path.join(runtime(root), 'core.lock'),
  token: root => path.join(runtime(root), 'session.token'),
  sessionSock: (root, sessionId) => path.join(sessionSockDir(root), shortId(sessionId) + '.sock'),
  hooktrace: (root, agentId) => path.join(agentsDir(root), 'agent-' + agentId + '.hooktrace.jsonl'),
  sidecarLog: (root, sessionId) => path.join(sessionSockDir(root), shortId(sessionId) + '.log'),
};
