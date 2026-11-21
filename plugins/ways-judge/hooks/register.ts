// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

// The relevance gate's Claude Code internal engine.
//
// The gate runs in the settings hooks: the prompt and state lanes in the
// UserPromptSubmit hooks, which `next(e)` runs at `prompt.submit`, and the
// Bash, file, task, tool and post-tool lanes in the PreToolUse,
// PostToolUse and PostToolUseFailure hooks, which `next(e)` runs at
// `tool.call` for the main loop and for every subagent's loop alike. Under
// `gate.engine: claude-code` the ways scan there writes each judge request
// as a job in the session's directory and waits for its answer, and fails
// open when no host claims the job within half a second.
//
// This module carries the jobs. At each of the two events it starts
// `next(e)` without awaiting it and, while that runs, keeps a
// `ways judge take` waiting beside it. On a job it starts the next take at
// once, so a second job written meanwhile (a second batch, a second
// settings hook, another loop's call) is claimed within the window, sends
// the request through `$.model.complete` unchanged and hands the reply's
// text and usage, or the failure reason, to `ways judge answer`. The hook
// side parses and validates the reply; this module never reads it. On
// `standby` (the session resolved to a key engine) it calls no model. When
// `next(e)` settles, the waiting take is stopped with the stream's
// `return()`, since `$.process.run` has no abort. A job already claimed is
// still judged and answered: any waiting take claims the session's oldest
// job, so the gate waiting on it may be another call's.
//
// The module registers itself as the session's judge host before each
// session id's first event reaches `next(e)`, not at `session.start`: a
// /clear or an in-session /resume changes the id with no `session.start`.
// A subagent's calls carry the session's id, so one registration serves
// every loop. It hooks no classic.* event, which an organization's policy
// plugin sends past the plugins a person installs.
//
// References:
// - ADR-708 §3, §5: the host registration, the job handoff, and the
//   bracket around `next(e)` with its stop by `return()`.
// - ADR-712: every lane and agent thread writes judge jobs.

import type { EngineInterface, HookStream, ProcessSpawnChunk, ProcessSpawnResult, Register } from 'claude-code'

import { REFUSED, STOP_GRACE_MS, TAKE_WAIT_MS, VERB_TIMEOUT_MS, answerArgv, answerOf, hostArgv, parseTake, requestOf, takeArgv } from './lib.ts'
import type { JudgeAnswer, JudgeJob, Take } from './lib.ts'

type TakeStream = HookStream<ProcessSpawnChunk, ProcessSpawnResult>

const debug = ($: EngineInterface, text: string) => $.ui.log(`ways-judge: ${text}`, { to: 'debug' })

export const register: Register = on => {
  // Each session id this module has registered as judge host, and whether
  // `ways judge host` took it. A reload starts the map over and registers
  // again, which the verb takes as the same registration.
  const hosted = new Map<string, boolean>()

  // Every prompt, whatever its origin: the settings hook that writes the job
  // cannot see the origin, so the mod takes for each one.
  on('prompt.submit', async ($, e, next) => {
    const id = await served($, hosted)
    return id === undefined ? next(e) : bracket($, id, next(e), next.signal)
  })

  // Every tool, in every loop: which calls write a job is settled by the
  // settings hooks' matchers, which the mod cannot read. A call with no job
  // costs one take, stopped when next(e) settles.
  on('tool.call', async ($, e, next) => {
    const id = await served($, hosted)
    return id === undefined ? next(e) : bracket($, id, next(e), next.signal)
  })
}

/** The session id to serve, or none: no id, or no host registration. */
async function served($: EngineInterface, hosted: Map<string, boolean>): Promise<string | undefined> {
  let id: string
  try {
    id = await $.session.id()
  } catch (err) {
    debug($, `no session id, so no judge here: ${String(err)}`)
    return undefined
  }
  return (await host($, hosted, id)) ? id : undefined
}

/** Registers the session id as judge host once: a host that registers
 *  after the session resolved stays idle until the next session, so a
 *  failure is not retried under the same id. */
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

/** Serves the session's jobs while `settling`, the event's `next(e)`, runs,
 *  and resolves to what it resolved to. `signal` is the event's own: an
 *  interrupted turn aborts a model call still running. */
async function bracket<R>($: EngineInterface, id: string, settling: Promise<R>, signal: AbortSignal): Promise<R> {
  let isSettled = false
  let stopTake: () => Promise<void> = async () => undefined
  const serving = serve($, id, signal, () => isSettled, take => {
    stopTake = async () => {
      await Promise.race([take.return(undefined as never).catch(() => undefined), $.clock.sleep(STOP_GRACE_MS)])
    }
  })
  try {
    return await settling
  } finally {
    // The call went on: a take still waiting has nothing to wait for.
    isSettled = true
    await stopTake()
    await serving
  }
}

/** Keeps one take waiting until `isSettled`, and judges each job a take
 *  hands over while the next take waits. Resolves once every claimed job
 *  is answered. */
async function serve(
  $: EngineInterface,
  id: string,
  signal: AbortSignal,
  isSettled: () => boolean,
  started: (take: TakeStream) => void,
): Promise<void> {
  const answering: Promise<void>[] = []
  while (!isSettled()) {
    // Nothing awaits between the check above and `started`, so a settle
    // always finds the take it has to stop.
    const begun = $.clock.now().catch(() => NaN)
    const take = $.process.spawn({ argv: takeArgv(id) })
    started(take)
    const t = await read($, take, isSettled)
    if (t.kind === 'job') {
      answering.push(answer($, id, t.job, signal))
      continue
    }
    if (t.kind === 'none' && !isSettled()) debug($, t.why)
    if (t.kind !== 'idle' || isSettled()) break
    // A take that waited its time out with no job is started again; one that
    // came back empty sooner, or with no clock to tell, is not, so a take
    // that fails fast cannot spin.
    const waited = (await $.clock.now().catch(() => NaN)) - (await begun)
    if (!(waited >= TAKE_WAIT_MS / 2)) break
  }
  await Promise.all(answering)
}

/** Reads a take to its end: a job, `standby`, nothing, or a failure. */
async function read($: EngineInterface, take: TakeStream, isSettled: () => boolean): Promise<Take> {
  let out = ''
  try {
    for (;;) {
      const r = await take.next()
      if (r.done) return parseTake(out, r.value?.code ?? null)
      if (r.value.stream === 'stdout') out += r.value.text
    }
  } catch (err) {
    // A stopped take may end by rejecting; only an unstopped one is news.
    if (!isSettled()) debug($, `ways judge take did not run: ${String(err)}`)
    return parseTake(out, null)
  }
}

/** Judges a claimed job and writes the answer. A claimed job is always
 *  answered, so its gate fails open at once rather than at its deadline. */
async function answer($: EngineInterface, id: string, job: JudgeJob, signal: AbortSignal): Promise<void> {
  const a = await judge($, job, signal)
  try {
    const w = await $.process.run(answerArgv(id, job.id), { stdin: JSON.stringify(a), timeoutMs: VERB_TIMEOUT_MS })
    if (w.exitCode !== 0) debug($, `ways judge answer failed (${w.exitCode}): ${w.stderr.trim()}`)
  } catch (err) {
    debug($, `ways judge answer did not run: ${String(err)}`)
  }
}

async function judge($: EngineInterface, job: JudgeJob, signal: AbortSignal): Promise<JudgeAnswer> {
  try {
    return answerOf(await $.model.complete(requestOf(job), { signal }))
  } catch (err) {
    debug($, `the model call was refused: ${String(err)}`)
    return REFUSED
  }
}
