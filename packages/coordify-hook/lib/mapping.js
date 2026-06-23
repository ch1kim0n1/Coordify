'use strict';

const MAX_SUMMARY = 200;

// Fixed-order, case-insensitive keyword rules. First match wins.
const INTENT_RULES = [
  [/secur/i, 'SECURITY'],
  [/\btest/i, 'TESTING'],
  [/\bdoc/i, 'DOCUMENTATION'],
  [/refactor/i, 'REFACTOR'],
  [/perf|optimi/i, 'PERFORMANCE'],
  [/fix|bug/i, 'BUGFIX'],
];

function classifyIntent(prompt) {
  const p = String(prompt == null ? '' : prompt);
  for (const [re, intent] of INTENT_RULES) {
    if (re.test(p)) return intent;
  }
  return 'FEATURE';
}

function isTestCommand(cmd) {
  return /\b(test|jest|pytest|cargo test|go test|npm test|vitest|mocha)\b/i.test(String(cmd || ''));
}

// Pure translation of one hook payload to an adapter action.
//   {kind:'bootstrap'}                  ensure the sidecar exists; no Core traffic
//   {kind:'forward', event:{type,...}}  CAP event for Core (caller injects agentId)
//   {kind:'release'}                    SessionEnd: release live claims, then disconnect
//   {kind:'record', record:{type,...}}  recorded-only (local trace, not sent to Core)
function mapEvent(hook, payload) {
  payload = payload || {};
  switch (hook) {
    case 'SessionStart':
      return payload.source === 'clear'
        ? { kind: 'forward', event: { type: 'CLEAR_INVOKED' } }
        : { kind: 'bootstrap' };

    case 'UserPromptSubmit': {
      const summary = String(payload.prompt || '').trim().slice(0, MAX_SUMMARY);
      return {
        kind: 'forward',
        event: {
          type: 'CLAIM_PROPOSED',
          intent: classifyIntent(payload.prompt),
          domains: [],
          estimatedFiles: [],
          confidence: 0.7,
          task: { summary },
        },
      };
    }

    case 'SubagentStart':
      return { kind: 'forward', event: { type: 'AGENT_STATE_CHANGED', state: 'SUBAGENT_WAITING' } };
    case 'SubagentStop':
      return { kind: 'forward', event: { type: 'AGENT_STATE_CHANGED', state: 'ACTIVE' } };

    case 'SessionEnd':
      return { kind: 'release' };

    case 'PreToolUse': {
      const tool = payload.tool_name || '';
      const type = tool === 'Edit' || tool === 'Write' || tool === 'MultiEdit'
        ? 'RISKY_WRITE_CHECKED'
        : 'TOOL_PRECHECK';
      return { kind: 'record', record: { type, tool, input: payload.tool_input || {} } };
    }

    case 'PostToolUse': {
      const tool = payload.tool_name || '';
      const ti = payload.tool_input || {};
      if (tool === 'Edit' || tool === 'Write' || tool === 'MultiEdit') {
        const file = ti.file_path || ti.path;
        return file
          ? { kind: 'forward', event: { type: 'FILE_TOUCHED', files: [file] } }
          : { kind: 'record', record: { type: 'FILE_TOUCHED', tool, file: null } };
      }
      if (tool === 'Read') {
        return { kind: 'record', record: { type: 'FILE_READ', tool, file: ti.file_path || ti.path || null } };
      }
      if (tool === 'Bash') {
        return { kind: 'record', record: { type: isTestCommand(ti.command) ? 'TEST_RUN' : 'COMMAND_EXECUTED', tool, command: ti.command || '' } };
      }
      return { kind: 'record', record: { type: 'TOOL_USED', tool } };
    }

    default:
      return { kind: 'record', record: { type: 'UNKNOWN_HOOK', hook } };
  }
}

module.exports = { mapEvent, classifyIntent, isTestCommand, MAX_SUMMARY };
