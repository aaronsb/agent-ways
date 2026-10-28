// Ported from skins by hellosverre (MIT; LICENSE beside this file),
// https://github.com/hellosverre/claude-skins at 985ad63, hooks/shell-rows.tsx.

import { outputLine } from './code-rows'
import { copyButton, onBorder } from './rows'
import type { Look } from './rows'
import { foldLines, linesOf, shellStatus } from './shell'
import type { Folded, ShellLine, ShellResult } from './shell'

// The terminal's shell card, the text counterpart of the desktop's terminal card: a
// header with the command and how it ended, on the skin's surface band as a table's header
// row is, so the command stands apart from what it printed (#80); stdout, then stderr in the error colour under
// a label of its own, each stream folded to its head and tail.

// How many lines of each stream stay above and below the fold; null keeps every line.
export type Fold = { head: number; tail: number } | null

// The key of the output's `▾ N more` control in the drawing's folds.
export const OUTPUT_FOLD = 'fold-output'

// Lines a stream may have before the drawing folds it, at the terminal card's 8 + 4.
const FOLDS_PAST = 13

export function shellRows(look: Look, command: string, result: ShellResult, isErrored: boolean, at: { stdout: Fold; stderr: Fold }) {
  const { Box, Button, Text } = look.ui
  const { palette } = look.skin
  const lines = linesOf(result)
  const status = shellStatus(result, isErrored, look.prefs.icons === 'ascii')
  const fold = (stream: ShellLine[], at: Fold): Folded<ShellLine>[] => (at === null ? stream : foldLines(stream, at.head, at.tail))
  const folds = look.folds
  // stderr's fold control opens the same output; its key only keeps the two apart.
  const row = (line: Folded<ShellLine>, key = OUTPUT_FOLD) =>
    'fold' in line ? (
      folds === undefined ? (
        <Text color={palette.muted}>{`… ${line.fold} lines hidden`}</Text>
      ) : (
        <Button key={key} label={`${look.prefs.icons === 'ascii' ? 'v' : '▾'} ${line.fold} more`} plain dimColor onPress={() => folds.toggle(OUTPUT_FOLD)} />
      )
    ) : look.prefs.highlight && !line.isErr && line.text !== '' ? (
      outputLine(look, line.text)
    ) : (
      <Text color={line.isErr ? palette.err : palette.fg}>{line.text === '' ? ' ' : line.text}</Text>
    )
  const text = [result.stdout, result.stderr].filter(part => part.trim() !== '').join('\n')
  const control = text === '' ? undefined : copyButton(look, 'copy-output', text)
  const isEmpty = lines.stdout.length === 0 && lines.stderr.length === 0

  return onBorder(
    look,
    <Box flexDirection="column" borderStyle="round" borderColor={palette.muted}>
      <Box key="command" flexDirection="row" paddingX={1} backgroundColor={palette.surface}>
        <Box flexGrow={1} flexShrink={1}>
          <Text wrap="truncate-end">
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
        {fold(lines.stdout, at.stdout).map(line => row(line))}
        {lines.stderr.length > 0 && lines.stdout.length > 0 ? <Text color={palette.muted}>stderr</Text> : ''}
        {fold(lines.stderr, at.stderr).map(line => row(line, `${OUTPUT_FOLD}-err`))}
        {folds?.open.has(OUTPUT_FOLD) === true && Math.max(lines.stdout.length, lines.stderr.length) > FOLDS_PAST ? (
          <Button key={OUTPUT_FOLD} label={`${look.prefs.icons === 'ascii' ? '^' : '▴'} less`} plain dimColor onPress={() => folds.toggle(OUTPUT_FOLD)} />
        ) : (
          ''
        )}
      </Box>
    </Box>,
    { control },
  )
}
