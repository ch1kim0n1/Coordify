import fs from 'fs';
import path from 'path';
import net from 'net';

// eslint-disable-next-line @typescript-eslint/no-implied-eval
const esmImport: (m: string) => Promise<Record<string, unknown>> = new Function('m', 'return import(m)') as never;

function readEvents(root: string, sessionId: string): Record<string, unknown>[] {
  const logPath = path.join(root, '.coordify', 'sessions', sessionId, 'events.log');
  try {
    return fs.readFileSync(logPath, 'utf8')
      .split('\n')
      .filter(l => l.trim())
      .map(l => { try { return JSON.parse(l); } catch { return null; } })
      .filter(Boolean) as Record<string, unknown>[];
  } catch { return []; }
}

export async function replayVisual(root: string, sessionId: string, opts: { speed?: number }): Promise<void> {
  const events = readEvents(root, sessionId);
  if (events.length === 0) { process.stdout.write(`no events in session ${sessionId}\n`); return; }
  const speed = opts.speed ?? 1;
  const { runReplayApp } = await esmImport(path.join(__dirname, 'tui', 'replay-app.js'));
  await (runReplayApp as (events: Record<string, unknown>[], speed: number) => Promise<void>)(events, speed);
}

export async function replayReconstruct(root: string, sessionId: string, opts: { stopAt?: number }): Promise<void> {
  const events = readEvents(root, sessionId);
  const sock = path.join(root, '.coordify', 'runtime', 'core.sock');
  const tok = (() => { try { return fs.readFileSync(path.join(root, '.coordify', 'runtime', 'session.token'), 'utf8').trim(); } catch { return ''; } })();
  if (!tok) { process.stdout.write('error: no session token\n'); return; }

  const limit = opts.stopAt ?? events.length;
  const slice = events.slice(0, limit);

  process.stdout.write(`Reconstructing session ${sessionId}...\n`);
  process.stdout.write(`  Submitting ${slice.length}/${events.length} events\n`);

  for (let i = 0; i < slice.length; i++) {
    const ev = slice[i];
    await new Promise<void>((resolve, reject) => {
      const s = net.createConnection(sock);
      let buf = '';
      const id = 'r' + i;
      s.setEncoding('utf8');
      s.once('connect', () => {
        s.write(JSON.stringify({ id, token: tok, action: 'submit_event', capVersion: '0.1', event: ev }) + '\n');
      });
      s.on('data', (d: string) => {
        buf += d;
        let idx: number;
        while ((idx = buf.indexOf('\n')) >= 0) {
          const line = buf.slice(0, idx); buf = buf.slice(idx + 1);
          if (!line.trim()) continue;
          try { JSON.parse(line); } catch {}
          s.destroy(); resolve();
        }
      });
      s.on('error', reject);
    });
    process.stdout.write(`  Submitted event ${i + 1}/${slice.length}: ${String(ev.type ?? '?')}\n`);
  }

  if (opts.stopAt && opts.stopAt < events.length) {
    process.stdout.write(`Core is running. Use 'coordify watch' to inspect state.\n`);
  } else {
    process.stdout.write(`Reconstruction complete.\n`);
  }
}
