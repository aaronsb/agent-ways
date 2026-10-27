// attend's agent-facing driver in Claude Code (ADR-406).
//
// Dormant is plain Claude Code: one tool, `attend_listen`, and no polling,
// delivery or row of this module's. A session is active from session start
// when its project resolves `mods.attend.listening: active` through the ways
// config, or from the agent's attend_listen call; it stays active until
// Claude Code exits. While active the mod polls attend, delivers peer
// messages as framed turns, serves the send/reply/join/peers tools, draws one
// status row and drives keepwarm.
//
// The keepalive policy (ADR-414) runs in every interactive session with the
// mod, dormant ones included, while autocompact is off: attend judges, and
// this module supplies the operator's prompts and the autocompact gate,
// submits the keepalive and wrap turns, and routes the wrap's reply. A
// dormant session polls nothing, delivers nothing and joins nothing for it:
// one timer, set again at each main-loop turn's end. Where the policy
// applies, it drives every cache wake, ADR-182's armed windows included.
//
// Every message travels through attend's signal store by the attend CLI, as
// an argv array with no shell. The mod never uses $.session.send.

import { atom, read, update } from 'claude-code'
import type { EngineInterface, ProcessRunResult, PromptSubmitInput, PromptSubmitResult, Register, Timer } from 'claude-code'

import type { AttendStatus, KeepaliveRow, MoveRecord } from '../types/index.d.ts'
import { adoptMove, claimGaveUp, claimMove, readOutcome, recordMove, recordOf } from './move.ts'
import type { Move } from './move.ts'
import {
  ACTIVE_TOOLS,
  DEFAULT_CONFIG,
  configNotice,
  DORMANT_REFUSAL,
  OPERATOR_ORIGINS,
  listenResult,
  toolResult,
  LISTEN_TOOL,
  BEAT_ARGV,
  PLAIN_BEAT_ARGV,
  DRAIN_ARGV,
  KEEPWARM_CHECK_ARGV,
  LISTEN_OFF_ARGV,
  LISTEN_ON_ARGV,
  PEERS_ARGV,
  UNREAD_ARGV,
  WAKE_DRAIN_ARGV,
  configArgv,
  formatMessage,
  frameInbound,
  compactFocus,
  compactedArgv,
  isUnknownFlag,
  skillsFrom,
  wrapFocus,
  firstCheckMs,
  isContextReset,
  gateFrom,
  gateIsFresh,
  isWrapTurn,
  maySubmit,
  planCheck,
  refusalDetail,
  resetArgv,
  saveArgv,
  retryAtMs,
  rowAfterPrompt,
  rowAfterTurnEnd,
  rowRefused,
  rowText,
  serialize,
  wrapPlan,
  isFullPreamble,
  joinArgv,
  keepaliveApplies,
  keepaliveRowOf,
  parseDecision,
  policyArgv,
  wrappedArgv,
  keepwarmLine,
  oneLine,
  parseConfig,
  parseDrain,
  parsePeerCount,
  parseUnread,
  peerLines,
  pollInterval,
  replyArgv,
  sendArgv,
  shortId,
} from './lib.ts'
import type { Decision, Gate, Keepalive, Listed, Mod, ModConfig, WrapOutcome, WrapRoute } from './lib.ts'


type $ = EngineInterface

const DEFAULTS = {
  mode: 'dormant',
  listening: null,
  peers: 0,
  unread: 0,
  channels: [],
  deliveries: 0,
  afterCompaction: false,
  turnsSinceFull: 0,
  heldNote: null,
  cacheLapsed: false,
  keepalive: null,
  move: null,
} satisfies AttendStatus

const status = atom({ plugin: 'attend', key: 'status' } as const, DEFAULTS)

// Tools are listed as mcp__attend__<name>; the matchers below spell them out,
// so the validator can read them.
const PLUGIN = 'attend'


// A drain that finds messages waits this long and drains again, so a burst
// arrives as one turn.
const BATCH_MS = 2_000
// The peer count and the keepwarm check run at most this often.
const PEERS_EVERY_MS = 30_000
const KEEPWARM_EVERY_MS = 60_000
// The heartbeat is touched this often, a third of attend's 90 s liveness
// grace (ADR-129), during a turn and at the slow interval alike.
const BEAT_EVERY_MS = 30_000
// Polls that retry a batch whose submit failed before it is let go.
const SUBMIT_TRIES = 3
// The $.store key naming the sessions that listened where the project's
// config would not start them listening, newest last, so a resume of one
// comes back listening (ADR-411).
const LISTENED = 'listened'
const LISTENED_KEPT = 50
// A /clear or a /resume tells attend the id that goes on after this long.
const RELISTEN_MS = 1_000
// The keepalive policy: a gate read as on or unknown is read again after
// this long, one read as off before every check, since a turn may follow
// it; the check reads the transcript and the settings through ways; the
// first check waits until session start has set the session listening or
// dormant; a check that could not run asks again.
const POLICY_TIMEOUT_MS = 30_000
const POLICY_START_MS = 1_000
// A submitted wrap whose turn has not started after this long, or has not
// answered by the time the cache lapses, is reported unfinished.
const WRAP_START_MS = 10 * 60_000
const WRAP_LAPSE_MS = 60 * 60_000

// agent-ways theme (tools/agent-theme/themes/agent-ways.theme).
const SUBTLE = '#3a3a3c'
const FG = '#d0d0d0'

async function readStatus($: $): Promise<AttendStatus> {
  return { ...DEFAULTS, ...(await read($, status)) }
}

async function updateStatus($: $, f: (s: AttendStatus) => AttendStatus) {
  await update($, status, s => f({ ...DEFAULTS, ...s }))
}

/** One CLI call; a command that cannot start reads as exit 127. */
async function run($: $, argv: readonly string[], timeoutMs = 15_000, stdin?: string): Promise<ProcessRunResult> {
  try {
    return await $.process.run(argv, stdin === undefined ? { timeoutMs } : { timeoutMs, stdin })
  } catch (err) {
    return { exitCode: 127, stdout: '', stderr: String(err), isStdoutTruncated: false, isStderrTruncated: false }
  }
}

/** Whether this process has said that ways could not answer for its
 *  settings: said once, not at every read. */
let configNoticeShown = false

async function readConfig($: $, cwd: string): Promise<ModConfig> {
  const r = await run($, configArgv(cwd), 10_000)
  if (r.exitCode === 0) return parseConfig(r.stdout)
  const notice = configNotice(r.exitCode)
  if (notice && !configNoticeShown) {
    configNoticeShown = true
    await $.ui.toast(notice)
  }
  return { ...DEFAULT_CONFIG }
}

async function registerListen($: $) {
  await $.tool.register(LISTEN_TOOL)
}

/** The tools of an active session. Registering one again with the same
 *  spec changes nothing, so every path that finds the session active
 *  registers them, a reloaded module's session.start included. */
async function registerActiveTools($: $) {
  for (const tool of ACTIVE_TOOLS) await $.tool.register(tool)
}

/** Tells attend this session is listening, with the mod as its host; the
 *  next drain applies the cold-start rule: what was addressed to the project
 *  since this Claude Code process started arrives, and what `#open` and the
 *  channels carry from now on. */
async function listenOn($: $): Promise<ProcessRunResult> {
  const r = await run($, LISTEN_ON_ARGV)
  await updateStatus($, s => ({ ...s, listening: r.exitCode === 0 }))
  return r
}

/** The sessions that listened, newest last, as $.store holds them. */
async function readListened($: $): Promise<string[]> {
  const v = await $.store.get(LISTENED)
  return Array.isArray(v) ? v.filter((x): x is string => typeof x === 'string') : []
}

/** Whether the project's config starts this session listening on its own,
 *  so a resume of it needs no record. */
function startsActive(mod: Mod): boolean {
  return mod.config.listening === 'active' && mod.canPoll
}

/** Records that session `id` listened, keeping the newest LISTENED_KEPT,
 *  where the config would not reproduce it and the record does not list it
 *  already. $.store has no versioned write, so two processes recording at
 *  once can lose one id; writing this rarely keeps that window small. The
 *  record serves a later resume and never this session: a store that fails
 *  is logged and the session goes on. */
async function rememberListening($: $, mod: Mod, id: string) {
  if (startsActive(mod)) return
  try {
    const ids = await readListened($)
    if (ids.includes(id)) return
    ids.push(id)
    await $.store.set(LISTENED, ids.slice(-LISTENED_KEPT))
  } catch (err) {
    $.ui.log(`attend: could not record this session for a resume: ${String(err)}`)
  }
}

/** Makes session `id` active for the rest of the process: listening, the
 *  poll, and the record a resume reads; the caller has registered the
 *  tools. Idempotent. Returns attend's answer to `listen on`. */
async function activate($: $, mod: Mod, id: string): Promise<ProcessRunResult> {
  mod.active = true
  let r: ProcessRunResult
  try {
    mod.config = await readConfig($, mod.cwd)
    mod.canPoll = mod.canPoll || mod.config.headlessPoll
    // The config read again may change whether the policy applies: one judge
    // per ledger, the policy's timer or the poll's plain check.
    const didApply = mod.ka.applies
    mod.ka.applies = keepaliveApplies(mod.config, mod.isInteractive)
    if (mod.ka.applies && !didApply) await armKeepalive($, mod, (await $.clock.now()) + POLICY_START_MS)
    if (!mod.ka.applies) mod.ka.timer?.cancel()
    mod.lastActivityMs = await $.clock.now()
    await updateStatus($, s => ({ ...s, mode: 'active' as const }))
    r = await listenOn($)
  } finally {
    mod.isRelistening = false
  }
  await rememberListening($, mod, id)
  if (mod.canPoll) {
    startTicker($, mod)
    $.clock.after(0, () => void tick($, mod, true))
  }
  return r
}

// ── following a /clear or an in-session /resume (move.ts) ──────────────
//
// The watch runs here, in the hooks module's own file, since the engine
// follows `$` only into functions declared in it; move.ts holds the state
// and its rules. watchMove runs on session.start's `$` (mod.startWatch), as
// session.end's bound aborts the waits of its own.

/** Starts the watch of the module's move: the hold at once, so a reload
 *  finds the move, then the reads on the backoff. One timer at a time. */
function watchMove($: $, mod: Mod) {
  const m = mod.move
  if (!m) return
  m.timer?.cancel()
  m.timer = $.clock.after(0, () => void holdFirst($, mod, m))
}

async function holdFirst($: $, mod: Mod, m: Move) {
  if (mod.move !== m) return
  try {
    await holdMove($, mod, recordOf(m))
  } catch {
    // Each read writes the hold again.
  }
  if (mod.move !== m) return
  m.timer = $.clock.after(m.delayMs, () => void stepMove($, mod, m))
}

/** One read of the id: settle on an id that did not end, otherwise wait
 *  longer (readOutcome). A read that fails counts as no change and is
 *  logged once; past the give-up the session stays as the hold left it. */
async function stepMove($: $, mod: Mod, m: Move) {
  if (mod.move !== m) return
  m.timer = undefined
  let id: string | undefined
  try {
    await holdMove($, mod, recordOf(m))
    id = await $.session.id()
  } catch (err) {
    m.failures += 1
    if (m.failures === 1) $.ui.log(`attend: could not read the session id after a /clear or a /resume: ${String(err)}`)
  }
  const next = readOutcome(mod, m, id)
  if (next.kind === 'settle' && id !== undefined) return void (await finishMove($, mod, id, next.record))
  if (next.kind === 'give-up') {
    const last = next.record.ended[next.record.ended.length - 1] ?? ''
    $.ui.log(`attend: the session id still reads ${last} ${Math.round(m.waitedMs / 1000)} s after a /${next.record.reason}; the next turn under another id records it`)
    try {
      await releaseMove($)
    } catch {
      // The record in $.state goes with the next settle, or the process.
    }
    return
  }
  if (next.kind === 'wait') m.timer = $.clock.after(next.delayMs, () => void stepMove($, mod, m))
}

/** A main-loop turn: settles the move, or one the watch gave up on, when
 *  `$.session.id()` names an id that did not end; otherwise the watch goes
 *  on. A turn under the new id so does not wait for the next read. */
async function settleIfMoved($: $, mod: Mod) {
  try {
    const id = await $.session.id()
    const m = mod.move
    if (m) {
      const record = claimMove(mod, m, id)
      if (record) await finishMove($, mod, id, record)
      return
    }
    const given = claimGaveUp(mod, id)
    if (given) await settleMove($, mod, id, given)
  } catch (err) {
    $.ui.log(`attend: could not read the session id after a /clear or a /resume: ${String(err)}`)
  }
}

async function finishMove($: $, mod: Mod, id: string, record: MoveRecord) {
  try {
    await settleMove($, mod, id, record)
  } finally {
    await releaseMove($)
  }
}

async function releaseMove($: $) {
  await updateStatus($, s => ({ ...s, move: null }))
}

/** Before each read of the id: the move stays in $.state, which Claude Code
 *  empties after session.end, so a reload takes the watch up; a session
 *  that was listening keeps its mode there, so a reload finds it active, and
 *  attend is told once that the session listens, whatever id it sees now.
 *  The next delivery carries the preamble: the model's context changed as
 *  at a compaction. */
async function holdMove($: $, mod: Mod, m: MoveRecord) {
  const s = await readStatus($)
  const isKept = s.move !== null && s.move.ended.join(' ') === m.ended.join(' ')
  if (isKept && (!m.wasOn || isActive(s))) return
  await updateStatus($, t => ({
    ...t,
    move: m,
    ...(m.wasOn ? { mode: 'active' as const, afterCompaction: true, turnsSinceFull: 0, heldNote: mod.heldNote ?? null } : {}),
  }))
  if (m.wasOn && !mod.isMoveListened) {
    mod.isMoveListened = true
    await listenOn($)
  }
}

/** The session goes on under `id`, which is none of the ids that ended. An
 *  active session, or a /resume to a session that listened, records the id
 *  and tells attend the id is listening; the poll holds its drain until
 *  then. */
async function settleMove($: $, mod: Mod, id: string, m: MoveRecord) {
  try {
    const goesOn = m.wasOn || mod.active || (m.reason === 'resume' && (await readListened($)).includes(id))
    if (!goesOn) return
    mod.isRelistening = true
    await updateStatus($, s => ({ ...s, mode: 'active' as const, afterCompaction: true, turnsSinceFull: 0, heldNote: mod.heldNote ?? null }))
    await rememberListening($, mod, id)
    if (mod.active) {
      $.clock.after(RELISTEN_MS, () => void listenOn($).finally(() => (mod.isRelistening = false)))
    } else {
      mod.active = true
      await registerActiveTools($)
      $.clock.after(RELISTEN_MS, () => void activate($, mod, id))
    }
  } catch (err) {
    // No timer will lower the guard, so the poll would stay held.
    mod.isRelistening = false
    $.ui.log(`attend: could not carry listening to the session that goes on: ${String(err)}`)
  }
}

/** Whether the context fill reads as absent, as it does after a compaction
 *  until the next response; a /compact that failed or was cancelled leaves
 *  it reported. Where usage cannot be read, it counts as cleared: an extra
 *  preamble costs one line. */
async function isFillCleared($: $): Promise<boolean> {
  try {
    return (await $.session.usage()).context.tokens === undefined
  } catch {
    return true
  }
}

/** The next delivery shows the preamble in full. */
async function markCompacted($: $) {
  await updateStatus($, s => ({ ...s, afterCompaction: true }))
}

function startTicker($: $, mod: Mod) {
  mod.ticker?.cancel()
  const every = Math.max(1, Math.min(mod.config.pollSeconds, 5)) * 1000
  mod.ticker = $.clock.every(every, () => void tick($, mod, false))
}

/** One beat of the poll: touch the heartbeat, drain while idle, refresh the
 *  row, run the keepwarm check. `force` skips the poll's interval
 *  (activation, a turn's end). The heartbeat keeps its own interval, so a
 *  turn or a slow poll longer than attend's grace does not leave the session
 *  looking gone. */
async function tick($: $, mod: Mod, force: boolean) {
  if (mod.isTicking) return
  if (!(await isOn($, mod))) return
  mod.isTicking = true
  try {
    const now = await $.clock.now()
    if (now - mod.lastBeatMs >= BEAT_EVERY_MS) {
      mod.lastBeatMs = now
      if (mod.isBeatPlain) {
        await run($, PLAIN_BEAT_ARGV)
      } else if ((await run($, BEAT_ARGV)).exitCode === 2) {
        // An older attend: a usage error on --mod. Beat without it from now on.
        mod.isBeatPlain = true
        await run($, PLAIN_BEAT_ARGV)
      }
    }
    const isTurnRunning = mod.running.size > 0
    const interval = pollInterval(mod.config, now - mod.lastActivityMs, isTurnRunning)
    if (!force && now - mod.lastPollMs < interval) return
    mod.lastPollMs = now
    if (!isTurnRunning && !mod.isRelistening) await poll($, mod)
    await refreshUnread($, mod)
    if (force || now - mod.lastPeersMs >= PEERS_EVERY_MS) await refreshPeers($, mod)
    // Where the keepalive policy applies, its own timer drives every wake.
    const isPlain = !mod.ka.applies
    if (isPlain && mod.config.keepwarm === 'on' && mod.running.size === 0 && now - mod.lastKeepwarmMs >= KEEPWARM_EVERY_MS) {
      mod.lastKeepwarmMs = now
      await keepwarm($)
    }
  } finally {
    mod.isTicking = false
  }
}

/** One wake drain; on messages, a plain drain after BATCH_MS, then one
 *  prompt for the batch. The wake drain delivers only once something pending
 *  would wake the session (a message addressed to it, or a post in one of
 *  its named channels), and then delivers all that is pending. A
 *  turn is submitted only when the drain delivered at least one message: the
 *  cold start's held-back note and the session's name are fields of the
 *  drain's document, never a message. The note is kept for the operator's
 *  next prompt.
 *
 *  A session its config started listening polls from its start like any
 *  other. attend's cold start holds back what was sent before the session
 *  began (ADR-404 D3b), so what wakes a new session is mail sent to it
 *  since, and a session with no turn yet has no cache to lapse.
 *
 *  The drain marks what it delivers read before the prompt is submitted, so
 *  the batch waits in `mod.pending` until a submit takes it. A submit that
 *  fails, or that a hook drops, leaves the batch there and the next poll
 *  tries it again before draining more, up to SUBMIT_TRIES times. */
async function poll($: $, mod: Mod) {
  if (mod.isPolling || mod.isPendingCold) return
  mod.isPolling = true
  try {
    if (mod.pending.length === 0) {
      // Only what is addressed to the session, or posted in one of its
      // named channels, starts a turn; other #open posts wait for its next
      // one (ADR-406 item 2).
      const first = parseDrain((await run($, WAKE_DRAIN_ARGV)).stdout)
      if (!first) return
      // Listening turned off elsewhere (attend listen off, a private scene),
      // or an identity attend cannot resolve: the row says so.
      const listening = first.status === 'drained'
      const was = await readStatus($)
      if (was.listening !== listening || was.cacheLapsed !== first.cacheLapsed) {
        await updateStatus($, s => ({ ...s, listening, cacheLapsed: first.cacheLapsed }))
      }
      if (first.note) await setHeldNote($, mod, first.note)
      if (first.messages.length === 0) return
      const msgs = [...first.messages]
      await updateStatus($, s => ({ ...s, unread: Math.max(s.unread, msgs.length) }))
      await new Promise<void>(resolve => $.clock.after(BATCH_MS, resolve))
      const second = parseDrain((await run($, DRAIN_ARGV)).stdout)
      if (second) msgs.push(...second.messages)
      mod.pending = msgs
      mod.submitTries = 0
    }
    if (await deliver($, mod, mod.pending)) {
      mod.pending = []
    } else if (++mod.submitTries >= SUBMIT_TRIES) {
      // Read by attend and refused by the session: they stay readable with
      // attend message inbox, and the poll goes on to new messages.
      await $.ui.toast(`attend: ${mod.pending.length} message(s) could not be delivered; attend message inbox lists them`)
      mod.pending = []
    }
  } finally {
    mod.isPolling = false
  }
}

/** Submits one framed batch, which starts a turn in an idle session.
 *  Resolves whether the prompt entered; the preamble counters move only
 *  when it did. */
async function deliver($: $, mod: Mod, msgs: readonly Listed[]): Promise<boolean> {
  // A check in flight submits nothing behind this turn.
  mod.ka.epoch += 1
  mod.lastActivityMs = await $.clock.now()
  for (const m of msgs) {
    mod.ids.set(shortId(m.id), m.id)
    if (m.re) mod.ids.set(shortId(m.re), m.re)
  }
  const s = await readStatus($)
  const full = isFullPreamble(s.deliveries, s.afterCompaction, s.turnsSinceFull, mod.config.preambleEveryTurns)
  const entered = await $.prompt
    .submit({ text: frameInbound(msgs.map(formatMessage), full), asUser: true })
    .then(r => r.drop === undefined)
    .catch(() => false)
  if (!entered) return false
  await updateStatus($, t => ({
    ...t,
    deliveries: t.deliveries + 1,
    afterCompaction: false,
    turnsSinceFull: full ? 0 : t.turnsSinceFull,
  }))
  return true
}

/** Keeps the held-back note, or clears it, in the module and in $.state. */
async function setHeldNote($: $, mod: Mod, note: string | undefined) {
  mod.heldNote = note
  await updateStatus($, s => ({ ...s, heldNote: note ?? null }))
}

/** The operator's prompt while a message waits on a lapsed cache, or while
 *  a held-back note waits: one drain, and what it delivered rides `e` as
 *  context, the note with it, rather than as a turn of its own. The
 *  operator sees what went, in the transcript under their prompt
 *  ({@link showWithPrompt}). A prompt that does not enter leaves the
 *  messages in `mod.pending`, and the note in `mod.heldNote`: taken on a
 *  lapsed cache they wait for the operator's next prompt, otherwise the poll
 *  submits the messages as it does a batch whose submit failed. */
async function enterWithPending($: $, mod: Mod, e: PromptSubmitInput, next: (e: PromptSubmitInput) => Promise<PromptSubmitResult>, isCold: boolean) {
  // A batch an earlier cold prompt failed to carry goes first.
  const carried = mod.isPendingCold ? mod.pending : []
  const d = parseDrain((await run($, DRAIN_ARGV)).stdout)
  if (!d && carried.length === 0) return next(e)
  if (d) {
    const listening = d.status === 'drained'
    if ((await readStatus($)).listening !== listening) await updateStatus($, s => ({ ...s, listening }))
  }
  const msgs = [...carried, ...(d?.messages ?? [])]
  if (carried.length > 0) {
    mod.pending = []
    mod.isPendingCold = false
  }
  for (const m of msgs) {
    mod.ids.set(shortId(m.id), m.id)
    if (m.re) mod.ids.set(shortId(m.re), m.re)
  }
  const heldNote = d?.note ?? mod.heldNote
  await setHeldNote($, mod, undefined)
  const note = heldNote ? `attend: ${oneLine(heldNote)}` : undefined
  const lines = msgs.map(formatMessage)
  const block = msgs.length > 0 ? frameInbound(lines, true) : undefined
  const attached = [block, note].filter((x): x is string => x !== undefined)
  if (attached.length === 0) return next(e)
  // Read by attend and not entered: they wait in `mod.pending`. Taken on a
  // lapsed cache, they wait for the operator's next prompt, with the row
  // still saying so; otherwise the poll submits them.
  const keep = async () => {
    await setHeldNote($, mod, heldNote)
    if (msgs.length === 0) return
    mod.pending = [...msgs]
    mod.submitTries = 0
    mod.isPendingCold = isCold
    if (isCold) await updateStatus($, s => ({ ...s, cacheLapsed: true, unread: Math.max(s.unread, msgs.length) }))
  }
  let r: PromptSubmitResult
  try {
    r = await next({ ...e, context: [...(e.context ?? []), attached.join('\n')] })
  } catch (err) {
    await keep()
    throw err
  }
  if (r.drop !== undefined) {
    await keep()
    return r
  }
  if (msgs.length > 0) await updateStatus($, t => ({ ...t, deliveries: t.deliveries + 1, afterCompaction: false, turnsSinceFull: 0 }))
  showWithPrompt($, lines, note)
  return r
}

/** What rode the operator's prompt as context, which the engine never draws:
 *  a line saying so, then one dim transcript line per message as a
 *  delivery's row draws them, and the held-back note. The model reads none
 *  of it. */
function showWithPrompt($: $, lines: readonly string[], note: string | undefined) {
  if (lines.length > 0) {
    $.ui.log(`attend: ${lines.length} peer message(s) went to Claude with your prompt`)
    for (const l of lines) $.ui.log(oneLine(l))
  }
  if (note) $.ui.log(note)
}


async function refreshUnread($: $, mod: Mod) {
  const u = parseUnread((await run($, UNREAD_ARGV)).stdout)
  if (!u) return
  await updateStatus($, s => ({ ...s, unread: u.unread, channels: [...new Set([...u.channels, ...mod.joined])] }))
}

async function refreshPeers($: $, mod: Mod) {
  mod.lastPeersMs = await $.clock.now()
  const r = await run($, PEERS_ARGV)
  if (r.exitCode === 0) await updateStatus($, s => ({ ...s, peers: parsePeerCount(r.stdout) }))
}

/** ADR-182's judgment, run by attend; a `keepwarm:` line becomes a turn. */
async function keepwarm($: $) {
  const line = keepwarmLine((await run($, KEEPWARM_CHECK_ARGV, 30_000)).stdout)
  if (line) await $.prompt.submit({ text: line })
}

/** Whether this session is active. The module's own flag answers too, and
 *  puts the mode back in $.state should it be missing there. */
async function isOn($: $, mod: Mod): Promise<boolean> {
  if (isActive(await readStatus($))) return true
  if (!mod.active) return false
  await updateStatus($, s => ({ ...s, mode: 'active' as const }))
  return true
}

function isActive(s: AttendStatus): boolean {
  return s.mode === 'active'
}

function input<T>(e: unknown): Partial<T> {
  return e as Partial<T>
}

// ── the keepalive policy (ADR-414) ──────────────────────────────────────

async function setKeepaliveRow($: $, f: (k: KeepaliveRow | null) => KeepaliveRow | null) {
  await updateStatus($, s => ({ ...s, keepalive: f(s.keepalive) }))
}

/** The autocompact gate (ADR-414 item 2): the usage breakdown, estimated
 *  locally with no request, else `autoCompactEnabled` in the merged
 *  settings, else unknown, which counts as on. `fresh` reads it whatever
 *  was kept, as before a submit. A gate that first reads off sends the
 *  operator's prompt held while it did not. The same breakdown lists the
 *  session's skills, which attend checks `wrap_skill` against. */
async function readGate($: $, ka: Keepalive, fresh = false): Promise<Gate> {
  const now = await $.clock.now()
  if (!fresh && gateIsFresh(ka.gate, ka.gateAtMs, now)) return ka.gate
  let usage: unknown
  try {
    usage = await $.session.usage({ breakdown: 'summary' })
  } catch {
    usage = undefined
  }
  ka.skills = skillsFrom(usage)
  let settings: unknown
  if (gateFrom(usage, undefined) === 'unknown') {
    try {
      settings = await $.settings.read()
    } catch {
      settings = undefined
    }
  }
  ka.gate = gateFrom(usage, settings)
  ka.gateAtMs = now
  if (ka.gate === 'off' && ka.heldPromptMs !== undefined) {
    const at = ka.heldPromptMs
    ka.heldPromptMs = undefined
    await run($, resetArgv(at / 1000))
  }
  return ka.gate
}

/** The policy's timer: a check at `atMs`. */
async function armKeepalive($: $, mod: Mod, atMs: number) {
  mod.ka.timer?.cancel()
  const wait = Math.max(0, atMs - (await $.clock.now()))
  mod.ka.timer = $.clock.after(wait, () => {
    mod.ka.timer = undefined
    void checkKeepalive($, mod)
  }) as unknown as Timer
}

/** One check: read the gate, ask attend, submit what it returns, and set the
 *  timer for when attend says to ask again. Nothing is due while a main-loop
 *  turn runs, and a prompt or a turn that starts while attend judges (the
 *  epoch moves) cancels the submit. A dormant session with autocompact on
 *  stays plain Claude Code and asks nothing. */
async function checkKeepalive($: $, mod: Mod) {
  const ka = mod.ka
  if (!ka.applies || mod.running.size > 0 || mod.config.keepwarm !== 'on') return
  const epoch = ka.epoch
  await serialize(ka, async () => {
    const gate = await readGate($, ka)
    const isListening = await isOn($, mod)
    if (!isListening && gate !== 'off') return
    let r = await run($, policyArgv(gate, ka.skills, ka.isLegacyAttend), POLICY_TIMEOUT_MS)
    // An attend from before --skills: ask as before, and keep asking so.
    if (!ka.isLegacyAttend && isUnknownFlag(r)) {
      ka.isLegacyAttend = true
      r = await run($, policyArgv(gate, null, true), POLICY_TIMEOUT_MS)
    }
    const d: Decision | undefined = r.exitCode === 0 ? parseDecision(r.stdout) : undefined
    const plan = d ? planCheck(d, isListening) : undefined
    if (!d || !plan || plan.retry) {
      const at = retryAtMs(await $.clock.now(), isListening, gate, ka.retries)
      ka.retries += 1
      if (at !== null) await armKeepalive($, mod, at)
      return
    }
    ka.retries = 0
    await setKeepaliveRow($, k => keepaliveRowOf(d, k))
    if (plan.submit) {
      const isPolicy = d.policy
      const gateNow = isPolicy ? await readGate($, ka, true) : gate
      if (!maySubmit({ epochThen: epoch, epochNow: ka.epoch, running: mod.running.size, isPolicy, gate: gateNow })) {
        if (plan.submit.kind === 'wrap') await run($, wrappedArgv('unfinished', 'a turn started before the wrap went in'))
        if (plan.submit.kind === 'compact') await run($, compactedArgv('a turn started before the compaction'))
        return
      }
      if (plan.submit.kind === 'compact') {
        // Between turns, and nothing is due after it: attend recorded it.
        await compactAfterWrap($, plan.submit.text)
        return
      }
      const { text, route } = plan.submit
      if (route) {
        const wrap = { route, text, turnId: undefined, reply: undefined, sawTurn: false }
        ka.wrap = wrap
        // A wrap whose turn never starts, or never answers before the cache
        // lapses, is reported rather than left pending.
        $.clock.after(WRAP_START_MS, () => {
          if (ka.wrap === wrap && wrap.turnId === undefined) {
            void reportUnrouted($, ka, wrap.sawTurn ? 'the wrap\'s turn was not recognised' : 'the wrap\'s turn never started')
          }
        })
        $.clock.after(WRAP_LAPSE_MS, () => {
          if (ka.wrap === wrap) void reportUnrouted($, ka, 'the wrap did not answer before the cache lapsed')
        })
      }
      const isEntered = await $.prompt
        .submit({ text })
        .then(x => x.drop === undefined)
        .catch(() => false)
      if (!isEntered && route) {
        ka.wrap = undefined
        await run($, wrappedArgv('unfinished', 'the wrap prompt did not enter'))
      }
    }
    if (plan.armAtMs !== null) await armKeepalive($, mod, plan.armAtMs)
  })
}

/** The operator is present: the count starts again, and a sent wrap and a
 *  stop are cleared (ADR-414 item 4). attend hears of it only while the
 *  gate last read off; otherwise the prompt is held, and sent when the gate
 *  first reads off, so a session with autocompact on runs nothing. */
async function keepaliveOperator($: $, mod: Mod) {
  const ka = mod.ka
  if (!ka.applies) return
  ka.epoch += 1
  // Never awaited: the prompt must not wait on the chain a check holds.
  if (ka.wrap) void reportUnrouted($, ka, 'the operator prompted before the wrap answered')
  const now = await $.clock.now()
  if ((await readGate($, ka, true)) === 'off') {
    ka.heldPromptMs = undefined
    void serialize(ka, () => run($, resetArgv(now / 1000)))
  } else {
    ka.heldPromptMs = now
  }
  await setKeepaliveRow($, rowAfterPrompt)
}

/** A main-loop turn ended, which restarts the clock: the timer is set for
 *  the first point a check could be due, and attend names the due point
 *  there. The wrap's turn routes its reply first. */
async function keepaliveTurnEnded($: $, mod: Mod, e: { turnId: string; reason: string; answer?: string }) {
  if (!mod.ka.applies) return
  await routeWrap($, mod.ka, e.turnId, e.reason, e.answer ?? '')
  if (mod.running.size > 0) return
  mod.ka.retries = 0
  const at = (await $.clock.now()) + firstCheckMs(mod.config)
  await armKeepalive($, mod, at)
  await setKeepaliveRow($, k => rowAfterTurnEnd(k, at))
}

/** The wrap's turn ended: its final reply goes where `wrap` says, and attend
 *  records where it went (ADR-414 item 6). Only the turn the wrap's own
 *  text started is routed; a turn that ended other than with an answer is
 *  written nowhere. */
async function routeWrap($: $, ka: Keepalive, turnId: string, reason: string, answer: string) {
  const w = ka.wrap
  if (!w || w.turnId === undefined || w.turnId !== turnId) return
  ka.wrap = undefined
  // Before the first await: a prompt during the reports or the send below
  // cancels the compaction scheduled after them.
  const epoch = ka.epoch
  // The wrap's own answer, from its first ending step, which stands even
  // when a Stop-hook continuation then ends aborted or in error; the
  // turn's final answer only where no step was seen.
  const reply = w.reply ?? answer
  const plan = wrapPlan(w.route, w.reply !== undefined ? 'answer' : reason, reply)
  // An unfinished wrap wrote no handoff, so it records no focus and
  // nothing compacts after it.
  if (plan.kind === 'unfinished') return void (await serialize(ka, () => run($, wrappedArgv('unfinished', plan.detail))))
  const focus = wrapFocus(w.route, reply)
  const report = (o: WrapOutcome, detail = '') => serialize(ka, () => run($, wrappedArgv(o, detail, focus)))
  if (plan.kind === 'transcript') await report('transcript')
  // attend writes the file and records the outcome, or why it could not.
  if (plan.kind === 'file') await serialize(ka, () => run($, plan.argv, undefined, reply))
  if (plan.kind === 'send') {
    const r = await run($, plan.argv)
    if (r.exitCode === 0) {
      await report('sent', w.route.to)
    } else {
      // Refused (no live session by that name, a channel it cannot post
      // to) or timed out: the reply goes to the file instead.
      const detail = refusalDetail(w.route.to, r)
      if (w.route.file === '') await report('refused', `${detail}; no safe wrap file to keep it in`)
      else await serialize(ka, () => run($, saveArgv('refused', detail, w.route.file, focus), undefined, reply))
      await setKeepaliveRow($, rowRefused)
    }
  }
  // With keepalives after the wrap, attend asks for the compaction at a
  // due point instead, with the focus reported above.
  if (w.route.after === 'compact' && w.route.afterKeepalives === 0) {
    const instructions = compactFocus(reply)
    // Between turns: a compaction rejects while a turn runs. A prompt or a
    // turn since the wrap answered (the epoch moved) cancels it.
    $.clock.after(1_000, () => {
      if (ka.epoch === epoch) void serialize(ka, () => compactAfterWrap($, instructions))
    })
  }
}

/** The compaction after the wrap, with the wrap's focus; attend records
 *  that it ran, or why it did not. The caller holds the keepalive chain. */
async function compactAfterWrap($: $, instructions: string) {
  let failed: string | undefined
  try {
    const r = await $.session.compact({ instructions })
    if (r.skip === undefined) await markCompacted($)
    else failed = `skipped: ${String(r.skip)}`
  } catch (err) {
    failed = String(err)
    $.ui.log(`attend: the compaction after the wrap failed: ${failed}`)
  }
  // The row took attend's word before the compaction ran.
  if (failed !== undefined) await setKeepaliveRow($, k => (k?.state === 'compacted' ? { ...k, state: 'wrapped', compactedAt: null } : k))
  await run($, compactedArgv(failed))
}

/** A wrap that went in and was never routed: its turn never started or
 *  was not recognised, or never ended with an answer before the cache
 *  lapsed, or the operator prompted first. attend records it as unfinished.
 *  The wrap is cleared before the first await, so a caller may fire it. */
async function reportUnrouted($: $, ka: Keepalive, why: string) {
  ka.wrap = undefined
  await serialize(ka, () => run($, wrappedArgv('unfinished', why)))
}

export const register: Register = on => {
  const mod: Mod = {
    config: { ...DEFAULT_CONFIG },
    canPoll: false,
    isInteractive: false,
    cwd: '.',
    running: new Set(),
    isPolling: false,
    isTicking: false,
    ticker: undefined,
    lastPollMs: 0,
    lastPeersMs: 0,
    lastKeepwarmMs: 0,
    lastBeatMs: 0,
    isBeatPlain: false,
    lastActivityMs: 0,
    ids: new Map(),
    active: false,
    pending: [],
    isPendingCold: false,
    submitTries: 0,
    joined: new Set(),
    isStarted: false,
    isRelistening: false,
    move: undefined,
    gaveUp: undefined,
    isMoveListened: false,
    startWatch: undefined,
    contextTokens: undefined,
    heldNote: undefined,
    ka: {
      applies: false,
      gate: 'unknown',
      gateAtMs: -Infinity,
      skills: null,
      isLegacyAttend: false,
      timer: undefined,
      chain: Promise.resolve(),
      epoch: 0,
      heldPromptMs: undefined,
      retries: 0,
      wrap: undefined,
    },
  }

  on('session.start', async ($, e, next) => {
    mod.cwd = e.cwd
    mod.config = await readConfig($, e.cwd)
    // mods.attend.install false: the mod is unavailable, nothing is listed.
    if (!mod.config.install) return next(e)
    mod.canPoll = e.isInteractive || mod.config.headlessPoll
    mod.isInteractive = e.isInteractive
    mod.ka.applies = keepaliveApplies(mod.config, e.isInteractive)
    // The first check reads where the transcript stands (a resumed session
    // may be warm) and records the gate for the status card.
    if (mod.ka.applies) await armKeepalive($, mod, (await $.clock.now()) + POLICY_START_MS)
    await registerListen($)
    mod.isStarted = true
    // The move's watch runs on this hook's `$`: session.end's bound aborts
    // the waits of its own.
    mod.startWatch = () => watchMove($, mod)
    // A reload finds the mode in $.state; a new process (`claude --resume`)
    // finds the session id in the record of sessions that listened, or the
    // project's config starts it, as it does every session there.
    const id = await $.session.id()
    const kept = await readStatus($)
    // A reload in the middle of a /clear or a /resume takes the watch up.
    if (kept.move) {
      adoptMove(mod, kept.move)
      watchMove($, mod)
    }
    const wasActive = isActive(kept) || (await readListened($)).includes(id)
    if (wasActive || startsActive(mod)) {
      // Listed by turn one; listening and the first drain follow the hook.
      // Active from here, so a /clear before activate runs keeps it. A
      // reload keeps a held-back note the cold start will not report again.
      mod.active = true
      mod.heldNote = kept.heldNote ?? undefined
      await registerActiveTools($)
      $.clock.after(0, () => void activate($, mod, id))
    } else {
      // Dormant: attend's drain delivers nothing to this session either.
      $.clock.after(0, () => void run($, LISTEN_OFF_ARGV))
    }
    return next(e)
  })

  // A /clear or an in-session /resume ends the conversation and the process
  // goes on under another session id, with no session.start, and Claude
  // Code empties this plugin's $.state. session.end with reason `clear` or
  // `resume` is the engine's word for it. The id that goes on shows later,
  // so this hook records the move as plain data and makes no call on its own
  // `$`, whose waits session.end's short bound aborts: mod.startWatch()
  // schedules the watch (watchMove) on session.start's `$`. The mod
  // hooks no classic.* event: where an organization manages Claude Code, its
  // policy plugin sends those past the plugins a person installs. An exit, a
  // logout or a -p run's end changes nothing here; at process start
  // session.start alone reads the record.
  on('session.end', ($, e, next) => {
    if (mod.isStarted && mod.startWatch && (e.reason === 'clear' || e.reason === 'resume')) {
      mod.isMoveListened = false
      recordMove(mod, e.sessionId, e.reason, mod.active)
      mod.startWatch()
    }
    return next(e)
  })

  // A compaction drops the earlier preamble from the model's view, so the
  // next delivery shows it in full. session.compact passes over the plugins
  // a person installs where an organization manages Claude Code, so the mod
  // reads a compaction from what reaches it there: the person's /compact
  // once it has run and left the fill unreported, its own compaction after a
  // wrap (markCompacted), and the context fill falling below half between
  // two measurements, as an automatic compaction usually leaves it.
  on('command.run', { command: 'compact' }, async ($, e, next) => {
    const done = await next(e)
    if (await isFillCleared($)) await markCompacted($)
    return done
  })

  on('session.measure', async ($, e, next) => {
    const before = mod.contextTokens
    mod.contextTokens = e.context.tokens
    if (isContextReset(before, e.context.tokens)) await markCompacted($)
    return next(e)
  })

  // The operator's next prompt while a message waits on a lapsed cache, or
  // while the cold start's held-back note waits: what is pending enters
  // beside it, read in the same turn, rather than as a turn the operator
  // never asked for.
  on('prompt.submit', async ($, e, next) => {
    // Any prompt (a schedule, a peer, a task notice) moves the epoch, so a
    // check in flight queues nothing behind its turn; only the operator's
    // starts the count again.
    mod.ka.epoch += 1
    if (!OPERATOR_ORIGINS.has(e.origin.kind)) return next(e)
    await keepaliveOperator($, mod)
    const isHeldCold = (await readStatus($)).cacheLapsed || mod.isPendingCold
    if (mod.heldNote === undefined && !isHeldCold) return next(e)
    if (isHeldCold) await updateStatus($, s => ({ ...s, cacheLapsed: false }))
    if (!(await isOn($, mod))) return next(e)
    return enterWithPending($, mod, e, next, isHeldCold)
  })

  // The main loop's turns only: a subagent's or a background agent's run
  // is a turn of its own loop and can outlast the operator's attention by
  // half an hour, while the main loop idles and a message should wake it.
  on('turn.start', async ($, e, next) => {
    if (e.agentId !== undefined) return next(e)
    mod.running.add(e.turnId)
    // Nothing is due while a main-loop turn runs, and a check in flight
    // submits nothing. The turn the wrap's own text started is the wrap's.
    mod.ka.epoch += 1
    mod.ka.timer?.cancel()
    mod.ka.timer = undefined
    const w = mod.ka.wrap
    if (w && w.turnId === undefined) {
      if (isWrapTurn(e.text ?? '', w.text)) w.turnId = e.turnId
      else w.sawTurn = true
    }
    mod.lastActivityMs = await $.clock.now()
    // A turn under the id that goes on, before the watch has read it; a
    // turn while the id still reads as one that ended leaves the watch on.
    if (mod.move || mod.gaveUp) await settleIfMoved($, mod)
    if (await isOn($, mod)) await updateStatus($, s => ({ ...s, turnsSinceFull: s.turnsSinceFull + 1 }))
    return next(e)
  })

  // The wrap's own answer: its turn's first step that ended it. A peer
  // message the Stop hook drains, or a notification, continues the turn
  // with more steps, and their answer is not the wrap.
  on('turn.step', async function* ($, e, next) {
    const r = yield* next(e)
    const w = mod.ka.wrap
    if (e.agentId === undefined && w && w.turnId === e.turnId && w.reply === undefined && r.stopReason === 'end_turn') w.reply = r.answer
    return r
  })

  on('turn.complete', async ($, e, next) => {
    const done = await next(e)
    if (e.agentId !== undefined) return done
    mod.running.delete(e.turnId)
    mod.lastActivityMs = await $.clock.now()
    await keepaliveTurnEnded($, mod, e)
    // Whatever the Stop hook left (or all of it, where no Stop hook runs).
    if (mod.canPoll) $.clock.after(250, () => void tick($, mod, true))
    return done
  })

  on('tool.call', { tool: 'mcp__attend__attend_listen' }, async $ => {
    const was = isActive(await readStatus($))
    await registerActiveTools($)
    return listenResult(await activate($, mod, await $.session.id()), was, mod.canPoll)
  })

  on('tool.call', { tool: 'mcp__attend__attend_send' }, async ($, e) => {
    if (!(await isOn($, mod))) return DORMANT_REFUSAL
    const a = input<{ message: unknown; channel: unknown; to: unknown }>(e)
    const message = typeof a.message === 'string' ? a.message : ''
    if (message.trim() === '') return { result: 'message is empty; nothing sent', isError: true }
    const channel = typeof a.channel === 'string' && a.channel.trim() !== '' ? a.channel : undefined
    const to = typeof a.to === 'string' && a.to.trim() !== '' ? a.to : undefined
    mod.lastActivityMs = await $.clock.now()
    return toolResult(await run($, sendArgv(message, channel, to)), 'attend message send')
  })

  on('tool.call', { tool: 'mcp__attend__attend_reply' }, async ($, e) => {
    if (!(await isOn($, mod))) return DORMANT_REFUSAL
    const a = input<{ message: unknown; re: unknown }>(e)
    const message = typeof a.message === 'string' ? a.message : ''
    if (message.trim() === '') return { result: 'message is empty; nothing sent', isError: true }
    // A handle this module delivered maps here; any other (one the Stop-hook
    // drain printed) goes to attend as written, which resolves it.
    const raw = typeof a.re === 'string' ? a.re.trim().replace(/^~/, '') : ''
    const re = raw === '' ? undefined : mod.ids.get(raw) ?? raw
    mod.lastActivityMs = await $.clock.now()
    return toolResult(await run($, replyArgv(message, re)), 'attend message reply')
  })

  on('tool.call', { tool: 'mcp__attend__attend_join' }, async ($, e) => {
    if (!(await isOn($, mod))) return DORMANT_REFUSAL
    const a = input<{ channel: unknown }>(e)
    const channel = typeof a.channel === 'string' ? a.channel.trim() : ''
    if (channel === '') return { result: 'channel is empty; nothing joined', isError: true }
    mod.lastActivityMs = await $.clock.now()
    const r = await run($, joinArgv(channel))
    if (r.exitCode === 0) {
      const label = `#${channel.replace(/^[#@]/, '')}`
      mod.joined.add(label)
      await updateStatus($, s => ({ ...s, channels: [...new Set([...s.channels, label])] }))
    }
    return toolResult(r, 'attend channel join')
  })

  on('tool.call', { tool: 'mcp__attend__attend_peers' }, async $ => {
    if (!(await isOn($, mod))) return DORMANT_REFUSAL
    const r = await run($, PEERS_ARGV)
    if (r.exitCode === 0) await updateStatus($, s => ({ ...s, peers: parsePeerCount(r.stdout) }))
    return toolResult(r, 'attend peer list')
  })

  // One row while active, in the shape of the operator's statusline: peers,
  // unread (or waiting, while the cache is cold), channels, and the keepalive
  // policy's segment while it runs. Name and project are the statusline's.
  // Dormant draws the keepalive segment alone while it shows, else nothing.
  on('ui.render', { component: 'AbovePrompt' }, async ($, e, next) => {
    if (e.props.hasSurvey) return next(e)
    const text = rowText(await readStatus($), await $.clock.now())
    if (text === undefined) return next(e)
    const { Box, Text } = $.ui.resolve(e)
    return (
      <Box flexDirection="row">
        <Text color={SUBTLE}>{'\uE0B6'}</Text>
        <Text color={FG} backgroundColor={SUBTLE}>
          {' '}
          {text}{' '}
        </Text>
        <Text color={SUBTLE}>{'\uE0B4'}</Text>
      </Box>
    )
  })

  // The human's row for a delivery this plugin submitted: one dim line per
  // message. ctrl+o (isExpanded) and rows of any other origin keep the
  // engine's drawing. The model still reads the full framed text.
  on('ui.render', { component: 'UserMessage' }, ($, e, next) => {
    const { origin, isExpanded, text } = e.props
    if (isExpanded || origin.kind !== 'plugin' || origin.name !== PLUGIN) return next(e)
    const lines = peerLines(text)
    if (!lines) return next(e)
    const { Box, Text } = $.ui.resolve(e)
    return (
      <Box flexDirection="column">
        {lines.map((l, i) => (
          <Text key={String(i)} dimColor wrap="truncate-end">
            {l}
          </Text>
        ))}
      </Box>
    )
  })
}
