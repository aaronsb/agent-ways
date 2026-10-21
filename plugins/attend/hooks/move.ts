// The watch that carries a session's listening across a /clear or an
// in-session /resume (ADR-406, ADR-411).
//
// session.end, with reason `clear` or `resume`, says the conversation
// ended and the process goes on under another session id. That id shows
// in $.session.id() only once Claude Code's settings SessionStart hooks
// have run, which can take seconds and, on a host whose organization adds
// its own hooks, much longer. The watch reads the id on a backoff and never
// takes an id that ended during the move as the one that goes on.
//
// session.end's whole chain shares one short bound, at which a `$` call in
// flight is aborted, a timer's wait among them. So session.end makes no call
// on its own `$`: it records the move (recordMove, plain data) and starts the
// watch on the `$` of a hook whose timers outlive that bound, session.start's.
//
// The watch first writes the hold at once, so a module reloaded right after
// the /clear finds the move, then reads the id on the backoff. After a give-up
// the ended ids are kept, and a later main-loop turn that reads another id
// settles the move then.

import type { EngineInterface, Timer } from 'claude-code'

import type { MoveReason, MoveRecord } from '../types/index.d.ts'

type $ = EngineInterface

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

/** What a move does in the module, bound by the caller to the `$` it
 *  passes the watch. */
export type MoveActions = {
  /** Called before each read of the id: keeps the move where a reload
   *  finds it, and, while the session was listening, the mode active.
   *  Claude Code empties $.state after session.end, so each step checks. */
  hold: (m: MoveRecord) => Promise<void>
  /** The session goes on under `id`, an id that did not end. */
  settle: (id: string, m: MoveRecord) => Promise<void>
  /** The move is over: settled or given up. */
  release: () => Promise<void>
}

function recordOf(m: Move): MoveRecord {
  return { ended: [...m.ended], reason: m.reason, wasOn: m.wasOn }
}

function fresh(record: MoveRecord): Move {
  return { ...record, ended: [...record.ended], waitedMs: 0, delayMs: MOVE_FIRST_MS, timer: undefined, failures: 0 }
}

/** Records a /clear or a /resume of `endedId`, with no `$` call: it runs
 *  inside session.end's bound. A move already in progress (a second /clear
 *  before the first id showed) takes the new ended id, stops its timer and
 *  starts its waits again; watchMove then runs one timer for it. */
export function recordMove(slot: MoveSlot, endedId: string, reason: MoveReason, wasOn: boolean) {
  const m = slot.move
  if (!m) {
    // The ids a move given up on still ended.
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

/** Takes up a move a reload found in $.state; watchMove starts it. */
export function adoptMove(slot: MoveSlot, record: MoveRecord) {
  slot.move?.timer?.cancel()
  slot.move = fresh(record)
}

/** Starts the watch of the slot's move on `$`, one timer at a time: the
 *  hold at once, then the reads. */
export function watchMove($: $, slot: MoveSlot, act: MoveActions) {
  const m = slot.move
  if (!m) return
  m.timer?.cancel()
  m.timer = $.clock.after(0, () => void holdFirst($, slot, act, m))
}

/** Writes the hold before the first read, so a reload finds the move
 *  without waiting for the backoff's first step, then starts the backoff. */
async function holdFirst($: $, slot: MoveSlot, act: MoveActions, m: Move): Promise<void> {
  if (slot.move !== m) return
  try {
    await act.hold(recordOf(m))
  } catch {
    // Each read writes the hold again.
  }
  if (slot.move !== m) return
  m.timer = $.clock.after(m.delayMs, () => void stepMove($, slot, act, m))
}

/** Settles the move when `$.session.id()` names an id that did not end;
 *  otherwise leaves the watch running. A main-loop turn calls it, so a turn
 *  under the new id does not wait for the next read, and a move the watch
 *  gave up on is settled by the first turn under another id. */
export async function settleIfMoved($: $, slot: MoveSlot, act: MoveActions, log: (text: string) => void): Promise<void> {
  const m = slot.move
  const g = slot.gaveUp
  if (!m && !g) return
  try {
    const id = await $.session.id()
    if (m) {
      if (slot.move !== m || m.ended.includes(id)) return
      await take(slot, act, m, id)
      return
    }
    if (slot.move || slot.gaveUp !== g || !g || g.ended.includes(id)) return
    slot.gaveUp = undefined
    await act.settle(id, g)
  } catch (err) {
    log(`attend: could not read the session id after a /clear or a /resume: ${String(err)}`)
  }
}

/** Ends the move under `id`. The slot is cleared before any await, so of
 *  the watch and a turn racing for it, one settles. */
async function take(slot: MoveSlot, act: MoveActions, m: Move, id: string) {
  slot.move = undefined
  m.timer?.cancel()
  try {
    await act.settle(id, recordOf(m))
  } finally {
    await act.release()
  }
}

/** One read of the id: settle on an id that did not end, otherwise wait
 *  longer. A read that fails counts as no change; past MOVE_GIVE_UP_MS the
 *  watch stops, the session stays as `hold` left it, and the move's record
 *  waits in `gaveUp` for a turn. */
export async function stepMove($: $, slot: MoveSlot, act: MoveActions, m: Move): Promise<void> {
  if (slot.move !== m) return
  m.timer = undefined
  try {
    await act.hold(recordOf(m))
    const id = await $.session.id()
    if (slot.move !== m) return
    if (!m.ended.includes(id)) return void (await take(slot, act, m, id))
  } catch (err) {
    m.failures += 1
    if (m.failures === 1) $.ui.log(`attend: could not read the session id after a /clear or a /resume: ${String(err)}`)
  }
  if (slot.move !== m) return
  m.waitedMs += m.delayMs
  if (m.waitedMs >= MOVE_GIVE_UP_MS) {
    slot.move = undefined
    slot.gaveUp = recordOf(m)
    const last = m.ended[m.ended.length - 1] ?? ''
    $.ui.log(`attend: the session id still reads ${last} ${Math.round(m.waitedMs / 1000)} s after a /${m.reason}; the next turn under another id records it`)
    try {
      await act.release()
    } catch {
      // The record in $.state is dropped by the next settle, or the process's end.
    }
    return
  }
  m.delayMs = Math.min(m.delayMs * 2, MOVE_MAX_MS)
  m.timer = $.clock.after(m.delayMs, () => void stepMove($, slot, act, m))
}
