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
  /** The config started this session listening and the operator has not
   *  prompted yet: no turn is submitted, and what is pending rides the first
   *  prompt. Kept here so a reloaded module keeps holding. */
  awaitingOperator: boolean
  /** A message that would wake the session waits because the prompt cache
   *  has lapsed: the session is not woken, and what waits rides the
   *  operator's next prompt (ADR-406 item 2). */
  cacheLapsed: boolean
}

declare module 'claude-code' {
  interface PluginState {
    attend: { status: AttendStatus }
  }
}
