'use strict';

const fs = require('fs');
const path = require('path');

const SETTINGS_PATH = path.resolve(process.cwd(), '.claude', 'settings.json');
const BACKUP_PATH = SETTINGS_PATH + '.backup';
const HOOKS_DIR = 'phase-0/hooks';

function run() {
  // Guard: must be run from project root
  if (!fs.existsSync(path.join(process.cwd(), 'phase-0'))) {
    console.error('Error: run install.js from the project root (the directory containing phase-0/)');
    process.exit(1);
  }

  // Backup existing settings
  if (fs.existsSync(SETTINGS_PATH)) {
    fs.copyFileSync(SETTINGS_PATH, BACKUP_PATH);
    console.log(`Backed up existing settings to ${BACKUP_PATH}`);
  }

  // Read existing settings or start fresh
  let settings = {};
  if (fs.existsSync(SETTINGS_PATH)) {
    try {
      settings = JSON.parse(fs.readFileSync(SETTINGS_PATH, 'utf8'));
    } catch (_) {
      console.warn('Could not parse existing settings.json — starting fresh');
    }
  }

  settings.hooks = {
    SessionStart:     [{ command: `node ${HOOKS_DIR}/session-start.js` }],
    UserPromptSubmit: [{ command: `node ${HOOKS_DIR}/user-prompt-submit.js` }],
    PreToolUse:       [{ command: `node ${HOOKS_DIR}/pre-tool-use.js` }],
    PostToolUse:      [{ command: `node ${HOOKS_DIR}/post-tool-use.js` }],
    SubagentStart:    [{ command: `node ${HOOKS_DIR}/subagent-start.js` }],
    SubagentStop:     [{ command: `node ${HOOKS_DIR}/subagent-stop.js` }],
    SessionEnd:       [{ command: `node ${HOOKS_DIR}/session-end.js` }],
  };

  fs.mkdirSync(path.dirname(SETTINGS_PATH), { recursive: true });
  fs.writeFileSync(SETTINGS_PATH, JSON.stringify(settings, null, 2));
  console.log(`Hook configuration written to ${SETTINGS_PATH}`);
  console.log('Run `claude` in this project root to begin validation.');
}

run();
