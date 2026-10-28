// Ported from skins by hellosverre (MIT; LICENSE beside this file),
// https://github.com/hellosverre/claude-skins at 985ad63, hooks/svg-usage.ts.

import type { UsageSnap } from '../../types'
import type { Palette } from './skin'
import { escape, FONT, measure } from './svg-kit'

// The band above the prompt: how full the context window is and how much of each plan
// limit is spent, as rings that fill in when they draw.

const RING_R = 9
const FILL_MS = 900
const ITEM_W = 132
const TEXT_SIZE = 12.5
const MINUTE_MS = 60_000

// `resets`, where shown, is the time left until the limit resets, as `countdownOf` writes it.
export type Meter = { label: string; percent: number; resets?: string }

// The time left until a reset, to the minute: `3d4h`, `1h05`, `12m`, and `now` once it is due.
export function countdownOf(ms: number): string {
  const minutes = Math.ceil(ms / MINUTE_MS)

  if (minutes <= 0) {
    return 'now'
  }

  const days = Math.floor(minutes / 1440)
  const hours = Math.floor((minutes % 1440) / 60)
  const rest = minutes % 60

  if (days > 0) {
    return `${days}d${hours}h`
  }

  return hours > 0 ? `${hours}h${String(rest).padStart(2, '0')}` : `${rest}m`
}

// Each limit's countdown by its label, for the limits that say when they reset.
export const countdownsOf = (usage: UsageSnap, now: number): Record<string, string> =>
  Object.fromEntries(usage.limits.flatMap(limit => (limit.resetsAt === undefined ? [] : [[limit.label, countdownOf(limit.resetsAt - now)]])))

// What the band's entrance is keyed by: the numbers, not the countdowns, so a countdown
// ticking down redraws the rings settled instead of filling them again.
export const metersKey = (meters: readonly Meter[]): string => meters.map(meter => `${meter.label} ${meter.percent}%`).join(', ')

const meterText = (meter: Meter): string => `${meter.percent}% ${meter.label}${meter.resets === undefined ? '' : ` · ${meter.resets}`}`

// `five_hour` reads as `5h`, `seven_day` as `7d`; any other kind as its own name.
export function limitLabel(kind: string): string {
  const name = kind.toLowerCase()

  if (name.includes('five') || name.includes('5h') || name.includes('5_h')) {
    return '5h'
  }

  if (name.includes('seven') || name.includes('7d') || name.includes('week')) {
    return '7d'
  }

  return name.replace(/_/g, ' ')
}

// `countdowns`, by limit label, puts each limit's time to reset beside its meter.
export function metersOf(usage: UsageSnap, countdowns: Record<string, string> = {}): Meter[] {
  return [
    ...(usage.context === null ? [] : [{ label: 'context', percent: usage.context }]),
    ...usage.limits.map(limit => {
      const resets = countdowns[limit.label]

      return { label: limit.label, percent: limit.percent, ...(resets === undefined ? {} : { resets }) }
    }),
  ].map(meter => ({ ...meter, percent: Math.max(0, Math.min(100, Math.round(meter.percent))) }))
}

// Accent while there is room; the warning colour from 80 %, the error colour from 95 %.
export const meterColor = (percent: number, palette: Palette): string =>
  percent >= 95 ? palette.err : percent >= 80 ? palette.warn : palette.user

// `tight` sizes each meter to its ring and words, with no fixed slot, so a row can spread them evenly.
export function usageSvg(meters: readonly Meter[], palette: Palette, tight = false): { source: string; width: number; height: number; alt: string; entranceMs: number } {
  // A meter with a countdown is wider than the rest; each starts where the one before ended.
  const content = (meter: Meter) => RING_R * 2 + 12 + measure(meterText(meter), false, TEXT_SIZE)
  const widths = meters.map(meter => (tight ? Math.ceil(content(meter) + 6) : Math.max(ITEM_W, Math.ceil(content(meter) + 20))))
  const starts = widths.map((_, i) => widths.slice(0, i).reduce((sum, w) => sum + w, 0))
  const width = widths.reduce((sum, w) => sum + w, 0)
  const height = 30
  const circumference = 2 * Math.PI * RING_R

  const items = meters
    .map((meter, i) => {
      const x = (starts[i] ?? 0) + RING_R + 4
      const filled = (circumference * meter.percent) / 100
      const color = meterColor(meter.percent, palette)

      return [
        `<circle cx="${x}" cy="15" r="${RING_R}" fill="none" stroke="${palette.muted}" stroke-opacity=".25" stroke-width="3"/>`,
        `<circle class="fill" cx="${x}" cy="15" r="${RING_R}" fill="none" stroke="${color}" stroke-width="3" stroke-linecap="round" stroke-dasharray="${filled} ${circumference}" transform="rotate(-90 ${x} 15)" style="--len:${filled}"/>`,
        `<text x="${x + RING_R + 8}" y="19.5" font-size="${TEXT_SIZE}"><tspan style="fill:${palette.fg};font-weight:600">${meter.percent}%</tspan><tspan style="fill:${palette.muted}"> ${escape(meter.label)}${meter.resets === undefined ? '' : ` · ${escape(meter.resets)}`}</tspan></text>`,
      ].join('')
    })
    .join('')

  const style = [
    `text{font-family:${FONT}}`,
    `.fill{animation:fill ${FILL_MS}ms cubic-bezier(.2,.8,.2,1)}`,
    '@keyframes fill{from{stroke-dasharray:0 100}}',
    '@media (prefers-reduced-motion:reduce){.fill{animation:none}}',
  ].join('')

  return {
    source: `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}"><style>${style}</style>${items}</svg>`,
    width,
    height,
    alt: meters.map(meter => `${meter.label} ${meter.percent}%${meter.resets === undefined ? '' : `, resets in ${meter.resets}`}`).join(', '),
    entranceMs: FILL_MS,
  }
}

// The terminal's version: one line of block meters, `text` what follows each bar.
export function usageLine(meters: readonly Meter[]): { label: string; bar: string; percent: number; text: string }[] {
  return meters.map(meter => {
    const filled = Math.round(meter.percent / 12.5)

    return { label: meter.label, bar: '▰'.repeat(filled) + '▱'.repeat(8 - filled), percent: meter.percent, text: meterText(meter) }
  })
}
