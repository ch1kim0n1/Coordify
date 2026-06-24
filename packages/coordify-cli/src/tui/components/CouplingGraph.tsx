import React from 'react';
import { Box, Text } from 'ink';

interface Edge { a: string; b: string; count: number; }
interface Props { edges: Edge[]; top: number; }

export default function CouplingGraph({ edges, top }: Props) {
  const sorted = [...edges].sort((x, y) => y.count - x.count).slice(0, top);
  return (
    <Box flexDirection="column" borderStyle="single" borderColor="blue" paddingX={1}>
      <Text bold color="blue">Coupling Graph (top {top})</Text>
      {sorted.length === 0
        ? <Text color="gray">no coupling data</Text>
        : sorted.map((e, i) => (
            <Box key={i}>
              <Text color="cyan" wrap="truncate-end">{e.a}</Text>
              <Text color="gray"> ↔ </Text>
              <Text color="cyan" wrap="truncate-end">{e.b}</Text>
              <Text color="blue">  count: {e.count}</Text>
            </Box>
          ))
      }
    </Box>
  );
}
