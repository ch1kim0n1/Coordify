'use strict';

const assert = require('assert');
const fs = require('fs');
const path = require('path');
const { spawnSync } = require('child_process');

const PAYLOADS_DIR = path.resolve(__dirname, '..', 'results', 'payloads');
const LATENCY_FILE = path.resolve(__dirname, '..', 'results', 'latency.jsonl');
const OUTPUT = path.resolve(__dirname, '..', 'results', 'hook-matrix.md');

// Seed fixture payloads
fs.mkdirSync(PAYLOADS_DIR, { recursive: true });

const fixtures = [
  { hookName: 'PreToolUse', capturedAt: new Date().toISOString(), payload: { tool_name: 'Read' } },
  { hookName: 'SessionStart', capturedAt: new Date().toISOString(), payload: { session_id: 'abc' } },
  { hookName: 'UserPromptSubmit', capturedAt: new Date().toISOString(), payload: { prompt: 'hello' } },
];

fixtures.forEach((f, i) => {
  fs.writeFileSync(path.join(PAYLOADS_DIR, `${f.hookName}-fixture-${i}.json`), JSON.stringify(f));
});

// Seed latency records
const latencyLines = Array.from({ length: 5 }, (_, i) =>
  JSON.stringify({ hookName: 'PreToolUse', startedAt: new Date().toISOString(), durationMs: 10 + i * 5 })
).join('\n') + '\n';
fs.writeFileSync(LATENCY_FILE, latencyLines);

// Run report
const result = spawnSync(process.execPath, ['phase-0/report.js'], { encoding: 'utf8' });
assert.strictEqual(result.status, 0, `report.js must exit 0, got: ${result.stderr}`);

// Verify output file
assert.ok(fs.existsSync(OUTPUT), 'hook-matrix.md must be written');
const md = fs.readFileSync(OUTPUT, 'utf8');

assert.ok(md.includes('H1'), 'matrix must include H1');
assert.ok(md.includes('H7'), 'matrix must include H7');
assert.ok(md.includes('PASS') || md.includes('PENDING') || md.includes('MANUAL'), 'matrix must include status values');
assert.ok(md.includes('PreToolUse'), 'matrix must reference PreToolUse');
assert.ok(md.includes('Generated:'), 'matrix must include generation timestamp');

console.log('test-report.js: all assertions passed');
