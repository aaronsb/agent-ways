// Ported from skins by hellosverre (MIT; LICENSE beside this file),
// https://github.com/hellosverre/claude-skins at 985ad63, hooks/tools.ts.

import { oneLine, shortenPath } from './format'
import type { ToolKind } from './skin'

const BUILTIN: Readonly<Record<string, ToolKind>> = {
  Bash: 'run',
  PowerShell: 'run',
  Read: 'read',
  Edit: 'write',
  MultiEdit: 'write',
  Write: 'write',
  NotebookEdit: 'write',
  Glob: 'search',
  Grep: 'search',
  WebFetch: 'web',
  WebSearch: 'web',
}

// The characters of another tool's first string field a row's target takes.
const MAX_TARGET = 120

// The tools whose target is the file they name, so it may wrap and is drawn whole.
export const PATH_TOOLS: ReadonlySet<string> = new Set(['Read', 'Edit', 'MultiEdit', 'Write', 'NotebookEdit'])

// The tools whose call row Claude Code draws empty: their user-facing name is
// empty in Claude Code 2.1.312, and what they show is their result, a dialog or
// a panel. A frame around such a row would hold nothing.
export const NO_ROW: ReadonlySet<string> = new Set([
  'AskUserQuestion',
  'EnterPlanMode',
  'ExitPlanMode',
  'ScheduleWakeup',
  'SendUserFile',
  'SendUserMessage',
  'TodoWrite',
  'ToolSearch',
])

const isImage = (value: unknown): boolean => typeof value === 'object' && value !== null && (value as { isImage?: unknown }).isImage === true

// Whether a call's result is a picture another mod may draw: a Read that
// returned an image, or a SendUserFile with an image among its files.
export function showsImage(tool: string, output: unknown): boolean {
  const fields = fieldsOf(output)
  if (tool === 'Read') return fields.type === 'image'
  if (tool === 'SendUserFile') return Array.isArray(fields.attachments) && fields.attachments.some(isImage)
  return false
}

// Null leaves the row to Claude Code: agents, plan mode and todos draw live progress
// and dialogs that a skin would have to rebuild.
export function kindOf(tool: string): ToolKind | null {
  if (tool.startsWith('mcp__')) {
    return 'mcp'
  }

  return BUILTIN[tool] ?? null
}

// `mcp__github__search_code` reads as `github:search_code`.
export function toolLabel(tool: string): string {
  if (!tool.startsWith('mcp__')) {
    return tool
  }

  const [, server = '', ...name] = tool.split('__')

  return `${server}:${name.join('__')}`
}

type Fields = Readonly<Record<string, unknown>>

const fieldsOf = (input: unknown): Fields =>
  typeof input === 'object' && input !== null ? (input as Fields) : {}

const textOf = (value: unknown): string => (typeof value === 'string' ? value : '')

const firstString = (fields: Fields): string =>
  Object.values(fields).map(textOf).find(value => value !== '') ?? ''

// The one thing about a call worth a glance, on one line: the command, the file, the
// pattern, whole. A command's later lines show as `…`, and another tool's first string
// field is cut at MAX_TARGET characters with `…`; the opened row shows the input in full.
export function summarize(tool: string, input: unknown, cwd: string): string {
  const fields = fieldsOf(input)
  let isLabel = false

  const summary = ((): string => {
    switch (tool) {
      case 'Bash':
      case 'PowerShell':
        return textOf(fields.command)
      case 'Read':
      case 'Edit':
      case 'MultiEdit':
      case 'Write':
        return shortenPath(textOf(fields.file_path), cwd)
      case 'NotebookEdit':
        return shortenPath(textOf(fields.notebook_path), cwd)
      case 'Glob':
        return textOf(fields.pattern)
      case 'Grep': {
        const where = textOf(fields.path)

        return where === ''
          ? textOf(fields.pattern)
          : `${textOf(fields.pattern)} in ${shortenPath(where, cwd)}`
      }
      case 'WebFetch':
        return textOf(fields.url).replace(/^https?:\/\//, '')
      case 'WebSearch':
        return textOf(fields.query)
      default:
        isLabel = true
        return firstString(fields)
    }
  })()

  const line = oneLine(summary)
  const chars = [...line]
  const shown = isLabel && chars.length > MAX_TARGET ? `${chars.slice(0, MAX_TARGET).join('')}…` : line
  const hasMore = summary.trim().includes('\n')

  return hasMore ? `${shown} …` : shown
}
