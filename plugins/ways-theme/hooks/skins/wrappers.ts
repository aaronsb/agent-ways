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
// goes on past them: a later whole block is still a block.

const OPENING = /^<([a-z][a-z0-9]*(?:-[a-z0-9]+)+)((?:[ \t]+[A-Za-z_][\w.:-]*="[^"]*")*)[ \t]*>$/
/** A quick test before the split: a line that starts a hyphenated tag. */
const MAY_OPEN = /(?:^|\n)<[a-z][a-z0-9]*-/
/** How many levels of block are drawn as blocks, the outermost the first; a
 *  block nested deeper draws as its text. */
export const MAX_DEPTH = 4

/** A stretch of the message: text, or a whole block, its tag lines in `raw`
 *  alone. */
export type Part = { kind: 'text'; text: string } | { kind: 'block'; name: string; attributes: string; body: string; raw: string }

/** Whether `text` may hold a block, so that drawing it needs the split. */
export const holdsBlock = (text: string): boolean => MAY_OPEN.test(text)

/** `text` cut into text stretches and whole blocks, line by line. The line
 *  breaks either side of a block belong to neither. */
export function parts(text: string): Part[] {
  const lines = text.split('\n')
  const out: Part[] = []
  let textFrom = 0
  for (let at = 0; at < lines.length; at++) {
    const open = OPENING.exec(lines[at]!)
    if (open === null) continue
    const name = open[1]!
    const closeAt = closingLine(lines, at, name)
    if (closeAt === -1) continue
    if (at > textFrom) out.push({ kind: 'text', text: lines.slice(textFrom, at).join('\n') })
    out.push({ kind: 'block', name, attributes: open[2]!.trim(), body: lines.slice(at + 1, closeAt).join('\n'), raw: lines.slice(at, closeAt + 1).join('\n') })
    at = closeAt
    textFrom = closeAt + 1
  }
  if (textFrom < lines.length) out.push({ kind: 'text', text: lines.slice(textFrom).join('\n') })
  return out
}

/** The line that closes the block `name` opened at `openAt`, past the blocks
 *  of its name inside it; -1 when none does. */
function closingLine(lines: readonly string[], openAt: number, name: string): number {
  const close = `</${name}>`
  let depth = 0
  for (let at = openAt + 1; at < lines.length; at++) {
    if (lines[at] === close) {
      if (depth === 0) return at
      depth--
    } else if (OPENING.exec(lines[at]!)?.[1] === name) {
      depth++
    }
  }
  return -1
}

/** A block's label: its tag name, the hyphens as spaces. */
export const labelOf = (name: string): string => name.replaceAll('-', ' ')
