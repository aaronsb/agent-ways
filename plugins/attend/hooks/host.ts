// SPDX-FileCopyrightText: 2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

// The session's sensor host as the mod runs it: the argv, the respawn cap,
// how a host ended, and its stdout cues. Plain data and the rules over it,
// no `$`: register.tsx spawns and reads the host, since the engine follows
// `$` only into functions declared in the hooks module's own file.
//
// References:
// - ADR-416 item 2: one host per listening session, its respawn cap, and
//   the stdout line that cues a poll.

import type { HostRecord, MoveRecord } from '../types/index.d.ts'
import { ATTEND, oneLine } from './lib.ts'

/** Which host a session runs: its id, and the id a /clear ended, whose
 *  sensor stores the host moves to it. */
export type HostSpec = { session: string; movedFrom: string | undefined }

/** The spec as $.state keeps it. */
export function hostRecord(h: HostSpec | undefined): HostRecord | null {
  return h === undefined ? null : { session: h.session, movedFrom: h.movedFrom ?? null }
}

/** The spec a kept record holds; undefined where it holds none. */
export function hostOfRecord(r: unknown): HostSpec | undefined {
  if (typeof r !== 'object' || r === null) return undefined
  const x = r as Record<string, unknown>
  if (typeof x.session !== 'string' || x.session === '') return undefined
  return { session: x.session, movedFrom: typeof x.movedFrom === 'string' ? x.movedFrom : undefined }
}

/** The host the session wants under `id`: the one the module already wants
 *  for it, else the one $.state kept for it (a reload), else a new one. A
 *  new one after a /clear carries the cleared id as `movedFrom`, so the
 *  first host under the new id moves the stores: attend keeps a store the
 *  new id already has, so a later move would not replace what that host
 *  wrote. `move` is the /clear or /resume being followed or settled.
 *
 *  After /clear steps alone the cleared id is the one the last host ran
 *  for, the first that ended. A move with a /resume among its steps ended
 *  ids of other conversations: it hands on the id its last /clear ended,
 *  where its last step was a /clear, and none after a /resume. */
export function hostSpecFor(id: string, current: HostSpec | undefined, kept: HostSpec | undefined, move: MoveRecord | undefined): HostSpec {
  if (current?.session === id) return current
  if (kept?.session === id) return kept
  if (move === undefined || move.reason !== 'clear' || move.ended.includes(id)) return { session: id, movedFrom: undefined }
  if (move.resumed) return { session: id, movedFrom: move.ended[move.ended.length - 1] }
  const from = current?.session ?? kept?.session
  return { session: id, movedFrom: from !== undefined && move.ended.includes(from) ? from : undefined }
}

/** A running host: what it was spawned for, and how to end it. `isEnding`
 *  is set once the module ends it, so its end is not taken for an exit.
 *  `exited` settles once its child has gone, or once it is known no child
 *  was spawned. */
export type HostChild = HostSpec & { isEnding: boolean; stop: () => void; exited: Promise<void> }

/** The longest a new host waits for the one it replaces to exit. A host
 *  looks at its stop flag after a sleep of up to a second, so it lingers
 *  after SIGTERM; past this bound the new one spawns anyway, so a host that
 *  will not exit cannot hold the session's sensors off. */
export const HOST_EXIT_WAIT_MS = 3_000

/** A host that exits on its own is spawned again after HOST_RESPAWN_MS, at
 *  most HOST_RESPAWNS times in HOST_WINDOW_MS; the next exit stops it. */
export const HOST_RESPAWNS = 3
export const HOST_WINDOW_MS = 10 * 60_000
export const HOST_RESPAWN_MS = 5_000

/** After an exit at `nowMs`: the respawns still inside the window, this one
 *  added when the cap allows it, and whether to respawn. */
export function respawnAfterExit(respawns: readonly number[], nowMs: number): { respawns: number[]; respawn: boolean } {
  const kept = respawns.filter(t => nowMs - t < HOST_WINDOW_MS)
  if (kept.length >= HOST_RESPAWNS) return { respawns: kept, respawn: false }
  return { respawns: [...kept, nowMs], respawn: true }
}

/** How a host ended: `$.process.spawn`'s `{ code, signal }`, a `{ deny }`
 *  where a policy refused the spawn, or why it could not start. */
export type HostEnd = { code?: number | null; signal?: string | null; deny?: unknown } | undefined

/** Whether the end says another spawn gets the same answer: attend's usage
 *  code 2 (an attend without `sensor host`, an argument it refuses) or a
 *  refused spawn. */
export function isHostRefused(end: HostEnd): boolean {
  return end !== undefined && (end.deny !== undefined || end.code === 2)
}

/** The end in a few words, for the log. */
export function hostEndText(end: HostEnd, failed?: string): string {
  if (failed !== undefined) return `could not start: ${oneLine(failed)}`
  if (end?.deny !== undefined) return `refused: ${oneLine(String(end.deny))}`
  if (typeof end?.signal === 'string') return end.signal
  return `exit ${end?.code ?? '?'}`
}

/** The host's stdout read so far: `held` is a line not yet ended. Each
 *  complete line is one cue, a signal the host wrote; the rest is held. A
 *  held part past 4096 characters keeps its end. */
export function cuesIn(held: string, text: string): { held: string; cues: number } {
  const lines = (held + text).split('\n')
  const rest = lines.pop() ?? ''
  return { held: rest.slice(-4096), cues: lines.filter(l => l.trim() !== '').length }
}

/** Claude Code's pid from the command-hook environment (CLAUDE_PID), or
 *  undefined where it is absent or not a pid. */
export function claudePid(env: Readonly<Record<string, string>> | undefined): string | undefined {
  const pid = env?.CLAUDE_PID
  return pid !== undefined && /^[1-9][0-9]*$/.test(pid) ? pid : undefined
}

/** The session's sensor host: `attend sensor host
 *  --session <id> --cwd <dir>`, with `--moved-from` the id a /clear ended,
 *  whose sensor stores move to `session`, and `--parent` Claude Code's pid,
 *  which the host watches so that it exits once Claude Code is gone, even
 *  when it went before the host started. argv[0] is attend itself: attend
 *  refuses the session's verbs to anything a host runs, and the host is
 *  spawned directly, never through a wrapper. */
export function hostArgv(h: HostSpec, cwd: string, parent?: string): string[] {
  const argv = [ATTEND, 'sensor', 'host', '--session', h.session, '--cwd', cwd]
  if (h.movedFrom !== undefined && h.movedFrom !== '' && h.movedFrom !== h.session) argv.push('--moved-from', h.movedFrom)
  if (parent !== undefined) argv.push('--parent', parent)
  return argv
}
