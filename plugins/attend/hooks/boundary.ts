// SPDX-FileCopyrightText: 2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

// The boundary tool's pure pieces: its spec, the mod's refusals, the brief's
// input, the judge's output and verdict, the disagreement rule, the move the
// mod carries out, and the tool's result. No `$` here, so the tests exercise
// them directly; register.tsx makes the calls.
//
// References:
// - ADR-418 §10: the judge decides and never works; the mod keeps only the
//   verdict; the working agent's call stays out of the brief; when the two
//   calls disagree, the more reversible move wins; the refusals come first.
// - ADR-418 §12: the stop and continue rows of the tool's table.
// - ADR-418 §13: every stop writes a handoff, the handoff is the exit flag,
//   and no self-compaction while the operator is talking.
// - ADR-418 §14 item 2(b): the agent's own wrap calls the boundary tool, and
//   a `/wrap` the operator types is not judged.

import { ATTEND, WAYS, oneLine } from './lib.ts'

/** The agent's own call, as the wrap skill's boundary mode passes it. */
export type Call = 'stop' | 'compact' | 'continue'
/** The judge's verdict. */
export type Verdict = Call | 'doubt'
export type Sufficiency = 'sufficient' | 'holds-detail'

/** The verdict fields the mod keeps from the judge, and nothing else. */
export type Judged = { verdict: Verdict; handoff: Sufficiency; reason: string; turnsLeft: number; nextStep: string }

/** The handoff the wrap skill writes, under the repository root. */
export const HANDOFF_FILE = '.claude/handoff.wrap.md'
export const ROOT_ARGV = ['git', 'rev-parse', '--show-toplevel'] as const
export const BRIEF_ARGV = [ATTEND, 'keepwarm', 'brief'] as const

/** The tool. Its description holds for either relay setting, since the
 *  setting is read at each check and can change after it registers; the
 *  result names the mode the call ran under. */
export const BOUNDARY_TOOL = {
  name: 'attend_boundary',
  description:
    'Call this only at the end of `/wrap boundary`, after the handoff is written, when a piece of work has landed. A judge, a separate model call that reads only the handoff and the conditions the mod measures, decides whether the session stops, compacts or continues; the mod refuses some calls first (a stop while agents, shells or watchers are in flight; continue while the session drains; compact in a turn the operator started). With mods.attend.continuity.relay on, the verdicts are stop, compact, continue and doubt. With relay off, the tool offers stop and continue only: compaction stays with the operator, so a compact verdict is carried out as stop, and doubt stops and leaves the handoff for the operator without compacting. The result names the outcome and what to do next; act on it, not on your own call. A `/wrap` the operator types is not judged: do not call this tool for one.',
  inputSchema: {
    type: 'object',
    properties: {
      wrap_id: { type: 'string', description: "The wrap id on the handoff's first line (wrap=<id>)." },
      own_call: {
        type: 'string',
        enum: ['stop', 'compact', 'continue'],
        description: 'Your own call: stop, compact or continue, as the wrap skill defines them. The mod records it beside the judge\'s verdict; it does not reach the judge.',
      },
    },
    required: ['wrap_id', 'own_call'],
  },
  isDeferred: false,
} as const

/** A wrap id: 8 to 64 letters, digits and `-`, as attend checks it. */
export function isWrapId(s: string): boolean {
  return /^[0-9A-Za-z-]{8,64}$/.test(s)
}

/** The tool's input, or why it cannot be used. */
export function boundaryInput(e: unknown): { wrapId: string; own: Call } | { error: string } {
  const a = (e ?? {}) as { wrap_id?: unknown; own_call?: unknown }
  const wrapId = typeof a.wrap_id === 'string' ? a.wrap_id.trim() : ''
  if (!isWrapId(wrapId)) return { error: `wrap_id ${JSON.stringify(wrapId)} is no wrap id: pass the id on the handoff's first line (wrap=<id>)` }
  const own = typeof a.own_call === 'string' ? a.own_call.trim() : ''
  if (own !== 'stop' && own !== 'compact' && own !== 'continue') return { error: `own_call ${JSON.stringify(own)} is not stop, compact or continue` }
  return { wrapId, own }
}

/** Whether the handoff's first line carries `wrap=<id>`, as one word. */
export function handoffCarries(text: string, wrapId: string): boolean {
  const first = text.split('\n', 1)[0] ?? ''
  return first.split(/\s+/).includes(`wrap=${wrapId}`)
}

// ── what is in flight, and the refusals ─────────────────────────────────

/** What is still running, one line each: the agents from `$.agent.list()`,
 *  the shells and watchers from the last Stop's record through the brief. */
export type InFlight = { agents: string[]; shells: string[]; watchers: string[] }

/** What the refusals read. `operatorTurn`: an operator's prompt started
 *  the turn the call runs in. */
export type Facts = { inFlight: InFlight; draining: boolean; operatorTurn: boolean }

export type Refusal = { call: Verdict; why: string; todo: string }

/** The in-flight agent loops, one line each: started, running a turn, or
 *  waiting on work of their own, as agentsInFlight counts them. */
export function agentLines(list: readonly { status: string; description?: string; type?: string; name?: string }[]): string[] {
  return list
    .filter(a => a.status === 'pending' || a.status === 'running' || a.status === 'waiting')
    .map(a => oneLine(`${a.name ?? a.type ?? 'agent'}: ${a.description ?? ''} (${a.status})`))
}

/** The task list as `TaskList` returns it, split as the brief takes it. */
export function taskLines(result: unknown): { open: string[]; inProgress: string[] } | undefined {
  const tasks = (result as { tasks?: unknown } | undefined)?.tasks
  if (!Array.isArray(tasks)) return undefined
  const line = (t: { id?: unknown; subject?: unknown }) => oneLine(`#${String(t.id ?? '?')} ${String(t.subject ?? '')}`)
  const of = (status: string) => tasks.filter(t => (t as { status?: unknown }).status === status).map(t => line(t as never))
  return { open: of('pending'), inProgress: of('in_progress') }
}

/** What the mod passes `attend keepwarm brief` on stdin: what only it sees.
 *  The working agent's own call is never in it. */
export function briefInput(o: { agents?: string[]; tasks?: { open: string[]; inProgress: string[] }; window?: number; operatorPromptAtMs?: number }): string {
  const doc: Record<string, unknown> = {}
  if (o.agents) doc.agents_in_flight = o.agents
  if (o.tasks) {
    doc.tasks_open = o.tasks.open
    doc.tasks_in_progress = o.tasks.inProgress
  }
  if (o.window !== undefined && o.window > 0) doc.window = o.window
  if (o.operatorPromptAtMs !== undefined) doc.operator_prompt_at = Math.floor(o.operatorPromptAtMs / 1000)
  return JSON.stringify(doc)
}

/** The brief's conditions as an object, or undefined where it printed none. */
export function parseBrief(stdout: string): Record<string, unknown> | undefined {
  try {
    const v = JSON.parse(stdout) as unknown
    return typeof v === 'object' && v !== null && !Array.isArray(v) ? (v as Record<string, unknown>) : undefined
  } catch {
    return undefined
  }
}

/** What is in flight: the mod's agents where it could read them, else the
 *  brief's, and the brief's shells and watchers. A list the brief reports
 *  unknown counts as none. */
export function inFlightOf(brief: Record<string, unknown> | undefined, agents: string[] | undefined): InFlight {
  const lines = (k: string) => {
    const v = brief?.[k]
    return Array.isArray(v) ? v.filter((x): x is string => typeof x === 'string') : []
  }
  return { agents: agents ?? lines('agents_in_flight'), shells: lines('shells_in_flight'), watchers: lines('watchers_in_flight') }
}

function count(n: number, one: string, many: string): string {
  return `${n} ${n === 1 ? one : many}`
}

/** `2 agents and 1 shell`, or undefined when nothing is in flight. */
export function inFlightText(f: InFlight): string | undefined {
  const parts = [
    f.agents.length > 0 ? count(f.agents.length, 'agent', 'agents') : '',
    f.shells.length > 0 ? count(f.shells.length, 'shell', 'shells') : '',
    f.watchers.length > 0 ? count(f.watchers.length, 'watcher', 'watchers') : '',
  ].filter(p => p !== '')
  if (parts.length === 0) return undefined
  return parts.length === 1 ? parts[0] : `${parts.slice(0, -1).join(', ')} and ${parts[parts.length - 1]}`
}

const COLLECT = 'Finish the work in flight and collect its reports, then run /wrap boundary again at the next boundary.'

/** The mod's refusal of `call`, or undefined. Things in flight refuse a
 *  stop (and a doubt, which stops), since exiting would end them; they do
 *  not refuse a compaction. During the drain continue is refused. In a turn
 *  an operator's prompt started, compact is refused. The same rules hold
 *  for the agent's call, before the judge, and for the move the mod would
 *  carry out after it.
 *
 *  References:
 *  - ADR-418 §10: the mod's refusals come first.
 *  - ADR-418 §13: no self-compaction while the operator is talking. */
export function refusalOf(call: Verdict, f: Facts): Refusal | undefined {
  const busy = inFlightText(f.inFlight)
  if ((call === 'stop' || call === 'doubt') && busy !== undefined) {
    return { call, why: `${busy} ${f.inFlight.agents.length + f.inFlight.shells.length + f.inFlight.watchers.length === 1 ? 'is' : 'are'} in flight, and a stop would end them`, todo: COLLECT }
  }
  if (call === 'continue' && f.draining) {
    return { call, why: 'the session drains, and from the drain point continue is off the table', todo: 'Finish the work in flight, then call attend_boundary again with own_call compact or stop.' }
  }
  if (call === 'compact' && f.operatorTurn) {
    return { call, why: 'the operator prompted this turn, and the agent does not compact itself while the operator is talking', todo: 'Answer the operator and go on; the handoff stands as a checkpoint.' }
  }
  return undefined
}

// ── the judge ────────────────────────────────────────────────────────────

/** `ways judge continuance` for the handoff, the brief on stdin. `--host`:
 *  on the claude-code engine the mod sends the job itself. */
export function judgeArgv(session: string, handoff: string): string[] {
  return [WAYS, 'judge', 'continuance', '--session', session, '--handoff', handoff, '--host']
}

/** The `$.model.complete` answer to a job, read on stdin. */
export function parseArgv(session: string): string[] {
  return [WAYS, 'judge', 'continuance', '--session', session, '--parse', '--host']
}

/** The job `ways judge continuance` prints on claude-code. */
export type Job = { model: string; system: string; prompt: string; max_tokens: number; timeout_ms: number }

export type JudgeOut = { kind: 'verdict'; judged: Judged } | { kind: 'job'; job: Job } | { kind: 'fallback'; reason: string }

function isJob(v: unknown): v is Job {
  if (typeof v !== 'object' || v === null) return false
  const j = v as Record<string, unknown>
  return typeof j.model === 'string' && typeof j.system === 'string' && typeof j.prompt === 'string' && typeof j.max_tokens === 'number' && typeof j.timeout_ms === 'number'
}

const VERDICTS: readonly Verdict[] = ['stop', 'compact', 'continue', 'doubt']
const MAX_LINE = 400

/** The five verdict fields of `v`, or undefined where any is missing or not
 *  in its form. Other keys are dropped. */
export function verdictOf(v: unknown): Judged | undefined {
  if (typeof v !== 'object' || v === null) return undefined
  const o = v as Record<string, unknown>
  const line = (k: string) => {
    const s = o[k]
    if (typeof s !== 'string') return undefined
    const t = oneLine(s)
    return t === '' ? undefined : t.slice(0, MAX_LINE)
  }
  const verdict = o.verdict
  const handoff = o.handoff
  const turns = o.turns_left
  const reason = line('reason')
  const nextStep = line('next_step')
  if (typeof verdict !== 'string' || !VERDICTS.includes(verdict as Verdict)) return undefined
  if (handoff !== 'sufficient' && handoff !== 'holds-detail') return undefined
  if (typeof turns !== 'number' || !Number.isInteger(turns) || turns < 0) return undefined
  if (reason === undefined || nextStep === undefined) return undefined
  return { verdict: verdict as Verdict, handoff, reason, turnsLeft: turns, nextStep }
}

/** What `ways judge continuance` printed: a verdict, a job, or a fallback.
 *  Output that is none of them is a fallback naming why. */
export function parseJudgeOut(r: { exitCode: number; stdout: string; stderr: string }): JudgeOut {
  if (r.exitCode !== 0) return { kind: 'fallback', reason: `ways judge continuance exited ${r.exitCode}: ${oneLine(r.stderr || r.stdout).slice(0, MAX_LINE)}` }
  let v: unknown
  try {
    v = JSON.parse(r.stdout.trim())
  } catch {
    return { kind: 'fallback', reason: 'ways judge continuance printed no JSON' }
  }
  const o = (typeof v === 'object' && v !== null ? v : {}) as Record<string, unknown>
  if (typeof o.fallback === 'string') return { kind: 'fallback', reason: oneLine(o.fallback).slice(0, MAX_LINE) }
  if ('job' in o) return isJob(o.job) ? { kind: 'job', job: o.job } : { kind: 'fallback', reason: 'ways judge continuance printed a job the mod cannot send' }
  const judged = verdictOf(o)
  return judged ? { kind: 'verdict', judged } : { kind: 'fallback', reason: 'ways judge continuance printed no verdict' }
}

/** The job as `$.model.complete` takes it: no tools and no history. */
export function requestOf(job: Job) {
  return { model: job.model, system: job.system, prompt: job.prompt, maxTokens: job.max_tokens, timeoutMs: job.timeout_ms }
}

/** The verdict object alone from a model's reply: the reply itself where it
 *  is one JSON object, else the last JSON object in it that carries a
 *  `verdict`. Prose around the object is dropped; a reply with no such
 *  object is passed on whole, and the parse finds no verdict in it.
 *
 *  References:
 *  - ADR-418 §10: the mod keeps only the verdict line and discards
 *    anything else. */
export function verdictText(reply: string): string {
  const t = reply.trim()
  if (isVerdictObject(t)) return t
  let found: string | undefined
  for (let i = t.indexOf('{'); i !== -1; i = t.indexOf('{', i + 1)) {
    const end = objectEnd(t, i)
    if (end === -1) continue
    const s = t.slice(i, end + 1)
    if (isVerdictObject(s)) found = s
  }
  return found ?? t
}

function isVerdictObject(s: string): boolean {
  try {
    const v = JSON.parse(s) as unknown
    return typeof v === 'object' && v !== null && !Array.isArray(v) && 'verdict' in v
  } catch {
    return false
  }
}

/** The index of the `}` that closes the object opening at `start`, strings
 *  skipped; -1 where it does not close. */
function objectEnd(s: string, start: number): number {
  let depth = 0
  let inString = false
  for (let i = start; i < s.length; i++) {
    const c = s[i]
    if (inString) {
      if (c === '\\') i++
      else if (c === '"') inString = false
    } else if (c === '"') inString = true
    else if (c === '{') depth++
    else if (c === '}' && --depth === 0) return i
  }
  return -1
}

/** The answer `--parse` reads, from `$.model.complete`'s result: the
 *  verdict object alone where the model answered. */
export function answerOf(r: { isAnswered: boolean; text?: string; usage?: unknown; reason?: string }): string {
  return JSON.stringify(r.isAnswered ? { text: verdictText(r.text ?? ''), usage: r.usage } : { reason: r.reason ?? 'no text', usage: r.usage })
}

/** No judge, a fallback, or an unparsable reply: doubt. The handoff is
 *  taken to hold detail, the side that keeps the session warm longer.
 *
 *  References:
 *  - ADR-418 §10: doubt when the judge cannot tell. */
export function doubtOf(reason: string): Judged {
  return { verdict: 'doubt', handoff: 'holds-detail', reason: oneLine(`no verdict from the judge: ${reason}`).slice(0, MAX_LINE), turnsLeft: 0, nextStep: 'the operator reads the handoff' }
}

// ── the move ─────────────────────────────────────────────────────────────

/** What the mod carries out: the move attend records (`--verdict`), which
 *  arms the exit on stop; the lines that say why it differs from the
 *  judge's verdict; what the agent does next; and the refusal, where the
 *  move the verdict led to was refused. */
export type Plan = { move: 'stop' | 'continue'; notes: string[]; todo: string; refused?: Refusal; isFixHandoff: boolean }

const TODO_STOP = 'End the turn and start nothing new. An operator prompt disarms the exit.'
const TODO_CONTINUE = 'Go back to work at DO NEXT. The handoff stands as the checkpoint for an exit that cannot run a wrap.'

/** The move from the judge's verdict `v` and the agent's `own` call.
 *
 *  - Own continue against a judge's stop or compact: the more reversible
 *    move wins, so the agent fixes the handoff and continues; nothing is
 *    armed. Own stop against compact: an armed stop beats a compaction.
 *  - During the drain a continue verdict goes on as a compaction.
 *  - With relay off, compaction stays with the operator: compact is
 *    carried out as stop, and doubt stops and leaves the handoff for the
 *    operator, whatever `canCompact` says.
 *  - With relay on, compact and doubt are carried out as stop until the
 *    mod compacts through the tool (`canCompact`), the more reversible move.
 *  - The refusals then hold for the move: a stop with things in flight
 *    arms nothing, and the agent collects their reports.
 *
 *  References:
 *  - ADR-418 §10: when the two calls disagree, the more reversible move
 *    wins.
 *  - ADR-418 §12: the stop, compact and continue rows. */
export function settle(own: Call, v: Verdict, f: Facts, relay: boolean, canCompact = false): Plan {
  if (own === 'continue' && (v === 'stop' || v === 'compact')) {
    return {
      move: 'continue',
      notes: [`The judge said ${v} and your call was continue: the more reversible move wins, so nothing is armed. The handoff is missing what you see.`],
      todo: `Add what is missing to ${HANDOFF_FILE}, then go back to work at DO NEXT.`,
      isFixHandoff: true,
    }
  }
  const notes: string[] = []
  let move: Verdict = v
  if (own === 'stop' && move === 'compact') {
    move = 'stop'
    notes.push('The judge said compact and your call was stop: an armed stop beats a compaction.')
  }
  if (move === 'continue' && f.draining) {
    move = 'compact'
    notes.push('The session drains, so the continue verdict goes on as a compaction.')
  }
  if ((move === 'compact' || move === 'doubt') && !relay) {
    notes.push(
      move === 'compact'
        ? 'Relay is off in this session, so compaction stays with the operator: the compact verdict is carried out as stop.'
        : `Relay is off in this session: doubt stops without compacting, and the handoff stays in ${HANDOFF_FILE} for the operator.`,
    )
    move = 'stop'
  }
  if ((move === 'compact' || move === 'doubt') && relay && !canCompact) {
    notes.push(`The mod does not carry out ${move} through this tool yet: it stops, the more reversible move, and does not compact.`)
    move = 'stop'
  }
  const refused = refusalOf(move, f)
  if (refused) return { move: 'continue', notes, todo: refused.todo, refused, isFixHandoff: false }
  return { move: move === 'stop' ? 'stop' : 'continue', notes, todo: move === 'stop' ? TODO_STOP : TODO_CONTINUE, isFixHandoff: false }
}

/** `attend keepwarm judged` for the move: attend records the move the mod
 *  carries out, which arms the exit on stop, beside the agent's call; the
 *  reason names the judge's verdict where the move differs from it. */
export function judgedArgv(wrapId: string, plan: Plan, j: Judged, own: Call): string[] {
  const reason = j.verdict === plan.move ? j.reason : `the judge said ${j.verdict}; carried out as ${plan.move}: ${j.reason}`
  return [ATTEND, 'keepwarm', 'judged', '--wrap', wrapId, '--verdict', plan.move, '--handoff', j.handoff, '--own', own, '--reason', oneLine(reason)]
}

// ── the result ───────────────────────────────────────────────────────────

function modeLine(relay: boolean): string {
  return relay ? 'Mode: relay on (stop, compact, continue and doubt).' : 'Mode: relay off (stop and continue only; compaction stays with the operator).'
}

/** A call the mod refused before any judge call: nothing judged, nothing
 *  recorded or armed. */
export function refusedResult(r: Refusal, relay: boolean) {
  return {
    result: [`Refused: your call ${r.call}, because ${r.why}. Nothing was judged and nothing is armed.`, modeLine(relay), r.todo].join('\n'),
    isError: true as const,
  }
}

/** The tool's result: the verdict fields, the move and why, attend's word,
 *  and the next step. Nothing else of the judge's text. */
export function boundaryResult(o: { judged: Judged; plan: Plan; relay: boolean; recorded: { exitCode: number; stdout: string; stderr: string } }) {
  const { judged: j, plan, relay, recorded } = o
  const fields = [
    `Verdict: ${j.verdict}; handoff ${j.handoff}; turns left ${j.turnsLeft}.`,
    `Reason: ${j.reason}`,
    `Next step: ${j.nextStep}`,
  ]
  if (recorded.exitCode !== 0) {
    const why = oneLine(recorded.stderr || recorded.stdout) || `exit ${recorded.exitCode}`
    return {
      result: [...fields, modeLine(relay), `attend did not record the boundary (${why}), so nothing is armed.`, 'Write the handoff with this wrap id on its first line, then call attend_boundary again; or go back to work at DO NEXT.'].join('\n'),
      isError: true as const,
    }
  }
  const outcome = plan.refused
    ? `Outcome: nothing is armed, because ${plan.refused.why}.`
    : plan.move === 'stop'
      ? `Outcome: stop. ${oneLine(recorded.stdout) || 'The exit is armed.'}`
      : 'Outcome: continue. Nothing is armed.'
  return { result: [...fields, modeLine(relay), ...plan.notes, outcome, plan.todo].join('\n') }
}

/** A typed `/wrap`, refused: the operator's own wrap is not judged.
 *
 *  References:
 *  - ADR-418 §14 item 2(b): a `/wrap` the operator types stays the
 *    operator's own and is not judged. */
export const TYPED_WRAP_REFUSAL = {
  result: 'Refused: this turn runs a /wrap the operator typed, which stays the operator\'s own and is not judged. Nothing was judged and nothing is armed. End the wrap as its mode says.',
  isError: true as const,
}

/** Whether a typed `/wrap`'s arguments ask for the boundary mode. */
export function isBoundaryWrap(args: string): boolean {
  return args.split(/\s+/).includes('boundary')
}
