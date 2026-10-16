// Pure pieces of the attend mod (ADR-406): argv building, parsing what the
// attend and ways CLIs print, framing a delivery, and the timing rules. No `$`
// here, so the tests exercise them directly.

import type { Timer } from 'claude-code'

import type { AttendStatus, KeepaliveRow } from '../types/index.d.ts'

export const ATTEND = 'attend'
/** This mod's version, equal to .claude-plugin/plugin.json's. A mod cannot read
 *  its manifest, so scripts/check-plugins.sh compares the two and
 *  scripts/release.sh bump moves both. The beat sends it to attend, which
 *  tells the operator when the session runs an older mod than the installed
 *  one. */
export const MOD_VERSION = '0.28.0'
export const WAYS = 'ways'

// ── argv ─────────────────────────────────────────────────────────────────
// Every call is an argv array with no shell. A body is one element after
// `--`, so no quoting rule applies to it and a body that starts with a dash
// stays a body. attend sends a long body as one message.

/** `attend message send [--channel C] [--to T] -- <message>`: `to` is a session's
 *  name, a person's member id or a project path, and makes it a direct
 *  message; with neither it posts to #open, as `channel` open does. */
export function sendArgv(message: string, channel?: string, to?: string): string[] {
  const argv = [ATTEND, 'message', 'send']
  if (channel) argv.push('--channel', bareChannel(channel))
  if (to) argv.push('--to', to)
  argv.push('--', message)
  return argv
}

/** `attend message reply [--re <id>] -- <message>`: threads to the newest message
 *  delivered, or to `re`, and goes to that message's sender as a direct
 *  message, or into its channel when it was said in a named channel. attend
 *  refuses a parent whose channel this session left, whose sender session
 *  has exited, or that it cannot find. */
export function replyArgv(message: string, re?: string): string[] {
  return re ? [ATTEND, 'message', 'reply', '--re', re, '--', message] : [ATTEND, 'message', 'reply', '--', message]
}

export function joinArgv(channel: string): string[] {
  return [ATTEND, 'channel', 'join', bareChannel(channel)]
}

function bareChannel(channel: string): string {
  return channel.trim().replace(/^[#@]/, '')
}

/** `listen on` names the mod as the host, so attend's Stop-hook drain speaks
 *  the mod's format and tools in this session. */
export const LISTEN_ON_ARGV = [ATTEND, 'listen', 'on', '--host', 'mod'] as const
export const LISTEN_OFF_ARGV = [ATTEND, 'listen', 'off'] as const
export const DRAIN_ARGV = [ATTEND, 'message', 'inbox', '--drain', '--json'] as const
/** The idle poll's drain: it takes nothing unless something pending is
 *  addressed to this session, sent to its project, a reply to a message it
 *  sent, or a post in one of its named channels (ADR-406 item 2). */
export const WAKE_DRAIN_ARGV = [ATTEND, 'message', 'inbox', '--drain', '--json', '--wake'] as const
export const UNREAD_ARGV = [ATTEND, 'message', 'unread', '--json'] as const
/** Touches the session's heartbeat while it listens, and records the mod's
 *  version for the session. The drain touches the heartbeat too, but runs
 *  only between turns and at the poll's interval, which can be longer than
 *  attend's 90 s liveness grace. */
export const BEAT_ARGV = [ATTEND, 'listen', 'beat', '--mod', MOD_VERSION] as const
/** The beat for an attend that predates `--mod` and exits 2 on it (a usage
 *  error): the heartbeat without the version. */
export const PLAIN_BEAT_ARGV = [ATTEND, 'listen', 'beat'] as const
export const PEERS_ARGV = [ATTEND, 'peer', 'list'] as const
export const KEEPWARM_CHECK_ARGV = [ATTEND, 'keepwarm', 'check'] as const
/** The operator prompted at `atS` (epoch seconds): the keepalive count
 *  starts again (ADR-414 item 4). */
export function resetArgv(atS: number): string[] {
  return [ATTEND, 'keepwarm', 'reset', '--at', String(Math.floor(atS))]
}

/** The keepalive policy's judgment, once, with the autocompact gate the mod
 *  read (ADR-414 item 7). */
export function policyArgv(gate: Gate): string[] {
  return [ATTEND, 'keepwarm', 'check', '--policy', '--json', '--autocompact', gate]
}

/** Where the wrap's reply went, for the status card. */
export function wrappedArgv(outcome: WrapOutcome, detail: string): string[] {
  return [ATTEND, 'keepwarm', 'wrapped', outcome, '--detail', oneLine(detail)]
}

/** The wrap's reply sent over attend: one message, its first line the
 *  handoff line attend wrote, to `#name` as a channel post and to anything
 *  else as a direct message (ADR-414 item 6). */
export function wrapSendArgv(route: WrapRoute, reply: string): string[] {
  const body = `${oneLine(route.handoff)}\n${reply}`
  return route.to.startsWith('#') ? sendArgv(body, route.to) : sendArgv(body, undefined, route.to)
}

/** Whether `to` names `#open`, which reaches every listening session on the
 *  host. attend refuses it as a wrap's recipient; the mod refuses it too. */
export function isOpenChannel(to: string): boolean {
  return /^#\s*open\s*$/i.test(to)
}

/** The resolved `attend_mod` section, through the ways config layers: the
 *  user file, the target's file, then the project's `.claude/ways.yaml`. */
export function configArgv(project: string): string[] {
  return [WAYS, 'settings', 'list', 'attend_mod', '--json', '--effective', '--project', project]
}

// ── the module's state ─────────────────────────────────────────────────

/** What the module keeps between dispatches. A reload starts it afresh, so
 *  the mode a reload reads is the one in $.state: session.start restores it
 *  from there, and a /clear or a /resume, which empty $.state, has it put
 *  back. Across processes the record in $.store answers instead. */
export type Mod = {
  config: ModConfig
  /** Whether this session may poll: interactive, or allowed by config. */
  canPoll: boolean
  isInteractive: boolean
  cwd: string
  /** The main loop's turns running now. The poll holds while one runs: the
   *  Stop-hook drain owns delivery at a turn's boundary. */
  running: Set<string>
  isPolling: boolean
  isTicking: boolean
  ticker: Timer | undefined
  lastPollMs: number
  lastPeersMs: number
  lastKeepwarmMs: number
  lastBeatMs: number
  /** attend rejected `--mod`, so beats go without it. */
  isBeatPlain: boolean
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
  /** Channels joined through attend_join, shown until attend reports rooms. */
  joined: Set<string>
  /** session.start has run; before it, at process start, the engine has not
   *  bound the session and refuses a tool registration. */
  isStarted: boolean
  /** A /clear or a /resume moved the session to an id attend has not been
   *  told is listening yet; the poll holds its drain until it has. */
  isRelistening: boolean
  /** The cold start's held-back note, from a drain the poll ran. A wake
   *  delivery carries messages alone, so the note rides the operator's next
   *  prompt as context. */
  heldNote: string | undefined
  /** The keepalive policy's state (ADR-414). */
  ka: Keepalive
}

export type Keepalive = {
  /** keepaliveApplies: an interactive session, keepwarm on, the policy
   *  enabled. Never a -p run or an SDK session. */
  applies: boolean
  gate: Gate
  gateAtMs: number
  /** The one timer: the next check. */
  timer: Timer | undefined
  /** The reset, the check and the wrap's report run one at a time, so a
   *  prompt's reset is never overwritten by a check that read the ledger
   *  before it. */
  chain: Promise<unknown>
  /** Moves at each operator prompt and main-loop turn start: a check whose
   *  epoch moved while attend judged submits nothing. */
  epoch: number
  /** The operator's last prompt, held while the gate last read on or
   *  unknown; sent to attend when the gate first reads off. */
  heldPromptMs: number | undefined
  /** Checks that could not judge since the last turn's end. */
  retries: number
  /** A wrap submitted, its text, the main-loop turn that text started, and
   *  the answer of that turn's first step that ended it (`end_turn`): the
   *  wrap itself, whatever a Stop-hook continuation or a notification adds
   *  to the turn after it. */
  wrap: { route: WrapRoute; text: string; turnId: string | undefined; reply: string | undefined; sawTurn: boolean } | undefined
}

// ── tools ────────────────────────────────────────────────────────────────

/** The one tool of a dormant session. */
export const LISTEN_TOOL = {
    name: 'attend_listen',
    description:
      'Turn on attend (peer messaging between Claude sessions and people in attend-chat) for the rest of this session. Call it only when the user asks to use attend. It delivers messages addressed to this session, shows one status row, and adds attend_send, attend_reply, attend_join and attend_peers. It stays on until Claude Code exits.',
    inputSchema: { type: 'object', properties: {} },
    isDeferred: false,
  } as const

/** The tools of an active session. */
export const ACTIVE_TOOLS = [
  {
    name: 'attend_send',
    description:
      'Start a new topic over attend. Name someone in `to` to send them a direct message. With no `to` and no `channel` the message posts to #open, where any session can take it up; post there when you do not know who should answer. #open reaches every listening session, in other projects too, so keep chatter about this project to a named channel or a direct message. `channel` posts to a named channel, which wakes its idle members; with `to` as well, the post stays in the channel addressed to them. To answer a message that arrived, use attend_reply. The message is passed as is: no quoting needed, and a long one goes out as one message.',
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
  },
  {
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
  },
  {
    name: 'attend_join',
    description: 'Join an attend channel (created if absent), so its messages reach this session.',
    inputSchema: {
      type: 'object',
      properties: { channel: { type: 'string', description: 'The channel name, with or without #.' } },
      required: ['channel'],
    },
    isDeferred: false,
  },
  {
    name: 'attend_peers',
    description: "List the active Claude Code sessions on this machine and their channels, and who is in each of this session's channels.",
    inputSchema: { type: 'object', properties: {} },
    isDeferred: false,
  },
] as const

/** A CLI call as a tool's result: its output, or why it failed. */
export function toolResult(r: { exitCode: number; stdout: string; stderr: string }, what: string) {
  const text = r.stdout.trim() || r.stderr.trim()
  return r.exitCode === 0
    ? { result: text || 'done' }
    : { result: `${what} failed (${r.exitCode}): ${r.stderr.trim() || r.stdout.trim()}`, isError: true as const }
}

export const DORMANT_REFUSAL = {
  result: 'attend is not listening in this session. Call attend_listen first, and only when the user asked to use attend.',
  isError: true as const,
}

/** attend_listen's answer, from attend's answer to `listen on`. */
export function listenResult(r: { exitCode: number; stderr: string }, was: boolean, canPoll: boolean) {
  const tools = ' attend_send, attend_reply, attend_join and attend_peers are available.'
  if (r.exitCode !== 0) {
    return {
      result: `attend could not set this session listening, so no message will be delivered to it; the tools can still send.${tools}\nattend listen on failed (${r.exitCode}): ${r.stderr.trim()}`,
      isError: true as const,
    }
  }
  const lead = was ? 'attend was already listening in this session.' : 'attend is listening for the rest of this session.'
  const poll = canPoll
    ? ' Messages addressed to this session and posts in its channels arrive as peer-data turns; other #open posts arrive with your next turn.'
    : ' This session does not poll (a -p or SDK run); messages arrive at the end of a turn.'
  return { result: `${lead}${poll}${tools}` }
}

/** Origins that are the operator's own prompt: the terminal, Remote
 *  Control, an SDK host's turn, and the owner's ping from Slack. An
 *  unclassified one is not counted as the operator's. */
export const OPERATOR_ORIGINS = new Set(['composer', 'bridge', 'sdk', 'slack-ping'])

// ── configuration ────────────────────────────────────────────────────────

export type ModConfig = {
  /** How a session in this project starts. */
  listening: 'active' | 'dormant'
  /** Whether an active session runs attend's keepwarm check (ADR-182). */
  keepwarm: 'on' | 'off'
  /** false: the mod is unavailable, nothing is listed. */
  install: boolean
  /** The poll interval while awake, in seconds. */
  pollSeconds: number
  /** The poll interval once the session has been quiet, in seconds. */
  slowPollSeconds: number
  /** Quiet this long and the poll slows, in minutes. */
  quietMinutes: number
  /** The preamble shows again once this many turns have passed. */
  preambleEveryTurns: number
  /** Whether a `-p` run or an SDK session may poll. */
  headlessPoll: boolean
  /** `attend_mod.keepalive.enabled`: the keepalive policy may run (ADR-414). */
  keepalive: boolean
  /** `attend_mod.keepalive.margin_minutes`: a keepalive is due this long
   *  before the cache lapses. attend judges; the mod uses it only to set a
   *  dormant session's timer. */
  marginMinutes: number
}

/** What the mod runs under when ways cannot answer. The keepalive policy
 *  fails closed: off, as `keepwarm` and `install` fall to their quiet side
 *  in the registry (ADR-414 item 8). */
export const DEFAULT_CONFIG: ModConfig = {
  listening: 'dormant',
  keepwarm: 'on',
  install: true,
  pollSeconds: 5,
  slowPollSeconds: 60,
  quietMinutes: 20,
  preambleEveryTurns: 20,
  headlessPoll: false,
  keepalive: false,
  marginMinutes: 10,
}

/** Reads `ways settings list attend_mod --json --effective`. A key that is
 *  absent or of the wrong type keeps its default, so a ways CLI that is
 *  missing or older than the section leaves the mod dormant. */
export function parseConfig(stdout: string): ModConfig {
  let keys: Record<string, { value?: unknown }> = {}
  try {
    const doc = JSON.parse(stdout) as { keys?: Record<string, { value?: unknown }> }
    keys = doc.keys ?? {}
  } catch {
    return { ...DEFAULT_CONFIG }
  }
  const v = (k: string) => keys[`attend_mod.${k}`]?.value
  const num = (k: string, d: number) => {
    const x = v(k)
    return typeof x === 'number' && Number.isFinite(x) && x > 0 ? x : d
  }
  const bool = (k: string, d: boolean) => {
    const x = v(k)
    return typeof x === 'boolean' ? x : d
  }
  return {
    listening: v('listening') === 'active' ? 'active' : 'dormant',
    keepwarm: v('keepwarm') === 'off' ? 'off' : 'on',
    install: bool('install', DEFAULT_CONFIG.install),
    pollSeconds: num('poll_seconds', DEFAULT_CONFIG.pollSeconds),
    slowPollSeconds: num('slow_poll_seconds', DEFAULT_CONFIG.slowPollSeconds),
    quietMinutes: num('quiet_minutes', DEFAULT_CONFIG.quietMinutes),
    preambleEveryTurns: num('preamble_every_turns', DEFAULT_CONFIG.preambleEveryTurns),
    headlessPoll: bool('headless_poll', DEFAULT_CONFIG.headlessPoll),
    // Only an explicit true: a ways that does not know the key runs none.
    keepalive: v('keepalive.enabled') === true,
    marginMinutes: num('keepalive.margin_minutes', DEFAULT_CONFIG.marginMinutes),
  }
}

// ── timing ───────────────────────────────────────────────────────────────

/** The interval the next poll waits: the slow one once the session has had
 *  no delivery, no send of its own and no turn for the quiet period. */
export function pollInterval(c: ModConfig, idleMs: number, isTurnRunning: boolean): number {
  const quiet = !isTurnRunning && idleMs >= c.quietMinutes * 60_000
  return (quiet ? c.slowPollSeconds : c.pollSeconds) * 1000
}

/** The preamble goes with delivery 0, the first after a compaction, and
 *  the first once `every` turns have passed since it last showed. */
export function isFullPreamble(deliveries: number, afterCompaction: boolean, turnsSinceFull: number, every: number): boolean {
  return deliveries === 0 || afterCompaction || turnsSinceFull >= every
}

/** Whether the keepalive policy may run in a session started with
 *  `isInteractive` under `c` (ADR-414 item 1): never a -p run or an SDK
 *  session, whatever `headless_poll` says; autocompact is judged at each
 *  check. Where it applies, keepalive.tsx drives every cache wake. */
export function keepaliveApplies(c: ModConfig, isInteractive: boolean): boolean {
  return isInteractive && c.install && c.keepwarm === 'on' && c.keepalive
}

/** When the policy's timer asks attend first after a main-loop turn's end: the margin before the hour tier's cache lapses, or ADR-182's
 *  50 minutes when that comes sooner, which an armed window wakes at. attend
 *  answers with the due point it judged. */
export function firstCheckMs(c: ModConfig): number {
  return Math.min(60 - c.marginMinutes, 50) * 60_000
}

// ── the keepalive policy (ADR-414) ───────────────────────────────────────

/** The autocompact gate: `off` lets the policy run; `unknown` counts as on,
 *  the side that starts no turn the operator did not type. */
export type Gate = 'on' | 'off' | 'unknown'

/** The gate from `$.session.usage({ breakdown: 'summary' })`, else from
 *  `autoCompactEnabled` in `$.settings.read()`, where only an explicit false
 *  is off and an absent key is Claude Code's default, on. Either read is
 *  undefined when it failed. */
export function gateFrom(usage: unknown, settings: unknown): Gate {
  const b = (usage as { context?: { breakdown?: { isAutoCompactEnabled?: unknown } } } | undefined)?.context?.breakdown
  if (typeof b?.isAutoCompactEnabled === 'boolean') return b.isAutoCompactEnabled ? 'on' : 'off'
  if (typeof settings !== 'object' || settings === null) return 'unknown'
  return (settings as { autoCompactEnabled?: unknown }).autoCompactEnabled === false ? 'off' : 'on'
}

/** Where the wrap's reply goes, as attend resolved it. */
export type WrapRoute = {
  dest: 'transcript' | 'file' | 'send' | 'off'
  /** The expanded `wrap_file`: `file`'s destination and a refused send's fallback. */
  file: string
  /** `send`'s recipient; `#name` is a channel. */
  to: string
  after: 'lapse' | 'compact'
  handoff: string
}

/** What `attend keepwarm check --policy --json` decided. */
export type Decision = {
  action: 'none' | 'keepalive' | 'wrap'
  text: string | null
  /** When to ask again, in epoch seconds. */
  nextAt: number | null
  count: number
  max: number
  wrapsAt: string | null
  wrappedAt: string | null
  /** Whether the policy runs; false is ADR-182's armed windows alone. */
  policy: boolean
  state: 'running' | 'wrapped' | 'stopped' | 'off'
  reason: string
  wrap: WrapRoute | null
  /** attend could not judge (the transcript was unreadable): ask again. */
  retry: boolean
  /** Why a running policy has nothing due: `under 50k`, `5m tier`. */
  idle: string | null
}

export type WrapOutcome = 'transcript' | 'file' | 'sent' | 'refused' | 'unfinished'

/** The decision; undefined where the output is not one (an attend without
 *  `--policy`, a build without the sensor). */
export function parseDecision(stdout: string): Decision | undefined {
  let d: Record<string, unknown>
  try {
    const doc = JSON.parse(stdout) as unknown
    if (typeof doc !== 'object' || doc === null) return undefined
    d = doc as Record<string, unknown>
  } catch {
    return undefined
  }
  const action = d.action
  if (action !== 'none' && action !== 'keepalive' && action !== 'wrap') return undefined
  const str = (v: unknown) => (typeof v === 'string' ? v : null)
  const num = (v: unknown) => (typeof v === 'number' && Number.isFinite(v) ? v : null)
  const states = ['running', 'wrapped', 'stopped', 'off'] as const
  const state = states.find(x => x === d.state) ?? 'off'
  const w = d.wrap as Record<string, unknown> | null | undefined
  const dests = ['transcript', 'file', 'send', 'off'] as const
  const dest = dests.find(x => x === w?.dest)
  const wrap: WrapRoute | null =
    w && dest && typeof w.file === 'string' && typeof w.to === 'string'
      ? { dest, file: w.file, to: w.to, after: w.after === 'compact' ? 'compact' : 'lapse', handoff: str(w.handoff) ?? 'handoff:' }
      : null
  return {
    action,
    text: str(d.text),
    nextAt: num(d.next_at),
    count: num(d.count) ?? 0,
    max: num(d.max) ?? 0,
    wrapsAt: str(d.wraps_at_hm),
    wrappedAt: str(d.wrapped_at_hm),
    policy: d.policy === true,
    state,
    reason: str(d.reason) ?? '',
    wrap,
    retry: d.retry === true,
    idle: str(d.idle),
  }
}

/** The focus a compaction after the wrap keeps: the wrap's closing
 *  `/compact <focus>` line, else the continuation prompt and the task list. */
export const DEFAULT_COMPACT_FOCUS = 'keep the continuation prompt and the task list'

export function compactFocus(reply: string): string {
  const lines = reply.split('\n').map(l => l.trim().replace(/^`+|`+$/g, ''))
  for (let i = lines.length - 1; i >= 0; i--) {
    const m = /^\/compact\s+(.+)$/.exec(lines[i] ?? '')
    if (m?.[1]) return m[1].trim()
  }
  return DEFAULT_COMPACT_FOCUS
}


/** The row's segment after a decision; null where the policy does not run.
 *  A dormant session shows it from the first turn the policy submits. */
export function keepaliveRowOf(d: Decision, was: KeepaliveRow | null): KeepaliveRow | null {
  if (!d.policy || d.state === 'off') return null
  return {
    state: d.state,
    count: d.count,
    max: d.max,
    nextAt: d.state === 'running' ? d.nextAt : null,
    wrapsAt: d.wrapsAt,
    wrappedAt: d.wrappedAt,
    idle: d.state === 'running' ? d.idle : null,
    isRefused: d.state === 'wrapped' && (was?.isRefused ?? false),
    isShown: (was?.isShown ?? false) || d.action !== 'none',
  }
}

/** `⟳ 2/8 · next 41m · wrap ~21:40`, `⟳ 0/1 · idle: under 50k`,
 *  `⟳ wrapped 21:40`, `⟳ stopped`. */
export function keepaliveSegment(k: KeepaliveRow, nowMs: number): string {
  if (k.state === 'wrapped') return `⟳ wrapped ${k.wrappedAt ?? ''}`.trimEnd() + (k.isRefused ? ' · handoff refused' : '')
  if (k.state === 'stopped') return '⟳ stopped'
  const parts = [`⟳ ${k.count}/${k.max}`]
  if (k.nextAt !== null) parts.push(`next ${Math.max(0, Math.round((k.nextAt * 1000 - nowMs) / 60_000))}m`)
  if (k.wrapsAt !== null) parts.push(`wrap ~${k.wrapsAt}`)
  if (k.idle) parts.push(`idle: ${k.idle}`)
  return parts.join(' · ')
}

/** Runs `f` after every earlier call on `h` settled, one at a time. */
export function serialize<T>(h: { chain: Promise<unknown> }, f: () => Promise<T>): Promise<T> {
  const next = h.chain.then(f, f)
  h.chain = next.catch(() => undefined)
  return next
}

/** A gate read as on or unknown is reused this long; one read as off is
 *  read again before every check and every submit, since a turn may follow. */
export const GATE_KEPT_MS = 5 * 60_000

export function gateIsFresh(gate: Gate, gateAtMs: number, nowMs: number): boolean {
  return gate !== 'off' && nowMs - gateAtMs < GATE_KEPT_MS
}

/** What one decision asks of the mod: a turn to submit, and when to ask
 *  again. A dormant session submits only the policy's own turns, never an
 *  armed window's wake, which stays a listening session's (ADR-406 item
 *  13). A retry is the caller's to bound. */
export type CheckPlan = {
  submit: { kind: 'keepalive' | 'wrap'; text: string; route: WrapRoute | null } | null
  armAtMs: number | null
  retry: boolean
}

export function planCheck(d: Decision, isListening: boolean): CheckPlan {
  let submit: CheckPlan['submit'] = null
  if (d.action === 'keepalive' && d.text && (isListening || d.policy)) submit = { kind: 'keepalive', text: d.text, route: null }
  if (d.action === 'wrap' && d.text && d.wrap && d.policy) submit = { kind: 'wrap', text: d.text, route: d.wrap }
  const armAtMs = !d.retry && d.nextAt !== null && (d.policy || isListening) ? d.nextAt * 1000 : null
  return { submit, armAtMs, retry: d.retry }
}

/** Whether a planned turn may still go in: no prompt or main-loop turn came
 *  since the check began (the epoch held), none runs now, and for the
 *  policy's own turn the gate, read again, is still off (ADR-414 item 2). */
export function maySubmit(o: { epochThen: number; epochNow: number; running: number; isPolicy: boolean; gate: Gate }): boolean {
  return o.epochThen === o.epochNow && o.running === 0 && (!o.isPolicy || o.gate === 'off')
}

/** A dormant session's retries after a check that could not judge, at most
 *  this many per stretch, this far apart; a listening session asks again on
 *  the plain cadence. */
export const RETRY_MAX = 3
export const RETRY_DORMANT_MS = 5 * 60_000
export const RETRY_LISTENING_MS = 60_000

export function retryAtMs(nowMs: number, isListening: boolean, gate: Gate, retries: number): number | null {
  if (isListening) return nowMs + RETRY_LISTENING_MS
  return gate === 'off' && retries < RETRY_MAX ? nowMs + RETRY_DORMANT_MS : null
}

/** The header Claude Code puts above a prompt a plugin submits without
 *  `asUser`, as `turn.start`'s text carries it (Claude Code 2.1.300):
 *  `The <plugin> plugin sent a message:`, then the prompt, then a closing
 *  paragraph. Only this mod's own name, the manifest's `attend`. */
const PLUGIN_FRAME = 'The attend plugin sent a message:'

/** Whether a main-loop turn that started with `turnText` is the turn a
 *  prompt the mod submitted as `submitted` started: the submitted first
 *  line as a whole line, either first (the bare prompt) or right under the
 *  engine's plugin header (the framed form). Never a line inside someone
 *  else's prose. */
export function isSubmittedTurn(turnText: string, submitted: string): boolean {
  const lines = turnText.split('\n').map(l => l.trim())
  const want = submitted.split('\n')[0]?.trim() ?? ''
  if (want === '') return false
  if (lines[0] === want) return true
  return lines[0] === PLUGIN_FRAME && lines[1] === want
}

/** Whether a main-loop turn is the wrap the mod submitted ([`isSubmittedTurn`]
 *  for a `wrap:` prompt). */
export function isWrapTurn(turnText: string, submitted: string): boolean {
  return submitted.startsWith('wrap:') && isSubmittedTurn(turnText, submitted)
}

/** The engine's framing of a prompt plugin `name` submits without
 *  `asUser`, as `turn.start` carries it; for tests and docs. */
export function framedPluginPrompt(name: string, text: string): string {
  return `The ${name} plugin sent a message:\n${text}\n\nThis is how Claude Code surfaces a prompt a plugin submits between turns — it starts this turn in the user's place. Address the message above.`
}

/** What the wrap's reply becomes once its turn ends (ADR-414 item 6). */
export type WrapPlan =
  | { kind: 'unfinished'; detail: string }
  | { kind: 'transcript' }
  | { kind: 'file'; argv: string[] }
  | { kind: 'send'; argv: string[] }

export function wrapPlan(route: WrapRoute, reason: string, reply: string): WrapPlan {
  if (reason !== 'answer') return { kind: 'unfinished', detail: `the turn ended: ${reason}` }
  if (reply.trim() === '') return { kind: 'unfinished', detail: 'the reply was empty' }
  if (route.dest === 'file' && route.file === '') return { kind: 'unfinished', detail: 'no safe wrap file: see attend keepwarm status' }
  if (route.dest === 'file') return { kind: 'file', argv: saveArgv('file', '', route.file) }
  if (route.dest === 'send' && !isOpenChannel(route.to)) return { kind: 'send', argv: wrapSendArgv(route, reply) }
  if (route.dest === 'send') return { kind: 'unfinished', detail: `${route.to} is refused: it reaches every listening session` }
  return { kind: 'transcript' }
}

/** attend writes the wrap's reply, given on stdin, to `path` and records
 *  the outcome: it checks the path again and renames a temp file over it,
 *  so a link planted during the wrap turn is never followed (ADR-414 item
 *  6). The mod never writes the file itself. */
export function saveArgv(outcome: 'file' | 'refused', detail: string, path: string): string[] {
  return [ATTEND, 'keepwarm', 'wrapped', outcome, '--detail', oneLine(detail), '--path', path, '--save']
}

/** The card's detail for a send attend refused or that timed out; attend
 *  adds where the reply went instead. */
export function refusalDetail(to: string, r: { exitCode: number; stderr: string; stdout: string }): string {
  const why = (r.stderr.trim() || r.stdout.trim() || `attend exited ${r.exitCode}`).split('\n')[0] ?? ''
  return `${to}: ${why}`
}

/** The row's segment after the operator's prompt: the count starts again,
 *  and a dormant session stops drawing it. */
export function rowAfterPrompt(k: KeepaliveRow | null): KeepaliveRow | null {
  return k && { ...k, state: 'running', count: 0, wrappedAt: null, isRefused: false, isShown: false }
}

/** After a main-loop turn's end: the next check's time, while it runs. */
export function rowAfterTurnEnd(k: KeepaliveRow | null, atMs: number): KeepaliveRow | null {
  return k?.state === 'running' ? { ...k, nextAt: Math.round(atMs / 1000), idle: null } : k
}

export function rowRefused(k: KeepaliveRow | null): KeepaliveRow | null {
  return k && { ...k, isRefused: true }
}

/** The row's text: an active session's peers, mail, channels and keepalive
 *  segment; a dormant session's keepalive segment alone, from its first
 *  keepalive until the operator's next prompt; otherwise none. */
export function rowText(s: AttendStatus, nowMs: number): string | undefined {
  const keepalive = s.keepalive ? keepaliveSegment(s.keepalive, nowMs) : undefined
  if (s.mode !== 'active') return s.keepalive?.isShown ? keepalive : undefined
  // A message held on a lapsed cache waits for the operator's prompt.
  const mail = s.cacheLapsed && s.unread > 0 ? `${s.unread} waiting · cold` : `${s.unread} unread`
  const state = s.listening === false ? '○ attend not listening' : `● ${s.peers} peers ✉ ${mail}`
  const channels = s.channels.length > 0 ? ` ${s.channels.join(' ')}` : ''
  return `${state}${channels}${keepalive ? ` ${keepalive}` : ''}`
}

// ── parsing ──────────────────────────────────────────────────────────────

/** One drained message, as `attend message inbox --drain --json` gives it. */
export type Listed = {
  /** The drain's scope label: `#open`, `project`, or `@name` for a channel. */
  scope: string
  sender: string
  /** attend's wire identity, `claude:<session>` or `external:<user>[@terminal]`;
   *  empty where absent. Self-reported by the writer, so never proof of who sent it. */
  senderId: string
  id: string
  /** The thread parent's id, where the message is a reply; empty otherwise. */
  re: string
  /** When it was sent, as attend shows it; empty where absent. */
  when: string
  message: string
}

/** What one drain said. `messages` is empty when nothing was delivered: the
 *  cold start's held-back note and the session's name are fields of their
 *  own, never messages. `status` is `drained`, or why nothing was:
 *  `not_listening` (listening was turned off, here or elsewhere) or
 *  `unresolved` (attend cannot tell which session this is). `cacheLapsed`:
 *  a wake drain held a message that would wake the session because its
 *  prompt cache has lapsed (ADR-406 item 2). Undefined where the output is not a
 *  schema-1 document (an attend without `--drain --json`, or one whose
 *  document changed shape). */
export type Drain = {
  status: string
  messages: Listed[]
  heldBack: number
  note: string | null
  youAre: string | null
  cacheLapsed: boolean
}

/** The `--drain --json` schema this mod reads. */
export const DRAIN_SCHEMA = 1

export function parseDrain(stdout: string): Drain | undefined {
  let doc: unknown
  try {
    doc = JSON.parse(stdout)
  } catch {
    return undefined
  }
  if (typeof doc !== 'object' || doc === null) return undefined
  const d = doc as Record<string, unknown>
  if (d.schema !== DRAIN_SCHEMA || typeof d.status !== 'string' || !Array.isArray(d.messages)) return undefined
  const str = (v: unknown) => (typeof v === 'string' ? v : '')
  const messages = d.messages.flatMap((m): Listed[] => {
    if (typeof m !== 'object' || m === null) return []
    const x = m as Record<string, unknown>
    if (typeof x.id !== 'string' || typeof x.message !== 'string') return []
    return [{ scope: str(x.scope), sender: str(x.sender), senderId: str(x.sender_id), id: x.id, re: str(x.re), when: str(x.when), message: x.message }]
  })
  return {
    status: d.status,
    messages,
    heldBack: typeof d.held_back === 'number' ? d.held_back : 0,
    note: typeof d.note === 'string' ? d.note : null,
    youAre: typeof d.you_are === 'string' ? d.you_are : null,
    cacheLapsed: d.cache_lapsed === true,
  }
}

/** The peer count from `attend peer list`: its footer `N agent(s)` less this
 *  session's own row. */
export function parsePeerCount(stdout: string): number {
  const m = /(\d+) agent\(s\)/.exec(stdout)
  return m ? Math.max(0, Number(m[1]) - 1) : 0
}

/** `attend message unread --json`: the count and the channels this session receives.
 *  Undefined where the output is not that shape (an attend without the verb). */
export function parseUnread(stdout: string): { unread: number; channels: string[] } | undefined {
  try {
    const doc = JSON.parse(stdout) as { unread?: unknown; rooms?: { label?: unknown; kind?: unknown }[] }
    if (typeof doc.unread !== 'number') return undefined
    const channels = (doc.rooms ?? [])
      .filter(r => r.kind === 'channel' && typeof r.label === 'string')
      .map(r => displayScope(String(r.label)))
    return { unread: doc.unread, channels }
  } catch {
    return undefined
  }
}

/** The `keepwarm:` wake line `attend keepwarm check` printed, if any. */
export function keepwarmLine(stdout: string): string | undefined {
  const line = stdout.split('\n').map(l => l.trim()).find(l => l.startsWith('keepwarm:'))
  return line && line.length > 'keepwarm:'.length ? line : undefined
}

// ── the view ─────────────────────────────────────────────────────────────

/** Short stable handle for a message id; `attend_reply`'s `re` takes it.
 *  attend computes the same handle (`short_id` in tools/attend/src/verbs/
 *  inbox.rs) for the lines its Stop-hook drain prints in a mod host, and
 *  `attend message reply --re` resolves one this module never mapped. */
export function shortId(id: string): string {
  let h = 0x811c9dc5
  for (let i = 0; i < id.length; i++) h = Math.imul(h ^ id.charCodeAt(i), 0x01000193) >>> 0
  return h.toString(36).padStart(5, '0').slice(-5)
}

/** A channel's drain label `@name` reads `#name` in the view. */
export function displayScope(scope: string): string {
  return scope.startsWith('@') ? `#${scope.slice(1)}` : scope
}

/** A field as one line: every run of whitespace, control or format
 *  characters (newlines, CR, NUL, U+2028, zero-width) becomes one space. Each
 *  field a store writer controls passes through it before it reaches the
 *  prompt, so no field can start a line of its own. */
export function oneLine(s: string): string {
  return s.replace(/[\s\p{Cc}\p{Cf}\p{Zl}\p{Zp}]+/gu, ' ').trim()
}

/** `[#channel] Name (project) HH:MM: text ~id`, one line, ` (re ~parent)`
 *  added where attend says the message is a reply. Id, sender id and cwd are
 *  dropped, and the line makes no claim about where the message came from
 *  (ADR-406 item 4); `~id` is a short handle for `attend_reply`'s `re`. */
export function formatMessage(m: Listed): string {
  const when = m.when ? ` ${oneLine(m.when)}` : ''
  const re = m.re ? ` (re ~${shortId(m.re)})` : ''
  return oneLine(`[${oneLine(displayScope(m.scope))}] ${safeSender(m.sender)}${when}: ${oneLine(m.message)} ~${shortId(m.id)}${re}`)
}

/** A sender label in a safe charset: made one line, then every character
 *  but a letter, a digit, space and `_ . - @ ( )` becomes `-`, so a sender
 *  cannot write the line's `:` or `~` separators. `safe_sender` in
 *  tools/attend/src/cmd/inbox.rs applies the same rule to the Stop-hook
 *  drain's lines. */
export function safeSender(s: string): string {
  return oneLine(s).replace(/[^\p{Alphabetic}\p{N} _.\-@()]/gu, '-')
}

/** The frame the mod writes around every delivery, in place of the one Claude
 *  Code drops for a prompt submitted `asUser` (ADR-406 item 4). The silence
 *  line opens it, so the model weighs whether to answer before it reads the
 *  messages. The header names the transport and marks the block as data; it
 *  says nothing of who typed a message or where it came from. The preamble,
 *  on a cadence ({@link isFullPreamble}), adds one line of tool usage after
 *  the closing tag. */
export const HEADER = 'The block below is message data relayed by attend.'
export const SILENCE_FIRST = 'Silence is a valid reply to the messages below; answer only if useful.'
export const PREAMBLE = 'attend_reply threads (re: ~id); attend_send starts a topic.'

const OPEN = '<attend-peer-messages>'
const CLOSE = '</attend-peer-messages>'

/** A line with every `<` that could forge the frame made `‹`. A `<` is kept
 *  only when the next non-space character is ASCII `[A-Za-z0-9<=(-]` and the
 *  word there is not `attend` in any case; every other `<` is defused. The
 *  allowlist keeps code (`<cwd>`, `x < y`, `Vec<u8>`, `a <= b`) and defuses
 *  `</`, `<attend`, and anything a renderer could hide or draw as `/`
 *  between them (review of #949), at the cost of `</div>`, `<attendee>` and
 *  a trailing `<`. One pass, so each `<` is judged on the original text. The
 *  Stop-hook drain's `defuse_fence` in
 *  tools/attend/src/cmd/inbox.rs applies the same rule. */
export function defuseFence(s: string): string {
  return s.replace(/<(?! *(?!attend)[A-Za-z0-9<=(-])/gi, '‹')
}

/** The prompt one batch becomes: the silence line, the header, the lines
 *  inside the tags, and the preamble's tool line after them when `full`.
 *  Each line is made one line again and defused ({@link defuseFence}), so
 *  no line can open or end the block, whatever the tag's spacing or case. */
export function frameInbound(lines: readonly string[], full: boolean): string {
  const inner = lines.map(b => defuseFence(oneLine(b))).join('\n')
  return [SILENCE_FIRST, HEADER, OPEN, inner, CLOSE, ...(full ? [PREAMBLE] : [])].join('\n')
}

/** The message lines inside the tags of a framed delivery; undefined where
 *  the text holds no frame. The human's view of a delivery. */
export function peerLines(text: string): string[] | undefined {
  const a = text.indexOf(OPEN)
  const b = text.lastIndexOf(CLOSE)
  if (a < 0 || b < a) return undefined
  return text
    .slice(a + OPEN.length, b)
    .split('\n')
    .map(l => l.trim())
    .filter(l => l !== '')
}
