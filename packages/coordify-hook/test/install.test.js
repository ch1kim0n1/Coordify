'use strict';
const test = require('node:test');
const assert = require('node:assert');
const os = require('os');
const fs = require('fs');
const path = require('path');
const { execFileSync } = require('child_process');

test('install.js writes 7 hooks and backs up existing settings', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'cc-install-'));
  fs.mkdirSync(path.join(dir, '.claude'), { recursive: true });
  fs.writeFileSync(path.join(dir, '.claude', 'settings.json'), JSON.stringify({ existing: true }));

  execFileSync(process.execPath, [path.resolve(__dirname, '..', 'install.js')], { cwd: dir });

  const settings = JSON.parse(fs.readFileSync(path.join(dir, '.claude', 'settings.json'), 'utf8'));
  assert.equal(settings.existing, true, 'preserves unrelated keys');
  const names = Object.keys(settings.hooks);
  for (const h of ['SessionStart', 'UserPromptSubmit', 'PreToolUse', 'PostToolUse', 'SubagentStart', 'SubagentStop', 'SessionEnd']) {
    assert.ok(names.includes(h), 'has hook ' + h);
  }
  assert.match(settings.hooks.SessionStart[0].hooks[0].command, /coordify-hook\/hooks\/session-start\.js/);
  assert.ok(fs.existsSync(path.join(dir, '.claude', 'settings.json.backup')), 'backed up');
});
