// Ported from skins by hellosverre (MIT; LICENSE beside this file),
// https://github.com/hellosverre/claude-skins at 985ad63, hooks/svg-code.ts.

import type { Palette } from './skin'
import { CONTROL_SLOT, escape, HEADER_MID, fitText, MAX_SVG, measure, MONO, riseDelay, staggerEnd, staggerStep, svgCard, tint } from './svg-kit'

// A fenced code block as a card: the language and line count in a header, line numbers
// in a gutter, and light highlighting of comments, strings, numbers and keywords. Every
// line draws, a long one wrapped onto rows under its number.

const HEADER_H = 56
const LINE_H = 20
const CODE = 12.5

const KEYWORDS = new Set([
  'const', 'let', 'var', 'function', 'return', 'if', 'else', 'for', 'while', 'switch', 'case', 'break',
  'continue', 'import', 'from', 'export', 'default', 'class', 'new', 'async', 'await', 'try', 'catch',
  'throw', 'type', 'interface', 'extends', 'implements', 'def', 'fn', 'pub', 'struct', 'impl', 'enum',
  'match', 'use', 'mod', 'true', 'false', 'null', 'undefined', 'None', 'True', 'False', 'self', 'this',
  'in', 'of', 'as', 'with', 'lambda', 'yield', 'static', 'public', 'private', 'protected', 'void',
])

// Languages whose line comments start with `#`.
const HASH_COMMENTS = new Set(['py', 'python', 'sh', 'bash', 'zsh', 'shell', 'yaml', 'yml', 'toml', 'ps1', 'powershell', 'rb', 'ruby', 'r'])

export type Token = { text: string; role: 'plain' | 'comment' | 'string' | 'number' | 'keyword' }

export function tokenize(line: string, lang: string): Token[] {
  const comment = HASH_COMMENTS.has(lang) ? '#.*$' : '\\/\\/.*$|--\\s.*$'
  const pattern = new RegExp(`(${comment})|("(?:[^"\\\\]|\\\\.)*"|'(?:[^'\\\\]|\\\\.)*'|\`[^\`]*\`)|(\\b\\d[\\d_.]*\\b)|([A-Za-z_][\\w]*)`, 'g')
  const tokens: Token[] = []
  let last = 0

  for (const match of line.matchAll(pattern)) {
    const at = match.index ?? 0
    const plain = line.slice(last, at)

    if (plain !== '') {
      tokens.push({ text: plain, role: 'plain' })
    }

    const [text = ''] = match
    const role = match[1] !== undefined ? 'comment' : match[2] !== undefined ? 'string' : match[3] !== undefined ? 'number' : KEYWORDS.has(text) ? 'keyword' : 'plain'
    tokens.push({ text, role })
    last = at + text.length
  }

  return line.slice(last) === '' ? tokens : [...tokens, { text: line.slice(last), role: 'plain' }]
}

// A token list as rows at most `room` pixels wide, each character measured as breakText
// measures it, so a wide character takes the room of two; a token splits where a row ends.
function rowsOf(tokens: Token[], room: number): Token[][] {
  const rows: Token[][] = [[]]
  let used = 0

  for (const token of tokens) {
    let piece = ''

    for (const char of token.text) {
      const width = measure(char, true, CODE)

      if (used > 0 && used + width > room) {
        if (piece !== '') rows.at(-1)!.push({ text: piece, role: token.role })
        rows.push([])
        piece = ''
        used = 0
      }

      piece += char
      used += width
    }

    if (piece !== '') rows.at(-1)!.push({ text: piece, role: token.role })
  }

  return rows
}

// `hasControl` leaves the header's right corner free for a Copy button laid over it. Null
// for code whose card would be past MAX_SVG, which keeps Claude Code's own drawing.
export function codeSvg(code: string, lang: string, palette: Palette, width: number, hasControl = false): { source: string; width: number; height: number; alt: string; entranceMs: number } | null {
  const lines = code.replace(/\t/g, '  ').split('\n')
  const gutter = String(lines.length).length * 8 + 24
  const color: Readonly<Record<Token['role'], string>> = {
    plain: palette.fg,
    comment: palette.muted,
    string: palette.read,
    number: palette.warn,
    keyword: palette.user,
  }
  const wrapped = lines.map(line => rowsOf(tokenize(line, lang), width - gutter - 24))
  const count = wrapped.reduce((sum, rows) => sum + rows.length, 0)
  const step = staggerStep(count, 12)
  let at = 0

  const rows = wrapped.flatMap((lineRows, i) =>
    lineRows.map((tokens, r) => {
      const top = HEADER_H + 8 + at * LINE_H
      const number = r === 0 ? `<text x="${gutter - 12}" y="${top + 14}" text-anchor="end" font-family="${MONO}" font-size="11" style="fill:${palette.muted};fill-opacity:.7">${i + 1}</text>` : ''
      const spans = tokens
        .map(token => `<tspan style="fill:${color[token.role]}${token.role === 'keyword' ? ';font-weight:600' : ''}${token.role === 'comment' ? ';font-style:italic' : ''}">${escape(token.text)}</tspan>`)
        .join('')

      return `<g class="rise" ${riseDelay(at++, step, 60)}>${number}<text x="${gutter}" y="${top + 14}" font-family="${MONO}" font-size="${CODE}" xml:space="preserve">${spans}</text></g>`
    }),
  )

  const height = HEADER_H + 8 + count * LINE_H + 10
  const header = [
    `<text x="16" y="${HEADER_MID + 4}" font-size="11" style="fill:${palette.muted};letter-spacing:.1em;font-weight:600">${escape((lang || 'code').toUpperCase())}</text>`,
    `<text x="${width - 16 - (hasControl ? CONTROL_SLOT : 0)}" y="${HEADER_MID + 4}" text-anchor="end" font-size="11" style="fill:${palette.muted}">${lines.length} line${lines.length === 1 ? '' : 's'}</text>`,
    `<line x1="0" y1="${HEADER_H - 0.5}" x2="${width}" y2="${HEADER_H - 0.5}" stroke="${palette.muted}" stroke-opacity=".3"/>`,
    `<g transform="translate(0 ${HEADER_H})">${tint(palette, 0, gutter - 4, height - HEADER_H, 0.04)}</g>`,
  ].join('')
  const source = svgCard(width, height, palette, '', header + rows.join(''))

  return source.length > MAX_SVG
    ? null
    : {
        source,
        width,
        height,
        alt: `${lang || 'code'}:\n${fitText(code, 4000, true, CODE)}`,
        entranceMs: staggerEnd(count, step, 60),
      }
}
