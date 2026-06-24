import { latestSession, readStats } from '../files.js';

export async function runStats(root: string, opts: { json?: boolean }): Promise<void> {
  const id = latestSession(root);
  if (!id) { process.stdout.write('no sessions found\n'); return; }
  const stats = readStats(root, id) as any;
  if (!stats) { process.stdout.write(`no stats.json for session ${id}\n`); return; }
  if (opts.json) { process.stdout.write(JSON.stringify(stats, null, 2) + '\n'); return; }
  process.stdout.write([
    `session:    ${id}`,
    `agents:     ${stats.agentsSeen ?? 0}`,
    `claims:     ${stats.claimsCreated ?? 0}`,
    `conflicts:  ${stats.conflictsOpened ?? 0}`,
    `peak heat:  ${stats.peakHeat?.heat ?? 0} (${(stats.peakHeat?.pair ?? []).join('↔')})`,
    `duration:   ${Math.round((stats.durationMs ?? 0) / 1000)}s`,
  ].join('\n') + '\n');
}
