// Ported from skins by hellosverre (MIT; LICENSE beside this file),
// https://github.com/hellosverre/claude-skins at 985ad63, hooks/svg-code.ts.

import type { Palette } from './skin'
import { CONTROL_SLOT, escape, HEADER_MID, fitText, MONO, riseDelay, staggerEnd, staggerStep, svgCard, tint } from './svg-kit'

// A fenced code block as a card: the language and line count in a header, line numbers
// in a gutter, and light highlighting of comments, strings, numbers and keywords.

const HEADER_H = 56
const LINE_H = 20
const CODE = 12.5
const MAX_LINES = 80

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

// Cuts a token list to `max` characters, ending in an ellipsis when it lost text.
function cutTokens(tokens: Token[], max: number): Token[] {
  let room = max
  const kept: Token[] = []

  for (const token of tokens) {
    if (room <= 0) {
      break
    }

    const chars = [...token.text]

    if (chars.length <= room) {
      kept.push(token)
      room -= chars.length
    } else {
      kept.push({ text: `${chars.slice(0, Math.max(0, room - 1)).join('')}…`, role: token.role })
      room = 0
    }
  }

  return kept
}

// `hasControl` leaves the header's right corner free for a Copy button laid over it.
export function codeSvg(code: string, lang: string, palette: Palette, width: number, hasControl = false): { source: string; width: number; height: number; alt: string; entranceMs: number } {
  const all = code.replace(/\t/g, '  ').split('\n')
  const lines = all.slice(0, MAX_LINES)
  const step = staggerStep(lines.length, 12)
  const gutter = String(all.length).length * 8 + 24
  const maxChars = Math.floor((width - gutter - 24) / (CODE * 0.6))
  const color: Readonly<Record<Token['role'], string>> = {
    plain: palette.fg,
    comment: palette.muted,
    string: palette.read,
    number: palette.warn,
    keyword: palette.user,
  }

  const rows = lines.map((line, i) => {
    const top = HEADER_H + 8 + i * LINE_H
    const spans = cutTokens(tokenize(line, lang), maxChars)
      .map(token => `<tspan style="fill:${color[token.role]}${token.role === 'keyword' ? ';font-weight:600' : ''}${token.role === 'comment' ? ';font-style:italic' : ''}">${escape(token.text)}</tspan>`)
      .join('')

    return `<g class="rise" ${riseDelay(i, step, 60)}><text x="${gutter - 12}" y="${top + 14}" text-anchor="end" font-family="${MONO}" font-size="11" style="fill:${palette.muted};fill-opacity:.7">${i + 1}</text><text x="${gutter}" y="${top + 14}" font-family="${MONO}" font-size="${CODE}" xml:space="preserve">${spans}</text></g>`
  })

  const hidden = all.length - lines.length
  const footer = hidden > 0 ? `<text x="${gutter}" y="${HEADER_H + 8 + lines.length * LINE_H + 14}" font-size="11.5" style="fill:${palette.muted}">${hidden} more lines</text>` : ''
  const height = HEADER_H + 8 + lines.length * LINE_H + (hidden > 0 ? 28 : 10)
  const header = [
    `<text x="16" y="${HEADER_MID + 4}" font-size="11" style="fill:${palette.muted};letter-spacing:.1em;font-weight:600">${escape((lang || 'code').toUpperCase())}</text>`,
    `<text x="${width - 16 - (hasControl ? CONTROL_SLOT : 0)}" y="${HEADER_MID + 4}" text-anchor="end" font-size="11" style="fill:${palette.muted}">${all.length} line${all.length === 1 ? '' : 's'}</text>`,
    `<line x1="0" y1="${HEADER_H - 0.5}" x2="${width}" y2="${HEADER_H - 0.5}" stroke="${palette.muted}" stroke-opacity=".3"/>`,
    `<g transform="translate(0 ${HEADER_H})">${tint(palette, 0, gutter - 4, height - HEADER_H, 0.04)}</g>`,
  ].join('')

  return {
    source: svgCard(width, height, palette, '', header + rows.join('') + footer),
    width,
    height,
    alt: `${lang || 'code'}:\n${fitText(code, 4000, true, CODE)}`,
    entranceMs: staggerEnd(lines.length, step, 60),
  }
}
