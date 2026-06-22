'use strict';

const logger = require('./logger');

const SENTINEL = 'phase-0/sentinel/BLOCK_TARGET';
const startedAt = Date.now();
let raw = '';

process.stdin.setEncoding('utf8');
process.stdin.on('data', chunk => { raw += chunk; });
process.stdin.on('end', () => {
  try {
    const payload = JSON.parse(raw);
    logger.capture('PreToolUse', payload);

    const inputPath = (payload.tool_input && payload.tool_input.path) || '';
    const isBlocked =
      inputPath === SENTINEL ||
      inputPath.endsWith('/' + SENTINEL) ||
      inputPath.endsWith(SENTINEL.replace(/\//g, require('path').sep));

    if (isBlocked) {
      process.stdout.write(JSON.stringify({
        decision: 'block',
        reason: 'Coordify Phase 0: sentinel path blocked for PreToolUse validation'
      }));
      logger.finish('PreToolUse', startedAt);
      process.exit(1);
    }
  } catch (_) {
    // never throw — pass through on parse error
  }

  logger.finish('PreToolUse', startedAt);
  process.exit(0);
});
