// The mod's options, read from ways settings (ADR-517 item 5): `mods.images.*`
// at user, target or project scope, the project winning, as `ways settings
// list mods.images --json --effective --project <root>` resolves them. They
// are read at an interactive session's start, and again before a picture is
// drawn when the last read is over OPTIONS_MAX_AGE_MS old; nothing polls, so
// a session that draws nothing calls `ways` once. A read is bounded at
// WAYS_TIMEOUT_MS. A missing or failing `ways`, or output that does not
// parse, keeps the last good options, the defaults before there are any, and
// writes one debug line per distinct reason.

import type { EngineInterface } from 'claude-code'

import { DEFAULT_OPTIONS, OPTIONS_MAX_AGE_MS, WAYS_TIMEOUT_MS, optionsArgv, parseOptions } from './lib.ts'
import type { Options } from './lib.ts'
import { run } from './run.ts'

// Per load: a reload starts these over.
let options: Options = DEFAULT_OPTIONS
let readAt = -Infinity
let reading: Promise<void> | null = null
/** Why the last read failed, so a repeated failure logs once. */
let lastFailure: string | undefined

/** The options as last read. */
export const currentOptions = (): Options => options

/** The clock's time; 0 where it does not answer, so the options are read once. */
function now($: EngineInterface): Promise<number> {
  return $.clock.now().catch(() => 0)
}

async function read($: EngineInterface): Promise<void> {
  readAt = await now($)
  const project = await $.session.root().catch(() => undefined)
  const argv = optionsArgv(project)
  const ran = await run($, argv, { timeoutMs: WAYS_TIMEOUT_MS, isOnce: true })
  const got =
    ran === null
      ? `\`${argv.join(' ')}\` did not run`
      : ran.exitCode !== 0
        ? `\`${argv.join(' ')}\` exited ${ran.exitCode}`
        : parseOptions(ran.stdout)
  if (typeof got !== 'string') {
    options = got
    lastFailure = undefined
    return
  }
  if (got === lastFailure) return
  lastFailure = got
  $.ui.log(`images: kept the options as they were: ${got}`, { to: 'debug' })
}

/** Reads the options now; renders that arrive meanwhile wait on the same read. */
export function readOptions($: EngineInterface): Promise<void> {
  reading ??= read($)
    .catch(() => undefined)
    .finally(() => {
      reading = null
    })
  return reading
}

/** Reads the options again when the last read is over OPTIONS_MAX_AGE_MS old. */
export async function freshOptions($: EngineInterface): Promise<Options> {
  if (reading === null && (await now($)) - readAt > OPTIONS_MAX_AGE_MS) await readOptions($)
  else if (reading !== null) await reading
  return options
}
