import test from 'node:test';
import assert from 'node:assert';
import React from 'react';
import { render } from 'ink-testing-library';
import ReplayFrame from '../../src/tui/replay-watch.js';

test('ReplayFrame renders event type and index', () => {
  const events = [
    { type: 'AGENT_JOINED', agentId: 'a1', ts: '2026-06-23T00:00:00Z' },
    { type: 'CLAIM_PROPOSED', agentId: 'a1', ts: '2026-06-23T00:00:01Z' },
  ];
  const { lastFrame } = render(React.createElement(ReplayFrame, {
    events, currentIndex: 0, total: 2, speed: 1, paused: false
  }));
  const frame = lastFrame() ?? '';
  assert.ok(frame.includes('AGENT_JOINED') || frame.includes('1/2'));
});
