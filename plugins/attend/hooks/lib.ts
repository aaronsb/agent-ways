// Pure pieces of the attend mod (ADR-406): argv building, parsing what the
// attend and ways CLIs print, framing a delivery, and the timing rules. No `$`
// here, so the tests exercise them directly.

export const ATTEND = 'attend'
export const WAYS = 'ways'

// ── argv ─────────────────────────────────────────────────────────────────
// Every call is an argv array with no shell. A body is one element after
// `--`, so no quoting rule applies to it and a body that starts with a dash
// stays a body. attend sends a long body as one message.

/** `attend send [--channel C] [--to T] -- <message>`: `to` is a session's
 *  name, a person's member id or a project path, and makes it a direct
 *  message; with neither it posts to #open, as `channel` open does. */
export function sendArgv(message: string, channel?: string, to?: string): string[] {
  const argv = [ATTEND, 'send']
  if (channel) argv.push('--channel', bareChannel(channel))
  if (to) argv.push('--to', to)
  argv.push('--', message)
  return argv
}

/** `attend reply [--re <id>] -- <message>`: threads to the newest message
 *  delivered, or to `re`, and goes to that message's sender as a direct
 *  message, or into its channel when it was said in a named channel. attend
 *  refuses a parent whose channel this session left, whose sender session
 *  has exited, or that it cannot find. */
export function replyArgv(message: string, re?: string): string[] {
  return re ? [ATTEND, 'reply', '--re', re, '--', message] : [ATTEND, 'reply', '--', message]
}

export function joinArgv(channel: string): string[] {
  return [ATTEND, 'join', bareChannel(channel)]
}

function bareChannel(channel: string): string {
  return channel.trim().replace(/^[#@]/, '')
}

/** `listen on` names the mod as the host, so attend's Stop-hook drain speaks
 *  the mod's format and tools in this session. An attend older than
 *  `--host` refuses the flag as a usage error (exit 2); the mod then turns
 *  listening on without it. */
export const LISTEN_ON_ARGV = [ATTEND, 'listen', 'on', '--host', 'mod'] as const
export const LISTEN_ON_PLAIN_ARGV = [ATTEND, 'listen', 'on'] as const
export const LISTEN_OFF_ARGV = [ATTEND, 'listen', 'off'] as const
export const DRAIN_ARGV = [ATTEND, 'inbox', '--drain', '--json'] as const
/** The idle poll's drain: it takes nothing unless something pending is
 *  addressed to this session, sent to its project, or a reply to a message
 *  it sent (ADR-406 item 1). An attend older than `--wake` refuses the flag
 *  as a usage error ([`isUnknownWake`]); the mod then drains without it for
 *  the rest of the session. */
export const WAKE_DRAIN_ARGV = [ATTEND, 'inbox', '--drain', '--json', '--wake'] as const

/** Whether attend refused the wake drain's `--wake` as an argument it does
 *  not know, the usage error (exit 2) of an attend older than the flag.
 *  Any other failure is not taken for an old attend. */
export function isUnknownWake(r: { exitCode: number; stderr: string }): boolean {
  return r.exitCode === 2 && r.stderr.includes('--wake')
}
export const UNREAD_ARGV = [ATTEND, 'unread', '--json'] as const
export const PEERS_ARGV = [ATTEND, 'peers'] as const
export const KEEPWARM_CHECK_ARGV = [ATTEND, 'keepwarm', 'check'] as const

/** The resolved `attend_mod` section, through the ways config layers: the
 *  user file, the target's file, then the project's `.claude/ways.yaml`. */
export function configArgv(project: string): string[] {
  return [WAYS, 'settings', 'list', 'attend_mod', '--json', '--effective', '--project', project]
}

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
}

export const DEFAULT_CONFIG: ModConfig = {
  listening: 'dormant',
  keepwarm: 'on',
  install: true,
  pollSeconds: 5,
  slowPollSeconds: 60,
  quietMinutes: 20,
  preambleEveryTurns: 20,
  headlessPoll: false,
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

// ── parsing ──────────────────────────────────────────────────────────────

/** One drained message, as `attend inbox --drain --json` gives it. */
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
 *  `unresolved` (attend cannot tell which session this is). Undefined where
 *  the output is not a schema-1 document (an attend without `--drain
 *  --json`, or one whose document changed shape). */
export type Drain = {
  status: string
  messages: Listed[]
  heldBack: number
  note: string | null
  youAre: string | null
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
  }
}

/** The peer count from `attend peers`: its footer `N agent(s)` less this
 *  session's own row. */
export function parsePeerCount(stdout: string): number {
  const m = /(\d+) agent\(s\)/.exec(stdout)
  return m ? Math.max(0, Number(m[1]) - 1) : 0
}

/** `attend unread --json`: the count and the channels this session receives.
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
 *  `attend reply --re` resolves one this module never mapped. */
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
 *  Code drops for a prompt submitted `asUser` (ADR-406 item 4). The header
 *  names the transport and marks the block as data; it says nothing of who
 *  typed a message or where it came from. The preamble, on a cadence
 *  ({@link isFullPreamble}), adds one line of tool usage. */
export const HEADER = 'The block below is message data relayed by attend.'
export const PREAMBLE = 'attend_reply threads (re: ~id); attend_send starts a topic.'
export const REPLY_OPTIONAL = 'Reply if useful; no reply is fine.'

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

/** The prompt one batch becomes: the header, the lines inside the tags,
 *  the preamble's tool line when `full`, and the reply-optional line last,
 *  where Claude Code's own frame put its imperative. Each line is made one
 *  line again and defused ({@link defuseFence}), so no line can open or end
 *  the block, whatever the tag's spacing or case. */
export function frameInbound(lines: readonly string[], full: boolean): string {
  const inner = lines.map(b => defuseFence(oneLine(b))).join('\n')
  return [HEADER, OPEN, inner, CLOSE, ...(full ? [PREAMBLE] : []), REPLY_OPTIONAL].join('\n')
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
