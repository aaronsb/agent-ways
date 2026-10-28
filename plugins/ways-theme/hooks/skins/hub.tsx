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
// logs one debug line per distinct reason. The register module's frames ask
// `framesWanted` whether to draw; the person's message draws here, in a
// gradient box, the outline, or not at all, after `render.prompt_box`.

import { atom, memberOf, read, update } from 'claude-code'
import type { EngineInterface, On, RenderSurface, Timer } from 'claude-code'

import { MOD_EXPORT_ARGV, parseExport, setupOf } from './config.ts'
import type { Setup } from './config.ts'
import { clipLines, diffstat, pick, shortenPath } from './format.ts'
import { bodyOf, inputFields, isOpen } from './detail.ts'
import { disclosedRow, openedRows } from './detail-rows.tsx'
import { commandSegments } from './command-output.ts'
import { askBand, diffCard, footerRow, groupRow, promptRow, quietResult, replyRows, spinnerRow, terminalCard, toolRow, usageBand, desktopSpinnerRow } from './rows.tsx'
import type { Look, SvgElement, Ui } from './rows.tsx'
import { keepsOwnRow, SHELLS, shellResultOf } from './shell.ts'
import { OUTPUT_FOLD, shellRows } from './shell-rows.tsx'
import { sightings } from './sightings.ts'
import { ICONS } from './skin.ts'
import { hunksOf } from './svg-diff.ts'
import { shellOutputOf } from './svg-terminal.ts'
import { countdownsOf, limitLabel, metersOf } from './svg-usage.ts'
import { kindOf, summarize, toolLabel } from './tools.ts'
import { commandOf, errorLine, isQuiet, quietLabel } from './quiet.ts'
import { MAX_MARKDOWN, replySegments } from './reply.ts'
import { hyperlinksFrom, touchOf } from './links.ts'
import { MIN_WIDTH, axisFor, boxLines, gradientBox, gradientRuns } from './prompt-box.tsx'
import type { Touch, TurnStats, UsageSnap } from './types.ts'

/** Each export call's bound. */
export const WAYS_TIMEOUT_MS = 1500
/** How often the export is read again, so a `ways settings` change lands
 *  without a restart. */
export const PICKUP_MS = 5000

const MAX_PROMPT = 4000
const CLIP_HEAD = 8
const CLIP_TAIL = 4
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
const allAtom = atom({ plugin: 'ways-theme', key: 'showAll' } as const, false)
const touchedAtom = atom({ plugin: 'ways-theme', key: 'touched' } as const, {} as Record<string, Touch>)
const foldsAtom = atom({ plugin: 'ways-theme', key: 'folds' } as const, [] as string[])

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

async function activeSetup($: EngineInterface): Promise<Setup | null> {
  const text = await read($, lookAtom)
  if (text !== decoded.text) {
    const parsed = text === '' ? 'none' : parseExport(text)
    decoded = { text, setup: typeof parsed === 'string' ? null : setupOf(parsed) }
  }
  return decoded.setup
}

/** Whether the register module's reply and tool frames draw: `render.frames`,
 *  and true while no export has loaded, so a failed export leaves them as they
 *  were. A row whose result skins draws as a shell card is that card's to
 *  frame. */
export function framesWanted(e: object): boolean {
  const setup = decoded.setup
  if (setup === null) return true
  if (!setup.frames) return false
  const row = e as { component?: unknown; props?: { tool?: unknown; output?: unknown; isErrored?: unknown } }
  if (row.component === 'ToolUse' && typeof row.props?.tool === 'string' && SHELLS.has(row.props.tool) && setup.prefs.shell) {
    return keepsOwnRow(row.props.output) || shellResultOf(row.props.output, row.props.isErrored === true) === null
  }
  return true
}

/** Whether the register module's frame will wrap this row: the same reading
 *  of the theme, the surface and ctrl+o that its own hooks make. */
function isFramed(setup: Setup, e: { component: string; surface: RenderSurface }, isExpanded: boolean): boolean {
  return setup.frames && setup.themed && e.surface === 'terminal' && !isExpanded && framesWanted(e)
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
let sessionCwd: string | undefined
let hasTouched = false

const cwdOf = async ($: EngineInterface): Promise<string> => sessionCwd ?? (await $.session.cwd())

const lookOf = (ui: Ui & { Svg?: SvgElement }, setup: Setup, surface: RenderSurface, copy?: (text: string) => void): Look => ({
  ui,
  skin: setup.skin,
  icons: ICONS[setup.prefs.icons],
  prefs: setup.prefs as Look['prefs'],
  surface,
  ...(surface !== 'terminal' && ui.Svg !== undefined ? { svg: ui.Svg } : {}),
  ...(copy === undefined || !setup.prefs.copy ? {} : { copy }),
})

type Needs = { links?: boolean; touched?: boolean; folds?: boolean; cwd?: string }

async function sessionLook($: EngineInterface, e: { requestId: string; surface: RenderSurface }, look: Look, needs: Needs): Promise<Look> {
  const folds = memberOf(foldsAtom, e)
  const links = needs.links === true && look.prefs.links && (e.surface !== 'terminal' || hyperlinks)
  const touched = needs.touched === true && hasTouched ? { files: await read($, touchedAtom), cwd: needs.cwd ?? (await cwdOf($)) } : undefined
  const open = needs.folds === true && look.prefs.fold ? new Set(await read($, folds)) : undefined

  return {
    ...look,
    links,
    ...(touched === undefined ? {} : { touched }),
    ...(open === undefined
      ? {}
      : {
          folds: {
            open,
            toggle: (key: string) => void update($, folds, keys => (keys.includes(key) ? keys.filter(kept => kept !== key) : [...keys, key])),
          },
        }),
  }
}

const replyNeeds = (text: string, segments: readonly { kind: string }[]): Needs => ({
  links: /https?:\/\//.test(text),
  touched: /[\w.-]\/[\w.-]|\w\.[A-Za-z]{1,5}\b/.test(text),
  folds: segments.some(segment => segment.kind === 'code' || segment.kind === 'table'),
})

async function readHyperlinks($: EngineInterface): Promise<void> {
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
function copier($: EngineInterface, surface: RenderSurface): (text: string) => void {
  return text => {
    void $.ui.copy({ text, surface }).then(result => $.ui.toast(result.isCopied ? 'Copied' : 'Could not copy here'))
  }
}

/** What the register module shares: whether the terminal's view is the ctrl+o
 *  transcript, as its TurnDuration hook reads it. */
export type Shared = { isTranscriptExpanded: () => boolean }

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
    await readHyperlinks($).catch(quietly($, 'hyperlink check'))
    await refreshUsage($).catch(quietly($, 'usage'))
    ticker?.cancel()
    pickup?.cancel()
    ticker = $.clock.every(FRAME_MS, () => {
      if (isWorking) void update($, frameAtom, frame => frame + 1)
      if (settling.size > 0) void settleEntrances($)
    })
    pickup = $.clock.every(PICKUP_MS, () => void pickUp($))
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
    const target = summarize(e.props.tool, e.props.input, cwd)
    const meta = { ...(ms >= 0 && !e.props.isRunning ? { ms } : {}), ...(diff === null ? {} : diff) }
    const fields = (e.props.input ?? {}) as { file_path?: unknown; notebook_path?: unknown }
    const path = fields.file_path ?? fields.notebook_path
    const disclosure = memberOf(disclosureAtom, e)
    const open = isOpen(await read($, disclosure), e.props.isErrored)
    const base = await sessionLook($, e, lookOf($.ui.resolve(e), setup, e.surface, copier($, e.surface)), { touched: typeof path === 'string', folds: open, cwd })
    const framed = isFramed(setup, e, shared.isTranscriptExpanded() || e.props.isExpanded === true)
    const plain = base.svg === undefined ? base : { ...base, isFirstDraw: await firstDraws($, e) }
    const look = framed ? forFrame(plain) : plain
    const touch = typeof path === 'string' && look.touched !== undefined ? touchOf(look.touched.files, path, cwd) : undefined
    const label = setup.prefs.quiet && isQuiet(e.props.tool, e.props.input) ? quietLabel(e.props.tool) : toolLabel(e.props.tool)
    const row = toolRow(look, e.props, kind, target, meta, label, touch)
    const toggle = () => void update($, disclosure, () => (open ? 'closed' : 'open'))

    const drawn = (row: ReturnType<Ui['Box']>) => withSettle($, e, framed ? marginRow(look, row) : row)

    if (!open) return drawn(disclosedRow(look, row, false, toggle))

    const all = memberOf(allAtom, e)
    const isAll = await read($, all)
    const opened = await openedRows(look, {
      fields: inputFields(e.props.tool, e.props.input, cwd),
      body: bodyOf(e.props.tool, e.props.output, e.props.isErrored, e.props.isRunning, cwd),
      command: commandOf(e.props.input),
      isErrored: e.props.isErrored,
      cwd,
      columns: e.viewport?.columns ?? 100,
      isAll,
      toggleAll: () => void update($, all, () => !isAll),
      stock: () => next(e),
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
    const look = lookOf($.ui.resolve(e), setup, e.surface)
    return isFramed(setup, e, shared.isTranscriptExpanded()) ? marginRow(look, groupRow(forFrame(look), e.props.calls)) : groupRow(look, e.props.calls)
  })

  on('ui.render', { component: 'ToolResult' }, async ($, e, next) => {
    const setup = await activeSetup($)
    if (setup === null) return next(e)
    const output = e.props.output as { stdout?: unknown } | null
    const needs = { folds: e.surface === 'terminal' && SHELLS.has(e.props.tool) }
    const look = await sessionLook($, e, lookOf($.ui.resolve(e), setup, e.surface, copier($, e.surface)), needs)
    const columns = e.viewport?.columns ?? 100
    // An opened row draws the answer itself; one it cannot draw keeps Claude Code's here.
    const isOpened =
      kindOf(e.props.tool) !== null &&
      isOpen(await read($, memberOf(disclosureAtom, e)), e.props.isErrored) &&
      bodyOf(e.props.tool, e.props.output, e.props.isErrored, false, '').kind !== 'stock'
    if (isOpened) {
      const { Box } = $.ui.resolve(e)
      return <Box />
    }
    if (look.prefs.quiet && (await read($, memberOf(quietAtom, e)))) {
      return quietResult(look, e.props.isErrored ? errorLine(e.props.output) : null)
    }
    if (look.svg !== undefined && EDITS.has(e.props.tool) && !e.props.isErrored) {
      const diff = hunksOf(e.props.output)
      if (diff !== null) {
        const shown = shortenPath(diff.path, await $.session.cwd())
        return withSettle($, e, diffCard({ ...look, isFirstDraw: await firstDraws($, e) }, look.svg, diff, shown, columns))
      }
    }
    const isShellCard = look.prefs.shell && SHELLS.has(e.props.tool) && !keepsOwnRow(e.props.output)
    if (look.svg !== undefined && isShellCard) {
      const shell = shellOutputOf(e.props.output)
      if (shell !== null) return withSettle($, e, terminalCard({ ...look, isFirstDraw: await firstDraws($, e) }, look.svg, shell, e.props.isErrored, columns))
    }
    if (e.surface === 'terminal' && isShellCard) {
      const shell = shellResultOf(e.props.output, e.props.isErrored)
      if (shell !== null) {
        const isWhole = !look.prefs.fold || look.folds?.open.has(OUTPUT_FOLD) === true
        const fold = isWhole ? null : { head: CLIP_HEAD, tail: CLIP_TAIL }
        return shellRows(look, await read($, memberOf(commandAtom, e)), shell, e.props.isErrored, { stdout: fold, stderr: fold })
      }
    }
    if (!look.prefs.clipOutput || !SHELLS.has(e.props.tool) || typeof output?.stdout !== 'string') return next(e)
    const stdout = clipLines(output.stdout, CLIP_HEAD, CLIP_TAIL)
    return stdout === output.stdout ? next(e) : next({ ...e, props: { ...e.props, output: { ...output, stdout } } })
  })

  // The person's message, after `render.prompt_box`. Off the terminal it keeps Claude
  // Code's own component: it carries the copy and rewind buttons, the context menu and
  // selectable text, which a drawing of the mod's would lose.
  on('ui.render', { component: 'UserMessage' }, async ($, e, next) => {
    const setup = await activeSetup($)
    if (setup === null || setup.promptBox === 'off' || e.surface !== 'terminal' || !TYPED.has(e.props.origin.kind) || e.props.text.length > MAX_PROMPT || CONTROL.test(e.props.text)) {
      return next(e)
    }
    const ui = $.ui.resolve(e)
    const hasMore = await read($, memberOf(imagesAtom, e))
    const isExpanded = shared.isTranscriptExpanded() || e.props.isExpanded
    const width = (e.viewport?.columns ?? 80) - 1
    const canGradient = setup.promptBox === 'gradient' && !hasMore && !isExpanded && !NOT_PLAIN.test(e.props.text) && width >= MIN_WIDTH
    if (canGradient && setup.ground !== null) {
      const lines = boxLines(e.props.text, width)
      const runs = gradientRuns(lines, width, setup.ground.stops, axisFor(lines.length - 2))
      return gradientBox(ui, runs, setup.ground.text)
    }
    const look = lookOf(ui, setup, e.surface)
    // Claude Code draws the images; the text is already in the outline above them.
    return promptRow(look, e.props.text, hasMore ? await next({ ...e, props: { ...e.props, text: '' } }) : undefined)
  })

  // A reply keeps Claude Code's own drawing unless it holds a card or markdown skins draws.
  on('ui.render', { component: 'AssistantMessage' }, async ($, e, next) => {
    const setup = await activeSetup($)
    if (setup === null) return next(e)
    const text = e.props.text
    const segments = replySegments(text, setup.prefs as Look['prefs'], e.surface)
    if (segments === null) return next(e)
    const ui = $.ui.resolve(e)
    const svg = e.surface !== 'terminal' && setup.prefs.tables === 'on' && 'Svg' in ui ? ui.Svg : undefined
    const base = await sessionLook($, e, lookOf(ui, setup, e.surface, copier($, e.surface)), replyNeeds(text, segments))
    const look = base.svg === undefined ? base : { ...base, isFirstDraw: await firstDraws($, e) }
    const rows = replyRows(look, segments, e.viewport?.columns ?? 100, svg, segments.length > 1 ? text : undefined)
    const framed = e.props.isFirstOfReply && isFramed(setup, e, shared.isTranscriptExpanded())
    return withSettle($, e, framed ? marginRow(look, rows) : rows)
  })

  // A slash command's output drawn the way a reply is.
  on('ui.render', { component: 'CommandOutput' }, async ($, e, next) => {
    const setup = await activeSetup($)
    if (setup === null || !setup.prefs.commands || setup.prefs.tables === 'off' || e.props.isErrored) return next(e)
    const segments = e.props.text.length > MAX_MARKDOWN ? null : commandSegments(e.props.text, e.props.command)
    if (segments === null) return next(e)
    const ui = $.ui.resolve(e)
    const svg = e.surface !== 'terminal' && setup.prefs.tables === 'on' && 'Svg' in ui ? ui.Svg : undefined
    const base = await sessionLook($, e, lookOf(ui, setup, e.surface, copier($, e.surface)), replyNeeds(e.props.text, segments))
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
