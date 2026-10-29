// The wrapper blocks a mod or a peer puts around the text it relays, such as
// attend's delivery:
//
//   The block below is message data relayed by attend.
//   <attend-peer-messages>
//   [project] Nina (vtium) 11:17: ...
//   </attend-peer-messages>
//
// A block is an opening tag alone on its line and the closing tag of its name
// alone on a later line, nothing else on either. The name is lowercase words
// joined by hyphens, at least two of them, so `pasted_content` (pastes.ts)
// and HTML's own one-word tags are never one. The opening tag may carry
// double-quoted attributes, `<teammate-message teammate_id="lead">`, which
// the block keeps as written. A block of the same name inside it nests, so
// its closing tag closes the inner one first.
//
// A tag with anything beside it on its line, an opening tag with no closing
// tag of its name, and a closing tag of another name are text, and the split
// goes on past them: a later whole block is still a block. So is every tag
// inside a fenced code block, from a ``` or ~~~ line to the fence that closes
// it, or to the end of the message when none does: a custom element in a
// relayed snippet, `<my-card>`, stays as written.
//
// The tags are paired in one pass over the lines, so a message costs time in
// proportion to its length, times MAX_DEPTH at most for the blocks inside
// blocks.

const OPENING = /^<([a-z][a-z0-9]*(?:-[a-z0-9]+)+)((?:[ \t]+[A-Za-z_][\w.:-]*="[^"]*")*)[ \t]*>$/
const CLOSING = /^<\/([a-z][a-z0-9]*(?:-[a-z0-9]+)+)>$/
/** A fence line: up to three spaces, then three or more backticks or tildes;
 *  a backtick fence's info string holds no backtick. */
const FENCE = /^ {0,3}(`{3,}|~{3,})(.*)$/
/** A quick test before the split: a line that starts a hyphenated tag. */
const MAY_OPEN = /(?:^|\n)<[a-z][a-z0-9]*-/
/** How many levels of block are drawn as blocks, the outermost the first; a
 *  block nested deeper draws as its text. */
export const MAX_DEPTH = 4

/** A stretch of the message: text, or a whole block, its tag lines in `raw`
 *  alone and its body split again in `inner`. */
export type Part = { kind: 'text'; text: string } | { kind: 'block'; name: string; attributes: string; body: string; raw: string; inner: Part[] }

/** Whether `text` may hold a block, so that drawing it needs the split. */
export const holdsBlock = (text: string): boolean => MAY_OPEN.test(text)

/** `text` cut into text stretches and whole blocks, line by line, to
 *  MAX_DEPTH levels. The line breaks either side of a block belong to
 *  neither. */
export function parts(text: string): Part[] {
  const lines = text.split('\n')
  return split(lines, pairs(lines), 0, lines.length, 1)
}

/** For each opening line that a closing line of its name closes, that
 *  closing line; tags inside a fence pair with nothing. */
function pairs(lines: readonly string[]): Map<number, number> {
  const closes = new Map<number, number>()
  const open = new Map<string, number[]>()
  let fence: string | null = null
  lines.forEach((line, at) => {
    const mark = FENCE.exec(line)
    if (fence !== null) {
      if (mark !== null && mark[1]![0] === fence[0] && mark[1]!.length >= fence.length && mark[2]!.trim() === '') fence = null
      return
    }
    if (mark !== null && !(mark[1]![0] === '`' && mark[2]!.includes('`'))) {
      fence = mark[1]!
      return
    }
    const opening = OPENING.exec(line)
    if (opening !== null) {
      const stack = open.get(opening[1]!)
      if (stack === undefined) open.set(opening[1]!, [at])
      else stack.push(at)
      return
    }
    const closing = CLOSING.exec(line)
    const openAt = closing === null ? undefined : open.get(closing[1]!)?.pop()
    if (openAt !== undefined) closes.set(openAt, at)
  })
  return closes
}

/** Lines `from` to `to` as parts: a block whose closing line lies past `to`
 *  is text here. Blocks `depth` levels in split their bodies again. */
function split(lines: readonly string[], closes: ReadonlyMap<number, number>, from: number, to: number, depth: number): Part[] {
  const out: Part[] = []
  let textFrom = from
  for (let at = from; at < to; at++) {
    const closeAt = closes.get(at)
    if (closeAt === undefined || closeAt >= to) continue
    if (at > textFrom) out.push({ kind: 'text', text: lines.slice(textFrom, at).join('\n') })
    const [, name, attributes] = OPENING.exec(lines[at]!)!
    const body = lines.slice(at + 1, closeAt).join('\n')
    const inner = depth < MAX_DEPTH ? split(lines, closes, at + 1, closeAt, depth + 1) : [{ kind: 'text' as const, text: body }]
    out.push({ kind: 'block', name: name!, attributes: attributes!.trim(), body, raw: lines.slice(at, closeAt + 1).join('\n'), inner })
    at = closeAt
    textFrom = closeAt + 1
  }
  if (textFrom < to) out.push({ kind: 'text', text: lines.slice(textFrom, to).join('\n') })
  return out
}

/** A block's label: its tag name, the hyphens as spaces. */
export const labelOf = (name: string): string => name.replaceAll('-', ' ')
