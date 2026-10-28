// Ported from skins by hellosverre (MIT; LICENSE beside this file),
// https://github.com/hellosverre/claude-skins at 985ad63, hooks/art-canvas.ts.

import { cutCell, widthOf } from './markdown'
import { sameTone } from './mermaid-art'
import type { Run, Tone } from './mermaid-art'

// The cell grid the mod's own terminal charts draw on before they become runs: text at
// a column and row, half blocks with a background, and a braille layer for lines finer
// than a cell.

export interface Cell {
  char: string
  tone: Tone | null
  back?: Tone
}

export type Grid = Cell[][]

const BLANK: Cell = { char: ' ', tone: null }

export const gridOf = (width: number, height: number): Grid =>
  Array.from({ length: height }, () => Array.from({ length: width }, () => BLANK))

// `text` from column `x` on row `y`, clipped at the grid's edge. A wide character takes
// two cells, the second left empty so the row keeps its width. Returns the next column.
export function write(grid: Grid, x: number, y: number, text: string, tone: Tone | null, back?: Tone): number {
  const row = grid[y]
  let at = x

  if (row === undefined) {
    return x + widthOf(text)
  }

  for (const char of text) {
    const wide = widthOf(char) === 2

    if (at >= 0 && at + (wide ? 1 : 0) < row.length) {
      row[at] = back === undefined ? { char, tone } : { char, tone, back }

      if (wide) {
        row[at + 1] = { char: '', tone }
      }
    }

    at += wide ? 2 : 1
  }

  return at
}

// The grid as runs: neighbours that share a tone and background merged, trailing blanks
// off each row, blank rows off both ends.
export function runsOf(grid: Grid): Run[][] {
  const rows = grid.map(cells => {
    let end = cells.length

    while (end > 0 && cells[end - 1]?.char === ' ' && cells[end - 1]?.back === undefined) {
      end--
    }

    const runs: Run[] = []

    for (const cell of cells.slice(0, end)) {
      const last = runs[runs.length - 1]
      // A blank takes the tone before it, so `3  75%` stays one run.
      const tone = cell.char === ' ' && cell.back === undefined ? (last?.tone ?? null) : cell.tone

      if (last !== undefined && sameTone(last.tone, tone) && sameTone(last.back, cell.back)) {
        last.text += cell.char
      } else {
        runs.push(cell.back === undefined ? { text: cell.char, tone } : { text: cell.char, tone, back: cell.back })
      }
    }

    return runs
  })

  while (rows.length > 0 && rows[0]?.length === 0) {
    rows.shift()
  }

  while (rows.length > 0 && rows[rows.length - 1]?.length === 0) {
    rows.pop()
  }

  return rows
}

// --- Braille ------------------------------------------------------------------------

// Each cell holds 2 × 4 dots; a cell takes the tone of the last dot set in it, so what
// is drawn last (the curves over the grid) wins.
export interface Dots {
  cols: number
  rows: number
  bits: number[][]
  tones: (Tone | null)[][]
}

const DOT_BIT = [
  [0x01, 0x08],
  [0x02, 0x10],
  [0x04, 0x20],
  [0x40, 0x80],
] as const

export const dotsOf = (cols: number, rows: number): Dots => ({
  cols,
  rows,
  bits: Array.from({ length: rows }, () => Array.from({ length: cols }, () => 0)),
  tones: Array.from({ length: rows }, () => Array.from({ length: cols }, () => null)),
})

// One dot at (`px`, `py`) in dot space: `cols * 2` across, `rows * 4` down.
export function dot(dots: Dots, px: number, py: number, tone: Tone): void {
  const x = Math.round(px)
  const y = Math.round(py)
  const cx = Math.floor(x / 2)
  const cy = Math.floor(y / 4)
  const bit = DOT_BIT[y - cy * 4]?.[x - cx * 2]

  if (bit === undefined || cx < 0 || cy < 0 || cx >= dots.cols || cy >= dots.rows) {
    return
  }

  const bits = dots.bits[cy] ?? []
  bits[cx] = (bits[cx] ?? 0) | bit
  const tones = dots.tones[cy] ?? []
  tones[cx] = tone
}

export function line(dots: Dots, x0: number, y0: number, x1: number, y1: number, tone: Tone): void {
  const steps = Math.max(1, Math.ceil(Math.max(Math.abs(x1 - x0), Math.abs(y1 - y0))))

  for (let i = 0; i <= steps; i++) {
    dot(dots, x0 + ((x1 - x0) * i) / steps, y0 + ((y1 - y0) * i) / steps, tone)
  }
}

// The dots onto `grid` at (`x`, `y`). In ASCII a dotted cell is `.` for the quiet layer
// and `*` for the rest.
export function stamp(grid: Grid, dots: Dots, x: number, y: number, ascii: boolean): void {
  dots.bits.forEach((bits, cy) =>
    bits.forEach((bit, cx) => {
      const tone = dots.tones[cy]?.[cx] ?? null

      if (bit !== 0) {
        write(grid, x + cx, y + cy, ascii ? (tone === 'muted' ? '.' : '*') : String.fromCodePoint(0x2800 + bit), tone)
      }
    }),
  )
}

// --- Text ---------------------------------------------------------------------------

export interface Segment {
  text: string
  tone: Tone | null
}

// Segments one after another from column `x`, each in its own tone.
export const writeAll = (grid: Grid, x: number, y: number, segments: readonly Segment[]): number =>
  segments.reduce((at, segment) => write(grid, at, y, segment.text, segment.tone), x)

// `text` cut to `width` cells and padded out to them.
export function padTo(text: string, width: number): string {
  const cut = cutCell(text, width)

  return cut + ' '.repeat(Math.max(0, width - widthOf(cut)))
}

// `text` wrapped at spaces to `width` cells, `most` lines at most, the last cut with an
// ellipsis where text is left over.
export function wrapTo(text: string, width: number, most: number): string[] {
  const lines: string[] = []
  let current = ''

  for (const word of text.split(' ').filter(word => word !== '')) {
    const next = current === '' ? word : `${current} ${word}`

    if (widthOf(next) <= width || current === '') {
      current = next
    } else {
      lines.push(current)
      current = word
    }
  }

  if (current !== '') {
    lines.push(current)
  }

  const kept = lines.slice(0, most)

  if (lines.length > most) {
    kept[most - 1] = `${kept[most - 1] ?? ''} ${lines.slice(most).join(' ')}`
  }

  return kept.map(line => cutCell(line, width))
}

// Rows of segments as runs, each row as wide as its segments.
export function runsOfLines(lines: readonly (readonly Segment[])[]): Run[][] {
  const width = Math.max(1, ...lines.map(segments => segments.reduce((sum, segment) => sum + widthOf(segment.text), 0)))
  const grid = gridOf(width, lines.length)
  lines.forEach((segments, y) => writeAll(grid, 0, y, segments))

  return runsOf(grid)
}

// `from` copied onto `grid` with its top left at (`x`, `y`).
export function blit(grid: Grid, from: Grid, x: number, y: number): void {
  from.forEach((cells, dy) =>
    cells.forEach((cell, dx) => {
      const row = grid[y + dy]

      if (row !== undefined && x + dx >= 0 && x + dx < row.length) {
        row[x + dx] = cell
      }
    }),
  )
}

export const seg = (text: string, tone: Tone | null = null): Segment => ({ text, tone })
