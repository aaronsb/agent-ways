// Ported from skins by hellosverre (MIT; LICENSE beside this file),
// https://github.com/hellosverre/claude-skins at 985ad63, hooks/detail.ts.

import { CONTROLS, shortenPath } from './format'
import { SHELLS, shellResultOf } from './shell'
import type { ShellResult } from './shell'
import { hunksOf } from './svg-diff'
import type { DiffInput } from './svg-diff'

// What a tool row shows when it is opened: the call's input in full, then its result in
// the form the call's kind reads best in. Anything this cannot read is `stock`, which the
// row answers with Claude Code's own drawing, so an opened row is never empty.

export type Field = { label: string; value: string }

export type Body =
  | { kind: 'file'; path: string; text: string; startLine: number; total: number }
  | { kind: 'matches'; lines: string[]; note: string }
  | { kind: 'shell'; result: ShellResult }
  | { kind: 'diff'; diff: DiffInput }
  | { kind: 'text'; text: string; isError: boolean }
  | { kind: 'none' }
  | { kind: 'stock' }

export const isOpen = (disclosure: string, isErrored: boolean): boolean =>
  disclosure === 'open' || (disclosure !== 'closed' && isErrored)

type Fields = Readonly<Record<string, unknown>>

// Tab and newline are the only control characters a drawn string may hold.
export const clean = (text: string): string => text.replace(/\r\n?/g, '\n').replace(CONTROLS, '')

const fieldsOf = (value: unknown): Fields => (typeof value === 'object' && value !== null ? (value as Fields) : {})

// Text the edit's diff already shows, which would only repeat it above.
const SHOWN_BY_DIFF = new Set(['old_string', 'new_string', 'content', 'edits'])

const LABELS: Readonly<Record<string, string>> = {
  file_path: 'path',
  notebook_path: 'path',
  run_in_background: 'background',
  output_mode: 'mode',
  '-i': 'ignore case',
  '-n': 'line numbers',
  '-A': 'after',
  '-B': 'before',
  '-C': 'context',
  head_limit: 'limit',
}

const valueText = (value: unknown): string =>
  typeof value === 'string' ? value : typeof value === 'number' || typeof value === 'boolean' ? String(value) : JSON.stringify(value, null, 2)

// A read's `offset` and `limit` as the line range they ask for.
function rangeOf(fields: Fields): Field[] {
  const offset = typeof fields.offset === 'number' ? fields.offset : undefined
  const limit = typeof fields.limit === 'number' ? fields.limit : undefined

  if (offset === undefined && limit === undefined) {
    return []
  }

  const from = offset ?? 1

  return [{ label: 'lines', value: limit === undefined ? `${from}–end` : `${from}–${from + limit - 1}` }]
}

// The call's input, each field as given; paths under the session's folder made relative.
export function inputFields(tool: string, input: unknown, cwd: string): Field[] {
  const fields = fieldsOf(input)
  const isRead = tool === 'Read'
  const isEdit = tool === 'Edit' || tool === 'MultiEdit' || tool === 'Write' || tool === 'NotebookEdit'
  const listed = Object.entries(fields)
    .filter(([, value]) => value !== undefined && value !== null && value !== '')
    .filter(([key]) => !(isRead && (key === 'offset' || key === 'limit')) && !(isEdit && SHOWN_BY_DIFF.has(key)))
    .map(([key, value]) => ({
      label: LABELS[key] ?? key,
      value: /path$/.test(key) && typeof value === 'string' ? shortenPath(value, cwd) : valueText(value),
    }))

  return isRead ? [...listed, ...rangeOf(fields)] : listed
}

// A failed call's output is the text the model read, its error tags taken off.
const errorText = (output: unknown): string =>
  (typeof output === 'string' ? output : valueText(output)).replace(/<\/?tool_use_error>/g, '').trim()

function readBody(output: unknown): Body {
  const record = fieldsOf(output)
  const file = fieldsOf(record.file)

  if (record.type !== 'text' || typeof file.content !== 'string') {
    // Images, PDFs and notebooks are Claude Code's to draw.
    return { kind: 'stock' }
  }

  return {
    kind: 'file',
    path: typeof file.filePath === 'string' ? file.filePath : '',
    text: clean(file.content).replace(/\n$/, ''),
    startLine: typeof file.startLine === 'number' ? file.startLine : 1,
    total: typeof file.totalLines === 'number' ? file.totalLines : 0,
  }
}

const count = (n: number, word: string) => `${n} ${word}${n === 1 ? '' : 's'}`

// Grep answers with matching lines, matching files or counts; Glob with files.
function searchBody(output: unknown, cwd: string): Body {
  const record = fieldsOf(output)
  const files = Array.isArray(record.filenames) ? record.filenames.filter((name): name is string => typeof name === 'string') : []
  const truncated = record.truncated === true ? ', truncated' : ''

  if (typeof record.content === 'string' && record.content.trim() !== '') {
    const lines = clean(record.content).replace(/\n$/, '').split('\n')
    const numLines = typeof record.numLines === 'number' ? record.numLines : lines.length

    return { kind: 'matches', lines: lines.map(line => shortenPath(line, cwd)), note: `${count(numLines, 'line')}${truncated}` }
  }

  if (files.length === 0 && typeof record.numFiles !== 'number') {
    return typeof output === 'string' ? { kind: 'matches', lines: output.trim().split('\n'), note: '' } : { kind: 'stock' }
  }

  return files.length === 0
    ? { kind: 'none' }
    : { kind: 'matches', lines: files.map(file => shortenPath(file, cwd)), note: `${count(files.length, 'file')}${truncated}` }
}

function webBody(tool: string, output: unknown): Body {
  const record = fieldsOf(output)

  if (tool === 'WebFetch' && typeof record.result === 'string') {
    return { kind: 'text', text: clean(record.result), isError: false }
  }

  if (tool === 'WebSearch' && Array.isArray(record.results)) {
    const lines = record.results.flatMap(result =>
      typeof result === 'string'
        ? [result]
        : (Array.isArray(fieldsOf(result).content) ? (fieldsOf(result).content as unknown[]) : []).map(hit => {
            const link = fieldsOf(hit)

            return `${valueText(link.title)}  ${valueText(link.url)}`
          }),
    )

    return { kind: 'matches', lines, note: count(lines.length, 'result') }
  }

  return { kind: 'stock' }
}

// An MCP tool's result: its text blocks, else the record as JSON; an image is stock.
function genericBody(output: unknown): Body {
  if (typeof output === 'string') {
    return output.trim() === '' ? { kind: 'none' } : { kind: 'text', text: clean(output), isError: false }
  }

  const blocks = Array.isArray(output) ? output : Array.isArray(fieldsOf(output).content) ? (fieldsOf(output).content as unknown[]) : null

  if (blocks !== null) {
    if (blocks.some(block => fieldsOf(block).type === 'image')) {
      return { kind: 'stock' }
    }

    const text = blocks.map(block => valueText(fieldsOf(block).text ?? block)).join('\n')

    return text.trim() === '' ? { kind: 'none' } : { kind: 'text', text: clean(text), isError: false }
  }

  return output === undefined || output === null ? { kind: 'none' } : { kind: 'text', text: clean(valueText(output)), isError: false }
}

// The result of a finished call, as the opened row draws it; `none` while it runs.
export function bodyOf(tool: string, output: unknown, isErrored: boolean, isRunning: boolean, cwd: string): Body {
  if (isRunning || output === undefined) {
    return { kind: 'none' }
  }

  if (SHELLS.has(tool)) {
    const result = shellResultOf(output, isErrored)

    // A command sent to the background, or one whose output is an image, is stock.
    return result === null || fieldsOf(output).backgroundTaskId !== undefined || fieldsOf(output).isImage === true
      ? { kind: 'stock' }
      : { kind: 'shell', result }
  }

  if (isErrored) {
    return { kind: 'text', text: clean(errorText(output)), isError: true }
  }

  switch (tool) {
    case 'Read':
      return readBody(output)
    case 'Grep':
    case 'Glob':
      return searchBody(output, cwd)
    case 'Edit':
    case 'MultiEdit':
    case 'Write': {
      const diff = hunksOf(output)

      return diff === null ? { kind: 'stock' } : { kind: 'diff', diff }
    }
    case 'WebFetch':
    case 'WebSearch':
      return webBody(tool, output)
    case 'NotebookEdit':
      return { kind: 'stock' }
    default:
      return genericBody(output)
  }
}

// A diff's hunks as unified-diff text, for the engine's `Code` in its `diff` format.
export function unifiedDiff(diff: DiffInput): string {
  return diff.hunks
    .map(hunk => {
      const lines = hunk.lines.filter(line => !line.startsWith('\\'))
      const oldCount = lines.filter(line => !line.startsWith('+')).length
      const newCount = lines.filter(line => !line.startsWith('-')).length

      return [`@@ -${hunk.oldStart},${oldCount} +${hunk.newStart},${newCount} @@`, ...lines].join('\n')
    })
    .join('\n')
}
