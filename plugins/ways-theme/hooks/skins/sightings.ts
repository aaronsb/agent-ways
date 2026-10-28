// Ported from skins by hellosverre (MIT; LICENSE beside this file),
// https://github.com/hellosverre/claude-skins at 985ad63, hooks/sightings.ts.

// Which cards have been drawn already, so a card's entrance plays on its first draw only.
// Keys are a render instance (a message id, a tool_use_id) and the card's place in it.
// Bounded: the oldest are forgotten first, and a forgotten card only animates once more.
const LIMIT = 2000

export function sightings(limit = LIMIT): (key: string) => boolean {
  const seen = new Set<string>()

  return key => {
    if (seen.has(key)) {
      return false
    }

    seen.add(key)

    if (seen.size > limit) {
      const [oldest] = seen

      if (oldest !== undefined) {
        seen.delete(oldest)
      }
    }

    return true
  }
}
