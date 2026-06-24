import { isLive, query } from '../ipc.js';
import { latestSession, readStats } from '../files.js';

export async function runStatus(root: string, opts: { json?: boolean }): Promise<void> {
  if (isLive(root)) {
    const resp = await query(root, 'get_state');
    if (!resp.ok) { process.stdout.write(`error: ${resp.error}\n`); return; }
    const d = resp.data as any;
    if (opts.json) { process.stdout.write(JSON.stringify(d, null, 2) + '\n'); return; }
    process.stdout.write(`status: live\nagents: ${d.agents?.length ?? 0}\nclaims: ${d.claims?.length ?? 0}\nconflicts: ${d.conflicts?.length ?? 0}\npeak heat: ${d.heat?.map((h: any) => `${h.pair?.join('↔')} ${h.heat}`).join(', ') || 'none'}\n`);
  } else {
    const id = latestSession(root);
    if (!id) { process.stdout.write('status: offline (no sessions found)\n'); return; }
    const stats = readStats(root, id) as any;
    if (!stats) { process.stdout.write(`status: offline (no stats for ${id})\n`); return; }
    if (opts.json) { process.stdout.write(JSON.stringify(stats, null, 2) + '\n'); return; }
    process.stdout.write(`status: offline (last session: ${id})\nagents seen: ${stats.agentsSeen ?? 0}\nclaims: ${stats.claimsCreated ?? 0}\nconflicts: ${stats.conflictsOpened ?? 0}\npeak heat: ${stats.peakHeat?.heat ?? 0}\n`);
  }
}
