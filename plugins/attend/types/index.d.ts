// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

/** The keepalive policy as the row draws it (ADR-414 item 9). */
export type KeepaliveRow = {
  /** `armed` and `finished` are a stop exit's (ADR-418 item 9). */
  state: 'running' | 'wrapped' | 'compacted' | 'resumed' | 'stopped' | 'armed' | 'finished'
  count: number
  max: number
  /** Keepalives since the wrap, of `afterMax` (`after_wrap_keepalives`). */
  afterCount: number
  afterMax: number
  /** When the compaction after the wrap goes out, as attend estimates it. */
  compactsAt: string | null
  compactedAt: string | null
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
  /** Relays since the operator's last prompt, of `maxRelays`. */
  relayCount: number
  maxRelays: number
  /** Why the relay stopped, in a few words; null while it runs. */
  relayStop: string | null
  /** This generation's drain has started; absent in a row a module from
   *  before the drain kept. */
  isDraining?: boolean
}

/** The sensor host a session wants: its id, and the id a /clear ended,
 *  whose sensor stores the host moves to it; null where none moves. */
export type HostRecord = {
  session: string
  movedFrom: string | null
}

/** Why the session ended under a running process (session.end's reason). */
export type MoveReason = 'clear' | 'resume'

/** A /clear or an in-session /resume whose new session id the mod has not
 *  read yet (hooks/move.ts). */
export type MoveRecord = {
  /** Every id that ended during the move; none of them is the one that
   *  goes on. */
  ended: string[]
  reason: MoveReason
  /** The session was listening when it ended. */
  wasOn: boolean
  /** A /resume was among the steps: the ids that ended are then not one
   *  conversation, and only the id the last /clear ended hands its sensor
   *  stores on. */
  resumed: boolean
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
  /** A /clear or a /resume the mod is still following, kept so a reloaded
   *  module takes the watch up; null when none is. */
  move: MoveRecord | null
  /** The session's sensor host exited past its respawn cap, or attend
   *  refused it: the row says so, and messages still arrive. */
  hostStopped: boolean
  /** The host the session wants, kept so a reloaded module spawns the
   *  same one, --moved-from included; null before activation. */
  hostFor: HostRecord | null
}

declare module 'claude-code' {
  interface PluginState {
    attend: { status: AttendStatus }
  }
}
