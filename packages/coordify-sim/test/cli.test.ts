import test from 'node:test';
import assert from 'node:assert';
import { execSync } from 'child_process';
import path from 'path';
import os from 'os';
import fs from 'fs';

const cli = path.resolve('src/cli.ts');
const run = (args: string) => {
  try { return execSync(`npx tsx ${cli} ${args}`, { encoding: 'utf8' }); }
  catch (e: any) { return e.stdout ?? e.stderr ?? ''; }
};

test('--help prints commands', () => {
  const out = run('--help');
  assert.ok(out.includes('simulate') || out.includes('replay') || out.includes('coordify'));
});

test('simulate --dry-run with valid script prints steps', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'sim-cli-'));
  const scriptPath = path.join(root, 'test.json');
  fs.writeFileSync(scriptPath, JSON.stringify({
    name: 'test', agents: ['a1'],
    steps: [{ delay_ms: 0, event: { type: 'AGENT_JOINED', agentId: 'a1' } }]
  }));
  const out = run(`simulate ${scriptPath} --dry-run --root ${root}`);
  assert.ok(out.includes('dry-run') || out.includes('AGENT_JOINED') || out.includes('step'));
  fs.rmSync(root, { recursive: true });
});

test('simulate with invalid script prints errors and exits', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'sim-cli2-'));
  const scriptPath = path.join(root, 'bad.json');
  fs.writeFileSync(scriptPath, JSON.stringify({ agents: [] })); // missing name
  const out = run(`simulate ${scriptPath} --dry-run --root ${root}`);
  assert.ok(out.includes('error') || out.includes('name') || out.includes('invalid'));
  fs.rmSync(root, { recursive: true });
});
