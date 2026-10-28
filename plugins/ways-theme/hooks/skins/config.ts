// The skins layer's one source of configuration: `ways settings theme export
// --mod`, read through a strict parse, and the look it gives the drawing: the
// prefs, the skin's colours mapped from the theme's roles, the gradient prompt
// box's grounds and the frame switch. Nothing here calls the engine.

import type { Prefs } from '../../types'
import type { Palette, Skin } from './skin.ts'

export const MOD_EXPORT_ARGV: readonly string[] = ['ways', 'settings', 'theme', 'export', '--mod']

export const TERMINAL = 'terminal'

export const ROLES = ['bg', 'body', 'accent', 'accent_dim', 'info', 'ok', 'warn', 'err', 'alt', 'hot', 'muted', 'rule', 'selection_bg', 'subtle'] as const
export type Role = (typeof ROLES)[number]
export type Roles = Readonly<Record<Role, string>>

export type Axis = 'vertical' | 'horizontal'
export type Gradient = { axis: Axis; stops: readonly string[] }

export type Render = {
  rail: boolean
  tables: 'on' | 'text' | 'off'
  shimmer: boolean
  charts: boolean
  markdown: boolean
  icons: 'unicode' | 'ascii'
  frames: boolean
  prompt_box: 'off' | 'frame' | 'gradient'
  /** The inline Copy, `as text` and `copy reply` controls. Optional in the
   *  export; absent reads as true. */
  copy: boolean
}

/** The export, as the contract with `ways` states it. */
export type ModExport = {
  theme: string
  kind: 'dark' | 'light'
  roles: Roles | null
  gradient: Gradient | null
  words: { spinner: readonly string[]; done: readonly string[] } | null
  render: Render
}

const HEX = /^#[0-9a-fA-F]{6}$/
const NAME = /^[a-z0-9-]+$/
// A spinner or footer word: printable, one line, short enough for the spinner's row.
const WORD = /^[^\p{Cc}]{1,24}$/u
const MAX_WORDS = 64
const MAX_STOPS = 16

const isRecord = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v)
const hasExactly = (v: Record<string, unknown>, keys: readonly string[]): boolean => {
  const own = Object.keys(v)
  return own.length === keys.length && keys.every(k => Object.hasOwn(v, k))
}
const isHex = (v: unknown): v is string => typeof v === 'string' && HEX.test(v)
const isWords = (v: unknown): v is string[] =>
  Array.isArray(v) && v.length > 0 && v.length <= MAX_WORDS && v.every(w => typeof w === 'string' && WORD.test(w))
const oneOf = <T extends string>(v: unknown, of: readonly T[]): v is T => typeof v === 'string' && (of as readonly string[]).includes(v)

const RENDER_KEYS = ['rail', 'tables', 'shimmer', 'charts', 'markdown', 'icons', 'frames', 'prompt_box'] as const

function parseRender(v: unknown): Render | string {
  if (!isRecord(v)) return 'render is not an object'
  const keys = Object.hasOwn(v, 'copy') ? [...RENDER_KEYS, 'copy'] : RENDER_KEYS
  if (!hasExactly(v, keys)) return 'render is not the eight keys and an optional copy'
  const { rail, tables, shimmer, charts, markdown, icons, frames, prompt_box } = v
  const copy = v.copy ?? true
  if (![rail, shimmer, charts, markdown, frames, copy].every(b => typeof b === 'boolean')) return 'a render switch is not a boolean'
  if (!oneOf(tables, ['on', 'text', 'off'] as const)) return 'render.tables is not on, text or off'
  if (!oneOf(icons, ['unicode', 'ascii'] as const)) return 'render.icons is not unicode or ascii'
  if (!oneOf(prompt_box, ['off', 'frame', 'gradient'] as const)) return 'render.prompt_box is not off, frame or gradient'
  return { rail, tables, shimmer, charts, markdown, icons, frames, prompt_box, copy } as Render
}

/** The export's stdout, held to the contract: the export, or why not. Every
 *  key but `render.copy` is required, none is extra, and every colour is
 *  `#rrggbb`. */
export function parseExport(stdout: string): ModExport | string {
  let v: unknown
  try {
    v = JSON.parse(stdout)
  } catch {
    return 'the export is not JSON'
  }
  if (!isRecord(v) || !hasExactly(v, ['theme', 'kind', 'roles', 'gradient', 'words', 'render'])) return 'the export is not the six keys'
  const { theme, kind, roles, gradient, words } = v
  if (typeof theme !== 'string' || !NAME.test(theme)) return 'theme is not a name'
  if (!oneOf(kind, ['dark', 'light'] as const)) return 'kind is not dark or light'
  if (theme === TERMINAL ? roles !== null : !isRecord(roles) || !hasExactly(roles, ROLES) || !ROLES.every(r => isHex(roles[r]))) {
    return theme === TERMINAL ? 'the terminal theme has roles' : 'roles are not the fourteen #rrggbb colours'
  }
  if (gradient !== null) {
    if (!isRecord(gradient) || !hasExactly(gradient, ['axis', 'stops'])) return 'gradient is not axis and stops'
    if (!oneOf(gradient.axis, ['vertical', 'horizontal'] as const)) return 'gradient.axis is not vertical or horizontal'
    const stops = gradient.stops
    if (!Array.isArray(stops) || stops.length < 2 || stops.length > MAX_STOPS || !stops.every(isHex)) return 'gradient.stops are not 2 to 16 #rrggbb colours'
  }
  if (words !== null && (!isRecord(words) || !hasExactly(words, ['spinner', 'done']) || !isWords(words.spinner) || !isWords(words.done))) {
    return 'words are not spinner and done lists of short words'
  }
  const render = parseRender(v.render)
  if (typeof render === 'string') return render
  return { ...(v as unknown as ModExport), render }
}

// Colour, in OKLab so a mix keeps its lightness even and its hue straight.

type Rgb = readonly [number, number, number]
type Lab = readonly [number, number, number]

const toLinear = (c: number): number => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4)
const toSrgb = (c: number): number => (c <= 0.0031308 ? 12.92 * c : 1.055 * c ** (1 / 2.4) - 0.055)

export function hexToRgb(hex: string): Rgb {
  return [1, 3, 5].map(i => parseInt(hex.slice(i, i + 2), 16) / 255) as unknown as Rgb
}

export function rgbToHex(rgb: Rgb): string {
  return `#${rgb.map(c => Math.round(Math.min(1, Math.max(0, c)) * 255).toString(16).padStart(2, '0')).join('')}`
}

export function toOklab(hex: string): Lab {
  const [r, g, b] = hexToRgb(hex).map(toLinear) as [number, number, number]
  const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b)
  const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b)
  const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b)
  return [
    0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s,
    1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s,
    0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s,
  ]
}

export function fromOklab([L, a, b]: Lab): string {
  const l = (L + 0.3963377774 * a + 0.2158037573 * b) ** 3
  const m = (L - 0.1055613458 * a - 0.0638541728 * b) ** 3
  const s = (L - 0.0894841775 * a - 1.291485548 * b) ** 3
  return rgbToHex(
    [
      4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
      -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
      -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s,
    ].map(toSrgb) as unknown as Rgb,
  )
}

/** `t` of the way from `a` to `b`, in OKLab. */
export function mix(a: string, b: string, t: number): string {
  const x = toOklab(a)
  const y = toOklab(b)
  return fromOklab([0, 1, 2].map(i => x[i]! + (y[i]! - x[i]!) * t) as unknown as Lab)
}

/** The gradient's colour at `t` in [0, 1], its stops spaced evenly. */
export function gradientAt(stops: readonly string[], t: number): string {
  if (stops.length === 1) return stops[0]!
  const at = Math.min(1, Math.max(0, t)) * (stops.length - 1)
  const i = Math.min(stops.length - 2, Math.floor(at))
  return mix(stops[i]!, stops[i + 1]!, at - i)
}

// Where the table bands sit between the theme's bg and its subtle.
const SURFACE_STEP = 0.6
const ZEBRA_STEP = 0.3

/** The skin's slots from the theme's roles. */
export function paletteOf(roles: Roles): Palette {
  return {
    user: roles.accent,
    fg: roles.body,
    muted: roles.muted,
    surface: mix(roles.bg, roles.subtle, SURFACE_STEP),
    zebra: mix(roles.bg, roles.subtle, ZEBRA_STEP),
    ok: roles.ok,
    err: roles.err,
    warn: roles.warn,
    read: roles.info,
    write: roles.hot,
    run: roles.warn,
    search: roles.alt,
    web: roles.accent_dim,
    mcp: roles.muted,
    other: roles.rule,
  }
}

// skins' own terminal look (hooks/themes/terminal.ts at 985ad63): the terminal's
// sixteen colours and Claude Code's theme keys, so it follows whatever the
// terminal and Claude Code's theme are.
export const TERMINAL_PALETTE: Palette = {
  read: 'ansi256(6)',
  write: 'ansi256(5)',
  run: 'ansi256(3)',
  search: 'ansi256(4)',
  web: 'ansi256(14)',
  mcp: 'ansi256(13)',
  other: 'ansi256(8)',
  user: 'ansi256(12)',
  fg: 'text',
  muted: 'ansi256(8)',
  surface: 'userMessageBackground',
  zebra: 'bashMessageBackgroundColor',
  ok: 'ansi256(2)',
  err: 'ansi256(1)',
  warn: 'ansi256(3)',
}

/** How the person's own message draws. `gradient` falls back to `frame` where
 *  it cannot draw (promptBoxOf). */
export type PromptBox = Render['prompt_box']

/** Everything the drawing reads, made once per export. */
export type Setup = {
  prefs: Prefs
  skin: Skin
  frames: boolean
  /** The theme has colours of its own: not `terminal`. The reply and tool
   *  frames draw only then. */
  themed: boolean
  promptBox: PromptBox
  /** The prompt box's grounds and its text, for a theme with colours: the
   *  gradient's stops, or the selection colour alone when it has none. */
  ground: { stops: readonly string[]; text: string } | null
}

export function setupOf(x: ModExport): Setup {
  const r = x.render
  const prefs: Prefs = {
    skin: x.theme,
    icons: r.icons,
    prompt: 'left',
    words: x.words === null ? 'claude' : 'skin',
    rail: r.rail,
    tables: r.tables,
    shimmer: r.shimmer,
    band: true,
    countdown: false,
    clipOutput: false,
    markdown: r.markdown,
    quiet: false,
    charts: r.charts,
    math: r.markdown,
    commands: true,
    shell: true,
    highlight: true,
    links: true,
    copy: r.copy,
    fold: true,
    expand: false,
    calm: null,
  }
  const palette = x.roles === null ? TERMINAL_PALETTE : paletteOf(x.roles)
  const skin: Skin = { name: x.theme, label: x.theme, palette, spinner: x.words?.spinner ?? [], done: x.words?.done ?? [] }
  const ground = x.roles === null ? null : { stops: x.gradient?.stops ?? [x.roles.selection_bg], text: x.roles.body }
  return { prefs, skin, frames: r.frames, themed: x.roles !== null, promptBox: r.prompt_box, ground }
}
