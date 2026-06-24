import net from 'net';
import type { CoreHandle } from './core-manager.js';
import type { ScenarioScript } from './schema.js';

class SimClient {
  private sock: net.Socket | null = null;
  private buf = '';
  private seq = 0;
  private pending = new Map<string, (r: any) => void>();
  private agentTokens = new Map<string, string>();
  private conflictTokens = new Map<string, string>(); // "conflict-1" -> core-assigned id
  private knownConflictIds = new Set<string>();
  private conflictSeq = 0;

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
      this.pending.set(id, (resp) => {
        if (!resp.ok) {
          reject(new Error(`Core rejected ${action}: ${JSON.stringify(resp)}`));
        } else {
          resolve(resp);
        }
      });
      this.sock.write(JSON.stringify({ id, token: this.masterToken, action, ...payload }) + '\n', err => {
        if (err) { this.pending.delete(id); reject(err); }
      });
    });
  }

  async registerAgent(agentId: string) {
    // Sim agents share a simulated branch so branch-proximity heat is non-zero
    // and overlapping claims can reach CONFLICT_CANDIDATE, matching how real
    // Claude Code agents on the same repo branch interact.
    const resp = await this.send('register', { meta: { agentId, branch: 'main' } });
    if (resp.agent_id) this.agentTokens.set(agentId, resp.agent_id);
    return resp;
  }

  // Core assigns its own agent ids at register time (agent-1, agent-2, ...).
  // Scenario scripts use stable human names (agent-a, ...); remap every agent
  // reference in the event to the Core-assigned id before submitting, otherwise
  // Core rejects with AGENT_NOT_FOUND.
  private remapAgents(event: Record<string, unknown>): Record<string, unknown> {
    const map = (v: unknown): unknown => {
      if (typeof v === 'string' && this.agentTokens.has(v)) return this.agentTokens.get(v);
      if (Array.isArray(v)) return v.map(map);
      if (v && typeof v === 'object') {
        const out: Record<string, unknown> = {};
        for (const [k, val] of Object.entries(v)) out[k] = map(val);
        return out;
      }
      return v;
    };
    return map(event) as Record<string, unknown>;
  }

  private remapConflicts(event: Record<string, unknown>): Record<string, unknown> {
    const map = (v: unknown): unknown => {
      if (typeof v === 'string' && this.conflictTokens.has(v)) return this.conflictTokens.get(v);
      if (Array.isArray(v)) return v.map(map);
      if (v && typeof v === 'object') {
        const out: Record<string, unknown> = {};
        for (const [k, val] of Object.entries(v)) out[k] = map(val);
        return out;
      }
      return v;
    };
    return map(event) as Record<string, unknown>;
  }

  async snapshotExistingConflicts(): Promise<void> {
    const resp = await this.send('get_state');
    const conflicts: Array<{ conflictId: string }> = resp.data?.conflicts ?? [];
    for (const c of conflicts) this.knownConflictIds.add(c.conflictId);
  }

  private async syncConflicts(): Promise<void> {
    const resp = await this.send('get_state');
    const conflicts: Array<{ conflictId: string }> = resp.data?.conflicts ?? [];
    for (const c of conflicts) {
      if (!this.knownConflictIds.has(c.conflictId)) {
        this.knownConflictIds.add(c.conflictId);
        this.conflictTokens.set(`conflict-${++this.conflictSeq}`, c.conflictId);
      }
    }
  }

  async submitEvent(event: Record<string, unknown>) {
    const remapped = this.remapConflicts(this.remapAgents(event));
    const resp = await this.send('submit_event', { capVersion: '0.1', event: remapped });
    if ((event as any).type === 'CLAIM_PROPOSED') await this.syncConflicts();
    return resp;
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
  await client.snapshotExistingConflicts();

  for (let i = 0; i < script.steps.length; i++) {
    const step = script.steps[i];
    if (step.delay_ms > 0) await new Promise(r => setTimeout(r, step.delay_ms));
    process.stdout.write(`  Step ${i + 1}/${script.steps.length}  ${(step.event as any).type ?? '?'}\n`);
    try {
      await client.submitEvent(step.event as Record<string, unknown>);
    } catch (e) {
      client.close();
      throw new Error(`Step ${i + 1} failed: ${String(e)}`);
    }
  }

  if (script.finalize && !opts.noFinalize) {
    process.stdout.write(`  Finalizing...\n`);
    // Core detects departure via socket close — no AGENT_LEFT event needed
  }
  client.close();
  process.stdout.write(`Done. Use 'coordify watch' or 'coordify stats' to inspect results.\n`);
}
