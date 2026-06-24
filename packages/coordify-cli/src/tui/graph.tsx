import React, { useState, useEffect } from 'react';
import { render, useApp, useInput } from 'ink';
import { Box, Text } from 'ink';
import CouplingGraph from './components/CouplingGraph.js';
import HeatMatrix from './components/HeatMatrix.js';
import { readKnowledge } from '../files.js';
import { query, isLive } from '../ipc.js';

function GraphApp({ root, mode, top }: { root: string; mode: 'coupling' | 'heat'; top: number }) {
  const { exit } = useApp();
  const [edges, setEdges] = useState<any[]>([]);
  const [heat, setHeat] = useState<any[]>([]);

  useInput((input, key) => {
    if (input === 'q' || (key.ctrl && input === 'c')) exit();
  });

  useEffect(() => {
    let alive = true;
    async function refresh() {
      while (alive) {
        const k = readKnowledge(root);
        if (mode === 'coupling') setEdges((k.coupling as any[]) ?? []);
        if (isLive(root)) {
          const resp = await query(root, 'get_state').catch(() => null);
          if (resp?.ok && alive) setHeat((resp.data as any)?.heat ?? []);
        }
        await new Promise(r => setTimeout(r, 2000));
      }
    }
    refresh();
    return () => { alive = false; };
  }, [root, mode]);

  return (
    <Box flexDirection="column">
      {mode === 'coupling' && <CouplingGraph edges={edges} top={top} />}
      {mode === 'heat' && <HeatMatrix heat={heat} />}
      <Text color="gray">[q] quit</Text>
    </Box>
  );
}

export async function renderGraph(root: string, mode: 'coupling' | 'heat', top: number): Promise<void> {
  const { waitUntilExit } = render(React.createElement(GraphApp, { root, mode, top }));
  await waitUntilExit();
}
