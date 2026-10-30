import { describe, expect, test } from 'claude-code/testing'
import type { Engine } from 'claude-code/testing'
import type { On, RenderElement } from 'claude-code'

import { parseExport } from '../hooks/skins/config.ts'
import { oneLine } from '../hooks/skins/format.ts'
import { PICKUP_MS } from '../hooks/skins/hub.tsx'
import { widthOf } from '../hooks/skins/markdown.ts'
import { pathLines } from '../hooks/skins/rows.tsx'
import { MIAMI_NIGHTS } from './fixtures.ts'
import { INTERACTIVE, host, json, walk } from './host.tsx'
import type { Node, World } from './host.tsx'

const KITTY = { TERM: 'xterm-kitty', HOME: '/home/dev' }
const COLUMNS = 60
// A framed row: the frame's four cells and the chevron's two.
const ROOM = COLUMNS - 4 - 2
// The node ahead of the label inside a frame, where the rail draws no branch: `● `.
const INDENT = 2

const at = (props: Record<string, unknown>, surface = 'terminal', columns = COLUMNS) =>
  ({ plugin: 'ways-theme', surface, component: 'ToolUse', requestId: 'r1', viewport: { columns, rows: 30 }, props }) as never

// A path of 129 characters, under no folder the session shortens it against.
const LONG = '/tmp/claude-1000/-home-aaron-Projects-ai-harness-agent-ways/55c9c23f-22df-457e-9446-90894eb6c3f4/scratchpad/ai-notes-for-review.md'
const PATCH = { structuredPatch: [{ lines: ['-old', '+new'] }] }
const edit = (file_path: string, extra: Record<string, unknown> = {}) => ({
  tool_use_id: 't1',
  tool: 'Edit',
  input: { file_path, old_string: 'old', new_string: 'new' },
  isRunning: false,
  isErrored: false,
  isInterrupted: false,
  isExpanded: false,
  output: PATCH,
  ...extra,
})

const withPaths = (paths: 'wrap' | 'truncate' | undefined) => {
  const { paths: _, ...render } = MIAMI_NIGHTS.render
  return json({ ...MIAMI_NIGHTS, render: paths === undefined ? render : { ...render, paths } })
}

const textOf = (x: unknown): string =>
  typeof x === 'string' ? x : Array.isArray(x) ? x.map(textOf).join('') : typeof x === 'object' && x !== null ? textOf((x as Node).children ?? []) : ''

// The column of target lines under the label, or undefined when the row drew on one line.
const wrapped = (tree: RenderElement): Node | undefined =>
  walk(tree).find(n => n.type === 'Box' && n.props?.flexDirection === 'column' && typeof n.props?.paddingLeft === 'number' && n.props?.flexShrink === undefined)

const pieces = (column: Node) =>
  (column.children ?? []).flat(Infinity as 1).filter((n): n is Node => typeof n === 'object' && n !== null && (n as Node).type === 'Text').map(n => ({
    text: textOf(n),
    color: n.props?.color,
    href: walk(n).find(m => m.type === 'Link')?.props?.href,
  }))

async function drawn($: Engine, on: On, props: Record<string, unknown>, world: World = {}, columns = COLUMNS) {
  host(on, { mod: json(MIAMI_NIGHTS), ...world })
  await $.session.start(INTERACTIVE)
  const ui = await $.ui.mount(at(props, 'terminal', columns))
  const tree = await ui.drawn()
  await ui.unmount()
  return tree
}

describe('pathLines', () => {
  test('a line ends after a /, and every character is kept', () => {
    const lines = pathLines('/work/proj/tools/ways-cli/src/cmd/settings.rs', 20)
    expect(lines).toEqual(['/work/proj/tools/', 'ways-cli/src/cmd/', 'settings.rs'])
  })

  test('a segment wider than the line breaks inside it, on a line of its own', () => {
    expect(pathLines(`/work/other/${'x'.repeat(30)}.ts`, 20)).toEqual(['/work/other/', 'x'.repeat(20), `${'x'.repeat(10)}.ts`])
  })

  test('cells, not characters, fill a line', () => {
    const lines = pathLines('/docs/日本語のファイル名/a.md', 10)
    expect(lines.join('')).toBe('/docs/日本語のファイル名/a.md')
    expect(lines.every(line => widthOf(line) <= 10)).toBe(true)
  })
})

describe('a tool row whose target path does not fit', () => {
  test('wraps after a /: the label and the facts keep the first line, every character drawn', async ($, on) => {
    const tree = await drawn($, on, edit(LONG))
    const column = wrapped(tree)
    expect(column?.props?.paddingLeft).toBe(INDENT)
    const lines = pieces(column!).map(p => p.text)
    expect(lines.join('')).toBe(LONG)
    expect(lines.length).toBeGreaterThan(1)
    expect(lines.every(line => widthOf(line) <= ROOM - INDENT)).toBe(true)
    expect(lines.slice(0, -1).every(line => line.endsWith('/'))).toBe(true)
    // The line above the target holds the label and the diffstat, and no part of the path.
    const row = walk(tree).find(n => n.type === 'Box' && n.props?.flexDirection === 'column' && (n.children ?? []).flat(Infinity as 1).includes(column))
    const head = textOf((row!.children ?? []).flat(Infinity as 1)[0])
    expect(head).toContain('Edit')
    expect(head).toContain('+1')
    expect(head).toContain(' −1')
    expect(head).not.toContain('/tmp')
  })

  test('the chevron sits on the first line', async ($, on) => {
    const tree = await drawn($, on, edit(LONG))
    const chevron = walk(tree).find(n => n.type === 'Box' && n.props?.paddingLeft === 1 && walk(n).some(m => m.type === 'Button'))
    const line = walk(tree).find(n => n.type === 'Box' && n.props?.flexDirection === 'row' && (n.children ?? []).flat(Infinity as 1).includes(chevron))
    expect(line?.props?.alignItems).toBe('flex-start')
    expect(chevron?.props?.marginTop).toBe(0)
  })

  test('unframed, the target aligns under the label past the rail branch', async ($, on) => {
    const tree = await drawn($, on, edit(LONG), { mod: json({ ...MIAMI_NIGHTS, render: { ...MIAMI_NIGHTS.render, frames: false } }) })
    const column = wrapped(tree)
    expect(column?.props?.paddingLeft).toBe(3)
    const lines = pieces(column!).map(p => p.text)
    expect(lines.join('')).toBe(LONG)
    expect(lines.every(line => widthOf(line) <= COLUMNS - 2 - 3)).toBe(true)
    // Under the rail's connector, the chevron drops one line to the label's.
    const chevron = walk(tree).find(n => n.type === 'Box' && n.props?.paddingLeft === 1 && walk(n).some(m => m.type === 'Button'))
    expect(chevron?.props?.marginTop).toBe(1)
  })

  test('a segment wider than the line breaks inside it', async ($, on) => {
    const long = `/work/other/${'x'.repeat(70)}.ts`
    const lines = pieces(wrapped(await drawn($, on, edit(long)))!).map(p => p.text)
    expect(lines).toEqual(['/work/other/', 'x'.repeat(ROOM - INDENT), `${'x'.repeat(70 - (ROOM - INDENT))}.ts`])
  })

  test('an errored call keeps the error colour on every line', async ($, on) => {
    const lines = pieces(wrapped(await drawn($, on, edit(LONG, { isErrored: true, output: 'File has been modified since read.' })))!)
    expect(lines.length).toBeGreaterThan(1)
    expect(lines.map(p => p.color)).toEqual(lines.map(() => MIAMI_NIGHTS.roles.err))
  })

  test('a file this turn edited keeps the edited colour on every line', async ($, on) => {
    host(on, { mod: json(MIAMI_NIGHTS) })
    on('tool.call', () => ({ result: { type: 'update' } }) as never)
    await $.session.start(INTERACTIVE)
    await $.tool.call({ tool: 'Edit', tool_use_id: 't1', file_path: LONG, old_string: 'old', new_string: 'new' } as never).catch(() => undefined)
    const ui = await $.ui.mount(at(edit(LONG)))
    const lines = pieces(wrapped(await ui.drawn())!)
    await ui.unmount()
    expect(lines.length).toBeGreaterThan(1)
    expect(lines.map(p => p.color)).toEqual(lines.map(() => MIAMI_NIGHTS.roles.warn))
  })

  test('each line links to the file', async ($, on) => {
    const lines = pieces(wrapped(await drawn($, on, edit(LONG), { env: KITTY }))!)
    expect(lines.length).toBeGreaterThan(1)
    expect(lines.map(p => p.href)).toEqual(lines.map(() => `file://${LONG}`))
  })

  test('with paths absent from the export, the path wraps', async ($, on) => {
    const export_ = parseExport(withPaths(undefined).stdout)
    expect(typeof export_ !== 'string' && export_.render.paths).toBe('wrap')
    expect(wrapped(await drawn($, on, edit(LONG), { mod: withPaths(undefined) }))).toBeDefined()
  })

  test('paths that are neither are refused', () => {
    expect(typeof parseExport(JSON.stringify({ ...MIAMI_NIGHTS, render: { ...MIAMI_NIGHTS.render, paths: 'fold' } }))).toBe('string')
  })
})

describe('where the row draws as before', () => {
  test('a row that fits draws as it does under truncate', async ($, on) => {
    const world: World = { mod: withPaths('wrap') }
    const h = host(on, world)
    await $.session.start(INTERACTIVE)
    const ui = await $.ui.mount(at(edit('/work/proj/src/a.ts')))
    const wrap = await ui.drawn()
    expect(wrapped(wrap)).toBeUndefined()
    expect(JSON.stringify(wrap)).toContain('  src/a.ts')
    world.mod = withPaths('truncate')
    await h.clock.advance(PICKUP_MS)
    expect(await ui.drawn()).toEqual(wrap)
    await ui.unmount()
  })

  test('truncate keeps a long path on the tool line, which the terminal cuts with an ellipsis', async ($, on) => {
    const tree = await drawn($, on, edit(LONG), { mod: withPaths('truncate') })
    expect(wrapped(tree)).toBeUndefined()
    const line = walk(tree).find(n => n.type === 'Text' && n.props?.wrap === 'truncate-end' && textOf(n).includes('Edit'))
    expect(textOf(line)).toContain(`  ${LONG}`)
  })

  test('a command is no path: it stays on its line, whole, and wraps', async ($, on) => {
    const command = `grep -rn ${'needle/'.repeat(30)} .`
    const tree = await drawn($, on, { tool_use_id: 't3', tool: 'Grep', input: { pattern: command }, isRunning: false, isErrored: false, isInterrupted: false, isExpanded: false, output: undefined })
    expect(wrapped(tree)).toBeUndefined()
    const line = walk(tree).find(n => n.type === 'Text' && n.props?.wrap === 'wrap' && textOf(n).includes('Grep'))
    expect(textOf(line)).toContain(`  ${command}`)
  })

  test('a Bash command past the row draws whole and wraps', async ($, on) => {
    const command = `git log --oneline ${'--author=someone '.repeat(10)}`.trim()
    const tree = await drawn($, on, { tool_use_id: 't5', tool: 'Bash', input: { command }, isRunning: false, isErrored: false, isInterrupted: false, isExpanded: false, output: undefined })
    const line = walk(tree).find(n => n.type === 'Text' && textOf(n).includes('Bash'))
    expect(line?.props?.wrap).toBe('wrap')
    expect(textOf(line)).toContain(`  ${command}`)
  })

  test("a command's later lines show as an ellipsis", async ($, on) => {
    const tree = await drawn($, on, { tool_use_id: 't6', tool: 'Bash', input: { command: 'cat <<EOF\nline two\nEOF' }, isRunning: false, isErrored: false, isInterrupted: false, isExpanded: false, output: undefined })
    const line = walk(tree).find(n => n.type === 'Text' && textOf(n).includes('Bash'))
    expect(textOf(line)).toContain('  cat <<EOF …')
    expect(textOf(line)).not.toContain('line two')
  })

  // An MCP tool's target is its first non-empty string field, which need not be its path.
  const mcp = (tool: string, input: Record<string, unknown>) => ({ tool_use_id: 't4', tool, input, isRunning: false, isErrored: false, isInterrupted: false, isExpanded: false, output: undefined })
  const MCP_CASES: [string, ReturnType<typeof mcp>, string][] = [
    ['content ahead of a file_path', mcp('mcp__fs__write', { content: 'lorem ipsum '.repeat(200), file_path: '/a/b.ts' }), 'lorem ipsum '.repeat(200)],
    ['a query beside an empty file_path', mcp('mcp__x__y', { file_path: '', query: 'q/'.repeat(300) }), 'q/'.repeat(300)],
  ]
  for (const [name, props, field] of MCP_CASES) {
    test(`an MCP tool with ${name} stays on its line, cut at 120 characters with an ellipsis`, async ($, on) => {
      const tree = await drawn($, on, props)
      expect(wrapped(tree)).toBeUndefined()
      const line = walk(tree).find(n => n.type === 'Text' && n.props?.wrap === 'wrap' && textOf(n).includes(':'))
      expect(textOf(line)).toContain(`  ${oneLine(field).slice(0, 120)}…`)
      expect(textOf(line)).not.toContain(oneLine(field).slice(0, 121))
    })
  }

  test('on the desktop the path stays on the tool line', async ($, on) => {
    host(on, { mod: json(MIAMI_NIGHTS) })
    await $.session.start({ ...INTERACTIVE, surface: 'desktop' })
    const ui = await $.ui.mount(at(edit(LONG), 'desktop'))
    expect(wrapped(await ui.drawn())).toBeUndefined()
    await ui.unmount()
  })
})
