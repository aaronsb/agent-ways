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

const MAX_TARGET = 120

// The tools whose target is the file they name, so it may wrap and is drawn whole.
export const PATH_TOOLS: ReadonlySet<string> = new Set(['Read', 'Edit', 'MultiEdit', 'Write', 'NotebookEdit'])

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

// The one thing about a call worth a glance: the command, the file, the pattern. `max`
// bounds its characters.
export function summarize(tool: string, input: unknown, cwd: string, max = MAX_TARGET): string {
  const fields = fieldsOf(input)

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
        return firstString(fields)
    }
  })()

  return oneLine(summary).slice(0, max)
}
