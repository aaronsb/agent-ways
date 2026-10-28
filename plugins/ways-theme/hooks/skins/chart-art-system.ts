// Ported from skins by hellosverre (MIT; LICENSE beside this file),
// https://github.com/hellosverre/claude-skins at 985ad63, hooks/chart-art-system.ts.

import { gridOf, runsOf, runsOfLines, seg, wrapTo, write } from './art-canvas'
import type { Grid, Segment } from './art-canvas'
import { cutCell, widthOf } from './markdown'
import { placeParts } from './mermaid-system'
import type { ArchGroup, ArchIcon, Architecture, Block, BoxPart, BoxShape, C4, Link, Metrics, Placed } from './mermaid-system'
import type { Run, Tone } from './mermaid-art'

// The terminal drawings for the system kinds: block diagrams and C4 as boxes flowed into
// frames, architecture as boxes on its grid. A link becomes a connector where a straight
// run between the two boxes is clear; the rest, and every labelled one, are listed under
// the drawing.

const series = (i: number): Tone => ({ series: i })

type Cells = { x: number; y: number; w: number; h: number }
type Heads = { start: boolean; end: boolean }

// --- Boxes and connectors -----------------------------------------------------------

const ROUNDED: ReadonlySet<BoxShape> = new Set(['round', 'stadium', 'circle', 'cylinder', 'queue'])
const POINTED: ReadonlySet<BoxShape> = new Set(['diamond', 'hexagon'])

function cornersOf(shape: BoxShape, ascii: boolean): readonly string[] {
  if (POINTED.has(shape)) {
    return ['/', '\\', '\\', '/']
  }

  if (ROUNDED.has(shape)) {
    return ascii ? ['.', '.', "'", "'"] : ['╭', '╮', '╰', '╯']
  }

  return ascii ? ['+', '+', '+', '+'] : ['┌', '┐', '└', '┘']
}

// An outline with `tag` in its top edge; dashed for what lies outside the system.
function outline(grid: Grid, at: Cells, corners: readonly string[], isDashed: boolean, tone: Tone, ascii: boolean): void {
  const [tl = '+', tr = '+', bl = '+', br = '+'] = corners
  const [across, down] = isDashed ? (ascii ? ['.', ':'] : ['┄', '┆']) : ascii ? ['-', '|'] : ['─', '│']

  write(grid, at.x, at.y, `${tl}${across.repeat(Math.max(0, at.w - 2))}${tr}`, tone)

  for (let y = at.y + 1; y < at.y + at.h - 1; y++) {
    write(grid, at.x, y, down, tone)
    write(grid, at.x + at.w - 1, y, down, tone)
  }

  write(grid, at.x, at.y + at.h - 1, `${bl}${across.repeat(Math.max(0, at.w - 2))}${br}`, tone)
}

const isBlank = (grid: Grid, x: number, y: number): boolean => grid[y]?.[x]?.char === ' '

const LINES: Readonly<Record<Link['line'], readonly [string, string, string, string]>> = {
  solid: ['─', '│', '-', '|'],
  dotted: ['┄', '┆', '.', ':'],
  thick: ['━', '┃', '=', '|'],
}

type Spot = { x: number; y: number }
// Whether a connector may pass through a cell: an empty one, or a frame's edge.
type Open = (x: number, y: number) => boolean

// A straight connector from `a` to `b` across the clear cells between them, sideways when
// they share rows and up or down when they share columns. False when neither run is clear.
function connect(grid: Grid, a: Cells, b: Cells, heads: Heads, line: Link['line'], ascii: boolean, isOpen: Open): boolean {
  const [flat, tall, flatAscii, tallAscii] = LINES[line]
  const top = Math.max(a.y, b.y)
  const bottom = Math.min(a.y + a.h, b.y + b.h) - 1
  const left = Math.max(a.x, b.x)
  const right = Math.min(a.x + a.w, b.x + b.w) - 1

  if (top <= bottom && (a.x + a.w <= b.x || b.x + b.w <= a.x)) {
    const aFirst = a.x + a.w <= b.x
    const [from, to] = aFirst ? [a.x + a.w, b.x] : [b.x + b.w, a.x]
    const [back, ahead] = ascii ? ['<', '>'] : ['←', '→']

    // Inside the boxes' borders, middle row first.
    return outward(top, bottom).some(y => run(grid, Array.from({ length: to - from }, (_, i) => ({ x: from + i, y })), ascii ? flatAscii : flat, aFirst ? back : ahead, aFirst ? ahead : back, aFirst, heads, isOpen))
  }

  if (left <= right && (a.y + a.h <= b.y || b.y + b.h <= a.y)) {
    const aFirst = a.y + a.h <= b.y
    const [from, to] = aFirst ? [a.y + a.h, b.y] : [b.y + b.h, a.y]
    const [up, down] = ascii ? ['^', 'v'] : ['↑', '↓']

    return outward(left, right).some(x => run(grid, Array.from({ length: to - from }, (_, i) => ({ x, y: from + i })), ascii ? tallAscii : tall, aFirst ? up : down, aFirst ? down : up, aFirst, heads, isOpen))
  }

  return elbow(grid, a, b, heads, line, ascii, isOpen)
}

// From `lo` to `hi` middle first, then alternately either side of it; a range wider than a
// border keeps its two border cells out, so a connector meets a box's side, not its corner.
function outward(lo: number, hi: number): number[] {
  const [from, to] = hi - lo >= 2 ? [lo + 1, hi - 1] : [lo, hi]
  const middle = Math.floor((from + to) / 2)

  return Array.from({ length: (to - from + 1) * 2 }, (_, i) => middle + (i % 2 === 0 ? i / 2 : -(i + 1) / 2)).filter(at => at >= from && at <= to)
}

type Dir = 'up' | 'down' | 'left' | 'right'

const STEP: Readonly<Record<Dir, Spot>> = { up: { x: 0, y: -1 }, down: { x: 0, y: 1 }, left: { x: -1, y: 0 }, right: { x: 1, y: 0 } }
const HEAD: Readonly<Record<Dir, readonly [string, string]>> = { up: ['↑', '^'], down: ['↓', 'v'], left: ['←', '<'], right: ['→', '>'] }
const BACK: Readonly<Record<Dir, Dir>> = { up: 'down', down: 'up', left: 'right', right: 'left' }

// The corner joining the leg it came in on to the leg it leaves on, keyed by the two
// sides of the cell the line touches.
const CORNERS: Readonly<Record<string, readonly [string, string]>> = {
  'down,right': ['┌', '┏'],
  'down,left': ['┐', '┓'],
  'right,up': ['└', '┗'],
  'left,up': ['┘', '┛'],
}

// One turn from the middle of a side of `a` to the middle of a side of `b`, for boxes that
// share neither rows nor columns: down or up then across, or across then down or up. The
// corner and both heads must fall on empty cells, never on a frame's edge.
function elbow(grid: Grid, a: Cells, b: Cells, heads: Heads, line: Link['line'], ascii: boolean, isOpen: Open): boolean {
  const mid = (box: Cells): Spot => ({ x: box.x + Math.floor(box.w / 2), y: box.y + Math.floor(box.h / 2) })
  const [am, bm] = [mid(a), mid(b)]
  const below = bm.y > am.y
  const rightward = bm.x > am.x
  const tries: [Spot, Dir, Spot, Dir, Spot][] = [
    // Out of a's top or bottom, along b's middle row, into b's side.
    [{ x: am.x, y: below ? a.y + a.h : a.y - 1 }, below ? 'down' : 'up', { x: am.x, y: bm.y }, rightward ? 'right' : 'left', { x: rightward ? b.x - 1 : b.x + b.w, y: bm.y }],
    // Out of a's side, along b's middle column, into b's top or bottom.
    [{ x: rightward ? a.x + a.w : a.x - 1, y: am.y }, rightward ? 'right' : 'left', { x: bm.x, y: am.y }, below ? 'down' : 'up', { x: bm.x, y: below ? b.y - 1 : b.y + b.h }],
  ]
  const [flat, tall, flatAscii, tallAscii] = LINES[line]
  const bodyOf = (dir: Dir): string => (dir === 'up' || dir === 'down' ? (ascii ? tallAscii : tall) : ascii ? flatAscii : flat)

  for (const [start, first, turn, second, end] of tries) {
    const legOne = walk(start, turn, first)
    const legTwo = walk({ x: turn.x + STEP[second].x, y: turn.y + STEP[second].y }, end, second)

    if (legOne === null || legTwo === null || legTwo.length < 1 || (heads.start && legOne.length < 2)) {
      continue
    }

    const cells = [...legOne, ...legTwo]
    const corner = legOne.length - 1
    const ends = [cells[0], turn, cells[cells.length - 1]].filter((cell): cell is Spot => cell !== undefined)

    // A frame's edge may be crossed, one cell at a time, but not followed.
    const crosses = cells.every((cell, i) => isBlank(grid, cell.x, cell.y) || (isOpen(cell.x, cell.y) && [cells[i - 1], cells[i + 1]].every(next => next === undefined || isBlank(grid, next.x, next.y))))

    if (!crosses || !ends.every(cell => isBlank(grid, cell.x, cell.y))) {
      continue
    }

    const [plain, heavy] = CORNERS[[BACK[first], second].sort().join(',')] ?? ['+', '+']
    const bend = ascii ? '+' : line === 'thick' ? heavy : plain

    cells.forEach((cell, i) => {
      const glyph =
        i === cells.length - 1 && heads.end ? HEAD[second][ascii ? 1 : 0]
        : i === 0 && heads.start ? HEAD[BACK[first]][ascii ? 1 : 0]
        : i === corner ? bend
        : bodyOf(i < corner ? first : second)

      write(grid, cell.x, cell.y, glyph, 'muted')
    })

    return true
  }

  return false
}

// The cells from `from` to `to` inclusive, stepping `dir`; null when `to` is not ahead.
function walk(from: Spot, to: Spot, dir: Dir): Spot[] | null {
  const step = STEP[dir]
  const length = step.x === 0 ? (to.y - from.y) * step.y + 1 : (to.x - from.x) * step.x + 1

  if (length < 1 || (step.x === 0 ? from.x !== to.x : from.y !== to.y)) {
    return null
  }

  return Array.from({ length }, (_, i) => ({ x: from.x + step.x * i, y: from.y + step.y * i }))
}

// `cells` run from the box listed first in space; each end carries its head when asked.
function run(grid: Grid, cells: readonly Spot[], body: string, towardsA: string, towardsB: string, aFirst: boolean, heads: Heads, isOpen: Open): boolean {
  const needed = Math.max(1, Number(heads.start) + Number(heads.end))

  if (cells.length < needed || !cells.every(cell => isOpen(cell.x, cell.y))) {
    return false
  }

  const atA = aFirst ? 0 : cells.length - 1
  const atB = aFirst ? cells.length - 1 : 0

  cells.forEach((cell, i) => write(grid, cell.x, cell.y, i === atB && heads.end ? towardsB : i === atA && heads.start ? towardsA : body, 'muted'))

  return true
}

// `A ─ label ─→ B`, for a link the drawing could not hold or that carries words. When the
// line runs long the label gives way first, then the two names share what is left.
function listed(from: string, fromTone: Tone, to: string, toTone: Tone, label: string, heads: Heads, ascii: boolean, room: number): Segment[] {
  const [across, back, ahead] = ascii ? ['-', '<', '>'] : ['─', '←', '→']
  const arrow = ` ${heads.start ? back : across}${label === '' ? across : `${across} `}`
  const tail = `${label === '' ? '' : ` ${across}`}${heads.end ? ahead : across} `
  const fixed = widthOf(arrow) + widthOf(tail)
  const names = widthOf(from) + widthOf(to)
  const said = cutCell(label, Math.max(Math.min(widthOf(label), 10), room - fixed - names))
  const each = Math.max(4, Math.floor((room - fixed - widthOf(said)) / 2))
  const [fromRoom, toRoom] = names + fixed + widthOf(said) <= room ? [widthOf(from), widthOf(to)] : widthOf(from) < each ? [widthOf(from), room - fixed - widthOf(said) - widthOf(from)] : widthOf(to) < each ? [room - fixed - widthOf(said) - widthOf(to), widthOf(to)] : [each, each]

  return [seg(cutCell(from, Math.max(4, fromRoom)), fromTone), seg(arrow, 'muted'), seg(said, 'fg'), seg(tail, 'muted'), seg(cutCell(to, Math.max(4, toRoom)), toTone)]
}

// --- Block and C4 -------------------------------------------------------------------

type Lines = { label: string[]; kind: string; notes: string[] }

function linesOf(box: BoxPart, w: number): Lines {
  const room = Math.max(1, w - 4)
  const [kind = '', ...rest] = box.notes

  return { label: wrapTo(box.label, room, 2), kind: kind === '' ? '' : cutCell(kind, room), notes: wrapTo(rest.join(' '), room, 3) }
}

const heightOf = (lines: Lines): number => 2 + lines.label.length + (lines.kind === '' ? 0 : 1) + lines.notes.length

function cellsOf(placed: Placed): Cells {
  const x = Math.round(placed.x)

  return { x, y: placed.y, w: Math.round(placed.x + placed.w) - x, h: placed.h }
}

function drawBox(grid: Grid, placed: Placed & { part: BoxPart }, tone: Tone, isC4: boolean, ascii: boolean): void {
  const at = cellsOf(placed)
  const box = placed.part
  const lines = linesOf(box, at.w)
  const rows: Segment[] = [
    ...lines.label.map(line => seg(line, isC4 ? 'title' : 'fg')),
    ...(lines.kind === '' ? [] : [seg(lines.kind, 'muted')]),
    ...lines.notes.map(line => seg(line, 'fg')),
  ]
  const top = at.y + 1 + Math.floor((at.h - 2 - rows.length) / 2)

  outline(grid, at, cornersOf(box.shape, ascii), box.isExternal, tone, ascii)
  rows.forEach((row, i) => write(grid, at.x + Math.floor((at.w - widthOf(row.text)) / 2), top + i, row.text, row.tone))
}

function drawFrame(grid: Grid, placed: Placed, isDashed: boolean, ascii: boolean): void {
  const at = cellsOf(placed)
  const [name = '', type = ''] = placed.part.label.split(' · ')
  const room = at.w - 6

  outline(grid, at, cornersOf('box', ascii), isDashed, 'muted', ascii)

  if (name !== '' && room >= 4) {
    const shown = cutCell(name, room)
    const end = write(grid, at.x + 2, at.y, ` ${shown} `, 'fg')

    if (type !== '' && shown === name && widthOf(name) + widthOf(type) + 3 <= room) {
      write(grid, end - 1, at.y, ` · ${type} `, 'muted')
    }
  }
}

const FRAME_ACROSS: ReadonlySet<string> = new Set(['─', '┄', '-', '.'])
const FRAME_LINE: ReadonlySet<string> = new Set([...FRAME_ACROSS, '│', '┆', '|', ':'])
const FRAME_CORNER: ReadonlySet<string> = new Set(['┌', '┐', '└', '┘', '+'])

// The cells on a frame's outline, as `x,y` keys.
function edgeCells(at: Cells): string[] {
  const across = Array.from({ length: at.w }, (_, i) => [`${at.x + i},${at.y}`, `${at.x + i},${at.y + at.h - 1}`])
  const down = Array.from({ length: at.h }, (_, i) => [`${at.x},${at.y + i}`, `${at.x + at.w - 1},${at.y + i}`])

  return [...across, ...down].flat()
}

const boxesOf = (placed: readonly Placed[]): (Placed & { part: BoxPart })[] => placed.filter((item): item is Placed & { part: BoxPart } => item.part.type === 'box')

function systemArt(chart: Block | C4, room: number, ascii: boolean, sizes: { most: number; least: number; widest: number }, toneOf: (box: BoxPart) => Tone): Run[][] | null {
  const width = Math.min(room, sizes.widest)
  const metrics: Metrics = {
    gap: 4,
    rowGap: chart.links.length > 0 ? 2 : 1,
    // A blank row and column inside every frame, so a head crossing its edge lands clear.
    side: 2,
    head: 2,
    foot: 2,
    least: sizes.least,
    most: sizes.most,
    boxHeight: (box, w) => heightOf(linesOf(box, Math.round(w))),
  }
  const layout = placeParts(chart.parts, chart.kind === 'block' ? chart.columns : 'fit', 0, 0, width, metrics)

  if (layout === null || layout.height === 0) {
    return null
  }

  const grid = gridOf(width, layout.height)
  const byId = new Map(layout.placed.map(placed => [placed.part.id, placed]))
  const names = new Map(boxesOf(layout.placed).map(placed => [placed.part.id, { label: placed.part.label, tone: toneOf(placed.part) }]))

  const frames = layout.placed.filter(placed => placed.part.type === 'frame')
  const edges = new Set(frames.flatMap(placed => edgeCells(cellsOf(placed))))
  // On a frame's outline only its line is open, with outline either side along it: never a
  // corner, its name, or the cell beside the name.
  const charAt = (x: number, y: number): string => grid[y]?.[x]?.char ?? ''
  const isOpen: Open = (x, y) => {
    if (!edges.has(`${x},${y}`)) {
      return isBlank(grid, x, y)
    }

    const along = FRAME_ACROSS.has(charAt(x, y)) ? [charAt(x - 1, y), charAt(x + 1, y)] : [charAt(x, y - 1), charAt(x, y + 1)]

    return FRAME_LINE.has(charAt(x, y)) && along.every(char => FRAME_LINE.has(char) || FRAME_CORNER.has(char))
  }

  frames.forEach(placed => drawFrame(grid, placed, chart.kind === 'c4', ascii))
  boxesOf(layout.placed).forEach(placed => drawBox(grid, placed, toneOf(placed.part), chart.kind === 'c4', ascii))

  const notes = chart.links.flatMap(link => {
    const from = byId.get(link.from)
    const to = byId.get(link.to)
    const heads = { start: link.arrow === 'both', end: link.arrow !== 'none' }
    const isDrawn = from !== undefined && to !== undefined && connect(grid, cellsOf(from), cellsOf(to), heads, link.line, ascii, isOpen)

    if (isDrawn && link.label === '') {
      return []
    }

    const a = names.get(link.from) ?? { label: from?.part.label || link.from, tone: 'muted' as const }
    const b = names.get(link.to) ?? { label: to?.part.label || link.to, tone: 'muted' as const }

    return [listed(a.label, a.tone, b.label, b.tone, link.label, heads, ascii, width)]
  })

  return notes.length === 0 ? runsOf(grid) : [...runsOf(grid), [], ...runsOfLines(notes)]
}

export function blockArt(chart: Block, room: number, ascii: boolean): Run[][] | null {
  return systemArt(chart, room, ascii, { most: 4, least: 10, widest: 96 }, box => series(box.tone))
}

export function c4Art(chart: C4, room: number, ascii: boolean): Run[][] | null {
  return systemArt(chart, room, ascii, { most: 3, least: 22, widest: 110 }, box => (box.isExternal ? 'muted' : series(box.tone)))
}

// --- Architecture -------------------------------------------------------------------

const ICON_TAG: Readonly<Record<ArchIcon, string>> = { cloud: 'cloud', database: 'db', disk: 'disk', internet: 'web', server: 'server', generic: '' }

export function architectureArt(chart: Architecture, room: number, ascii: boolean): Run[][] | null {
  // Narrow terminals trade a column of gap, then the padding inside a box, for name room.
  const cellAt = (gap: number) => Math.min(24, Math.floor((room - gap * (chart.columns - 1)) / chart.columns))
  const gap = cellAt(4) >= 14 ? 4 : 3
  const cell = cellAt(gap)

  if (cell < 10) {
    return null
  }

  const labels = new Map(chart.services.map(service => [service.id, service.isJunction ? [] : wrapTo(service.label, cell - 2, 2)]))
  const rowH = 2 + Math.max(1, ...[...labels.values()].map(lines => lines.length))
  const rowGap = 2
  const grid = gridOf(chart.columns * cell + gap * (chart.columns - 1), chart.rows * rowH + rowGap * (chart.rows - 1))
  const groupOf = new Map(chart.groups.map(group => [group.id, group]))

  // Each group its own colour, so a nested group stands apart from the one around it.
  const toneOf = (id: string | null): Tone => series(id === null ? 0 : chart.groups.findIndex(group => group.id === id) + 1)

  // Each box as wide as its name needs, centred in its cell; a junction is one dot.
  const at = new Map(
    chart.services.map(service => {
      const left = service.col * (cell + gap)
      const top = service.row * (rowH + rowGap)
      const lines = labels.get(service.id) ?? []
      const w = Math.min(cell, Math.max(8, ...lines.map(line => widthOf(line) + 4), widthOf(ICON_TAG[service.icon]) + 4))

      return [service.id, service.isJunction ? { x: left + Math.floor(cell / 2), y: top + Math.floor(rowH / 2), w: 1, h: 1 } : { x: left + Math.floor((cell - w) / 2), y: top, w, h: rowH }]
    }),
  )

  for (const service of chart.services) {
    const cells = at.get(service.id)

    if (cells === undefined) {
      continue
    }

    if (service.isJunction) {
      write(grid, cells.x, cells.y, ascii ? '*' : '●', 'muted')
      continue
    }

    const tone = toneOf(service.group)
    const tag = ICON_TAG[service.icon]
    const lines = labels.get(service.id) ?? []

    outline(grid, cells, cornersOf(service.icon === 'database' ? 'cylinder' : 'box', ascii), false, tone, ascii)

    if (tag !== '' && widthOf(tag) <= cells.w - 4) {
      write(grid, cells.x + 2, cells.y, tag, 'muted')
    }

    lines.forEach((line, i) => write(grid, cells.x + Math.floor((cells.w - widthOf(line)) / 2), cells.y + 1 + Math.floor((rowH - 2 - lines.length) / 2) + i, line, 'fg'))
  }

  const named = new Map(chart.services.map(service => [service.id, service]))
  const notes = chart.edges.flatMap(edge => {
    const from = at.get(edge.from)
    const to = at.get(edge.to)
    const heads = { start: edge.arrow === 'from' || edge.arrow === 'both', end: edge.arrow === 'to' || edge.arrow === 'both' }
    const isDrawn = from !== undefined && to !== undefined && connect(grid, from, to, heads, 'solid', ascii, (x, y) => isBlank(grid, x, y))

    if (isDrawn && edge.label === '') {
      return []
    }

    const a = named.get(edge.from)
    const b = named.get(edge.to)
    const nameOf = (service: typeof a, id: string) => (service === undefined ? id : service.isJunction ? (ascii ? '*' : '●') : service.label)

    return [listed(nameOf(a, edge.from), toneOf(a?.group ?? null), nameOf(b, edge.to), toneOf(b?.group ?? null), edge.label, heads, ascii, grid[0]?.length ?? room)]
  })

  // The groups in their colours, a nested one after the group it sits in.
  const pathOf = (group: ArchGroup): string => (group.parent === null ? group.label : `${pathOf(groupOf.get(group.parent) ?? group)}${ascii ? ' > ' : ' › '}${group.label}`)
  const legend = chart.groups.map(group => [seg(ascii ? '#' : '■', toneOf(group.id)), seg(` ${cutCell(pathOf(group), (grid[0]?.length ?? room) - 2)}`, 'fg')])

  return [...runsOf(grid), ...(legend.length > 0 ? [[], ...runsOfLines(legend)] : []), ...(notes.length > 0 ? [[], ...runsOfLines(notes)] : [])]
}
