'use strict';

const logger = require('./logger');

const startedAt = Date.now();
let raw = '';

process.stdin.setEncoding('utf8');
process.stdin.on('error', () => {
  logger.finish('SessionStart', startedAt);
  process.exit(0);
});
process.stdin.on('data', chunk => { raw += chunk; });
process.stdin.on('end', () => {
  try {
    const payload = JSON.parse(raw);
    logger.capture('SessionStart', payload);
  } catch (_) {
    // never throw
  }
  logger.finish('SessionStart', startedAt);
  process.exit(0);
});
