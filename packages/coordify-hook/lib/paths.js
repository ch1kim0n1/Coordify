'use strict';

const path = require('path');
const crypto = require('crypto');

function coordify(root) { return path.join(root, '.coordify'); }
function runtime(root) { return path.join(coordify(root), 'runtime'); }
function agentsDir(root) { return path.join(runtime(root), 'agents'); }

// Short, stable id for filenames — keeps the per-session socket path well under
// the ~104-byte macOS Unix-socket limit even for long session UUIDs / deep roots.
function shortId(sessionId) {
  return crypto.createHash('sha1').update(String(sessionId)).digest('hex').slice(0, 12);
}

module.exports = {
  coordify,
  runtime,
  agentsDir,
  shortId,
  socket: root => path.join(runtime(root), 'core.sock'),
  lock: root => path.join(runtime(root), 'core.lock'),
  token: root => path.join(runtime(root), 'session.token'),
  sessionSock: (root, sessionId) => path.join(agentsDir(root), shortId(sessionId) + '.sock'),
  hooktrace: (root, agentId) => path.join(agentsDir(root), 'agent-' + agentId + '.hooktrace.jsonl'),
  sidecarLog: (root, sessionId) => path.join(agentsDir(root), shortId(sessionId) + '.log'),
};
