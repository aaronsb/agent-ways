// Ported from skins by hellosverre (MIT; LICENSE beside this file),
// https://github.com/hellosverre/claude-skins at 985ad63, hooks/command-output.ts.

import { splitReply } from './markdown'
import type { Segment, Table } from './markdown'

// What a slash command printed, as the segments a reply is drawn from: its own tables and
// fences, or a run of `key: value` lines (`/cost`, `/status`) as a two-column table. Null
// when there is nothing to draw, so the row keeps Claude Code's drawing.

const PAIR = /^\s*([^:|`]{1,40}?):\s+(\S.*?)\s*$/

// Colour codes some commands still print; a card cannot show them.
const ANSI = /\u001b\[[0-9;]*m/g

const pairOf = (line: string): [string, string] | null => {
  const match = PAIR.exec(line)

  return match === null ? null : [(match[1] ?? '').trim(), match[2] ?? '']
}

function pairTables(text: string, command: string): Segment[] {
  const segments: Segment[] = []
  let prose: string[] = []
  let pairs: [string, string][] = []

  const flushProse = () => {
    if (prose.join('\n').trim() !== '') {
      segments.push({ kind: 'text', text: prose.join('\n') })
    }

    prose = []
  }

  // One pair alone reads better as the line it was.
  const flushPairs = () => {
    if (pairs.length >= 2) {
      flushProse()
      const table: Table = { kind: 'table', header: [command, ''], align: ['left', 'left'], rows: pairs }
      segments.push(table)
    } else {
      prose.push(...pairs.map(([key, value]) => `${key}: ${value}`))
    }

    pairs = []
  }

  for (const line of text.split('\n')) {
    const pair = pairOf(line)

    if (pair !== null) {
      pairs.push(pair)
      continue
    }

    flushPairs()
    prose.push(line)
  }

  flushPairs()
  flushProse()

  return segments
}

export function commandSegments(raw: string, command: string): Segment[] | null {
  const text = raw.replace(ANSI, '')
  const split = splitReply(text)

  if (split.some(segment => segment.kind !== 'text')) {
    return split
  }

  const paired = pairTables(text, command === '' ? 'output' : command)

  return paired.some(segment => segment.kind === 'table') ? paired : null
}
