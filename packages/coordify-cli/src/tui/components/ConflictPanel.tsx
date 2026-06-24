import React from 'react';
import { Box, Text } from 'ink';

interface Conflict { conflictId: string; agents: string[]; paths: string[]; state: string; ageMs?: number; }
interface Props { conflicts: Conflict[]; }

export default function ConflictPanel({ conflicts }: Props) {
  return (
    <Box flexDirection="column" borderStyle="single" borderColor="yellow" paddingX={1}>
      <Text bold color="yellow">Conflicts</Text>
      {conflicts.length === 0
        ? <Text color="gray">none</Text>
        : conflicts.map(c => (
            <Box key={c.conflictId} flexDirection="column">
              <Text color="yellow">{c.conflictId} <Text color="gray">({(c.agents ?? []).join(',')})</Text></Text>
              <Text color="gray">{(c.paths ?? []).slice(0, 2).join(', ')} - <Text color="yellow">{c.state}</Text>{c.ageMs ? ` ${Math.round(c.ageMs / 1000)}s` : ''}</Text>
            </Box>
          ))
      }
    </Box>
  );
}
