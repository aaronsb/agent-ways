// The relevance gate's Claude Code internal engine (ADR-708).
//
// The prompt lane's gate runs in the settings UserPromptSubmit hooks, which
// `next(e)` runs at `prompt.submit`. Under `gate.engine: claude-code` the
// ways scan there writes the judge request as a job in the session's
// directory and waits for the answer. This module carries the job: it
// starts `next(e)` without awaiting it and, alongside, waits on
// `ways judge take`. On a job it sends the request through
// `$.model.complete` unchanged and hands the reply's text and usage, or the
// failure reason, to `ways judge answer`. The hook side parses and
// validates the reply; this module never reads it. On `standby` (the
// session resolved to a key engine) it calls no model. When `next(e)`
// settles first, the take is stopped with the stream's `return()`, since
// `$.process.run` has no abort (ADR-708 §5, ADR-709).
//
// The module registers itself as the session's judge host before each
// session id's first prompt reaches `next(e)`, not at `session.start`: a
// /clear or an in-session /resume changes the id with no `session.start`
// (§3). It hooks no classic.* event, which an organization's policy plugin
// sends past the plugins a person installs.

import type { EngineInterface, HookStream, ProcessSpawnChunk, ProcessSpawnResult, PromptSubmitInput, PromptSubmitResult, Register } from 'claude-code'

import {
  REFUSED,
  STOP_GRACE_MS,
  VERB_TIMEOUT_MS,
  answerArgv,
  answerOf,
  hostArgv,
  parseTake,
  requestOf,
  takeArgv,
} from './lib.ts'
import type { JudgeAnswer, JudgeJob } from './lib.ts'

type Next = (e: PromptSubmitInput) => Promise<PromptSubmitResult>
type TakeStream = HookStream<ProcessSpawnChunk, ProcessSpawnResult>

const debug = ($: EngineInterface, text: string) => $.ui.log(`ways-judge: ${text}`, { to: 'debug' })

export const register: Register = on => {
  // Each session id this module has registered as judge host, and whether
  // `ways judge host` took it. A reload starts the map over and registers
  // again, which the verb takes as the same registration.
  const hosted = new Map<string, boolean>()

  // Every prompt, whatever its origin: the settings hook that writes the job
  // cannot see the origin, so the mod takes for each one. A take with no
  // job costs one spawn, stopped when next(e) settles.
  on('prompt.submit', async ($, e, next) => {
    let id: string
    try {
      id = await $.session.id()
    } catch (err) {
      debug($, `no session id, so no judge for this prompt: ${String(err)}`)
      return next(e)
    }
    if (!(await host($, hosted, id))) return next(e)
    return bracket($, id, e, next)
  })
}

/** Registers the session id as judge host once: a host that registers
 *  after the session resolved stays idle until the next session (§2), so
 *  a failure is not retried under the same id. */
async function host($: EngineInterface, hosted: Map<string, boolean>, id: string): Promise<boolean> {
  const known = hosted.get(id)
  if (known !== undefined) return known
  let isHost = false
  try {
    const r = await $.process.run(hostArgv(id), { timeoutMs: VERB_TIMEOUT_MS })
    isHost = r.exitCode === 0
    if (!isHost) debug($, `ways judge host failed (${r.exitCode}): ${r.stderr.trim()}`)
  } catch (err) {
    debug($, `ways judge host did not run: ${String(err)}`)
  }
  hosted.set(id, isHost)
  return isHost
}

/** Runs `next(e)` with `ways judge take` beside it, and serves the job the
 *  take hands over. Resolves to what `next(e)` resolved to. */
async function bracket($: EngineInterface, id: string, e: PromptSubmitInput, next: Next): Promise<PromptSubmitResult> {
  const settled = next(e)
  const take = $.process.spawn({ argv: takeArgv(id) })
  const stop = new AbortController()
  const served = serve($, id, take, stop.signal)
  try {
    return await settled
  } finally {
    // The prompt went on: a take still waiting has nothing to wait for, and
    // a model call still running has no reader.
    stop.abort()
    await Promise.race([take.return(undefined as never).catch(() => undefined), $.clock.sleep(STOP_GRACE_MS)])
    await served
  }
}

/** Reads the take to its end; on a job, judges it and writes the answer. */
async function serve($: EngineInterface, id: string, take: TakeStream, stopped: AbortSignal): Promise<void> {
  let out = ''
  let code: number | null = null
  try {
    for (;;) {
      const r = await take.next()
      if (r.done) {
        code = r.value?.code ?? null
        break
      }
      if (r.value.stream === 'stdout') out += r.value.text
    }
  } catch (err) {
    if (!stopped.aborted) debug($, `ways judge take did not run: ${String(err)}`)
    return
  }
  if (stopped.aborted) return
  const t = parseTake(out, code)
  if (t.kind === 'none') debug($, t.why)
  if (t.kind !== 'job') return
  const answer = await judge($, t.job, stopped)
  try {
    const w = await $.process.run(answerArgv(id, t.job.id), { stdin: JSON.stringify(answer), timeoutMs: VERB_TIMEOUT_MS })
    if (w.exitCode !== 0) debug($, `ways judge answer failed (${w.exitCode}): ${w.stderr.trim()}`)
  } catch (err) {
    debug($, `ways judge answer did not run: ${String(err)}`)
  }
}

async function judge($: EngineInterface, job: JudgeJob, stopped: AbortSignal): Promise<JudgeAnswer> {
  try {
    return answerOf(await $.model.complete(requestOf(job), { signal: stopped }))
  } catch (err) {
    debug($, `the model call was refused: ${String(err)}`)
    return REFUSED
  }
}
