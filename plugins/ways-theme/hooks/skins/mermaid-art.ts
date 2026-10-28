// Ported from skins by hellosverre (MIT; LICENSE beside this file),
// https://github.com/hellosverre/claude-skins at 985ad63, hooks/mermaid-art.ts.

import { renderMermaidAscii, seriesColors, setChartSize } from './vendor/mermaid-ascii.js'
import type { AsciiTheme } from './vendor/mermaid-ascii.js'
import { charWidth, widthOf } from './markdown'
import { frontmatterOf } from './mermaid-lex'
import type { Slot } from './skin'

// A Mermaid fence laid out in two dimensions by beautiful-mermaid's terminal renderer,
// then handed back as runs of text tagged with what they draw, so the skin can colour
// them: each box its own colour, the links quiet, the arrowheads and labels plain.

// The same runs carry the charts this mod draws itself (chart-art.ts): those also name a
// skin slot outright, and a background for half-block cells.
export type Tone = 'fg' | 'muted' | 'title' | { box: number } | { series: number } | { slot: Slot }

export interface Run {
  text: string
  tone: Tone | null
  back?: Tone
}

export interface Art {
  kind: string
  title?: string
  rows: Run[][]
}

const KINDS: readonly (readonly [RegExp, string])[] = [
  [/^(flowchart|graph)\b/i, 'Flowchart'],
  [/^statediagram(-v2)?\s*$/i, 'State'],
  [/^sequencediagram\s*$/i, 'Sequence'],
  [/^classdiagram\s*$/i, 'Class'],
  [/^erdiagram\s*$/i, 'ER'],
  [/^xychart(-beta)?\b/i, 'Chart'],
]

const MAX_LINES = 80
const MAX_CHARS = 8000
const MAX_ROWS = 120
// The widest gap the renderer leaves between a box and a link starting from it.
const MAX_GAP = 3
// Follows every wide character sent to the renderer, which counts code points, so the
// character gets the two columns it takes on screen. It comes back as an empty cell.
const SPARE = '\ue000'
const CACHE_SIZE = 64

// Colours no theme uses, one per role, so the renderer's HTML output says what each
// cell is.
const SENTINEL: AsciiTheme = {
  fg: '#000001',
  border: '#000002',
  line: '#000003',
  arrow: '#000004',
  corner: '#000005',
  junction: '#000006',
  accent: '#000007',
  bg: '#000000',
}

type Role = 'text' | 'border' | 'line' | 'arrow'

const ROLE_OF: Readonly<Record<string, Role>> = {
  [SENTINEL.fg]: 'text',
  [SENTINEL.border]: 'border',
  [SENTINEL.line]: 'line',
  [SENTINEL.arrow]: 'arrow',
  [SENTINEL.corner ?? '']: 'line',
  [SENTINEL.junction ?? '']: 'border',
}

interface Cell {
  char: string
  role: Role | null
  series?: number
}

// Diamond corners come back as label text; they belong to the box.
const BOX_GLYPHS = new Set(['◇'])

const cache = new Map<string, Art | null>()

// What kind of diagram the renderer draws for `source`, or null for one it does not.
export function artKind(source: string): string | null {
  const header = frontmatterOf(source).lines.find(line => line.trim() !== '' && !line.trim().startsWith('%%'))?.trim() ?? ''

  return KINDS.find(([pattern]) => pattern.test(header))?.[1] ?? null
}

// `source` drawn to fit `columns`, or null where it cannot be: unsupported, malformed,
// too long, or too wide even top to bottom. The fence then shows as code.
export function mermaidArt(source: string, columns: number, ascii: boolean): Art | null {
  const key = `${ascii ? 'a' : 'u'}|${columns}|${source}`

  if (cache.has(key)) {
    return cache.get(key) ?? null
  }

  const art = drawArt(source, columns, ascii)

  if (cache.size >= CACHE_SIZE) {
    cache.delete(cache.keys().next().value ?? '')
  }

  cache.set(key, art)

  return art
}

function drawArt(source: string, columns: number, ascii: boolean): Art | null {
  const kind = artKind(source)
  // The renderer reads no frontmatter; its title becomes the card's.
  const { title, lines } = frontmatterOf(source)
  const text = lines.join('\n').trim()

  if (kind === null || text.length > MAX_CHARS || text.split('\n').length > MAX_LINES) {
    return null
  }

  const room = columns - 2
  // Left to right first where asked; a wide one is tried again top to bottom.
  const attempts = kind === 'Flowchart' && /^(flowchart|graph)\s+(LR|RL)\b/i.test(text)
    ? [text, text.replace(/^(flowchart|graph)\s+(LR|RL)\b/i, '$1 TD')]
    : [text]

  for (const attempt of attempts) {
    const drawn = render(attempt, kind, room, ascii)

    // The renderer draws what it cannot read as nothing rather than throwing.
    if (drawn === null || drawn.length === 0) {
      return null
    }

    const grid = kind === 'Flowchart' || kind === 'State' ? reattach(drawn) : drawn

    if (grid.length <= MAX_ROWS && grid.every(row => widthOf(row.map(cell => cell.char).join('')) <= room)) {
      return { kind, title, rows: paint(grid, kind) }
    }
  }

  return null
}

const LINE: Cell = { char: '─', role: 'line' }
const BLANK: Cell = { char: ' ', role: null }

const isGap = (cell: Cell | undefined): boolean => cell?.char === ' '
const isEdgeLabel = (cell: Cell | undefined): boolean => cell?.role === 'text' && cell.char !== ' '

// Where the renderer loses a link at a box's side, put it back. Three slips, all seen
// in real replies: a link leaving sideways starts a few cells out from the border; one
// leaving upward starts on the row inside the border; and a label written next to a box
// blanks the link's last cells, arrowhead and all.
function reattach(grid: Cell[][]): Cell[][] {
  const rows = grid.map(row => row.slice())

  // Read from the renderer's grid, so a junction already moved onto a border is not
  // moved again.
  grid.forEach((cells, y) => {
    const row = rows[y] ?? []

    cells.forEach((cell, x) => {
      if (cell.char === '├' || cell.char === '┤') {
        joinSideways(row, x, cell.char === '├' ? -1 : 1)
      } else if (cell.char === '┴') {
        joinUpward(rows, x, y)
      } else if (cell.char === '│' && cell.role === 'border') {
        rejoinLabel(row, x, 1)
        rejoinLabel(row, x, -1)
      }
    })
  })

  return rows
}

// A junction at `x` whose box lies `step` away across a gap: the junction onto the
// border, the gap filled with line. An incoming arrowhead goes up to the border instead.
function joinSideways(row: Cell[], x: number, step: -1 | 1): void {
  let border = x + step

  while (Math.abs(border - x) <= MAX_GAP && isGap(row[border])) {
    border += step
  }

  const onBorder = row[border]
  const onward = row[x - step]?.char ?? ''

  if (border === x + step || onBorder?.char !== '│' || !'─┼◄►'.includes(onward)) {
    return
  }

  for (let at = border - step; at !== x - step; at -= step) {
    row[at] = LINE
  }

  if (onward === (step < 0 ? '◄' : '►')) {
    row[x - step] = LINE
    row[border - step] = { char: onward, role: 'arrow' }
  } else {
    row[border] = { ...onBorder, char: step < 0 ? '├' : '┤' }
  }
}

// A `┴` alone inside a box, under its top border: the link it starts runs up from the
// border, so the junction goes there; where an arrowhead already lands on that cell,
// the arrowhead says enough and the junction goes.
function joinUpward(rows: Cell[][], x: number, y: number): void {
  const row = rows[y] ?? []
  const border = rows[y - 1]?.[x]

  if (border?.char !== '─' || !isGap(row[x - 1]) || !isGap(row[x + 1])) {
    return
  }

  if (rows[y - 2]?.[x]?.char === '│') {
    rows[y - 1]?.splice(x, 1, { ...border, char: '┴' })
  }

  row[x] = BLANK
}

// A box border at `x`, a gap, then a label running on into a link: the link's end was
// blanked by the label, so draw it back to the box with its arrowhead.
function rejoinLabel(row: Cell[], x: number, step: -1 | 1): void {
  let label = x + step

  while (Math.abs(label - x) <= MAX_GAP && isGap(row[label])) {
    label += step
  }

  if (label === x + step || !isEdgeLabel(row[label])) {
    return
  }

  let end = label

  while (isEdgeLabel(row[end])) {
    end += step
  }

  if (row[end]?.role !== 'line') {
    return
  }

  row[x + step] = { char: step > 0 ? '◄' : '►', role: 'arrow' }

  for (let at = x + 2 * step; at !== label; at += step) {
    row[at] = LINE
  }
}

// `text` with a spare after each wide character.
const widened = (text: string): string => [...text].map(char => (charWidth(char) === 2 ? char + SPARE : char)).join('')

// Whether every unbroken stretch of wide characters in `text` shows in `grid`. The
// renderer drops a node whose id is wide and draws the rest without a word.
function drawsWide(text: string, grid: Cell[][]): boolean {
  const drawn = grid.map(row => row.map(cell => cell.char).join('')).join('\n')
  const stretches = [...text].map(char => (charWidth(char) === 2 ? char : ' ')).join('').split(' ')

  return stretches.every(stretch => drawn.includes(stretch))
}

function render(text: string, kind: string, room: number, ascii: boolean): Cell[][] | null {
  if (kind === 'Chart') {
    const width = Math.max(24, Math.min(60, room - 10))
    setChartSize(width, Math.max(8, Math.min(16, Math.round(width * 0.3))))
  }

  // Charts size their own labels.
  const sent = kind === 'Chart' ? text : widened(text)

  try {
    const html = renderMermaidAscii(sent, { useAscii: ascii, paddingX: 4, paddingY: 2, colorMode: 'html', theme: SENTINEL })
    const series = new Map(seriesColors(8, SENTINEL).map((hex, i) => [hex.toLowerCase(), i]))
    series.set(SENTINEL.accent ?? '', 0)

    const grid = trimGrid(html.split('\n').map(line => cellsOf(line, series)))

    return sent !== text && !drawsWide(text, grid) ? null : grid
  } catch {
    return null
  }
}

const unescape = (text: string): string => text.replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&amp;/g, '&')

// One line of the renderer's HTML as cells, each with the role its colour names.
function cellsOf(line: string, series: ReadonlyMap<string, number>): Cell[] {
  const cells: Cell[] = []
  const span = /<span style="color:(#[0-9a-fA-F]{6})">(.*?)<\/span>/g
  let at = 0

  const plain = (text: string) => {
    for (const char of unescape(text)) {
      cells.push({ char, role: null })
    }
  }

  for (const match of line.matchAll(span)) {
    plain(line.slice(at, match.index))
    const hex = (match[1] ?? '').toLowerCase()
    const role = ROLE_OF[hex]
    const index = role === undefined ? series.get(hex) : undefined

    for (const char of unescape(match[2] ?? '')) {
      cells.push(index === undefined ? { char, role: role ?? 'text' } : { char, role: null, series: index })
    }

    at = (match.index ?? 0) + match[0].length
  }

  plain(line.slice(at))

  return cells.map(cell => (cell.char === SPARE ? { ...cell, char: '' } : cell))
}

// Trailing blanks off every row, and blank rows off both ends.
function trimGrid(grid: Cell[][]): Cell[][] {
  const rows = grid.map(row => {
    let end = row.length

    while (end > 0 && row[end - 1]?.char === ' ') {
      end--
    }

    return row.slice(0, end)
  })

  while (rows.length > 0 && rows[0]?.length === 0) {
    rows.shift()
  }

  while (rows.length > 0 && rows[rows.length - 1]?.length === 0) {
    rows.pop()
  }

  return rows
}

// --- Colour ------------------------------------------------------------------------

function paint(grid: Cell[][], kind: string): Run[][] {
  const boxes = kind === 'Chart' ? null : boxColours(grid)
  const titleRow = kind === 'Chart' ? grid.findIndex(row => row.some(cell => cell.role === 'text')) : -1

  return grid.map((row, y) => {
    const runs: Run[] = []

    row.forEach((cell, x) => {
      const tone = toneOf(cell, boxes?.get(`${x},${y}`), y === titleRow)
      const last = runs[runs.length - 1]

      if (last !== undefined && sameTone(last.tone, tone)) {
        last.text += cell.char
      } else {
        runs.push({ text: cell.char, tone })
      }
    })

    return runs
  })
}

function toneOf(cell: Cell, box: number | undefined, title: boolean): Tone | null {
  if (cell.series !== undefined) {
    return { series: cell.series }
  }

  if (box !== undefined) {
    return { box }
  }

  switch (cell.role) {
    case 'text':
      return title ? 'title' : 'fg'
    case 'arrow':
      return 'fg'
    case 'border':
    case 'line':
      return 'muted'
    default:
      return null
  }
}

export const sameTone = (a: Tone | null | undefined, b: Tone | null | undefined): boolean =>
  a === b || (typeof a === 'object' && typeof b === 'object' && a !== null && b !== null && JSON.stringify(a) === JSON.stringify(b))

const isBorder = (cell: Cell | undefined): boolean =>
  cell !== undefined && (cell.role === 'border' || (cell.role === 'text' && BOX_GLYPHS.has(cell.char)))

// Every box outline found, its cells mapped to a colour index. Outlines with the same
// label inside share one, so an actor drawn at both ends of a sequence matches itself.
function boxColours(grid: Cell[][]): Map<string, number> {
  const seen = new Set<string>()
  const colours = new Map<string, number>()
  const byLabel = new Map<string, number>()

  grid.forEach((row, y) => {
    row.forEach((cell, x) => {
      if (!isBorder(cell) || seen.has(`${x},${y}`)) {
        return
      }

      const outline = floodBorder(grid, x, y, seen)
      const label = labelInside(grid, outline)
      const colour = byLabel.get(label) ?? byLabel.size

      byLabel.set(label, colour)
      outline.forEach(([cx, cy]) => colours.set(`${cx},${cy}`, colour))
    })
  })

  return colours
}

function floodBorder(grid: Cell[][], x: number, y: number, seen: Set<string>): [number, number][] {
  const outline: [number, number][] = []
  const stack: [number, number][] = [[x, y]]
  seen.add(`${x},${y}`)

  while (stack.length > 0) {
    const [cx, cy] = stack.pop() ?? [0, 0]
    outline.push([cx, cy])

    for (const [nx, ny] of [[cx + 1, cy], [cx - 1, cy], [cx, cy + 1], [cx, cy - 1]] as const) {
      if (!seen.has(`${nx},${ny}`) && isBorder(grid[ny]?.[nx])) {
        seen.add(`${nx},${ny}`)
        stack.push([nx, ny])
      }
    }
  }

  return outline
}

function labelInside(grid: Cell[][], outline: readonly [number, number][]): string {
  const xs = outline.map(([x]) => x)
  const ys = outline.map(([, y]) => y)
  const [left, right, top, bottom] = [Math.min(...xs), Math.max(...xs), Math.min(...ys), Math.max(...ys)]
  let label = ''

  for (let y = top + 1; y < bottom; y++) {
    for (let x = left + 1; x < right; x++) {
      const cell = grid[y]?.[x]
      label += cell?.role === 'text' ? cell.char : ' '
    }
  }

  return label.replace(/\s+/g, ' ').trim() || `@${left},${top}`
}
