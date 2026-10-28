// Ported from skins by hellosverre (MIT; LICENSE beside this file),
// https://github.com/hellosverre/claude-skins at 985ad63, hooks/chart-art-xy.ts.

import { dotsOf, gridOf, line, runsOf, seg, write, writeAll } from './art-canvas'
import type { Dots, Segment } from './art-canvas'
import { cutCell, widthOf } from './markdown'
import { formatValue, niceStep, seriesName } from './mermaid'
import type { XyChart } from './mermaid'
import type { Run, Tone } from './mermaid-art'

// An xy chart in cells: bars as columns topped to the eighth of a cell, lines in braille
// over them, ticks up the left and the labels under each slot. See chart-art.ts for how
// it is chosen.

const series = (i: number): Tone => ({ series: i })

const PLOT_ROWS = 12
const SLOT_MAX = 10
const TOPS = ['', '▁', '▂', '▃', '▄', '▅', '▆', '▇']
// In ASCII each bar series gets its own mark, so two of them read apart without colour.
const MARKS = ['#', '%', '@', '=', '+', 'o']

const markOf = (j: number): string => MARKS[j % MARKS.length] ?? '#'

export function xyArt(chart: XyChart, room: number, ascii: boolean): Run[][] | null {
  const points = Math.max(...chart.series.map(one => one.values.length))
  const bars = chart.series.filter(one => one.kind === 'bar')
  const span = chart.max - chart.min
  const step = niceStep(span, 4)
  const ticks: number[] = []

  for (let at = Math.ceil(chart.min / step) * step; at <= chart.max + step / 1e6; at += step) {
    ticks.push(at)
  }

  const axisW = Math.max(...ticks.map(at => widthOf(formatValue(at)))) + 1
  const slotW = Math.min(SLOT_MAX, Math.floor((room - axisW - 1) / points))
  // Each bar series a column of its own in the slot, together about three fifths of it so
  // the slots read apart; at least a cell left between them.
  const barW = bars.length === 0 ? 0 : Math.min(Math.floor((slotW - 1) / bars.length), Math.max(1, Math.round((slotW * 0.6) / bars.length)))

  if (points === 0 || slotW < 3 || (bars.length > 0 && barW < 1)) {
    return null
  }

  const plotW = slotW * points
  const top = chart.yLabel === '' ? 0 : 2
  const grid = gridOf(axisW + 1 + plotW, top + PLOT_ROWS + 2)
  const share = (value: number): number => Math.max(0, Math.min(1, (value - chart.min) / span))
  const rowOf = (value: number): number => top + PLOT_ROWS - 1 - Math.min(PLOT_ROWS - 1, Math.floor(share(value) * PLOT_ROWS))

  if (top > 0) {
    write(grid, 0, 0, cutCell(chart.yLabel, axisW + 1 + plotW), 'muted')
  }

  // The tick rows first, dotted across, so the bars and lines draw over them.
  for (let y = top; y < top + PLOT_ROWS; y++) {
    write(grid, axisW, y, ascii ? '|' : '│', 'muted')
  }

  for (const at of ticks) {
    const y = rowOf(at)

    write(grid, axisW - 1 - widthOf(formatValue(at)), y, formatValue(at), 'muted')
    write(grid, axisW, y, ascii ? '+' : '┤', 'muted')
    write(grid, axisW + 1, y, (ascii ? '.' : '·').repeat(plotW), 'muted')
  }

  const left = axisW + 1
  const full = new Set<string>()

  bars.forEach((one, j) => {
    const tone = series(chart.series.indexOf(one))
    const x = left + Math.floor((slotW - barW * bars.length) / 2) + j * barW

    one.values.forEach((value, i) => {
      const eighths = Math.round(share(value) * PLOT_ROWS * 8)

      for (let r = 0; r * 8 < eighths; r++) {
        const left8 = eighths - r * 8
        const char = ascii ? (left8 >= 4 ? markOf(j) : '') : left8 >= 8 ? '█' : (TOPS[left8] ?? '')
        const y = top + PLOT_ROWS - 1 - r

        if (char !== '') {
          write(grid, x + i * slotW, y, char.repeat(barW), tone)
        }

        if (left8 >= 8) {
          for (let c = 0; c < barW; c++) {
            full.add(`${x + i * slotW + c},${y}`)
          }
        }
      }
    })
  })

  const dots = dotsOf(plotW, PLOT_ROWS)
  const dotOf = (value: number, i: number): [number, number] => [(i * slotW + slotW / 2) * 2 - 1, (PLOT_ROWS * 4 - 1) * (1 - share(value))]

  chart.series.forEach((one, j) => {
    if (one.kind !== 'line') {
      return
    }

    for (let i = 0; i + 1 < one.values.length; i++) {
      line(dots, ...dotOf(one.values[i] ?? chart.min, i), ...dotOf(one.values[i + 1] ?? chart.min, i + 1), series(j))
    }

    if (one.values.length === 1) {
      line(dots, ...dotOf(one.values[0] ?? chart.min, 0), ...dotOf(one.values[0] ?? chart.min, 0), series(j))
    }
  })

  stampOver(grid, dots, left, top, full, ascii)

  // The baseline, then each slot's label under its middle, every other one where they crowd.
  const base = top + PLOT_ROWS
  write(grid, axisW, base, `${ascii ? '+' : '└'}${(ascii ? '-' : '─').repeat(plotW)}`, 'muted')

  const labelOf = (i: number): string => chart.labels[i] ?? String(i + 1)
  const widest = Math.max(...Array.from({ length: points }, (_, i) => widthOf(labelOf(i))))
  const every = Math.max(1, Math.ceil((Math.min(widest, 12) + 1) / slotW))

  for (let i = 0; i < points; i += every) {
    const label = cutCell(labelOf(i), slotW * every - 1)
    const middle = left + i * slotW + Math.floor(slotW / 2)

    write(grid, Math.max(left, middle - Math.floor(widthOf(label) / 2)), base + 1, label, 'fg')
  }

  return [...runsOf(grid), ...legendOf(chart, ascii)]
}

// A line crossing a full bar cell keeps the bar behind it as the background, so the bar
// does not break where the line runs through.
function stampOver(grid: ReturnType<typeof gridOf>, dots: Dots, x: number, y: number, full: ReadonlySet<string>, ascii: boolean): void {
  dots.bits.forEach((bits, cy) =>
    bits.forEach((bit, cx) => {
      const tone = dots.tones[cy]?.[cx] ?? null
      const under = grid[y + cy]?.[x + cx]

      if (bit === 0 || tone === null) {
        return
      }

      const back = !ascii && full.has(`${x + cx},${y + cy}`) ? (under?.tone ?? undefined) : undefined

      write(grid, x + cx, y + cy, ascii ? '*' : String.fromCodePoint(0x2800 + bit), tone, back)
    }),
  )
}

// Names only where they say something: a series named in the source, or two of a kind
// that would otherwise be told apart by colour alone.
function legendOf(chart: XyChart, ascii: boolean): Run[][] {
  const kinds = chart.series.map(one => one.kind)
  const isNeeded = chart.series.some(one => one.name !== '') || kinds.some((kind, i) => kinds.indexOf(kind) !== i)

  if (!isNeeded) {
    return []
  }

  const bars = chart.series.filter(one => one.kind === 'bar')
  const entries = chart.series.flatMap((one, j): Segment[] => [
    ...(j === 0 ? [] : [seg('   ')]),
    seg(one.kind === 'line' ? (ascii ? '-*' : '─●') : ascii ? markOf(bars.indexOf(one)) : '■', series(j)),
    seg(` ${seriesName(chart, j)}`, 'fg'),
  ])
  const grid = gridOf(entries.reduce((sum, part) => sum + widthOf(part.text), 0), 1)

  writeAll(grid, 0, 0, entries)

  return [[], ...runsOf(grid)]
}
