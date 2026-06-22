'use strict';

const assert = require('assert');
const { spawnSync } = require('child_process');
const path = require('path');

const SCRIPT = path.resolve(__dirname, '..', 'hooks', 'pre-tool-use.js');

function run(payload) {
  return spawnSync(process.execPath, [SCRIPT], {
    input: JSON.stringify(payload),
    encoding: 'utf8'
  });
}

// Test 1: non-sentinel path exits 0 (pass through)
const pass = run({ tool_name: 'Write', tool_input: { path: 'src/index.js', content: 'x' } });
assert.strictEqual(pass.status, 0, 'non-sentinel path must exit 0');

// Test 2: sentinel path exits 1 (blocked)
const block = run({ tool_name: 'Write', tool_input: { path: 'phase-0/sentinel/BLOCK_TARGET' } });
assert.strictEqual(block.status, 1, 'sentinel path must exit 1');

const blockResponse = JSON.parse(block.stdout);
assert.strictEqual(blockResponse.decision, 'block', 'block response must have decision: block');
assert.ok(blockResponse.reason, 'block response must have a reason');

// Test 3: malformed JSON exits 0 (never crashes)
const bad = run(null);  // spawnSync sends "null" string
assert.strictEqual(bad.status, 0, 'malformed payload must still exit 0 (no crash)');

// Test 4: no tool_input exits 0
const noInput = run({ tool_name: 'Read' });
assert.strictEqual(noInput.status, 0, 'missing tool_input must exit 0');

console.log('test-pre-tool-use.js: all assertions passed');
