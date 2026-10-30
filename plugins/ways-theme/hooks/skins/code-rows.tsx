// Ported from skins by hellosverre (MIT; LICENSE beside this file),
// https://github.com/hellosverre/claude-skins at 985ad63, hooks/code-rows.tsx.

import { highlight, outputTokens, roleColors } from './highlight'
import type { Token } from './highlight'
import { FOLD_CODE, foldButton, shownCount } from './fold'
import { onBorder } from './rows'
import type { Look, Ui } from './rows'

// Code on the terminal in the skin's colours: a fenced block as a framed run of lines with
// its language on the top border, and shell output with its paths, numbers and verdicts
// picked out. The roles come from highlight.ts.

export function tokenSpans(look: Look, tokens: readonly Token[]) {
  const { Text } = look.ui
  const colors = roleColors(look.skin.palette)

  return tokens.map(token => (
    <Text color={colors[token.role]} bold={token.role === 'keyword' || token.role === 'heading'} italic={token.role === 'comment'}>
      {token.text}
    </Text>
  ))
}

// A shell output line in colour; stderr stays the error colour whole.
export function outputLine(look: Look, text: string) {
  const { Text } = look.ui

  return <Text wrap="truncate-end">{tokenSpans(look, outputTokens(text))}</Text>
}

// A fenced block: the language and a control (Copy) on the frame, then each line highlighted.
// Past FOLD_CODE lines it folds under `foldKey`.
export function codeRows(look: Look, lang: string, code: string, control?: ReturnType<Ui['Button']>, foldKey = 'fold-code') {
  const { Box, Text } = look.ui
  const { palette } = look.skin
  const lines = highlight(code.replace(/\t/g, '  ').replace(/\n$/, ''), lang)
  const shown = lines.slice(0, shownCount(look, foldKey, lines.length, FOLD_CODE))

  return (
    <Box flexDirection="column" marginY={1}>
      {onBorder(
        look,
        <Box flexDirection="column" borderStyle="round" borderColor={palette.muted} paddingX={1}>
          {shown.map(tokens => (
            <Text>{tokens.length === 0 || tokens.every(token => token.text === '') ? ' ' : tokenSpans(look, tokens)}</Text>
          ))}
        </Box>,
        { label: <Text color={palette.muted}>{` ${lang} `}</Text>, control },
      )}
      {foldButton(look, foldKey, lines.length, FOLD_CODE)}
    </Box>
  )
}

// A fence's code as a copy block (commands.ts): the language and Copy on the line above,
// then each line highlighted with no border, padding or margin beside it, so a selection
// over the lines takes the code alone. A blank line draws nothing, not a space. Past
// FOLD_CODE lines it folds under `foldKey`; Copy still takes every line.
export function codeCopyRows(look: Look, lang: string, code: string, control: ReturnType<Ui['Button']> | undefined, foldKey: string, margins: Record<string, number>) {
  const { Box, Text } = look.ui
  const { palette } = look.skin
  const lines = highlight(code.replace(/\t/g, '  '), look.prefs.highlight ? lang : '')
  const shown = lines.slice(0, shownCount(look, foldKey, lines.length, FOLD_CODE))

  return (
    <Box flexDirection="column" {...margins}>
      <Box flexDirection="row" justifyContent="space-between">
        <Text color={palette.muted}>{lang}</Text>
        {control ?? ''}
      </Box>
      {shown.map(tokens => (tokens.every(token => token.text === '') ? <Box height={1} /> : <Text>{tokenSpans(look, tokens)}</Text>))}
      {foldButton(look, foldKey, lines.length, FOLD_CODE)}
    </Box>
  )
}
