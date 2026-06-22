# Phase 0 Technical Validation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build Node.js hook scripts that install into `.claude/settings.json` and capture real Claude Code hook payloads to validate 7 integration assumptions.

**Architecture:** Shared `logger.js` writes raw payloads and latency records to `phase-0/results/`. Seven thin hook scripts each read stdin, call logger, and exit. `install.js` wires all hooks into `.claude/settings.json`. `report.js` reads results and generates `hook-matrix.md`.

**Tech Stack:** Node.js (built-ins only — `fs`, `path`, `child_process`, `assert`). No npm deps.

## Global Constraints

- No `require()` of any external package — only Node.js built-ins
- Every hook script must catch all errors and never throw unhandled exceptions
- `PreToolUse` blocks ONLY when path matches `phase-0/sentinel/BLOCK_TARGET`
- All scripts run from the project root (not from `phase-0/`)
- Test files use only `assert` from Node stdlib — no test framework
- `phase-0/results/payloads/` is gitignored (may contain prompt content)

---

## File Map

| File | Create/Modify | Purpose |
|------|--------------|---------|
| `phase-0/hooks/logger.js` | Create | Shared payload capture + latency recording |
| `phase-0/hooks/pre-tool-use.js` | Create | H1, H2, H7: intercept + sentinel block + timing |
| `phase-0/hooks/session-start.js` | Create | H4: /clear vs startup classification |
| `phase-0/hooks/user-prompt-submit.js` | Create | H3: context injection attempt |
| `phase-0/hooks/post-tool-use.js` | Create | Supporting: log PostToolUse payloads |
| `phase-0/hooks/subagent-start.js` | Create | H5: subagent start capture |
| `phase-0/hooks/subagent-stop.js` | Create | H5: subagent stop capture |
| `phase-0/hooks/session-end.js` | Create | H6: clean exit capture |
| `phase-0/install.js` | Create | Write hooks to .claude/settings.json |
| `phase-0/report.js` | Create | Generate hook-matrix.md from results |
| `phase-0/sentinel/BLOCK_TARGET` | Create | Sentinel file PreToolUse blocks on |
| `phase-0/test/test-logger.js` | Create | Assert-based tests for logger.js |
| `phase-0/test/test-pre-tool-use.js` | Create | Assert-based tests for pre-tool-use.js |
| `phase-0/test/test-install.js` | Create | Assert-based tests for install.js |
| `phase-0/test/test-report.js` | Create | Assert-based tests for report.js |
| `.gitignore` | Modify | Ignore phase-0/results/payloads/ |

---

### Task 1: Scaffold + logger.js

**Files:**
- Create: `phase-0/hooks/logger.js`
- Create: `phase-0/sentinel/BLOCK_TARGET`
- Create: `phase-0/results/.gitkeep`
- Create: `phase-0/results/payloads/.gitkeep`
- Create: `phase-0/test/test-logger.js`
- Modify: `.gitignore`

**Interfaces:**
- Produces:
  - `logger.capture(hookName: string, payload: object): void` — writes `results/payloads/<hookName>-<ts>.json`
  - `logger.finish(hookName: string, startedAt: number): void` — appends to `results/latency.jsonl`

- [ ] **Step 1: Create folder structure**

```bash
mkdir -p phase-0/hooks phase-0/results/payloads phase-0/sentinel phase-0/test
touch phase-0/results/.gitkeep phase-0/results/payloads/.gitkeep
echo "Coordify Phase 0 sentinel file. PreToolUse blocks writes to this path." > phase-0/sentinel/BLOCK_TARGET
```

- [ ] **Step 2: Add gitignore entry**

Append to `.gitignore` (create if absent):

```
# Phase 0 — raw hook payloads may contain prompt content
phase-0/results/payloads/
phase-0/results/latency.jsonl
```

- [ ] **Step 3: Write logger.js**

Create `phase-0/hooks/logger.js`:

```js
'use strict';

const fs = require('fs');
const path = require('path');

const RESULTS_DIR = path.resolve(__dirname, '..', 'results');
const PAYLOADS_DIR = path.join(RESULTS_DIR, 'payloads');
const LATENCY_FILE = path.join(RESULTS_DIR, 'latency.jsonl');

function ensureDirs() {
  fs.mkdirSync(PAYLOADS_DIR, { recursive: true });
}

function capture(hookName, payload) {
  try {
    ensureDirs();
    const ts = new Date().toISOString().replace(/[:.]/g, '-');
    const file = path.join(PAYLOADS_DIR, `${hookName}-${ts}.json`);
    const record = { hookName, capturedAt: new Date().toISOString(), payload };
    fs.writeFileSync(file, JSON.stringify(record, null, 2));
  } catch (_) {
    // never throw from hook context
  }
}

function finish(hookName, startedAt) {
  try {
    const durationMs = Date.now() - startedAt;
    const line = JSON.stringify({ hookName, startedAt: new Date(startedAt).toISOString(), durationMs }) + '\n';
    fs.appendFileSync(LATENCY_FILE, line);
  } catch (_) {
    // never throw from hook context
  }
}

module.exports = { capture, finish };
```

- [ ] **Step 4: Write test-logger.js**

Create `phase-0/test/test-logger.js`:

```js
'use strict';

const assert = require('assert');
const fs = require('fs');
const path = require('path');

// Clean slate for test
const PAYLOADS_DIR = path.resolve(__dirname, '..', 'results', 'payloads');
const LATENCY_FILE = path.resolve(__dirname, '..', 'results', 'latency.jsonl');

// Remove old latency file for clean test
if (fs.existsSync(LATENCY_FILE)) fs.unlinkSync(LATENCY_FILE);

const logger = require('../hooks/logger');
const startedAt = Date.now() - 42; // simulate 42ms elapsed

// Test 1: capture writes a payload file
logger.capture('TestHook', { foo: 'bar', nested: { x: 1 } });

const files = fs.readdirSync(PAYLOADS_DIR).filter(f => f.startsWith('TestHook-'));
assert.ok(files.length > 0, 'capture() must write at least one payload file');

const written = JSON.parse(fs.readFileSync(path.join(PAYLOADS_DIR, files[files.length - 1]), 'utf8'));
assert.strictEqual(written.hookName, 'TestHook', 'hookName must match');
assert.strictEqual(written.payload.foo, 'bar', 'payload must be preserved');
assert.strictEqual(written.payload.nested.x, 1, 'nested payload must be preserved');
assert.ok(written.capturedAt, 'capturedAt must be set');

// Test 2: finish writes a latency record
logger.finish('TestHook', startedAt);

const lines = fs.readFileSync(LATENCY_FILE, 'utf8').trim().split('\n').filter(Boolean);
assert.ok(lines.length > 0, 'finish() must write at least one latency record');

const latency = JSON.parse(lines[lines.length - 1]);
assert.strictEqual(latency.hookName, 'TestHook', 'latency hookName must match');
assert.ok(latency.durationMs >= 42, 'durationMs must be at least the simulated elapsed time');

// Test 3: capture does not throw on bad payload
assert.doesNotThrow(() => logger.capture('BadHook', undefined));
assert.doesNotThrow(() => logger.capture('BadHook', null));

console.log('test-logger.js: all assertions passed');
```

- [ ] **Step 5: Run test**

```bash
node phase-0/test/test-logger.js
```

Expected output: `test-logger.js: all assertions passed`

- [ ] **Step 6: Commit**

```bash
git add phase-0/ .gitignore
git commit -m "feat(phase-0): scaffold + shared logger module"
```

---

### Task 2: pre-tool-use.js

**Files:**
- Create: `phase-0/hooks/pre-tool-use.js`
- Create: `phase-0/test/test-pre-tool-use.js`

**Interfaces:**
- Consumes: `logger.capture`, `logger.finish` from Task 1
- Produces: exit code 0 (allow) or 1 (block) + JSON on stdout when blocking

- [ ] **Step 1: Write pre-tool-use.js**

Create `phase-0/hooks/pre-tool-use.js`:

```js
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
```

- [ ] **Step 2: Write test-pre-tool-use.js**

Create `phase-0/test/test-pre-tool-use.js`:

```js
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
```

- [ ] **Step 3: Run test**

```bash
node phase-0/test/test-pre-tool-use.js
```

Expected output: `test-pre-tool-use.js: all assertions passed`

- [ ] **Step 4: Commit**

```bash
git add phase-0/hooks/pre-tool-use.js phase-0/test/test-pre-tool-use.js
git commit -m "feat(phase-0): pre-tool-use hook — sentinel blocking + latency capture"
```

---

### Task 3: session-start.js, user-prompt-submit.js, post-tool-use.js

**Files:**
- Create: `phase-0/hooks/session-start.js`
- Create: `phase-0/hooks/user-prompt-submit.js`
- Create: `phase-0/hooks/post-tool-use.js`

**Interfaces:**
- Consumes: `logger.capture`, `logger.finish` from Task 1
- `user-prompt-submit.js` writes JSON to stdout: `{ "context": "..." }` to attempt injection

- [ ] **Step 1: Write session-start.js**

Create `phase-0/hooks/session-start.js`:

```js
'use strict';

const logger = require('./logger');

const startedAt = Date.now();
let raw = '';

process.stdin.setEncoding('utf8');
process.stdin.on('data', chunk => { raw += chunk; });
process.stdin.on('end', () => {
  try {
    const payload = JSON.parse(raw);
    logger.capture('SessionStart', payload);
  } catch (_) {
    // never throw
  }
  logger.finish('SessionStart', startedAt);
  process.exit(0);
});
```

- [ ] **Step 2: Write user-prompt-submit.js**

Create `phase-0/hooks/user-prompt-submit.js`:

```js
'use strict';

const logger = require('./logger');

const startedAt = Date.now();
let raw = '';

process.stdin.setEncoding('utf8');
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
```

- [ ] **Step 3: Write post-tool-use.js**

Create `phase-0/hooks/post-tool-use.js`:

```js
'use strict';

const logger = require('./logger');

const startedAt = Date.now();
let raw = '';

process.stdin.setEncoding('utf8');
process.stdin.on('data', chunk => { raw += chunk; });
process.stdin.on('end', () => {
  try {
    const payload = JSON.parse(raw);
    logger.capture('PostToolUse', payload);
  } catch (_) {
    // never throw
  }
  logger.finish('PostToolUse', startedAt);
  process.exit(0);
});
```

- [ ] **Step 4: Smoke-test all three scripts with piped input**

```bash
echo '{"hook_event_name":"SessionStart","session_id":"test"}' | node phase-0/hooks/session-start.js
echo $?
```

Expected: `0`

```bash
echo '{"hook_event_name":"UserPromptSubmit","prompt":"hello"}' | node phase-0/hooks/user-prompt-submit.js
echo $?
```

Expected: `0` and JSON on stdout: `{"context":"[Coordify Phase 0] Hook injection active..."}`

```bash
echo '{"hook_event_name":"PostToolUse","tool_name":"Read"}' | node phase-0/hooks/post-tool-use.js
echo $?
```

Expected: `0`

- [ ] **Step 5: Commit**

```bash
git add phase-0/hooks/session-start.js phase-0/hooks/user-prompt-submit.js phase-0/hooks/post-tool-use.js
git commit -m "feat(phase-0): session-start, user-prompt-submit, post-tool-use hooks"
```

---

### Task 4: subagent-start.js, subagent-stop.js, session-end.js

**Files:**
- Create: `phase-0/hooks/subagent-start.js`
- Create: `phase-0/hooks/subagent-stop.js`
- Create: `phase-0/hooks/session-end.js`

**Interfaces:**
- Consumes: `logger.capture`, `logger.finish` from Task 1
- Produces: captured payloads under `results/payloads/SubagentStart-*.json`, `SubagentStop-*.json`, `SessionEnd-*.json`

- [ ] **Step 1: Write subagent-start.js**

Create `phase-0/hooks/subagent-start.js`:

```js
'use strict';

const logger = require('./logger');

const startedAt = Date.now();
let raw = '';

process.stdin.setEncoding('utf8');
process.stdin.on('data', chunk => { raw += chunk; });
process.stdin.on('end', () => {
  try {
    const payload = JSON.parse(raw);
    logger.capture('SubagentStart', payload);
  } catch (_) {
    // never throw
  }
  logger.finish('SubagentStart', startedAt);
  process.exit(0);
});
```

- [ ] **Step 2: Write subagent-stop.js**

Create `phase-0/hooks/subagent-stop.js`:

```js
'use strict';

const logger = require('./logger');

const startedAt = Date.now();
let raw = '';

process.stdin.setEncoding('utf8');
process.stdin.on('data', chunk => { raw += chunk; });
process.stdin.on('end', () => {
  try {
    const payload = JSON.parse(raw);
    logger.capture('SubagentStop', payload);
  } catch (_) {
    // never throw
  }
  logger.finish('SubagentStop', startedAt);
  process.exit(0);
});
```

- [ ] **Step 3: Write session-end.js**

Create `phase-0/hooks/session-end.js`:

```js
'use strict';

const logger = require('./logger');

const startedAt = Date.now();
let raw = '';

process.stdin.setEncoding('utf8');
process.stdin.on('data', chunk => { raw += chunk; });
process.stdin.on('end', () => {
  try {
    const payload = JSON.parse(raw);
    logger.capture('SessionEnd', payload);
  } catch (_) {
    // never throw
  }
  logger.finish('SessionEnd', startedAt);
  process.exit(0);
});
```

- [ ] **Step 4: Smoke-test all three**

```bash
echo '{"hook_event_name":"SubagentStart","subagent_id":"test"}' | node phase-0/hooks/subagent-start.js && echo "subagent-start: OK"
echo '{"hook_event_name":"SubagentStop","subagent_id":"test"}' | node phase-0/hooks/subagent-stop.js && echo "subagent-stop: OK"
echo '{"hook_event_name":"SessionEnd","session_id":"test"}' | node phase-0/hooks/session-end.js && echo "session-end: OK"
```

Expected output:
```
subagent-start: OK
subagent-stop: OK
session-end: OK
```

- [ ] **Step 5: Commit**

```bash
git add phase-0/hooks/subagent-start.js phase-0/hooks/subagent-stop.js phase-0/hooks/session-end.js
git commit -m "feat(phase-0): subagent-start, subagent-stop, session-end hooks"
```

---

### Task 5: install.js

**Files:**
- Create: `phase-0/install.js`
- Create: `phase-0/test/test-install.js`

**Interfaces:**
- Produces: writes `{ hooks: { ... } }` into `.claude/settings.json`, backs up existing file to `.claude/settings.json.backup`

- [ ] **Step 1: Write install.js**

Create `phase-0/install.js`:

```js
'use strict';

const fs = require('fs');
const path = require('path');

const SETTINGS_PATH = path.resolve(process.cwd(), '.claude', 'settings.json');
const BACKUP_PATH = SETTINGS_PATH + '.backup';
const HOOKS_DIR = 'phase-0/hooks';

function run() {
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
```

- [ ] **Step 2: Write test-install.js**

Create `phase-0/test/test-install.js`:

```js
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
```

- [ ] **Step 3: Run test**

```bash
node phase-0/test/test-install.js
```

Expected output: `test-install.js: all assertions passed`

- [ ] **Step 4: Commit**

```bash
git add phase-0/install.js phase-0/test/test-install.js
git commit -m "feat(phase-0): install.js writes hook config to .claude/settings.json"
```

---

### Task 6: report.js

**Files:**
- Create: `phase-0/report.js`
- Create: `phase-0/test/test-report.js`

**Interfaces:**
- Consumes: `phase-0/results/payloads/*.json`, `phase-0/results/latency.jsonl`
- Produces: `phase-0/results/hook-matrix.md`

- [ ] **Step 1: Write report.js**

Create `phase-0/report.js`:

```js
'use strict';

const fs = require('fs');
const path = require('path');

const RESULTS_DIR = path.resolve(__dirname, 'results');
const PAYLOADS_DIR = path.join(RESULTS_DIR, 'payloads');
const LATENCY_FILE = path.join(RESULTS_DIR, 'latency.jsonl');
const OUTPUT = path.join(RESULTS_DIR, 'hook-matrix.md');

function readPayloadFiles() {
  if (!fs.existsSync(PAYLOADS_DIR)) return [];
  return fs.readdirSync(PAYLOADS_DIR).filter(f => f.endsWith('.json'));
}

function readLatencyRecords() {
  if (!fs.existsSync(LATENCY_FILE)) return [];
  return fs.readFileSync(LATENCY_FILE, 'utf8')
    .trim().split('\n').filter(Boolean)
    .map(line => { try { return JSON.parse(line); } catch (_) { return null; } })
    .filter(Boolean);
}

function percentile(sorted, p) {
  if (!sorted.length) return null;
  const i = Math.max(0, Math.ceil((p / 100) * sorted.length) - 1);
  return sorted[i];
}

function hookName(filename) {
  // Files are named: HookName-2026-06-22T....json
  return filename.split('-')[0];
}

function buildMatrix(payloadFiles, latencyRecords) {
  const hooksSeen = new Set(payloadFiles.map(hookName));
  const countFor = name => payloadFiles.filter(f => hookName(f) === name).length;

  const preDurations = latencyRecords
    .filter(r => r.hookName === 'PreToolUse')
    .map(r => r.durationMs)
    .sort((a, b) => a - b);

  const p99 = percentile(preDurations, 99);
  const latencyStatus = preDurations.length === 0 ? 'PENDING'
    : p99 < 100 ? 'PASS' : 'FAIL';
  const latencyEvidence = preDurations.length === 0
    ? 'No latency data yet'
    : `p50=${percentile(preDurations, 50)}ms p95=${percentile(preDurations, 95)}ms p99=${p99}ms (${preDurations.length} samples)`;

  return [
    {
      id: 'H1',
      desc: '`PreToolUse` fires before file mutation',
      status: hooksSeen.has('PreToolUse') ? 'PASS' : 'PENDING',
      evidence: hooksSeen.has('PreToolUse')
        ? `${countFor('PreToolUse')} payload(s) in results/payloads/`
        : 'No PreToolUse payloads yet — ask Claude to read or write a file'
    },
    {
      id: 'H2',
      desc: '`PreToolUse` can block writes via exit code 1',
      status: hooksSeen.has('PreToolUse') ? 'MANUAL' : 'PENDING',
      evidence: hooksSeen.has('PreToolUse')
        ? 'Hook fired — verify by asking Claude to write to phase-0/sentinel/BLOCK_TARGET'
        : 'No PreToolUse payloads yet'
    },
    {
      id: 'H3',
      desc: '`UserPromptSubmit` can inject context into Claude input',
      status: hooksSeen.has('UserPromptSubmit') ? 'MANUAL' : 'PENDING',
      evidence: hooksSeen.has('UserPromptSubmit')
        ? `${countFor('UserPromptSubmit')} payload(s) — verify injection string visible in Claude context`
        : 'No UserPromptSubmit payloads yet — submit a prompt'
    },
    {
      id: 'H4',
      desc: '`/clear` produces detectable SessionStart event',
      status: hooksSeen.has('SessionStart') ? 'MANUAL' : 'PENDING',
      evidence: hooksSeen.has('SessionStart')
        ? `${countFor('SessionStart')} SessionStart payload(s) — inspect for /clear indicator field`
        : 'No SessionStart payloads yet — run /clear in Claude'
    },
    {
      id: 'H5',
      desc: '`SubagentStart` / `SubagentStop` fire at subagent boundaries',
      status: (hooksSeen.has('SubagentStart') && hooksSeen.has('SubagentStop')) ? 'PASS'
             : (hooksSeen.has('SubagentStart') || hooksSeen.has('SubagentStop')) ? 'PARTIAL'
             : 'PENDING',
      evidence: `SubagentStart: ${countFor('SubagentStart')}, SubagentStop: ${countFor('SubagentStop')}`
    },
    {
      id: 'H6',
      desc: 'Clean exit vs hard crash distinguishable via SessionEnd presence',
      status: hooksSeen.has('SessionEnd') ? 'MANUAL' : 'PENDING',
      evidence: hooksSeen.has('SessionEnd')
        ? `${countFor('SessionEnd')} SessionEnd payload(s) — compare with hard kill (no SessionEnd expected)`
        : 'No SessionEnd yet — close Claude cleanly, then repeat with hard kill'
    },
    {
      id: 'H7',
      desc: 'PreToolUse latency p99 < 100ms',
      status: latencyStatus,
      evidence: latencyEvidence
    }
  ];
}

function render(matrix, payloadFiles, latencyRecords) {
  const rows = matrix.map(r =>
    `| ${r.id} | ${r.desc} | **${r.status}** | ${r.evidence} |`
  ).join('\n');

  const hooksSeen = [...new Set(payloadFiles.map(hookName))].sort();

  return `# Coordify Phase 0 — Hook Validation Matrix

Generated: ${new Date().toISOString()}

## Results

| ID | Assumption | Status | Evidence |
|----|-----------|--------|----------|
${rows}

## Status Key

| Status | Meaning |
|--------|---------|
| PASS | Confirmed with captured payload evidence |
| PARTIAL | Hook fires but payload structure differs from assumption |
| FAIL | Does not fire or cannot achieve required behavior |
| MANUAL | Hook fired; requires manual observation to confirm |
| PENDING | Not yet tested |

## Coverage

- Total payload files: ${payloadFiles.length}
- Hooks seen: ${hooksSeen.length > 0 ? hooksSeen.join(', ') : 'none'}
- Latency samples: ${latencyRecords.length}
`;
}

const payloadFiles = readPayloadFiles();
const latencyRecords = readLatencyRecords();
const matrix = buildMatrix(payloadFiles, latencyRecords);
const md = render(matrix, payloadFiles, latencyRecords);

fs.mkdirSync(RESULTS_DIR, { recursive: true });
fs.writeFileSync(OUTPUT, md);
console.log(`Report written to ${OUTPUT}`);
```

- [ ] **Step 2: Write test-report.js**

Create `phase-0/test/test-report.js`:

```js
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
```

- [ ] **Step 3: Run test**

```bash
node phase-0/test/test-report.js
```

Expected output: `test-report.js: all assertions passed`

- [ ] **Step 4: Commit**

```bash
git add phase-0/report.js phase-0/test/test-report.js
git commit -m "feat(phase-0): report.js generates hook-matrix.md from captured results"
```

---

### Task 7: Run all tests + install + validate

**Files:**
- No new files

- [ ] **Step 1: Run all tests clean**

```bash
node phase-0/test/test-logger.js && \
node phase-0/test/test-pre-tool-use.js && \
node phase-0/test/test-install.js && \
node phase-0/test/test-report.js
```

Expected: four lines each ending in `all assertions passed`

- [ ] **Step 2: Install hooks**

```bash
node phase-0/install.js
```

Expected output:
```
Hook configuration written to .claude/settings.json
Run `claude` in this project root to begin validation.
```

- [ ] **Step 3: Verify .claude/settings.json**

```bash
cat .claude/settings.json
```

Verify all 7 hook keys are present: `SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `SubagentStart`, `SubagentStop`, `SessionEnd`.

- [ ] **Step 4: Open Claude Code and run validation procedure**

```bash
claude
```

Inside Claude Code, execute these in order:

1. Submit any prompt — captures `UserPromptSubmit` (H3). If injection string `[Coordify Phase 0] Hook injection active` appears in your context, H3 is PASS.
2. Ask Claude to read any file — captures `PreToolUse` read (H1).
3. Ask Claude to write to `phase-0/sentinel/BLOCK_TARGET` — if Claude is blocked, H2 is PASS.
4. Ask Claude to write to any other file — confirm it succeeds (H1 pass-through confirmed).
5. Run `/clear` — watch for `SessionStart` payload in `phase-0/results/payloads/`.
6. Ask Claude to use the Agent tool or spawn a subagent — watch for `SubagentStart`/`SubagentStop` (H5).
7. Exit Claude Code cleanly (`/exit`) — captures `SessionEnd` (H6 clean side).
8. Reopen Claude Code: `claude`. Then hard-kill with `kill -9 <pid>` from another terminal — confirm no `SessionEnd` payload written (H6 crash side).

- [ ] **Step 5: Generate report**

```bash
node phase-0/report.js
cat phase-0/results/hook-matrix.md
```

- [ ] **Step 6: Inspect raw payloads for H4**

```bash
ls phase-0/results/payloads/ | grep SessionStart
```

Open the payload captured after `/clear`. Look for a field indicating clear vs startup (e.g., `hook_event_type`, `reason`, `init_hook_args.type`). Record the actual field name in `TECHNICAL_VALIDATION.md`.

- [ ] **Step 7: Record results in TECHNICAL_VALIDATION.md**

Update `absolute-docs/TECHNICAL_VALIDATION.md` with:
- actual field names observed in each hook payload
- PASS / PARTIAL / FAIL / MANUAL for each assumption H1–H7
- raw payload file references as evidence
- any architecture implications for FAIL or PARTIAL results

- [ ] **Step 8: Final commit**

```bash
git add absolute-docs/TECHNICAL_VALIDATION.md phase-0/results/hook-matrix.md
git commit -m "feat(phase-0): validation complete — hook-matrix.md + TECHNICAL_VALIDATION updated"
```
