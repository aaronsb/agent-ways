// What the mod keeps in step with Claude Code (ADR-514): the theme file,
// `custom:ways`, and the two `/config` rows the frames read, `verbose` and
// `theme`. The register module calls it at session start and from its
// `config.set` hook; the skins layer calls `onPickup` after each later read of
// its export, so one timer carries every mid-session read. Every failure
// does nothing but log one debug line per distinct reason, as register.tsx's
// header says.

import type { EngineInterface } from 'claude-code'

import { DEFAULT_THEME, TERMINAL, WAYS_THEME, WAYS_TIMEOUT_MS, activeArgv, configDir, exportArgv, parseActive, parseTheme, themeFile, themeText } from './lib.ts'
import type { ClaudeCodeTheme, Run } from './lib.ts'

function debug($: EngineInterface, text: string): void {
  $.ui.log(`ways-theme: ${text}`, { to: 'debug' })
}

/** Claude Code's `verbose` row, and whether its `theme` row is `custom:ways`. */
export type Rows = { isVerbose: boolean; isWaysTheme: boolean }

let rows: Rows = { isVerbose: false, isWaysTheme: false }
/** Bumped by each `config.set` the hook saw, so a `config.list` answer taken
 *  before it cannot overwrite it. */
let rowsChanges = 0
/** Whether the last read of the `theme` row failed, so a failure logs once. */
let isSelectionUnread = false

/** The look (the skins export's text) the theme file was last brought
 *  current for, or undefined while no rewrite has finished this session. */
let followedLook: string | undefined
/** Why the last rewrite failed, so a repeated failure logs once. */
let lastFailure: string | undefined

/** The rows as last read or changed. */
export const configRows = (): Rows => rows

/** Logs a rewrite's failure unless it is the one logged last. */
function failed($: EngineInterface, reason: string): void {
  if (reason === lastFailure) return
  lastFailure = reason
  debug($, reason)
}

/** One bounded `ways` call: the run, or why it did not run. */
async function ways($: EngineInterface, argv: string[]): Promise<Run | string> {
  try {
    return await $.process.run(argv, { timeoutMs: WAYS_TIMEOUT_MS })
  } catch (err) {
    return `\`${argv.join(' ')}\` did not run: ${String(err)}`
  }
}

/** The theme the file should hold now, or why ways gave nothing to trust. */
async function wanted($: EngineInterface): Promise<ClaudeCodeTheme | string> {
  const got = await ways($, activeArgv())
  if (typeof got === 'string') return got
  const active = parseActive(got)
  if (active === undefined) return `\`ways settings get theme.active\` gave nothing usable (exit ${got.exitCode})`
  if (active === TERMINAL) return DEFAULT_THEME
  const exported = await ways($, exportArgv(active))
  if (typeof exported === 'string') return exported
  return parseTheme(exported) ?? `the export of ${active} gave no theme (exit ${exported.exitCode})`
}

/** Brings the theme file to `theme.active`, writing only when its content
 *  changed: the file's text, or why it could not be brought current. */
async function follow($: EngineInterface): Promise<string | undefined> {
  const theme = await wanted($)
  if (typeof theme === 'string') return theme
  const dir = configDir(await $.env.get('CLAUDE_CONFIG_DIR'), await $.env.get('HOME'))
  if (!dir) return 'no CLAUDE_CONFIG_DIR or HOME, so no theme folder'
  const file = themeFile(dir)
  const text = themeText(theme)
  const old = await $.fs.read(file).catch(() => undefined)
  if (old !== text) await $.fs.write(file, text)
  return undefined
}

/** Brings the theme file current for `look`, the skins export's text as last
 *  read. On success later polls skip it until the look changes; on failure
 *  the next poll tries again. Throws nothing. */
async function rewrite($: EngineInterface, look: string): Promise<void> {
  const failure = await follow($).catch((err: unknown) => `left the theme file as it was: ${String(err)}`)
  if (failure !== undefined) return failed($, failure)
  lastFailure = undefined
  followedLook = look
}

/** A session's start: the theme file brought current for the first look,
 *  every failure logging again. */
export function startFollow($: EngineInterface, look: string): Promise<void> {
  followedLook = undefined
  lastFailure = undefined
  return rewrite($, look)
}

/** Reads the `verbose` and `theme` rows and redraws when either moved. A row
 *  that cannot be read reads as off, and as not `custom:ways`. A row another
 *  plugin hides through `config.describe` is left out of the list, so it
 *  reads as unread too. */
export async function readRows($: EngineInterface): Promise<void> {
  const changes = rowsChanges
  const listed = await $.config.list().catch(() => undefined)
  if (changes !== rowsChanges) return
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
  rowsChanges += 1
  rows = key === 'verbose' ? { ...rows, isVerbose: value === true } : { ...rows, isWaysTheme: value === WAYS_THEME }
}

/** The skins layer's turn after each later read of its export. The theme
 *  file follows any change in the look (another theme, or new colours under
 *  the same name) and is tried again after a rewrite that failed, and the
 *  rows are read again for a change made outside /config, such as
 *  `ways manage repair` writing settings.json. */
export async function onPickup($: EngineInterface, look: string): Promise<void> {
  if (look !== followedLook) await rewrite($, look)
  await readRows($)
}
