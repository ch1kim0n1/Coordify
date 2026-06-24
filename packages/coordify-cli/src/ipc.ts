import net from 'net';
import fs from 'fs';
import { socket as socketPath, readToken } from './paths.js';

export interface IpcResponse {
  id: string;
  ok: boolean;
  agent_id?: string;
  error?: string;
  data?: unknown;
}

export function isLive(root: string): boolean {
  return fs.existsSync(socketPath(root));
}

export class CoreClient {
  private sock: net.Socket | null = null;
  private buf = '';
  private seq = 0;
  private pending = new Map<string, (r: IpcResponse) => void>();

  constructor(private readonly sockPath: string, private readonly tok: string) {}

  connect(): Promise<void> {
    return new Promise((resolve, reject) => {
      const s = net.createConnection(this.sockPath);
      s.setEncoding('utf8');
      s.once('connect', () => { this.sock = s; resolve(); });
      s.once('error', reject);
      s.on('data', (chunk: string) => {
        this.buf += chunk;
        let i: number;
        while ((i = this.buf.indexOf('\n')) >= 0) {
          const line = this.buf.slice(0, i); this.buf = this.buf.slice(i + 1);
          if (!line.trim()) continue;
          let resp: IpcResponse;
          try { resp = JSON.parse(line); } catch { continue; }
          const resolve = this.pending.get(resp.id);
          if (resolve) { this.pending.delete(resp.id); resolve(resp); }
        }
      });
    });
  }

  query(action: string, payload: Record<string, unknown> = {}): Promise<IpcResponse> {
    return new Promise((resolve, reject) => {
      if (!this.sock) { reject(new Error('not connected')); return; }
      const id = 'q' + (++this.seq);
      this.pending.set(id, resolve);
      const msg = JSON.stringify({ id, token: this.tok, action, ...payload }) + '\n';
      this.sock.write(msg, err => { if (err) { this.pending.delete(id); reject(err); } });
    });
  }

  close(): void {
    try { this.sock?.end(); } catch (_) {}
    this.sock = null;
  }
}

export async function query(root: string, action: string, payload: Record<string, unknown> = {}): Promise<IpcResponse> {
  const tok = readToken(root);
  if (!tok) return { id: '?', ok: false, error: 'coordify-core is not running; open a Claude Code session to start it' };
  const sock = socketPath(root);
  const client = new CoreClient(sock, tok);
  try {
    await client.connect();
    const resp = await client.query(action, payload);
    // A stale token (Core restarted with a new one) surfaces as "unauthorized"
    // from Core. Translate to a clearer, actionable message for the CLI user.
    if (!resp.ok && resp.error === 'unauthorized') {
      return { ...resp, error: 'coordify-core restarted; please retry' };
    }
    return resp;
  } catch (e) {
    // Connection refused / Core crashed / stale socket. Never throw — return a
    // consistent, actionable error so the CLI does not dump a stack trace.
    const msg = e instanceof Error ? e.message : String(e);
    if (msg.includes('ECONNREFUSED') || msg.includes('ENOENT') || msg.includes('connect')) {
      return { id: '?', ok: false, error: 'coordify-core is not running; open a Claude Code session to start it' };
    }
    return { id: '?', ok: false, error: 'coordify-core unreachable: ' + msg };
  } finally {
    client.close();
  }
}
