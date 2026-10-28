// Claude Code follows the active agent-ways theme (ADR-514).
//
// Two layers. The theme file: Claude Code keeps one custom theme for
// agent-ways, `custom:ways`, at `<config dir>/themes/ways.json`. `ways manage
// repair` selects it in settings.json while this mod is wanted, through the
// settings merge that owns the key, because `$.config.set` refuses a custom
// theme in Claude Code 2.1.312 (anthropics/claude-code#105739). At an
// interactive session's start the mod reads `theme.active` through `ways
// settings get` and rewrites that file when its content changed: the export
// of the theme (`ways settings theme export <name> --claude-code`), or Claude
// Code's own dark for `terminal`. Claude Code watches its themes folder, so
// the change applies live.
//
// The frames: on the terminal, under an agent-ways theme, each reply sits in
// a border on the theme's ground, and each tool call and tool group in a
// border, wrapped around the engine's own drawing. The colours are Claude
// Code theme keys, so they follow `custom:ways`. The ctrl+o transcript, read
// from the line that closes a turn, an expanded tool row, `verbose`, a reply
// longer than MAX_FRAMED_CHARS and every other surface draw the engine's own
// row. Only a full `borderStyle` is used: a Box's per-side border props are
// refused.
//
// Every failure does nothing: `ways` missing, a non-zero exit, output that
// does not parse, a call past its bound, a write refused. The file stays as
// last written, no frame draws, and one debug-log line says why. A headless
// session changes nothing. The mod hooks no classic.* event, which an
// organization's policy plugin passes over.

import type { EngineInterface, Register, RenderElement } from 'claude-code'

import { DEFAULT_THEME, FRAME, TERMINAL, WAYS_TIMEOUT_MS, activeArgv, configDir, exportArgv, parseActive, parseTheme, shouldFrame, themeFile, themeText } from './lib.ts'
import type { ClaudeCodeTheme, Run } from './lib.ts'
import { framesWanted, registerSkins } from './skins/hub.tsx'

const debug = ($: EngineInterface, text: string) => $.ui.log(`ways-theme: ${text}`, { to: 'debug' })

/** What the frames read: set at session start, when `verbose` changes, and
 *  when the terminal's view enters or leaves the ctrl+o transcript. */
const frames = { isThemed: false, isVerbose: false }
let isTranscriptExpanded = false
/** Each terminal turn line's last `isExpanded`, by row. */
const turnLines = new Map<string, boolean>()

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
  frames.isThemed = theme.overrides !== undefined
}

/** Claude Code's `verbose` row, read once; off when it cannot be read. */
async function readVerbose($: EngineInterface): Promise<boolean> {
  const rows = await $.config.list().catch(() => [])
  return rows.find(row => row.key === 'verbose')?.value === true
}

/** `inner` inside a frame. The engine's row opens with a blank margin row;
 *  the inner Box takes it back and the frame keeps the gap outside, so the
 *  ground starts at the first line of text. */
function framed($: EngineInterface, e: Parameters<EngineInterface['ui']['resolve']>[0], inner: RenderElement, frame: Record<string, string>): RenderElement {
  if (!framesWanted(e)) return inner
  const { Box } = $.ui.resolve(e)
  return (
    <Box marginTop={1} borderStyle="round" paddingX={1} {...frame}>
      <Box marginTop={-1}>{inner}</Box>
    </Box>
  )
}

export const register: Register = on => {
  on('session.start', async ($, e, next) => {
    const started = await next(e)
    if (!e.isInteractive) return started
    await follow($).catch(err => debug($, `left the theme file as it was: ${String(err)}`))
    frames.isVerbose = await readVerbose($)
    $.ui.invalidate('ui.render')
    return started
  })

  // Observes `verbose` only; the row is the person's, and the answer is
  // passed on unchanged.
  on('config.set', { key: 'verbose' }, async ($, e, next) => {
    const result = await next(e)
    if (!result.deny) {
      frames.isVerbose = result.value === true
      $.ui.invalidate('ui.render')
    }
    return result
  })

  // A reply carries no `isExpanded` of its own. The line that closes a turn
  // reports whether the whole view is the ctrl+o transcript, where a
  // person's row reports only its own drawing. A line moves the state when
  // its own value changed, so lines that disagree settle in one redraw. The
  // line is drawn as the engine draws it.
  on('ui.render', { component: 'TurnDuration' }, async ($, e, next) => {
    const isExpanded = e.props.isExpanded
    if (e.surface === 'terminal' && isExpanded !== undefined && turnLines.get(e.requestId) !== isExpanded) {
      turnLines.set(e.requestId, isExpanded)
      if (isExpanded !== isTranscriptExpanded) {
        isTranscriptExpanded = isExpanded
        $.ui.invalidate('ui.render')
      }
    }
    return next(e)
  })

  on('ui.render', { component: 'AssistantMessage' }, async ($, e, next) => {
    const c = { surface: e.surface, ...frames, isExpanded: isTranscriptExpanded, length: e.props.text.length }
    if (!shouldFrame(c) || !e.props.isFirstOfReply) return next(e)
    return framed($, e, await next(e), FRAME.reply)
  })

  on('ui.render', { component: 'ToolUse' }, async ($, e, next) => {
    if (!shouldFrame({ surface: e.surface, ...frames, isExpanded: isTranscriptExpanded || e.props.isExpanded })) return next(e)
    return framed($, e, await next(e), e.props.isErrored ? FRAME.toolErrored : FRAME.tool)
  })

  on('ui.render', { component: 'ToolGroup' }, async ($, e, next) => {
    if (!shouldFrame({ surface: e.surface, ...frames, isExpanded: isTranscriptExpanded || e.props.isExpanded })) return next(e)
    return framed($, e, await next(e), FRAME.tool)
  })

  // The skins layer, after the hooks above so they wrap what it draws.
  registerSkins(on, { isTranscriptExpanded: () => isTranscriptExpanded })
}
