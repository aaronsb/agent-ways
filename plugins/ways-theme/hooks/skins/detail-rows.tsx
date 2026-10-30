// Ported from skins by hellosverre (MIT; LICENSE beside this file),
// https://github.com/hellosverre/claude-skins at 985ad63, hooks/detail-rows.tsx.

import type { RenderElement } from 'claude-code'

import { unifiedDiff } from './detail'
import type { Body, Field } from './detail'
import { MAX_DRAWN, shortenPath } from './format'
import { fileHref } from './links'
import { diffCard, terminalCard } from './rows'
import type { Look } from './rows'
import { shellRows } from './shell-rows'

// An opened tool row: what the call was asked, then what it answered, under the row the
// chevron sits on, every line of it and each long line wrapped. The terminal prints a
// transcript row into scrollback, where no control can be pressed, so nothing folds. An
// answer this cannot draw (drawsBody) keeps Claude Code's own drawing, under the row.

export type Opened = {
  fields: readonly Field[]
  body: Body
  // The shell command, for its card's header.
  command: string
  isErrored: boolean
  cwd: string
  columns: number
  // Claude Code's own drawing of the call, for a body this cannot draw.
  stock: () => Promise<RenderElement>
}

// Cells the chevron takes at a row's right: its padding and its glyph.
export const DISCLOSE_INSET = 2

// The row with its chevron at the right; `▸` closed, `▾` open. On the terminal the chevron
// sits on the row's first line, under the rail's connector, where a wrapped target puts
// lines below it.
export function disclosedRow(look: Look, row: RenderElement, isOpen: boolean, toggle: () => void, opened?: RenderElement) {
  const { Box, Button } = look.ui
  const ascii = look.prefs.icons === 'ascii'
  const chevron = isOpen ? (ascii ? 'v' : '▾') : ascii ? '>' : '▸'
  const isTerminal = look.surface === 'terminal'
  const railed = look.prefs.rail && isTerminal

  return (
    <Box flexDirection="column">
      <Box flexDirection="row" alignItems={isTerminal ? 'flex-start' : 'flex-end'}>
        <Box flexGrow={1} flexShrink={1} flexDirection="column">
          {row}
        </Box>
        <Box flexShrink={0} paddingLeft={1} marginTop={railed ? 1 : 0}>
          <Button key="disclose" label={chevron} plain dimColor onPress={toggle} />
        </Box>
      </Box>
      {opened ?? ''}
    </Box>
  )
}

// The input, a field a row; a path field links to its file where the surface draws links.
function fieldRows(look: Look, fields: readonly Field[]) {
  const { Link, Text } = look.ui
  const { palette } = look.skin

  return fields.map(field => {
    const href = field.label === 'path' && look.links === true && look.paths !== undefined ? fileHref(field.value, look.paths) : null

    return (
      <Text>
        <Text color={palette.muted}>{`${field.label}  `}</Text>
        <Text color={palette.fg}>{href !== null ? <Link href={href}>{field.value}</Link> : field.value}</Text>
      </Text>
    )
  })
}

function textLines(look: Look, lines: readonly string[], color: string) {
  const { Box, Text } = look.ui

  return (
    <Box flexDirection="column">
      {lines.map(line => (
        <Text color={color}>{line === '' ? ' ' : line}</Text>
      ))}
    </Box>
  )
}

// Whether an answer of `text` fits one drawing.
const isDrawable = (text: string): boolean => text.length <= MAX_DRAWN

// What an opened row draws of the call's answer.
export type Answer = Omit<Opened, 'fields' | 'stock'>

// The answer's drawing, or null where the row falls back to Claude Code's own: an answer
// past one drawing (MAX_DRAWN), a card past what a card holds (MAX_SVG), a body this
// cannot read.
function answerRows(look: Look, answer: Answer): RenderElement | string | null {
  const { Box, Code, Text } = look.ui
  const { palette } = look.skin
  const { body } = answer

  switch (body.kind) {
    case 'file':
      return isDrawable(body.text) ? <Code source={body.text} path={body.path} startLine={body.startLine} /> : null
    case 'matches':
      return isDrawable(body.lines.join('\n')) ? (
        <Box flexDirection="column">
          {body.note === '' ? '' : <Text color={palette.muted}>{body.note}</Text>}
          {textLines(look, body.lines, palette.fg)}
        </Box>
      ) : null
    case 'shell':
      return look.svg !== undefined ? terminalCard(look, look.svg, body.result, answer.isErrored, answer.columns) : shellRows(look, answer.command, body.result, answer.isErrored)
    case 'diff': {
      if (look.svg !== undefined) {
        return diffCard(look, look.svg, body.diff, shortenPath(body.diff.path, answer.cwd), answer.columns)
      }

      const source = unifiedDiff(body.diff)

      return isDrawable(source) ? <Code source={source} format="diff" path={body.diff.path} /> : null
    }
    case 'text':
      return isDrawable(body.text) ? textLines(look, body.text.split('\n'), body.isError ? palette.err : palette.fg) : null
    case 'none':
      return ''
    case 'stock':
      return null
  }
}

// Whether an opened row draws the answer itself. Where it does not, Claude Code's result
// row keeps its own drawing of it.
export const drawsBody = (look: Look, answer: Answer): boolean => answerRows(look, answer) !== null

// What an opened row holds: the input, a line's breath, the answer.
export async function openedRows(look: Look, opened: Opened) {
  const { Box } = look.ui
  const body = answerRows(look, opened) ?? (await opened.stock())

  return (
    <Box flexDirection="column" paddingLeft={2} marginBottom={1}>
      {fieldRows(look, opened.fields)}
      {body === '' ? '' : <Box marginTop={opened.fields.length === 0 ? 0 : 1} flexDirection="column">{body}</Box>}
    </Box>
  )
}
