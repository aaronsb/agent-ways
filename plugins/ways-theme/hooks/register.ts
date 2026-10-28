// Claude Code follows the active agent-ways theme (ADR-513).
//
// At an interactive session's start the mod reads `theme.active` through
// `ways settings get`. For a theme it runs `ways settings theme export
// <name> --claude-code`, writes the result to Claude Code's custom theme
// folder as `ways-<name>.json` when the file differs, and selects
// `custom:ways-<name>` when the theme row differs. For `terminal` it moves a
// row that holds one of its own themes back to `dark`, and leaves any other
// theme, which is the person's choice.
//
// Every failure does nothing: `ways` missing, a non-zero exit, output that
// does not parse, a call past its bound, a write or a selection refused. One
// debug-log line says which. There is no config.set hook, so a theme the
// person picks stays until the next session start. The mod hooks no
// classic.* event, which an organization's policy plugin passes over.

import type { EngineInterface, Register } from 'claude-code'

import { DEFAULT_THEME, WAYS_TIMEOUT_MS, activeArgv, configDir, exportArgv, parseActive, parseTheme, plan, preference, themeFile, themeText } from './lib.ts'
import type { Run } from './lib.ts'

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

/** Selects `value` on the theme row unless it already holds it. */
async function select($: EngineInterface, current: unknown, value: string): Promise<void> {
  if (current === value) return
  const r = await $.config.set({ key: 'theme', value })
  if ('deny' in r && r.deny) debug($, `theme ${value} refused: ${r.deny}`)
}

/** The session start's work; every path that cannot finish returns. */
export async function follow($: EngineInterface): Promise<void> {
  const rows = await $.config.list()
  const current = rows.find(row => row.key === 'theme')?.value
  const got = await ways($, activeArgv())
  const active = got && parseActive(got)
  if (active === undefined) {
    if (got) debug($, `\`ways settings get theme.active\` gave nothing usable (exit ${got.exitCode})`)
    return
  }
  const step = plan(active, current)
  if (step.kind === 'none') return
  if (step.kind === 'reset') return select($, current, DEFAULT_THEME)
  const exported = await ways($, exportArgv(step.name))
  const theme = exported && parseTheme(exported)
  if (!theme) {
    if (exported) debug($, `the export of ${step.name} gave no theme (exit ${exported.exitCode})`)
    return
  }
  const dir = configDir(await $.env.get('CLAUDE_CONFIG_DIR'), await $.env.get('HOME'))
  if (!dir) return debug($, 'no CLAUDE_CONFIG_DIR or HOME, so no theme folder')
  const file = themeFile(dir, step.name)
  const text = themeText(theme)
  const old = await $.fs.read(file).catch(() => undefined)
  if (old !== text) await $.fs.write(file, text)
  await select($, current, preference(step.name))
}

export const register: Register = on => {
  on('session.start', async ($, e, next) => {
    const started = await next(e)
    if (!e.isInteractive) return started
    await follow($).catch(err => debug($, `left the theme as it was: ${String(err)}`))
    return started
  })
}
