import React from 'react';
import { Box, Text } from 'ink';

interface HeatEdge { pair: string[]; heat: number; band: string; }
interface Props { heat: HeatEdge[]; }

function bandColor(band: string): string {
  if (band.includes('CONFLICT')) return 'red';
  if (band.includes('OVERLAP')) return 'yellow';
  if (band.includes('MONITOR')) return 'cyan';
  return 'gray';
}

export default function HeatPanel({ heat }: Props) {
  return (
    <Box flexDirection="column" borderStyle="single" borderColor="gray" paddingX={1}>
      <Text bold>Heat</Text>
      {heat.length === 0
        ? <Text color="gray">no heat</Text>
        : heat.slice(0, 8).map((e, i) => (
            <Box key={i}>
              <Text color={bandColor(e.band)} wrap="truncate-end">{(e.pair ?? []).join(' <> ').padEnd(30)}</Text>
              <Text color={bandColor(e.band)}>{String(e.heat).padEnd(5)}</Text>
              <Text color="gray">{e.band}</Text>
            </Box>
          ))
      }
    </Box>
  );
}
