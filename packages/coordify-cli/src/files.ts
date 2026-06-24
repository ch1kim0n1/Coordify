import fs from 'fs';
import path from 'path';
import { sessionDir, sessions, knowledgeDir } from './paths.js';

function readJson<T>(filePath: string): T | null {
  try { return JSON.parse(fs.readFileSync(filePath, 'utf8')) as T; } catch { return null; }
}

export function listSessions(root: string): string[] {
  const dir = sessions(root);
  let entries: fs.Dirent[];
  try { entries = fs.readdirSync(dir, { withFileTypes: true }); }
  catch { return []; }
  // Only directories (a stray file under sessions/ should not appear as a
  // session), newest-first. Session ids are ISO-ish timestamps so lexical
  // reverse = chronological reverse.
  return entries
    .filter(e => e.isDirectory())
    .map(e => e.name)
    .sort()
    .reverse();
}

export function latestSession(root: string): string | null {
  const s = listSessions(root);
  return s.length > 0 ? s[0] : null;
}

export function readStats(root: string, id: string): Record<string, unknown> | null {
  return readJson(path.join(sessionDir(root, id), 'stats.json'));
}

export function readSummary(root: string, id: string): Record<string, unknown> | null {
  return readJson(path.join(sessionDir(root, id), 'session-summary.json'));
}

export function readHeatHistory(root: string, id: string): unknown[] | null {
  return readJson(path.join(sessionDir(root, id), 'heat-history.json'));
}

export function readEntertainment(root: string, id: string): Record<string, unknown> | null {
  return readJson(path.join(sessionDir(root, id), 'entertainment.json'));
}

export function readEventLog(root: string, id: string): string[] {
  const p = path.join(sessionDir(root, id), 'events.log');
  try { return fs.readFileSync(p, 'utf8').split('\n').filter(l => l.trim()); } catch { return []; }
}

export function readKnowledge(root: string): Record<string, unknown> {
  const dir = knowledgeDir(root);
  return {
    hotzones: readJson(path.join(dir, 'hotzones.json')),
    coupling: readJson(path.join(dir, 'coupling-graph.json')),
    profiles: readJson(path.join(dir, 'agent-profiles.json')),
    velocity: readJson(path.join(dir, 'velocity-profiles.json')),
    overhead: readJson(path.join(dir, 'coordination-overhead.json')),
  };
}
