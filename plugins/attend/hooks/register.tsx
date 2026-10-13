// attend's agent-facing driver in Claude Code (ADR-406).
//
// Dormant is plain Claude Code: one tool, `attend_listen`, and nothing else.
// A session is active from session start when its project resolves
// `attend_mod.listening: active` through the ways config, or from the
// agent's attend_listen call; it stays active until Claude Code exits. While
// active the mod polls attend, delivers peer messages as framed turns, serves
// the send/reply/join/peers tools, draws one status row and drives keepwarm.
//
// Every message travels through attend's signal store by the attend CLI, as
// an argv array with no shell. The mod never uses $.session.send.

import { atom, read, update } from 'claude-code'
import type { EngineInterface, ProcessRunResult, PromptSubmitInput, PromptSubmitResult, Register, Timer } from 'claude-code'

import type { AttendStatus } from '../types/index.d.ts'
import {
  DEFAULT_CONFIG,
  DRAIN_ARGV,
  KEEPWARM_CHECK_ARGV,
  LISTEN_OFF_ARGV,
  LISTEN_ON_ARGV,
  LISTEN_ON_PLAIN_ARGV,
  PEERS_ARGV,
  UNREAD_ARGV,
  WAKE_DRAIN_ARGV,
  configArgv,
  formatMessage,
  frameInbound,
  isFullPreamble,
  joinArgv,
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
  isUnknownWake,
  shortId,
} from './lib.ts'
import type { Listed, ModConfig } from './lib.ts'

type $ = EngineInterface

// Tools are listed as mcp__attend__<name>; the matchers below spell them out,
// so the validator can read them.
const PLUGIN = 'attend'

const DEFAULTS = {
  mode: 'dormant',
  listening: null,
  peers: 0,
  unread: 0,
  channels: [],
  deliveries: 0,
  afterCompaction: false,
  turnsSinceFull: 0,
  awaitingOperator: false,
  cacheLapsed: false,
} satisfies AttendStatus

const status = atom({ plugin: 'attend', key: 'status' } as const, DEFAULTS)

// A drain that finds messages waits this long and drains again, so a burst
// arrives as one turn.
const BATCH_MS = 2_000
// The peer count and the keepwarm check run at most this often.
const PEERS_EVERY_MS = 30_000
const KEEPWARM_EVERY_MS = 60_000
// Polls that retry a batch whose submit failed before it is let go.
const SUBMIT_TRIES = 3
// The $.store key naming the sessions that listened where the project's
// config would not start them listening, newest last, so a resume of one
// comes back listening (ADR-411).
const LISTENED = 'listened'
const LISTENED_KEPT = 50
// A /clear or a /resume tells attend the id that goes on after this long.
const RELISTEN_MS = 1_000

// agent-ways theme (tools/agent-theme/themes/agent-ways.theme).
const SUBTLE = '#3a3a3c'
const FG = '#d0d0d0'

/** What the module keeps between dispatches. A reload starts it afresh, so
 *  the mode a reload reads is the one in $.state: session.start restores it
 *  from there, and a /clear or a /resume, which empty $.state, has it put
 *  back. Across processes the record in $.store answers instead. */
type Mod = {
  config: ModConfig
  /** Whether this session may poll: interactive, or allowed by config. */
  canPoll: boolean
  cwd: string
  /** Turns running now. The poll holds while one runs: the Stop-hook drain
   *  owns delivery at a turn's boundary. */
  running: Set<string>
  isPolling: boolean
  isTicking: boolean
  ticker: Timer | undefined
  lastPollMs: number
  lastPeersMs: number
  lastKeepwarmMs: number
  /** The last delivery, send, reply, join or turn. */
  lastActivityMs: number
  /** Short `~id` handles of delivered messages, to their full ids. */
  ids: Map<string, string>
  /** Set on activation; the mode in $.state is the copy a reload reads. */
  active: boolean
  /** A drained batch no submit has taken yet. */
  pending: Listed[]
  /** `pending` was taken from a prompt the operator made while the cache
   *  had lapsed, and that prompt did not enter. The poll leaves it for the
   *  operator's next prompt instead of starting a turn on the cold cache. */
  isPendingCold: boolean
  /** Submits of `pending` that failed. */
  submitTries: number
  /** attend refused `--wake` as an unknown argument: an attend older than
   *  the wake drain, which the poll then drains without it. */
  noWakeDrain: boolean
  /** Channels joined through attend_join, shown until attend reports rooms. */
  joined: Set<string>
  /** session.start has run; before it, at process start, the engine has not
   *  bound the session and refuses a tool registration. */
  isStarted: boolean
  /** A /clear or a /resume moved the session to an id attend has not been
   *  told is listening yet; the poll holds its drain until it has. */
  isRelistening: boolean
  /** The session started listening from its project's config and the
   *  operator has not prompted yet. The poll submits no turn: what is
   *  pending rides as context on the operator's first prompt, and the row
   *  counts it meanwhile. */
  isAwaitingOperator: boolean
}

async function readStatus($: $): Promise<AttendStatus> {
  return { ...DEFAULTS, ...(await read($, status)) }
}

async function updateStatus($: $, f: (s: AttendStatus) => AttendStatus) {
  await update($, status, s => f({ ...DEFAULTS, ...s }))
}

/** One CLI call; a command that cannot start reads as exit 127. */
async function run($: $, argv: readonly string[], timeoutMs = 15_000): Promise<ProcessRunResult> {
  try {
    return await $.process.run(argv, { timeoutMs })
  } catch (err) {
    return { exitCode: 127, stdout: '', stderr: String(err), isStdoutTruncated: false, isStderrTruncated: false }
  }
}

async function readConfig($: $, cwd: string): Promise<ModConfig> {
  const r = await run($, configArgv(cwd), 10_000)
  return r.exitCode === 0 ? parseConfig(r.stdout) : { ...DEFAULT_CONFIG }
}

function toolResult(r: ProcessRunResult, what: string) {
  const text = r.stdout.trim() || r.stderr.trim()
  return r.exitCode === 0
    ? { result: text || 'done' }
    : { result: `${what} failed (${r.exitCode}): ${r.stderr.trim() || r.stdout.trim()}`, isError: true as const }
}

const DORMANT_REFUSAL = {
  result: 'attend is not listening in this session. Call attend_listen first, and only when the user asked to use attend.',
  isError: true as const,
}

async function registerListen($: $) {
  await $.tool.register({
    name: 'attend_listen',
    description:
      'Turn on attend (peer messaging between Claude sessions and people in attend-chat) for the rest of this session. Call it only when the user asks to use attend. It delivers messages addressed to this session, shows one status row, and adds attend_send, attend_reply, attend_join and attend_peers. It stays on until Claude Code exits.',
    inputSchema: { type: 'object', properties: {} },
    isDeferred: false,
  })
}

/** The tools of an active session. Registering one again with the same
 *  spec changes nothing, so every path that finds the session active
 *  registers them, a reloaded module's session.start included. */
async function registerActiveTools($: $) {
  await $.tool.register({
    name: 'attend_send',
    description:
      'Start a new topic over attend. Name someone in `to` to send them a direct message. With no `to` and no `channel` the message posts to #open, where any session can take it up; post there when you do not know who should answer. `channel` posts to a named channel; with `to` as well, the post stays in the channel addressed to them, which wakes them. To answer a message that arrived, use attend_reply. The message is passed as is: no quoting needed, and a long one goes out as one message.',
    inputSchema: {
      type: 'object',
      properties: {
        message: { type: 'string', description: 'The message body.' },
        channel: { type: 'string', description: 'A channel name, with or without #. open or #open posts to #open.' },
        to: {
          type: 'string',
          description: "Who the direct message is for: a session's name (such as Chaucer-beta), a person's member id (such as aaron), or a project path for every session there.",
        },
      },
      required: ['message'],
    },
    isDeferred: false,
  })
  await $.tool.register({
    name: 'attend_reply',
    description:
      "Reply to a message that arrived over attend. The reply is a direct message to its sender, an answer to an #open post included; a reply to a post in a named channel stays in that channel. It threads to the newest inbound message, or to the one given as `re` (the `~id` at the end of its line, or a full id). Silence is a valid reply.",
    inputSchema: {
      type: 'object',
      properties: {
        message: { type: 'string', description: 'The reply body.' },
        re: { type: 'string', description: 'The message replied to: the `~id` at the end of a delivery line.' },
      },
      required: ['message'],
    },
    isDeferred: false,
  })
  await $.tool.register({
    name: 'attend_join',
    description: 'Join an attend channel (created if absent), so its messages reach this session.',
    inputSchema: {
      type: 'object',
      properties: { channel: { type: 'string', description: 'The channel name, with or without #.' } },
      required: ['channel'],
    },
    isDeferred: false,
  })
  await $.tool.register({
    name: 'attend_peers',
    description: "List the active Claude Code sessions on this machine and their channels, and who is in each of this session's channels.",
    inputSchema: { type: 'object', properties: {} },
    isDeferred: false,
  })
}

/** Tells attend this session is listening, with the mod as its host; the
 *  next drain applies the cold-start rule: what was addressed to the project
 *  since this Claude Code process started arrives, and what `#open` and the
 *  channels carry from now on. */
async function listenOn($: $): Promise<ProcessRunResult> {
  let r = await run($, LISTEN_ON_ARGV)
  if (r.exitCode === 2) r = await run($, LISTEN_ON_PLAIN_ARGV)
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

function startTicker($: $, mod: Mod) {
  mod.ticker?.cancel()
  const every = Math.max(1, Math.min(mod.config.pollSeconds, 5)) * 1000
  mod.ticker = $.clock.every(every, () => void tick($, mod, false))
}

/** One beat of the poll: drain while idle, refresh the row, run the keepwarm
 *  check. `force` skips the interval (activation, a turn's end). */
async function tick($: $, mod: Mod, force: boolean) {
  if (mod.isTicking) return
  if (!(await isOn($, mod))) return
  mod.isTicking = true
  try {
    const now = await $.clock.now()
    const isTurnRunning = mod.running.size > 0
    const interval = pollInterval(mod.config, now - mod.lastActivityMs, isTurnRunning)
    if (!force && now - mod.lastPollMs < interval) return
    mod.lastPollMs = now
    if (!isTurnRunning && !mod.isRelistening && !mod.isAwaitingOperator) await poll($, mod)
    await refreshUnread($, mod)
    if (force || now - mod.lastPeersMs >= PEERS_EVERY_MS) await refreshPeers($, mod)
    if (mod.config.keepwarm === 'on' && mod.running.size === 0 && now - mod.lastKeepwarmMs >= KEEPWARM_EVERY_MS) {
      mod.lastKeepwarmMs = now
      await keepwarm($)
    }
  } finally {
    mod.isTicking = false
  }
}

/** One wake drain; on messages, a plain drain after BATCH_MS, then one
 *  prompt for the batch. The wake drain delivers only once something pending
 *  is addressed to the session, and then delivers all that is pending. A
 *  turn is submitted only when the drain delivered at least one message: the
 *  cold start's held-back note and the session's name are fields of the
 *  drain's document, never a message.
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
      // Only what is addressed to the session starts a turn; #open and
      // channel posts wait for its next one (ADR-406 item 1).
      let r = await run($, mod.noWakeDrain ? DRAIN_ARGV : WAKE_DRAIN_ARGV)
      if (!mod.noWakeDrain && isUnknownWake(r)) {
        mod.noWakeDrain = true
        r = await run($, DRAIN_ARGV)
      }
      const first = parseDrain(r.stdout)
      if (!first) return
      // Listening turned off elsewhere (attend listen off, a private scene),
      // or an identity attend cannot resolve: the row says so.
      const listening = first.status === 'drained'
      const was = await readStatus($)
      if (was.listening !== listening || was.cacheLapsed !== first.cacheLapsed) {
        await updateStatus($, s => ({ ...s, listening, cacheLapsed: first.cacheLapsed }))
      }
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
      // attend inbox, and the poll goes on to new messages.
      await $.ui.toast(`attend: ${mod.pending.length} message(s) could not be delivered; attend inbox lists them`)
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

/** Holds, or stops holding, the session's turns until the operator's first
 *  prompt. The module's flag serves a /clear, which empties $.state; the
 *  copy in $.state serves a reload, which starts the module afresh. */
async function setAwaiting($: $, mod: Mod, awaiting: boolean) {
  mod.isAwaitingOperator = awaiting
  await updateStatus($, s => ({ ...s, awaitingOperator: awaiting }))
}

/** The operator's first prompt in a session the config started listening:
 *  one drain, and what it delivered rides `e` as context, the cold start's
 *  held-back note with it, rather than as a turn of its own. A prompt that
 *  does not enter leaves the messages in `mod.pending`, which the poll
 *  submits as it does a batch whose submit failed. */
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
  const note = d?.note ? `attend: ${oneLine(d.note)}` : undefined
  const block = msgs.length > 0 ? frameInbound(msgs.map(formatMessage), true) : undefined
  const attached = [block, note].filter((x): x is string => x !== undefined)
  if (attached.length === 0) return next(e)
  // Read by attend and not entered: they wait in `mod.pending`. Taken on a
  // lapsed cache, they wait for the operator's next prompt, with the row
  // still saying so; otherwise the poll submits them.
  const keep = async () => {
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
  } else if (msgs.length > 0) {
    await updateStatus($, t => ({ ...t, deliveries: t.deliveries + 1, afterCompaction: false, turnsSinceFull: 0 }))
    $.ui.toast(`attend: ${msgs.length} peer message(s) went to Claude with your prompt`)
  }
  return r
}

/** Origins that are the operator's own prompt: the terminal, Remote
 *  Control, an SDK host's turn, and the owner's ping from Slack. An
 *  unclassified one is not counted as the operator's. */
const OPERATOR_ORIGINS = new Set(['composer', 'bridge', 'sdk', 'slack-ping'])

async function refreshUnread($: $, mod: Mod) {
  const u = parseUnread((await run($, UNREAD_ARGV)).stdout)
  const joined = [...mod.joined]
  if (u) {
    await updateStatus($, s => ({ ...s, unread: u.unread, channels: [...new Set([...u.channels, ...joined])] }))
  } else {
    // An attend without `unread`: the row counts nothing it cannot see.
    await updateStatus($, s => ({ ...s, unread: 0, channels: joined }))
  }
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

export const register: Register = on => {
  const mod: Mod = {
    config: { ...DEFAULT_CONFIG },
    canPoll: false,
    cwd: '.',
    running: new Set(),
    isPolling: false,
    isTicking: false,
    ticker: undefined,
    lastPollMs: 0,
    lastPeersMs: 0,
    lastKeepwarmMs: 0,
    lastActivityMs: 0,
    ids: new Map(),
    active: false,
    pending: [],
    isPendingCold: false,
    submitTries: 0,
    noWakeDrain: false,
    joined: new Set(),
    isStarted: false,
    isRelistening: false,
    isAwaitingOperator: false,
  }

  on('session.start', async ($, e, next) => {
    mod.cwd = e.cwd
    mod.config = await readConfig($, e.cwd)
    // attend_mod.install false: the mod is unavailable, nothing is listed.
    if (!mod.config.install) return next(e)
    mod.canPoll = e.isInteractive || mod.config.headlessPoll
    await registerListen($)
    mod.isStarted = true
    // A reload finds the mode in $.state; a new process (`claude --resume`)
    // finds the session id in the record of sessions that listened.
    const id = await $.session.id()
    const kept = await readStatus($)
    const wasActive = isActive(kept) || (await readListened($)).includes(id)
    if (wasActive || (mod.config.listening === 'active' && mod.canPoll)) {
      // Listed by turn one; listening and the first drain follow the hook.
      // Active from here, so a /clear before activate runs keeps it. A
      // session the config starts listening has had no prompt yet: nothing
      // it finds pending starts a turn before the operator's first prompt.
      // A reload keeps whatever hold $.state records.
      mod.active = true
      await setAwaiting($, mod, isActive(kept) ? kept.awaitingOperator : !wasActive)
      await registerActiveTools($)
      $.clock.after(0, () => void activate($, mod, id))
    } else {
      // Dormant: attend's drain delivers nothing to this session either.
      $.clock.after(0, () => void run($, LISTEN_OFF_ARGV))
    }
    return next(e)
  })

  // A /clear or an in-session /resume goes on under another session id with
  // no session.start, and Claude Code empties this plugin's $.state. The
  // settings SessionStart (source `clear` or `resume`, `session_id` the id
  // that goes on) fires after that. An active session, or a /resume to a
  // session that listened, puts the mode back at once, so a reload finds it
  // active, records the id, and tells attend the id is listening. The poll
  // holds its drain until then. The model's context changed as at a
  // compaction, so the next delivery carries the preamble line. At process
  // start this event comes before session.start, which handles that case.
  on('classic.SessionStart', async ($, e, next) => {
    const done = await next(e)
    if (!mod.isStarted || (e.source !== 'clear' && e.source !== 'resume')) return done
    const id = e.session_id || (await $.session.id())
    const goesOn = (await isOn($, mod)) || (e.source === 'resume' && (await readListened($)).includes(id))
    if (!goesOn) return done
    await updateStatus($, s => ({
      ...s,
      mode: 'active' as const,
      afterCompaction: true,
      turnsSinceFull: 0,
      awaitingOperator: mod.isAwaitingOperator,
    }))
    await rememberListening($, mod, id)
    mod.isRelistening = true
    if (mod.active) {
      $.clock.after(RELISTEN_MS, () => void listenOn($).finally(() => (mod.isRelistening = false)))
    } else {
      mod.active = true
      await registerActiveTools($)
      $.clock.after(RELISTEN_MS, () => void activate($, mod, id))
    }
    return done
  })

  // A compaction drops the earlier preamble from the model's view, so the
  // next delivery shows it in full. A precompute installs nothing.
  on('session.compact', async ($, e, next) => {
    const done = await next(e)
    if (e.trigger !== 'precompute' && e.agentId === undefined && done.skip === undefined) {
      await updateStatus($, s => ({ ...s, afterCompaction: true }))
    }
    return done
  })

  // The operator's first prompt in a session the config started listening,
  // or their next prompt while a message waits on a lapsed cache: what is
  // pending enters beside it, read in the same turn, rather than as a turn
  // the operator never asked for.
  on('prompt.submit', async ($, e, next) => {
    if (!OPERATOR_ORIGINS.has(e.origin.kind)) return next(e)
    const isHeldCold = (await readStatus($)).cacheLapsed || mod.isPendingCold
    if (!mod.isAwaitingOperator && !isHeldCold) return next(e)
    await setAwaiting($, mod, false)
    if (isHeldCold) await updateStatus($, s => ({ ...s, cacheLapsed: false }))
    if (!(await isOn($, mod))) return next(e)
    return enterWithPending($, mod, e, next, isHeldCold)
  })

  on('turn.start', async ($, e, next) => {
    mod.running.add(e.turnId)
    mod.lastActivityMs = await $.clock.now()
    if (await isOn($, mod)) await updateStatus($, s => ({ ...s, turnsSinceFull: s.turnsSinceFull + 1 }))
    return next(e)
  })

  on('turn.complete', async ($, e, next) => {
    const done = await next(e)
    mod.running.delete(e.turnId)
    mod.lastActivityMs = await $.clock.now()
    // Whatever the Stop hook left (or all of it, where no Stop hook runs).
    if (mod.canPoll) $.clock.after(250, () => void tick($, mod, true))
    return done
  })

  on('tool.call', { tool: 'mcp__attend__attend_listen' }, async $ => {
    const was = isActive(await readStatus($))
    await registerActiveTools($)
    const r = await activate($, mod, await $.session.id())
    const tools = ' attend_send, attend_reply, attend_join and attend_peers are available.'
    if (r.exitCode !== 0) {
      return {
        result: `attend could not set this session listening, so no message will be delivered to it; the tools can still send.${tools}\nattend listen on failed (${r.exitCode}): ${r.stderr.trim()}`,
        isError: true,
      }
    }
    const lead = was ? 'attend was already listening in this session.' : 'attend is listening for the rest of this session.'
    const poll = mod.canPoll
      ? ' Messages addressed to this session arrive as peer-data turns; #open and channel posts arrive with your next turn.'
      : ' This session does not poll (a -p or SDK run); messages arrive at the end of a turn.'
    return { result: `${lead}${poll}${tools}` }
  })

  on('tool.call', { tool: 'mcp__attend__attend_send' }, async ($, e) => {
    if (!(await isOn($, mod))) return DORMANT_REFUSAL
    const a = input<{ message: unknown; channel: unknown; to: unknown }>(e)
    const message = typeof a.message === 'string' ? a.message : ''
    if (message.trim() === '') return { result: 'message is empty; nothing sent', isError: true }
    const channel = typeof a.channel === 'string' && a.channel.trim() !== '' ? a.channel : undefined
    const to = typeof a.to === 'string' && a.to.trim() !== '' ? a.to : undefined
    mod.lastActivityMs = await $.clock.now()
    return toolResult(await run($, sendArgv(message, channel, to)), 'attend send')
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
    return toolResult(await run($, replyArgv(message, re)), 'attend reply')
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
    return toolResult(r, 'attend join')
  })

  on('tool.call', { tool: 'mcp__attend__attend_peers' }, async $ => {
    if (!(await isOn($, mod))) return DORMANT_REFUSAL
    const r = await run($, PEERS_ARGV)
    if (r.exitCode === 0) await updateStatus($, s => ({ ...s, peers: parsePeerCount(r.stdout) }))
    return toolResult(r, 'attend peers')
  })

  // One row while active, in the shape of the operator's statusline: peers,
  // unread (or waiting, while the cache is cold), channels. Name and project are the statusline's. Dormant draws
  // nothing.
  on('ui.render', { component: 'AbovePrompt' }, async ($, e, next) => {
    if (e.props.hasSurvey) return next(e)
    const s = await readStatus($)
    if (!isActive(s)) return next(e)
    const { Box, Text } = $.ui.resolve(e)
    // A message held on a lapsed cache waits for the operator's prompt.
    const mail = s.cacheLapsed && s.unread > 0 ? `${s.unread} waiting · cold` : `${s.unread} unread`
    const state = s.listening === false ? '○ attend not listening' : `● ${s.peers} peers ✉ ${mail}`
    const channels = s.channels.length > 0 ? ` ${s.channels.join(' ')}` : ''
    return (
      <Box flexDirection="row">
        <Text color={SUBTLE}>{''}</Text>
        <Text color={FG} backgroundColor={SUBTLE}>
          {' '}
          {state}
          {channels}{' '}
        </Text>
        <Text color={SUBTLE}>{''}</Text>
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
