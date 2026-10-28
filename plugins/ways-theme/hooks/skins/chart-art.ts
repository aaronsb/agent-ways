// Ported from skins by hellosverre (MIT; LICENSE beside this file),
// https://github.com/hellosverre/claude-skins at 985ad63, hooks/chart-art.ts.

import { blit, gridOf, padTo, runsOf, runsOfLines, seg, wrapTo, write, writeAll } from './art-canvas'
import type { Grid, Segment } from './art-canvas'
import { gitArt, mindmapArt, quadrantArt, radarArt, sankeyArt } from './chart-art-shape'
import { packetArt, treemapArt } from './chart-art-data'
import { architectureArt, blockArt, c4Art } from './chart-art-system'
import { xyArt } from './chart-art-xy'
import { cutCell, widthOf } from './markdown'
import { chartHeading, formatPercent, formatValue } from './mermaid'
import type { Chart, Gantt, Journey, Kanban, Pie, Timeline } from './mermaid'
import type { Art, Run, Tone } from './mermaid-art'

// The mod's own terminal drawings: a donut for pies, bars and lines for xy charts, then
// the planning charts and the shapes; the vendored renderer keeps flowcharts. Each answers null when it
// cannot fit `columns`, and the caller falls back to bars or to the code.

export function chartArt(chart: Chart, columns: number, ascii: boolean): Art | null {
  const drawnRows = drawn(chart, columns - 2, ascii)
  // A label cut short ends in `…` and parts of a line are kept apart by `·`; plain ASCII
  // spells them as a dot and a dash.
  const rows = ascii ? drawnRows?.map(row => row.map(run => ({ ...run, text: run.text.replaceAll('…', '.').replaceAll('·', '-') }))) : drawnRows

  return rows === null || rows === undefined || rows.length === 0 ? null : { kind: chartHeading(chart).kind, title: chart.title, rows }
}

function drawn(chart: Chart, room: number, ascii: boolean): Run[][] | null {
  switch (chart.kind) {
    case 'pie':
      return pieArt(chart, room, ascii)
    case 'gantt':
      return ganttArt(chart, room, ascii)
    case 'timeline':
      return timelineArt(chart, room, ascii)
    case 'journey':
      return journeyArt(chart, room, ascii)
    case 'kanban':
      return kanbanArt(chart, room, ascii)
    case 'mindmap':
      return mindmapArt(chart, room, ascii)
    case 'quadrant':
      return quadrantArt(chart, room, ascii)
    case 'radar':
      return radarArt(chart, room, ascii)
    case 'sankey':
      return sankeyArt(chart, room, ascii)
    case 'git':
      return gitArt(chart, room, ascii)
    case 'treemap':
      return treemapArt(chart, room, ascii)
    case 'packet':
      return packetArt(chart, room, ascii)
    case 'block':
      return blockArt(chart, room, ascii)
    case 'architecture':
      return architectureArt(chart, room, ascii)
    case 'c4':
      return c4Art(chart, room, ascii)
    case 'xy':
      return xyArt(chart, room, ascii)
    case 'flow':
      return null
  }
}

export const series = (i: number): Tone => ({ series: i })

const LABEL_MAX = 24

// --- Pie ----------------------------------------------------------------------------

// Cells across; in half blocks the disc is as tall as it is wide.
const PIE = 16

// In ASCII each slice gets its own mark, so the pie reads without colour.
const PIE_MARKS = ['#', '%', '@', '=', '+', 'o', '~', ':']

function pieArt(pie: Pie, room: number, ascii: boolean): Run[][] | null {
  const total = pie.slices.reduce((sum, slice) => sum + slice.value, 0)

  if (total <= 0 || room < PIE) {
    return null
  }

  let sum = 0
  const ends = pie.slices.map(slice => (sum += slice.value) / total)
  const radius = PIE / 2

  // The slice under a point in pixels, clockwise from twelve o'clock; null off the ring.
  const sliceAt = (x: number, y: number): number | null => {
    const dx = x - radius
    const dy = y - radius
    const distance = Math.hypot(dx, dy)

    if (distance > radius || distance < radius / 2) {
      return null
    }

    const turn = (Math.atan2(dx, -dy) / (2 * Math.PI) + 1) % 1
    const i = ends.findIndex(end => turn < end)

    return i === -1 ? ends.length - 1 : i
  }

  const disc = gridOf(PIE, PIE / 2)

  for (let cy = 0; cy < PIE / 2; cy++) {
    for (let cx = 0; cx < PIE; cx++) {
      if (ascii) {
        const at = sliceAt(cx + 0.5, cy * 2 + 1)

        if (at !== null) {
          write(disc, cx, cy, PIE_MARKS[at % PIE_MARKS.length] ?? '#', series(at))
        }

        continue
      }

      const top = sliceAt(cx + 0.5, cy * 2 + 0.5)
      const bottom = sliceAt(cx + 0.5, cy * 2 + 1.5)

      if (top !== null && top === bottom) {
        write(disc, cx, cy, '█', series(top))
      } else if (top !== null && bottom !== null) {
        write(disc, cx, cy, '▀', series(top), series(bottom))
      } else if (top !== null) {
        write(disc, cx, cy, '▀', series(top))
      } else if (bottom !== null) {
        write(disc, cx, cy, '▄', series(bottom))
      }
    }
  }

  const labelW = Math.min(LABEL_MAX, Math.max(...pie.slices.map(slice => widthOf(slice.label))))
  const legend = pie.slices.map((slice, i): Segment[] => {
    const share = formatPercent(slice.value / total)

    return [
      seg(ascii ? (PIE_MARKS[i % PIE_MARKS.length] ?? '#') : '■', series(i)),
      seg(` ${padTo(slice.label, labelW)}  `, 'fg'),
      seg(total === 100 ? share : `${formatValue(slice.value)}  ${share}`, 'muted'),
    ]
  })
  const legendW = Math.max(...legend.map(widthOfLine))
  const discRows = PIE / 2

  // Beside the disc and centred on it where both fit, under it otherwise.
  if (PIE + 3 + legendW <= room) {
    const grid = gridOf(PIE + 3 + legendW, Math.max(discRows, legend.length))
    blit(grid, disc, 0, Math.max(0, Math.floor((legend.length - discRows) / 2)))
    const top = Math.max(0, Math.floor((discRows - legend.length) / 2))
    legend.forEach((segments, i) => writeAll(grid, PIE + 3, top + i, segments))

    return runsOf(grid)
  }

  const grid = gridOf(room, discRows + 1 + legend.length)
  blit(grid, disc, 0, 0)
  legend.forEach((segments, i) => writeAll(grid, 0, discRows + 1 + i, segments))

  return runsOf(grid)
}

export const widthOfLine = (segments: readonly Segment[]): number =>
  segments.reduce((sum, segment) => sum + widthOf(segment.text), 0)

// --- Gantt --------------------------------------------------------------------------

const MINUTE = 60_000
export const HOUR = 60 * MINUTE
const DAY = 24 * HOUR
const STEPS = [HOUR, 2 * HOUR, 3 * HOUR, 6 * HOUR, 12 * HOUR, DAY, 2 * DAY, 7 * DAY, 14 * DAY]
const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec']

export interface Tick {
  at: number
  label: string
}

const twoDigits = (n: number): string => String(n).padStart(2, '0')

const dayOf = (at: number): string => {
  const date = new Date(at)

  return `${MONTHS[date.getUTCMonth()] ?? ''} ${date.getUTCDate()}`
}

// Ticks a label apart at most every eight columns: hours, days, weeks, then whole
// calendar months.
export function ticksOf(min: number, max: number, width: number): Tick[] {
  const most = Math.max(2, Math.floor(width / 8))
  const step = STEPS.find(candidate => (max - min) / candidate <= most)
  const ticks: Tick[] = []

  if (step !== undefined) {
    for (let at = Math.ceil(min / step) * step; at <= max; at += step) {
      const date = new Date(at)
      ticks.push({ at, label: step < DAY ? `${twoDigits(date.getUTCHours())}:${twoDigits(date.getUTCMinutes())}` : dayOf(at) })
    }

    return ticks
  }

  const start = new Date(min)
  const onFirst = min === Date.UTC(start.getUTCFullYear(), start.getUTCMonth(), 1)
  const first = start.getUTCFullYear() * 12 + start.getUTCMonth() + (onFirst ? 0 : 1)
  const months = (max - min) / (30 * DAY)
  const every = [1, 3, 6, 12].find(candidate => months / candidate <= most) ?? Math.ceil(months / most / 12) * 12

  for (let month = Math.ceil(first / every) * every; ; month += every) {
    const year = Math.floor(month / 12)
    const at = Date.UTC(year, month % 12, 1)

    if (at > max) {
      break
    }

    ticks.push({ at, label: every >= 12 ? String(year) : `${MONTHS[month % 12] ?? ''} ${year}` })
  }

  return ticks
}

export function lengthOf(ms: number): string {
  const [unit, size] = ms >= DAY ? ['d', DAY] : ms >= HOUR ? ['h', HOUR] : ['m', MINUTE]
  const count = ms / size

  return `${Number(count.toFixed(count < 10 && count % 1 !== 0 ? 1 : 0))}${unit}`
}

function ganttArt(chart: Gantt, room: number, ascii: boolean): Run[][] | null {
  const tasks = chart.tasks

  if (tasks.length === 0) {
    return null
  }

  const min = Math.min(...tasks.map(task => task.start))
  const max = Math.max(min + HOUR, ...tasks.map(task => task.end))
  const span = max - min
  const grouped = tasks.some(task => task.section !== '')
  const indent = grouped ? 2 : 0
  const labelW = Math.min(20, Math.max(...tasks.map(task => indent + widthOf(task.label))))
  const lengths = tasks.map(task => (task.milestone ? '' : lengthOf(task.end - task.start)))
  const lengthW = Math.max(0, ...lengths.map(widthOf))
  const left = labelW + 2
  const width = room - left - (lengthW > 0 ? lengthW + 2 : 0)

  if (width < 10) {
    return null
  }

  const xOf = (at: number): number => ((at - min) / span) * width
  const ticks = ticksOf(min, max, width).map(tick => ({ ...tick, x: Math.min(width - 1, Math.round(xOf(tick.at))) }))
  const sections = grouped ? new Set(tasks.map(task => task.section)).size : 0
  const grid = gridOf(room, 2 + tasks.length + sections)
  let end = -1

  for (const tick of ticks) {
    if (left + tick.x > end && left + tick.x + widthOf(tick.label) <= room) {
      end = write(grid, left + tick.x, 0, tick.label, 'muted')
    }
  }

  write(grid, left, 1, (ascii ? '-' : '─').repeat(width), 'muted')
  ticks.forEach(tick => write(grid, left + tick.x, 1, ascii ? '+' : '┬', 'muted'))

  let y = 2
  let section: string | null = null

  tasks.forEach((task, i) => {
    const sectionAt = Math.max(0, chart.sections.indexOf(task.section))

    if (grouped && task.section !== section) {
      section = task.section
      write(grid, 0, y++, cutCell(task.section === '' ? '—' : task.section, room), series(sectionAt))
    }

    write(grid, indent, y, cutCell(task.label, labelW - indent), 'fg')
    ticks.forEach(tick => write(grid, left + tick.x, y, ascii ? '.' : '·', 'muted'))

    const from = Math.min(width - 1, Math.max(0, Math.floor(xOf(task.start))))

    if (task.milestone) {
      write(grid, left + from, y, ascii ? '*' : '◆', { slot: 'warn' })
    } else {
      const to = Math.max(from + 1, Math.min(width, Math.round(xOf(task.end))))
      const tone: Tone = task.crit ? { slot: 'err' } : task.done ? 'muted' : series(sectionAt)
      write(grid, left + from, y, (ascii ? (task.done ? '=' : '#') : '█').repeat(to - from), tone)
    }

    write(grid, left + width + 2, y, lengths[i] ?? '', 'muted')
    y++
  })

  return runsOf(grid)
}

// --- Timeline -----------------------------------------------------------------------

function timelineArt(chart: Timeline, room: number, ascii: boolean): Run[][] | null {
  const periods = chart.periods

  if (periods.length === 0) {
    return null
  }

  const labelW = Math.min(16, Math.max(...periods.map(period => widthOf(period.label))))
  const textW = room - labelW - 4

  if (textW < 8) {
    return null
  }

  const [node, rail] = ascii ? ['o', '|'] : ['●', '│']
  const gutter = ' '.repeat(labelW + 2)
  const lines: Segment[][] = []
  let section = ''

  periods.forEach((period, i) => {
    const tone = series(chart.sections.length > 0 ? Math.max(0, chart.sections.indexOf(period.section)) : i)

    if (period.section !== section) {
      section = period.section

      if (lines.length > 0) {
        lines.push([])
      }

      lines.push([seg(cutCell(section, room), tone)])
    } else if (i > 0) {
      lines.push([seg(gutter), seg(rail, tone)])
    }

    const events = period.events.length > 0 ? period.events : ['']

    events.forEach((event, k) =>
      lines.push([
        seg(k === 0 ? padTo(period.label, labelW) : ' '.repeat(labelW), 'title'),
        seg('  '),
        seg(k === 0 ? node : rail, tone),
        seg(event === '' ? '' : ` ${cutCell(event, textW)}`, 'fg'),
      ]),
    )
  })

  return runsOfLines(lines)
}

// --- Journey ------------------------------------------------------------------------

const scoreTone = (score: number): Tone => ({ slot: score >= 4 ? 'ok' : score === 3 ? 'warn' : 'err' })

function journeyArt(chart: Journey, room: number, ascii: boolean): Run[][] | null {
  const steps = chart.steps

  if (steps.length === 0) {
    return null
  }

  const grouped = steps.some(step => step.section !== '')
  const indent = grouped ? 2 : 0
  const labelW = Math.min(LABEL_MAX, Math.max(...steps.map(step => indent + widthOf(step.label))))

  if (room < labelW + 10) {
    return null
  }

  const [full, empty] = ascii ? ['#', '.'] : ['●', '○']
  const lines: Segment[][] = []
  let section: string | null = null

  for (const step of steps) {
    if (grouped && step.section !== section) {
      section = step.section

      if (lines.length > 0) {
        lines.push([])
      }

      lines.push([seg(cutCell(section, room), series(Math.max(0, chart.sections.indexOf(section))))])
    }

    const score = Math.max(0, Math.min(5, Math.round(step.score)))
    const actors = step.actors.join(', ')
    const rest = room - labelW - 12

    lines.push([
      seg(' '.repeat(indent) + padTo(step.label, labelW - indent), 'fg'),
      seg('  '),
      seg(full.repeat(score), scoreTone(score)),
      seg(empty.repeat(5 - score), 'muted'),
      seg(`  ${score}`, 'muted'),
      seg(actors !== '' && rest > 4 ? `  ${cutCell(actors, rest)}` : '', 'muted'),
    ])
  }

  return runsOfLines(lines)
}

// --- Kanban -------------------------------------------------------------------------

const GAP = 3

function priorityTone(priority: string): Tone {
  const level = priority.toLowerCase()

  return level.includes('high') ? { slot: 'err' } : level.includes('medium') ? { slot: 'warn' } : level.includes('low') ? { slot: 'ok' } : 'muted'
}

// One column drawn on its own grid, so nothing in it spills into its neighbour.
function columnGrid(column: Kanban['columns'][number], i: number, width: number, ascii: boolean): Grid {
  const tone = series(i)
  const lines: Segment[][] = [
    [seg(cutCell(column.label, width - 4), tone), seg(`  ${column.cards.length}`, 'muted')],
    [seg((ascii ? '-' : '─').repeat(width), 'muted')],
  ]

  column.cards.forEach((card, k) => {
    if (k > 0) {
      lines.push([])
    }

    wrapTo(card.label, width - 2, 3).forEach(text => lines.push([seg(ascii ? '|' : '▌', tone), seg(` ${text}`, 'fg')]))

    const meta = [card.assigned === '' ? '' : `@${card.assigned}`, card.priority, card.ticket].filter(part => part !== '')

    if (meta.length > 0) {
      lines.push([
        seg('  '),
        ...meta.flatMap((part, m) => [
          seg(m === 0 ? '' : ' · ', 'muted'),
          seg(part, part === card.priority ? priorityTone(part) : 'muted'),
        ]),
      ])
    }
  })

  if (column.cards.length === 0) {
    lines.push([seg('  empty', 'muted')])
  }

  const grid = gridOf(width, lines.length)
  lines.forEach((segments, y) => writeAll(grid, 0, y, segments))

  return grid
}

function kanbanArt(chart: Kanban, room: number, ascii: boolean): Run[][] | null {
  const columns = chart.columns

  if (columns.length === 0 || room < 16) {
    return null
  }

  const across = Math.min(32, Math.floor((room - GAP * (columns.length - 1)) / columns.length))

  // Side by side when every column gets room for a card, one under another otherwise.
  if (across >= 16) {
    const grids = columns.map((column, i) => columnGrid(column, i, across, ascii))
    const grid = gridOf(room, Math.max(...grids.map(part => part.length)))
    grids.forEach((part, i) => blit(grid, part, i * (across + GAP), 0))

    return runsOf(grid)
  }

  const width = Math.min(room, 40)
  const grids = columns.map((column, i) => columnGrid(column, i, width, ascii))
  const grid = gridOf(width, grids.reduce((sum, part) => sum + part.length + 1, 0))
  grids.reduce((y, part) => {
    blit(grid, part, 0, y)

    return y + part.length + 1
  }, 0)

  return runsOf(grid)
}
