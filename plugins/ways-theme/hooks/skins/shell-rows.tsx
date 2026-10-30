// Ported from skins by hellosverre (MIT; LICENSE beside this file),
// https://github.com/hellosverre/claude-skins at 985ad63, hooks/shell-rows.tsx.

import { outputLine } from './code-rows'
import { copyControl, single } from './copy-rows'
import { onBorder } from './rows'
import type { Look } from './rows'
import { linesOf, shellStatus } from './shell'
import type { ShellLine, ShellResult } from './shell'

// The terminal's shell card, the text counterpart of the desktop's terminal card: a
// header with the command and how it ended, on the skin's surface band as a table's header
// row is, so the command stands apart from what it printed (#80); stdout, then stderr in the error colour under
// a label of its own, every line of each drawn.

export function shellRows(look: Look, command: string, result: ShellResult, isErrored: boolean) {
  const { Box, Text } = look.ui
  const { palette } = look.skin
  const lines = linesOf(result)
  const status = shellStatus(result, isErrored, look.prefs.icons === 'ascii')
  const row = (line: ShellLine) =>
    look.prefs.highlight && !line.isErr && line.text !== '' ? (
      outputLine(look, line.text)
    ) : (
      <Text color={line.isErr ? palette.err : palette.fg}>{line.text === '' ? ' ' : line.text}</Text>
    )
  const text = [result.stdout, result.stderr].filter(part => part.trim() !== '').join('\n')
  const control = text === '' ? undefined : copyControl(look, 'copy-output', single(text))
  const isEmpty = lines.stdout.length === 0 && lines.stderr.length === 0

  return onBorder(
    look,
    <Box flexDirection="column" borderStyle="round" borderColor={palette.muted}>
      <Box key="command" flexDirection="row" paddingX={1} backgroundColor={palette.surface}>
        <Box flexGrow={1} flexShrink={1}>
          <Text wrap="wrap">
            <Text color={palette.run}>$ </Text>
            <Text color={palette.fg} bold>
              {command === '' ? 'Output' : command}
            </Text>
          </Text>
        </Box>
        <Box flexShrink={0} paddingLeft={2}>
          <Text color={palette[status.tone]}>{status.text}</Text>
        </Box>
      </Box>
      <Box flexDirection="column" paddingX={1}>
        {isEmpty ? <Text color={palette.muted}>no output</Text> : ''}
        {lines.stdout.map(row)}
        {lines.stderr.length > 0 && lines.stdout.length > 0 ? <Text color={palette.muted}>stderr</Text> : ''}
        {lines.stderr.map(row)}
      </Box>
    </Box>,
    { control },
  )
}
