// The text in a reply that the person copies: a suggestion to type into
// Claude Code's prompt, a fenced block of code, or a code span too wide for
// its row. Drawn as prose, a long one wraps inside the reply's frame, and a
// terminal selection over it takes the frame's border, its padding and the
// line breaks along with the text. The skin draws each one as a block of its
// own, outside the frame, with nothing beside any of its lines and a Copy
// control that copies the text whole (rows.tsx, `copyRows`). A block's
// `form` says what it is:
//
// - `shell`, a `! <command>` suggestion. Claude Code runs a prompt that
//   starts with `!` as a shell command in the session, and Claude suggests
//   one for the person to run as `! git status`. A suggestion starts `! ` and
//   a command word: an environment assignment (`VAR=1`, `RUST_LOG=debug`), or
//   a lowercase letter, a digit, `.`, `/`, `~`, `$`, `(`, `[` or a quote.
// - `slash`, a slash command such as `/compact keep the handoff`: `/`, a
//   name of a lowercase letter and then lowercase letters, digits, `:` or
//   `-`, and then a space or the end, where the name is one Claude Code lists
//   as a command the person can run (`isCommand`, from `$.command.list()`,
//   hub.tsx). `/home/aaron`, `/tmp/x` and `a/b` are paths, a URL starts
//   otherwise, and a route such as `/health` names no command. Until the list
//   is read, and where Claude Code has none, every name but a top-level
//   directory (ROOTS: `/tmp`, `/etc`, `/nix`) counts.
// - `code`, a closed fence's code, as written. It keeps its language and its
//   highlighting. A fence with no code in it stays as it is, and so does a
//   fence the skin draws as a picture, a Mermaid chart or a diff (reply.ts).
// - `span`, an inline code span whose content is wider than 60% of the row
//   the span's line is drawn in: the reply's prose, less an open list item's
//   indentation. A span that wraps inside a frame has the frame's border
//   beside each of its lines, which no drawing of the span inside the frame
//   avoids, and a span that starts late in its line wraps sooner than its
//   width alone says, so the span leaves the frame. A narrower span stays in
//   its sentence.
//
// A suggestion, shell or slash, is one of three forms:
//
// - A line whose text, after its indentation and an optional list marker
//   (`-`, `*`, `+`, `1.`, `1)`), is a suggestion. A shell one runs to the
//   end of the line, trailing spaces off. A slash one is the bare name
//   (`/clear`) and nothing else, since a line that starts `/compact is
//   useful` or `/dev and /proc` is prose. A lead-in sentence on the line
//   before stays prose.
// - An inline code span whose content, less the one space CommonMark strips
//   from each side, is a suggestion, arguments and all. The line is cut
//   there: the prose before the span, the suggestion, the prose after it. A
//   list marker alone before the span, and punctuation alone after it, go
//   with the span. A `span` block cuts its line the same way.
// - A closed fence whose code is one suggestion. For a shell one the fence
//   has no language or a shell's (markdown.ts, SHELLS), the first line is a
//   suggestion, and every line before the last ends in a continuation, `\`,
//   `&&`, `||` or `|`. The command is the code as written. For a slash one
//   the fence has no language and the code is one line.
//
// A shell line that ends in a continuation takes the lines after it the same
// way, up to the first that does not continue, the first blank line or the
// first table row.
//
// Nothing else is a suggestion: a `!` inside a sentence ("that's it!"),
// `!=`, `!!`, `!important`, an image (`![alt](src)`), `!` with no space
// after it, prose such as `! This is not a command` or a TeX log's
// `! Undefined control sequence.`, a `/` inside a sentence, a path, a span
// whose content starts otherwise, and anything inside other markdown: a
// quote, a table row, link text, an HTML comment (a `<!--` inside a code span
// opens none), an indented code block (four columns past the open list
// item's content, or past the margin outside a list, after a blank line), and
// a fence still streaming. The same markdown keeps a `span` block in its
// place. The copied text is the suggestion with its `! ` or its `/`, as the
// person types it into Claude Code's prompt, and a fence's code or a span's
// content as written. A fence's code is read as CommonMark reads it: each
// line less up to as many columns of indentation as the opening line has,
// spaces or tabs, a tab to the next multiple of four, so a fence in a list
// item copies without the item's indentation, and its trailing blank lines
// off.

import { FENCE, fenceClose, fenceCode, indentOf, isShellLang, widthOf } from './markdown'

/** What a copy block is (the header comment). */
export type Form = 'shell' | 'slash' | 'span' | 'code'

/** A copy block: `text` as drawn and copied, `raw` the source it came from,
 *  and for a fence's code its `lang`. */
export type Copy = { kind: 'copy'; form: Exclude<Form, 'code'>; text: string; raw: string } | { kind: 'copy'; form: 'code'; lang: string; text: string; raw: string }

/** A stretch of the reply: prose, or a whole copy block. */
export type Piece = { kind: 'text'; text: string } | Copy

/** What may follow `! `: an environment assignment, or a command's first character. */
const WORD = String.raw`(?:[A-Z_][A-Z0-9_]*=|[a-z0-9./~$("'\[])`
/** A slash command's name, after its `/`. */
const NAME = '[a-z][a-z0-9:-]*'
const SHELL_STARTS = new RegExp(`^! ${WORD}`)
/** A slash command at the start of a span's content, its name ending at a space or the end. */
const SLASH_STARTS = new RegExp(`^\\/(${NAME})(?= |$)`)
/** A line that is a suggestion: a shell one to the end of the line, a slash one the bare name. */
const LINE = new RegExp(String.raw`^[ \t]*(?:(?:[-*+]|\d{1,3}[.)])[ \t]+)?(! ${WORD}.*?|\/${NAME})[ \t]*$`)
/** The top-level directories of a Unix filesystem, which read as paths, never as slash commands. */
const ROOTS = new Set(['bin', 'boot', 'dev', 'etc', 'home', 'lib', 'lib64', 'media', 'mnt', 'nix', 'opt', 'private', 'proc', 'root', 'run', 'sbin', 'srv', 'sys', 'tmp', 'usr', 'var'])

/** Whether a name is a slash command the person can run. */
export type IsCommand = (name: string) => boolean

/** The answer while Claude Code's command list is unread: any name but a top-level directory. */
export const notRoot: IsCommand = name => !ROOTS.has(name)
/** The share of its row a span's content may take before it leaves its sentence. */
const SPAN_SHARE = 0.6
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
/** A list item's opening: its indentation, its marker and the spaces after it. */
const LIST_ITEM = /^([ \t]*)([-*+]|\d{1,9}[.)])([ \t]+)\S/
/** A link's text, up to the `(` or `[` that follows it. */
const LINK_TEXT = /\[[^\]]*\][([]/g
/** A quick test before the split: a line or a span that starts a suggestion, or a fence. */
const MAY_HOLD = /(?:^|\n)[ \t]*(?:(?:[-*+]|\d{1,3}[.)])[ \t]+)?(?:! \S|\/[a-z])|` ?(?:! \S|\/[a-z])|(?:^|\n)[ \t]*(?:`{3}|~{3})/

/** A code span's content as CommonMark reads it: one space off each side
 *  when both sides have one and it is not all spaces. */
const spanContent = (inner: string): string => (inner.length > 1 && inner.startsWith(' ') && inner.endsWith(' ') && inner.trim() !== '' ? inner.slice(1, -1) : inner)

/** Whether a span's content is wider than SPAN_SHARE of its row: `width`, the
 *  columns of the reply's prose, less `indent`, the open list item's content column. */
const isWide = (content: string, width: number | undefined, indent: number): boolean => width !== undefined && widthOf(content) > SPAN_SHARE * (width - indent)

/** Whether `text` may hold a copy block, so that drawing it needs the split.
 *  `width` is the columns the reply's prose is drawn in; without it no span
 *  is wide. */
export const holdsCopy = (text: string, width?: number, isCommand: IsCommand = notRoot): boolean =>
  MAY_HOLD.test(text) || (width !== undefined && text.includes('`') && copyPieces(text, width, undefined, isCommand).some(piece => piece.kind === 'copy'))

/** The suggestion `text` starts, or null. */
function suggestionForm(text: string, isCommand: IsCommand): 'shell' | 'slash' | null {
  if (SHELL_STARTS.test(text)) return 'shell'
  const name = SLASH_STARTS.exec(text)?.[1]
  return name !== undefined && isCommand(name) ? 'slash' : null
}

/** Where a list item's content starts: past its indentation, its marker and
 *  the spaces after it, or one space past the marker when five or more follow,
 *  as CommonMark reads it. Null for a line that opens no item. */
function itemContent(line: string): number | null {
  const item = LIST_ITEM.exec(line)
  if (item === null) return null
  const marker = indentOf(item[1]!) + item[2]!.length
  const gap = indentOf(item[3]!)
  return marker + (gap > 4 ? 1 : gap)
}

/** `line` with its code spans taken out, so a `<!--` inside one opens no comment. */
const outsideSpans = (line: string): string => line.replace(SPAN, '')

/** A fence's code as a copy block: a suggestion when it is one, else the
 *  code, its trailing blank lines off. Null for a fence with no code. `raw`
 *  is the fence as written. */
export function fenceCopy(lang: string, code: string, raw: string, isCommand: IsCommand = notRoot): Copy | null {
  if (code.trim() === '') return null
  const lines = code.trim().split('\n')
  const isShell = (lang === '' || isShellLang(lang)) && SHELL_STARTS.test(lines[0]!) && lines.slice(0, -1).every(line => CONTINUES.test(line))
  if (isShell) return { kind: 'copy', form: 'shell', text: lines.map(line => line.trimEnd()).join('\n'), raw }
  if (lang === '' && lines.length === 1 && suggestionForm(lines[0]!, isCommand) === 'slash') return { kind: 'copy', form: 'slash', text: lines[0]!.trimEnd(), raw }
  return { kind: 'copy', form: 'code', lang, text: code.replace(/(?:\n[ \t]*)+$/, ''), raw }
}

/** `text` cut into prose and whole copy blocks, line by line. `width` is the
 *  columns the reply's prose is drawn in, for the spans too wide for it.
 *  `isPicture` names the fences the skin draws as pictures, which stay
 *  prose, and `isCommand` the slash commands. A fence still streaming, and
 *  every line after it, stay prose. A CRLF line ending reads as LF. */
export function copyPieces(text: string, width?: number, isPicture: (lang: string, code: string) => boolean = () => false, isCommand: IsCommand = notRoot): Piece[] {
  const lines = text.replace(/\r\n/g, '\n').split('\n')
  const out: Piece[] = []
  let prose: string[] = []
  // A stretch of prose, the blank lines at either end of it off.
  const flush = () => {
    const stretch = prose.join('\n').replace(/^(?:[ \t]*\n)+|(?:\n[ \t]*)+$/g, '')
    if (stretch.trim() !== '') out.push({ kind: 'text', text: stretch })
    prose = []
  }
  const copy = (found: Copy) => {
    flush()
    out.push(found)
  }

  let isComment = false
  let isIndented = false
  // The content column of the list item still open, or null outside a list.
  let item: number | null = null
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i]!
    if (isComment) {
      prose.push(line)
      isComment = !outsideSpans(line).includes('-->')
      continue
    }
    const isAfterBlank = i === 0 || lines[i - 1]!.trim() === ''
    const fence = FENCE.exec(line)
    if (fence !== null) {
      isIndented = false
      const close = fenceClose(lines, i)
      if (close === -1) {
        prose.push(...lines.slice(i))
        break
      }
      const lang = (fence[2] ?? '').toLowerCase()
      const code = fenceCode(lines, i, close)
      const found = isPicture(lang, code) ? null : fenceCopy(lang, code, lines.slice(i, close + 1).join('\n'), isCommand)
      if (found === null) prose.push(...lines.slice(i, close + 1))
      else copy(found)
      i = close
      continue
    }
    // An HTML comment, from the line that opens it to the line that closes it, is prose.
    const bare = outsideSpans(line)
    if (bare.includes('<!--')) {
      prose.push(line)
      isComment = !bare.slice(bare.lastIndexOf('<!--')).includes('-->')
      continue
    }
    if (line.trim() !== '' && indentOf(line) >= (item ?? 0) + 4 && (isIndented || isAfterBlank)) {
      isIndented = true
      prose.push(line)
      continue
    }
    if (line.trim() !== '') {
      isIndented = false
      const opens = itemContent(line)
      if (opens !== null) item = opens
      else if (isAfterBlank && indentOf(line) < (item ?? 0)) item = null
    }

    const whole = LINE.exec(line)
    const form = whole === null ? null : suggestionForm(whole[1]!, isCommand)
    if (whole !== null && form !== null) {
      const taken = [whole[1]!]
      let end = i
      const continues = () => form === 'shell' && CONTINUES.test(lines[end]!) && end + 1 < lines.length && lines[end + 1]!.trim() !== '' && !TABLE_ROW.test(lines[end + 1]!)
      while (continues()) taken.push(lines[++end]!.trimEnd())
      copy({ kind: 'copy', form, text: taken.join('\n'), raw: lines.slice(i, end + 1).join('\n') })
      i = end
      continue
    }

    const links = [...line.matchAll(LINK_TEXT)].map(link => [link.index, link.index + link[0].length] as const)
    const isLinkText = (at: number) => links.some(([from, to]) => at > from && at < to)
    let at = 0
    for (const span of QUOTE_OR_ROW.test(line) ? [] : line.matchAll(SPAN)) {
      const content = spanContent(span[2]!)
      const form = suggestionForm(content, isCommand) ?? (isWide(content, width, item ?? 0) ? 'span' : null)
      if (form === null || isLinkText(span.index)) continue
      const before = line.slice(at, span.index)
      const isFirst = at === 0
      if (!(isFirst ? MARKER_ONLY : PUNCTUATION_ONLY).test(before)) prose.push(isFirst ? before.trimEnd() : before.trim())
      copy({ kind: 'copy', form, text: form === 'span' ? content : content.trimEnd(), raw: span[0] })
      at = span.index + span[0].length
    }
    const rest = line.slice(at)
    if (at === 0) prose.push(line)
    else if (!PUNCTUATION_ONLY.test(rest)) prose.push(rest.trimStart())
  }
  flush()

  return out
}
