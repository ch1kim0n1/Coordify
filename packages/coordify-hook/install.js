'use strict';

const fs = require('fs');
const path = require('path');

const SETTINGS_PATH = path.resolve(process.cwd(), '.claude', 'settings.json');
const BACKUP_PATH = SETTINGS_PATH + '.backup';
// Absolute path so hooks work regardless of CWD at invocation time
const HOOKS_DIR = path.resolve(__dirname, 'hooks');
const NODE_BIN = process.execPath;

function hookEntry(file) {
  return {
    matcher: '',
    hooks: [{ type: 'command', command: JSON.stringify(NODE_BIN) + ' ' + JSON.stringify(path.join(HOOKS_DIR, file)) }],
  };
}

function run() {
  if (fs.existsSync(SETTINGS_PATH)) {
    fs.copyFileSync(SETTINGS_PATH, BACKUP_PATH);
    console.log('Backed up existing settings to ' + BACKUP_PATH);
  }

  let settings = {};
  if (fs.existsSync(SETTINGS_PATH)) {
    try { settings = JSON.parse(fs.readFileSync(SETTINGS_PATH, 'utf8')); }
    catch (_) { console.warn('Could not parse existing settings.json — starting fresh'); }
  }

  settings.hooks = {
    SessionStart:     [hookEntry('session-start.js')],
    UserPromptSubmit: [hookEntry('user-prompt-submit.js')],
    PreToolUse:       [hookEntry('pre-tool-use.js')],
    PostToolUse:      [hookEntry('post-tool-use.js')],
    SubagentStart:    [hookEntry('subagent-start.js')],
    SubagentStop:     [hookEntry('subagent-stop.js')],
    SessionEnd:       [hookEntry('session-end.js')],
  };

  fs.mkdirSync(path.dirname(SETTINGS_PATH), { recursive: true });
  fs.writeFileSync(SETTINGS_PATH, JSON.stringify(settings, null, 2));
  console.log('Coordify hook configuration written to ' + SETTINGS_PATH);
}

run();
