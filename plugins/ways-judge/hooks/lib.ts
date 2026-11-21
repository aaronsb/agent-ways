// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

// The ways-judge mod's plain parts: the ways CLI's argv, what `ways judge
// take` prints, and the answer `ways judge answer` reads. The shapes mirror
// JudgeJob and JudgeAnswer in tools/ways-agent-core/src/protocol.rs.
//
// References:
// - ADR-708 §4, §5: the job and answer the verbs carry.

import type { ModelCompleteRequest, ModelCompleteResult, ModelUsage } from 'claude-code'

export const WAYS = 'ways'

/** Registers the mod as the session's judge host, once per session id. */
export function hostArgv(id: string): string[] {
  return [WAYS, 'judge', 'host', '--session', id]
}

/** How long one take may wait: the prompt hook's own timeout, so a take
 *  whose stop was missed cannot outlive the event that started it by more.
 *  The mod starts another while the event's `next(e)` still runs. */
export const TAKE_WAIT_MS = 30_000

/** Waits for the session's next job; prints it, or `standby`. */
export function takeArgv(id: string): string[] {
  return [WAYS, 'judge', 'take', '--session', id, '--wait', '--timeout-ms', String(TAKE_WAIT_MS)]
}

/** Reads a JudgeAnswer as JSON on stdin, for the job `take` printed when it
 *  carried an id, so an answer can never land on a newer job. */
export function answerArgv(id: string, job?: string): string[] {
  const argv = [WAYS, 'judge', 'answer', '--session', id]
  return job ? [...argv, '--job', job] : argv
}

/** How long the host and answer verbs may run. Each is a file write. */
export const VERB_TIMEOUT_MS = 5_000

/** How long stopping `take` may hold the event once `next(e)` settled. */
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

export type Take = { kind: 'job'; job: JudgeJob } | { kind: 'standby' } | { kind: 'idle' } | { kind: 'none'; why: string }

/** What `take` printed and how it exited: a job, `standby`, nothing (its
 *  wait ran out with no job, or it was stopped), or a failure. */
export function parseTake(stdout: string, code: number | null): Take {
  const out = stdout.trim()
  if (code !== 0) return { kind: 'none', why: `take exited ${code ?? 'on a signal'}` }
  if (out === '') return { kind: 'idle' }
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
