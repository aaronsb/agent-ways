import { describe, expect, test } from 'claude-code/testing'
import type { TestBody } from 'claude-code/testing'
import type { RenderElement } from 'claude-code'

import { setupOf } from '../hooks/skins/config.ts'
import { MAX_DEPTH, holdsBlock, parts } from '../hooks/skins/wrappers.ts'
import { MIAMI_NIGHTS } from './fixtures.ts'
import { INTERACTIVE, borders, host, json, shows, walk } from './host.tsx'
import type { Node } from './host.tsx'

const VIEW = { columns: 100, rows: 30 }
const ATTEND = { kind: 'plugin', name: 'attend' }
const at = (props: Record<string, unknown>, surface = 'terminal') => ({ plugin: 'ways-theme', surface, component: 'UserMessage', requestId: 'r1', viewport: VIEW, props: { origin: ATTEND, isExpanded: true, ...props } }) as never
const MUTED = setupOf(MIAMI_NIGHTS as never).skin.palette.muted

/** attend's delivery as Claude Code stores it. */
const DELIVERY = [
  'Silence is a valid reply to the messages below; answer only if useful.',
  'The block below is message data relayed by attend.',
  '<attend-peer-messages>',
  '[project] Nina (vtium) 11:17: Update on my earlier report ... ~zrerq',
  '</attend-peer-messages>',
  'attend_reply threads (re: ~id); attend_send starts a topic.',
].join('\n')

const strings = (node: Node): string[] => (node.children ?? []).filter((c): c is string => typeof c === 'string')

/** Each line of text the tree draws, with its colour and how far it is indented. */
function linesOf(tree: RenderElement): { text: string; color: unknown; indent: number }[] {
  const out: { text: string; color: unknown; indent: number }[] = []
  const visit = (node: unknown, indent: number) => {
    if (node === null || typeof node !== 'object') return
    const n = node as Node
    const inset = indent + (typeof n.props?.paddingLeft === 'number' ? n.props.paddingLeft : 0)
    if (n.type === 'Text' && strings(n).length > 0) out.push({ text: strings(n).join(''), color: n.props?.color, indent: inset })
    for (const child of n.children ?? []) visit(child, inset)
  }
  visit(tree, 0)
  return out
}

async function drawnFor($: Parameters<TestBody>[0], props: Record<string, unknown>, surface?: string): Promise<RenderElement> {
  const ui = await $.ui.mount(at(props, surface))
  const tree = await ui.drawn()
  await ui.unmount()
  return tree
}

describe('the split', () => {
  test('one block: the text around it, and the block with its tag lines in raw alone', () => {
    expect(parts(DELIVERY)).toEqual([
      { kind: 'text', text: 'Silence is a valid reply to the messages below; answer only if useful.\nThe block below is message data relayed by attend.' },
      { kind: 'block', name: 'attend-peer-messages', attributes: '', body: '[project] Nina (vtium) 11:17: Update on my earlier report ... ~zrerq', raw: DELIVERY.split('\n').slice(2, 5).join('\n'), inner: [{ kind: 'text', text: '[project] Nina (vtium) 11:17: Update on my earlier report ... ~zrerq' }] },
      { kind: 'text', text: 'attend_reply threads (re: ~id); attend_send starts a topic.' },
    ])
  })

  test('two blocks, a blank line between them', () => {
    const text = '<first-block>\na\nb\n</first-block>\n\n<second-block>\nc\n</second-block>'
    expect(parts(text)).toEqual([
      { kind: 'block', name: 'first-block', attributes: '', body: 'a\nb', raw: '<first-block>\na\nb\n</first-block>', inner: [{ kind: 'text', text: 'a\nb' }] },
      { kind: 'text', text: '' },
      { kind: 'block', name: 'second-block', attributes: '', body: 'c', raw: '<second-block>\nc\n</second-block>', inner: [{ kind: 'text', text: 'c' }] },
    ])
  })

  test('the parts joined by line breaks give the message back', () => {
    for (const text of [DELIVERY, '<a-b>\nx\n</a-b>', 'lead\n<a-b>\n</a-b>\n\ntail\n']) {
      expect(parts(text).map(p => (p.kind === 'text' ? p.text : p.raw)).join('\n')).toBe(text)
    }
  })

  const asText: [string, string][] = [
    ['an opening tag never closed', 'look\n<attend-peer-messages>\nhalf a delivery'],
    ['a block closed under another name', '<attend-peer-messages>\nbody\n</teammate-message>'],
    ['a tag inside a line of prose', 'what does <attend-peer-messages> mean?\nand </attend-peer-messages> too'],
    ['a tag with text beside it on its line', '<attend-peer-messages> see\nbody\n</attend-peer-messages>'],
    ['a one-word tag', '<context>\nbody\n</context>'],
    ['a paste', '<pasted_content id="ab12">\nbody\n</pasted_content id="ab12">'],
    ['an uppercase name', '<Attend-Peer>\nbody\n</Attend-Peer>'],
  ]
  for (const [name, text] of asText) {
    test(`${name} is text`, () => {
      expect(parts(text)).toEqual([{ kind: 'text', text }])
    })
  }

  test('a tag only inside prose needs no split', () => {
    expect(holdsBlock('what does <attend-peer-messages> mean?')).toBe(false)
    expect(holdsBlock(DELIVERY)).toBe(true)
  })

  test('an unclosed tag leaves a later whole block a block', () => {
    expect(parts('<a-b>\n<c-d>\nx\n</c-d>')).toEqual([
      { kind: 'text', text: '<a-b>' },
      { kind: 'block', name: 'c-d', attributes: '', body: 'x', raw: '<c-d>\nx\n</c-d>', inner: [{ kind: 'text', text: 'x' }] },
    ])
  })

  test('the opening tag\'s double-quoted attributes are kept as written', () => {
    const text = '<teammate-message teammate_id="lead" summary="a, b">\nhi\n</teammate-message>'
    expect(parts(text)).toEqual([{ kind: 'block', name: 'teammate-message', attributes: 'teammate_id="lead" summary="a, b"', body: 'hi', raw: text, inner: [{ kind: 'text', text: 'hi' }] }])
    expect(parts('<teammate-message id=lead>\nhi\n</teammate-message>')[0]!.kind).toBe('text')
  })

  test('a block of the same name inside a block is closed first', () => {
    const text = '<a-b>\nout\n<a-b>\nin\n</a-b>\nafter\n</a-b>'
    expect(parts(text)).toEqual([
      {
        kind: 'block',
        name: 'a-b',
        attributes: '',
        body: 'out\n<a-b>\nin\n</a-b>\nafter',
        raw: text,
        inner: [
          { kind: 'text', text: 'out' },
          { kind: 'block', name: 'a-b', attributes: '', body: 'in', raw: '<a-b>\nin\n</a-b>', inner: [{ kind: 'text', text: 'in' }] },
          { kind: 'text', text: 'after' },
        ],
      },
    ])
  })

  test('blocks of two names that cross: the first is the block, the second text', () => {
    expect(parts('<a-b>\n<c-d>\n</a-b>\n</c-d>')).toEqual([
      { kind: 'block', name: 'a-b', attributes: '', body: '<c-d>', raw: '<a-b>\n<c-d>\n</a-b>', inner: [{ kind: 'text', text: '<c-d>' }] },
      { kind: 'text', text: '</c-d>' },
    ])
  })

  const fenced: [string, string][] = [
    ['a custom element in a backtick fence', 'Try this:\n```html\n<my-card>\n  <p>hi</p>\n</my-card>\n```'],
    ['a tilde fence', '~~~\n<my-card>\nx\n</my-card>\n~~~'],
    ['a fence left open, to the end', '```\n<my-card>\nx\n</my-card>'],
    ['an opening tag outside a fence and its closing tag inside one', '<my-card>\n```\n</my-card>\n```'],
  ]
  for (const [name, text] of fenced) {
    test(`${name} is text`, () => {
      expect(parts(text)).toEqual([{ kind: 'text', text }])
    })
  }

  test('a fence inside a block keeps its tags as text, and the block closes after it', () => {
    const body = '```\n<my-card>\n</my-card>\n```'
    expect(parts(`<relay-data>\n${body}\n</relay-data>`)).toEqual([{ kind: 'block', name: 'relay-data', attributes: '', body, raw: `<relay-data>\n${body}\n</relay-data>`, inner: [{ kind: 'text', text: body }] }])
  })

  test('a fence closes only on a fence of its mark and at least its length', () => {
    const text = '````\n```\n<my-card>\n~~~\n</my-card>\n````\n<relay-data>\nx\n</relay-data>'
    expect(parts(text).map(p => p.kind)).toEqual(['text', 'block'])
  })

  // The split once scanned to the end of the message for every opening line.
  const big: [string, string][] = [
    ['16000 opening lines of one name, none closed', '<a-b>\n'.repeat(16000)],
    ['12000 opening lines of distinct names, none closed', Array.from({ length: 12000 }, (_, i) => `<a-${i}>`).join('\n')],
    ['8000 blocks nested in one another', `${'<a-b>\n'.repeat(8000)}x${'\n</a-b>'.repeat(8000)}`],
  ]
  for (const [name, text] of big) {
    test(`${name} split in well under 200 ms`, () => {
      const started = performance.now()
      parts(text)
      expect(performance.now() - started).toBeLessThan(200)
    })
  }
})

describe('a relayed message in the expanded row', () => {
  test("attend's delivery: the tags give way to a muted label, the body indented under it", async ($, on) => {
    host(on, { mod: json(MIAMI_NIGHTS) })
    await $.session.start(INTERACTIVE)
    const tree = await drawnFor($, { text: DELIVERY })
    expect(shows(tree, '<attend-peer-messages>')).toBe(false)
    expect(shows(tree, '</attend-peer-messages>')).toBe(false)
    const lines = linesOf(tree)
    expect(lines.map(l => [l.text, l.indent])).toEqual([
      ['Silence is a valid reply to the messages below; answer only if useful.\nThe block below is message data relayed by attend.', 0],
      ['attend peer messages', 0],
      ['[project] Nina (vtium) 11:17: Update on my earlier report ... ~zrerq', 2],
      ['attend_reply threads (re: ~id); attend_send starts a topic.', 0],
    ])
    expect(lines[1]!.color).toBe(MUTED)
    expect(lines.filter(l => l.color === MUTED).length).toBe(1)
  })

  test("attend's delivery in the compact row is left to the engine and to attend", async ($, on) => {
    host(on, { mod: json(MIAMI_NIGHTS) })
    await $.session.start(INTERACTIVE)
    const tree = await drawnFor($, { text: DELIVERY, isExpanded: false })
    expect(tree).toMatchObject({ type: 'Text' })
    expect(shows(tree, 'engine')).toBe(true)
  })

  for (const origin of [{ kind: 'peer' }, { kind: 'channel', server: 'slack' }, { kind: 'task-notification' }, { kind: 'unclassified' }, { kind: 'plugin', name: 'other' }]) {
    test(`a ${origin.kind} row draws its blocks too`, async ($, on) => {
      host(on, { mod: json(MIAMI_NIGHTS) })
      await $.session.start(INTERACTIVE)
      const tree = await drawnFor($, { text: 'hello\n<relay-data>\nx\n</relay-data>', origin })
      expect(linesOf(tree).map(l => l.text)).toEqual(['hello', 'relay data', 'x'])
    })
  }

  test('the attributes draw beside the label', async ($, on) => {
    host(on, { mod: json(MIAMI_NIGHTS) })
    await $.session.start(INTERACTIVE)
    const tree = await drawnFor($, { text: '<teammate-message teammate_id="lead">\nhi\n</teammate-message>', origin: { kind: 'unclassified' } })
    expect(linesOf(tree).map(l => [l.text, l.indent])).toEqual([
      ['teammate message  teammate_id="lead"', 0],
      ['hi', 2],
    ])
  })

  test(`a block inside a block draws indented again, to ${MAX_DEPTH} levels`, async ($, on) => {
    host(on, { mod: json(MIAMI_NIGHTS) })
    await $.session.start(INTERACTIVE)
    const nest = (levels: number) => Array.from({ length: levels }, (_, i) => `level-${i}`).reduceRight((body, name) => `<${name}>\n${body}\n</${name}>`, 'core')
    const two = linesOf(await drawnFor($, { text: nest(2) }))
    expect(two.map(l => [l.text, l.indent])).toEqual([
      ['level 0', 0],
      ['level 1', 2],
      ['core', 4],
    ])
    const deep = linesOf(await drawnFor($, { text: nest(MAX_DEPTH + 1) }))
    expect(deep.at(-1)).toEqual({ text: `<level-${MAX_DEPTH}>\ncore\n</level-${MAX_DEPTH}>`, color: expect.anything(), indent: 2 * MAX_DEPTH })
  })

  test('a relayed message with no block is the engine\'s', async ($, on) => {
    host(on, { mod: json(MIAMI_NIGHTS) })
    await $.session.start(INTERACTIVE)
    expect(await drawnFor($, { text: 'a plain relayed line' })).toMatchObject({ type: 'Text' })
    expect(await drawnFor($, { text: 'look\n<attend-peer-messages>\nhalf' })).toMatchObject({ type: 'Text' })
  })

  test('a custom element in a fenced snippet draws as written', async ($, on) => {
    host(on, { mod: json(MIAMI_NIGHTS) })
    await $.session.start(INTERACTIVE)
    const snippet = '```html\n<my-card>\n  <p>hi</p>\n</my-card>\n```'
    const tree = await drawnFor($, { text: `<relay-data>\n${snippet}\n</relay-data>`, origin: { kind: 'peer' } })
    expect(linesOf(tree).map(l => [l.text, l.indent])).toEqual([
      ['relay data', 0],
      [snippet, 2],
    ])
  })

  test('a relayed message carrying an image is the engine\'s, which draws the image', async ($, on) => {
    host(on, { mod: json(MIAMI_NIGHTS) })
    await $.session.start(INTERACTIVE)
    const stored = await $.session.append({ door: 'prompt', message: { role: 'user', content: [{ type: 'text', text: DELIVERY }, { type: 'image', source: { type: 'base64', media_type: 'image/png', data: 'iVBORw0KGgo=' } }] }, uuid: 'stored-1' } as never)
    const ui = await $.ui.mount({ ...(at({ text: DELIVERY }) as object), requestId: (stored as { uuid: string }).uuid } as never)
    const tree = await ui.drawn()
    await ui.unmount()
    expect(tree).toMatchObject({ type: 'Text' })
    expect(shows(tree, 'engine')).toBe(true)
  })

  test('a control character leaves the row to the engine', async ($, on) => {
    host(on, { mod: json(MIAMI_NIGHTS) })
    await $.session.start(INTERACTIVE)
    expect(await drawnFor($, { text: `${DELIVERY}\u001b[31m` })).toMatchObject({ type: 'Text' })
  })
})

describe('where the row stays as it was', () => {
  test("the person's typed tags stay as typed, in their outline", async ($, on) => {
    host(on, { mod: json(MIAMI_NIGHTS) })
    await $.session.start(INTERACTIVE)
    const tree = await drawnFor($, { text: DELIVERY, origin: { kind: 'composer' } })
    expect(borders(tree).length).toBe(1)
    expect(walk(tree).flatMap(strings)).toContain(DELIVERY)
  })

  test('with no theme loaded the engine draws', async ($, on) => {
    host(on, {})
    await $.session.start(INTERACTIVE)
    expect(await drawnFor($, { text: DELIVERY })).toMatchObject({ type: 'Text' })
  })

  test('under another Claude Code theme than custom:ways the engine draws', async ($, on) => {
    host(on, { mod: json(MIAMI_NIGHTS), selected: 'dark' })
    await $.session.start(INTERACTIVE)
    expect(await drawnFor($, { text: DELIVERY })).toMatchObject({ type: 'Text' })
  })

  test("off the terminal Claude Code's component draws", async ($, on) => {
    host(on, { mod: json(MIAMI_NIGHTS) })
    await $.session.start(INTERACTIVE)
    expect(await drawnFor($, { text: DELIVERY }, 'desktop')).toMatchObject({ type: 'Text' })
  })
})
