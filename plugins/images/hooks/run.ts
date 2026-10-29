// The mod's children, one at a time, from `/`: the host refuses a child
// started while another runs, and a session's working directory can vanish
// under it (a removed worktree or temp dir), which fails every spawn that
// inherits it.

import type { EngineInterface, ProcessRunResult } from 'claude-code'

const RUN_TIMEOUT_MS = 15_000

/** The children in flight, one after another: renders arrive in pairs within a millisecond. */
let runs: Promise<unknown> = Promise.resolve()

export type RunOptions = {
  stdin?: string
  /** The child's bound; RUN_TIMEOUT_MS when left out. */
  timeoutMs?: number
  /** Not tried again when refused, so the bound holds for the whole call. */
  isOnce?: boolean
}

/**
 * Runs `argv` from `/`, queued behind the runs before it. A run the host
 * refused is tried once more unless `isOnce`; null after the last refusal,
 * or when the command cannot start.
 */
export function run($: EngineInterface, argv: string[], options: RunOptions = {}): Promise<ProcessRunResult | null> {
  const once = () => $.process.run(argv, { cwd: '/', stdin: options.stdin, timeoutMs: options.timeoutMs ?? RUN_TIMEOUT_MS })
  const done = runs.then(() => (options.isOnce === true ? once() : once().catch(once))).catch(() => null)
  runs = done
  return done
}
