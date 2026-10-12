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
import type { EngineInterface, ProcessRunResult, Register, Timer } from 'claude-code'

import type { AttendStatus } from '../types/index.d.ts'
import {
  DEFAULT_CONFIG,
  DRAIN_ARGV,
  KEEPWARM_CHECK_ARGV,
  LISTEN_OFF_ARGV,
  LISTEN_ON_ARGV,
  PEERS_ARGV,
  UNREAD_ARGV,
  configArgv,
  formatMessage,
  frameInbound,
  isFullPreamble,
  joinArgv,
  keepwarmLine,
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

// agent-ways theme (tools/agent-theme/themes/agent-ways.theme).
const SUBTLE = '#3a3a3c'
const FG = '#d0d0d0'

/** What the module keeps between dispatches. A reload starts it afresh; the
 *  mode survives in $.state and session.start restores it. */
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
  toolsRegistered: boolean
  /** Short `~id` handles of delivered messages, to their full ids. */
  ids: Map<string, string>
  /** Set on activation; the mode in $.state is the copy a reload reads. */
  active: boolean
  /** A drained batch no submit has taken yet. */
  pending: Listed[]
  /** Submits of `pending` that failed. */
  submitTries: number
  /** Channels joined through attend_join, shown until attend reports rooms. */
  joined: Set<string>
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
      'Turn on attend (peer messaging between Claude sessions and people in attend-chat) for the rest of this session. Call it only when the user asks to use attend. It delivers messages addressed to this project, shows one status row, and adds attend_send, attend_reply, attend_join and attend_peers. It stays on until Claude Code exits.',
    inputSchema: { type: 'object', properties: {} },
    isDeferred: false,
  })
}

async function registerActiveTools($: $, mod: Mod) {
  if (mod.toolsRegistered) return
  mod.toolsRegistered = true
  await $.tool.register({
    name: 'attend_send',
    description:
      'Start a new topic with peer sessions over attend. Defaults to the #open base channel; `channel` scopes it to a named channel, `to` to one project path. To answer a message that arrived, use attend_reply. The message is passed as is: no quoting needed, and attend splits a long one into numbered parts.',
    inputSchema: {
      type: 'object',
      properties: {
        message: { type: 'string', description: 'The message body.' },
        channel: { type: 'string', description: 'A named channel, with or without #.' },
        to: { type: 'string', description: 'A project path to address the message to.' },
      },
      required: ['message'],
    },
    isDeferred: false,
  })
  await $.tool.register({
    name: 'attend_reply',
    description:
      "Reply to a peer message that arrived over attend. Threads to the newest inbound message, or to the one given as `re` (the `~id` at the end of its line, or a full id), in that message's channel. Silence is a valid reply.",
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
    description: 'List the active Claude Code sessions on this machine and their channels.',
    inputSchema: { type: 'object', properties: {} },
    isDeferred: false,
  })
}

/** Tells attend this session is listening; the next drain applies the
 *  cold-start rule, so messages addressed to the project while it was
 *  dormant arrive. */
async function listenOn($: $): Promise<ProcessRunResult> {
  const r = await run($, LISTEN_ON_ARGV)
  await updateStatus($, s => ({ ...s, listening: r.exitCode === 0 }))
  return r
}

/** Makes the session active for the rest of the process: tools, listening,
 *  the poll. Idempotent. Returns attend's answer to `listen on`. */
async function activate($: $, mod: Mod): Promise<ProcessRunResult> {
  mod.config = await readConfig($, mod.cwd)
  mod.canPoll = mod.canPoll || mod.config.headlessPoll
  mod.lastActivityMs = await $.clock.now()
  mod.active = true
  await updateStatus($, s => ({ ...s, mode: 'active' as const }))
  await registerActiveTools($, mod)
  const r = await listenOn($)
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
    if (!isTurnRunning) await poll($, mod)
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

/** One drain; on messages, a second after BATCH_MS, then one prompt for the
 *  batch. A turn is submitted only when the drain delivered at least one
 *  message: the cold start's held-back note and the session's name are
 *  fields of the drain's document, never a message.
 *
 *  The drain marks what it delivers read before the prompt is submitted, so
 *  the batch waits in `mod.pending` until a submit takes it. A submit that
 *  fails, or that a hook drops, leaves the batch there and the next poll
 *  tries it again before draining more, up to SUBMIT_TRIES times. */
async function poll($: $, mod: Mod) {
  if (mod.isPolling) return
  mod.isPolling = true
  try {
    if (mod.pending.length === 0) {
      const first = parseDrain((await run($, DRAIN_ARGV)).stdout)
      if (!first || first.messages.length === 0) return
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
    .submit({ text: frameInbound(msgs.map(formatMessage), msgs.length, full) })
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
 *  puts the mode back in $.state, so an engine that clears the session's
 *  state on /clear does not drop a session that is still listening. */
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
    toolsRegistered: false,
    ids: new Map(),
    active: false,
    pending: [],
    submitTries: 0,
    joined: new Set(),
  }

  on('session.start', async ($, e, next) => {
    mod.cwd = e.cwd
    mod.config = await readConfig($, e.cwd)
    // attend_mod.install false: the mod is unavailable, nothing is listed.
    if (!mod.config.install) return next(e)
    mod.canPoll = e.isInteractive || mod.config.headlessPoll
    await registerListen($)
    const wasActive = isActive(await readStatus($))
    if (wasActive || (mod.config.listening === 'active' && mod.canPoll)) {
      // Listed by turn one; listening and the first drain follow the hook.
      await registerActiveTools($, mod)
      $.clock.after(0, () => void activate($, mod))
    } else {
      // Dormant: attend's drain delivers nothing to this session either.
      $.clock.after(0, () => void run($, LISTEN_OFF_ARGV))
    }
    return next(e)
  })

  // A /clear goes on under a new session id with no session.start: an active
  // session tells attend again that the new id is listening.
  // A /clear empties the model's context as a compaction does, so the next
  // delivery carries the full preamble again.
  on('session.end', async ($, e, next) => {
    const done = await next(e)
    if (e.reason === 'clear' && (await isOn($, mod))) {
      await updateStatus($, s => ({ ...s, afterCompaction: true, turnsSinceFull: 0 }))
      $.clock.after(1_000, () => void listenOn($))
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
    const r = await activate($, mod)
    const tools = ' attend_send, attend_reply, attend_join and attend_peers are available.'
    if (r.exitCode !== 0) {
      return {
        result: `attend could not set this session listening, so no message will be delivered to it; the tools can still send.${tools}\nattend listen on failed (${r.exitCode}): ${r.stderr.trim()}`,
        isError: true,
      }
    }
    const lead = was ? 'attend was already listening in this session.' : 'attend is listening for the rest of this session.'
    const poll = mod.canPoll
      ? ' Messages addressed to this project arrive as peer-data turns.'
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
  // unread, channels. Name and project are the statusline's. Dormant draws
  // nothing.
  on('ui.render', { component: 'AbovePrompt' }, async ($, e, next) => {
    if (e.props.hasSurvey) return next(e)
    const s = await readStatus($)
    if (!isActive(s)) return next(e)
    const { Box, Text } = $.ui.resolve(e)
    const state = s.listening === false ? '○ attend not listening' : `● ${s.peers} peers ✉ ${s.unread} unread`
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
