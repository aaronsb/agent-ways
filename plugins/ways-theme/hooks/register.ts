// Claude Code follows the active agent-ways theme (ADR-513).
//
// Claude Code keeps one custom theme for agent-ways, `custom:ways`, whose
// file is `<config dir>/themes/ways.json`. `ways manage repair` selects it in
// settings.json while this mod is wanted, through the settings merge that
// owns the key. At an interactive session's start the mod reads
// `theme.active` through `ways settings get` and rewrites that file when its
// content changed: the export of the theme (`ways settings theme export
// <name> --claude-code`), or Claude Code's own dark for `terminal`. Claude
// Code watches its themes folder, so the change applies live. The mod
// selects nothing: `$.config.set` refuses a custom theme in Claude Code
// 2.1.312, and the person's own choice in /theme stays theirs.
//
// Every failure does nothing: `ways` missing, a non-zero exit, output that
// does not parse, a call past its bound, a write refused. The file stays as
// last written, and one debug-log line says why. A headless session changes
// nothing. The mod hooks no classic.* event, which an organization's policy
// plugin passes over.

import type { EngineInterface, Register } from 'claude-code'

import { DEFAULT_THEME, TERMINAL, WAYS_TIMEOUT_MS, activeArgv, configDir, exportArgv, parseActive, parseTheme, themeFile, themeText } from './lib.ts'
import type { ClaudeCodeTheme, Run } from './lib.ts'

const debug = ($: EngineInterface, text: string) => $.ui.log(`ways-theme: ${text}`, { to: 'debug' })

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

/** The session start's work; every path that cannot finish returns. */
export async function follow($: EngineInterface): Promise<void> {
  const theme = await wanted($)
  if (!theme) return
  const dir = configDir(await $.env.get('CLAUDE_CONFIG_DIR'), await $.env.get('HOME'))
  if (!dir) return debug($, 'no CLAUDE_CONFIG_DIR or HOME, so no theme folder')
  const file = themeFile(dir)
  const text = themeText(theme)
  const old = await $.fs.read(file).catch(() => undefined)
  if (old !== text) await $.fs.write(file, text)
}

export const register: Register = on => {
  on('session.start', async ($, e, next) => {
    const started = await next(e)
    if (!e.isInteractive) return started
    await follow($).catch(err => debug($, `left the theme file as it was: ${String(err)}`))
    return started
  })
}
