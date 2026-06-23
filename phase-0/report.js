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
