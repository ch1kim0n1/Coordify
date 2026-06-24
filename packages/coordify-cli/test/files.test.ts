import test from 'node:test';
import assert from 'node:assert';
import os from 'os';
import path from 'path';
import fs from 'fs';
import { listSessions, latestSession, readStats, readKnowledge } from '../src/files.js';

function tmpRoot(): string {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'cli-files-'));
  // scaffold .coordify/sessions/2026-06-23_12-00-00/stats.json
  const sid = '2026-06-23_12-00-00';
  const sdir = path.join(dir, '.coordify', 'sessions', sid);
  fs.mkdirSync(sdir, { recursive: true });
  fs.writeFileSync(path.join(sdir, 'stats.json'), JSON.stringify({
    agentsSeen: 2, claimsCreated: 3, peakHeat: { heat: 82, pair: ['a', 'b'] }
  }));
  // knowledge
  const kdir = path.join(dir, '.coordify', 'knowledge');
  fs.mkdirSync(kdir, { recursive: true });
  fs.writeFileSync(path.join(kdir, 'hotzones.json'), JSON.stringify({ 'src/x.rs': 3 }));
  return dir;
}

test('listSessions returns sorted session ids', () => {
  const root = tmpRoot();
  const sessions = listSessions(root);
  assert.equal(sessions.length, 1);
  assert.equal(sessions[0], '2026-06-23_12-00-00');
  fs.rmSync(root, { recursive: true });
});

test('latestSession returns last session id', () => {
  const root = tmpRoot();
  const id = latestSession(root);
  assert.equal(id, '2026-06-23_12-00-00');
  fs.rmSync(root, { recursive: true });
});

test('latestSession returns null when no sessions', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'cli-empty-'));
  assert.equal(latestSession(root), null);
  fs.rmSync(root, { recursive: true });
});

test('readStats returns parsed JSON', () => {
  const root = tmpRoot();
  const stats = readStats(root, '2026-06-23_12-00-00');
  assert.equal(stats?.agentsSeen, 2);
  fs.rmSync(root, { recursive: true });
});

test('readStats returns null for missing session', () => {
  const root = tmpRoot();
  assert.equal(readStats(root, 'nonexistent'), null);
  fs.rmSync(root, { recursive: true });
});

test('readKnowledge returns hotzones', () => {
  const root = tmpRoot();
  const k = readKnowledge(root);
  assert.equal(k.hotzones?.['src/x.rs'], 3);
  fs.rmSync(root, { recursive: true });
});
