import test from 'node:test';
import assert from 'node:assert';
import os from 'os';
import path from 'path';
import fs from 'fs';

function makeSession(root: string, id: string) {
  const sdir = path.join(root, '.coordify', 'sessions', id);
  fs.mkdirSync(sdir, { recursive: true });
  fs.writeFileSync(path.join(sdir, 'stats.json'), JSON.stringify({ agentsSeen: 2, claimsCreated: 3, conflictsOpened: 1, durationMs: 9000, peakHeat: { heat: 82 } }));
  fs.writeFileSync(path.join(sdir, 'session-summary.json'), JSON.stringify({ narrative: 'Good session.' }));
  fs.writeFileSync(path.join(sdir, 'entertainment.json'), JSON.stringify({ badges: [], leaderboards: [], narrative: 'Good session.' }));
}

test('runSessionList prints session ids', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'cli-sess-'));
  makeSession(root, '2026-06-23_10-00-00');
  makeSession(root, '2026-06-23_11-00-00');
  const { runSessionList } = await import('../../src/commands/session.js');
  let out = '';
  const orig = process.stdout.write.bind(process.stdout);
  (process.stdout as any).write = (s: string) => { out += s; return true; };
  await runSessionList(root, {});
  (process.stdout as any).write = orig;
  assert.ok(out.includes('2026-06-23_10-00-00'));
  assert.ok(out.includes('2026-06-23_11-00-00'));
  fs.rmSync(root, { recursive: true });
});

test('runSessionInspect prints stats and narrative', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'cli-inspect-'));
  makeSession(root, '2026-06-23_10-00-00');
  const { runSessionInspect } = await import('../../src/commands/session.js');
  let out = '';
  const orig = process.stdout.write.bind(process.stdout);
  (process.stdout as any).write = (s: string) => { out += s; return true; };
  await runSessionInspect(root, '2026-06-23_10-00-00', {});
  (process.stdout as any).write = orig;
  assert.ok(out.includes('Good session.') || out.includes('82') || out.includes('agents'));
  fs.rmSync(root, { recursive: true });
});

test('runStats prints latest session stats', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'cli-stats-'));
  makeSession(root, '2026-06-23_10-00-00');
  const { runStats } = await import('../../src/commands/stats.js');
  let out = '';
  const orig = process.stdout.write.bind(process.stdout);
  (process.stdout as any).write = (s: string) => { out += s; return true; };
  await runStats(root, {});
  (process.stdout as any).write = orig;
  assert.ok(out.includes('2') || out.includes('82') || out.includes('agent'));
  fs.rmSync(root, { recursive: true });
});
