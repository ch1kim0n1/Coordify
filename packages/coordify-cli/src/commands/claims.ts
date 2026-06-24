import { isLive, query } from '../ipc.js';

export async function runClaims(root: string, opts: { json?: boolean }): Promise<void> {
  if (!isLive(root)) { process.stdout.write('coordify-core is not running; open a Claude Code session to start it\n'); return; }
  const resp = await query(root, 'get_state');
  if (!resp.ok) { process.stdout.write(`error: ${resp.error}\n`); return; }
  const claims = (resp.data as any)?.claims ?? [];
  if (opts.json) { process.stdout.write(JSON.stringify(claims, null, 2) + '\n'); return; }
  if (claims.length === 0) { process.stdout.write('no active claims\n'); return; }
  process.stdout.write('CLAIM ID        AGENT           FILES\n');
  for (const c of claims) {
    const files = (c.files ?? []).slice(0, 3).join(', ') + ((c.files ?? []).length > 3 ? '...' : '');
    process.stdout.write(`${String(c.claimId).padEnd(16)}${String(c.agentId).padEnd(16)}${files}\n`);
  }
}
