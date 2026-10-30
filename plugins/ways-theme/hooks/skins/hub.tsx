// The skins layer: skins' drawing of the transcript, headless, configured
// by the active ways theme.
//
// Adapted from skins by hellosverre (MIT; LICENSE beside this file),
// https://github.com/hellosverre/claude-skins at 985ad63, hooks/register.tsx.
// Kept: the tool rows on a rail with their fold, expand and copy, the shell
// card, replies and command output as tables, code, math, charts and markdown
// blocks, the spinner and its words, the turn footer, the usage band and the
// band over AskUserQuestion. Gone: the /skin command and its panes, the design
// tool, the settings store, the update check and star toast, the model-only
// chart hint, the `$.skins` API for other mods, the system-appearance probe,
// the Compact button, and the classic.SessionStart reload.
//
// The look comes from one call, `ways settings theme export --mod`, at an
// interactive session's start and every PICKUP_MS after. Any failure leaves
// the look as it was (none at the start, so every row is the engine's own) and
// logs one debug line per distinct reason. After each later read follow.ts
// takes its turn on the same timer (onPickup), handed the look as it stands. Whether a row is framed is `isFramed`, which the register
// module's frames ask too; the person's message draws here, in a gradient box,
// the outline, or not at all, after `render.prompt_box`, and is Claude Code's
// own while another Claude Code theme than `custom:ways` is selected.

import { atom, memberOf, read, update } from 'claude-code'
import type { EngineInterface, On, RenderElement, RenderSurface, Timer } from 'claude-code'

import { onPickup } from '../follow.ts'
import { FRAME, FRAME_INSET, FRAME_PAD_X, isExpandedView, shouldFrame } from '../lib.ts'

import { MOD_EXPORT_ARGV, parseExport, setupOf } from './config.ts'
import type { Setup } from './config.ts'
import { diffstat, MAX_DRAWN, pick, shortenPath } from './format.ts'
import { bodyOf, inputFields, isOpen } from './detail.ts'
import { DISCLOSE_INSET, disclosedRow, drawsBody, openedRows } from './detail-rows.tsx'
import { commandSegments } from './command-output.ts'
import { askBand, diffCard, footerRow, groupRow, promptRow, quietResult, relayedRows, replyRows, spinnerRow, terminalCard, toolRow, usageBand, desktopSpinnerRow } from './rows.tsx'
import type { Framing, Look, SvgElement, Ui } from './rows.tsx'
import { keepsOwnRow, SHELLS, shellResultOf } from './shell.ts'
import { shellRows } from './shell-rows.tsx'
import { sightings } from './sightings.ts'
import { ICONS } from './skin.ts'
import { hunksOf } from './svg-diff.ts'
import { shellOutputOf } from './svg-terminal.ts'
import { countdownsOf, limitLabel, metersOf } from './svg-usage.ts'
import { NO_ROW, PATH_TOOLS, kindOf, showsImage, summarize, toolLabel } from './tools.ts'
import { fitted, namesImage, withRow } from './pictures.ts'
import { commandOf, errorLine, isQuiet, quietLabel } from './quiet.ts'
import { replySegments } from './reply.ts'
import { holdsCopy, notRoot } from './commands.ts'
import type { IsCommand } from './commands.ts'
import { fileHref, hyperlinksFrom, touchOf } from './links.ts'
import { MIN_WIDTH, axisFor, boxLines, canMeasure, gradientBox, gradientRuns } from './prompt-box.tsx'
import { holdsPaste, pasteIdOf, shownText } from './pastes.ts'
import { holdsBlock, parts } from './wrappers.ts'
import type { Touch, TurnStats, UsageSnap } from '../../types'

/** Each export call's bound. */
export const WAYS_TIMEOUT_MS = 1500
/** How often the export is read again, so a `ways settings` change lands
 *  without a restart. */
export const PICKUP_MS = 5000

const MAX_PROMPT = 4000
// The most of a message body the engine draws.
const MAX_RELAYED = 100_000
const FRAME_MS = 90
const SETTLE_LEAD_MS = FRAME_MS
const FAST_RETRIES = 4
const RETRY_MAX_MS = 1000
const STALE_AHEAD_MS = RETRY_MAX_MS + 1000
const KEPT_TURNS = 40

const lookAtom = atom({ plugin: 'ways-theme', key: 'look' } as const, '')
const startedAtom = atom({ plugin: 'ways-theme', key: 'startedAt' } as const, 0)
const frameAtom = atom({ plugin: 'ways-theme', key: 'frame' } as const, 0)
const turnsAtom = atom({ plugin: 'ways-theme', key: 'turns' } as const, {})
const durationAtom = atom({ plugin: 'ways-theme', key: 'duration' } as const, -1)
const imagesAtom = atom({ plugin: 'ways-theme', key: 'images' } as const, false)
const quietAtom = atom({ plugin: 'ways-theme', key: 'quiet' } as const, false)
const commandAtom = atom({ plugin: 'ways-theme', key: 'command' } as const, '')
const usageAtom = atom({ plugin: 'ways-theme', key: 'usage' } as const, { context: null, limits: [] } as UsageSnap)
const countdownsAtom = atom({ plugin: 'ways-theme', key: 'countdowns' } as const, {} as Record<string, string>)
const settleAtom = atom({ plugin: 'ways-theme', key: 'settle' } as const, 0)
const disclosureAtom = atom({ plugin: 'ways-theme', key: 'disclosure' } as const, 'auto' as 'auto' | 'open' | 'closed')
const touchedAtom = atom({ plugin: 'ways-theme', key: 'touched' } as const, {} as Record<string, Touch>)

const WRITES = new Set(['Edit', 'MultiEdit', 'Write', 'NotebookEdit'])
const EDITS = new Set(['Edit', 'MultiEdit', 'Write'])

// Only a person's own typing becomes a prompt row: a task notification or a peer's
// message is not theirs to dress as theirs.
const TYPED = new Set(['composer', 'bridge', 'sdk'])

// Text the engine stands in for something that is not plain typing.
const NOT_PLAIN = /\[(Pasted text|Image|File) #?\d*/
// A control character no tree of the mod's may carry: the engine draws the row.
const CONTROL = /[\u0000-\u0008\u000b-\u001f\u007f]/

const NO_STATS: TurnStats = { tools: 0, added: 0, removed: 0 }

const debug = ($: EngineInterface, text: string) => $.ui.log(`ways-theme skins: ${text}`, { to: 'debug' })

// The look last decoded, keyed by the atom's text, so a draw parses nothing.
let decoded: { text: string; setup: Setup | null } = { text: '', setup: null }

/** The look the export last gave, or null while none has loaded. A row that
 *  reads it redraws when it changes. */
export async function activeSetup($: EngineInterface): Promise<Setup | null> {
  const text = await read($, lookAtom)
  if (text !== decoded.text) {
    const parsed = text === '' ? 'none' : parseExport(text)
    decoded = { text, setup: typeof parsed === 'string' ? null : setupOf(parsed) }
  }
  return decoded.setup
}

/** What the register module knows of the terminal's view: `verbose`,
 *  whether the view is the ctrl+o transcript, as its TurnDuration hook reads
 *  it, and whether Claude Code's selected theme is `custom:ways`. */
export type View = { isVerbose: boolean; isTranscriptExpanded: boolean; isWaysTheme: boolean }

/** Whether a row draws expanded: the ctrl+o transcript, or the row opened on
 *  its own, and neither under `verbose` (isExpandedView). */
const isExpanded = (view: View, row: unknown): boolean => isExpandedView(view.isTranscriptExpanded || row === true, view.isVerbose)

type Row = { component: string; surface: RenderSurface; props: object; viewport?: { columns: number } }

const drawsRow = (call: unknown): boolean => !NO_ROW.has(String((call as { tool?: unknown }).tool))

/** Whether a frame goes around this row, whole or in pieces. No look loaded,
 *  `render.frames` off, the `terminal` theme or another Claude Code theme than
 *  `custom:ways` draws none. A reply is framed at the block that opens it; a
 *  row whose result skins draws as a shell card is that card's to frame. A
 *  call row Claude Code draws empty, and a group of only such calls, is not
 *  framed; of the results, only a SendUserFile's that sends an image is, the
 *  row its call does not draw. */
function isFramedAround(setup: Setup | null, e: Row, view: View): boolean {
  if (setup === null) return false
  const props = e.props as { isExpanded?: unknown; isFirstOfReply?: unknown; text?: unknown; tool?: unknown; output?: unknown; isErrored?: unknown; calls?: unknown }
  if (e.component === 'AssistantMessage' && props.isFirstOfReply !== true) return false
  if (e.component === 'ToolUse' && !drawsRow(props)) return false
  if (e.component === 'ToolGroup' && Array.isArray(props.calls) && props.calls.length > 0 && !props.calls.some(drawsRow)) return false
  if (e.component === 'ToolResult' && !(props.tool === 'SendUserFile' && showsImage(props.tool, props.output))) return false
  const length = e.component === 'AssistantMessage' && typeof props.text === 'string' ? props.text.length : undefined
  if (!shouldFrame({ surface: e.surface, isThemed: setup.frames && setup.themed, isWaysTheme: view.isWaysTheme, isVerbose: view.isVerbose, isExpanded: isExpanded(view, props.isExpanded), ...(length === undefined ? {} : { length }) })) return false
  if (e.component === 'ToolUse' && typeof props.tool === 'string' && SHELLS.has(props.tool) && setup.prefs.shell) {
    return keepsOwnRow(props.output) || shellResultOf(props.output, props.isErrored === true) === null
  }
  return true
}

/** The columns a reply's prose is drawn in, inside the frame or not. */
const proseWidth = (e: Row, framed: boolean): number => (e.viewport?.columns ?? 100) - (framed ? FRAME_INSET : 0)

/** Whether a framed reply's drawing holds a copy block (commands.ts), which
 *  keeps out of every frame (replyRows). */
function holdsCopyBlock(setup: Setup, e: Row): boolean {
  const text = (e.props as { text?: unknown }).text
  const width = proseWidth(e, true)
  return typeof text === 'string' && holdsCopy(text, width, isCommand) && (replySegments(text, setup.prefs as Look['prefs'], e.surface, width, isCommand)?.some(segment => segment.kind === 'copy') ?? false)
}

/** Whether the frame wraps this row whole. The register module asks it to
 *  draw the frame and this layer to draw the row inside it, of the same look
 *  and view, so the two never disagree. This layer frames a reply holding a
 *  copy block itself, in pieces around the block. */
export function isFramed(setup: Setup | null, e: Row, view: View): boolean {
  return setup !== null && isFramedAround(setup, e, view) && !(e.component === 'AssistantMessage' && holdsCopyBlock(setup, e))
}

/** `inner` inside a frame. The engine's row opens with a blank margin row;
 *  the inner Box takes it back and the frame keeps the gap outside, so the
 *  ground starts at the first line of text. Given the transcript's `columns`,
 *  another mod's picture in the row is narrowed to the frame's room
 *  (pictures.ts). */
export function framedRow(ui: Pick<Ui, 'Box'>, inner: RenderElement, frame: Record<string, string>, columns?: number): RenderElement {
  const { Box } = ui
  return (
    <Box marginTop={1} borderStyle="round" paddingX={FRAME_PAD_X} {...frame}>
      <Box marginTop={-1}>{columns === undefined ? inner : fitted(inner, columns - FRAME_INSET)}</Box>
    </Box>
  )
}

/** A row the frame wraps opens on a blank line, as the engine's own does, for
 *  the frame to take back; inside the frame the rail's connector has nothing
 *  to connect, so the row draws without it. */
function forFrame(look: Look): Look {
  return { ...look, prefs: { ...look.prefs, rail: false } }
}

function marginRow(look: Look, row: ReturnType<Ui['Box']>): ReturnType<Ui['Box']> {
  const { Box } = look.ui
  return (
    <Box flexDirection="column" marginTop={1}>
      {row}
    </Box>
  )
}

/** Why the last read failed, so a reason repeats in the debug log once. */
let lastFailure: string | undefined

/** The look's text as the export last gave it; empty while none has loaded. */
export function currentLook($: EngineInterface): Promise<string> {
  return read($, lookAtom)
}

/** One bounded read of the export into the look atom. A failure keeps the
 *  look the atom holds. */
export async function pickUp($: EngineInterface): Promise<void> {
  let failure: string
  try {
    const run = await $.process.run([...MOD_EXPORT_ARGV], { timeoutMs: WAYS_TIMEOUT_MS })
    if (run.exitCode === 0) {
      const parsed = parseExport(run.stdout)
      if (typeof parsed !== 'string') {
        lastFailure = undefined
        const text = JSON.stringify(parsed)
        if ((await read($, lookAtom)) !== text) {
          await settleEntrances($, true)
          await update($, lookAtom, () => text)
        }
        return
      }
      failure = parsed
    } else {
      failure = `the export exited ${run.exitCode}`
    }
  } catch (err) {
    failure = `the export did not run: ${err instanceof Error ? err.message : String(err)}`
  }
  if (failure !== lastFailure) {
    lastFailure = failure
    debug($, `${failure}; the look stays as it was`)
  }
}

// Read once a session or a turn and kept here: each read through `$` is a dispatch, and
// every row draws often.
let hyperlinks = false
let home: string | undefined
let sessionCwd: string | undefined
let hasTouched = false

const cwdOf = async ($: EngineInterface): Promise<string> => sessionCwd ?? (await $.session.cwd())

// The slash commands the person can run, by name, as `$.command.list()` last answered:
// built-in, plugin and MCP alike. Null until it has answered, and where the engine has no
// list or the read failed; a reply's slash blocks then fall back to `notRoot` (commands.ts).
let commandNames: ReadonlySet<string> | null = null

const isCommand: IsCommand = name => (commandNames === null ? notRoot(name) : commandNames.has(name))

// Reads the command list, and redraws when it changed. Claude Code names no event for a
// change to it, and an MCP server or another mod's command can arrive after the session
// starts, so it is read at the session's start and at each turn of the main loop.
async function readCommands($: EngineInterface): Promise<void> {
  const names = new Set((await $.command.list()).map(command => command.name))
  if (commandNames !== null && commandNames.size === names.size && [...names].every(name => commandNames!.has(name))) return
  commandNames = names
  $.ui.invalidate('ui.render')
}

// The id Claude Code gives this session's pastes, worked out by `idOf` once per
// session id; null when the session id or its digest cannot be had.
let pastes: { session: string; id: string } | undefined
export async function sessionPasteId($: Pick<EngineInterface, 'session'>, idOf = pasteIdOf): Promise<string | null> {
  try {
    const session = await $.session.id()
    if (pastes?.session !== session) pastes = { session, id: await idOf(session) }
    return pastes.id
  } catch {
    return null
  }
}

const lookOf = (ui: Ui & { Svg?: SvgElement }, setup: Setup, surface: RenderSurface, copy?: (text: string) => void): Look => ({
  ui,
  skin: setup.skin,
  icons: ICONS[setup.prefs.icons],
  prefs: setup.prefs as Look['prefs'],
  surface,
  ...(surface !== 'terminal' && ui.Svg !== undefined ? { svg: ui.Svg } : {}),
  ...(copy === undefined || !setup.prefs.copy ? {} : { copy }),
})

type Needs = { links?: boolean; touched?: boolean; cwd?: string }

async function sessionLook($: EngineInterface, e: { requestId: string; surface: RenderSurface }, look: Look, needs: Needs): Promise<Look> {
  const links = needs.links === true && look.prefs.links && (e.surface !== 'terminal' || hyperlinks)
  // A `file://` URL names this machine, which only the terminal is on.
  const paths = links && e.surface === 'terminal' ? { cwd: needs.cwd ?? (await cwdOf($)), ...(home === undefined ? {} : { home }) } : undefined
  const touched = needs.touched === true && hasTouched ? { files: await read($, touchedAtom), cwd: needs.cwd ?? (await cwdOf($)) } : undefined

  return {
    ...look,
    links,
    ...(paths === undefined ? {} : { paths }),
    ...(touched === undefined ? {} : { touched }),
  }
}

// Text that may hold a path: one slash between names, a file name, or a home path.
const hasPathish = (text: string): boolean => /[\w.-]\/[\w.-]|\w\.[A-Za-z]{1,5}\b|~\//.test(text)

const replyNeeds = (text: string, segments: readonly { kind: string }[]): Needs => ({
  links: /https?:\/\//.test(text) || hasPathish(text),
  touched: hasPathish(text),
})

// Whether the terminal draws links, and the home a `~/` path links under.
async function readLinkEnv($: EngineInterface): Promise<void> {
  // Each name spelled out: the engine lists the variables a module reads from its source.
  hyperlinks = hyperlinksFrom({
    FORCE_HYPERLINK: await $.env.get('FORCE_HYPERLINK'),
    TERM_PROGRAM: await $.env.get('TERM_PROGRAM'),
    TERM_PROGRAM_VERSION: await $.env.get('TERM_PROGRAM_VERSION'),
    TERM: await $.env.get('TERM'),
    WT_SESSION: await $.env.get('WT_SESSION'),
    VTE_VERSION: await $.env.get('VTE_VERSION'),
    KONSOLE_VERSION: await $.env.get('KONSOLE_VERSION'),
    TMUX: await $.env.get('TMUX'),
    CI: await $.env.get('CI'),
  })
  home = (await $.env.get('HOME')) ?? (await $.env.get('USERPROFILE'))
}

// A drawing's cards animate on their first draw only, keyed by the drawing's instance.
const seenCards = sightings()
const settling = new Map<string, { at: number; cards: string[] }>()
let settlingNow = new Set<string>()
let refusals = 0

const drawingOf = (e: { surface: RenderSurface; requestId: string }): string => `${e.surface}:${e.requestId}`

const groupStarts = new Map<string, boolean>()

const drawnBy = (drawing: string, drawnAt: number) => (key: string, entranceMs: number) => {
  const card = `${drawing}:${key}`
  if (settlingNow.delete(card)) {
    seenCards(card)
    return false
  }
  const isFirst = seenCards(card)
  if (isFirst) {
    const pending = settling.get(drawing)
    settling.set(drawing, {
      at: Math.max(pending?.at ?? 0, drawnAt + entranceMs - SETTLE_LEAD_MS),
      cards: [...(pending?.cards ?? []), card],
    })
  }
  return isFirst
}

async function firstDraws($: EngineInterface, e: { surface: RenderSurface; requestId: string }): Promise<(key: string, entranceMs: number) => boolean> {
  const drawing = drawingOf(e)
  settling.delete(drawing)
  return drawnBy(drawing, await $.clock.now())
}

async function withSettle<T>($: EngineInterface, e: { surface: RenderSurface; requestId: string }, drawn: T): Promise<T> {
  if (settling.has(drawingOf(e))) await read($, settleAtom)
  return drawn
}

async function settleEntrances($: EngineInterface, isForced = false): Promise<boolean> {
  const now = await $.clock.now()
  const isDue = (at: number): boolean => at <= now || at > now + STALE_AHEAD_MS
  if (settling.size === 0 || (!isForced && ![...settling.values()].some(pending => isDue(pending.at)))) return true
  const moved = [...settling]
  settlingNow = new Set(moved.flatMap(([, pending]) => pending.cards))
  settling.clear()
  return update($, settleAtom, round => round + 1).then(
    () => {
      refusals = 0
      return true
    },
    (error: unknown) => {
      refusals += 1
      const retryAt = now + Math.min(RETRY_MAX_MS, refusals <= FAST_RETRIES ? 0 : FRAME_MS * 2 ** (refusals - FAST_RETRIES))
      for (const [drawing, pending] of moved) {
        if (!settling.has(drawing)) settling.set(drawing, { ...pending, at: retryAt })
      }
      debug($, `settle: ${error instanceof Error ? error.message : String(error)}`)
      return false
    },
  )
}

async function refreshUsage($: EngineInterface): Promise<void> {
  const usage = await $.session.usage()
  const snap: UsageSnap = {
    context: usage.context.percent ?? null,
    limits: usage.rateLimits.map(limit => {
      const resetsAt = limit.resetsAt === undefined ? NaN : Date.parse(limit.resetsAt)
      return { label: limitLabel(limit.kind), percent: limit.percentUsed, ...(Number.isNaN(resetsAt) ? {} : { resetsAt }) }
    }),
  }
  const drawn = (of: UsageSnap) => JSON.stringify([metersOf(of), of.limits.map(limit => limit.resetsAt ?? null)])
  if (drawn(await read($, usageAtom)) === drawn(snap)) return
  await settleEntrances($, true)
  await update($, usageAtom, () => snap)
}

const quietly = ($: EngineInterface, what: string) => (error: unknown) => debug($, `${what}: ${error instanceof Error ? error.message : String(error)}`)

/** Puts a card's text on the clipboard of the surface that drew it. */
// A press reaches a terminal row only under the fullscreen layout: on the main screen a
// row is printed into scrollback, where no Button can be pressed, so it draws no Copy.
function copier($: EngineInterface, e: { surface: RenderSurface; viewport?: { isFullscreen?: boolean } }): ((text: string) => void) | undefined {
  if (e.surface === 'terminal' && e.viewport?.isFullscreen !== true) return undefined
  return text => {
    void $.ui.copy({ text, surface: e.surface }).then(result => $.ui.toast(result.isCopied ? 'Copied' : 'Could not copy here'))
  }
}

/** What the register module shares: the terminal's view. */
export type Shared = { view: () => View }

/** Registers the skins layer. Called last in the module's register, so each
 *  hook here sits inside the module's own: the frames wrap what it draws. */
export function registerSkins(on: On, shared: Shared): void {
  let stats: TurnStats = NO_STATS
  let isWorking = false
  let ticker: Timer | undefined
  let pickup: Timer | undefined

  // A headless session draws nothing, so it reads nothing.
  on('session.start', { isInteractive: true }, async ($, e, next) => {
    lastFailure = undefined
    await pickUp($)
    sessionCwd = e.cwd
    await readLinkEnv($).catch(quietly($, 'link check'))
    await refreshUsage($).catch(quietly($, 'usage'))
    await readCommands($).catch(quietly($, 'command list'))
    ticker?.cancel()
    pickup?.cancel()
    ticker = $.clock.every(FRAME_MS, () => {
      if (isWorking) void update($, frameAtom, frame => frame + 1)
      if (settling.size > 0) void settleEntrances($)
    })
    pickup = $.clock.every(PICKUP_MS, async () => {
      await pickUp($)
      await onPickup($, await read($, lookAtom)).catch(quietly($, 'pickup'))
    })
    return next(e)
  })

  on('ui.invalidate', async ($, e, next) => {
    if (e.event === 'ui.render' && !(await settleEntrances($, true)) && refusals <= FAST_RETRIES) {
      await $.clock.sleep(FRAME_MS / 3)
      await settleEntrances($, true)
    }
    return next(e)
  })

  on('turn.start', async ($, e, next) => {
    if (e.agentId === undefined) await readCommands($).catch(quietly($, 'command list'))
    stats = NO_STATS
    isWorking = true
    sessionCwd = await $.session.cwd()
    hasTouched = false
    await update($, touchedAtom, () => ({}))
    const now = await $.clock.now()
    await update($, startedAtom, () => now)
    return next(e)
  })

  // Notes which prompts carried an image or another block that is not text, so their
  // row keeps Claude Code's drawing of it.
  on('session.append', { door: 'prompt' }, async ($, e, next) => {
    const stored = await next(e)
    const hasMore = e.message.content.some(block => block.type !== 'text')
    if (hasMore && stored.uuid !== undefined) await update($, memberOf(imagesAtom, { requestId: stored.uuid }), () => true)
    return stored
  })

  on('session.measure', async ($, e, next) => {
    await refreshUsage($).catch(quietly($, 'usage'))
    return next(e)
  })

  on('turn.complete', async ($, e, next) => {
    if (e.agentId === undefined) {
      await refreshUsage($).catch(quietly($, 'usage'))
      isWorking = false
      const finished = stats
      await update($, turnsAtom, turns => Object.fromEntries([...Object.entries(turns), [String(e.durationMs), finished]].slice(-KEPT_TURNS)))
    }
    return next(e)
  })

  // Times every call and counts the main loop's calls and changed lines.
  on('tool.call', async ($, e, next) => {
    if (SHELLS.has(e.tool)) {
      const command = commandOf(e)
      await update($, memberOf(commandAtom, { requestId: e.tool_use_id }), () => command)
    }
    const startedAt = await $.clock.now()
    const ran = await next(e)
    const ms = (await $.clock.now()) - startedAt
    await update($, memberOf(durationAtom, { requestId: e.tool_use_id }), () => ms)
    if (isQuiet(e.tool, e)) await update($, memberOf(quietAtom, { requestId: e.tool_use_id }), () => true)
    const file = (e as { file_path?: unknown }).file_path ?? (e as { notebook_path?: unknown }).notebook_path
    if (WRITES.has(e.tool) && ran.deny === undefined && typeof file === 'string' && file !== '') {
      const touch: Touch = (ran.result as { type?: unknown } | undefined)?.type === 'create' ? 'created' : 'edited'
      hasTouched = true
      await update($, touchedAtom, touched => (touched[file] === 'created' ? touched : { ...touched, [file]: touch }))
    }
    if (e.agentId === undefined && ran.deny === undefined) {
      const diff = diffstat(ran.result)
      stats = { tools: stats.tools + 1, added: stats.added + (diff?.added ?? 0), removed: stats.removed + (diff?.removed ?? 0) }
    }
    return ran
  })

  on('ui.render', { component: 'ToolUse' }, async ($, e, next) => {
    const kind = kindOf(e.props.tool)
    const setup = await activeSetup($)
    if (kind === null || setup === null) return next(e)

    const cwd = await cwdOf($)
    const ms = e.props.isRunning ? -1 : await read($, memberOf(durationAtom, e))
    const diff = diffstat(e.props.output)
    const meta = { ...(ms >= 0 && !e.props.isRunning ? { ms } : {}), ...(diff === null ? {} : diff) }
    const fields = (e.props.input ?? {}) as { file_path?: unknown; notebook_path?: unknown }
    const path = fields.file_path ?? fields.notebook_path
    const isPathRow = PATH_TOOLS.has(e.props.tool) && typeof path === 'string'
    const target = summarize(e.props.tool, e.props.input, cwd)
    const disclosure = memberOf(disclosureAtom, e)
    const open = isOpen(await read($, disclosure), e.props.isErrored)
    const base = await sessionLook($, e, lookOf($.ui.resolve(e), setup, e.surface, copier($, e)), { links: open || typeof path === 'string', touched: typeof path === 'string', cwd })
    const framed = isFramed(setup, e, shared.view())
    const plain = base.svg === undefined ? base : { ...base, isFirstDraw: await firstDraws($, e) }
    const look = framed ? forFrame(plain) : plain
    const touch = typeof path === 'string' && look.touched !== undefined ? touchOf(look.touched.files, path, cwd) : undefined
    const label = setup.prefs.quiet && isQuiet(e.props.tool, e.props.input) ? quietLabel(e.props.tool) : toolLabel(e.props.tool)
    const href = typeof path === 'string' && look.links === true && look.paths !== undefined ? fileHref(path, look.paths) : null
    // A file's path may wrap under the label, in the row's room beside the frame and the chevron.
    const room = isPathRow ? (e.viewport?.columns ?? 100) - (framed ? FRAME_INSET : 0) - DISCLOSE_INSET : undefined
    const row = toolRow(look, e.props, kind, target, meta, label, touch, href, room)
    const toggle = () => void update($, disclosure, () => (open ? 'closed' : 'open'))

    // What draws beneath, asked once. A Read of an image keeps the picture a mod
    // beneath draws (pictures.ts), unless the opened body already holds it.
    let beneath: Promise<RenderElement> | undefined
    let isStockBody = false
    const below = () => (beneath ??= next(e))
    const stock = () => {
      isStockBody = true
      return below()
    }
    const drawn = async (row: ReturnType<Ui['Box']>) => {
      const shown = showsImage(e.props.tool, e.props.output) && !isStockBody ? withRow(await below(), row) : row
      return withSettle($, e, framed ? marginRow(look, shown) : shown)
    }

    if (!open) return drawn(disclosedRow(look, row, false, toggle))

    const opened = await openedRows(look, {
      fields: inputFields(e.props.tool, e.props.input, cwd),
      body: bodyOf(e.props.tool, e.props.output, e.props.isErrored, e.props.isRunning, cwd),
      command: commandOf(e.props.input),
      isErrored: e.props.isErrored,
      cwd,
      columns: e.viewport?.columns ?? 100,
      stock,
    })
    return drawn(disclosedRow(look, row, true, toggle, opened))
  })

  on('ui.render', { component: 'ToolGroup' }, async ($, e, next) => {
    const setup = await activeSetup($)
    if (setup === null) return next(e)
    const drawing = drawingOf(e)
    if (setup.prefs.expand) {
      const start = groupStarts.get(drawing) ?? e.props.isExpanded
      groupStarts.set(drawing, start)
      if (e.props.isExpanded === start) return next({ ...e, props: { ...e.props, isExpanded: true } })
    } else {
      groupStarts.delete(drawing)
      if (e.props.isExpanded) return next(e)
    }
    // A group of only calls Claude Code draws no row for is its own.
    if (e.props.calls.length > 0 && !e.props.calls.some(drawsRow)) return next(e)
    const look = lookOf($.ui.resolve(e), setup, e.surface)
    const framed = isFramed(setup, e, shared.view())
    const row = groupRow(framed ? forFrame(look) : look, e.props.calls)
    // Folded Reads of images keep the pictures a mod beneath draws under the line (pictures.ts).
    const shown = e.props.calls.some(call => showsImage(call.tool, call.output)) ? withRow(await next(e), row) : row
    return framed ? marginRow(look, shown) : shown
  })

  on('ui.render', { component: 'ToolResult' }, async ($, e, next) => {
    const setup = await activeSetup($)
    if (setup === null) return next(e)
    const look = await sessionLook($, e, lookOf($.ui.resolve(e), setup, e.surface, copier($, e)), {})
    const columns = e.viewport?.columns ?? 100
    // An opened row draws the answer itself; one it cannot draw keeps Claude Code's here.
    const isOpened = kindOf(e.props.tool) !== null && isOpen(await read($, memberOf(disclosureAtom, e)), e.props.isErrored)
    const cwd = isOpened ? await cwdOf($) : ''
    if (isOpened && drawsBody(look, { body: bodyOf(e.props.tool, e.props.output, e.props.isErrored, false, cwd), command: '', isErrored: e.props.isErrored, cwd, columns })) {
      const { Box } = $.ui.resolve(e)
      return <Box />
    }
    if (look.prefs.quiet && (await read($, memberOf(quietAtom, e)))) {
      return quietResult(look, e.props.isErrored ? errorLine(e.props.output) : null)
    }
    if (look.svg !== undefined && EDITS.has(e.props.tool) && !e.props.isErrored) {
      const diff = hunksOf(e.props.output)
      const card = diff === null ? null : diffCard({ ...look, isFirstDraw: await firstDraws($, e) }, look.svg, diff, shortenPath(diff.path, await $.session.cwd()), columns)
      if (card !== null) return withSettle($, e, card)
    }
    const isShellCard = look.prefs.shell && SHELLS.has(e.props.tool) && !keepsOwnRow(e.props.output)
    if (look.svg !== undefined && isShellCard) {
      const shell = shellOutputOf(e.props.output)
      const card = shell === null ? null : terminalCard({ ...look, isFirstDraw: await firstDraws($, e) }, look.svg, shell, e.props.isErrored, columns)
      if (card !== null) return withSettle($, e, card)
    }
    if (e.surface === 'terminal' && isShellCard) {
      const shell = shellResultOf(e.props.output, e.props.isErrored)
      if (shell !== null) return shellRows(look, await read($, memberOf(commandAtom, e)), shell, e.props.isErrored)
    }
    // A framed result opens on a blank line for the frame to take back.
    if (isFramed(setup, e, shared.view())) return marginRow(look, await next(e))
    return next(e)
  })

  // The person's message, after `render.prompt_box`, its pastes unwrapped as Claude Code
  // shows them (pastes.ts). Another party's message, where the row draws it in full, has
  // its wrapper blocks set apart (wrappers.ts); its compact row is the engine's, or the
  // drawing of the mod that sent it, and one carrying an image is the engine's, which
  // draws the image. Off the terminal both keep Claude Code's own
  // component: it carries the copy and rewind buttons, the context menu and selectable
  // text, which a drawing of the mod's would lose.
  on('ui.render', { component: 'UserMessage' }, async ($, e, next) => {
    const setup = await activeSetup($)
    if (setup === null || e.surface !== 'terminal' || !shared.view().isWaysTheme || CONTROL.test(e.props.text)) return next(e)
    if (!TYPED.has(e.props.origin.kind)) {
      if (!e.props.isExpanded || e.props.text.length > MAX_RELAYED || !holdsBlock(e.props.text) || (await read($, memberOf(imagesAtom, e)))) return next(e)
      const all = parts(e.props.text)
      return all.every(part => part.kind === 'text') ? next(e) : relayedRows(lookOf($.ui.resolve(e), setup, e.surface), all)
    }
    if (setup.promptBox === 'off' || e.props.text.length > MAX_PROMPT) return next(e)
    // A paste is unwrapped by the session's paste id; without it, Claude Code draws.
    let text = e.props.text
    if (holdsPaste(text)) {
      const pasteId = await sessionPasteId($)
      if (pasteId === null) return next(e)
      text = shownText(text, pasteId)
    }
    const ui = $.ui.resolve(e)
    const hasMore = await read($, memberOf(imagesAtom, e))
    const isRowOpen = isExpanded(shared.view(), e.props.isExpanded)
    const width = (e.viewport?.columns ?? 80) - 1
    const canGradient = setup.promptBox === 'gradient' && !hasMore && !isRowOpen && !NOT_PLAIN.test(text) && canMeasure(text) && width >= MIN_WIDTH
    if (canGradient && setup.ground !== null) {
      const lines = boxLines(text, width)
      const runs = gradientRuns(lines, width, setup.ground.stops, axisFor(lines.length - 2))
      return gradientBox(ui, runs, setup.ground.text)
    }
    const look = lookOf(ui, setup, e.surface)
    // Claude Code draws the images; the text is already in the outline above them.
    return promptRow(look, text, hasMore ? await next({ ...e, props: { ...e.props, text: '' } }) : undefined)
  })

  // A reply keeps Claude Code's own drawing unless it holds a card or markdown skins draws.
  on('ui.render', { component: 'AssistantMessage' }, async ($, e, next) => {
    const setup = await activeSetup($)
    if (setup === null) return next(e)
    const text = e.props.text
    const framed = isFramedAround(setup, e, shared.view())
    const segments = replySegments(text, setup.prefs as Look['prefs'], e.surface, proseWidth(e, framed), isCommand)
    if (segments === null) return next(e)
    const ui = $.ui.resolve(e)
    const svg = e.surface !== 'terminal' && setup.prefs.tables === 'on' && 'Svg' in ui ? ui.Svg : undefined
    const base = await sessionLook($, e, lookOf(ui, setup, e.surface, copier($, e)), replyNeeds(text, segments))
    const look = base.svg === undefined ? base : { ...base, isFirstDraw: await firstDraws($, e) }
    const columns = e.viewport?.columns ?? 100
    const whole = segments.length > 1 ? text : undefined
    // A reply that names an image file keeps the picture a mod beneath draws (pictures.ts).
    const pictured = async (rows: ReturnType<Ui['Box']>) => (e.surface === 'terminal' && namesImage(text) ? withRow(await next(e), rows) : rows)
    // A framed reply is laid out inside the frame the register module draws around it,
    // or, holding a copy block, in frames of this layer's around each run of blocks.
    if (!framed) return withSettle($, e, await pictured(replyRows(look, segments, columns, svg, whole)))
    if (segments.some(segment => segment.kind === 'copy')) {
      const framing: Framing = { inset: FRAME_INSET, frame: rows => framedRow(ui, marginRow(look, rows), FRAME.reply) }
      return withSettle($, e, await pictured(replyRows(look, segments, columns, svg, whole, framing)))
    }
    return withSettle($, e, marginRow(look, await pictured(replyRows(look, segments, columns - FRAME_INSET, svg, whole))))
  })

  // A slash command's output drawn the way a reply is.
  on('ui.render', { component: 'CommandOutput' }, async ($, e, next) => {
    const setup = await activeSetup($)
    if (setup === null || !setup.prefs.commands || setup.prefs.tables === 'off' || e.props.isErrored) return next(e)
    const segments = e.props.text.length > MAX_DRAWN ? null : commandSegments(e.props.text, e.props.command)
    if (segments === null) return next(e)
    const ui = $.ui.resolve(e)
    const svg = e.surface !== 'terminal' && setup.prefs.tables === 'on' && 'Svg' in ui ? ui.Svg : undefined
    const base = await sessionLook($, e, lookOf(ui, setup, e.surface, copier($, e)), replyNeeds(e.props.text, segments))
    const look = base.svg === undefined ? base : { ...base, isFirstDraw: await firstDraws($, e) }
    return withSettle($, e, replyRows(look, segments, e.viewport?.columns ?? 100, svg))
  })

  // The terminal's spinner gets the theme's word with a shimmer; the desktop's keeps its
  // word beside an animated icon.
  on('ui.render', { component: 'Spinner' }, async ($, e, next) => {
    const setup = await activeSetup($)
    if (setup === null) return next(e)
    if (e.surface !== 'terminal') {
      const look = lookOf($.ui.resolve(e), setup, e.surface)
      return look.svg === undefined ? next(e) : desktopSpinnerRow(look, look.svg, e.props.mode, e.props.message ?? e.props.word)
    }
    const word = setup.prefs.words === 'claude' ? e.props.word : (pick(setup.skin.spinner, e.props.word) ?? e.props.word)
    if (!setup.prefs.shimmer || e.props.message !== null) return next({ ...e, props: { ...e.props, word } })
    const frame = await read($, frameAtom)
    const startedAt = await read($, startedAtom)
    const elapsed = startedAt === 0 ? 0 : (await $.clock.now()) - startedAt
    return spinnerRow(lookOf($.ui.resolve(e), setup, e.surface), word, frame, elapsed)
  })

  on('ui.render', { component: 'TurnDuration' }, async ($, e, next) => {
    const setup = await activeSetup($)
    if (setup === null) return next(e)
    const turns = await read($, turnsAtom)
    const word = setup.prefs.words === 'claude' ? e.props.word : (pick(setup.skin.done, e.props.word) ?? e.props.word)
    return footerRow(lookOf($.ui.resolve(e), setup, e.surface), word, e.props.durationMs, turns[String(e.props.durationMs)])
  })

  // The band above the prompt: context and plan limits. Another mod's drawing there,
  // and a survey, keep their place.
  on('ui.render', { component: 'AbovePrompt' }, async ($, e, next) => {
    const setup = await activeSetup($)
    const usage = await read($, usageAtom)
    const meters = metersOf(usage, setup?.prefs.countdown === true ? (await read($, countdownsAtom), countdownsOf(usage, await $.clock.now())) : {})
    if (setup === null || !setup.prefs.band || e.props.hasSurvey || meters.length === 0) return next(e)
    const plain = lookOf($.ui.resolve(e), setup, e.surface)
    const { Box } = plain.ui
    const theirs = await next(e)
    const look = { ...plain, ...(plain.svg === undefined ? {} : { isFirstDraw: await firstDraws($, e) }) }
    return withSettle(
      $,
      e,
      <Box flexDirection="column">
        {usageBand(look, meters, false, () => undefined)}
        {theirs}
      </Box>,
    )
  })

  // Claude Code's own dialog stays whole: the skin only adds a band above it.
  on('ui.render', { component: 'AskUserQuestion' }, async ($, e, next) => {
    const setup = await activeSetup($)
    const theirs = await next(e)
    if (setup === null) return theirs
    const headers = e.props.questions
      .map(question => (question as { header?: unknown } | null)?.header)
      .filter((header): header is string => typeof header === 'string' && header !== '')
    const look = lookOf($.ui.resolve(e), setup, e.surface)
    const { Box } = look.ui
    return headers.length === 0 ? (
      theirs
    ) : (
      <Box flexDirection="column">
        {askBand(look, headers)}
        {theirs}
      </Box>
    )
  })
}
