/** The keepalive policy as the row draws it (ADR-414 item 9). */
export type KeepaliveRow = {
  state: 'running' | 'wrapped' | 'stopped'
  count: number
  max: number
  /** Epoch seconds. */
  nextAt: number | null
  wrapsAt: string | null
  wrappedAt: string | null
  /** Why a running policy has nothing due, from attend's decision. */
  idle: string | null
  /** A wrap sent over attend was refused and went to the file instead. */
  isRefused: boolean
  /** A dormant session draws the segment from its first keepalive until
   *  the operator's next prompt. */
  isShown: boolean
}

export type AttendStatus = {
  /** Dormant: plain Claude Code, nothing polled or drawn. Active: listening
   *  until Claude Code exits (ADR-406 item 2). */
  mode: 'dormant' | 'active'
  /** Whether attend has this session listening; false when `attend listen on`
   *  failed, null before it ran. */
  listening: boolean | null
  peers: number
  /** Messages waiting for this session (`attend message unread`), or drained and not
   *  yet delivered where attend has no unread count. */
  unread: number
  /** The channels this session receives or joined, as `#name`. */
  channels: string[]
  /** Deliveries made so far; the preamble line shows on delivery 0. */
  deliveries: number
  /** A compaction happened since the last delivery: the next is full. */
  afterCompaction: boolean
  /** Turns since the preamble line last showed. */
  turnsSinceFull: number
  /** The cold start's held-back note a poll's drain returned, waiting to
   *  ride the operator's next prompt; null when none waits. Kept here so a
   *  reloaded module still carries it. */
  heldNote: string | null
  /** A message that would wake the session waits because the prompt cache
   *  has lapsed: the session is not woken, and what waits rides the
   *  operator's next prompt (ADR-406 item 2). */
  cacheLapsed: boolean
  /** The keepalive policy's segment of the row (ADR-414 item 9); null
   *  where the policy does not run. */
  keepalive: KeepaliveRow | null
}

declare module 'claude-code' {
  interface PluginState {
    attend: { status: AttendStatus }
  }
}
