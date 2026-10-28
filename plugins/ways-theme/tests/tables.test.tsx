import { describe, expect, test } from 'claude-code/testing'
import type { RenderElement } from 'claude-code'

import { columnWidths, splitReply, widthOf, wrapLines } from '../hooks/skins/markdown.ts'
import type { Table } from '../hooks/skins/markdown.ts'
import { MIAMI_NIGHTS } from './fixtures.ts'
import { INTERACTIVE, host, json, walk } from './host.tsx'
import type { Node } from './host.tsx'

// The table from a reply drawn with lines cut short, a header cut, and a row drawn twice:
// six columns, cells of several lines, continuation rows whose first two cells are empty.
const DECISIONS = [
  '| # | Decision | Options | For | Against | Recommendation |',
  '|---|---|---|---|---|---|',
  `| 1 | A panic in one font source (D14). vtium aborts on panic and the sandbox allows no child processes, so a panic, fault or hang in one source kills the whole helper. Today D14's "fails that source and nothing else" holds only for ordinary errors. | (a) Before drawing from a source, the helper names it. After a crash restart, the console skips the source named last, for the rest of the boot. | One bad font can't take down the rest. No new processes, no sandbox change. | A little protocol, and one extra restart's worth of delay on the first crash. | (a) |`,
  '|  |  | (b) Accept it: a crashing source gives up the whole frit once the restart cap is reached. | Nothing to build. | One broken font in a frit silently loses every other source for the boot. |  |',
  "|  |  | (c) One child process per source. | Exact isolation. | Breaks D15's no-children sandbox, adds process cost on minspec. |  |",
  "| 2 | The binary size budget, now about 62 KB over 2.5 MiB. You've accepted it for now. | (a) Decide after S4, once the store's code is gone and the size is re-measured. | The store removal returns some bytes, and the real figure is known before choosing. | The overrun stays a few more days. | (a) |",
  "|  |  | (b) Raise the budget now. | Unblocks tier 3's exit code. | Sets the limit before knowing what S4 frees. |  |",
  "|  |  | (c) Shrink the built-in unifont now (it's about half the binary). | The big lever. | It's the fallback when the helper is down; what to cut is a design question of its own. |  |",
  "| 3 | Reviewing the provisional goldens: the specimen profiles (#142) and the tier 3 frit captures. | (a) Review once S2 and S3 land. | They'll stop moving then, so you review once. | Stays provisional a bit longer. | (a) |",
  "|  |  | (b) Review now. | Earlier sign-off. | S2's cell rules and S3's engine path may move them again. |  |",
].join('\n')

const TABLE = splitReply(DECISIONS)[0] as Table

const reply = (isFirstOfReply: boolean) => ({ text: DECISIONS, isFirstOfReply })
const at = (columns: number, props: Record<string, unknown>) =>
  ({ plugin: 'ways-theme', surface: 'terminal', component: 'AssistantMessage', requestId: 'r1', viewport: { columns, rows: 50 }, props }) as never

const num = (value: unknown): number => (typeof value === 'number' ? value : 0)
const textOf = (node: Node): string => (node.children ?? []).map(child => (typeof child === 'string' ? child : textOf(child as Node))).join('')
const kids = (node: Node): Node[] => (node.children ?? []).filter((child): child is Node => typeof child === 'object' && child !== null)

// A cell, as the kit hands it: a column Box with the table's padding.
const isCell = (node: Node) => node.type === 'Box' && node.props?.flexDirection === 'column' && node.props?.paddingX !== undefined
const isRow = (node: Node) => node.type === 'Box' && node.props?.flexDirection === 'row' && kids(node).length === TABLE.header.length && kids(node).every(isCell)

// The kit hands the tree, not the terminal's paint. This lays the table out the way the
// terminal does, to the cell: the room is the viewport's columns less the reply's frame;
// a column of fixed width takes it, a fluid one (width 0, flexGrow) takes its share of
// what the table's border leaves, rounded down; a line wider than its column is cut to
// it with `…`, as `wrap="truncate-end"` cuts it.
type Painted = { room: number; drawn: number; rows: { cells: string[][]; height: number }[] }

function paint(tree: RenderElement, columns: number): Painted {
  const nodes = walk(tree)
  const frame = nodes.find(n => n.type === 'Box' && n.props?.borderStyle !== undefined && n.props?.marginTop === 1 && !kids(n).some(isRow))
  const room = columns - (frame === undefined ? 0 : 2 + 2 * num(frame.props?.paddingX))
  const rows = nodes.filter(isRow)
  const first = kids(rows[0] ?? {})
  const isFluid = first.some(cell => cell.props?.flexGrow !== undefined)
  const grows = first.reduce((sum, cell) => sum + num(cell.props?.flexGrow), 0)
  const inside = room - 2
  const widthAt = (cell: Node) => (isFluid ? Math.floor((num(cell.props?.flexGrow) / grows) * inside) : num(cell.props?.width))
  const drawn = first.reduce((sum, cell) => sum + widthAt(cell), 2)
  const cut = (line: string, width: number) => {
    if (widthOf(line) <= width) return line
    let kept = ''
    for (const char of line) {
      if (widthOf(kept + char) > width - 1) break
      kept += char
    }
    return `${kept}…`
  }
  return {
    room,
    drawn,
    rows: rows.map(row => {
      const cells = kids(row).map(cell => {
        const inner = widthAt(cell) - 2 * num(cell.props?.paddingX)
        return kids(cell)
          .filter(n => n.type === 'Text')
          .map(text => cut(textOf(text), inner))
      })
      return { cells, height: Math.max(...cells.map(lines => lines.length)) }
    }),
  }
}

const words = (text: string) => text.split(/\s+/).filter(Boolean)

// The part of a test's engine this file uses: mount a component, read its tree.
type Kit = { ui: { mount: (input: never) => Promise<{ drawn: () => Promise<RenderElement>; unmount: () => Promise<unknown> }> } }

async function drawn($: Kit, columns: number, isFirstOfReply: boolean): Promise<Painted> {
  const ui = await $.ui.mount(at(columns, reply(isFirstOfReply)))
  const painted = paint(await ui.drawn(), columns)
  await ui.unmount()
  return painted
}

describe('a table drawn in the terminal', () => {
  // 117 columns, framed, is the reply as it was reported; 80 and 150 frame it narrow and
  // wide; unframed, the reply takes the terminal's whole width.
  const CASES: [string, number, boolean][] = [
    ['framed at 117 columns', 117, true],
    ['framed at 80 columns', 80, true],
    ['framed at 150 columns', 150, true],
    ['unframed at 117 columns', 117, false],
  ]

  for (const [name, columns, isFramed] of CASES) {
    test(`${name}: fits the room it is drawn in`, async ($, on) => {
      host(on, { mod: json(MIAMI_NIGHTS) })
      await $.session.start(INTERACTIVE)
      const painted = await drawn($, columns, isFramed)
      expect(painted.room).toBe(isFramed ? columns - 4 : columns)
      expect(painted.drawn).toBeLessThanOrEqual(painted.room)
    })

    test(`${name}: no cell line is cut, and every word of every cell is drawn`, async ($, on) => {
      host(on, { mod: json(MIAMI_NIGHTS) })
      await $.session.start(INTERACTIVE)
      const painted = await drawn($, columns, isFramed)
      const cells = [TABLE.header, ...TABLE.rows]
      expect(painted.rows.length).toBe(cells.length)
      painted.rows.forEach((row, r) => {
        row.cells.forEach((lines, c) => {
          for (const line of lines) expect(line).not.toContain('…')
          expect(words(lines.join(' '))).toEqual(words(cells[r]?.[c] ?? ''))
        })
      })
    })

    test(`${name}: the header is drawn whole`, async ($, on) => {
      host(on, { mod: json(MIAMI_NIGHTS) })
      await $.session.start(INTERACTIVE)
      const painted = await drawn($, columns, isFramed)
      expect(painted.rows[0]?.cells.map(lines => lines.join(' '))).toEqual(TABLE.header)
    })

    test(`${name}: each row is drawn once, in order, as tall as its tallest cell`, async ($, on) => {
      host(on, { mod: json(MIAMI_NIGHTS) })
      await $.session.start(INTERACTIVE)
      const painted = await drawn($, columns, isFramed)
      const keys = painted.rows.slice(1).map(row => row.cells.map(lines => lines.join(' ')).join('|'))
      expect(keys).toEqual(TABLE.rows.map(row => row.join('|')))
      expect(new Set(keys).size).toBe(keys.length)
      // Every Text is one terminal line: none wraps on its own, so the height the tree
      // lays out is the line count the table wrapped to.
      for (const row of painted.rows) expect(row.height).toBe(Math.max(...row.cells.map(lines => lines.length)))
    })
  }
})

describe('column widths', () => {
  const budget = (widths: number[], gap: number) => widths.reduce((sum, width) => sum + width, 0) + gap * (widths.length - 1)
  const longest = (col: number) => Math.max(...[TABLE.header[col] ?? '', ...TABLE.rows.map(row => row[col] ?? '')].flatMap(words).map(widthOf))

  test('no column is narrowed below its longest word while the words fit', () => {
    const words = budget(TABLE.header.map((_, col) => longest(col)), 2)
    // An 80-column terminal's framed table has 72 cells for its columns and gaps.
    expect(words).toBeLessThanOrEqual(72)
    for (const room of [words, 72, 109, 142]) {
      const widths = columnWidths(TABLE, room, 2)
      expect(budget(widths, 2)).toBeLessThanOrEqual(room)
      widths.forEach((width, col) => expect(width).toBeGreaterThanOrEqual(longest(col)))
    }
  })

  test('a table whose words cannot fit still fits, no column narrowed below three', () => {
    const widths = columnWidths(TABLE, 40, 2)
    expect(budget(widths, 2)).toBeLessThanOrEqual(40)
    widths.forEach((width, col) => expect(width).toBeGreaterThanOrEqual(Math.min(3, longest(col))))
  })

  test('a word longer than its column breaks inside, between graphemes, and loses nothing', () => {
    const word = 'https://example.com/a/very/long/path/👩‍👩‍👧/end'
    const lines = wrapLines(`see ${word} here`, 12, Infinity)
    for (const line of lines) expect(widthOf(line)).toBeLessThanOrEqual(12)
    expect(lines.join('').replace(/\s/g, '')).toBe(`see${word}here`)
    expect(lines.some(line => line.includes('👩‍👩‍👧'))).toBe(true)
  })
})
