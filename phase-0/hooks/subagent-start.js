'use strict';

const logger = require('./logger');

const startedAt = Date.now();
let raw = '';

process.stdin.setEncoding('utf8');
process.stdin.on('data', chunk => { raw += chunk; });
process.stdin.on('end', () => {
  try {
    const payload = JSON.parse(raw);
    logger.capture('SubagentStart', payload);
  } catch (_) {
    // never throw
  }
  logger.finish('SubagentStart', startedAt);
  process.exit(0);
});
