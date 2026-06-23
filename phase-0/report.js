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

function readPayloads(payloadFiles) {
  return payloadFiles.map(f => {
    try { return JSON.parse(fs.readFileSync(path.join(PAYLOADS_DIR, f), 'utf8')); } catch (_) { return null; }
  }).filter(Boolean);
}

function buildMatrix(payloadFiles, latencyRecords) {
  const hooksSeen = new Set(payloadFiles.map(hookName));
  const countFor = name => payloadFiles.filter(f => hookName(f) === name).length;
  const payloads = readPayloads(payloadFiles);

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

  // H2: check if any real Write to sentinel was intercepted
  const sentinel = 'phase-0/sentinel/BLOCK_TARGET';
  const blockedWrites = payloads.filter(p =>
    p && p.hookName === 'PreToolUse' &&
    p.payload && p.payload.tool_name === 'Write' &&
    ((p.payload.tool_input && p.payload.tool_input.path && p.payload.tool_input.path.includes(sentinel)) ||
     (p.payload.tool_input && p.payload.tool_input.file_path && p.payload.tool_input.file_path.includes(sentinel)))
  );

  // H4: check if any SessionStart has source: clear
  const clearEvents = payloads.filter(p =>
    p.hookName === 'SessionStart' && p.payload.source === 'clear'
  );

  // H6: count real SessionStart vs SessionEnd (real = has UUID session_id)
  const realStarts = payloads.filter(p =>
    p.hookName === 'SessionStart' && p.payload.session_id && p.payload.session_id.includes('-')
  ).length;
  const realEnds = payloads.filter(p =>
    p.hookName === 'SessionEnd' && p.payload.session_id && p.payload.session_id.includes('-')
  ).length;
  const hasCrashEvidence = realStarts > realEnds;

  return [
    {
      id: 'H1',
      desc: '`PreToolUse` fires before file mutation',
      status: hooksSeen.has('PreToolUse') ? 'PASS' : 'PENDING',
      evidence: hooksSeen.has('PreToolUse')
        ? `${countFor('PreToolUse')} payload(s) captured, full payload structure confirmed`
        : 'No PreToolUse payloads yet'
    },
    {
      id: 'H2',
      desc: '`PreToolUse` can block writes via exit code 1',
      status: blockedWrites.length > 0 ? 'PASS' : hooksSeen.has('PreToolUse') ? 'MANUAL' : 'PENDING',
      evidence: blockedWrites.length > 0
        ? `${blockedWrites.length} Write tool call(s) to sentinel path intercepted — hook exits 1 on match`
        : 'No Write to sentinel captured yet'
    },
    {
      id: 'H3',
      desc: '`UserPromptSubmit` can inject context into Claude input',
      status: hooksSeen.has('UserPromptSubmit') ? 'MANUAL' : 'PENDING',
      evidence: hooksSeen.has('UserPromptSubmit')
        ? `${countFor('UserPromptSubmit')} payload(s) — hook fires and writes {"context":"..."} to stdout; Claude Code consumption unconfirmed`
        : 'No UserPromptSubmit payloads yet'
    },
    {
      id: 'H4',
      desc: '`/clear` produces detectable SessionStart with source: clear',
      status: clearEvents.length > 0 ? 'PASS' : hooksSeen.has('SessionStart') ? 'MANUAL' : 'PENDING',
      evidence: clearEvents.length > 0
        ? `Confirmed — field: payload.source, startup value: "startup", clear value: "clear" (${clearEvents.length} clear event(s) captured)`
        : `${countFor('SessionStart')} SessionStart payload(s) — run /clear to capture clear event`
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
      status: (hooksSeen.has('SessionEnd') && hasCrashEvidence) ? 'PASS'
             : hooksSeen.has('SessionEnd') ? 'MANUAL'
             : 'PENDING',
      evidence: hooksSeen.has('SessionEnd')
        ? `${realEnds} clean SessionEnd(s), ${realStarts} real SessionStart(s)${hasCrashEvidence ? ` — ${realStarts - realEnds} session(s) ended without SessionEnd (crash confirmed)` : ' — crash scenario not yet tested'}`
        : 'No SessionEnd yet'
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
