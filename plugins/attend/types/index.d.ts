export type AttendStatus = {
  /** Dormant: plain Claude Code, nothing polled or drawn. Active: listening
   *  until Claude Code exits (ADR-406 item 2). */
  mode: 'dormant' | 'active'
  /** Whether attend has this session listening; false when `attend listen on`
   *  failed, null before it ran. */
  listening: boolean | null
  peers: number
  /** Messages waiting for this session (`attend unread`), or drained and not
   *  yet delivered where attend has no unread count. */
  unread: number
  /** The channels this session receives or joined, as `#name`. */
  channels: string[]
  /** Deliveries made so far; the full preamble shows on delivery 0. */
  deliveries: number
  /** A compaction happened since the last delivery: the next is full. */
  afterCompaction: boolean
  /** Turns since the full preamble last showed. */
  turnsSinceFull: number
}

declare module 'claude-code' {
  interface PluginState {
    attend: { status: AttendStatus }
  }
}
