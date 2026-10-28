// What the ways-theme mod reads, writes and decides, apart from the engine
// calls that carry it (register.ts), so the tests can hold each rule.

export const WAYS = 'ways'

/** Each `ways` call's bound. A call past it counts as a failure, and a
 *  failure leaves Claude Code's theme as it is. */
export const WAYS_TIMEOUT_MS = 1500

/** The prefix of every Claude Code theme this mod writes. A Claude Code
 *  theme of `custom:ways-*` is the mod's own, and only that one is moved
 *  back to `dark` when no agent-ways theme is active. */
export const SLUG_PREFIX = 'ways-'
export const OWN_PREFIX = `custom:${SLUG_PREFIX}`

/** Claude Code's own default, which the mod switches back to. */
export const DEFAULT_THEME = 'dark'

/** The agent-ways theme that is the terminal's own colours: no export. */
export const TERMINAL = 'terminal'

export const activeArgv = (): string[] => [WAYS, 'settings', 'get', 'theme.active']
export const exportArgv = (name: string): string[] => [WAYS, 'settings', 'theme', 'export', name, '--claude-code']

/** The Claude Code theme preference for agent-ways theme `name`. */
export const preference = (name: string): string => `${OWN_PREFIX}${name}`

/** Where Claude Code reads custom theme `name`: `<config dir>/themes/`. */
export const themeFile = (configDir: string, name: string): string =>
  `${configDir.replace(/\/+$/, '')}/themes/${SLUG_PREFIX}${name}.json`

/** The Claude Code config directory: CLAUDE_CONFIG_DIR, else ~/.claude. */
export function configDir(claudeConfigDir: string | undefined, home: string | undefined): string | undefined {
  if (claudeConfigDir) return claudeConfigDir
  return home ? `${home.replace(/\/+$/, '')}/.claude` : undefined
}

const NAME = /^[a-z0-9-]+$/
const HEX = /^#[0-9a-fA-F]{6}$/
const BASES = ['dark', 'light', 'dark-daltonized', 'light-daltonized', 'dark-ansi', 'light-ansi']

/** One `ways` call's result, as `$.process.run` resolves it. */
export type Run = { exitCode: number; stdout: string }

/** `ways settings get theme.active`: the theme's name, or undefined for a
 *  call that failed or printed anything but one name. An empty value is
 *  the terminal theme. */
export function parseActive(r: Run): string | undefined {
  if (r.exitCode !== 0) return undefined
  const name = r.stdout.trim()
  if (name === '') return TERMINAL
  return NAME.test(name) ? name : undefined
}

/** A Claude Code custom theme file. */
export type ClaudeCodeTheme = { name: string; base: string; overrides: Record<string, string> }

/** `ways settings theme export --claude-code`: the theme, or undefined for a
 *  call that failed or printed anything but a whole, valid theme. */
export function parseTheme(r: Run): ClaudeCodeTheme | undefined {
  if (r.exitCode !== 0) return undefined
  let v: unknown
  try {
    v = JSON.parse(r.stdout)
  } catch {
    return undefined
  }
  if (typeof v !== 'object' || v === null || Array.isArray(v)) return undefined
  const { name, base, overrides } = v as Record<string, unknown>
  if (typeof name !== 'string' || typeof base !== 'string' || !BASES.includes(base)) return undefined
  if (typeof overrides !== 'object' || overrides === null || Array.isArray(overrides)) return undefined
  const entries = Object.entries(overrides as Record<string, unknown>)
  if (entries.length === 0 || !entries.every(([, c]) => typeof c === 'string' && HEX.test(c))) return undefined
  return { name, base, overrides: overrides as Record<string, string> }
}

/** The file's text as the mod writes it. */
export const themeText = (t: ClaudeCodeTheme): string => `${JSON.stringify(t, null, 2)}\n`

/** What the session start does, from the active agent-ways theme and
 *  Claude Code's theme row: follow an agent-ways theme, return a theme of
 *  the mod's own to Claude Code's default, or leave the row alone. */
export type Plan = { kind: 'follow'; name: string } | { kind: 'reset' } | { kind: 'none' }

export function plan(active: string, current: unknown): Plan {
  if (active !== TERMINAL) return { kind: 'follow', name: active }
  const isOwn = typeof current === 'string' && current.startsWith(OWN_PREFIX)
  return isOwn ? { kind: 'reset' } : { kind: 'none' }
}
