// Ported from skins by hellosverre (MIT; LICENSE beside this file),
// https://github.com/hellosverre/claude-skins at 985ad63, hooks/shell.ts.

import { shellOutputOf, streamLines } from './svg-terminal'
import type { ShellOutput } from './svg-terminal'

// What a shell call's output card shows on the terminal: its lines by stream, how it
// ended, and long runs folded to their head and tail.

export type ShellResult = ShellOutput & { exitCode?: number }

export type ShellLine = { text: string; isErr: boolean }

export type Folded<T> = T | { fold: number }

export type ShellStatus = { tone: 'ok' | 'err' | 'warn'; text: string }

export const SHELLS = new Set(['Bash', 'PowerShell'])

// A command sent to the background, or one whose output is an image, has nothing a card
// can show: Claude Code's own row says where it went or draws the image.
export function keepsOwnRow(output: unknown): boolean {
  const record = typeof output === 'object' && output !== null ? (output as Record<string, unknown>) : {}

  return typeof record.backgroundTaskId === 'string' || record.isImage === true
}

// `Exit code 2`, the line a failed call's error text opens with.
const EXIT_LINE = /^\s*Exit code (-?\d+)[^\S\n]*\r?\n?/

// A failed call can carry its error text in place of the record: then the exit code is
// read off its first line and the rest is the error, stdout and stderr already merged.
export function shellResultOf(output: unknown, isErrored: boolean): ShellResult | null {
  const record = shellOutputOf(output)

  if (record !== null) {
    const exit = EXIT_LINE.exec(record.stderr)

    return exit === null ? record : { ...record, stderr: record.stderr.slice(exit[0].length), exitCode: Number(exit[1]) }
  }

  if (typeof output !== 'string') {
    return null
  }

  const text = output.replace(/<\/?tool_use_error>/g, '')

  if (!isErrored) {
    return { stdout: text, stderr: '', interrupted: false }
  }

  const exit = EXIT_LINE.exec(text)

  return exit === null
    ? { stdout: '', stderr: text, interrupted: false }
    : { stdout: '', stderr: text.slice(exit[0].length), interrupted: false, exitCode: Number(exit[1]) }
}

export const linesOf = (result: ShellResult): { stdout: ShellLine[]; stderr: ShellLine[] } => ({
  stdout: streamLines(result.stdout, false),
  stderr: streamLines(result.stderr, true),
})

// The first `head` and last `tail` lines with a count of the rest between them. A single
// hidden line is shown instead: its fold row would take the same room.
export function foldLines<T>(lines: readonly T[], head: number, tail: number): Folded<T>[] {
  const hidden = lines.length - head - tail

  return hidden <= 1 ? [...lines] : [...lines.slice(0, head), { fold: hidden }, ...lines.slice(lines.length - tail)]
}

// How the call ended, worst first; an exit code only where the call reported one.
export function shellStatus(result: ShellResult, isErrored: boolean, ascii: boolean): ShellStatus {
  if (result.interrupted) {
    return { tone: 'warn', text: 'interrupted' }
  }

  if (result.timedOutAfterMs !== undefined) {
    return { tone: 'warn', text: 'timed out' }
  }

  if (isErrored) {
    return { tone: 'err', text: `${ascii ? 'x' : '✗'} ${result.exitCode === undefined ? 'failed' : `exit ${result.exitCode}`}` }
  }

  const note = result.returnCodeInterpretation === undefined ? '' : ` ${result.returnCodeInterpretation}`

  return { tone: 'ok', text: `${ascii ? 'ok' : '✓'}${note}` }
}
