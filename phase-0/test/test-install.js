'use strict';

const assert = require('assert');
const fs = require('fs');
const path = require('path');
const { spawnSync } = require('child_process');

const SETTINGS_PATH = path.resolve(process.cwd(), '.claude', 'settings.json');
const BACKUP_PATH = SETTINGS_PATH + '.backup';

// Stash existing settings
const hadExisting = fs.existsSync(SETTINGS_PATH);
const originalContent = hadExisting ? fs.readFileSync(SETTINGS_PATH, 'utf8') : null;

// Run install
const result = spawnSync(process.execPath, ['phase-0/install.js'], { encoding: 'utf8' });
assert.strictEqual(result.status, 0, `install.js must exit 0, got: ${result.stderr}`);

// Verify settings written
assert.ok(fs.existsSync(SETTINGS_PATH), 'settings.json must exist after install');
const settings = JSON.parse(fs.readFileSync(SETTINGS_PATH, 'utf8'));

const expectedHooks = ['SessionStart', 'UserPromptSubmit', 'PreToolUse', 'PostToolUse', 'SubagentStart', 'SubagentStop', 'SessionEnd'];
for (const hook of expectedHooks) {
  assert.ok(settings.hooks[hook], `settings.hooks.${hook} must be present`);
  assert.ok(Array.isArray(settings.hooks[hook]), `settings.hooks.${hook} must be an array`);
  assert.ok(settings.hooks[hook][0].command.includes(`phase-0/hooks/`), `${hook} command must reference phase-0/hooks/`);
}

// Verify backup created when original existed
if (hadExisting) {
  assert.ok(fs.existsSync(BACKUP_PATH), 'backup must be created when original settings existed');
}

// Restore original
if (originalContent !== null) {
  fs.writeFileSync(SETTINGS_PATH, originalContent);
} else {
  fs.unlinkSync(SETTINGS_PATH);
}

console.log('test-install.js: all assertions passed');
