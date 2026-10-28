// What Claude Code's built-in policy plugin (cc-plugin-sec-default) sends
// past the plugins a person installs, where an organization manages Claude
// Code (Team and Enterprise, or a machine with managed settings). A hook a
// mod has on one of these never runs there. The lists are the events and
// session.append doors the policy plugin registers `next.to(e, "append")`
// on in Claude Code 2.1.306, besides every classic.* event.

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

export const PASSED_OVER_DOORS = ['attachment', 'hook-context', 'compaction'] as const

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

/** The registrations a hook of which the policy plugin would pass over,
 *  as `pattern` or `pattern{door=...}`. A session.append hook with no door
 *  matcher, or one naming a passed-over door, counts. */
export function passedOverHits(regs: readonly Registration[]): string[] {
  const hits: string[] = []
  for (const r of regs) {
    const isClassic = r.pattern.startsWith('classic.') || reaches(r.pattern, 'classic.SessionStart')
    if (isClassic || PASSED_OVER_EVENTS.some(e => reaches(r.pattern, e))) {
      hits.push(r.pattern)
      continue
    }
    if (!reaches(r.pattern, 'session.append')) continue
    const door = (r.matcher as { door?: unknown } | undefined)?.door
    const doors = door === undefined ? [...PASSED_OVER_DOORS] : Array.isArray(door) ? door : [door]
    const passed = doors.filter(d => (PASSED_OVER_DOORS as readonly unknown[]).includes(d))
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
