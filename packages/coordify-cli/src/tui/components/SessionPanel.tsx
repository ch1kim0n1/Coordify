import React from 'react';
import { Box, Text } from 'ink';

interface Props { agents: number; claims: number; conflicts: number; peakHeat: number; }

export default function SessionPanel({ agents, claims, conflicts, peakHeat }: Props) {
  return (
    <Box borderStyle="single" borderColor="gray" paddingX={1} gap={3}>
      <Text bold>Session</Text>
      <Text>agents: <Text color="green">{agents}</Text></Text>
      <Text>claims: <Text color="cyan">{claims}</Text></Text>
      <Text>conflicts: <Text color={conflicts > 0 ? 'yellow' : 'gray'}>{conflicts}</Text></Text>
      <Text>peak heat: <Text color={peakHeat >= 80 ? 'red' : 'gray'}>{peakHeat}</Text></Text>
    </Box>
  );
}
