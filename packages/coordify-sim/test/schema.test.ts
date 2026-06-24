import test from 'node:test';
import assert from 'node:assert';
import { validateScript } from '../src/schema.js';

test('validates a correct script', () => {
  const result = validateScript({
    name: 'test',
    agents: ['a1'],
    steps: [{ delay_ms: 0, event: { type: 'AGENT_JOINED', agentId: 'a1' } }],
  });
  assert.ok(!Array.isArray(result));
  assert.equal((result as any).name, 'test');
});

test('rejects script missing name', () => {
  const result = validateScript({ agents: [], steps: [] });
  assert.ok(Array.isArray(result));
  assert.ok((result as string[]).some(e => e.includes('name')));
});

test('rejects script with bad step (missing event)', () => {
  const result = validateScript({ name: 'x', agents: [], steps: [{ delay_ms: 0 }] });
  assert.ok(Array.isArray(result));
});

test('rejects step with non-object event', () => {
  const result = validateScript({ name: 'x', agents: [], steps: [{ delay_ms: 0, event: 'bad' }] });
  assert.ok(Array.isArray(result));
});
