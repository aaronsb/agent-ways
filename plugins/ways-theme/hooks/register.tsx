// Claude Code follows the active agent-ways theme (ADR-514).
//
// Two layers. The theme file (follow.ts): Claude Code keeps one custom theme
// for agent-ways, `custom:ways`, at `<config dir>/themes/ways.json`. `ways manage
// repair` selects it in settings.json while this mod is wanted, through the
// settings merge that owns the key, because `$.config.set` refuses a custom
// theme in Claude Code 2.1.312 (anthropics/claude-code#105739). At an
// interactive session's start the mod reads `theme.active` through `ways
// settings get` and rewrites that file when its content changed: the export
// of the theme (`ways settings theme export <name> --claude-code`), or Claude
// Code's own dark for `terminal`. It does the same mid-session whenever the
// skins layer's live export changes, on that layer's timer, and tries again on
// each read after a rewrite that failed. Claude Code watches its themes
// folder, so the change applies live.
//
// The frames: on the terminal, under a theme with colours of its own, each
// reply sits in a border on the theme's ground, and each tool call and tool
// group in a border, wrapped around the engine's own drawing. A call whose row
// Claude Code draws empty gets no border; a SendUserFile that sends an image
// gets its border around its result, the row that shows it. Another mod's
// picture in a framed row (the images mod's) is drawn inside the border when
// that mod sits beneath this one in the chain (skins/pictures.ts). Whether a
// row is framed is one question, `isFramed` (hub.tsx), which this module and the
// skins layer ask of the same look, the live export's, so a theme switch
// mid-session reframes both. A reply holding a `! <command>` block the skins
// layer frames itself, in pieces around the block, so that no frame line runs
// beside the command. The colours are Claude Code theme keys, which
// name the agent-ways theme only while Claude Code's selected theme is
// `custom:ways`. The mod reads that selection from the `theme` row of
// `/config` at session start, on each change through `config.set`, which it
// observes and passes on unchanged, and on the skins layer's timer, which
// catches a settings.json that `ways manage repair` wrote. Under any other
// theme the person picked in /theme or /config, the frames and the person's
// message box step aside for Claude Code's own drawing; selecting
// `custom:ways` again brings them back. The ctrl+o transcript, read
// from the line that closes a turn, an expanded tool row, a reply longer than
// MAX_FRAMED_CHARS and every other surface draw the engine's own row. Under
// `verbose` every row reports itself expanded, so the frames draw there as
// with it off (isExpandedView). Only a full `borderStyle` is used: a Box's
// per-side border props are refused.
//
// Every failure does nothing: `ways` missing, a non-zero exit, output that
// does not parse, a call past its bound, a write refused, a `theme` row that
// cannot be read. The file stays as last written, no frame draws, and one
// debug-log line per distinct reason says why. A headless session changes
// nothing. The mod hooks no classic.* event, which an organization's policy
// plugin passes over.

import type { Register } from 'claude-code'

import { FRAME } from './lib.ts'
import { configRows, noteRow, startFollow, startRows } from './follow.ts'
import { activeSetup, currentLook, framedRow, isFramed, registerSkins } from './skins/hub.tsx'
import type { View } from './skins/hub.tsx'

/** Whether the terminal's view is the ctrl+o transcript, as the turn line
 *  reports it. */
let isTranscriptExpanded = false
/** What the frames read of the view: Claude Code's `verbose` and selected
 *  theme (follow.ts), and the ctrl+o transcript. */
const view = (): View => ({ ...configRows(), isTranscriptExpanded })
/** Each terminal turn line's last `isExpanded`, by row. */
const turnLines = new Map<string, boolean>()

export const register: Register = on => {
  on('session.start', async ($, e, next) => {
    const started = await next(e)
    if (!e.isInteractive) return started
    await startRows($)
    await startFollow($, await currentLook($))
    $.ui.invalidate('ui.render')
    return started
  })

  // Observes `verbose` and `theme` only; the rows are the person's, and the
  // answer is passed on unchanged.
  on('config.set', { key: ['verbose', 'theme'] }, async ($, e, next) => {
    const result = await next(e)
    if (!result.deny) {
      noteRow(e.key, result.value)
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
    if (!isFramed(await activeSetup($), e, view())) return next(e)
    return framedRow($.ui.resolve(e), await next(e), FRAME.reply, e.viewport?.columns)
  })

  on('ui.render', { component: 'ToolUse' }, async ($, e, next) => {
    if (!isFramed(await activeSetup($), e, view())) return next(e)
    return framedRow($.ui.resolve(e), await next(e), e.props.isErrored ? FRAME.toolErrored : FRAME.tool, e.viewport?.columns)
  })

  on('ui.render', { component: 'ToolGroup' }, async ($, e, next) => {
    if (!isFramed(await activeSetup($), e, view())) return next(e)
    return framedRow($.ui.resolve(e), await next(e), FRAME.tool, e.viewport?.columns)
  })

  on('ui.render', { component: 'ToolResult' }, async ($, e, next) => {
    if (!isFramed(await activeSetup($), e, view())) return next(e)
    return framedRow($.ui.resolve(e), await next(e), FRAME.tool, e.viewport?.columns)
  })

  // The skins layer, after the hooks above so they wrap what it draws.
  registerSkins(on, { view })
}
