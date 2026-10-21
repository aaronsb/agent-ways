// The state of a /clear or an in-session /resume the mod is following to the
// session id that goes on (ADR-406, ADR-411). Plain data and the rules over
// it, no `$`: register.tsx drives the watch, since the engine follows `$`
// only into functions declared in the hooks module's own file.
//
// session.end, with reason `clear` or `resume`, says the conversation ended
// and the process goes on under another session id. That id shows in
// $.session.id() only once Claude Code's settings SessionStart hooks have
// run, which can take seconds and, on a host whose organization adds its own
// hooks, much longer. The watch reads the id on a backoff and never takes an
// id that ended during the move as the one that goes on. After a give-up the
// ended ids are kept, and a later main-loop turn under another id settles
// the move then.

import type { Timer } from 'claude-code'

import type { MoveReason, MoveRecord } from '../types/index.d.ts'

/** The first read waits this long after the hold, the waits double to
 *  MOVE_MAX_MS, and after MOVE_GIVE_UP_MS of waiting the watch stops. */
export const MOVE_FIRST_MS = 250
export const MOVE_MAX_MS = 2_000
export const MOVE_GIVE_UP_MS = 5 * 60_000

/** A move in progress: the record, and the watch's own clock. */
export type Move = MoveRecord & {
  /** Waited so far, and the next wait. */
  waitedMs: number
  delayMs: number
  timer: Timer | undefined
  /** Reads of the id that failed; the first is logged. */
  failures: number
}

/** Where the module keeps its one move, and the record of one the watch
 *  gave up on, whose ids a later turn still holds apart. */
export type MoveSlot = { move: Move | undefined; gaveUp: MoveRecord | undefined }

/** What a read of the id leads to (readOutcome). */
export type Outcome =
  | { kind: 'settle'; record: MoveRecord }
  | { kind: 'wait'; delayMs: number }
  | { kind: 'give-up'; record: MoveRecord }
  | { kind: 'gone' }

export function recordOf(m: Move): MoveRecord {
  return { ended: [...m.ended], reason: m.reason, wasOn: m.wasOn }
}

function fresh(record: MoveRecord): Move {
  return { ...record, ended: [...record.ended], waitedMs: 0, delayMs: MOVE_FIRST_MS, timer: undefined, failures: 0 }
}

/** Records a /clear or a /resume of `endedId`: plain data, so session.end
 *  can call it inside its bound. A move already in progress (a second /clear
 *  before the first id showed) takes the new ended id, stops its timer and
 *  starts its waits again. A move given up on lends its ended ids. */
export function recordMove(slot: MoveSlot, endedId: string, reason: MoveReason, wasOn: boolean) {
  const m = slot.move
  if (!m) {
    const g = slot.gaveUp
    slot.gaveUp = undefined
    const ended = (g?.ended ?? []).filter(id => id !== endedId)
    slot.move = fresh({ ended: [...ended, endedId], reason, wasOn: wasOn || (g?.wasOn ?? false) })
    return
  }
  m.timer?.cancel()
  m.timer = undefined
  if (!m.ended.includes(endedId)) m.ended.push(endedId)
  m.reason = reason
  m.wasOn = m.wasOn || wasOn
  m.waitedMs = 0
  m.delayMs = MOVE_FIRST_MS
}

/** Takes up a move a reload found in $.state. */
export function adoptMove(slot: MoveSlot, record: MoveRecord) {
  slot.move?.timer?.cancel()
  slot.move = fresh(record)
}

/** Ends move `m` under `id` when it is still the slot's and `id` did not
 *  end; the slot is cleared at once, so of the watch and a turn racing for
 *  it, one settles. Returns the record to settle, or undefined. */
export function claimMove(slot: MoveSlot, m: Move, id: string): MoveRecord | undefined {
  if (slot.move !== m || m.ended.includes(id)) return undefined
  slot.move = undefined
  m.timer?.cancel()
  m.timer = undefined
  return recordOf(m)
}

/** Ends a move the watch gave up on, when `id` did not end. */
export function claimGaveUp(slot: MoveSlot, id: string): MoveRecord | undefined {
  const g = slot.gaveUp
  if (!g || slot.move || g.ended.includes(id)) return undefined
  slot.gaveUp = undefined
  return g
}

/** What one read of the id leads to: `id` undefined is a read that failed,
 *  which counts as no change. A wait doubles up to MOVE_MAX_MS; past
 *  MOVE_GIVE_UP_MS the move goes to `gaveUp`. */
export function readOutcome(slot: MoveSlot, m: Move, id: string | undefined): Outcome {
  if (slot.move !== m) return { kind: 'gone' }
  if (id !== undefined) {
    const record = claimMove(slot, m, id)
    if (record) return { kind: 'settle', record }
  }
  m.waitedMs += m.delayMs
  if (m.waitedMs >= MOVE_GIVE_UP_MS) {
    slot.move = undefined
    slot.gaveUp = recordOf(m)
    return { kind: 'give-up', record: slot.gaveUp }
  }
  m.delayMs = Math.min(m.delayMs * 2, MOVE_MAX_MS)
  return { kind: 'wait', delayMs: m.delayMs }
}
