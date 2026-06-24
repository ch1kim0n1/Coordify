'use strict';
const test = require('node:test');
const assert = require('node:assert');
const os = require('os');
const fs = require('fs');
const path = require('path');
const { execFileSync, spawnSync } = require('child_process');

const INSTALL = path.resolve(__dirname, '..', 'install.js');

// The install harness has no real coordify-core on PATH; the production check
// is exercised by the dedicated fail-loud test below.
const SKIP_CORE_CHECK = { ...process.env, COORDIFY_SKIP_CORE_CHECK: '1' };

test('install.js writes 7 hooks and backs up existing settings', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'cc-install-'));
  fs.mkdirSync(path.join(dir, '.claude'), { recursive: true });
  fs.writeFileSync(path.join(dir, '.claude', 'settings.json'), JSON.stringify({ existing: true }));

  execFileSync(process.execPath, [INSTALL], { cwd: dir, env: SKIP_CORE_CHECK });

  const settings = JSON.parse(fs.readFileSync(path.join(dir, '.claude', 'settings.json'), 'utf8'));
  assert.equal(settings.existing, true, 'preserves unrelated keys');
  const names = Object.keys(settings.hooks);
  for (const h of ['SessionStart', 'UserPromptSubmit', 'PreToolUse', 'PostToolUse', 'SubagentStart', 'SubagentStop', 'SessionEnd']) {
    assert.ok(names.includes(h), 'has hook ' + h);
  }
  assert.match(settings.hooks.SessionStart[0].hooks[0].command, /coordify-hook\/hooks\/session-start\.js/);
  assert.ok(fs.existsSync(path.join(dir, '.claude', 'settings.json.backup')), 'backed up');
});

test('install.js is idempotent — re-running does not duplicate or drift', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'cc-install-idem-'));
  fs.mkdirSync(path.join(dir, '.claude'), { recursive: true });
  execFileSync(process.execPath, [INSTALL], { cwd: dir, env: SKIP_CORE_CHECK });
  const first = fs.readFileSync(path.join(dir, '.claude', 'settings.json'), 'utf8');
  execFileSync(process.execPath, [INSTALL], { cwd: dir, env: SKIP_CORE_CHECK });
  const second = fs.readFileSync(path.join(dir, '.claude', 'settings.json'), 'utf8');
  assert.equal(second, first, 'second run produces identical bytes');
  const settings = JSON.parse(second);
  assert.equal(settings.hooks.SessionStart.length, 1, 'no duplicate entries');
});

test('install.js preserves foreign hook entries under shared event keys', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'cc-install-merge-'));
  fs.mkdirSync(path.join(dir, '.claude'), { recursive: true });
  const foreign = { hooks: { SessionStart: [{ matcher: '', hooks: [{ type: 'command', command: 'echo foreign' }] }] } };
  fs.writeFileSync(path.join(dir, '.claude', 'settings.json'), JSON.stringify(foreign));
  execFileSync(process.execPath, [INSTALL], { cwd: dir, env: SKIP_CORE_CHECK });
  const settings = JSON.parse(fs.readFileSync(path.join(dir, '.claude', 'settings.json'), 'utf8'));
  const cmds = settings.hooks.SessionStart.map(e => e.hooks[0].command);
  assert.ok(cmds.some(c => c.includes('echo foreign')), 'foreign entry preserved');
  assert.ok(cmds.some(c => c.includes('coordify-hook/hooks/session-start.js')), 'coordify entry present');
});

test('install.js fails loudly when coordify-core is not on PATH', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'cc-install-nocore-'));
  fs.mkdirSync(path.join(dir, '.claude'), { recursive: true });
  // Empty PATH guarantees `command -v coordify-core` finds nothing.
  const res = spawnSync(process.execPath, [INSTALL], { cwd: dir, env: { PATH: '' } });
  assert.notEqual(res.status, 0, 'non-zero exit');
  assert.match(res.stderr.toString(), /coordify-core is not on PATH/);
  assert.ok(!fs.existsSync(path.join(dir, '.claude', 'settings.json')), 'no settings written on failure');
});
