import { isLive, query } from '../ipc.js';
import { latestSession, readHeatHistory } from '../files.js';

export async function runHeat(root: string, opts: { json?: boolean }): Promise<void> {
  let edges: any[] = [];
  if (isLive(root)) {
    const resp = await query(root, 'get_state');
    if (!resp.ok) { process.stdout.write(`error: ${resp.error}\n`); return; }
    edges = ((resp.data as any)?.heat ?? []).sort((a: any, b: any) => b.heat - a.heat);
  } else {
    const id = latestSession(root);
    const history = id ? readHeatHistory(root, id) as any[] : [];
    // Last entry per pair
    const byPair = new Map<string, any>();
    for (const e of (history ?? [])) { byPair.set((e.pair ?? []).join('↔'), e); }
    edges = [...byPair.values()].sort((a, b) => b.heat - a.heat);
  }
  if (opts.json) { process.stdout.write(JSON.stringify(edges, null, 2) + '\n'); return; }
  if (edges.length === 0) { process.stdout.write('no heat data\n'); return; }
  process.stdout.write('PAIR                          HEAT   BAND\n');
  for (const e of edges) {
    const pair = (e.pair ?? []).join(' ↔ ');
    process.stdout.write(`${String(pair).padEnd(30)}${String(e.heat).padEnd(7)}${e.band ?? ''}\n`);
  }
}
