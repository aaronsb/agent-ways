// Ported from skins by hellosverre (MIT; LICENSE beside this file),
// https://github.com/hellosverre/claude-skins at 985ad63, hooks/detail-rows.tsx.

import type { RenderElement } from 'claude-code'

import { chunksOf, DETAIL_LINES, MAX_BLOCK, unifiedDiff } from './detail'
import type { Body, Field } from './detail'
import { shortenPath } from './format'
import { fileHref } from './links'
import { diffCard, terminalCard } from './rows'
import type { Look } from './rows'
import { shellRows } from './shell-rows'

// An opened tool row: what the call was asked, then what it answered, under the row the
// chevron sits on. Long answers fold behind a `show all` control of their own.

export type Opened = {
  fields: readonly Field[]
  body: Body
  // The shell command, for its card's header.
  command: string
  isErrored: boolean
  cwd: string
  columns: number
  isAll: boolean
  toggleAll: () => void
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
    const isWhole = field.value.length <= MAX_BLOCK
    const href = isWhole && field.label === 'path' && look.links === true && look.paths !== undefined ? fileHref(field.value, look.paths) : null

    return (
      <Text>
        <Text color={palette.muted}>{`${field.label}  `}</Text>
        <Text color={palette.fg}>{href !== null ? <Link href={href}>{field.value}</Link> : isWhole ? field.value : `${field.value.slice(0, MAX_BLOCK)}…`}</Text>
      </Text>
    )
  })
}

// `show all 120 lines` under a folded answer, `show less` once it is open.
function moreButton(look: Look, total: number, opened: Opened) {
  const { Button } = look.ui

  return total <= DETAIL_LINES ? (
    ''
  ) : (
    <Button key="show-all" label={opened.isAll ? 'show less' : `show all ${total} lines`} plain dimColor onPress={opened.toggleAll} />
  )
}

function textLines(look: Look, lines: readonly string[], color: string, opened: Opened) {
  const { Box, Text } = look.ui
  const shown = opened.isAll ? lines : lines.slice(0, DETAIL_LINES)

  return (
    <Box flexDirection="column">
      {shown.map(line => (
        <Text color={color} wrap="truncate-end">
          {line === '' ? ' ' : line}
        </Text>
      ))}
      {moreButton(look, lines.length, opened)}
    </Box>
  )
}

async function bodyRows(look: Look, opened: Opened): Promise<RenderElement | string> {
  const { Box, Code, Text } = look.ui
  const { palette } = look.skin
  const { body } = opened

  switch (body.kind) {
    case 'file': {
      const lines = body.text.split('\n')
      const shown = opened.isAll ? lines : lines.slice(0, DETAIL_LINES)

      return (
        <Box flexDirection="column">
          {chunksOf(shown, body.startLine).map(chunk => (
            <Code source={chunk.text} path={body.path} startLine={chunk.startLine} wrap="truncate-end" />
          ))}
          {moreButton(look, lines.length, opened)}
        </Box>
      )
    }
    case 'matches':
      return (
        <Box flexDirection="column">
          {body.note === '' ? '' : <Text color={palette.muted}>{body.note}</Text>}
          {textLines(look, body.lines, palette.fg, opened)}
        </Box>
      )
    case 'shell':
      return look.svg !== undefined
        ? terminalCard(look, look.svg, body.result, opened.isErrored, opened.columns)
        : shellRows(look, opened.command, body.result, opened.isErrored)
    case 'diff': {
      const path = shortenPath(body.diff.path, opened.cwd)

      if (look.svg !== undefined) {
        return diffCard(look, look.svg, body.diff, path, opened.columns)
      }

      const source = unifiedDiff(body.diff)

      // A diff cut mid-hunk no longer parses: one too long for a block is Claude Code's.
      return source.length > MAX_BLOCK ? opened.stock() : <Code source={source} format="diff" path={body.diff.path} wrap="truncate-end" />
    }
    case 'text':
      return textLines(look, body.text.split('\n'), body.isError ? palette.err : palette.fg, opened)
    case 'none':
      return ''
    case 'stock':
      return opened.stock()
  }
}

// What an opened row holds: the input, a line's breath, the answer.
export async function openedRows(look: Look, opened: Opened) {
  const { Box } = look.ui
  const body = await bodyRows(look, opened)

  return (
    <Box flexDirection="column" paddingLeft={2} marginBottom={1}>
      {fieldRows(look, opened.fields)}
      {body === '' ? '' : <Box marginTop={opened.fields.length === 0 ? 0 : 1} flexDirection="column">{body}</Box>}
    </Box>
  )
}
