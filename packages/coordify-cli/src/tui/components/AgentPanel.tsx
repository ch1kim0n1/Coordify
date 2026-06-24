import React from 'react';
import { Box, Text } from 'ink';

interface Agent { agentId: string; state: string; claimId?: string | null; }
interface Props { agents: Agent[]; }

export default function AgentPanel({ agents }: Props) {
  return (
    <Box flexDirection="column" borderStyle="single" borderColor="gray" paddingX={1}>
      <Text bold>Agents</Text>
      {agents.length === 0
        ? <Text color="gray">no agents</Text>
        : agents.map(a => (
            <Box key={a.agentId}>
              <Text color={a.state === 'ACTIVE' ? 'green' : 'gray'} wrap="truncate-end">{String(a.agentId).padEnd(16)}</Text>
              <Text color={a.state === 'ACTIVE' ? 'green' : 'gray'}>{String(a.state).padEnd(10)}</Text>
              <Text color="gray">{a.claimId ?? '-'}</Text>
            </Box>
          ))
      }
    </Box>
  );
}
