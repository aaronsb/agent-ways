// What Claude Code's built-in policy plugin (cc-plugin-sec-default) sends
// past the plugins a person installs, where an organization manages Claude
// Code (Team and Enterprise, or a machine with managed settings). A hook a
// mod has there does not run, and Claude Code tells the session so, once per
// event: `<mod>: its <event> hook was skipped. "cc-plugin-sec-default" runs
// first and sent the event on past it.` The lists are the hooks the policy
// plugin registers with `next.to(e, "append")` in Claude Code 2.1.318, the
// same as in 2.1.316. `claude plugin validate` reads a mod alone and cannot
// see them, so this check is the place they are written down.

/** Passed over every time, besides every classic.* event. */
export const PASSED_OVER_EVENTS = [
  'prompt.section',
  'prompt.context',
  'prompt.attachment',
  'prompt.compose',
  'skill.prompt',
  'attribution.text',
  'session.compact',
  'settings.read',
] as const

/** The session.append doors passed over for every row. */
export const PASSED_OVER_DOORS = ['attachment', 'hook-context', 'compaction'] as const

/** The session.append doors passed over for some rows: `tool-message` for
 *  the Skill tool's, `note` for a note a managed plugin appends. */
export const SOMETIMES_PASSED_OVER_DOORS = ['tool-message', 'note'] as const

/** Passed over for a tool, command or agent that a managed plugin or a
 *  managed MCP server provides; run for the user, builtin and core tiers. */
export const PROVIDER_GATED_EVENTS = ['tool.describe', 'command.describe', 'agent.offer', 'agent.spawn'] as const

/** Passed over when a managed plugin makes the call. */
export const CALLER_GATED_EVENTS = ['tool.register', 'ui.log', 'env.get', 'env.set'] as const

/** One registration as a hooks module made it: `on(pattern, matcher?, hook)`. */
export type Registration = { pattern: string; matcher?: unknown }

/** Whether an event pattern reaches `event`, as the engine reads it: an
 *  exact name, `*`, a namespace glob (`classic.*`), or a negation of one. */
export function reaches(pattern: string, event: string): boolean {
  if (pattern.startsWith('!')) return !reaches(pattern.slice(1), event)
  if (pattern === '*') return true
  if (pattern.endsWith('.*')) return event.startsWith(pattern.slice(0, -1))
  return pattern === event
}

/** A matcher's value for `key` as a list, or undefined where it names none. */
function named(matcher: unknown, key: string): unknown[] | undefined {
  const value = (matcher as Record<string, unknown> | undefined)?.[key]
  return value === undefined ? undefined : Array.isArray(value) ? value : [value]
}

/** Whether a tool.describe hook's matcher names built-in tools alone. A name
 *  without the `mcp__` prefix is taken as a built-in tool. */
function describesBuiltInsOnly(r: Registration): boolean {
  if (r.pattern !== 'tool.describe') return false
  const tools = named(r.matcher, 'tool')
  return tools !== undefined && tools.every(t => typeof t === 'string' && !t.startsWith('mcp__'))
}

/** The registrations the policy plugin would pass over, every time or for
 *  some events, as `pattern` or `pattern{door=...}`. A session.append hook
 *  with no door matcher, or one whose door matcher (a name, a RegExp, or a
 *  list of them) takes a passed-over door, counts. */
export function passedOverHits(regs: readonly Registration[]): string[] {
  const counted = [...PASSED_OVER_EVENTS, ...PROVIDER_GATED_EVENTS, ...CALLER_GATED_EVENTS]
  const doorsPassed: readonly string[] = [...PASSED_OVER_DOORS, ...SOMETIMES_PASSED_OVER_DOORS]
  const hits: string[] = []
  for (const r of regs) {
    const isClassic = r.pattern.startsWith('classic.') || reaches(r.pattern, 'classic.SessionStart')
    if (isClassic || counted.some(e => reaches(r.pattern, e) && !describesBuiltInsOnly({ ...r, pattern: e }))) {
      hits.push(r.pattern)
      continue
    }
    if (!reaches(r.pattern, 'session.append')) continue
    const doors = named(r.matcher, 'door')
    const passed = doorsPassed.filter(d => doors === undefined || doors.some(m => (m instanceof RegExp ? ((m.lastIndex = 0), m.test(d)) : m === d)))
    if (passed.length > 0) hits.push(`${r.pattern}{door=${passed.join('|')}}`)
  }
  return hits
}

/** Runs a hooks module's `register` with an `on` that records what it
 *  hooks, and returns the registrations. */
export function registrationsOf(register: (on: never) => unknown): Registration[] {
  const regs: Registration[] = []
  const on = (pattern: string, ...rest: unknown[]) => {
    regs.push({ pattern, matcher: rest.length > 1 ? rest[0] : undefined })
  }
  register(on as never)
  return regs
}
