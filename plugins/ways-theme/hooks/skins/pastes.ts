// The person's pastes as Claude Code shows them. A paste reaches the message
// wrapped, `<pasted_content id="ab12">\n...\n</pasted_content id="ab12">`,
// with two newlines around it, and the id is the session's: the first four
// hex digits of the SHA-256 of its session id. Claude Code 2.1.312 draws such
// a message with the wrapper taken off and the paste on lines of its own when
// it spans lines; a block whose id is not the session's stays as typed, so a
// person's own literal tags never become a paste. These are its rules, read
// from its `V3t` (the split) and `ols` (the layout).
//
// Inside a paste, a tag of a paste the copied text carried arrives escaped
// (`<\pasted_content`, `<\/pasted_content`), whatever its id. The skin takes
// one level of escape off and draws a whole inner paste by the same rules, so
// no wrapper of any depth shows. Claude Code leaves the escaped tags in.
//
// Nothing is dropped but the wrappers and the whitespace beside a paste that
// Claude Code drops too. A tag left open, or closed under another id, is text:
// the split stops there and the rest of the message draws as it came.

const OPEN = '<pasted_content id="'
const OPEN_END = '">\n'
const closeTag = (id: string) => `</pasted_content id="${id}">`
const ID_LENGTH = 4
const ID = /^[0-9a-f]{4}$/
/** One level of escape on a nested paste's tag. */
const ESCAPED_TAG = /<\\(?=\/?pasted_content)/g
/** How deep inner pastes are drawn; deeper ones keep their tags. */
const MAX_DEPTH = 8
/** Marks a paste may sit after, with no space, on its own line. */
const OPENERS = /[([{"'`]/
/** Marks that may follow a paste on its line. */
const CLOSERS = /[)\]}"'`.,;:!?]/

/** A stretch of the message: typed text, or a whole paste. */
export type Piece = { kind: 'text'; text: string } | { kind: 'paste'; id: string; body: string; raw: string }

/** `text` cut into typed stretches and whole pastes. A paste takes up to two
 *  newlines either side of it; an opening tag with no closing tag of its id
 *  ends the split, and what follows stays text. */
export function pieces(text: string): Piece[] {
  const out: Piece[] = []
  let textFrom = 0
  let searchFrom = 0
  for (;;) {
    const open = text.indexOf(OPEN, searchFrom)
    if (open === -1) break
    const idAt = open + OPEN.length
    const id = text.slice(idAt, idAt + ID_LENGTH)
    if (!ID.test(id) || !text.startsWith(OPEN_END, idAt + ID_LENGTH)) {
      searchFrom = idAt
      continue
    }
    const bodyAt = idAt + ID_LENGTH + OPEN_END.length
    const close = closeTag(id)
    const closeAt = text.indexOf(`\n${close}`, bodyAt - 1) + 1
    if (closeAt === 0) break
    let from = open
    for (let n = 0; n < 2 && from > textFrom && text[from - 1] === '\n'; n++) from--
    if (from > textFrom) out.push({ kind: 'text', text: text.slice(textFrom, from) })
    textFrom = closeAt + close.length
    for (let n = 0; n < 2 && text[textFrom] === '\n'; n++) textFrom++
    out.push({ kind: 'paste', id, body: text.slice(bodyAt, closeAt - 1), raw: text.slice(from, textFrom) })
    searchFrom = textFrom
  }
  if (textFrom < text.length) out.push({ kind: 'text', text: text.slice(textFrom) })
  return out
}

/** Whether what follows a paste lets it end its line: nothing, or closing
 *  marks up to a space. */
const endsLine = (after: string): boolean => {
  let at = 0
  while (at < after.length && CLOSERS.test(after[at]!)) at++
  return at >= after.length || /\s/.test(after[at]!)
}

/** The message as drawn: each paste `isPaste` accepts unwrapped, on lines of
 *  its own when it spans lines or sits against typed text, its body drawn by
 *  `bodyOf`; every other block stays as it came. */
function laidOut(text: string, isPaste: (id: string) => boolean, depth: number): string {
  const all = pieces(text)
  const drawnBody = (p: Piece | undefined): string | null => (p?.kind === 'paste' && isPaste(p.id) ? p.body : null)
  if (all.every(p => drawnBody(p) === null)) return text
  let out = ''
  let isBroken = false
  all.forEach((piece, i) => {
    const next = all[i + 1]
    const body = drawnBody(piece)
    if (body === null) {
      let typed = piece.kind === 'text' ? piece.text : piece.raw
      if (isBroken) typed = typed.replace(/^\s+/, '')
      if (drawnBody(next)?.includes('\n')) typed = typed.replace(/\s+$/, '')
      if (typed === '') return
      if (isBroken && out !== '') out += '\n'
      out += typed
      isBroken = false
      return
    }
    const spansLines = body.includes('\n')
    let end = out.length
    while (end > 0 && OPENERS.test(out[end - 1]!)) end--
    const isSpaced = end === 0 || /\s/.test(out[end - 1]!)
    if ((spansLines || isBroken || !isSpaced) && out !== '') out += '\n'
    out += bodyOf(body, depth)
    const after = next === undefined ? '' : next.kind === 'text' ? next.text : next.raw
    isBroken = spansLines || drawnBody(next) !== null || /^\s\s/.test(after) || !endsLine(after)
  })
  return out
}

/** A paste's body as drawn: one level of escape off, and the pastes it
 *  carried drawn whole, whatever their id. */
const bodyOf = (body: string, depth: number): string => {
  const unescaped = body.replace(ESCAPED_TAG, '<')
  return depth >= MAX_DEPTH ? unescaped : laidOut(unescaped, () => true, depth + 1)
}

/** The person's message as Claude Code shows it: the session's pastes
 *  unwrapped. With no paste id, or no paste of it, the text as it came. */
export function shownText(text: string, pasteId: string | null): string {
  if (pasteId === null || !text.includes(OPEN)) return text
  return laidOut(text, id => id === pasteId, 0)
}

/** The id Claude Code gives the session's pastes: the first four hex digits
 *  of the SHA-256 of `sessionId`. */
export async function pasteIdOf(sessionId: string): Promise<string> {
  const digest = new Uint8Array(await crypto.subtle.digest('SHA-256', new TextEncoder().encode(sessionId)))
  return Array.from(digest.slice(0, ID_LENGTH / 2), b => b.toString(16).padStart(2, '0')).join('')
}
