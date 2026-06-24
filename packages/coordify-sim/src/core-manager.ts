import net from 'net';
import fs from 'fs';
import path from 'path';
import { spawn, ChildProcess } from 'child_process';

export interface CoreHandle {
  socketPath: string;
  token: string;
  spawned: boolean;
}

function socketPath(root: string) { return path.join(root, '.coordify', 'runtime', 'core.sock'); }
function tokenPath(root: string)  { return path.join(root, '.coordify', 'runtime', 'session.token'); }

function readToken(root: string): string | null {
  try { return fs.readFileSync(tokenPath(root), 'utf8').trim(); } catch { return null; }
}

function resolveBin(override?: string): string | null {
  if (override) return fs.existsSync(override) ? override : null;
  if (process.env.COORDIFY_CORE_BIN) return process.env.COORDIFY_CORE_BIN;
  const base = path.resolve(__dirname, '..', '..', 'coordify-core', 'target');
  for (const p of [
    path.join(base, 'release', 'coordify-core'),
    path.join(base, 'debug', 'coordify-core'),
  ]) { if (fs.existsSync(p)) return p; }
  return 'coordify-core'; // PATH fallback
}

function waitForSocket(sockPath: string, timeoutMs: number): Promise<void> {
  return new Promise((resolve, reject) => {
    const deadline = Date.now() + timeoutMs;
    function check() {
      if (fs.existsSync(sockPath)) { resolve(); return; }
      if (Date.now() > deadline) { reject(new Error(`socket never appeared: ${sockPath}`)); return; }
      setTimeout(check, 100);
    }
    check();
  });
}

export class CoreManager {
  private spawned: ChildProcess | null = null;

  constructor(private readonly root: string, private readonly binOverride?: string) {}

  async ensure(): Promise<CoreHandle> {
    const sock = socketPath(this.root);
    if (fs.existsSync(sock)) {
      const tok = readToken(this.root) ?? '';
      return { socketPath: sock, token: tok, spawned: false };
    }
    const bin = resolveBin(this.binOverride);
    if (!bin) throw new Error(`coordify-core binary not found`);
    // Try spawning — if binary doesn't exist, spawn throws
    const child = spawn(bin, ['--root', this.root], {
      detached: false,
      stdio: 'ignore',
      env: { ...process.env },
    });
    child.on('error', err => { throw err; });
    this.spawned = child;
    await waitForSocket(sock, 5000);
    const tok = readToken(this.root) ?? '';
    return { socketPath: sock, token: tok, spawned: true };
  }

  async stop(): Promise<void> {
    if (!this.spawned) return;
    this.spawned.kill('SIGTERM');
    this.spawned = null;
    // wait for socket to disappear (up to 3s)
    const sock = socketPath(this.root);
    const deadline = Date.now() + 3000;
    while (fs.existsSync(sock) && Date.now() < deadline) {
      await new Promise(r => setTimeout(r, 100));
    }
  }
}
