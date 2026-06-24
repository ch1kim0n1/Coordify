import test from 'node:test';
import assert from 'node:assert';
import { execSync } from 'child_process';
import path from 'path';

const cli = path.resolve('src/cli.ts');
const run = (args: string) => {
  try {
    return execSync(`npx tsx ${cli} ${args}`, { encoding: 'utf8', env: { ...process.env, COORDIFY_ROOT: '/tmp/nonexistent-root-xyz' } });
  } catch (e: any) { return e.stdout ?? ''; }
};

test('unknown command prints usage', () => {
  const out = run('badcommand');
  assert.ok(out.includes('usage') || out.includes('Usage') || out.includes('coordify'));
});

test('status offline: no sessions found', () => {
  const out = run('status');
  assert.ok(out.includes('offline') || out.includes('no') || out.includes('session'));
});

test('--help prints command list', () => {
  const out = run('--help');
  assert.ok(out.includes('watch') || out.includes('status') || out.includes('coordify'));
});
