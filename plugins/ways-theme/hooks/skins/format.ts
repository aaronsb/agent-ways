// Ported from skins by hellosverre (MIT; LICENSE beside this file),
// https://github.com/hellosverre/claude-skins at 985ad63, hooks/format.ts.

// Plain string helpers behind the row builders.

// One drawing draws at most 100000 characters in all, its `Code` and `Markdown` blocks of
// any length; text longer than this keeps Claude Code's own drawing.
export const MAX_DRAWN = 90000

// A terminal's escape sequences, payload and all: CSI (colour, cursor moves), OSC (a
// title, a hyperlink) to its BEL or ST, and two-character escapes; then every C0 control
// but tab and newline, and DEL. None of them can be drawn, and XML forbids the C0 ones.
export const CONTROLS = /\u001b\[[0-?]*[ -/]*[@-~]|\u001b\][^\u0007\u001b\n]*(?:\u0007|\u001b\\)?|\u001b[ -/]*[0-~]|[\u0000-\u0008\u000b-\u001f\u007f]/g

export const oneLine = (text: string): string =>
  (text.split('\n', 1)[0] ?? '').replace(/\s+/g, ' ').trim()

const slashed = (path: string): string => path.replace(/\\/g, '/')

// A path under the session's directory shows relative to it; any other stays as given.
export function shortenPath(path: string, cwd: string): string {
  const full = slashed(path)
  const root = slashed(cwd).replace(/\/+$/, '')

  if (root === '' || !full.toLowerCase().startsWith(`${root.toLowerCase()}/`)) {
    return path
  }

  return full.slice(root.length + 1)
}

export function formatDuration(ms: number): string {
  const seconds = Math.round(ms / 1000)

  if (seconds < 1) {
    return '<1s'
  }

  return seconds < 60 ? `${seconds}s` : `${Math.floor(seconds / 60)}m ${seconds % 60}s`
}

// A tool call's own time: `340ms`, `2.1s`, `1m 4s`.
export function formatMs(ms: number): string {
  if (ms < 1000) {
    return `${Math.round(ms)}ms`
  }

  return ms < 60_000 ? `${(ms / 1000).toFixed(1)}s` : formatDuration(ms)
}

// Lines added and removed, read from an Edit or Write record's patch.
export function diffstat(output: unknown): { added: number; removed: number } | null {
  const patch = (output as { structuredPatch?: unknown } | null)?.structuredPatch

  if (!Array.isArray(patch)) {
    return null
  }

  const lines = patch.flatMap(hunk => {
    const hunkLines = (hunk as { lines?: unknown } | null)?.lines

    return Array.isArray(hunkLines) ? hunkLines.filter(line => typeof line === 'string') : []
  }) as string[]

  return {
    added: lines.filter(line => line.startsWith('+')).length,
    removed: lines.filter(line => line.startsWith('-')).length,
  }
}

// The same seed always picks the same item, so a row keeps its word when it redraws.
export function pick<T>(items: readonly T[], seed: string): T | undefined {
  let hash = 5381

  for (let i = 0; i < seed.length; i++) {
    hash = (hash * 33 + seed.charCodeAt(i)) >>> 0
  }

  return items[hash % items.length]
}
