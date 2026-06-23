'use strict';

const logger = require('./logger');

const startedAt = Date.now();
let raw = '';

process.stdin.setEncoding('utf8');
process.stdin.on('error', () => {
  logger.finish('UserPromptSubmit', startedAt);
  process.exit(0);
});
process.stdin.on('data', chunk => { raw += chunk; });
process.stdin.on('end', () => {
  try {
    const payload = JSON.parse(raw);
    logger.capture('UserPromptSubmit', payload);

    // Attempt context injection — Phase 0 validates whether this appears in Claude context
    process.stdout.write(JSON.stringify({
      context: '[Coordify Phase 0] Hook injection active. If you see this, H3 is PASS.'
    }));
  } catch (_) {
    // never throw
  }
  logger.finish('UserPromptSubmit', startedAt);
  process.exit(0);
});
