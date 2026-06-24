#!/usr/bin/env node
import path from 'path';
import fs from 'fs';
import { CoreManager } from './core-manager.js';
import { validateScript } from './schema.js';
import { runScenario } from './runner.js';
import { replayVisual, replayReconstruct } from './replayer.js';

const HELP = `coordify-sim <command> [options]

Commands:
  simulate <script.json>   Run a JSON scenario script against Core
  replay <session-id>      Replay a past session (visual or reconstruct)

Simulate options:
  --dry-run        Validate and print steps; do not connect to Core
  --no-finalize    Skip AGENT_LEFT events at end
  --core-bin <p>   Path to coordify-core binary
  --root <dir>     Project root (default: cwd)

Replay options:
  --visual         Visual ink playback (default)
  --reconstruct    Re-submit events to live Core
  --speed <Nx>     Playback speed (0.5|1|2|4), visual only (default: 1)
  --stop-at <N>    Stop after N events, reconstruct only
  --root <dir>     Project root
`;

const argv = process.argv.slice(2);
const root = (() => {
  const i = argv.indexOf('--root');
  if (i >= 0 && argv[i + 1]) return path.resolve(argv[i + 1]);
  return process.env.COORDIFY_ROOT ? path.resolve(process.env.COORDIFY_ROOT) : process.cwd();
})();
const cmd = argv.find(a => !a.startsWith('-'));

function flag(name: string): boolean { return argv.includes(name); }
function opt(name: string): string | undefined {
  const i = argv.indexOf(name);
  return i >= 0 && argv[i + 1] ? argv[i + 1] : undefined;
}

async function main() {
  switch (cmd) {
    case 'simulate': {
      const scriptPath = argv.find(a => !a.startsWith('-') && a !== 'simulate');
      if (!scriptPath) { process.stdout.write('usage: coordify-sim simulate <script.json>\n'); process.exit(1); }
      let raw: unknown;
      try { raw = JSON.parse(fs.readFileSync(scriptPath, 'utf8')); }
      catch { process.stdout.write(`error: cannot read ${scriptPath}\n`); process.exit(1); return; }
      const result = validateScript(raw);
      if (Array.isArray(result)) {
        process.stdout.write('invalid script:\n' + result.map(e => `  ${e}`).join('\n') + '\n');
        process.exit(1); return;
      }
      const dryRun = flag('--dry-run');
      if (dryRun) {
        await runScenario({ socketPath: '', token: '', spawned: false }, result, { dryRun: true });
        return;
      }
      const binOverride = opt('--core-bin');
      const cm = new CoreManager(root, binOverride);
      const handle = await cm.ensure();
      try {
        await runScenario(handle, result, { noFinalize: flag('--no-finalize') });
      } finally {
        if (handle.spawned) await cm.stop();
      }
      break;
    }
    case 'replay': {
      const sessionId = argv.find(a => !a.startsWith('-') && a !== 'replay');
      if (!sessionId) { process.stdout.write('usage: coordify-sim replay <session-id>\n'); process.exit(1); }
      if (flag('--reconstruct')) {
        const stopAt = opt('--stop-at') ? Number(opt('--stop-at')) : undefined;
        await replayReconstruct(root, sessionId, { stopAt });
      } else {
        const speedStr = opt('--speed') ?? '1';
        const speed = parseFloat(speedStr);
        await replayVisual(root, sessionId, { speed: isNaN(speed) ? 1 : speed });
      }
      break;
    }
    case '--help':
    case undefined: process.stdout.write(HELP); break;
    default: process.stdout.write(`unknown command: ${cmd}\n\n${HELP}`);
  }
}

main().catch(e => { process.stderr.write(String(e) + '\n'); process.exit(1); });
