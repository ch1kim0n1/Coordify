#!/usr/bin/env node
import path from 'path';
import { runStatus } from './commands/status.js';
import { runAgents } from './commands/agents.js';
import { runHeat } from './commands/heat.js';
import { runClaims } from './commands/claims.js';
import { runConflicts } from './commands/conflicts.js';
import { runLogs } from './commands/logs.js';
import { runStats } from './commands/stats.js';
import { runSessionList, runSessionInspect } from './commands/session.js';

const HELP = `coordify <command> [options]

Commands:
  status                  Live or offline overview
  agents                  List agents and their state
  heat                    Heat edges between agent pairs
  claims                  Active claims (live only)
  conflicts               Active conflicts (live only)
  logs [--tail N] [--follow]  Print event log
  stats                   Last session statistics
  session list            List finalized sessions
  session inspect <id>    Inspect a session
  watch                   Live terminal dashboard
  graph --coupling|--heat Graph view

Options:
  --json    Output raw JSON
  --root    Project root (default: cwd)
`;

// TUI modules are compiled as ESM (tsconfig.tui.json). CJS require() cannot load them.
// We use new Function to get a real import() at runtime, invisible to tsc's static resolver.
// eslint-disable-next-line @typescript-eslint/no-implied-eval
const esmImport: (m: string) => Promise<Record<string, unknown>> = new Function('m', 'return import(m)') as never;

const argv = process.argv.slice(2);
const root = (() => {
  const i = argv.indexOf('--root');
  if (i >= 0 && argv[i + 1]) return path.resolve(argv[i + 1]);
  return process.env.COORDIFY_ROOT ? path.resolve(process.env.COORDIFY_ROOT) : process.cwd();
})();
const json = argv.includes('--json');
const cmd = argv.find(a => !a.startsWith('-'));
const rest = argv.filter(a => !a.startsWith('-') && a !== cmd);

async function main() {
  switch (cmd) {
    case 'status': await runStatus(root, { json }); break;
    case 'agents': await runAgents(root, { json }); break;
    case 'heat':   await runHeat(root, { json }); break;
    case 'claims': await runClaims(root, { json }); break;
    case 'conflicts': await runConflicts(root, { json }); break;
    case 'logs': {
      const tail = Number(argv[argv.indexOf('--tail') + 1] ?? 20);
      await runLogs(root, { json, tail, follow: argv.includes('--follow') });
      break;
    }
    case 'stats':   await runStats(root, { json }); break;
    case 'session': {
      if (rest[0] === 'list' || argv.includes('list')) await runSessionList(root, { json });
      else if (rest[0] === 'inspect' || argv.includes('inspect')) {
        const id = rest[1] ?? argv[argv.indexOf('inspect') + 1];
        if (!id) { process.stdout.write('usage: coordify session inspect <id>\n'); process.exit(1); }
        await runSessionInspect(root, id, { json });
      } else { process.stdout.write(HELP); }
      break;
    }
    case 'watch': {
      const { renderWatch } = await esmImport(require.resolve('./tui/watch.js'));
      await (renderWatch as (root: string) => Promise<void>)(root);
      break;
    }
    case 'graph': {
      const { renderGraph } = await esmImport(require.resolve('./tui/graph.js'));
      const mode = argv.includes('--heat') ? 'heat' : 'coupling';
      const top = Number(argv[argv.indexOf('--top') + 1] ?? 20);
      await (renderGraph as (root: string, mode: string, top: number) => Promise<void>)(root, mode, top);
      break;
    }
    case '--help':
    case undefined: process.stdout.write(HELP); break;
    default: process.stdout.write(`unknown command: ${cmd}\n\n${HELP}`);
  }
}

main().catch(e => { process.stderr.write(String(e) + '\n'); process.exit(1); });
