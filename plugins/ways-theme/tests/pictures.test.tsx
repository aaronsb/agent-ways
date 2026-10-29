// Rows that carry another mod's picture, and rows Claude Code draws empty.
// The stand-in below draws as the images mod (images@dotfiles) does: under
// the row `next(e)` gave it, a column of a picture (a half-block `Raster`, or
// an `Image` in its `image` mode) sized to the transcript's width less its
// indent, then its caption line, the file's name as a `file://` link and the
// size and scale beside it. For a row whose requestId holds `host` it also
// does what the images mod would need to, sitting above the theme, to draw
// inside the theme's frame: given a bordered Box another plugin drew, it sizes
// the picture to the room inside the border and adds it under that Box's child.
// An inline plugin's register closes over nothing of the test file, so the
// stand-in reads its modes off the requestId: an `Image` for one that starts
// `image`, half-blocks for any other.

import { describe, expect, test } from 'claude-code/testing'
import type { Plugin } from 'claude-code/testing'
import type { RenderElement } from 'claude-code'

import { MIAMI_NIGHTS } from './fixtures.ts'
import { INTERACTIVE, borders, host, json, shows, walk } from './host.tsx'
import type { Node } from './host.tsx'

const VIEW = { columns: 100, rows: 30 }
const at = (surface: string, component: string, props: Record<string, unknown>, requestId = 'r1') => ({ plugin: 'ways-theme', surface, component, requestId, viewport: VIEW, props }) as never

const PNG = '/tmp/figures/ADR-516-item-card.png'
const SENT = { tool_use_id: 't1', tool: 'SendUserFile', output: { caption: 'the item cards', attachments: [{ path: PNG, size: 80_590, isImage: true }] }, isErrored: false }
const READ_IMAGE = { tool_use_id: 't2', tool: 'Read', input: { file_path: PNG }, isRunning: false, isErrored: false, isInterrupted: false, isExpanded: false, output: { type: 'image', file: { base64: 'iVBORw0KGgo=', type: 'image/png' } } }
const READ_TEXT = { ...READ_IMAGE, tool_use_id: 't3', input: { file_path: '/work/proj/a.ts' }, output: { type: 'text', file: { content: 'one', numLines: 1 } } }
const GROUP_IMAGE = { calls: [{ tool: 'Read', input: READ_IMAGE.input, isRunning: false, isErrored: false, isInterrupted: false, output: READ_IMAGE.output }], isActive: false, isExpanded: false }
const REPLY_IMAGE = { text: `Saved the chart to ${PNG}.`, isFirstOfReply: true }
const CALL = { isRunning: false, isErrored: false, isInterrupted: false, isExpanded: false }

const standIn = (tier?: Plugin['tier']): Plugin => ({
  name: 'images',
  ...(tier === undefined ? {} : { tier }),
  register(on) {
    const INDENT = 5
    const FILE = '/tmp/figures/ADR-516-item-card.png'
    // One `▀` cell, orange over the terminal's default, little-endian u32s.
    const cell = String.fromCharCode(0x80, 0x25, 0, 0, 0x00, 0x88, 0xff, 0, 0, 0, 0, 1)
    const draws = (component: string, p: Record<string, unknown>): boolean => {
      const output = p.output as { type?: unknown; attachments?: unknown } | undefined
      if (component === 'ToolResult') return p.tool === 'SendUserFile' && Array.isArray(output?.attachments)
      if (component === 'ToolUse') return p.tool === 'Read' && output?.type === 'image'
      if (component === 'ToolGroup') return p.isExpanded === false
      if (component === 'AssistantMessage') return typeof p.text === 'string' && p.text.includes(FILE)
      return false
    }
    on('ui.render', async ($, e, next) => {
      const own = await next(e)
      if (e.surface !== 'terminal' || !draws(e.component, e.props as Record<string, unknown>)) return own
      const { Box, Image, Link, Raster, Text } = $.ui.resolve(e)
      const frame = own as { type?: unknown; props?: Record<string, unknown>; children?: unknown[]; part?: { plugin?: unknown } }
      const hosts = e.requestId.includes('host') && frame.type === 'Box' && typeof frame.props?.borderStyle === 'string' && frame.part?.plugin !== 'images'
      const inset = hosts ? 2 + 2 * (typeof frame.props?.paddingX === 'number' ? frame.props.paddingX : 0) : 0
      const columns = (e.viewport?.columns ?? 80) - inset - INDENT - 1
      const rows = 4
      const picture =
        e.requestId.startsWith('image') ? (
          <Image key="images-0" source={{ file: '/run/claude-images/abc.png', format: 'png' }} columns={columns} rows={rows} alt="image: ADR-516-item-card.png" />
        ) : (
          <Raster key="images-0" cells={btoa(cell.repeat(columns * rows))} columns={columns} rows={rows} />
        )
      const drawn = (
        <Box flexDirection="column" paddingLeft={INDENT}>
          <Box flexDirection="column">
            {picture}
            <Box>
              <Link href={`file://${FILE}`} label="ADR-516-item-card.png" />
              <Text dimColor>{' · 1020×300 · 90%'}</Text>
            </Box>
          </Box>
        </Box>
      )
      if (hosts) {
        return {
          ...frame,
          children: [
            <Box flexDirection="column">
              {frame.children as never}
              {drawn}
            </Box>,
          ],
        } as never
      }
      return (
        <Box flexDirection="column">
          {own}
          {drawn}
        </Box>
      )
    })
  },
})

/** The host, with Claude Code's own drawing of each row beneath every plugin
 *  as the engine answers it, a node of its own. */
function hostOver(on: Parameters<typeof host>[0], world: Parameters<typeof host>[1]) {
  const engine = () => ({ type: 'engine', ref: 0 }) as never
  on('ui.render', { component: 'ToolUse' }, engine)
  on('ui.render', { component: 'ToolGroup' }, engine)
  on('ui.render', { component: 'ToolResult' }, engine)
  on('ui.render', { component: 'AssistantMessage' }, engine)
  return host(on, world)
}

/** Each picture in the tree, with the columns the Boxes around it take. */
function pictures(tree: unknown, inset = 0, out: { node: Node; inset: number }[] = []) {
  const node = tree as Node | null
  if (node === null || typeof node !== 'object') return out
  if (node.type === 'Image' || node.type === 'Raster') out.push({ node, inset })
  const p = node.type === 'Box' ? (node.props ?? {}) : {}
  const num = (...names: string[]) => names.map(name => p[name]).find(v => typeof v === 'number') as number | undefined
  const own = (typeof p.borderStyle === 'string' ? 2 : 0) + (num('paddingLeft', 'paddingX', 'padding') ?? 0) + (num('paddingRight', 'paddingX', 'padding') ?? 0)
  for (const child of node.children ?? []) pictures(child, inset + own, out)
  return out
}

/** The Box drawing the frame, as the register module draws it. */
const frameOf = (tree: RenderElement) => borders(tree)[0]

const fitsInside = (tree: RenderElement): boolean =>
  pictures(tree).every(({ node, inset }) => {
    const p = node.props ?? {}
    const columns = p.columns as number
    const isWhole = node.type !== 'Raster' || atob(p.cells as string).length === columns * (p.rows as number) * 12
    return isWhole && columns + inset <= VIEW.columns
  })

const captionKept = (tree: unknown): boolean =>
  walk(tree).some(n => n.type === 'Link' && n.props?.href === `file://${PNG}` && n.props?.label === 'ADR-516-item-card.png') && shows(tree as RenderElement, ' · 1020×300 · 90%')

describe('a picture another mod draws beneath the theme', () => {
  const sites: [string, string, Record<string, unknown>][] = [
    ['a SendUserFile result', 'ToolResult', SENT],
    ['a Read of an image', 'ToolUse', READ_IMAGE],
    ['folded Reads of images', 'ToolGroup', GROUP_IMAGE],
    ['a reply naming an image', 'AssistantMessage', REPLY_IMAGE],
  ]
  for (const renderer of ['halfblock', 'image'] as const) {
    for (const [name, component, props] of sites) {
      test(`${name} (${renderer}): the frame holds the row, the picture and its caption, the picture within the frame`, { plugins: [standIn()] }, async ($, on) => {
        hostOver(on, { mod: json(MIAMI_NIGHTS) })
        await $.session.start(INTERACTIVE)
        const ui = await $.ui.mount(at('terminal', component, props, `${renderer}-1`))
        const tree = await ui.drawn()
        expect(borders(tree).length).toBe(1)
        const frame = frameOf(tree)
        expect(pictures(frame).length).toBe(1)
        expect(captionKept(frame)).toBe(true)
        expect(fitsInside(tree)).toBe(true)
        await ui.unmount()
      })
    }
  }

  test("the theme's own Read row stands in the engine's place over the picture", { plugins: [standIn()] }, async ($, on) => {
    hostOver(on, { mod: json(MIAMI_NIGHTS) })
    await $.session.start(INTERACTIVE)
    const ui = await $.ui.mount(at('terminal', 'ToolUse', READ_IMAGE))
    const tree = await ui.drawn()
    expect(walk(tree).some(n => n.type === 'engine')).toBe(false)
    expect(shows(tree, 'ADR-516-item-card.png')).toBe(true)
    expect(walk(tree).some(n => n.type === 'Button' && n.props?.key === 'disclose')).toBe(true)
    await ui.unmount()
  })

  // The opened body is Claude Code's drawing beneath, picture and all; a second
  // picture would repeat the mod's key and the theme's tree would be refused.
  test("an opened Read of an image draws its picture once, in the theme's frame", { plugins: [standIn()] }, async ($, on) => {
    hostOver(on, { mod: json(MIAMI_NIGHTS) })
    await $.session.start(INTERACTIVE)
    const ui = await $.ui.mount(at('terminal', 'ToolUse', READ_IMAGE))
    const closed = await ui.drawn()
    await ui.press({ key: 'disclose' })
    const tree = await ui.drawn()
    expect(JSON.stringify(tree)).not.toBe(JSON.stringify(closed))
    expect(walk(tree).some(n => n.type === 'Button' && n.props?.key === 'disclose')).toBe(true)
    expect(borders(tree).length).toBe(1)
    expect(pictures(frameOf(tree)).length).toBe(1)
    expect(walk(tree).filter(n => n.props?.key === 'images-0').length).toBe(1)
    await ui.unmount()
  })

  test('a reply holding a `!` command and naming an image keeps its picture and caption, drawn once', { plugins: [standIn()] }, async ($, on) => {
    hostOver(on, { mod: json(MIAMI_NIGHTS) })
    await $.session.start(INTERACTIVE)
    const ui = await $.ui.mount(at('terminal', 'AssistantMessage', { text: `Saved the chart to ${PNG}.\n\n! xdg-open ${PNG}`, isFirstOfReply: true }))
    const tree = await ui.drawn()
    expect(borders(tree).length).toBeGreaterThan(0)
    expect(shows(tree, ` xdg-open ${PNG}`)).toBe(true)
    expect(pictures(tree).length).toBe(1)
    expect(captionKept(tree)).toBe(true)
    expect(fitsInside(tree)).toBe(true)
    await ui.unmount()
  })

  test('the head of a SendUserFile frame is its result as Claude Code draws it, the caption line under the picture', { plugins: [standIn()] }, async ($, on) => {
    hostOver(on, { mod: json(MIAMI_NIGHTS) })
    await $.session.start(INTERACTIVE)
    const ui = await $.ui.mount(at('terminal', 'ToolResult', SENT))
    const text = JSON.stringify(frameOf(await ui.drawn()))
    expect(text.indexOf('engine')).toBeGreaterThan(-1)
    expect(text.indexOf('engine')).toBeLessThan(text.indexOf('Raster'))
    expect(text.indexOf('Raster')).toBeLessThan(text.indexOf('1020×300'))
    await ui.unmount()
  })

  test('off the terminal, and under another Claude Code theme, nothing is framed', { plugins: [standIn()] }, async ($, on) => {
    hostOver(on, { mod: json(MIAMI_NIGHTS), selected: 'dark' })
    await $.session.start(INTERACTIVE)
    const terminal = await $.ui.mount(at('terminal', 'ToolResult', SENT))
    const tree = await terminal.drawn()
    expect(borders(tree).length).toBe(0)
    expect(captionKept(tree)).toBe(true)
    await terminal.unmount()
    const desktop = await $.ui.mount(at('desktop', 'ToolResult', SENT, 'r2'))
    expect(borders(await desktop.drawn()).length).toBe(0)
    await desktop.unmount()
  })
})

describe('a picture another mod draws above the theme', () => {
  // The mod wraps what the theme answered: the frame is whole, the picture under it.
  test('the frame holds the result alone, the picture and caption sit under it at the width the mod chose', { plugins: [standIn('prepend')] }, async ($, on) => {
    hostOver(on, { mod: json(MIAMI_NIGHTS) })
    await $.session.start(INTERACTIVE)
    const ui = await $.ui.mount(at('terminal', 'ToolResult', SENT))
    const tree = await ui.drawn()
    expect(borders(tree).length).toBe(1)
    expect(pictures(frameOf(tree)).length).toBe(0)
    expect(pictures(tree).length).toBe(1)
    expect(captionKept(tree)).toBe(true)
    await ui.unmount()
  })

  // The change the images mod needs to draw inside the frame from above: the
  // frame is the root Box the theme answers, its border and padding the inset.
  for (const [name, component, props] of [['a SendUserFile result', 'ToolResult', SENT], ['a Read of an image', 'ToolUse', READ_IMAGE], ['a reply naming an image', 'AssistantMessage', REPLY_IMAGE]] as const) {
    test(`${name}: a mod that adds its picture under the frame's child draws inside the frame, within its room`, { plugins: [standIn('prepend')] }, async ($, on) => {
      hostOver(on, { mod: json(MIAMI_NIGHTS) })
      await $.session.start(INTERACTIVE)
      const ui = await $.ui.mount(at('terminal', component, props, 'host-1'))
      const tree = await ui.drawn()
      expect(borders(tree).length).toBe(1)
      expect(pictures(frameOf(tree)).length).toBe(1)
      expect(captionKept(frameOf(tree))).toBe(true)
      expect(fitsInside(tree)).toBe(true)
      await ui.unmount()
    })
  }
})

describe('rows Claude Code draws empty', () => {
  for (const tool of ['ToolSearch', 'SendUserFile', 'TodoWrite']) {
    test(`a ${tool} call draws no frame`, async ($, on) => {
      host(on, { mod: json(MIAMI_NIGHTS) })
      await $.session.start(INTERACTIVE)
      const ui = await $.ui.mount(at('terminal', 'ToolUse', { ...CALL, tool_use_id: 't9', tool, input: {}, output: undefined }))
      const tree = await ui.drawn()
      expect(borders(tree).length).toBe(0)
      expect(shows(tree, 'engine')).toBe(true)
      await ui.unmount()
    })
  }

  test("a group of only such calls is Claude Code's own, unframed; one with a Read beside them is framed", async ($, on) => {
    host(on, { mod: json(MIAMI_NIGHTS) })
    await $.session.start(INTERACTIVE)
    const search = { tool: 'ToolSearch', input: { query: 'select:SendUserFile' }, isRunning: false, isErrored: false, isInterrupted: false }
    const empty = await $.ui.mount(at('terminal', 'ToolGroup', { calls: [search], isActive: false, isExpanded: false }))
    const tree = await empty.drawn()
    expect(borders(tree).length).toBe(0)
    expect(shows(tree, 'engine')).toBe(true)
    expect(shows(tree, 'Other')).toBe(false)
    await empty.unmount()
    const read = { tool: 'Read', input: { file_path: '/work/proj/a.ts' }, isRunning: false, isErrored: false, isInterrupted: false }
    const mixed = await $.ui.mount(at('terminal', 'ToolGroup', { calls: [search, read], isActive: false, isExpanded: false }, 'r2'))
    expect(borders(await mixed.drawn()).length).toBe(1)
    await mixed.unmount()
  })
})

describe('rows without a picture', () => {
  test('a Read of a text file is drawn by the theme alone, framed once', { plugins: [standIn()] }, async ($, on) => {
    hostOver(on, { mod: json(MIAMI_NIGHTS) })
    await $.session.start(INTERACTIVE)
    const ui = await $.ui.mount(at('terminal', 'ToolUse', READ_TEXT))
    const tree = await ui.drawn()
    expect(borders(tree).length).toBe(1)
    expect(shows(tree, 'engine')).toBe(false)
    expect(pictures(tree).length).toBe(0)
    await ui.unmount()
  })

  test('a SendUserFile sending no image, and any other result, is not framed', { plugins: [standIn()] }, async ($, on) => {
    hostOver(on, { mod: json(MIAMI_NIGHTS) })
    await $.session.start(INTERACTIVE)
    const report = { ...SENT, output: { attachments: [{ path: '/tmp/report.pdf', size: 1200, isImage: false }] } }
    const sent = await $.ui.mount(at('terminal', 'ToolResult', report))
    expect(borders(await sent.drawn()).length).toBe(0)
    await sent.unmount()
    const grep = await $.ui.mount(at('terminal', 'ToolResult', { tool_use_id: 't4', tool: 'Grep', output: { mode: 'files_with_matches', filenames: [], numFiles: 0 }, isErrored: false }, 'r2'))
    const tree = await grep.drawn()
    expect(borders(tree).length).toBe(0)
    expect(shows(tree, 'engine')).toBe(true)
    await grep.unmount()
  })
})
