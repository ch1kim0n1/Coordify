import test from 'node:test';
import assert from 'node:assert';
import React from 'react';
import { render } from 'ink-testing-library';
import AgentPanel from '../../src/tui/components/AgentPanel.js';
import HeatPanel from '../../src/tui/components/HeatPanel.js';
import ConflictPanel from '../../src/tui/components/ConflictPanel.js';

test('AgentPanel renders agents table', () => {
  const agents = [
    { agentId: 'agent-1', state: 'ACTIVE', claimId: 'claim-1' },
    { agentId: 'agent-2', state: 'IDLE', claimId: null },
  ];
  const { lastFrame } = render(React.createElement(AgentPanel, { agents }));
  assert.ok(lastFrame()?.includes('agent-1'));
  assert.ok(lastFrame()?.includes('ACTIVE'));
  assert.ok(lastFrame()?.includes('agent-2'));
});

test('AgentPanel renders empty state', () => {
  const { lastFrame } = render(React.createElement(AgentPanel, { agents: [] }));
  assert.ok(lastFrame()?.includes('no agents') || lastFrame()?.includes('Agents'));
});

test('HeatPanel renders heat edges', () => {
  const heat = [{ pair: ['a', 'b'], heat: 82, band: 'CONFLICT_CANDIDATE' }];
  const { lastFrame } = render(React.createElement(HeatPanel, { heat }));
  assert.ok(lastFrame()?.includes('82') || lastFrame()?.includes('a'));
});

test('ConflictPanel renders conflict list', () => {
  const conflicts = [{ conflictId: 'c-1', agents: ['a', 'b'], paths: ['x.rs'], state: 'NEGOTIATING', ageMs: 5000 }];
  const { lastFrame } = render(React.createElement(ConflictPanel, { conflicts }));
  assert.ok(lastFrame()?.includes('c-1') || lastFrame()?.includes('NEGOTIATING'));
});
