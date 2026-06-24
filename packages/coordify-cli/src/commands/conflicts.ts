import { isLive, query } from '../ipc.js';

export async function runConflicts(root: string, opts: { json?: boolean }): Promise<void> {
  if (!isLive(root)) { process.stdout.write('conflicts: no live network\n'); return; }
  const resp = await query(root, 'get_state');
  if (!resp.ok) { process.stdout.write(`error: ${resp.error}\n`); return; }
  const conflicts = (resp.data as any)?.conflicts ?? [];
  if (opts.json) { process.stdout.write(JSON.stringify(conflicts, null, 2) + '\n'); return; }
  if (conflicts.length === 0) { process.stdout.write('no active conflicts\n'); return; }
  process.stdout.write('CONFLICT ID     AGENTS                STATE               AGE\n');
  for (const c of conflicts) {
    const agents = (c.agents ?? []).join(',');
    const age = c.ageMs ? `${Math.round(c.ageMs / 1000)}s` : '?';
    process.stdout.write(`${String(c.conflictId).padEnd(16)}${String(agents).padEnd(22)}${String(c.state).padEnd(20)}${age}\n`);
  }
}
