import React from 'react';
import { Box, Text } from 'ink';

interface Props {
  events: Record<string, unknown>[];
  currentIndex: number;
  total: number;
  speed: number;
  paused: boolean;
}

export default function ReplayFrame({ events, currentIndex, total, speed, paused }: Props) {
  const ev = events[currentIndex];
  const ts = ev ? String(ev.ts ?? '').replace('T', ' ').replace('Z', '') : '';
  return (
    <Box flexDirection="column">
      <Box borderStyle="single" borderColor="magenta" paddingX={1}>
        <Text bold color="magenta">Replay</Text>
        <Text color="gray">  {currentIndex + 1}/{total}  speed: {speed}x{paused ? '  [PAUSED]' : ''}</Text>
      </Box>
      {ev ? (
        <Box flexDirection="column" paddingX={1}>
          <Text color="cyan">[{ts}] <Text bold>{String(ev.type ?? '')}</Text></Text>
          {Object.entries(ev)
            .filter(([k]) => !['type', 'ts'].includes(k))
            .map(([k, v]) => <Text key={k} color="gray">  {k}: {JSON.stringify(v)}</Text>)
          }
        </Box>
      ) : <Text color="gray">end of replay</Text>}
      <Text color="gray">[q] quit  [space] pause  [←] -10  [→] +10  [+/-] speed</Text>
    </Box>
  );
}
