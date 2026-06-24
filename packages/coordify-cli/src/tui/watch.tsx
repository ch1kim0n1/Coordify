import React, { useState, useEffect } from 'react';
import { render, useApp, useInput, Box, Text } from 'ink';
import AgentPanel from './components/AgentPanel.js';
import HeatPanel from './components/HeatPanel.js';
import ConflictPanel from './components/ConflictPanel.js';
import SessionPanel from './components/SessionPanel.js';
import { query, isLive } from '../ipc.js';
import { latestSession, readStats } from '../files.js';

interface State { agents: any[]; claims: any[]; heat: any[]; conflicts: any[]; error?: string; }

function WatchApp({ root }: { root: string }) {
  const { exit } = useApp();
  const [state, setState] = useState<State>({ agents: [], claims: [], heat: [], conflicts: [] });

  useInput((input, key) => {
    if (input === 'q' || (key.ctrl && input === 'c')) exit();
  });

  useEffect(() => {
    let alive = true;
    async function poll() {
      while (alive) {
        if (isLive(root)) {
          const resp = await query(root, 'get_state').catch(() => null);
          if (resp?.ok && alive) {
            const d = resp.data as any;
            setState({
              agents: d.agents ?? [],
              claims: d.claims ?? [],
              heat: (d.heat ?? []).sort((a: any, b: any) => b.heat - a.heat),
              conflicts: d.conflicts ?? [],
            });
          }
        } else {
          const id = latestSession(root);
          const _stats = id ? readStats(root, id) as any : null;
          if (alive) {
            setState({ agents: [], claims: [], heat: [], conflicts: [], error: `offline${id ? ` (last: ${id})` : ''}` });
          }
        }
        await new Promise(r => setTimeout(r, 500));
      }
    }
    poll();
    return () => { alive = false; };
  }, [root]);

  const peakHeat = Math.max(0, ...state.heat.map((h: any) => h.heat ?? 0));

  return (
    <Box flexDirection="column" width="100%">
      {state.error && <Text color="yellow">{state.error}</Text>}
      <Box gap={1}>
        <Box flexDirection="column" flexGrow={1}>
          <AgentPanel agents={state.agents} />
          <HeatPanel heat={state.heat} />
        </Box>
        <Box flexDirection="column" flexGrow={1}>
          <ConflictPanel conflicts={state.conflicts} />
          <SessionPanel
            agents={state.agents.length}
            claims={state.claims.length}
            conflicts={state.conflicts.length}
            peakHeat={peakHeat}
          />
        </Box>
      </Box>
      <Text color="gray">[q] quit</Text>
    </Box>
  );
}

export async function renderWatch(root: string): Promise<void> {
  const { waitUntilExit } = render(React.createElement(WatchApp, { root }));
  await waitUntilExit();
}
