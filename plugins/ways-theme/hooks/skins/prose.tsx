// Ported from skins by hellosverre (MIT; LICENSE beside this file),
// https://github.com/hellosverre/claude-skins at 985ad63, hooks/prose.tsx.

import type { AlertType, Block, Line, Run, RunStyle, Task } from './blocks'
import { isPath } from './blocks'
import { fileHref, safeHref, touchOf } from './links'
import type { Look } from './rows'
import type { Palette } from './skin'

type Glyphs = { mark: string; done: string; open: string }

const ALERTS: Record<AlertType, { title: string; slot: keyof Palette; unicode: string; ascii: string }> = {
  note: { title: 'Note', slot: 'web', unicode: 'ℹ', ascii: 'i' },
  tip: { title: 'Tip', slot: 'ok', unicode: '✦', ascii: '*' },
  important: { title: 'Important', slot: 'mcp', unicode: '❖', ascii: '!' },
  warning: { title: 'Warning', slot: 'warn', unicode: '▲', ascii: '!' },
  caution: { title: 'Caution', slot: 'err', unicode: '✖', ascii: 'x' },
}

const glyphs = (look: Look): Glyphs =>
  look.prefs.icons === 'ascii' ? { mark: '-', done: '[x]', open: '[ ]' } : { mark: '•', done: '☑', open: '☐' }

// The slot each highlighted kind of text takes from the skin.
const SLOTS: Partial<Record<RunStyle, keyof Palette>> = {
  code: 'search',
  number: 'user',
  duration: 'user',
  version: 'mcp',
  path: 'read',
  url: 'web',
}

// The colour a path takes: created this turn (ok), edited (warn), else the path colour.
function pathSlot(look: Look, text: string): keyof Palette {
  const touch = look.touched === undefined ? undefined : touchOf(look.touched.files, text, look.touched.cwd)

  return touch === 'created' ? 'ok' : touch === 'edited' ? 'warn' : 'read'
}

// Where a run links to, on a surface that draws links: a URL the engine's `Link` takes, and
// a path, or a code span that is one whole path, as its file. Null leaves the run text.
function hrefOf(look: Look, run: Run): string | null {
  if (look.links !== true) {
    return null
  }

  if (run.style === 'url') {
    return safeHref(run.text)
  }

  const isFile = run.style === 'path' || (run.style === 'code' && isPath(run.text))

  return isFile && look.paths !== undefined ? fileHref(run.text, look.paths) : null
}

// A linked run is drawn as it would be unlinked; a linked URL is underlined besides.
function runText(look: Look, run: Run) {
  const { Link, Text } = look.ui
  const { palette } = look.skin
  const slot = run.style === 'path' ? pathSlot(look, run.text) : SLOTS[run.style]
  const href = hrefOf(look, run)

  return (
    <Text
      color={slot === undefined ? palette.fg : palette[slot]}
      bold={run.style === 'bold'}
      italic={run.style === 'italic'}
      strikethrough={run.style === 'strike'}
      {...(run.style === 'url' && href !== null ? { underline: true } : {})}
      {...(run.style === 'code' ? { backgroundColor: palette.surface } : {})}
    >
      {href === null ? run.text : <Link href={href}>{run.text}</Link>}
    </Text>
  )
}

function lineRow(look: Look, line: Line) {
  const { Box, Text } = look.ui
  const body = <Text>{line.runs.map(run => runText(look, run))}</Text>

  if (line.bullet === null) {
    return body
  }

  const mark = line.bullet === '•' ? glyphs(look).mark : line.bullet

  return (
    <Box flexDirection="row" paddingLeft={line.depth * 2}>
      <Text color={look.skin.palette.muted}>{`${mark} `}</Text>
      {body}
    </Box>
  )
}

function headingRow(look: Look, level: number, text: string) {
  const { Text } = look.ui
  const { palette } = look.skin

  return (
    <Text color={level <= 2 ? palette.user : palette.fg} bold underline={level === 1}>
      {level <= 2 ? `${look.prefs.icons === 'ascii' ? '#' : '▍'} ${text}` : text}
    </Text>
  )
}

function alertBox(look: Look, type: AlertType, body: string) {
  const { Box, Text, Markdown } = look.ui
  const alert = ALERTS[type]
  const color = look.skin.palette[alert.slot]

  return (
    <Box flexDirection="column" borderStyle="round" borderColor={color} paddingX={1}>
      <Text color={color} bold>{`${look.prefs.icons === 'ascii' ? alert.ascii : alert.unicode} ${alert.title}`}</Text>
      {body === '' ? null : <Markdown text={body} />}
    </Box>
  )
}

function taskRows(look: Look, items: readonly Task[]) {
  const { Box, Text } = look.ui
  const { palette } = look.skin
  const mark = glyphs(look)
  const done = items.filter(item => item.done).length

  return (
    <Box flexDirection="column">
      {items.map(item => (
        <Box flexDirection="row" paddingLeft={item.depth * 2}>
          <Text color={item.done ? palette.ok : palette.muted}>{`${item.done ? mark.done : mark.open} `}</Text>
          <Text color={item.done ? palette.muted : palette.fg}>{item.text}</Text>
        </Box>
      ))}
      <Text color={palette.muted}>{`${done}/${items.length} done`}</Text>
    </Box>
  )
}

// One row per block; paragraphs and headings sit a line apart, as markdown would set them.
export function blockRows(look: Look, blocks: readonly Block[]) {
  const { Box, Markdown } = look.ui

  return blocks.map((block, i) => {
    const gap = i === 0 ? 0 : 1

    switch (block.kind) {
      case 'markdown':
        return <Markdown text={block.text} />
      case 'heading':
        return <Box marginTop={gap}>{headingRow(look, block.level, block.text)}</Box>
      case 'alert':
        return <Box marginTop={gap}>{alertBox(look, block.type, block.body)}</Box>
      case 'tasks':
        return <Box marginTop={gap}>{taskRows(look, block.items)}</Box>
      case 'paragraph':
        return (
          <Box flexDirection="column" marginTop={blocks[i - 1]?.kind === 'heading' ? 0 : gap}>
            {block.lines.map(line => lineRow(look, line))}
          </Box>
        )
    }
  })
}
