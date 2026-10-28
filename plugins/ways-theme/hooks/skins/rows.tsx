// Ported from skins by hellosverre (MIT; LICENSE beside this file),
// https://github.com/hellosverre/claude-skins at 985ad63, hooks/rows.tsx.

import type { ElementTable, RenderSurface } from 'claude-code'

import type { Prefs, Touch, TurnStats } from '../../types'
import { formatDuration, formatMs } from './format'
import type { PathBase } from './links'
import { columnWidths, isShell, tableArt, widthOf, wrapLines } from './markdown'
import { diffFenceTag, diffRows, fenceDiff, newOnly } from './diff-fence'
import { FOLD_CODE, FOLD_ROWS, foldButton, shownCount } from './fold'
import type { Segment, Table } from './markdown'
import { splitBlocks } from './blocks'
import { artRows, chartRows } from './chart-rows'
import { chartArt } from './chart-art'
import { chartHeading, parseMermaid } from './mermaid'
import { mermaidArt } from './mermaid-art'
import type { Art } from './mermaid-art'
import type { Chart } from './mermaid'
import { blockRows } from './prose'
import { MAX_MARKDOWN } from './reply'
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
import { codeRows } from './code-rows'
import { languageOf } from './highlight'
import type { Meter } from './svg-usage'
import { kindOf, toolLabel } from './tools'
import { inlineMath, mathArt, parseTex } from './math'
import { mathSvg } from './svg-math'

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
  // The blocks the person unfolded, by key, and how to flip one. Absent, nothing folds.
  folds?: { open: ReadonlySet<string>; toggle: (key: string) => void }
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

function node(look: Look, calls: readonly Call[]) {
  const { Text } = look.ui
  const { glyph, color } = status(look, calls)
  const branch = look.prefs.rail && look.surface === 'terminal' ? look.icons.branch : ''

  return (
    <Text color={color}>
      {`${glyph}`}
      <Text color={look.skin.palette.muted}>{`${branch} `}</Text>
    </Text>
  )
}

function metaSpans(look: Look, meta: Meta) {
  const { Text } = look.ui
  const { palette } = look.skin
  const spans = []

  if (meta.added !== undefined && meta.removed !== undefined && meta.added + meta.removed > 0) {
    spans.push(<Text color={palette.ok}>{`+${meta.added}`}</Text>)
    spans.push(<Text color={palette.err}>{` −${meta.removed}`}</Text>)
  }

  if (meta.ms !== undefined) {
    spans.push(<Text color={palette.muted}>{`${spans.length > 0 ? '  ' : ''}${formatMs(meta.ms)}`}</Text>)
  }

  return spans
}

// A line with its facts flush right on the terminal and trailing it on the desktop,
// which lays a row out its own way.
function withMeta(look: Look, main: ReturnType<Ui['Text']>, meta: Meta) {
  const { Box, Text } = look.ui
  const spans = metaSpans(look, meta)

  if (spans.length === 0) {
    return main
  }

  if (look.surface !== 'terminal') {
    return (
      <Text wrap="truncate-end">
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

// `touch` colours the target of a file this turn created (ok) or edited (warn).
export function toolRow(look: Look, call: Call, kind: Kind, target: string, meta: Meta, label = toolLabel(call.tool), touch?: Touch) {
  const { Text } = look.ui
  const { palette } = look.skin
  const targetColor = call.isErrored ? palette.err : touch === 'created' ? palette.ok : touch === 'edited' ? palette.warn : palette.muted

  if (look.svg !== undefined) {
    const line = (
      <Text wrap="truncate-end">
        <Text color={palette[kind]} bold>
          {label}
        </Text>
        <Text color={targetColor}>{`  ${target}`}</Text>
      </Text>
    )

    return iconRow(look, look.svg, kind, [call], withMeta(look, line, meta))
  }

  const main = (
    <Text wrap="truncate-end">
      {node(look, [call])}
      <Text color={palette[kind]}>{label}</Text>
      <Text color={targetColor}>{`  ${target}`}</Text>
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
    <Text color={look.skin.palette.err} wrap="truncate-end">{`${look.prefs.icons === 'ascii' ? 'x' : '✖'} ${failure}`}</Text>
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
// a prompt too long for one Markdown block keeps the Text.
export function promptRow(look: Look, text: string, images?: ReturnType<Ui['Text']>) {
  const { Box, Markdown, Text } = look.ui
  const markdown = look.surface === 'terminal' ? null : literal(text)
  const plain = look.prefs.prompt === 'plain'

  return (
    <Box flexDirection="column" alignItems={look.prefs.prompt === 'right' ? 'flex-end' : 'flex-start'} marginY={1}>
      <Box {...(plain ? {} : { borderStyle: 'round' as const, borderColor: look.skin.palette.muted, paddingX: 1 })} flexShrink={1}>
        {plain ? <Text color={look.skin.palette.user}>{`${look.icons.prompt} `}</Text> : ''}
        {markdown === null || markdown.length > MAX_MARKDOWN ? (
          <Text color={look.skin.palette.fg}>{text}</Text>
        ) : (
          <Markdown text={markdown} />
        )}
      </Box>
      {images ?? ''}
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
// Past FOLD_ROWS rows it folds under `foldKey`. Long cells wrap within their column in
// full: unlike the desktop card, the terminal has no tooltip to read a cut cell by (#59).
export function tableRows(look: Look, table: Table, maxWidth: number, control?: ReturnType<Ui['Button']>, foldKey = 'fold-table') {
  const { Box, Text } = look.ui
  const rows = table.rows.slice(0, shownCount(look, foldKey, table.rows.length, FOLD_ROWS))
  const { palette } = look.skin
  const widths = columnWidths(table, maxWidth - 4, CELL_PAD * 2)
  const natural = widths.reduce((sum, width) => sum + width + CELL_PAD * 2, 2)
  const isFluid = natural > maxWidth * NATURAL_SHARE
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
            <Text color={palette.fg} bold={isHeader} wrap="truncate-end">
              {line}
            </Text>
          ))}
        </Box>
      ))}
    </Box>
  )

  const fold = foldButton(look, foldKey, table.rows.length, FOLD_ROWS)
  const frame = onBorder(
    look,
    <Box flexDirection="column" borderStyle="round" borderColor={palette.muted}>
      {row(table.header, true, palette.surface)}
      {rows.map((cells, i) => row(cells, false, i % 2 === 1 ? palette.zebra : undefined))}
    </Box>,
    { control, box: { marginY: fold === '' ? 1 : 0, ...(isFluid ? { width: '100%' } : { alignSelf: 'flex-start' }) } },
  )

  return fold === '' ? (
    frame
  ) : (
    <Box flexDirection="column" marginY={1}>
      {frame}
      {fold}
    </Box>
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
      {cardWithCopy(look, Svg, card, key, tableMarkdown(table))}
      {copyRow(look, `${key}-art`, tableArt(table), 'Copy as text')}
    </Box>
  )
}

// The table's two copies on its frame: markdown, and the table as drawn.
function tableControls(look: Look, key: string, table: Table) {
  const { Box } = look.ui
  const markdown = copyButton(look, key, tableMarkdown(table))
  const art = copyButton(look, `${key}-art`, tableArt(table), ' as text ')

  return markdown === undefined || art === undefined ? undefined : (
    <Box flexDirection="row">
      {markdown}
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

// A table as markdown again, for the clipboard.
const tableMarkdown = (table: Table): string =>
  [table.header, table.header.map(() => '---'), ...table.rows].map(cells => `| ${cells.join(' | ')} |`).join('\n')

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
      return diffFence(look, diff, segment.code, maxWidth, i, Svg, tag?.lang)
    }

    const { chart, art } = drawingOf(look, segment, maxWidth, Svg)

    if (art !== null) {
      return (
        <Box flexDirection="column">
          {artRows(look, art, chart === null ? '' : chartHeading(chart).count)}
          {copyRow(look, `copy-${i}`, segment.code)}
        </Box>
      )
    }

    if (chart !== null && Svg !== undefined) {
      return chartCard(look, chart, segment.code, Svg, maxWidth, `copy-${i}`)
    }

    // A terminal too narrow for a drawing falls back to rows, or to the code.
    if (chart !== null && (chart.kind === 'flow' || chart.kind === 'xy' || chart.kind === 'pie')) {
      return (
        <Box flexDirection="column">
          {chartRows(look, chart, maxWidth)}
          {copyRow(look, `copy-${i}`, segment.code)}
        </Box>
      )
    }

    // The terminal highlights the languages it knows in the skin's colours.
    if (look.surface === 'terminal' && look.prefs.highlight && languageOf(segment.lang) !== undefined) {
      return codeRows(look, segment.lang, segment.code, copyButton(look, `copy-${i}`, segment.code), `fold-${i}`)
    }

    return Svg === undefined ? (
      <Box flexDirection="column">
        <Markdown text={segment.raw} />
        {copyRow(look, `copy-${i}`, segment.code)}
      </Box>
    ) : (
      codeCard(look, segment.lang, segment.code, Svg, maxWidth, `copy-${i}`, `fold-${i}`)
    )
  }

  return Svg === undefined ? (
    tableRows(look, segment, maxWidth, tableControls(look, `copy-${i}`, segment), `fold-${i}`)
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
// the fence and `new only` for the code as it stands after the change.
function diffFence(look: Look, diff: DiffInput, code: string, maxWidth: number, i: number, Svg?: SvgElement, fenceLang?: string) {
  const { Box } = look.ui
  const whole = copyButton(look, `copy-${i}`, code)
  const fresh = copyButton(look, `copy-${i}-new`, newOnly(diff), ' new only ')

  if (Svg !== undefined) {
    return (
      <Box flexDirection="column">
        {diffCard(look, Svg, diff, diff.path === '' ? 'diff' : diff.path, maxWidth)}
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

  return diffRows(look, diff, controls, `fold-${i}`, fenceLang)
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

// `whole`, the reply as Claude wrote it, adds a `copy reply` control under it.
export function replyRows(look: Look, segments: readonly Segment[], maxWidth: number, Svg?: SvgElement, whole?: string) {
  const { Box } = look.ui

  return (
    <Box flexDirection="column">
      {layoutRows(look, segments, maxWidth, Svg).map(row =>
        row.length === 1 ? (
          segmentRow(look, row[0]!.segment, row[0]!.i, maxWidth, Svg)
        ) : (
          <Box flexDirection="row" columnGap={BESIDE_GAP} alignItems="flex-start">
            {row.map(placed => (
              <Box flexDirection="column" flexShrink={0} {...(Svg === undefined ? { width: placed.width } : { width: 0, flexGrow: 1 })}>
                {segmentRow(look, placed.segment, placed.i, placed.width, Svg)}
              </Box>
            ))}
          </Box>
        ),
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

// Past FOLD_CODE lines the card is cut there, with `▾ N more` under it; opened, it runs to
// the card's own 80-line cap.
export function codeCard(look: Look, lang: string, code: string, Svg: SvgElement, columns: number, key = 'copy-code', foldKey = `${key}-fold`) {
  const { Box } = look.ui
  const count = code.split('\n').length
  const shown = shownCount(look, foldKey, count, FOLD_CODE)
  const card = cardWithCopy(look, Svg, codeSvg(code, lang, look.skin.palette, cardWidth(columns), look.copy !== undefined, shown === count ? undefined : shown), key, code)

  return look.folds === undefined || count <= FOLD_CODE ? (
    card
  ) : (
    <Box flexDirection="column">
      {card}
      {foldButton(look, foldKey, count, FOLD_CODE)}
    </Box>
  )
}

// A chart too crowded to draw at this width keeps its source as a code card.
function chartCard(look: Look, chart: Chart, source: string, Svg: SvgElement, columns: number, key: string) {
  const built = chartSvg(chart, look.skin.palette, cardWidth(columns), look.copy !== undefined)

  return built === null ? codeCard(look, 'mermaid', source, Svg, columns, key) : cardWithCopy(look, Svg, built, key, source)
}

export function diffCard(look: Look, Svg: SvgElement, input: DiffInput, shownPath: string, columns: number) {
  return card(look, Svg, diffSvg(input, shownPath, look.skin.palette, cardWidth(columns)), 'diff')
}

export function terminalCard(look: Look, Svg: SvgElement, output: ShellOutput, isErrored: boolean, columns: number, key = 'copy-output') {
  const text = [output.stdout, output.stderr].filter(part => part.trim() !== '').join('\n')
  const withCopy = text === '' ? { ...look, copy: undefined } : look

  return cardWithCopy(withCopy, Svg, terminalSvg(output, isErrored, look.skin.palette, cardWidth(columns), withCopy.copy !== undefined, look.prefs.highlight), key, text)
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
