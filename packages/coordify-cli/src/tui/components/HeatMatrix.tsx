import React from 'react';
import { Box, Text } from 'ink';

interface HeatEdge { pair: string[]; heat: number; band: string; }
interface Props { heat: HeatEdge[]; }

function cell(heat: number, band: string): { label: string; color: string } {
  if (band.includes('CONFLICT')) return { label: String(heat).padStart(4), color: 'red' };
  if (band.includes('OVERLAP')) return { label: String(heat).padStart(4), color: 'yellow' };
  if (band.includes('MONITOR')) return { label: String(heat).padStart(4), color: 'cyan' };
  return { label: String(heat).padStart(4), color: 'gray' };
}

export default function HeatMatrix({ heat }: Props) {
  const agents = [...new Set(heat.flatMap(e => e.pair ?? []))].sort();
  const lookup = new Map(heat.map(e => [(e.pair ?? []).join('↔'), e]));
  const getEdge = (a: string, b: string) => lookup.get(`${a}↔${b}`) ?? lookup.get(`${b}↔${a}`);

  return (
    <Box flexDirection="column" borderStyle="single" borderColor="red" paddingX={1}>
      <Text bold color="red">Heat Matrix</Text>
      {agents.length === 0
        ? <Text color="gray">no heat data</Text>
        : (
          <>
            <Box>
              <Text color="gray">{''.padEnd(12)}</Text>
              {agents.map(a => <Text key={a} color="gray">{String(a).slice(0, 8).padStart(9)}</Text>)}
            </Box>
            {agents.map(row => (
              <Box key={row}>
                <Text color="gray">{String(row).slice(0, 10).padEnd(12)}</Text>
                {agents.map(col => {
                  if (row === col) return <Text key={col} color="gray">{'  --'.padStart(9)}</Text>;
                  const e = getEdge(row, col);
                  const { label, color } = e ? cell(e.heat, e.band) : { label: '   0', color: 'gray' };
                  return <Text key={col} color={color as any}>{label.padStart(9)}</Text>;
                })}
              </Box>
            ))}
          </>
        )
      }
    </Box>
  );
}
