import { isLive } from '../ipc.js';
import { latestSession, readEventLog } from '../files.js';

export async function runLogs(root: string, opts: { tail?: number; follow?: boolean; json?: boolean }): Promise<void> {
  const id = latestSession(root);
  if (!id) { process.stdout.write('no sessions found\n'); return; }
  const lines = readEventLog(root, id);
  const tail = opts.tail ?? 20;
  const slice = lines.slice(-tail);
  if (opts.json) { process.stdout.write(JSON.stringify(slice.map(l => { try { return JSON.parse(l); } catch { return l; } }), null, 2) + '\n'); return; }
  for (const line of slice) {
    try {
      const e = JSON.parse(line);
      const ts = e.ts ? e.ts.replace('T', ' ').replace('Z', '') : '?';
      const fields = Object.entries(e).filter(([k]) => !['type', 'ts'].includes(k)).map(([k, v]) => `${k}=${JSON.stringify(v)}`).join(' ');
      process.stdout.write(`[${ts}] ${e.type} ${fields}\n`);
    } catch {
      process.stdout.write(line + '\n');
    }
  }
  if (opts.follow && isLive(root)) {
    // tail -f: re-check every 500ms, print new lines
    let known = lines.length;
    const interval = setInterval(() => {
      const fresh = readEventLog(root, id);
      if (fresh.length > known) {
        fresh.slice(known).forEach(l => process.stdout.write(l + '\n'));
        known = fresh.length;
      }
    }, 500);
    process.on('SIGINT', () => { clearInterval(interval); process.exit(0); });
    await new Promise(() => {}); // wait for SIGINT
  }
}
