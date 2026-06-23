'use strict';
const test = require('node:test');
const assert = require('node:assert');
const { mapEvent, classifyIntent, isTestCommand } = require('../lib/mapping');

test('classifyIntent keyword rules and default', () => {
  assert.equal(classifyIntent('fix the login bug'), 'BUGFIX');
  assert.equal(classifyIntent('write tests for auth'), 'TESTING');
  assert.equal(classifyIntent('update the docs'), 'DOCUMENTATION');
  assert.equal(classifyIntent('refactor the parser'), 'REFACTOR');
  assert.equal(classifyIntent('optimize perf of query'), 'PERFORMANCE');
  assert.equal(classifyIntent('security review of tokens'), 'SECURITY');
  assert.equal(classifyIntent('add a new dashboard'), 'FEATURE');
  assert.equal(classifyIntent(''), 'FEATURE');
  assert.equal(classifyIntent(undefined), 'FEATURE');
});

test('isTestCommand', () => {
  assert.equal(isTestCommand('cargo test'), true);
  assert.equal(isTestCommand('npm test'), true);
  assert.equal(isTestCommand('pytest -k foo'), true);
  assert.equal(isTestCommand('ls -la'), false);
});

test('SessionStart clear vs startup', () => {
  assert.deepEqual(mapEvent('SessionStart', { source: 'clear' }), { kind: 'forward', event: { type: 'CLEAR_INVOKED' } });
  assert.deepEqual(mapEvent('SessionStart', { source: 'startup' }), { kind: 'bootstrap' });
  assert.deepEqual(mapEvent('SessionStart', { source: 'resume' }), { kind: 'bootstrap' });
});

test('UserPromptSubmit -> CLAIM_PROPOSED with heuristic claim', () => {
  const r = mapEvent('UserPromptSubmit', { prompt: 'fix the bug in session expiry' });
  assert.equal(r.kind, 'forward');
  assert.equal(r.event.type, 'CLAIM_PROPOSED');
  assert.equal(r.event.intent, 'BUGFIX');
  assert.deepEqual(r.event.domains, []);
  assert.deepEqual(r.event.estimatedFiles, []);
  assert.equal(r.event.confidence, 0.7);
  assert.equal(r.event.task.summary, 'fix the bug in session expiry');
});

test('UserPromptSubmit truncates summary to 200 chars', () => {
  const long = 'x'.repeat(500);
  const r = mapEvent('UserPromptSubmit', { prompt: long });
  assert.equal(r.event.task.summary.length, 200);
});

test('Subagent start/stop -> AGENT_STATE_CHANGED', () => {
  assert.deepEqual(mapEvent('SubagentStart', {}), { kind: 'forward', event: { type: 'AGENT_STATE_CHANGED', state: 'SUBAGENT_WAITING' } });
  assert.deepEqual(mapEvent('SubagentStop', {}), { kind: 'forward', event: { type: 'AGENT_STATE_CHANGED', state: 'ACTIVE' } });
});

test('SessionEnd -> release', () => {
  assert.deepEqual(mapEvent('SessionEnd', { reason: 'other' }), { kind: 'release' });
});

test('PreToolUse -> recorded TOOL_PRECHECK / RISKY_WRITE_CHECKED', () => {
  assert.equal(mapEvent('PreToolUse', { tool_name: 'Read', tool_input: { file_path: 'a' } }).record.type, 'TOOL_PRECHECK');
  assert.equal(mapEvent('PreToolUse', { tool_name: 'Edit', tool_input: { file_path: 'a' } }).record.type, 'RISKY_WRITE_CHECKED');
});

test('PostToolUse Edit/Write -> forwarded FILE_TOUCHED', () => {
  const w = mapEvent('PostToolUse', { tool_name: 'Write', tool_input: { file_path: 'src/x.rs' } });
  assert.equal(w.kind, 'forward');
  assert.equal(w.event.type, 'FILE_TOUCHED');
  assert.deepEqual(w.event.files, ['src/x.rs']);
  const e = mapEvent('PostToolUse', { tool_name: 'Edit', tool_input: { file_path: 'src/y.rs' } });
  assert.equal(e.kind, 'forward');
  assert.deepEqual(e.event.files, ['src/y.rs']);
  // Read + Bash stay recorded-only.
  assert.equal(mapEvent('PostToolUse', { tool_name: 'Read', tool_input: { file_path: 'r' } }).kind, 'record');
  assert.equal(mapEvent('PostToolUse', { tool_name: 'Bash', tool_input: { command: 'ls' } }).kind, 'record');
});

test('PostToolUse -> recorded file/read/command (Read/Bash only)', () => {
  assert.equal(mapEvent('PostToolUse', { tool_name: 'Read', tool_input: { file_path: 'src/x.rs' } }).record.type, 'FILE_READ');
  assert.equal(mapEvent('PostToolUse', { tool_name: 'Bash', tool_input: { command: 'cargo test' } }).record.type, 'TEST_RUN');
  assert.equal(mapEvent('PostToolUse', { tool_name: 'Bash', tool_input: { command: 'ls' } }).record.type, 'COMMAND_EXECUTED');
});

test('all recorded events carry kind:record', () => {
  for (const t of ['PreToolUse', 'PostToolUse']) {
    assert.equal(mapEvent(t, { tool_name: 'X' }).kind, 'record');
  }
});
