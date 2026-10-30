// Ported from skins by hellosverre (MIT; LICENSE beside this file),
// https://github.com/hellosverre/claude-skins at 985ad63, hooks/rows.tsx.

import type { ElementTable, RenderSurface } from 'claude-code'

import type { Prefs, Touch, TurnStats } from '../../types'
import { formatDuration, formatMs, MAX_DRAWN } from './format'
import type { PathBase } from './links'
import { columnWidths, graphemes, isShell, tableArt, tableCsv, widthOf, wrapLines } from './markdown'
import { diffFenceTag, diffRows, fenceDiff, newOnly } from './diff-fence'
import type { Code, Segment, Table } from './markdown'
import type { Copy } from './commands'
import { splitBlocks } from './blocks'
import { artRows, chartRows } from './chart-rows'
import { chartArt } from './chart-art'
import { chartHeading, parseMermaid } from './mermaid'
import { mermaidArt } from './mermaid-art'
import type { Art } from './mermaid-art'
import type { Chart } from './mermaid'
import { blockRows } from './prose'
import type { Icons, Kind, Skin } from './skin'
import { spinnerColor, spinnerIcon, toolIcon } from './icons'
import type { SpinnerMode } from './icons'
import { chartSvg } from './svg-chart'
import { codeSvg } from './svg-code'
import { diffSvg } from './svg-diff'
import type { DiffInput } from './svg-diff'
import { cardWidth, settled } from './svg-kit'
import { tableSvg } from './svg-table'
import { terminalSvg } from './svg-terminal'
import type { ShellOutput } from './svg-terminal'
import { metersKey, usageLine, usageSvg } from './svg-usage'
import { codeCopyRows, codeRows } from './code-rows'
import { languageOf } from './highlight'
import type { Meter } from './svg-usage'
import { kindOf, toolLabel } from './tools'
import { inlineMath, mathArt, parseTex } from './math'
import { mathSvg } from './svg-math'
import { labelOf } from './wrappers'
import type { Part } from './wrappers'

export type Ui = Pick<ElementTable, 'Box' | 'Text' | 'Markdown' | 'Button' | 'Code' | 'Link'>

// The vector element, on the surfaces that have one (the desktop app).
export type SvgElement = ElementTable<'desktop'>['Svg']

export type Look = {
  ui: Ui
  skin: Skin
  icons: Icons
  prefs: Prefs
  surface: RenderSurface
  // The vector element where the surface draws one: icons replace glyphs there.
  svg?: SvgElement
  // Puts text on the clipboard of the surface drawing; absent where nothing can copy.
  copy?: (text: string) => void
  // True the first time this drawing shows the card by that key; a card drawn again is
  // drawn settled. Absent, every card animates.
  isFirstDraw?: (key: string, entranceMs: number) => boolean
  // Bare URLs and paths drawn as links: the terminal's OSC 8, the desktop's anchors. Absent, text.
  links?: boolean
  // Where a path links from: the session's folder, and home for `~/`. Absent, paths stay text.
  paths?: PathBase
  // The files this turn created or edited, by absolute path, and the folder paths are under.
  touched?: { files: Readonly<Record<string, Touch>>; cwd: string }
}

export type Call = {
  tool: string
  input: unknown
  isRunning: boolean
  isErrored: boolean
  isInterrupted: boolean
}

// What a tool row shows at its right edge.
export type Meta = { ms?: number; added?: number; removed?: number }

const KIND_LABEL: Readonly<Record<Kind, string>> = {
  read: 'Read',
  write: 'Edit',
  run: 'Run',
  search: 'Search',
  web: 'Web',
  mcp: 'MCP',
  other: 'Other',
}

const CELL_PAD = 1

// The worst thing that happened to any of the calls, else how far along they are.
function status({ skin, icons }: Look, calls: readonly Call[]) {
  if (calls.some(call => call.isInterrupted)) {
    return { glyph: icons.interrupted, color: skin.palette.warn }
  }

  if (calls.some(call => call.isErrored)) {
    return { glyph: icons.err, color: skin.palette.err }
  }

  if (calls.some(call => call.isRunning)) {
    return { glyph: icons.running, color: skin.palette.muted }
  }

  return { glyph: icons.ok, color: skin.palette.ok }
}

// The connector above a node, which ties a turn's calls into one line down the left.
function railLine(look: Look) {
  const { Text } = look.ui

  return <Text color={look.skin.palette.user}>{look.icons.rail}</Text>
}

const branchOf = (look: Look): string => (look.prefs.rail && look.surface === 'terminal' ? look.icons.branch : '')

function node(look: Look, calls: readonly Call[]) {
  const { Text } = look.ui
  const { glyph, color } = status(look, calls)

  return (
    <Text color={color}>
      {`${glyph}`}
      <Text color={look.skin.palette.muted}>{`${branchOf(look)} `}</Text>
    </Text>
  )
}

// Cells the node takes ahead of the label.
const nodeWidth = (look: Look, calls: readonly Call[]): number => widthOf(`${status(look, calls).glyph}${branchOf(look)} `)

function metaParts({ skin: { palette } }: Look, meta: Meta): { text: string; color: string }[] {
  const parts = []

  if (meta.added !== undefined && meta.removed !== undefined && meta.added + meta.removed > 0) {
    parts.push({ text: `+${meta.added}`, color: palette.ok }, { text: ` −${meta.removed}`, color: palette.err })
  }

  if (meta.ms !== undefined) {
    parts.push({ text: `${parts.length > 0 ? '  ' : ''}${formatMs(meta.ms)}`, color: palette.muted })
  }

  return parts
}

function metaSpans(look: Look, meta: Meta) {
  const { Text } = look.ui

  return metaParts(look, meta).map(part => <Text color={part.color}>{part.text}</Text>)
}

// Cells the facts take at the line's right, with the gap withMeta leaves before them.
function metaWidth(look: Look, meta: Meta): number {
  const text = metaParts(look, meta)
    .map(part => part.text)
    .join('')

  return text === '' ? 0 : 2 + widthOf(text)
}

// A line with its facts flush right on the terminal and trailing it on the desktop,
// which lays a row out its own way and wraps it.
function withMeta(look: Look, main: ReturnType<Ui['Text']>, meta: Meta) {
  const { Box, Text } = look.ui
  const spans = metaSpans(look, meta)

  if (spans.length === 0) {
    return main
  }

  if (look.surface !== 'terminal') {
    return (
      <Text wrap="wrap">
        {main}
        {'   '}
        {spans}
      </Text>
    )
  }

  return (
    <Box flexDirection="row">
      <Box flexGrow={1} flexShrink={1}>
        {main}
      </Box>
      <Box flexShrink={0} paddingLeft={2}>
        <Text>{spans}</Text>
      </Box>
    </Box>
  )
}

function stack(look: Look, line: ReturnType<Ui['Text']>) {
  const { Box } = look.ui

  // Box-drawing rails need a monospace font, which only the terminal draws mod text in.
  return look.prefs.rail && look.surface === 'terminal' ? (
    <Box flexDirection="column">
      {railLine(look)}
      {line}
    </Box>
  ) : (
    line
  )
}

// On a surface with vector icons, the icon leads the row in place of the status glyph.
function iconRow(look: Look, Svg: SvgElement, kind: Kind, calls: readonly Call[], line: ReturnType<Ui['Text']>) {
  const { Box } = look.ui
  const { color } = status(look, calls)
  // Calm keeps a running icon still.
  const source = toolIcon(kind, color, look.prefs.calm === null && calls.some(call => call.isRunning))

  return (
    <Box flexDirection="row" columnGap={1} alignItems="center">
      <Svg source={source} alt={kind} width={16} height={16} />
      {line}
    </Box>
  )
}

// `path` in lines of at most `width` cells, every character kept: a line ends after a `/`,
// and inside a segment only where the segment alone is wider than the line.
export function pathLines(path: string, width: number): string[] {
  const room = Math.max(1, width)
  const lines: string[] = []
  let line = ''
  let used = 0
  const push = () => {
    lines.push(line)
    line = ''
    used = 0
  }

  for (const segment of path.split(/(?<=\/)/)) {
    const cells = widthOf(segment)

    if (used > 0 && used + cells > room) push()

    if (cells <= room - used) {
      line += segment
      used += cells
      continue
    }

    for (const grapheme of graphemes(segment)) {
      const w = widthOf(grapheme)

      if (used > 0 && used + w > room) push()
      line += grapheme
      used += w
    }
  }

  if (line !== '') push()

  return lines
}

// `touch` colours the target of a file this turn created (ok) or edited (warn); `href`, the
// file's link where the surface draws one, makes the target a link drawn the same.
// `room` is the cells a file's row has on the terminal. Given, under `paths: wrap`, a
// target that does not fit beside the label and the facts goes on lines of its own under
// the label, each piece in the target's colour and linked to the same file; under `paths:
// truncate` it stays on the line, cut with an ellipsis. Every other target wraps.
export function toolRow(look: Look, call: Call, kind: Kind, target: string, meta: Meta, label = toolLabel(call.tool), touch?: Touch, href?: string | null, room?: number) {
  const { Box, Link, Text } = look.ui
  const { palette } = look.skin
  const targetColor = call.isErrored ? palette.err : touch === 'created' ? palette.ok : touch === 'edited' ? palette.warn : palette.muted
  const indent = nodeWidth(look, [call])

  if (
    look.svg === undefined &&
    look.prefs.paths === 'wrap' &&
    room !== undefined &&
    indent + widthOf(`${label}  ${target}`) + metaWidth(look, meta) > room
  ) {
    const head = (
      <Text wrap="truncate-end">
        {node(look, [call])}
        <Text color={palette[kind]}>{label}</Text>
      </Text>
    )

    return stack(
      look,
      <Box flexDirection="column">
        {withMeta(look, head, meta)}
        <Box flexDirection="column" paddingLeft={indent}>
          {pathLines(target, room - indent).map(piece => (
            <Text color={targetColor}>{href === undefined || href === null ? piece : <Link href={href}>{piece}</Link>}</Text>
          ))}
        </Box>
      </Box>,
    )
  }

  const targetText =
    href === undefined || href === null ? (
      <Text color={targetColor}>{`  ${target}`}</Text>
    ) : (
      <Text color={targetColor}>
        {'  '}
        <Link href={href}>{target}</Link>
      </Text>
    )

  if (look.svg !== undefined) {
    const line = (
      <Text wrap="wrap">
        <Text color={palette[kind]} bold>
          {label}
        </Text>
        {targetText}
      </Text>
    )

    return iconRow(look, look.svg, kind, [call], withMeta(look, line, meta))
  }

  const main = (
    <Text wrap={room === undefined ? 'wrap' : 'truncate-end'}>
      {node(look, [call])}
      <Text color={palette[kind]}>{label}</Text>
      {targetText}
    </Text>
  )

  return stack(look, withMeta(look, main, meta))
}

// A quiet call's result: nothing when it worked, the one line that says why when it failed.
export function quietResult(look: Look, failure: string | null) {
  const { Box, Text } = look.ui

  return failure === null ? (
    <Box />
  ) : (
    <Text color={look.skin.palette.err}>{`${look.prefs.icons === 'ascii' ? 'x' : '✖'} ${failure}`}</Text>
  )
}

// A run of reads and searches on one node: `●─ Read 3 · Search 2 ▸`. The `▸` is plain text,
// not a button: a click anywhere on the line is Claude Code's, and opens the group.
export function groupRow(look: Look, calls: readonly Call[]) {
  const { Text } = look.ui
  const { palette } = look.skin
  const counts = new Map<Kind, number>()

  for (const call of calls) {
    const kind = kindOf(call.tool) ?? 'other'
    counts.set(kind, (counts.get(kind) ?? 0) + 1)
  }

  const parts = [...counts].flatMap(([kind, count], i) => [
    ...(i === 0 ? [] : [<Text color={palette.muted}>{' · '}</Text>]),
    <Text color={palette[kind]}>{KIND_LABEL[kind]}</Text>,
    <Text color={palette.muted}>{` ${count}`}</Text>,
  ])

  parts.push(<Text color={palette.muted} dimColor>{look.prefs.icons === 'ascii' ? '  >' : '  ▸'}</Text>)

  if (look.svg !== undefined) {
    const lead = [...counts].sort((a, b) => b[1] - a[1])[0]?.[0] ?? 'other'

    return iconRow(look, look.svg, lead, calls, <Text wrap="truncate-end">{parts}</Text>)
  }

  return stack(
    look,
    <Text wrap="truncate-end">
      {node(look, calls)}
      {parts}
    </Text>,
  )
}

// The desktop's spinner row: an animated icon for what the turn is doing, beside the
// step the desktop names (`Creating notes.md`).
export function desktopSpinnerRow(look: Look, Svg: SvgElement, mode: SpinnerMode, text: string) {
  const { Box, Text } = look.ui

  return (
    <Box flexDirection="row" columnGap={1} alignItems="center">
      <Svg source={spinnerIcon(mode, spinnerColor(mode, look.skin.palette))} alt={mode} width={20} height={20} />
      <Text color={look.skin.palette.muted}>{text}</Text>
    </Box>
  )
}

// Markdown reads a prompt's marks as formatting, so each ASCII mark is escaped to show as
// typed and each line break becomes a hard break. Leading spaces become no-break spaces,
// which indent the same without turning the line into a code block. Brackets and
// parentheses are character references instead: escaped, `\(` and `\[` open math in the
// desktop app's Markdown, whether or not `/skin math` is on.
const REFERENCES: Readonly<Record<string, string>> = { '(': '&#40;', ')': '&#41;', '[': '&#91;', ']': '&#93;' }

export const literal = (text: string): string =>
  text
    .replace(/\r\n?/g, '\n')
    .split('\n')
    .map(line => line.replace(/[!-\/:-@\[-`{-~]/g, mark => REFERENCES[mark] ?? `\\${mark}`).replace(/^ +/, spaces => ' '.repeat(spaces.length)))
    .join('  \n')

// Your message in a rounded outline sized to what you typed, so it stands apart from
// replies without taking the full width. Images it carried follow below it, drawn by
// Claude Code. `/skin prompt right` puts it on the right, where the desktop app draws it,
// and `/skin prompt plain` drops the outline for the skin's prompt marker.
// On the desktop app the text is the app's own Markdown, which can be selected and copied;
// a prompt past one drawing keeps the Text.
export function promptRow(look: Look, text: string, images?: ReturnType<Ui['Text']>) {
  const { Box, Markdown, Text } = look.ui
  const markdown = look.surface === 'terminal' ? null : literal(text)
  const plain = look.prefs.prompt === 'plain'

  return (
    <Box flexDirection="column" alignItems={look.prefs.prompt === 'right' ? 'flex-end' : 'flex-start'} marginY={1}>
      <Box {...(plain ? {} : { borderStyle: 'round' as const, borderColor: look.skin.palette.muted, paddingX: 1 })} flexShrink={1}>
        {plain ? <Text color={look.skin.palette.user}>{`${look.icons.prompt} `}</Text> : ''}
        {markdown === null || markdown.length > MAX_DRAWN ? (
          <Text color={look.skin.palette.fg}>{text}</Text>
        ) : (
          <Markdown text={markdown} />
        )}
      </Box>
      {images ?? ''}
    </Box>
  )
}

// A message another party relayed, drawn in full: each wrapper block's tags give way to
// its label in muted, the attributes its opening tag carried beside it, and its body is
// indented under the label. Text around the blocks draws plain. A block inside a block
// draws the same way, to the depth the split went (wrappers.ts).
export function relayedRows(look: Look, all: readonly Part[]): ReturnType<Ui['Box']> {
  const { Box, Text } = look.ui
  const { palette } = look.skin

  return (
    <Box flexDirection="column">
      {all.map(part =>
        part.kind === 'text' ? (
          <Text color={palette.fg}>{part.text === '' ? ' ' : part.text}</Text>
        ) : (
          <Box flexDirection="column">
            <Text color={palette.muted}>{part.attributes === '' ? labelOf(part.name) : `${labelOf(part.name)}  ${part.attributes}`}</Text>
            {part.body === '' ? '' : <Box flexDirection="column" paddingLeft={2}>{relayedRows(look, part.inner)}</Box>}
          </Box>
        ),
      )}
    </Box>
  )
}

const JUSTIFY = { left: 'flex-start', right: 'flex-end', center: 'center' } as const

// The share of the reported width a table may take at its natural size. Past it, the
// table spans its container and splits it between columns in proportion: the desktop
// app reports the window's width, wider than the transcript column a reply sits in.
const NATURAL_SHARE = 0.55

// A grid of cells, so columns line up in the terminal's monospace and in the desktop
// app's proportional font alike. `control`, such as a Copy button, sits on the frame's
// top border at the right, as ╭──── Copy ─╮.
// Every row is drawn. Long cells wrap within their column in full: unlike the desktop card, the terminal has no tooltip to read a cut cell by (#59).
// `maxWidth` is the room the table is drawn in, not the terminal's width: in a frame it
// is the frame's inside.
//
// On the terminal every column is drawn exactly as wide as its text is wrapped to, so a
// line that fills its column is drawn whole and a row is as tall as its tallest cell's
// lines. Elsewhere the font is proportional, so the cells' lines wrap again where the
// surface needs them to, and a wide table splits its container between columns; so does
// a terminal table of more columns than its room holds at one cell each.
export function tableRows(look: Look, table: Table, maxWidth: number, control?: ReturnType<Ui['Button']>) {
  const { Box, Text } = look.ui
  const { palette } = look.skin
  const widths = columnWidths(table, maxWidth - 4, CELL_PAD * 2)
  const natural = widths.reduce((sum, width) => sum + width + CELL_PAD * 2, 2)
  const isTerminal = look.surface === 'terminal'
  // On the terminal only a table wider than the room even at one cell a column goes fluid.
  const isFluid = isTerminal ? natural > maxWidth : natural > maxWidth * NATURAL_SHARE
  // A fluid column grows from nothing, padding included, so its share counts the padding
  // too: by text width alone a narrow column gets less than it needs, or no room at all.
  const size = (i: number) =>
    isFluid
      ? { width: 0, flexGrow: (widths[i] ?? 0) + CELL_PAD * 2, flexShrink: 1 }
      : { width: (widths[i] ?? 0) + CELL_PAD * 2, flexShrink: 0 }
  const row = (cells: readonly string[], isHeader: boolean, band: string | undefined) => (
    <Box flexDirection="row" {...(band === undefined ? {} : { backgroundColor: band })}>
      {cells.map((cell, i) => (
        <Box {...size(i)} flexDirection="column" paddingX={CELL_PAD} alignItems={JUSTIFY[table.align[i] ?? 'left']}>
          {wrapLines(cell, widths[i] ?? 0, Infinity).map(line => (
            <Text color={palette.fg} bold={isHeader}>
              {line}
            </Text>
          ))}
        </Box>
      ))}
    </Box>
  )

  return onBorder(
    look,
    <Box flexDirection="column" borderStyle="round" borderColor={palette.muted}>
      {row(table.header, true, palette.surface)}
      {table.rows.map((cells, i) => row(cells, false, i % 2 === 1 ? palette.zebra : undefined))}
    </Box>,
    { control, box: { marginY: 1, ...(isFluid ? { width: '100%' } : { alignSelf: 'flex-start' }) } },
  )
}

// A card's source: animated on its first draw, settled on every draw after it.
const sourceOf = (look: Look, key: string, built: { source: string; entranceMs: number }): string =>
  look.isFirstDraw === undefined || look.isFirstDraw(key, built.entranceMs) ? built.source : settled(built.source)

// A card drawn as an image, with its Copy button laid over the top-right corner the card
// left free. The image cannot be pressed, so the button is a real one on top of it; the
// box hugs the image so the corner is the card's, not the column's. The image takes the
// markup's own size, so one drawn wider than the reply column is scaled down to fit it.
function cardWithCopy(look: Look, Svg: SvgElement, built: { source: string; alt: string; entranceMs: number }, key: string, text: string) {
  const { Box, Button } = look.ui
  const copy = look.copy

  return (
    <Box marginY={1} alignSelf="flex-start">
      <Svg source={sourceOf(look, key, built)} alt={built.alt} />
      {copy === undefined ? (
        ''
      ) : (
        <Box position="absolute" top={1} right={3}>
          <Button key={key} label="Copy" plain dimColor onPress={() => copy(text)} />
        </Box>
      )}
    </Box>
  )
}

// Copy takes the markdown; `as text` under the card takes the table as drawn.
function tableCard(look: Look, table: Table, Svg: SvgElement, columns: number, key: string) {
  const { Box } = look.ui
  const card = tableSvg(table, look.skin.palette, cardWidth(columns), look.copy !== undefined)

  return (
    <Box flexDirection="column">
      {cardWithCopy(look, Svg, card, key, copyText(look, table))}
      {copyRow(look, `${key}-art`, tableArt(table), 'Copy as text')}
    </Box>
  )
}

// The table's two copies on its frame: Copy (`copyText`), and the table as drawn.
function tableControls(look: Look, key: string, table: Table) {
  const { Box } = look.ui
  const whole = copyButton(look, key, copyText(look, table))
  const art = copyButton(look, `${key}-art`, tableArt(table), ' as text ')

  return whole === undefined || art === undefined ? undefined : (
    <Box flexDirection="row">
      {whole}
      {art}
    </Box>
  )
}

function mathCard(look: Look, tex: string, Svg: SvgElement, columns: number, key: string) {
  const card = mathSvg(tex, look.skin.palette, cardWidth(columns), look.copy !== undefined)

  return cardWithCopy(look, Svg, card, key, tex.trim())
}

// A formula in cells: stacked fractions, radicals and tall brackets, one Text per row.
function mathRows(look: Look, tex: string, maxWidth: number, key: string) {
  const { Box, Text } = look.ui
  const { palette } = look.skin

  return (
    <Box flexDirection="column" marginY={1}>
      <Text color={palette.muted} bold>
        MATH
      </Text>
      <Box flexDirection="column" marginTop={1} paddingLeft={2}>
        {mathArt(parseTex(tex), Math.max(10, maxWidth - 2)).map(row => (
          <Text color={palette.fg}>{row === '' ? ' ' : row}</Text>
        ))}
      </Box>
      {copyRow(look, key, tex.trim())}
    </Box>
  )
}

// A table as markdown again, for one that has no source of its own.
const tableMarkdown = (table: Table): string =>
  [table.header, table.header.map(() => '---'), ...table.rows].map(cells => `| ${cells.join(' | ')} |`).join('\n')

// What a block's Copy takes, after `copy_as`. As content: a suggestion as typed, a fence's
// code, a table as CSV, a span's text. As markdown: a suggestion as typed, a fence, a table
// and a span as written. The `as text`, `new only` and `copy reply` controls take what they
// always have.
export function copyText(look: Look, block: Copy | Code | Table): string {
  const isMarkdown = look.prefs.copyAs === 'markdown'

  if (block.kind === 'table') {
    return isMarkdown ? (block.raw ?? tableMarkdown(block)) : tableCsv(block)
  }

  if (block.kind === 'code') {
    return isMarkdown ? block.raw : block.code
  }

  return isMarkdown && (block.form === 'code' || block.form === 'span') ? block.raw : block.text
}

// A frame with a label and a control on its top border. They are laid over it from a plain
// Box around the frame, not from inside it: inside a bordered Box, Windows Terminal drew the
// border over them (#54). `box` sizes the plain Box the way the frame should sit.
export function onBorder(
  look: Look,
  frame: ReturnType<Ui['Box']>,
  parts: { label?: ReturnType<Ui['Text']>; control?: ReturnType<Ui['Button']> | ReturnType<Ui['Box']>; box?: Record<string, unknown> },
) {
  const { Box } = look.ui

  return (
    <Box flexDirection="column" {...parts.box}>
      {frame}
      {parts.label === undefined ? (
        ''
      ) : (
        <Box position="absolute" top={0} left={2}>
          {parts.label}
        </Box>
      )}
      {parts.control === undefined ? (
        ''
      ) : (
        <Box position="absolute" top={0} right={2}>
          {parts.control}
        </Box>
      )}
    </Box>
  )
}

// A Copy button padded to sit on a border line; nothing where nothing can copy.
export function copyButton(look: Look, key: string, text: string, label = ' Copy ') {
  const { Button } = look.ui
  const copy = look.copy

  return copy === undefined ? undefined : <Button key={key} label={label} plain dimColor onPress={() => copy(text)} />
}

// A small Copy button under a card or block, flush right; nothing where nothing can copy.
export function copyRow(look: Look, key: string, text: string, label = 'Copy') {
  const { Box, Button } = look.ui
  const copy = look.copy

  if (copy === undefined) {
    return ''
  }

  return (
    <Box flexDirection="row" justifyContent="flex-end">
      <Button key={key} label={label} plain dimColor onPress={() => copy(text)} />
    </Box>
  )
}

// A copy block (commands.ts): its text as one Text the terminal wraps, nothing beside it
// on any of its lines, so a selection over it crosses no border, padding or margin glyph,
// and Copy on the line under it, which copies the text whole. A suggestion's `!` or `/`
// is drawn bold; a fence's code is drawn by `codeCopyRows`. `gapBelow` keeps a blank line
// under it for prose that follows, which opens on none; a frame opens on one.
export function copyRows(look: Look, copy: Copy, key: string, gapBelow: boolean) {
  const { Box, Text } = look.ui
  const { palette } = look.skin
  const margins = { marginTop: 1, marginBottom: gapBelow ? 1 : 0 }

  if (copy.form === 'code') {
    return codeCopyRows(look, copy.lang, copy.text, copyButton(look, key, copyText(look, copy)), margins)
  }

  const lead = copy.form === 'span' ? 0 : 1

  return (
    <Box flexDirection="column" {...margins}>
      <Text>
        {lead === 0 ? (
          ''
        ) : (
          <Text color={palette.user} bold>
            {copy.text.slice(0, lead)}
          </Text>
        )}
        <Text color={palette.search}>{copy.text.slice(lead)}</Text>
      </Text>
      {copyRow(look, key, copyText(look, copy))}
    </Box>
  )
}

// One block of a reply, `maxWidth` columns wide.
function segmentRow(look: Look, segment: Segment, i: number, maxWidth: number, Svg?: SvgElement) {
  const { Box, Markdown } = look.ui

  if (segment.kind === 'text') {
    const text = look.prefs.math ? inlineMath(segment.text) : segment.text

    return look.prefs.markdown ? (
      <Box flexDirection="column">{blockRows(look, splitBlocks(text, { prose: look.surface === 'terminal' }))}</Box>
    ) : (
      <Markdown text={text} />
    )
  }

  if (segment.kind === 'copy') {
    return copyRows(look, segment, `copy-${i}`, true)
  }

  if (segment.kind === 'math') {
    return Svg === undefined ? mathRows(look, segment.tex, maxWidth, `copy-${i}`) : mathCard(look, segment.tex, Svg, maxWidth, `copy-${i}`)
  }

  // A shell fence on the desktop stays the app's own block, which has Run and Copy.
  if (segment.kind === 'code' && Svg !== undefined && isShell(segment)) {
    return <Markdown text={segment.raw} />
  }

  if (segment.kind === 'code') {
    // A ```diff fence with changes in it is a diff, not code; ```diff-fsharp names its language.
    const tag = diffFenceTag(segment.lang)
    const diff = tag === null ? null : fenceDiff(segment.code)

    if (diff !== null) {
      return diffFence(look, segment, diff, maxWidth, i, Svg, tag?.lang)
    }

    const { chart, art } = drawingOf(look, segment, maxWidth, Svg)

    if (art !== null) {
      return (
        <Box flexDirection="column">
          {artRows(look, art, chart === null ? '' : chartHeading(chart).count)}
          {copyRow(look, `copy-${i}`, copyText(look, segment))}
        </Box>
      )
    }

    if (chart !== null && Svg !== undefined) {
      return chartCard(look, chart, segment, Svg, maxWidth, `copy-${i}`)
    }

    // A terminal too narrow for a drawing falls back to rows, or to the code.
    if (chart !== null && (chart.kind === 'flow' || chart.kind === 'xy' || chart.kind === 'pie')) {
      return (
        <Box flexDirection="column">
          {chartRows(look, chart, maxWidth)}
          {copyRow(look, `copy-${i}`, copyText(look, segment))}
        </Box>
      )
    }

    // The terminal highlights the languages it knows in the skin's colours.
    if (look.surface === 'terminal' && look.prefs.highlight && languageOf(segment.lang) !== undefined) {
      return codeRows(look, segment.lang, segment.code, copyButton(look, `copy-${i}`, copyText(look, segment)))
    }

    return (Svg === undefined ? null : codeCard(look, segment.lang, segment.code, Svg, maxWidth, `copy-${i}`, copyText(look, segment))) ?? stockFence(look, segment, `copy-${i}`)
  }

  return Svg === undefined ? (
    tableRows(look, segment, maxWidth, tableControls(look, `copy-${i}`, segment))
  ) : (
    tableCard(look, segment, Svg, maxWidth, `copy-${i}`)
  )
}

// A Mermaid fence's chart, and on the terminal its drawing in cells: the kinds we parse in
// our own drawings, flowcharts, xy charts and the kinds we do not parse in the vendored one.
// Kept per segment for the draw that made it: laying a reply out measures a diagram
// before drawing it.
const drawings = new WeakMap<Segment, Map<string, { chart: Chart | null; art: Art | null }>>()

function drawingOf(look: Look, segment: Segment & { kind: 'code' }, maxWidth: number, Svg?: SvgElement) {
  const key = `${maxWidth}|${Svg === undefined}|${look.prefs.charts}|${look.prefs.icons}`
  const kept = drawings.get(segment)?.get(key)

  if (kept !== undefined) {
    return kept
  }

  const chart = segment.lang === 'mermaid' && look.prefs.charts ? parseMermaid(segment.code) : null
  const ascii = look.prefs.icons === 'ascii'
  const ours = Svg === undefined && chart !== null ? chartArt(chart, maxWidth, ascii) : null
  const vendored = Svg === undefined && segment.lang === 'mermaid' && look.prefs.charts && (chart === null || chart.kind === 'flow' || chart.kind === 'xy')
  const drawing = { chart, art: ours ?? (vendored ? mermaidArt(segment.code, maxWidth, ascii) : null) }

  drawings.set(segment, (drawings.get(segment) ?? new Map()).set(key, drawing))

  return drawing
}

// A diff fence: the diff card on the desktop, numbered rows on the terminal, with Copy for
// the fence and `new only` for the code as it stands after the change. A diff past what a
// card holds keeps Claude Code's drawing of the fence.
function diffFence(look: Look, fence: Code, diff: DiffInput, maxWidth: number, i: number, Svg?: SvgElement, fenceLang?: string) {
  const { Box, Markdown } = look.ui
  const whole = copyButton(look, `copy-${i}`, copyText(look, fence))
  const fresh = copyButton(look, `copy-${i}-new`, newOnly(diff), ' new only ')
  const built = Svg === undefined ? null : diffCard(look, Svg, diff, diff.path === '' ? 'diff' : diff.path, maxWidth)

  if (Svg !== undefined) {
    return (
      <Box flexDirection="column">
        {built ?? <Markdown text={fence.raw} />}
        <Box flexDirection="row" justifyContent="flex-end">
          {whole ?? ''}
          {fresh ?? ''}
        </Box>
      </Box>
    )
  }

  const controls =
    whole === undefined || fresh === undefined ? undefined : (
      <Box flexDirection="row">
        {whole}
        {fresh}
      </Box>
    )

  return diffRows(look, diff, controls, fenceLang)
}

// Columns between blocks laid side by side.
const BESIDE_GAP = 2
// The narrowest a card may be drawn beside another on the desktop, in columns.
const BESIDE_MIN_CARD = 75

// How wide a block is when it may stand beside another: a table, or a drawn diagram. On
// the terminal its natural width in columns; on the desktop 0, for an even share. Null
// for any other block.
function besideWidth(look: Look, segment: Segment, maxWidth: number, Svg?: SvgElement): number | null {
  if (segment.kind === 'table') {
    return Svg !== undefined ? 0 : columnWidths(segment, maxWidth - 4, CELL_PAD * 2).reduce((sum, width) => sum + width + CELL_PAD * 2, 2)
  }

  if (segment.kind !== 'code' || segment.lang !== 'mermaid' || !look.prefs.charts) {
    return null
  }

  const { chart, art } = drawingOf(look, segment, maxWidth, Svg)

  if (Svg !== undefined) {
    return chart === null ? null : 0
  }

  // A drawing is laid out within two columns less than it is given (`chartArt`,
  // `mermaidArt`), so its place beside another block keeps those two.
  return art === null ? null : Math.max(...art.rows.map(row => row.reduce((sum, run) => sum + widthOf(run.text), 0))) + 2
}

type Placed = { segment: Segment; i: number; width: number }

// The reply's blocks in rows: a run of tables and diagrams with only blank text between
// them shares a row while the row has room; everything else, and a run that does not fit,
// stacks as before.
export function layoutRows(look: Look, segments: readonly Segment[], maxWidth: number, Svg?: SvgElement): Placed[][] {
  const rows: Placed[][] = []
  let run: Placed[] = []
  const flush = () => {
    rows.push(...packRun(run, maxWidth, Svg))
    run = []
  }

  for (const [i, segment] of segments.entries()) {
    const width = besideWidth(look, segment, maxWidth, Svg)

    if (width !== null) {
      run.push({ segment, i, width })
    } else if (!(segment.kind === 'text' && segment.text.trim() === '' && run.length > 0)) {
      flush()
      rows.push([{ segment, i, width: maxWidth }])
    }
  }

  flush()

  return rows
}

// Fills rows left to right; a block that does not fit starts the next row.
function packRun(run: readonly Placed[], maxWidth: number, Svg?: SvgElement): Placed[][] {
  if (Svg !== undefined) {
    const perRow = Math.max(1, Math.floor((maxWidth + BESIDE_GAP) / (BESIDE_MIN_CARD + BESIDE_GAP)))
    const rows: Placed[][] = []

    for (let at = 0; at < run.length; at += perRow) {
      const row = run.slice(at, at + perRow)
      const share = row.length === 1 ? maxWidth : Math.floor((maxWidth - BESIDE_GAP * (row.length - 1)) / row.length)
      rows.push(row.map(placed => ({ ...placed, width: share })))
    }

    return rows
  }

  const rows: Placed[][] = []
  let row: Placed[] = []
  let used = 0

  for (const placed of run) {
    const width = Math.min(placed.width, maxWidth)

    if (row.length > 0 && used + BESIDE_GAP + width > maxWidth) {
      rows.push(row)
      row = []
      used = 0
    }

    used += (row.length === 0 ? 0 : BESIDE_GAP) + width
    row.push({ ...placed, width: row.length === 0 && width === maxWidth ? maxWidth : width })
  }

  // A block alone on its row takes the whole width, as it would stacked.
  return [...rows, ...(row.length === 0 ? [] : [row])].map(placed => (placed.length === 1 ? placed.map(one => ({ ...one, width: maxWidth })) : placed))
}

/** A reply the frame would wrap, laid out in frames of its own around its copy blocks
 *  (replyRows): `frame` wraps a run of blocks, and takes `inset` columns of the width. */
export type Framing = { inset: number; frame: (rows: ReturnType<Ui['Box']>) => ReturnType<Ui['Box']> }

// `whole`, the reply as Claude wrote it, adds a `copy reply` control under it. With
// `framing`, each run of blocks between copy blocks (commands.ts) sits in a frame of its
// own and each copy block between the frames, the whole width wide, so no frame line runs
// beside a line of it.
export function replyRows(look: Look, segments: readonly Segment[], maxWidth: number, Svg?: SvgElement, whole?: string, framing?: Framing) {
  const { Box } = look.ui
  const inner = maxWidth - (framing?.inset ?? 0)
  const rows = layoutRows(look, segments, inner, Svg)
  const drawn = (row: Placed[]) =>
    row.length === 1 ? (
      segmentRow(look, row[0]!.segment, row[0]!.i, inner, Svg)
    ) : (
      <Box flexDirection="row" columnGap={BESIDE_GAP} alignItems="flex-start">
        {row.map(placed => (
          <Box flexDirection="column" flexShrink={0} {...(Svg === undefined ? { width: placed.width } : { width: 0, flexGrow: 1 })}>
            {segmentRow(look, placed.segment, placed.i, placed.width, Svg)}
          </Box>
        ))}
      </Box>
    )
  const copyOf = (row: Placed[]) => (row.length === 1 && row[0]!.segment.kind === 'copy' ? { copy: row[0]!.segment, i: row[0]!.i } : null)
  const runs: (Placed[][] | { copy: Copy; i: number })[] = []

  for (const row of framing === undefined ? [] : rows) {
    const copy = copyOf(row)
    const last = runs.at(-1)

    if (copy !== null) {
      runs.push(copy)
    } else if (Array.isArray(last)) {
      last.push(row)
    } else {
      runs.push([row])
    }
  }

  return (
    <Box flexDirection="column">
      {framing === undefined
        ? rows.map(drawn)
        : runs.map(run =>
            Array.isArray(run) ? framing.frame(<Box flexDirection="column">{run.map(drawn)}</Box>) : copyRows(look, run.copy, `copy-${run.i}`, false),
          )}
      {whole === undefined ? '' : copyRow(look, 'copy-reply', whole, 'copy reply')}
    </Box>
  )
}

// The word with a band of light sweeping through it, one letter a frame.
export function spinnerRow(look: Look, word: string, frame: number, elapsedMs: number) {
  const { Text } = look.ui
  const { palette } = look.skin
  const letters = [...word]
  const head = (frame % (letters.length + 8)) - 4
  const shade = (i: number): string => {
    const distance = Math.abs(i - head)

    return distance === 0 ? palette.fg : distance === 1 ? palette.user : palette.muted
  }
  const runs: { color: string; text: string }[] = []

  for (const [i, letter] of letters.entries()) {
    const color = shade(i)
    const last = runs.at(-1)

    if (last?.color === color) {
      last.text += letter
    } else {
      runs.push({ color, text: letter })
    }
  }

  return (
    <Text>
      <Text color={palette.user}>{`${look.icons.frames[frame % look.icons.frames.length] ?? ''} `}</Text>
      {runs.map(run => (
        <Text color={run.color}>{run.text}</Text>
      ))}
      <Text color={palette.muted}>{`…  ${formatDuration(elapsedMs)}`}</Text>
    </Text>
  )
}

export function footerRow(look: Look, word: string, durationMs: number, stats: TurnStats | undefined) {
  const { Text } = look.ui
  const { palette } = look.skin
  const facts = [
    ...(stats === undefined || stats.tools === 0 ? [] : [`${stats.tools} tool${stats.tools === 1 ? '' : 's'}`]),
  ]
  const hasDiff = stats !== undefined && stats.added + stats.removed > 0

  return (
    <Text color={palette.muted}>
      <Text color={palette.user}>{`${look.icons.done} `}</Text>
      {`${word} in ${formatDuration(durationMs)}`}
      {facts.length > 0 ? `  ·  ${facts.join('  ·  ')}` : ''}
      {hasDiff ? '  ·  ' : ''}
      {hasDiff ? <Text color={palette.ok}>{`+${stats.added}`}</Text> : ''}
      {hasDiff ? <Text color={palette.err}>{` −${stats.removed}`}</Text> : ''}
    </Text>
  )
}

// A band above Claude Code's own question dialog: the topics it asks about, in the skin.
export function askBand(look: Look, headers: readonly string[]) {
  const { Text } = look.ui
  const { palette } = look.skin

  return (
    <Text color={palette.muted}>
      <Text color={palette.user}>{`${look.icons.prompt} `}</Text>
      {headers.map((header, i) => (
        <Text color={i === 0 ? palette.fg : palette.muted} bold={i === 0}>
          {`${i === 0 ? '' : '  ·  '}${header}`}
        </Text>
      ))}
    </Text>
  )
}

// A vector card in a reply or under a tool row, a line's breath above and below it.
function card(look: Look, Svg: SvgElement, built: { source: string; alt: string; entranceMs: number }, key: string) {
  const { Box } = look.ui

  return (
    <Box marginY={1}>
      <Svg source={sourceOf(look, key, built)} alt={built.alt} />
    </Box>
  )
}

// A code block as a card, every line drawn. Copy takes `copied`. Null for code past what
// a card holds (MAX_SVG), which the caller leaves to Claude Code's own drawing.
export function codeCard(look: Look, lang: string, code: string, Svg: SvgElement, columns: number, key = 'copy-code', copied = code) {
  const built = codeSvg(code, lang, look.skin.palette, cardWidth(columns), look.copy !== undefined)

  return built === null ? null : cardWithCopy(look, Svg, built, key, copied)
}

// A fence in Claude Code's own drawing, with Copy under it.
function stockFence(look: Look, fence: Code, key: string) {
  const { Box, Markdown } = look.ui

  return (
    <Box flexDirection="column">
      <Markdown text={fence.raw} />
      {copyRow(look, key, copyText(look, fence))}
    </Box>
  )
}

// A chart too crowded to draw at this width keeps its source as a code card.
function chartCard(look: Look, chart: Chart, fence: Code, Svg: SvgElement, columns: number, key: string) {
  const built = chartSvg(chart, look.skin.palette, cardWidth(columns), look.copy !== undefined)

  return built === null
    ? (codeCard(look, 'mermaid', fence.code, Svg, columns, key, copyText(look, fence)) ?? stockFence(look, fence, key))
    : cardWithCopy(look, Svg, built, key, copyText(look, fence))
}

// An edit as a card, or null for one past what a card holds (MAX_SVG).
export function diffCard(look: Look, Svg: SvgElement, input: DiffInput, shownPath: string, columns: number) {
  const built = diffSvg(input, shownPath, look.skin.palette, cardWidth(columns))

  return built === null ? null : card(look, Svg, built, 'diff')
}

// Shell output as a card, or null for output past what a card holds (MAX_SVG).
export function terminalCard(look: Look, Svg: SvgElement, output: ShellOutput, isErrored: boolean, columns: number, key = 'copy-output') {
  const text = [output.stdout, output.stderr].filter(part => part.trim() !== '').join('\n')
  const withCopy = text === '' ? { ...look, copy: undefined } : look
  const built = terminalSvg(output, isErrored, look.skin.palette, cardWidth(columns), withCopy.copy !== undefined, look.prefs.highlight)

  return built === null ? null : cardWithCopy(withCopy, Svg, built, key, text)
}

// From this full, the band suggests compacting and makes it the main action.
export const COMPACT_NUDGE = 70

export const COMPACT_HOTKEY = '0'

// One drawing or line per meter, so the band can spread them evenly across its width. Whether
// the rings fill in is still decided once for the whole band, keyed on every meter, so they
// fill together when any number changes and hold still together otherwise.
function meterView(look: Look, meters: readonly Meter[]) {
  const { Text } = look.ui
  const { palette } = look.skin

  if (look.svg !== undefined) {
    const Svg = look.svg
    const builds = meters.map(meter => ({ meter, built: usageSvg([meter], palette, true) }))
    const entranceMs = Math.max(0, ...builds.map(({ built }) => built.entranceMs))
    const isFirst = look.isFirstDraw === undefined || look.isFirstDraw(`usage:${metersKey(meters)}`, entranceMs)

    return builds.map(({ meter, built }) => (
      <Svg key={`usage-${meter.label}`} source={isFirst ? built.source : settled(built.source)} alt={built.alt} width={built.width} height={built.height} />
    ))
  }

  return usageLine(meters).map(meter => (
    <Text color={palette.muted}>
      <Text color={palette.user}>{meter.bar}</Text>
      {` ${meter.text}`}
    </Text>
  ))
}

// The band above the prompt: the meters, spread evenly, and a Compact button that becomes
// the main action, with a word on why, once the context is full enough to be worth it.
// `/skin compact off` drops the button and the word, and the meters take the whole width.
export function usageBand(look: Look, meters: readonly Meter[], canCompact: boolean, compact: () => void) {
  const { Box, Text, Button } = look.ui
  const { palette } = look.skin
  const context = meters.find(meter => meter.label === 'context')?.percent ?? 0
  const isNudge = context >= COMPACT_NUDGE

  return (
    // The right edge stays clear: the band draws its own collapse mark ([-]) there.
    <Box flexDirection="row" alignItems="center" columnGap={2} paddingRight={5}>
      <Box flexDirection="row" alignItems="center" columnGap={3} justifyContent="space-evenly" flexGrow={1}>
        {meterView(look, meters)}
      </Box>
      {canCompact && isNudge ? <Text color={palette.warn}>{`Context is ${context}% full`}</Text> : ''}
      {canCompact ? (
        // A digit hotkey: typed alone into an empty prompt it presses the band's button, which
        // is the only way in where the terminal reports no clicks. `plain` shows it: `0: Compact`.
        <Button
          key="compact"
          label={isNudge ? 'Compact now' : 'Compact'}
          hotkey={COMPACT_HOTKEY}
          plain
          {...(isNudge ? { variant: 'primary' as const } : { dimColor: true })}
          onPress={compact}
        />
      ) : (
        ''
      )}
    </Box>
  )
}
