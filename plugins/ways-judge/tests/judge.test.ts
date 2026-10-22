import { describe, expect, test } from 'claude-code/testing'
import type { Engine } from 'claude-code/testing'
import type { ModelCompleteResult, On, ProcessRunResult } from 'claude-code'

import { register } from '../hooks/register.ts'
import { answerOf, hostArgv, parseTake, requestOf, takeArgv } from '../hooks/lib.ts'
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

const USAGE = { input_tokens: 812, output_tokens: 41, cache_read_input_tokens: 0, cache_creation_input_tokens: 0 }
const ZERO = { input_tokens: 0, output_tokens: 0, cache_read_input_tokens: 0, cache_creation_input_tokens: 0 }

const ok = (stdout = ''): ProcessRunResult => ({ exitCode: 0, stdout, stderr: '', isStdoutTruncated: false, isStderrTruncated: false })

type World = {
  /** What `ways judge take` prints, and how it exits; absent, it waits
   *  until it is stopped, as with no job and no key engine. */
  take?: { stdout: string; code?: number }
  /** What `$.model.complete` resolves to. */
  model?: ModelCompleteResult
  /** `ways judge host` fails. */
  hostFails?: boolean
  /** The scan writes no job: the prompt's chain settles at once. */
  noScan?: boolean
}

/** The host beneath the mod: the ways CLI and the model answered from
 *  `world`, every argv, model request and answer recorded. The prompt's
 *  own chain (the settings UserPromptSubmit hooks) settles once the take
 *  has ended and any answer has been written, as the ways scan waits for
 *  the verdict; with a take that waits, it settles at once, as a scan
 *  that matched nothing to judge. */
function host(on: On, world: World) {
  const argvs: string[][] = []
  const models: unknown[] = []
  const answers: unknown[] = []
  const take = { started: 0, ended: 0, stopped: 0 }
  let tookEnd: () => void = () => undefined
  const took = new Promise<void>(resolve => (tookEnd = resolve))
  let answeredEnd: () => void = () => undefined
  const answered = new Promise<void>(resolve => (answeredEnd = resolve))

  on('session.id', () => ({ value: ID }) as never)
  on('ui.log', () => ({ value: undefined }) as never)
  on('process.run', ($, e) => {
    const ev = e as unknown as { argv: string[]; init?: { stdin?: string }; stdin?: string }
    argvs.push([...ev.argv])
    const cmd = ev.argv.slice(1, 3).join(' ')
    if (cmd === 'judge host' && world.hostFails) return { value: { ...ok(), exitCode: 2, stderr: "unrecognized subcommand 'host'" } } as never
    if (cmd === 'judge answer') {
      answers.push(JSON.parse(ev.init?.stdin ?? ev.stdin ?? 'null'))
      answeredEnd()
    }
    return { value: ok() } as never
  })
  on('process.spawn', async function* ($, e, next) {
    argvs.push([...e.argv])
    take.started += 1
    try {
      if (world.take === undefined) {
        await new Promise<void>(resolve => {
          if (next.signal.aborted) resolve()
          next.signal.addEventListener('abort', () => resolve())
        })
        take.stopped += 1
        return { value: { code: null, signal: 'SIGTERM' } } as never
      }
      yield { stream: 'stdout' as const, text: world.take.stdout + '\n' }
      take.ended += 1
      return { value: { code: world.take.code ?? 0, signal: null } } as never
    } finally {
      tookEnd()
    }
  })
  on('model.complete', ($, e) => {
    models.push({ ...e })
    return { value: world.model } as never
  })
  on('prompt.submit', async ($, e) => {
    if (world.take !== undefined && !world.noScan) {
      await took
      if (parseTake(world.take.stdout, world.take.code ?? 0).kind === 'job') await answered
      // On standby the scan goes on to a key engine, which takes its time:
      // the mod has read the take before the chain settles.
      else await new Promise<void>(resolve => setTimeout(resolve, 50))
    }
    return { text: e.text }
  })
  return { argvs, models, answers, take }
}

async function typed($: Engine, kind = 'composer') {
  return $.prompt.submit({ text: 'review the release notes', wait: false, origin: { kind } } as never)
}

describe('a job: one model call, one answer', () => {
  test('the job goes to $.model.complete unchanged, and its text and usage to ways judge answer', async ($, on) => {
    const h = host(on, { take: { stdout: JSON.stringify(JOB) }, model: { isAnswered: true, text: '{"judgements":[]}', usage: USAGE } })
    await typed($)
    expect(h.models.length).toBe(1)
    expect(h.models[0]).toMatchObject({ model: JOB.model, system: JOB.system, prompt: JOB.prompt, maxTokens: JOB.max_tokens, timeoutMs: JOB.timeout_ms })
    expect(h.answers).toEqual([{ text: '{"judgements":[]}', usage: USAGE }])
    expect(h.argvs).toContainEqual(hostArgv(ID))
    expect(h.argvs).toContainEqual(takeArgv(ID))
    expect(h.argvs).toContainEqual(['ways', 'judge', 'answer', '--session', ID])
  })

  test('a model call past its time limit answers the reason and no text', async ($, on) => {
    const h = host(on, { take: { stdout: JSON.stringify(JOB) }, model: { isAnswered: false, reason: 'aborted', usage: ZERO } })
    await typed($)
    expect(h.models.length).toBe(1)
    expect(h.answers).toEqual([{ reason: 'aborted', usage: ZERO }])
    expect((h.answers[0] as { text?: string }).text).toBeUndefined()
  })

  // The settings hook that writes the job cannot see a prompt's origin, so
  // the mod takes for every prompt, not only the operator's.
  for (const kind of ['plugin', 'task-notification', 'peer']) {
    test(`a ${kind} prompt takes a job and judges it`, async ($, on) => {
      const h = host(on, { take: { stdout: JSON.stringify(JOB) }, model: { isAnswered: true, text: 'verdict', usage: USAGE } })
      await typed($, kind)
      expect(h.take.started).toBe(1)
      expect(h.models.length).toBe(1)
      expect(h.answers).toEqual([{ text: 'verdict', usage: USAGE }])
    })
  }

  test('the session id is registered as judge host once, before its first prompt', async ($, on) => {
    const h = host(on, { take: { stdout: 'standby' } })
    await typed($)
    await typed($)
    const hosts = h.argvs.filter(a => a.slice(1, 3).join(' ') === 'judge host')
    expect(hosts.length).toBe(1)
    expect(h.argvs.findIndex(a => a.slice(1, 3).join(' ') === 'judge host')).toBeLessThan(
      h.argvs.findIndex(a => a.slice(1, 3).join(' ') === 'judge take'),
    )
    expect(h.take.started).toBe(2)
  })
})

describe('no job: no model call', () => {
  test('standby calls no model and writes no answer', async ($, on) => {
    const h = host(on, { take: { stdout: 'standby' }, model: { isAnswered: true, text: 'x', usage: USAGE } })
    await typed($)
    expect(h.take.ended).toBe(1)
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
    const h = host(on, { hostFails: true, noScan: true, take: { stdout: JSON.stringify(JOB) }, model: { isAnswered: true, text: 'x', usage: USAGE } })
    await typed($)
    expect(h.take.started).toBe(0)
    expect(h.models.length).toBe(0)
  })
})

describe('the parts', () => {
  test('parseTake reads a job, standby, and neither', () => {
    expect(parseTake(JSON.stringify(JOB), 0)).toEqual({ kind: 'job', job: JOB })
    expect(parseTake('standby\n', 0)).toEqual({ kind: 'standby' })
    expect(parseTake('', 1).kind).toBe('none')
    expect(parseTake('{"model":"x"}', 0).kind).toBe('none')
    expect(parseTake('not json', 0).kind).toBe('none')
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
    // The recording saw the module's hook: the control for the empty lists below.
    expect(regs.map(r => r.pattern)).toEqual(['prompt.submit'])
    expect(regs.filter(r => r.pattern.startsWith('classic.'))).toEqual([])
    expect(passedOverHits(regs)).toEqual([])
  })
})
