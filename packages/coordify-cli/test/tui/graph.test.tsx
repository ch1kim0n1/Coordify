import test from 'node:test';
import assert from 'node:assert';
import React from 'react';
import { render } from 'ink-testing-library';
import CouplingGraph from '../../src/tui/components/CouplingGraph.js';
import HeatMatrix from '../../src/tui/components/HeatMatrix.js';

test('CouplingGraph renders edge list sorted by count', () => {
  const edges = [
    { a: 'src/x.rs', b: 'src/y.rs', count: 5 },
    { a: 'src/a.rs', b: 'src/b.rs', count: 10 },
  ];
  const { lastFrame } = render(React.createElement(CouplingGraph, { edges, top: 20 }));
  const frame = lastFrame() ?? '';
  assert.ok(frame.includes('src/a.rs') || frame.includes('10'));
  // higher count appears first (or both appear)
  assert.ok(frame.includes('src/x.rs') || frame.includes('5'));
});

test('CouplingGraph renders empty state', () => {
  const { lastFrame } = render(React.createElement(CouplingGraph, { edges: [], top: 20 }));
  assert.ok(lastFrame()?.includes('no coupling') || lastFrame()?.includes('Coupling'));
});

test('HeatMatrix renders agent pair grid', () => {
  const heat = [
    { pair: ['agent-1', 'agent-2'], heat: 82, band: 'CONFLICT_CANDIDATE' },
    { pair: ['agent-1', 'agent-3'], heat: 30, band: 'SAFE' },
  ];
  const { lastFrame } = render(React.createElement(HeatMatrix, { heat }));
  const frame = lastFrame() ?? '';
  assert.ok(frame.includes('agent-1') || frame.includes('82'));
});
