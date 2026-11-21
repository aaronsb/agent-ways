// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

import { describe, expect, test } from 'claude-code/testing'
import type { Engine } from 'claude-code/testing'
import type { ModelCompleteResult, On, ProcessRunResult } from 'claude-code'

import { register } from '../hooks/register.ts'
import { JUDGED_TOOLS, REFUSED, TAKE_WAIT_MS, answerOf, hostArgv, parseTake, requestOf, takeArgv } from '../hooks/lib.ts'
import type { JudgeJob } from '../hooks/lib.ts'
import { passedOverHits, registrationsOf } from './passed-over.ts'

// The test's environment has a timer; Claude Code's declarations name none.
declare function setTimeout(fn: () => void, ms: number): unknown

const ID = 'sess-1'

const JOB: JudgeJob = {
  model: 'claude-haiku-4-5',
  system: 'You judge relevance.',
  prompt: 'Candidates: ...',
  max_tokens: 224,
  timeout_ms: 2000,
}
const JOB_2: JudgeJob = { ...JOB, prompt: 'Candidates: the second batch' }

const USAGE = { input_tokens: 812, output_tokens: 41, cache_read_input_tokens: 0, cache_creation_input_tokens: 0 }
const ZERO = { input_tokens: 0, output_tokens: 0, cache_read_input_tokens: 0, cache_creation_input_tokens: 0 }
const VERDICT: ModelCompleteResult = { isAnswered: true, text: '{"judgements":[]}', usage: USAGE }

/** How long the ways gate waits for a host to claim its job before it
 *  withdraws the job and fails open (CLAIM_WINDOW in scan/gate.rs). */
const CLAIM_MS = 500
/** How long it waits for the answer once a host claimed the job. */
const ANSWER_MS = 3000

const ok = (stdout = ''): ProcessRunResult => ({ exitCode: 0, stdout, stderr: '', isStdoutTruncated: false, isStderrTruncated: false })
const sleep = (ms: number) => new Promise<void>(resolve => setTimeout(resolve, ms))

type World = {
  /** The jobs the UserPromptSubmit hooks write, all at once, as parallel
   *  settings hooks (the prompt and state lanes) do. */
  prompt?: JudgeJob[]
  /** The jobs the PreToolUse hooks write, one after another. */
  pre?: JudgeJob[]
  /** The jobs the PostToolUse hooks write once the tool ran. */
  post?: JudgeJob[]
  /** The session resolved to a key engine: `take` says standby. */
  standby?: boolean
  /** What `$.model.complete` resolves to, after `modelMs`. */
  model?: ModelCompleteResult
  modelMs?: number
  /** `$.model.complete` rejects: the engine refused to send the call. */
  modelThrows?: boolean
  /** `ways judge host` fails. */
  hostFails?: boolean
  /** How long the prompt's settings hooks run after their jobs. */
  promptMs?: number
  /** How long the tool runs, between its PreToolUse and PostToolUse hooks. */
  toolMs?: number
  /** The jobs the prompt's settings hooks write and leave without waiting:
   *  the job is pending as the prompt's chain settles. */
  trailing?: JudgeJob[]
  /** A take claims only once it is stopped, and prints the job as it is
   *  killed: the stop lands during the claim. */
  claimOnStop?: boolean
  /** A take with no job runs out its wait, which the clock shows as
   *  TAKE_WAIT_MS gone, and exits 0 with nothing printed. */
  idleTakes?: boolean
  /** A take with no job exits 0 with nothing printed at once, the clock
   *  unmoved. */
  fastEmpty?: boolean
}

type Entry = { job: JudgeJob & { id: string }; claim: () => void; answer: (a: unknown) => void; claimed: Promise<void>; answered: Promise<unknown> }

/** The host beneath the mod: the session's job store, the ways CLI over it
 *  and the model, answered from `world`, every argv, model request and
 *  answer recorded. The settings hooks stand where a session runs them: the
 *  prompt's at the bottom of `prompt.submit`, PreToolUse at
 *  `classic.PreToolUse` and PostToolUse after the tool, at the bottom of
 *  `tool.call`. Each writes its job as the ways scan does and records how
 *  its wait ended: `answered`, or `unclaimed` when no take claimed the job
 *  within CLAIM_MS. */
function host(on: On, world: World) {
  const argvs: string[][] = []
  const models: unknown[] = []
  const answers: unknown[] = []
  const outcomes: string[] = []
  const take = { started: 0, claimed: 0, stopped: 0 }
  const pending: Entry[] = []
  const written: Entry[] = []
  /** What happened, in order: each answer by job id, and what the test
   *  marks. */
  const order: string[] = []
  const clock = { t: 0 }
  let wake: (() => void)[] = []

  async function scan(job: JudgeJob): Promise<void> {
    let claim: () => void = () => undefined
    let answer: (a: unknown) => void = () => undefined
    const claimed = new Promise<void>(resolve => (claim = resolve))
    const answered = new Promise<unknown>(resolve => (answer = resolve))
    const entry: Entry = { job: { ...job, id: `j-${written.length + 1}` }, claim, answer, claimed, answered }
    written.push(entry)
    pending.push(entry)
    for (const w of wake.splice(0)) w()
    const isClaimed = await Promise.race([claimed.then(() => true), sleep(CLAIM_MS).then(() => false)])
    if (!isClaimed) {
      pending.splice(pending.indexOf(entry), 1)
      outcomes.push('unclaimed')
      return
    }
    const isAnswered = await Promise.race([answered.then(() => true), sleep(ANSWER_MS).then(() => false)])
    outcomes.push(isAnswered ? 'answered' : 'silent')
  }

  on('session.id', () => ({ value: ID }) as never)
  on('ui.log', () => ({ value: undefined }) as never)
  on('clock.now', () => ({ value: clock.t }) as never)
  on('clock.sleep', async ($, e) => {
    const ms = (e as unknown as { ms: number }).ms
    await sleep(ms)
    clock.t += ms
    return { value: undefined } as never
  })
  on('process.run', ($, e) => {
    const ev = e as unknown as { argv: string[]; init?: { stdin?: string }; stdin?: string }
    argvs.push([...ev.argv])
    const cmd = ev.argv.slice(1, 3).join(' ')
    if (cmd === 'judge host' && world.hostFails) return { value: { ...ok(), exitCode: 2, stderr: "unrecognized subcommand 'host'" } } as never
    if (cmd === 'judge answer') {
      const a = JSON.parse(ev.init?.stdin ?? ev.stdin ?? 'null')
      answers.push(a)
      const job = ev.argv[ev.argv.indexOf('--job') + 1]
      order.push(`answer ${job}`)
      written.find(w => w.job.id === job)?.answer(a)
    }
    return { value: ok() } as never
  })
  // `ways judge take --wait`: claims the oldest pending job, or waits for one
  // until it is stopped; says standby when the session judges elsewhere.
  on('process.spawn', async function* ($, e, next) {
    argvs.push([...e.argv])
    take.started += 1
    if (world.standby) {
      yield { stream: 'stdout' as const, text: 'standby\n' }
      return { value: { code: 0, signal: null } } as never
    }
    if (world.claimOnStop) {
      await new Promise<void>(resolve => next.signal.addEventListener('abort', () => resolve()))
      take.stopped += 1
      const entry = pending.shift()
      if (entry) {
        entry.claim()
        take.claimed += 1
        yield { stream: 'stdout' as const, text: JSON.stringify(entry.job) + '\n' }
      }
      return { value: { code: null, signal: 'SIGTERM' } } as never
    }
    for (;;) {
      const entry = pending.shift()
      if (entry) {
        entry.claim()
        take.claimed += 1
        yield { stream: 'stdout' as const, text: JSON.stringify(entry.job) + '\n' }
        return { value: { code: 0, signal: null } } as never
      }
      if (next.signal.aborted) {
        take.stopped += 1
        return { value: { code: null, signal: 'SIGTERM' } } as never
      }
      if (world.fastEmpty) return { value: { code: 0, signal: null } } as never
      if (world.idleTakes) {
        await sleep(10)
        clock.t += TAKE_WAIT_MS
        return { value: { code: 0, signal: null } } as never
      }
      await new Promise<void>(resolve => {
        wake.push(resolve)
        next.signal.addEventListener('abort', () => resolve())
      })
    }
  })
  on('model.complete', async ($, e, next) => {
    models.push({ ...e })
    if (world.modelMs) await sleep(world.modelMs)
    if (world.modelThrows) throw new Error('the model is blocked')
    if (next.signal.aborted) return { value: { isAnswered: false, reason: 'aborted', usage: ZERO } } as never
    return { value: world.model } as never
  })
  on('prompt.submit', async ($, e) => {
    for (const job of world.trailing ?? []) void scan(job)
    await Promise.all((world.prompt ?? []).map(scan))
    if (world.promptMs) await sleep(world.promptMs)
    return { text: e.text }
  })
  on('classic.PreToolUse', async ($, e, next) => {
    for (const job of world.pre ?? []) await scan(job)
    return next(e)
  })
  on('tool.call', async () => {
    if (world.toolMs) await sleep(world.toolMs)
    for (const job of world.post ?? []) await scan(job)
    return { result: { stdout: 'ran', stderr: '', interrupted: false } } as never
  })
  /** Resolves once `n` gates have ended their wait, or after `ms`. */
  async function settled(n: number, ms = ANSWER_MS + CLAIM_MS + 500): Promise<void> {
    for (let waited = 0; outcomes.length < n && waited < ms; waited += 20) await sleep(20)
  }
  return { argvs, models, answers, outcomes, take, order, settled, scan }
}

type Hook = ($: unknown, e: unknown, next: unknown) => Promise<unknown>

/** The hook `register` adds on `pattern`, to call with a `next` of the
 *  test's own, whose signal the test aborts. */
function hookOf(pattern: string): Hook {
  let found: Hook | undefined
  const on = (p: string, ...rest: unknown[]) => {
    if (p === pattern) found = rest[rest.length - 1] as Hook
  }
  register(on as never, {})
  if (!found) throw new Error(`register adds no ${pattern} hook`)
  return found
}

async function typed($: Engine, kind = 'composer') {
  return $.prompt.submit({ text: 'review the release notes', wait: false, origin: { kind } } as never)
}

describe('the prompt lane: a job, one model call, one answer', () => {
  test('the job goes to $.model.complete unchanged, and its text and usage to ways judge answer for that job', async ($, on) => {
    const h = host(on, { prompt: [JOB], model: VERDICT })
    await typed($)
    expect(h.outcomes).toEqual(['answered'])
    expect(h.models.length).toBe(1)
    expect(h.models[0]).toMatchObject({ model: JOB.model, system: JOB.system, prompt: JOB.prompt, maxTokens: JOB.max_tokens, timeoutMs: JOB.timeout_ms })
    expect(h.models[0]).not.toHaveProperty('id')
    expect(h.answers).toEqual([{ text: '{"judgements":[]}', usage: USAGE }])
    expect(h.argvs).toContainEqual(hostArgv(ID))
    expect(h.argvs).toContainEqual(takeArgv(ID))
    expect(h.argvs).toContainEqual(['ways', 'judge', 'answer', '--session', ID, '--job', 'j-1'])
  })

  test('a model call the engine refuses to send answers the refusal', async ($, on) => {
    const h = host(on, { prompt: [JOB], modelThrows: true })
    await typed($)
    expect(h.outcomes).toEqual(['answered'])
    expect(h.answers).toEqual([REFUSED])
  })

  test('a model call past its time limit answers the reason and no text', async ($, on) => {
    const h = host(on, { prompt: [JOB], model: { isAnswered: false, reason: 'aborted', usage: ZERO } })
    await typed($)
    expect(h.models.length).toBe(1)
    expect(h.answers).toEqual([{ reason: 'aborted', usage: ZERO }])
    expect((h.answers[0] as { text?: string }).text).toBeUndefined()
  })

  // The settings hook that writes the job cannot see a prompt's origin, so
  // the mod takes for every prompt, not only the operator's.
  for (const kind of ['plugin', 'task-notification', 'peer']) {
    test(`a ${kind} prompt takes a job and judges it`, async ($, on) => {
      const h = host(on, { prompt: [JOB], model: { isAnswered: true, text: 'verdict', usage: USAGE } })
      await typed($, kind)
      expect(h.outcomes).toEqual(['answered'])
      expect(h.models.length).toBe(1)
      expect(h.answers).toEqual([{ text: 'verdict', usage: USAGE }])
    })
  }

  // The prompt and state lanes run as parallel settings hooks, and each
  // writes its own job: the second is claimed while the first is judged.
  test('two jobs written at once are both claimed within the window and judged', async ($, on) => {
    const h = host(on, { prompt: [JOB, JOB_2], model: VERDICT, modelMs: CLAIM_MS + 200 })
    await typed($)
    expect(h.outcomes).toEqual(['answered', 'answered'])
    expect(h.models.length).toBe(2)
  })

  test('the session id is registered as judge host once, before its first take', async ($, on) => {
    const h = host(on, { standby: true })
    await typed($)
    await typed($)
    await $.tool.call({ tool: 'Bash', command: 'ls' })
    const hosts = h.argvs.filter(a => a.slice(1, 3).join(' ') === 'judge host')
    expect(hosts.length).toBe(1)
    expect(h.argvs.findIndex(a => a.slice(1, 3).join(' ') === 'judge host')).toBeLessThan(
      h.argvs.findIndex(a => a.slice(1, 3).join(' ') === 'judge take'),
    )
    expect(h.take.started).toBe(3)
  })
})

describe('the tool lanes: a job from a tool call is judged', () => {
  test('a Bash call whose PreToolUse hook writes a job has it claimed, judged and answered', async ($, on) => {
    const h = host(on, { pre: [JOB], model: VERDICT })
    const r = await $.tool.call({ tool: 'Bash', command: 'git status' })
    expect(r.deny).toBeUndefined()
    expect(h.outcomes).toEqual(['answered'])
    expect(h.models.length).toBe(1)
    expect(h.answers).toEqual([{ text: '{"judgements":[]}', usage: USAGE }])
    expect(h.argvs).toContainEqual(takeArgv(ID))
  })

  test("a subagent's call is served under the session's id", async ($, on) => {
    const h = host(on, { pre: [JOB], model: VERDICT })
    await $.tool.call({ tool: 'Edit', file_path: '/p/a.rs', old_string: 'a', new_string: 'b', agentId: 'a1' } as never)
    expect(h.outcomes).toEqual(['answered'])
    expect(h.models.length).toBe(1)
    expect(h.argvs).toContainEqual(takeArgv(ID))
  })

  test('a job before the tool runs and one after it are each judged', async ($, on) => {
    const h = host(on, { pre: [JOB], post: [JOB_2], model: VERDICT })
    await $.tool.call({ tool: 'Bash', command: 'git status' })
    expect(h.outcomes).toEqual(['answered', 'answered'])
    expect(h.models.map(m => (m as { prompt: string }).prompt)).toEqual([JOB.prompt, JOB_2.prompt])
  })

  test('a call that writes no job stops its take once the call settles', async ($, on) => {
    const h = host(on, {})
    await $.tool.call({ tool: 'Bash', command: 'ls' })
    expect(h.take.started).toBe(1)
    expect(h.take.stopped).toBe(1)
    expect(h.models.length).toBe(0)
  })

  // Hooking a tool turns off its move to the background and its live
  // output, so only the tools the settings hooks judge are hooked.
  test('a tool the settings hooks do not judge is not hooked: no host, no take', async ($, on) => {
    const h = host(on, { pre: [JOB], model: VERDICT })
    await $.tool.call({ tool: 'Read', file_path: '/p/a.rs' } as never)
    expect(h.argvs).toEqual([])
    expect(h.take.started).toBe(0)
  })
})

describe('one call and another call\'s job', () => {
  // A take claims the session's oldest job, so a call's take can claim a
  // job another call wrote. The claiming call does not wait for that judge.
  test('a call whose take claims another call\'s job settles first, and that job is answered by its id', async ($, on) => {
    const h = host(on, { pre: [JOB], promptMs: 200, model: VERDICT, modelMs: 600 })
    const a = typed($).then(() => h.order.push('A settled'))
    await sleep(50)
    const b = $.tool.call({ tool: 'Bash', command: 'git status' })
    await Promise.all([a, b])
    expect(h.outcomes).toEqual(['answered'])
    expect(h.order).toEqual(['A settled', 'answer j-1'])
    expect(h.argvs).toContainEqual(['ways', 'judge', 'answer', '--session', ID, '--job', 'j-1'])
  })

  // The test's engine cannot abort an event's signal beneath the mod, so
  // this one calls the mod's hook with an engine and a `next` of its own.
  test('an event aborted while its claimed job is judged still answers the job', async () => {
    const answers: unknown[] = []
    let takes = 0
    const engine = {
      session: { id: async () => ID },
      ui: { log: async () => undefined },
      clock: { now: async () => 0, sleep: (ms: number) => sleep(ms) },
      process: {
        run: async (argv: string[], init?: { stdin?: string }) => {
          if (argv[2] === 'answer') answers.push(JSON.parse(init?.stdin ?? 'null'))
          return ok()
        },
        // The first take hands over the job; the next finds none.
        spawn: () =>
          (async function* () {
            takes += 1
            if (takes === 1) yield { stream: 'stdout' as const, text: JSON.stringify({ ...JOB, id: 'j-1' }) + '\n' }
            else await sleep(20)
            return { code: 0, signal: null }
          })(),
      },
      // A call whose signal aborted before the reply resolves `aborted`.
      model: {
        complete: async (_: unknown, options?: { signal?: AbortSignal }) => {
          await sleep(300)
          return options?.signal?.aborted ? { isAnswered: false, reason: 'aborted', usage: ZERO } : VERDICT
        },
      },
    }
    const stop = new AbortController()
    const interrupted = new Promise(resolve => stop.signal.addEventListener('abort', () => resolve({ result: 'interrupted' })))
    const next = Object.assign(() => interrupted, { signal: stop.signal })
    const call = hookOf('tool.call')(engine, { tool: 'Bash', command: 'git status' }, next)
    await sleep(100)
    stop.abort()
    await call
    for (let waited = 0; answers.length === 0 && waited < 2000; waited += 20) await sleep(20)
    expect(answers).toEqual([{ text: '{"judgements":[]}', usage: USAGE }])
  })

  test('a take stopped as it claims hands over the job it printed, which is answered', async ($, on) => {
    const h = host(on, { trailing: [JOB], claimOnStop: true, model: VERDICT })
    await typed($)
    await h.settled(1)
    expect(h.take.claimed).toBe(1)
    expect(h.outcomes).toEqual(['answered'])
    expect(h.answers).toEqual([{ text: '{"judgements":[]}', usage: USAGE }])
  })
})

describe('a take that ends with no job', () => {
  test('a take that waited out its time is started again while the call runs', async ($, on) => {
    const h = host(on, { idleTakes: true, toolMs: 200 })
    await $.tool.call({ tool: 'Bash', command: 'make' })
    expect(h.take.started).toBeGreaterThan(2)
  })

  test('a take that came back empty at once is not started again', async ($, on) => {
    const h = host(on, { fastEmpty: true, toolMs: 200 })
    await $.tool.call({ tool: 'Bash', command: 'make' })
    expect(h.take.started).toBe(1)
  })
})

describe('no job: no model call', () => {
  test('standby calls no model and writes no answer', async ($, on) => {
    const h = host(on, { standby: true, model: VERDICT })
    await typed($)
    expect(h.take.started).toBe(1)
    expect(h.models.length).toBe(0)
    expect(h.answers.length).toBe(0)
  })

  test('the take is stopped with return() once next(e) settles', async ($, on) => {
    const h = host(on, {})
    await typed($)
    expect(h.take.started).toBe(1)
    expect(h.take.stopped).toBe(1)
    expect(h.models.length).toBe(0)
  })

  test('a failed host registration starts no take', async ($, on) => {
    const h = host(on, { hostFails: true, prompt: [JOB], pre: [JOB], model: VERDICT })
    await typed($)
    await $.tool.call({ tool: 'Bash', command: 'ls' })
    expect(h.take.started).toBe(0)
    expect(h.models.length).toBe(0)
    expect(h.outcomes).toEqual(['unclaimed', 'unclaimed'])
  })
})

describe('the parts', () => {
  test('parseTake reads a job, standby, nothing, and a failure', () => {
    expect(parseTake(JSON.stringify(JOB), 0)).toEqual({ kind: 'job', job: JOB })
    expect(parseTake('standby\n', 0)).toEqual({ kind: 'standby' })
    expect(parseTake('', 0)).toEqual({ kind: 'idle' })
    expect(parseTake('', 1).kind).toBe('none')
    expect(parseTake('', null).kind).toBe('none')
    expect(parseTake('{"model":"x"}', 0).kind).toBe('none')
    expect(parseTake('not json', 0).kind).toBe('none')
    // A take killed once it printed a whole job line hands that job over.
    expect(parseTake(JSON.stringify(JOB) + '\n', null)).toEqual({ kind: 'job', job: JOB })
    expect(parseTake(JSON.stringify(JOB), null).kind).toBe('none')
  })

  test('requestOf and answerOf carry the fields across unchanged', () => {
    expect(requestOf(JOB)).toEqual({ model: JOB.model, system: JOB.system, prompt: JOB.prompt, maxTokens: 224, timeoutMs: 2000 })
    expect(answerOf({ isAnswered: true, text: 't', usage: USAGE })).toEqual({ text: 't', usage: USAGE })
    expect(answerOf({ isAnswered: false, reason: 'api-error', status: 529, error: 'overloaded', usage: ZERO } as never)).toEqual({
      reason: 'api-error',
      usage: ZERO,
    })
  })
})

describe('what the mod hooks reaches it on every host', () => {
  test('no hook is on an event an organization\'s policy plugin passes over, and none is classic', () => {
    const regs = registrationsOf(register as never)
    // The recording saw the module's hooks: the control for the empty lists below.
    expect(regs.map(r => r.pattern)).toEqual(['prompt.submit', 'tool.call', 'session.end'])
    expect(regs.find(r => r.pattern === 'tool.call')?.matcher).toEqual({ tool: [...JUDGED_TOOLS] })
    expect(regs.filter(r => r.pattern.startsWith('classic.'))).toEqual([])
    expect(passedOverHits(regs)).toEqual([])
  })
})
