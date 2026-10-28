// What the mod keeps in step with Claude Code (ADR-514): the theme file,
// `custom:ways`, and the two `/config` rows the frames read, `verbose` and
// `theme`. The register module calls it at session start and from its
// `config.set` hook; the skins layer calls `onPickup` on each later read of
// its export, so one timer carries every mid-session read. Every failure
// does nothing but log one debug line, as register.tsx's header says.

import type { EngineInterface } from 'claude-code'

import { DEFAULT_THEME, TERMINAL, WAYS_THEME, WAYS_TIMEOUT_MS, activeArgv, configDir, exportArgv, parseActive, parseTheme, themeFile, themeText } from './lib.ts'
import type { ClaudeCodeTheme, Run } from './lib.ts'

function debug($: EngineInterface, text: string): void {
  $.ui.log(`ways-theme: ${text}`, { to: 'debug' })
}

/** Claude Code's `verbose` row, and whether its `theme` row is `custom:ways`. */
export type Rows = { isVerbose: boolean; isWaysTheme: boolean }

let rows: Rows = { isVerbose: false, isWaysTheme: false }
/** Whether the last read of the `theme` row failed, so a failure logs once. */
let isSelectionUnread = false

/** The rows as last read or changed. */
export const configRows = (): Rows => rows

/** One bounded `ways` call; undefined when it could not start or ran past
 *  its bound. */
async function ways($: EngineInterface, argv: string[]): Promise<Run | undefined> {
  try {
    return await $.process.run(argv, { timeoutMs: WAYS_TIMEOUT_MS })
  } catch (err) {
    debug($, `\`${argv.join(' ')}\` did not run: ${String(err)}`)
    return undefined
  }
}

/** The theme the file should hold now, or undefined when ways gave nothing
 *  to trust. */
async function wanted($: EngineInterface): Promise<ClaudeCodeTheme | undefined> {
  const got = await ways($, activeArgv())
  const active = got && parseActive(got)
  if (active === undefined) {
    if (got) debug($, `\`ways settings get theme.active\` gave nothing usable (exit ${got.exitCode})`)
    return undefined
  }
  if (active === TERMINAL) return DEFAULT_THEME
  const exported = await ways($, exportArgv(active))
  const theme = exported && parseTheme(exported)
  if (!theme && exported) debug($, `the export of ${active} gave no theme (exit ${exported.exitCode})`)
  return theme
}

/** Rewrites the theme file when its content changed; every path that cannot
 *  finish returns. */
async function follow($: EngineInterface): Promise<void> {
  const theme = await wanted($)
  if (!theme) return
  const dir = configDir(await $.env.get('CLAUDE_CONFIG_DIR'), await $.env.get('HOME'))
  if (!dir) return debug($, 'no CLAUDE_CONFIG_DIR or HOME, so no theme folder')
  const file = themeFile(dir)
  const text = themeText(theme)
  const old = await $.fs.read(file).catch(() => undefined)
  if (old !== text) await $.fs.write(file, text)
}

/** The theme file brought to `theme.active`: at session start, and after a
 *  switch mid-session. Throws nothing. */
export function rewrite($: EngineInterface): Promise<void> {
  return follow($).catch(err => debug($, `left the theme file as it was: ${String(err)}`))
}

/** Reads the `verbose` and `theme` rows and redraws when either moved. A row
 *  that cannot be read reads as off, and as not `custom:ways`. */
export async function readRows($: EngineInterface): Promise<void> {
  const listed = await $.config.list().catch(() => undefined)
  const theme = listed?.find(row => row.key === 'theme')
  if (theme === undefined && !isSelectionUnread) debug($, "Claude Code's theme row could not be read, so no frame draws")
  isSelectionUnread = theme === undefined
  const read: Rows = { isVerbose: listed?.find(row => row.key === 'verbose')?.value === true, isWaysTheme: theme?.value === WAYS_THEME }
  if (read.isVerbose === rows.isVerbose && read.isWaysTheme === rows.isWaysTheme) return
  rows = read
  $.ui.invalidate('ui.render')
}

/** A session's first read: a theme row that cannot be read logs again. */
export function startRows($: EngineInterface): Promise<void> {
  isSelectionUnread = false
  return readRows($)
}

/** A `verbose` or `theme` change that `config.set` wrote. */
export function noteRow(key: string, value: unknown): void {
  rows = key === 'verbose' ? { ...rows, isVerbose: value === true } : { ...rows, isWaysTheme: value === WAYS_THEME }
}

/** The skins layer's turn after each later read of its export: the theme file
 *  follows a switch made mid-session, and the rows are read again for a
 *  change made outside /config, such as `ways manage repair` writing
 *  settings.json. */
export async function onPickup($: EngineInterface, isSwitched: boolean): Promise<void> {
  if (isSwitched) await rewrite($)
  await readRows($)
}
