// Ported from skins by hellosverre (MIT; LICENSE beside this file),
// https://github.com/hellosverre/claude-skins at 985ad63, hooks/markdown.ts.

import type { Copy } from './commands'

// Finds the tables and fenced code in a reply so they can be drawn as cards; the rest
// stays markdown.

export type Align = 'left' | 'right' | 'center'

// `raw` is the table as written, where it came from markdown.
export type Table = { kind: 'table'; header: string[]; align: Align[]; rows: string[][]; raw?: string }

// `raw` is the fence as written, for the surfaces that keep Claude Code's own drawing.
export type Code = { kind: 'code'; lang: string; code: string; raw: string }

// `tex` is the formula alone; `raw` the block as written, kept for the copy button.
export type Math = { kind: 'math'; tex: string; raw: string }

// A copy is the text the person copies, as a block of its own (commands.ts).
export type Segment = { kind: 'text'; text: string } | Table | Code | Math | Copy

// Fences the desktop marks runnable with a Run button of its own. A card is an image and
// cannot carry it, so these keep Claude Code's drawing there.
const SHELLS = new Set(['bash', 'sh', 'shell', 'zsh', 'fish', 'console', 'powershell', 'pwsh', 'ps1', 'cmd', 'bat'])

export const isShellLang = (lang: string): boolean => SHELLS.has(lang)

export const isShell = (segment: Segment): boolean => segment.kind === 'code' && isShellLang(segment.lang)

const SEPARATOR = /^\s*\|?\s*:?-+:?\s*(\|\s*:?-+:?\s*)*\|?\s*$/

// A fence's opening line: its mark, three or more backticks or tildes, and its language.
export const FENCE = /^\s*(`{3,}|~{3,})\s*([\w+#.-]*)/
const FENCE_END = /^\s*(`{3,}|~{3,})\s*$/

// The line that closes the fence opened on line `i`, or -1 while it is still streaming: a
// line of the opening's mark alone, at least as long, so a ```` fence holds ``` lines and
// a ~~~ fence holds ``` ones.
export function fenceClose(lines: readonly string[], i: number): number {
  const mark = FENCE.exec(lines[i] ?? '')?.[1]

  if (mark === undefined) {
    return -1
  }

  return lines.findIndex((other, j) => {
    const end = j > i ? FENCE_END.exec(other)?.[1] : undefined

    return end !== undefined && end[0] === mark[0] && end.length >= mark.length
  })
}

// The code between a fence's opening on line `open` and its closing on line `close`, each
// line less up to as many spaces as the opening is indented, as CommonMark reads a fence:
// a fence in a list item keeps none of the item's indentation.
export function fenceCode(lines: readonly string[], open: number, close: number): string {
  const indent = /^ */.exec(lines[open] ?? '')?.[0].length ?? 0
  const margin = new RegExp(`^ {0,${indent}}`)

  return lines
    .slice(open + 1, close)
    .map(line => line.replace(margin, ''))
    .join('\n')
}

// A code span in a cell: a run of backticks, the content, a run of the same length.
const CELL_SPAN = /(?<![`\\])(`+)(?!`)(.+?)(?<!`)\1(?!`)/g

// A span's content as CommonMark reads it: one space off each side when both have one.
const spanText = (inner: string): string => (inner.length > 1 && inner.startsWith(' ') && inner.endsWith(' ') && inner.trim() !== '' ? inner.slice(1, -1) : inner)

// `cell` with `reduce` applied outside its code spans, and each span as its content
// verbatim, its ticks off. While `reduce` runs, each span is held out as its index between
// two private-use marks, so a link or emphasis around a span still reads whole; the mark
// is one the cell does not hold, so none of the cell's own text reads as a span.
function outsideCode(cell: string, reduce: (text: string) => string): string {
  const spans: string[] = []
  let code = 0xe000
  while (cell.includes(String.fromCharCode(code))) code++
  const mark = String.fromCharCode(code)
  const masked = cell.replace(CELL_SPAN, (_, _ticks: string, inner: string) => `${mark}${spans.push(spanText(inner)) - 1}${mark}`)

  return reduce(masked).replace(new RegExp(`${mark}(\\d+)${mark}`, 'g'), (_, at: string) => spans[Number(at)] ?? '')
}

// A cell as written, `\|` read as `|`, which GFM takes inside code spans too.
const cellSource = (cell: string): string => cell.replace(/\\\|/g, '|').trim()

// Bold read as noise in a padded cell; a code span as its content.
const clean = (cell: string): string => outsideCode(cellSource(cell), text => text.replace(/\*\*|__/g, ''))

const splitCells = (line: string): string[] => line.trim().replace(/^\|/, '').replace(/(?<!\\)\|$/, '').split(/(?<!\\)\|/)

export const cellsOf = (line: string): string[] => splitCells(line).map(clean)

const alignOf = (cell: string): Align => {
  const spec = cell.trim()

  if (spec.startsWith(':') && spec.endsWith(':')) {
    return 'center'
  }

  return spec.endsWith(':') ? 'right' : 'left'
}

const fit = (cells: string[], width: number): string[] =>
  Array.from({ length: width }, (_, i) => cells[i] ?? '')

// `tables` off leaves tables as text; a fence `fence` turns down stays text too, so a
// reply can be split for its charts alone. A CRLF line ending reads as LF. `math` takes display formulas out as their own
// segments: ````math`` fences, `$$…$$` and `\[…\]`.
export type SplitOptions = { tables: boolean; fence: (lang: string, code: string) => boolean; math?: boolean }

const SPLIT_ALL: SplitOptions = { tables: true, fence: () => true }

export function splitReply(markdown: string, options: SplitOptions = SPLIT_ALL): Segment[] {
  const lines = markdown.replace(/\r\n/g, '\n').split('\n')
  const segments: Segment[] = []
  let text: string[] = []

  const flush = () => {
    if (text.join('\n').trim() !== '') {
      segments.push({ kind: 'text', text: text.join('\n') })
    }

    text = []
  }

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i] ?? ''
    const next = lines[i + 1] ?? ''

    const fence = FENCE.exec(line)

    if (options.math === true) {
      const formula = displayMath(lines, i, fence)

      if (formula !== null) {
        flush()
        segments.push({ kind: 'math', tex: formula.tex, raw: lines.slice(i, formula.end + 1).join('\n') })
        i = formula.end
        continue
      }
    }

    // A closed fence becomes a code segment; one still streaming stays text, and so does
    // every line after it.
    if (fence !== null) {
      const close = fenceClose(lines, i)

      if (close === -1) {
        text.push(...lines.slice(i))
        break
      }

      const lang = (fence[2] ?? '').toLowerCase()
      const code = fenceCode(lines, i, close)

      if (!options.fence(lang, code)) {
        text.push(...lines.slice(i, close + 1))
        i = close
        continue
      }

      flush()
      segments.push({ kind: 'code', lang, code, raw: lines.slice(i, close + 1).join('\n') })
      i = close
      continue
    }

    const header = cellsOf(line)
    const isTable =
      options.tables &&
      line.includes('|') &&
      SEPARATOR.test(next) &&
      header.length > 1 &&
      cellsOf(next).length === header.length

    if (!isTable) {
      text.push(line)
      continue
    }

    flush()

    const start = i
    const rows: string[][] = []
    i += 2

    while (i < lines.length && (lines[i] ?? '').includes('|') && (lines[i] ?? '').trim() !== '') {
      rows.push(fit(cellsOf(lines[i] ?? ''), header.length))
      i++
    }

    i--
    segments.push({ kind: 'table', header, align: cellsOf(next).map(alignOf), rows, raw: lines.slice(start, i + 1).join('\n') })
  }

  flush()

  return segments
}

const DISPLAY_OPEN = /^\s*(\$\$|\\\[)(.*)$/

// A display formula starting on line `i`: where it ends and the TeX inside. One still
// streaming (no closing delimiter yet) is not one, so it stays text until it closes.
function displayMath(lines: readonly string[], i: number, fence: RegExpExecArray | null): { tex: string; end: number } | null {
  const line = lines[i] ?? ''

  if (fence !== null) {
    if ((fence[2] ?? '').toLowerCase() !== 'math') {
      return null
    }

    const close = fenceClose(lines, i)

    return close === -1 ? null : { tex: lines.slice(i + 1, close).join('\n'), end: close }
  }

  const open = DISPLAY_OPEN.exec(line)

  if (open === null) {
    return null
  }

  const closer = open[1] === '$$' ? '$$' : '\\]'
  const rest = open[2] ?? ''
  const sameLine = rest.indexOf(closer)

  // Whole on one line, ` x `, with nothing but space after it.
  if (sameLine !== -1) {
    const tex = rest.slice(0, sameLine)

    return rest.slice(sameLine + closer.length).trim() === '' && tex.trim() !== '' ? { tex, end: i } : null
  }

  for (let j = i + 1; j < lines.length; j++) {
    const other = lines[j] ?? ''
    const at = other.indexOf(closer)

    if (at !== -1) {
      if (other.slice(at + closer.length).trim() !== '') {
        return null
      }

      const tex = [rest, ...lines.slice(i + 1, j), other.slice(0, at)].join('\n')

      return tex.trim() === '' ? null : { tex, end: j }
    }

    // A blank line ends a paragraph, and a formula with it.
    if (other.trim() === '') {
      return null
    }
  }

  return null
}

// Terminal cells a character takes: wide East Asian characters and emoji take two,
// combining marks, zero-width joiners and skin-tone modifiers none.
const WIDE: readonly (readonly [number, number])[] = [
  [0x1100, 0x115f],
  [0x2e80, 0x303e],
  [0x3041, 0x33ff],
  [0x3400, 0x4dbf],
  [0x4e00, 0x9fff],
  [0xa000, 0xa4cf],
  [0xa960, 0xa97f],
  [0xac00, 0xd7a3],
  [0xf900, 0xfaff],
  [0xfe10, 0xfe19],
  [0xfe30, 0xfe6f],
  [0xff00, 0xff60],
  [0xffe0, 0xffe6],
  [0x1f300, 0x1f64f],
  [0x1f680, 0x1f6ff],
  [0x1f900, 0x1f9ff],
  [0x1fa70, 0x1faff],
  [0x20000, 0x3fffd],
]

export const charWidth = (char: string): number => {
  const code = char.codePointAt(0) ?? 0

  if (/\p{Mn}|\p{Me}|\u200d|[\ufe00-\ufe0f]|\p{Emoji_Modifier}/u.test(char)) {
    return 0
  }

  return WIDE.some(([from, to]) => code >= from && code <= to) ? 2 : 1
}

type Segmenter = { segment: (text: string) => Iterable<{ segment: string }> }
const SEGMENTER: Segmenter | undefined = (() => {
  const Make = (Intl as { Segmenter?: new (locale: undefined, options: { granularity: 'grapheme' }) => Segmenter }).Segmenter
  return Make === undefined ? undefined : new Make(undefined, { granularity: 'grapheme' })
})()

// Whether the engine segments text into graphemes.
export const IS_SEGMENTED = SEGMENTER !== undefined

// `text` a grapheme at a time where the engine segments text, else a code point at a time.
export const graphemes = (text: string): string[] => (SEGMENTER === undefined ? [...text] : Array.from(SEGMENTER.segment(text), s => s.segment))

// A grapheme drawn as an emoji, two cells: one with emoji presentation of its own
// (✅, ⭐), one a variation selector turns into an emoji (⚠️, ❤️), or a joined sequence.
const EMOJI_GRAPHEME = /\p{Emoji_Presentation}|\ufe0f|\u200d/u

const graphemeWidth = (grapheme: string): number =>
  EMOJI_GRAPHEME.test(grapheme) ? 2 : [...grapheme].reduce((sum, char) => sum + charWidth(char), 0)

// Terminal cells `text` takes, a grapheme at a time.
export const widthOf = (text: string): number =>
  /^[\x20-\x7e]*$/.test(text) ? text.length : graphemes(text).reduce((sum, grapheme) => sum + graphemeWidth(grapheme), 0)

// `text` cut to `width` cells, marked with an ellipsis where it lost text.
const cutTo = (text: string, width: number): string => {
  if (widthOf(text) <= width) {
    return text
  }

  let kept = ''
  let used = 0

  for (const char of graphemes(text)) {
    const next = widthOf(char)

    if (used + next > width - 1) {
      break
    }

    kept += char
    used += next
  }

  return `${kept}…`
}

// The narrowest a column is narrowed to while the table still fits with every column
// this wide; past that, columns go down to one cell.
const MIN_COLUMN = 3

// The cells of the widest word in `text`: the narrowest it wraps to without breaking one.
const longestWord = (text: string): number => Math.max(0, ...text.split(/\s+/).map(widthOf))

// Natural column widths, narrowed from the widest down until the table fits: first no
// column below its longest word, the header's included, so cells wrap between words;
// then, when the words alone are too wide for `maxWidth`, down to MIN_COLUMN, and last
// down to one cell, the words that no longer fit breaking inside. A table of more
// columns than one cell each can hold is still wider than `maxWidth`.
export function columnWidths(table: Table, maxWidth: number, gap: number): number[] {
  const cells = (col: number) => [table.header[col] ?? '', ...table.rows.map(row => row[col] ?? '')]
  const widths = table.header.map((_, col) => Math.max(...cells(col).map(widthOf)))
  const words = table.header.map((_, col) => Math.max(...cells(col).map(longestWord)))
  const budget = maxWidth - gap * (widths.length - 1)
  const floors = [(col: number) => Math.max(words[col] ?? 0, MIN_COLUMN), () => MIN_COLUMN, () => 1]

  for (const floor of floors) {
    while (widths.reduce((sum, width) => sum + width, 0) > budget) {
      const narrowable = widths.map((width, col) => (width > floor(col) ? width : -1))
      const widest = narrowable.indexOf(Math.max(...narrowable))

      if ((narrowable[widest] ?? -1) < 0) {
        break
      }

      widths[widest] = (widths[widest] ?? 0) - 1
    }
  }

  return widths
}

// A cell cut to its column, marked with an ellipsis where it lost text.
export function cutCell(text: string, width: number): string {
  return cutTo(text, width)
}

// Splits a word too long for its column into pieces that each fit, never inside a grapheme.
function breakWord(word: string, width: number, measure: (text: string) => number): string[] {
  const pieces: string[] = []
  let piece = ''

  for (const char of graphemes(word)) {
    if (piece !== '' && measure(piece + char) > width) {
      pieces.push(piece)
      piece = char
    } else {
      piece += char
    }
  }

  return piece === '' ? pieces : [...pieces, piece]
}

// `text` as lines that fit `width`; past `maxLines`, the last line ends in `…`. `measure`
// is terminal cells by default; the desktop card passes its pixel measure.
export function wrapLines(text: string, width: number, maxLines: number, measure: (text: string) => number = widthOf): string[] {
  const lines: string[] = []
  let line = ''

  for (const word of text.split(/\s+/).filter(Boolean)) {
    const candidate = line === '' ? word : `${line} ${word}`

    if (measure(candidate) <= width) {
      line = candidate
      continue
    }

    if (line !== '') {
      lines.push(line)
    }

    const pieces = measure(word) <= width ? [word] : breakWord(word, width, measure)
    lines.push(...pieces.slice(0, -1))
    line = pieces.at(-1) ?? ''
  }

  const all = line === '' ? lines : [...lines, line]

  if (all.length <= maxLines) {
    return all.length === 0 ? [''] : all
  }

  const kept = all.slice(0, maxLines)
  const chars = [...(kept[maxLines - 1] ?? '')]

  while (chars.length > 0 && measure(`${chars.join('')}…`) > width) {
    chars.pop()
  }

  return [...kept.slice(0, -1), `${chars.join('')}…`]
}

export function padCell(text: string, width: number, align: Align): string {
  const cut = cutTo(text, width)
  const room = width - widthOf(cut)

  if (align === 'right') {
    return ' '.repeat(room) + cut
  }

  if (align === 'center') {
    const left = Math.floor(room / 2)

    return ' '.repeat(left) + cut + ' '.repeat(room - left)
  }

  return cut + ' '.repeat(room)
}

// What `/skin copy` puts on the clipboard: the whole reply, or with `code` its last code
// block; a message instead when there is nothing to copy.
export function copyOf(reply: string, code: boolean): { text: string } | { message: string } {
  if (reply.trim() === '') {
    return { message: 'Nothing to copy yet' }
  }

  if (!code) {
    return { text: reply }
  }

  const last = splitReply(reply).findLast((segment): segment is Code => segment.kind === 'code')

  return last === undefined ? { message: 'No code block in the last reply' } : { text: last.code }
}

// A cell's inline markdown as its text: an image or a link as its text, an autolink as its
// address, `<br>` as a line break, emphasis and strikethrough as the words inside them, a
// backslash escape as the character, and a code span as its content verbatim. A link's
// address may hold one level of parentheses, `(https://x.com/(p))`.
const plainCell = (cell: string): string =>
  outsideCode(cellSource(cell), text =>
    text
      .replace(/!?\[([^\]]*)\]\((?:[^()]|\([^()]*\))*\)/g, '$1')
      .replace(/<((?:[a-z][a-z0-9+.-]{1,31}:[^\s<>]*)|(?:[^\s<>@]+@[^\s<>]+))>/gi, '$1')
      .replace(/<br\s*\/?>/gi, '\n')
      .replace(/\*\*(.+?)\*\*|__(.+?)__/g, '$1$2')
      .replace(/~~(.+?)~~/g, '$1')
      .replace(/(?<![\w*])\*(?!\s)(.+?)(?<!\s)\*(?![\w*])/g, '$1')
      .replace(/(?<!\w)_(?!\s)(.+?)(?<!\s)_(?!\w)/g, '$1')
      .replace(/\\([\\`*_{}[\]()#+\-.!|~<>])/g, '$1'),
  )

// The cells of a table as written, for the copy: its header and rows from `raw` where it
// came from markdown, since the drawn cells have lost their code spans' ticks.
function writtenCells(table: Table): string[][] {
  const lines = table.raw?.split('\n')

  if (lines === undefined) {
    return [table.header, ...table.rows]
  }

  return [lines[0] ?? '', ...lines.slice(2)].map(line => fit(splitCells(line), table.header.length))
}

// A table as CSV, its header row first. A cell holding a comma, a quote or a line break is
// quoted and its quotes doubled, as RFC 4180 has it; rows end in a line feed.
export function tableCsv(table: Table): string {
  const field = (cell: string) => {
    const text = plainCell(cell)
    return /[",\r\n]/.test(text) ? `"${text.replace(/"/g, '""')}"` : text
  }

  return writtenCells(table)
    .map(cells => cells.map(field).join(','))
    .join('\n')
}

// A table as it is drawn, boxed in monospace, for pasting where markdown does not render.
export function tableArt(table: Table): string {
  const widths = table.header.map((cell, col) => Math.max(widthOf(cell), ...table.rows.map(row => widthOf(row[col] ?? ''))))
  const rule = (left: string, mid: string, right: string) => `${left}${widths.map(width => '─'.repeat(width + 2)).join(mid)}${right}`
  const row = (cells: readonly string[]) => `│${widths.map((width, col) => ` ${padCell(cells[col] ?? '', width, table.align[col] ?? 'left')} `).join('│')}│`

  return [rule('╭', '┬', '╮'), row(table.header), rule('├', '┼', '┤'), ...table.rows.map(row), rule('╰', '┴', '╯')].join('\n')
}
