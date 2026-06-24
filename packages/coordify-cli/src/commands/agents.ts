import { isLive, query } from '../ipc.js';
import { latestSession, readStats } from '../files.js';

export async function runAgents(root: string, opts: { json?: boolean }): Promise<void> {
  if (isLive(root)) {
    const resp = await query(root, 'get_state');
    if (!resp.ok) { process.stdout.write(`error: ${resp.error}\n`); return; }
    const agents = (resp.data as any)?.agents ?? [];
    if (opts.json) { process.stdout.write(JSON.stringify(agents, null, 2) + '\n'); return; }
    if (agents.length === 0) { process.stdout.write('no agents\n'); return; }
    process.stdout.write('AGENT ID        STATE       CLAIM\n');
    for (const a of agents) {
      process.stdout.write(`${String(a.agentId).padEnd(16)}${String(a.state).padEnd(12)}${a.claimId ?? '-'}\n`);
    }
  } else {
    const id = latestSession(root);
    const stats = id ? readStats(root, id) as any : null;
    if (opts.json) { process.stdout.write(JSON.stringify(stats?.agents ?? {}, null, 2) + '\n'); return; }
    process.stdout.write('offline — showing last session per-agent tallies\n');
    const agents = Object.entries(stats?.agents ?? {});
    for (const [aid, t] of agents) {
      process.stdout.write(`${String(aid).padEnd(16)}sessions: ${(t as any).sessions ?? 0}\n`);
    }
  }
}
