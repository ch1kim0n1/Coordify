import { listSessions, readStats, readSummary, readEntertainment } from '../files.js';

export async function runSessionList(root: string, opts: { json?: boolean }): Promise<void> {
  const sessions = listSessions(root);
  if (opts.json) { process.stdout.write(JSON.stringify(sessions, null, 2) + '\n'); return; }
  if (sessions.length === 0) { process.stdout.write('no sessions\n'); return; }
  process.stdout.write('SESSION ID               \n');
  for (const s of sessions) process.stdout.write(s + '\n');
}

export async function runSessionInspect(root: string, id: string, opts: { json?: boolean }): Promise<void> {
  const stats = readStats(root, id) as any;
  const summary = readSummary(root, id) as any;
  const ent = readEntertainment(root, id) as any;
  if (!stats) { process.stdout.write(`no session with id '${id}'\n`); return; }
  if (opts.json) {
    process.stdout.write(JSON.stringify({ stats, summary, entertainment: ent }, null, 2) + '\n');
    return;
  }
  process.stdout.write(`=== Session ${id} ===\n`);
  process.stdout.write(`agents: ${stats.agentsSeen ?? 0}  claims: ${stats.claimsCreated ?? 0}  conflicts: ${stats.conflictsOpened ?? 0}\n`);
  process.stdout.write(`peak heat: ${stats.peakHeat?.heat ?? 0}  duration: ${Math.round((stats.durationMs ?? 0) / 1000)}s\n`);
  if (summary?.narrative) process.stdout.write(`\nNarrative: ${summary.narrative}\n`);
  if (ent?.badges?.length) {
    process.stdout.write('\nBadges:\n');
    for (const b of ent.badges) process.stdout.write(`  ${b.label}: ${b.agent}\n`);
  }
}
