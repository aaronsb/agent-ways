export type AttendModStatus = {
  /** Dormant: no line, no wakes, one cheap check per period. Awake: the
   *  inbound conduit runs and the status line shows. */
  mode: 'dormant' | 'awake'
  /** This session's display name (`attend whoami --display`). */
  self: string | null
  /** The basename of this session's origin path. */
  project: string | null
  /** Whether the drain delivers here; null before the first awake drain. */
  enrolled: boolean | null
  peers: number
  /** Messages drained and waiting for their turn to start. */
  unread: number
  /** Channels this session joined or has seen messages on, for the band. */
  channels: string[]
  /** Deliveries made so far; the full preamble shows on 0 and every 10th. */
  deliveries: number
  /** A compaction happened since the last delivery: the next shows the full preamble. */
  afterCompaction: boolean
}

declare module 'claude-code' {
  interface PluginState {
    'attend-mod': { status: AttendModStatus }
  }
}
