import net from 'net';
import type { CoreHandle } from './core-manager.js';
import type { ScenarioScript } from './schema.js';

class SimClient {
  private sock: net.Socket | null = null;
  private buf = '';
  private seq = 0;
  private pending = new Map<string, (r: any) => void>();
  private agentTokens = new Map<string, string>();

  constructor(private sockPath: string, private masterToken: string) {}

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
          let resp: any;
          try { resp = JSON.parse(line); } catch { continue; }
          const res = this.pending.get(resp.id);
          if (res) { this.pending.delete(resp.id); res(resp); }
        }
      });
    });
  }

  send(action: string, payload: Record<string, unknown> = {}): Promise<any> {
    return new Promise((resolve, reject) => {
      if (!this.sock) { reject(new Error('not connected')); return; }
      const id = 's' + (++this.seq);
      this.pending.set(id, resolve);
      this.sock.write(JSON.stringify({ id, token: this.masterToken, action, ...payload }) + '\n', err => {
        if (err) { this.pending.delete(id); reject(err); }
      });
    });
  }

  async registerAgent(agentId: string) {
    const resp = await this.send('register', { meta: { agentId } });
    if (resp.agent_id) this.agentTokens.set(agentId, resp.agent_id);
    return resp;
  }

  async submitEvent(event: Record<string, unknown>) {
    return this.send('submit_event', { capVersion: '0.1', event });
  }

  close() { try { this.sock?.end(); } catch (_) {} this.sock = null; }
}

export async function runScenario(
  handle: CoreHandle,
  script: ScenarioScript,
  opts: { dryRun?: boolean; noFinalize?: boolean }
): Promise<void> {
  if (opts.dryRun) {
    process.stdout.write(`[dry-run] ${script.name} — ${script.steps.length} steps\n`);
    for (let i = 0; i < script.steps.length; i++) {
      const s = script.steps[i];
      process.stdout.write(`  step ${i + 1}: ${(s.event as any).type ?? '?'} delay=${s.delay_ms}ms\n`);
    }
    return;
  }

  const client = new SimClient(handle.socketPath, handle.token);
  await client.connect();

  process.stdout.write(`Running: ${script.name}\n`);
  process.stdout.write(`  Registering agents...\n`);
  for (const agentId of script.agents) {
    await client.registerAgent(agentId);
  }

  for (let i = 0; i < script.steps.length; i++) {
    const step = script.steps[i];
    if (step.delay_ms > 0) await new Promise(r => setTimeout(r, step.delay_ms));
    process.stdout.write(`  Step ${i + 1}/${script.steps.length}  ${(step.event as any).type ?? '?'}\n`);
    await client.submitEvent(step.event as Record<string, unknown>);
  }

  if (script.finalize && !opts.noFinalize) {
    process.stdout.write(`  Finalizing...\n`);
    for (const agentId of script.agents) {
      await client.submitEvent({ type: 'AGENT_LEFT', agentId }).catch(() => {});
    }
  }

  client.close();
  process.stdout.write(`Done. Use 'coordify watch' or 'coordify stats' to inspect results.\n`);
}
