// What the ways-theme mod reads and writes, apart from the engine calls
// that carry it (register.ts), so the tests can hold each rule.

export const WAYS = 'ways'

/** Each `ways` call's bound. A call past it counts as a failure, and a
 *  failure leaves the theme file as it was. */
export const WAYS_TIMEOUT_MS = 1500

/** The agent-ways theme that is the terminal's own colours: no export. */
export const TERMINAL = 'terminal'

export const activeArgv = (): string[] => [WAYS, 'settings', 'get', 'theme.active']
export const exportArgv = (name: string): string[] => [WAYS, 'settings', 'theme', 'export', name, '--claude-code']

/** The one Claude Code theme the mod keeps, `custom:ways`. `ways manage
 *  repair` selects it in settings.json while the mod is wanted; the mod only
 *  rewrites the file, which Claude Code reloads live. */
export const themeFile = (configDir: string): string => `${configDir.replace(/\/+$/, '')}/themes/ways.json`

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
export type ClaudeCodeTheme = { name: string; base: string; overrides?: Record<string, string> }

/** The file while no agent-ways theme is active: Claude Code's own dark. */
export const DEFAULT_THEME: ClaudeCodeTheme = { name: 'ways', base: 'dark' }

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
