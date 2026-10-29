// The `! <command>` suggestions in a reply. Claude Code runs a prompt that
// starts with `!` as a shell command in the session, and Claude suggests one
// for the person to run as `! git status`. Drawn as prose, a long one wraps
// inside the reply's frame, and a terminal selection over it takes the
// frame's border, its padding and the line breaks along with the command.
// The skin draws each one as a block of its own, outside the frame, with a
// Copy control (rows.tsx, `commandRows`).
//
// A suggestion starts `! ` and a command word: a lowercase letter, a digit,
// `.`, `/`, `~`, `$`, `(` or a quote. It is one of three forms:
//
// - A line whose text, after its indentation and an optional list marker
//   (`-`, `*`, `+`, `1.`, `1)`), is a suggestion. The command runs from the
//   `!` to the end of the line, trailing spaces off. A lead-in sentence on the
//   line before stays prose.
// - An inline code span whose content, less the one space CommonMark strips
//   from each side, is a suggestion. The line is cut there: the prose before
//   the span, the command, the prose after it. A list marker alone before the
//   span, and punctuation alone after it, go with the span.
// - A closed fence, with no language or a shell's (markdown.ts, SHELLS),
//   whose code is one logical command: the first line is a suggestion, and
//   every line before the last ends in a continuation, `\`, `&&`, `||` or
//   `|`. The command is the code as written.
//
// A command line that ends in a continuation takes the lines after it the
// same way, up to the first that does not continue, the first blank line or
// the first table row.
//
// Nothing else is a suggestion: a `!` inside a sentence ("that's it!"),
// `!=`, `!!`, `!important`, an image (`![alt](src)`), `!` with no space
// after it, prose such as `! This is not a command` or a TeX log's
// `! Undefined control sequence.`, a span whose content starts otherwise,
// and anything inside other markdown: a quote, a table row, link text, an
// HTML comment, an indented code block (four spaces or a tab, after a blank
// line), and a fence of another language or one still streaming. The copied
// text is the command with its `! `, as the person types it into Claude
// Code's prompt.

import { FENCE, fenceClose, isShellLang } from './markdown'

/** A suggestion: `command` as drawn and copied, `raw` the source it came from. */
export type Command = { kind: 'command'; command: string; raw: string }

/** A stretch of the reply: prose, or a whole suggestion. */
export type Piece = { kind: 'text'; text: string } | Command

const STARTS = /^! [a-z0-9./~$("']/
const LINE = /^[ \t]*(?:(?:[-*+]|\d{1,3}[.)])[ \t]+)?(! [a-z0-9./~$("'].*?)[ \t]*$/
/** A line that a shell reads on past. */
const CONTINUES = /(?:\\|&&|\|)[ \t]*$/
/** A code span on one line: a run of backticks, the content, a run of the same length. */
const SPAN = /(?<!`)(`+)(?!`)(.+?)(?<!`)\1(?!`)/g
/** Prose beside a span that goes with it: a list marker before, punctuation after. */
const MARKER_ONLY = /^[ \t]*(?:(?:[-*+]|\d{1,3}[.)])[ \t]*)?$/
const PUNCTUATION_ONLY = /^[\s.,;:!?)\]]*$/
/** Lines whose spans are never lifted out: a quote, a table row. */
const QUOTE_OR_ROW = /^[ \t]*[>|]/
const TABLE_ROW = /^[ \t]*\|/
/** A line of an indented code block, after a blank line or another such line. */
const INDENTED = /^(?: {4}|[ \t]*\t)/
/** A link's text, up to the `(` or `[` that follows it. */
const LINK_TEXT = /\[[^\]]*\][([]/g
/** A quick test before the split: a line or a span that starts a suggestion. */
const MAY_HOLD = /(?:^|\n)[ \t]*(?:(?:[-*+]|\d{1,3}[.)])[ \t]+)?! \S|` ?! \S/

/** Whether `text` may hold a suggestion, so that drawing it needs the split. */
export const holdsCommand = (text: string): boolean => MAY_HOLD.test(text)

/** A code span's content as CommonMark reads it: one space off each side
 *  when both sides have one and it is not all spaces. */
const spanContent = (inner: string): string => (inner.length > 1 && inner.startsWith(' ') && inner.endsWith(' ') && inner.trim() !== '' ? inner.slice(1, -1) : inner)

/** A fence's code as a suggestion, or null when it is not one. `raw` is the
 *  fence as written. */
export function fenceCommand(lang: string, code: string, raw: string): Command | null {
  if (lang !== '' && !isShellLang(lang)) return null
  const lines = code.trim().split('\n')
  if (!STARTS.test(lines[0]!) || !lines.slice(0, -1).every(line => CONTINUES.test(line))) return null
  return { kind: 'command', command: lines.map(line => line.trimEnd()).join('\n'), raw }
}

/** `text` cut into prose and whole suggestions, line by line. A fence still
 *  streaming, and every line after it, stay prose. */
export function commandPieces(text: string): Piece[] {
  const lines = text.split('\n')
  const out: Piece[] = []
  let prose: string[] = []
  // A stretch of prose, the blank lines at either end of it off.
  const flush = () => {
    const stretch = prose.join('\n').replace(/^(?:[ \t]*\n)+|(?:\n[ \t]*)+$/g, '')
    if (stretch.trim() !== '') out.push({ kind: 'text', text: stretch })
    prose = []
  }
  const command = (found: Command) => {
    flush()
    out.push(found)
  }

  let isComment = false
  let isIndented = false
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i]!
    if (isComment) {
      prose.push(line)
      isComment = !line.includes('-->')
      continue
    }
    const fence = FENCE.exec(line)
    if (fence !== null) {
      isIndented = false
      const close = fenceClose(lines, i)
      if (close === -1) {
        prose.push(...lines.slice(i))
        break
      }
      const raw = lines.slice(i, close + 1).join('\n')
      const found = fenceCommand((fence[2] ?? '').toLowerCase(), lines.slice(i + 1, close).join('\n'), raw)
      if (found === null) prose.push(...lines.slice(i, close + 1))
      else command(found)
      i = close
      continue
    }
    // An HTML comment, from the line that opens it to the line that closes it, is prose.
    if (line.includes('<!--')) {
      prose.push(line)
      isComment = !line.slice(line.lastIndexOf('<!--')).includes('-->')
      continue
    }
    if (INDENTED.test(line) && (isIndented || i === 0 || lines[i - 1]!.trim() === '')) {
      isIndented = true
      prose.push(line)
      continue
    }
    if (line.trim() !== '') isIndented = false

    const whole = LINE.exec(line)
    if (whole !== null) {
      const taken = [whole[1]!]
      let end = i
      const continues = () => CONTINUES.test(lines[end]!) && end + 1 < lines.length && lines[end + 1]!.trim() !== '' && !TABLE_ROW.test(lines[end + 1]!)
      while (continues()) taken.push(lines[++end]!.trimEnd())
      command({ kind: 'command', command: taken.join('\n'), raw: lines.slice(i, end + 1).join('\n') })
      i = end
      continue
    }

    const links = [...line.matchAll(LINK_TEXT)].map(link => [link.index, link.index + link[0].length] as const)
    const isLinkText = (at: number) => links.some(([from, to]) => at > from && at < to)
    let at = 0
    for (const span of QUOTE_OR_ROW.test(line) ? [] : line.matchAll(SPAN)) {
      const content = spanContent(span[2]!)
      if (!STARTS.test(content) || isLinkText(span.index)) continue
      const before = line.slice(at, span.index)
      const isFirst = at === 0
      if (!(isFirst ? MARKER_ONLY : PUNCTUATION_ONLY).test(before)) prose.push(isFirst ? before.trimEnd() : before.trim())
      command({ kind: 'command', command: content.trimEnd(), raw: span[0] })
      at = span.index + span[0].length
    }
    const rest = line.slice(at)
    if (at === 0) prose.push(line)
    else if (!PUNCTUATION_ONLY.test(rest)) prose.push(rest.trimStart())
  }
  flush()

  return out
}
