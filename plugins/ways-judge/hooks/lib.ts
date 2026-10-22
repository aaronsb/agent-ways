// The ways-judge mod's plain parts: the ways CLI's argv, what `ways judge
// take` prints, and the answer `ways judge answer` reads (ADR-708 §4, §5).
// The shapes mirror JudgeJob and JudgeAnswer in
// tools/ways-agent-core/src/protocol.rs.

import type { ModelCompleteRequest, ModelCompleteResult, ModelUsage } from 'claude-code'

export const WAYS = 'ways'

/** Registers the mod as the session's judge host, once per session id. */
export function hostArgv(id: string): string[] {
  return [WAYS, 'judge', 'host', '--session', id]
}

/** Waits for the session's next job; prints it, or `standby`. */
export function takeArgv(id: string): string[] {
  return [WAYS, 'judge', 'take', '--session', id, '--wait']
}

/** Reads a JudgeAnswer as JSON on stdin, for the job `take` printed when it
 *  carried an id, so an answer can never land on a newer job. */
export function answerArgv(id: string, job?: string): string[] {
  const argv = [WAYS, 'judge', 'answer', '--session', id]
  return job ? [...argv, '--job', job] : argv
}

/** How long the host and answer verbs may run. Each is a file write. */
export const VERB_TIMEOUT_MS = 5_000

/** How long stopping `take` may hold the prompt once `next(e)` settled. */
export const STOP_GRACE_MS = 3_000

/** A judge request, as `ways judge take` prints it. */
export type JudgeJob = {
  id?: string
  model: string
  system: string
  prompt: string
  max_tokens: number
  timeout_ms: number
}

/** What `ways judge answer` reads: the model's text and usage, or why there
 *  is none. The mod never reads the verdict in the text. */
export type JudgeAnswer = {
  text?: string
  usage?: ModelUsage
  reason?: string
}

export type Take = { kind: 'job'; job: JudgeJob } | { kind: 'standby' } | { kind: 'none'; why: string }

/** What `take` printed and how it exited: a job, `standby`, or neither. */
export function parseTake(stdout: string, code: number | null): Take {
  const out = stdout.trim()
  if (code !== 0) return { kind: 'none', why: `take exited ${code ?? 'on a signal'}` }
  if (out === 'standby') return { kind: 'standby' }
  let v: unknown
  try {
    v = JSON.parse(out)
  } catch {
    return { kind: 'none', why: `take printed neither a job nor standby: ${out.slice(0, 80)}` }
  }
  return isJob(v) ? { kind: 'job', job: v } : { kind: 'none', why: 'take printed a job without its fields' }
}

function isJob(v: unknown): v is JudgeJob {
  if (typeof v !== 'object' || v === null) return false
  const j = v as Record<string, unknown>
  return (
    typeof j.model === 'string' &&
    typeof j.system === 'string' &&
    typeof j.prompt === 'string' &&
    typeof j.max_tokens === 'number' &&
    typeof j.timeout_ms === 'number'
  )
}

/** The job as `$.model.complete` takes it, unchanged. */
export function requestOf(job: JudgeJob): ModelCompleteRequest {
  return { model: job.model, system: job.system, prompt: job.prompt, maxTokens: job.max_tokens, timeoutMs: job.timeout_ms }
}

/** The model's result as the answer carries it: text and usage when it
 *  answered, else the reason (`aborted` for a call past `timeoutMs`). */
export function answerOf(r: ModelCompleteResult): JudgeAnswer {
  return r.isAnswered ? { text: r.text, usage: r.usage } : { reason: r.reason, usage: r.usage }
}

/** The answer for a call the engine refused to send (a blocked model, a
 *  cap or a time limit it does not take). */
export const REFUSED: JudgeAnswer = { reason: 'error' }
