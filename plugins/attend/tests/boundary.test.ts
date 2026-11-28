// SPDX-FileCopyrightText: 2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

// The boundary tool's pure pieces (hooks/boundary.ts): the refusals, the
// verdict's parse and the prose around it, the move the mod carries out,
// the argv, attend's record and the move it leaves. The handler is tested
// through the mod in attend.test.ts.
//
// References:
// - ADR-418 §10: the refusals come first; when the two calls disagree, the
//   more reversible move wins; the mod keeps only the verdict; the
//   `max_relays` cap ends the chain.
// - ADR-418 §11: doubt sends the handoff to the operator.
// - ADR-418 §12: the stop, compact and continue rows.

import { describe, expect, test } from 'claude-code/testing'

import {
  agentLines,
  boundaryInput,
  boundaryResult,
  briefInput,
  doubtOf,
  followRecord,
  handoffCarries,
  hasRoom,
  hasTasks,
  inFlightOf,
  isBoundaryWrap,
  isBusy,
  judgedArgv,
  parseJudgeOut,
  parseRecorded,
  refusalOf,
  relaysOf,
  settle,
  taskLines,
  verdictOf,
  verdictText,
} from '../hooks/boundary.ts'
import type { Facts, InFlight, Recorded } from '../hooks/boundary.ts'

const NONE: InFlight = { agents: [], shells: [], watchers: [] }
const QUIET: Facts = { inFlight: NONE, draining: false, operatorTurn: false }
const BUSY: Facts = { ...QUIET, inFlight: { agents: ['general-purpose: review (running)'], shells: [], watchers: [] } }
const DRAINING: Facts = { ...QUIET, draining: true }
const OPERATOR: Facts = { ...QUIET, operatorTurn: true }
const V = { verdict: 'stop', handoff: 'sufficient', reason: 'nothing remains', turns_left: 0, next_step: 'pick the next card' }

describe('the refusals', () => {
  test('things in flight refuse a stop and a doubt, never a compact or a continue', () => {
    expect(refusalOf('stop', BUSY)?.why).toBe('1 agent is in flight, and a stop would end them')
    expect(refusalOf('doubt', BUSY)?.call).toBe('doubt')
    expect(refusalOf('compact', BUSY)).toBeUndefined()
    expect(refusalOf('continue', BUSY)).toBeUndefined()
    const many = { ...QUIET, inFlight: { agents: ['a', 'b'], shells: ['s'], watchers: ['w', 'x'] } }
    expect(refusalOf('stop', many)?.why).toBe('2 agents, 1 shell and 2 watchers are in flight, and a stop would end them')
    expect(refusalOf('stop', QUIET)).toBeUndefined()
  })

  test('the drain refuses continue alone, and an operator\'s turn refuses compact alone', () => {
    expect(refusalOf('continue', DRAINING)?.why).toBe('the session drains, and from the drain point continue is off the table')
    expect(['stop', 'compact', 'doubt'].map(c => refusalOf(c as never, DRAINING))).toEqual([undefined, undefined, undefined])
    expect(refusalOf('compact', OPERATOR)?.why).toBe('the operator prompted this turn, and the agent does not compact itself while the operator is talking')
    expect(['stop', 'continue', 'doubt'].map(c => refusalOf(c as never, OPERATOR))).toEqual([undefined, undefined, undefined])
  })
})

describe('the move', () => {
  test('relay off never compacts: compact and doubt stop, even once the tool can compact', () => {
    for (const canCompact of [false, true]) {
      expect(settle('stop', 'compact', QUIET, false, canCompact).move).toBe('stop')
      expect(settle('compact', 'compact', QUIET, false, canCompact).move).toBe('stop')
      expect(settle('stop', 'doubt', QUIET, false, canCompact).move).toBe('stop')
    }
    expect(settle('compact', 'compact', QUIET, false).notes).toEqual(['Relay is off in this session, so compaction stays with the operator: the compact verdict is carried out as stop.'])
  })

  test('relay off records doubt, which arms and sends the handoff, and still compacts nothing', () => {
    const p = settle('stop', 'doubt', QUIET, false, true)
    expect([p.move, p.record, p.isHandoffSent]).toEqual(['stop', 'doubt', true])
    expect(p.notes).toEqual(['Relay is off in this session: doubt stops without compacting, and the handoff in .claude/handoff.wrap.md goes to the operator over attend.'])
    const c = settle('compact', 'compact', QUIET, false, true)
    expect([c.move, c.record, c.isHandoffSent]).toEqual(['stop', 'stop', false])
  })

  test('relay on compacts below max_relays, and at the cap or with no count the compact verdict is carried out as stop', () => {
    const p = settle('compact', 'compact', QUIET, true, true, { n: 2, max: 5 })
    expect([p.move, p.record, p.notes, p.todo]).toEqual(['compact', 'compact', [], 'End the turn now and start nothing new: the mod compacts once it ends.'])
    const cap = settle('compact', 'compact', QUIET, true, false, { n: 5, max: 5 })
    expect([cap.move, cap.record, cap.notes]).toEqual(['stop', 'stop', ["5 of 5 relays have run since the operator's last prompt, the most this session allows: the compact verdict is carried out as stop."]])
    expect(settle('compact', 'compact', QUIET, true, false).notes).toEqual(['The relay count could not be read, so max_relays cannot be checked: the compact verdict is carried out as stop.'])
    // An agent in flight comes through a compaction; it refuses a stop.
    expect(settle('compact', 'compact', BUSY, true, true, { n: 0, max: 5 }).move).toBe('compact')
    expect(settle('compact', 'compact', BUSY, true, false, { n: 5, max: 5 }).refused?.call).toBe('stop')
    expect([hasRoom({ n: 4, max: 5 }), hasRoom({ n: 5, max: 5 }), hasRoom(undefined)]).toEqual([true, false, false])
  })

  test('relay on, doubt compacts and sends the handoff; in an operator\'s turn it stops without compacting; things in flight refuse it', () => {
    const p = settle('stop', 'doubt', QUIET, true, true)
    expect([p.move, p.record, p.isHandoffSent, p.notes]).toEqual(['doubt', 'doubt', true, []])
    // At the cap too: a doubt counts no relay.
    expect(settle('compact', 'doubt', QUIET, true, false, { n: 5, max: 5 }).move).toBe('doubt')
    const o = settle('stop', 'doubt', OPERATOR, true, true)
    expect([o.move, o.record, o.isHandoffSent]).toEqual(['stop', 'doubt', true])
    const busy = settle('stop', 'doubt', BUSY, true, true)
    expect([busy.move, busy.record, busy.isHandoffSent, busy.refused?.call]).toEqual(['continue', 'continue', false, 'doubt'])
  })

  test('a fallback doubt, which no verdict produced, stops without compacting even with relay on, and records doubt', () => {
    const f = settle('stop', 'doubt', QUIET, true, true, { n: 0, max: 5 }, true)
    expect([f.move, f.record, f.isHandoffSent]).toEqual(['stop', 'doubt', true])
    expect(f.notes).toEqual(['The judge gave no verdict, so this doubt stops without compacting, and the handoff in .claude/handoff.wrap.md goes to the operator over attend.'])
    // Relay off names relay off, as before.
    expect(settle('stop', 'doubt', QUIET, false, true, undefined, true).notes[0]).toMatch(/^Relay is off in this session/)
  })

  test('the disagreement rule: own continue against stop or compact continues and fixes the handoff; own stop against compact stops', () => {
    for (const v of ['stop', 'compact'] as const) {
      const p = settle('continue', v, QUIET, true)
      expect([p.move, p.isFixHandoff, p.todo]).toEqual(['continue', true, 'Add what is missing to .claude/handoff.wrap.md, then go back to work at DO NEXT.'])
    }
    expect(settle('stop', 'compact', QUIET, true).notes[0]).toBe('The judge said compact and your call was stop: an armed stop beats a compaction.')
    // The judge's continue stands against the agent's stop.
    expect(settle('stop', 'continue', QUIET, false).move).toBe('continue')
    expect(settle('continue', 'doubt', QUIET, false).move).toBe('stop')
  })

  test('the refusals hold for the move: a stop with an agent in flight arms nothing, and the drain turns continue into a compaction', () => {
    const p = settle('compact', 'stop', BUSY, false)
    expect([p.move, p.refused?.call]).toEqual(['continue', 'stop'])
    const d = settle('stop', 'continue', DRAINING, true)
    expect([d.move, d.notes[0]]).toEqual(['stop', 'The session drains, so the continue verdict goes on as a compaction.'])
  })

  test('attend records the judge\'s verdict and the move apart, with the judge\'s own reason', () => {
    const j = verdictOf({ ...V, verdict: 'compact' })!
    expect(judgedArgv('20261127T0900Z-3f9a1c2e', settle('compact', 'compact', QUIET, false).record, j, 'compact')).toEqual([
      'attend', 'keepwarm', 'judged', '--json', '--wrap', '20261127T0900Z-3f9a1c2e', '--verdict', 'compact', '--handoff', 'sufficient', '--own', 'compact', '--move', 'stop', '--reason', 'nothing remains',
    ])
    const d = verdictOf({ ...V, verdict: 'doubt', reason: 'two\nlines' })!
    expect(judgedArgv('20261127T0900Z-3f9a1c2e', 'doubt', d, 'stop').slice(-6)).toEqual(['--own', 'stop', '--move', 'doubt', '--reason', 'two lines'])
    // In the operator's turn attend hears so, and records no compaction.
    expect(judgedArgv('20261127T0900Z-3f9a1c2e', 'doubt', d, 'stop', true).slice(-3)).toEqual(['--reason', 'two lines', '--operator-turn'])
  })

  const RECORD = { recorded: true, armed: false, acted_on: true, move: 'compact', compact_refused: null, wrap: null, instructions: 'compact as the relay does', resume: 'relay: resume' }
  const rec = (o: Record<string, unknown> = {}): Recorded => parseRecorded(JSON.stringify({ ...RECORD, ...o }))!

  test('attend\'s record: the move it recorded, whether it armed and acts, why it refused a compaction, the doubt\'s route, and its instructions and resume', () => {
    const route = { dest: 'send', file: '/state/wrap/s1.md', to: 'aaron', after: 'relay', after_keepalives: 0, handoff: 'handoff: the boundary judge said doubt.' }
    expect(parseRecorded(JSON.stringify({ ...RECORD, armed: true, move: 'doubt', wrap: route, resume: null }))).toEqual({
      move: 'doubt', isArmed: true, isActedOn: true, compactRefused: null, instructions: 'compact as the relay does', resume: null,
      route: { dest: 'send', file: '/state/wrap/s1.md', to: 'aaron', after: 'relay', afterKeepalives: 0, handoff: 'handoff: the boundary judge said doubt.' },
    })
    expect(rec({ move: 'stop', armed: true, acted_on: false, compact_refused: 'max_relays is reached', instructions: null, resume: null })).toEqual({
      move: 'stop', isArmed: true, isActedOn: false, compactRefused: 'max_relays is reached', route: null, instructions: null, resume: null,
    })
    // A route that does not parse is none.
    expect(rec({ move: 'doubt', wrap: { to: 'aaron' } }).route).toBeNull()
    expect(parseRecorded('exit armed for wrap 20261127T0900Z-3f9a1c2e')).toBeUndefined()
    expect(parseRecorded('{"recorded":true,"armed":true}')).toBeUndefined()
    // A record from before acted_on, or with a move attend has no word for, is no record.
    expect(parseRecorded('{"recorded":true,"armed":false,"move":"compact","wrap":null}\n')).toBeUndefined()
    expect(parseRecorded(JSON.stringify({ ...RECORD, move: 'pause' }))).toBeUndefined()
  })

  test('the move attend recorded wins: its stop for a compact stops and says why, and a compaction without its instructions does not run', () => {
    const compact = settle('compact', 'compact', QUIET, true, true, { n: 1, max: 5 })
    expect(followRecord(compact, rec())).toEqual({ plan: compact, isStopDue: false })
    const refused = followRecord(compact, rec({ move: 'stop', armed: true, compact_refused: 'a relay wrap is pending', instructions: null, resume: null }))
    expect([refused.plan.move, refused.plan.record, refused.isStopDue]).toEqual(['stop', 'stop', false])
    expect(refused.plan.notes).toEqual(['attend recorded stop, not compact: a relay wrap is pending.'])
    expect(refused.plan.todo).toBe('End the turn and start nothing new. An operator prompt disarms the exit.')
    // No instructions or no resume: the compact stops, and the stop is recorded again.
    for (const o of [{ instructions: null }, { resume: null }]) {
      const none = followRecord(compact, rec(o))
      expect([none.plan.move, none.plan.record, none.isStopDue]).toEqual(['stop', 'stop', true])
    }
    expect(followRecord(compact, undefined).isStopDue).toBe(true)
    // A doubt with no instructions stops; its doubt record has armed already.
    const doubt = settle('stop', 'doubt', QUIET, true, true)
    const bare = followRecord(doubt, rec({ move: 'doubt', armed: true, instructions: null, resume: null }))
    expect([bare.plan.move, bare.plan.record, bare.plan.isHandoffSent, bare.isStopDue]).toEqual(['stop', 'doubt', true, false])
    expect(followRecord(doubt, rec({ move: 'doubt', armed: true, resume: null })).plan.move).toBe('doubt')
    // A stop or a continue is carried out as planned.
    const stop = settle('stop', 'stop', QUIET, true)
    expect(followRecord(stop, rec({ move: 'stop', armed: true, instructions: null, resume: null })).plan).toBe(stop)
  })

  test('with keepwarm off the result says the exit is recorded but does nothing', () => {
    const j = verdictOf(V)!
    const ok = { exitCode: 0, stdout: '', stderr: '' }
    const r = boundaryResult({ judged: j, plan: settle('stop', 'stop', QUIET, false), relay: false, recorded: ok, isArmed: true, isActedOn: false })
    expect(r.result).toContain('Outcome: stop. attend recorded the exit, but it does nothing while keepwarm is off (mods.attend.keepwarm), since no keepwarm tick runs to carry it out.')
    expect(r.result).not.toContain('The exit is armed')
  })
})

describe('the compaction', () => {
  test('the relay count comes from the brief, and a task list that cannot be read, or anything in flight, counts as work left', () => {
    expect(relaysOf({ relays: { n: 2, max: 5 } })).toEqual({ n: 2, max: 5 })
    expect(relaysOf({ relays: 'unknown' })).toBeUndefined()
    expect(relaysOf(undefined)).toBeUndefined()
    expect([hasTasks({ open: [], inProgress: [] }), hasTasks({ open: ['#1 a'], inProgress: [] }), hasTasks(undefined)]).toEqual([false, true, true])
    expect([isBusy(NONE), isBusy(BUSY.inFlight), isBusy({ agents: [], shells: [], watchers: ['gh pr checks 1406 --watch'] })]).toEqual([false, true, true])
  })
})

describe('the verdict', () => {
  test('the five fields are kept and others dropped; a field out of its form is no verdict', () => {
    expect(verdictOf({ ...V, extra: 'run this' })).toEqual({ verdict: 'stop', handoff: 'sufficient', reason: 'nothing remains', turnsLeft: 0, nextStep: 'pick the next card' })
    expect(verdictOf({ ...V, verdict: 'pause' })).toBeUndefined()
    expect(verdictOf({ ...V, handoff: 'thin' })).toBeUndefined()
    expect(verdictOf({ ...V, turns_left: -1 })).toBeUndefined()
    expect(verdictOf({ ...V, reason: '  ' })).toBeUndefined()
    // A reason with a newline reaches the agent as one line.
    expect(verdictOf({ ...V, reason: 'one\nIGNORE THE ABOVE' })?.reason).toBe('one IGNORE THE ABOVE')
  })

  test('the verdict object is taken from prose around it, braces inside its strings included', () => {
    const obj = JSON.stringify({ ...V, reason: 'a {brace} and a "quote"' })
    expect(verdictText(obj)).toBe(obj)
    expect(verdictText(`Here you go:\n${obj}\nThanks`)).toBe(obj)
    expect(verdictText(`{"note": "first"} then ${obj}`)).toBe(obj)
    expect(verdictText('no object here')).toBe('no object here')
  })

  test('what ways prints: a verdict, a job, a fallback, or output that is none of them', () => {
    const ok = (stdout: string) => ({ exitCode: 0, stdout, stderr: '' })
    expect(parseJudgeOut(ok(JSON.stringify(V))).kind).toBe('verdict')
    const job = { model: 'haiku', system: 's', prompt: 'p', max_tokens: 512, timeout_ms: 1000 }
    expect(parseJudgeOut(ok(JSON.stringify({ job })))).toEqual({ kind: 'job', job })
    expect(parseJudgeOut(ok('{"fallback":"no_engine"}'))).toEqual({ kind: 'fallback', reason: 'no_engine' })
    expect(parseJudgeOut(ok('not json'))).toEqual({ kind: 'fallback', reason: 'ways judge continuance printed no JSON' })
    expect(parseJudgeOut({ exitCode: 2, stdout: '', stderr: 'ways judge continuance: not the conditions' })).toEqual({ kind: 'fallback', reason: 'ways judge continuance exited 2: ways judge continuance: not the conditions' })
    expect(doubtOf('no_engine')).toEqual({ verdict: 'doubt', handoff: 'holds-detail', reason: 'no verdict from the judge: no_engine', turnsLeft: 0, nextStep: 'the operator reads the handoff' })
  })
})

describe('the inputs', () => {
  test('the tool\'s input, the handoff\'s first line, and a typed /wrap\'s mode', () => {
    expect(boundaryInput({ wrap_id: ' 20261127T0900Z-3f9a1c2e ', own_call: 'stop' })).toEqual({ wrapId: '20261127T0900Z-3f9a1c2e', own: 'stop' })
    expect('error' in boundaryInput({ wrap_id: 'short', own_call: 'stop' })).toBe(true)
    expect('error' in boundaryInput({ wrap_id: '20261127T0900Z-3f9a1c2e', own_call: 'doubt' })).toBe(true)
    expect(handoffCarries('2026-11-27 e6bbfbdd wrap=20261127T0900Z-3f9a1c2e\nbody', '20261127T0900Z-3f9a1c2e')).toBe(true)
    expect(handoffCarries('2026-11-27 e6bbfbdd wrap=20261127T0900Z-3f9a1c2e-x\nbody', '20261127T0900Z-3f9a1c2e')).toBe(false)
    expect(handoffCarries('2026-11-27 e6bbfbdd\nwrap=20261127T0900Z-3f9a1c2e', '20261127T0900Z-3f9a1c2e')).toBe(false)
    expect([isBoundaryWrap(''), isBoundaryWrap('stay'), isBoundaryWrap('boundary')]).toEqual([false, false, true])
  })

  test('the brief\'s input carries the mod\'s lists and the prompt time, and what is in flight reads the mod\'s agents first', () => {
    const agents = agentLines([
      { status: 'running', type: 'general-purpose', description: 'review the card' },
      { status: 'waiting', type: 'Explore', name: 'scout', description: 'find the hook' },
      { status: 'idle', type: 'teammate', description: 'idle' },
      { status: 'completed', type: 'Plan', description: 'done' },
    ])
    expect(agents).toEqual(['general-purpose: review the card (running)', 'scout: find the hook (waiting)'])
    const tasks = taskLines({ tasks: [{ id: '1', subject: 'a', status: 'pending' }, { id: '2', subject: 'b', status: 'in_progress' }, { id: '3', subject: 'c', status: 'completed' }] })
    expect(tasks).toEqual({ open: ['#1 a'], inProgress: ['#2 b'] })
    expect(taskLines(undefined)).toBeUndefined()
    expect(JSON.parse(briefInput({ agents, tasks, window: 200_000, operatorPromptAtMs: 1_800_000_000_999 }))).toEqual({
      agents_in_flight: agents, tasks_open: ['#1 a'], tasks_in_progress: ['#2 b'], window: 200_000, operator_prompt_at: 1_800_000_000,
    })
    expect(briefInput({})).toBe('{}')
    const brief = { agents_in_flight: ['stale agent'], shells_in_flight: ['cargo test'], watchers_in_flight: 'unknown' }
    expect(inFlightOf(brief, [])).toEqual({ agents: [], shells: ['cargo test'], watchers: [] })
    expect(inFlightOf(brief, undefined)).toEqual({ agents: ['stale agent'], shells: ['cargo test'], watchers: [] })
  })
})
