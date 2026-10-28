import { describe, expect, test } from 'claude-code/testing'
import type { TestBody } from 'claude-code/testing'
import type { RenderElement } from 'claude-code'

import { sessionPasteId } from '../hooks/skins/hub.tsx'
import { PAD_X } from '../hooks/skins/prompt-box.tsx'
import { MAX_DEPTH, pasteIdOf, pieces, shownText } from '../hooks/skins/pastes.ts'
import { MIAMI_NIGHTS } from './fixtures.ts'
import { INTERACTIVE, borders, grounds, host, json, walk } from './host.tsx'
import type { World } from './host.tsx'

const VIEW = { columns: 100, rows: 30 }
const at = (props: Record<string, unknown>) => ({ plugin: 'ways-theme', surface: 'terminal', component: 'UserMessage', requestId: 'r1', viewport: VIEW, props: { origin: { kind: 'composer' }, isExpanded: false, ...props } }) as never

/** The session the host answers, and the id Claude Code gives its pastes. */
const SESSION = 'session-1'
const ID = '8409'

/** A paste as Claude Code stores it in the message, two newlines either side. */
const paste = (body: string, id = ID) => `\n\n<pasted_content id="${id}">\n${body}\n</pasted_content id="${id}">\n\n`

/** The text of each line of a gradient box, its padding off, the blank
 *  lines above and below left out. */
const boxText = (tree: RenderElement): string[] =>
  walk(tree)
    .filter(n => n.type === 'Text' && n.props?.backgroundColor === undefined && (n.children ?? []).some(c => typeof c === 'object'))
    .map(line => walk(line).flatMap(n => (n.children ?? []).filter((c): c is string => typeof c === 'string')).join('').slice(PAD_X).trimEnd())
    .slice(1, -1)

/** The gradient box's lines for `text`, the way the mod lays out a message. */
async function drawn($: Parameters<TestBody>[0], text: string): Promise<string[]> {
  const ui = await $.ui.mount(at({ text }))
  const tree = await ui.drawn()
  expect(borders(tree).length).toBe(0)
  expect(grounds(tree).length).toBeGreaterThan(0)
  await ui.unmount()
  return boxText(tree)
}

/** The user's report: a paste that carried a paste of another session's. */
const NESTED = paste(['<\\pasted_content id="6778">', '1. a) and log the error', '2. b) retry once', '<\\/pasted_content id="6778">'].join('\n')).replace(/\n$/, '')

describe('a paste in the person\'s message draws as Claude Code draws it', () => {
  test('the paste id is the first four hex digits of the SHA-256 of the session id', async () => {
    // Claude Code 2.1.312 stored id="9dc1" for session 4016d9de-ceb0-4004-a822-032f8901c7aa.
    expect(await pasteIdOf('4016d9de-ceb0-4004-a822-032f8901c7aa')).toBe('9dc1')
    expect(await pasteIdOf(SESSION)).toBe(ID)
  })

  test('a single paste loses its wrapper and sits on lines of its own', async ($, on) => {
    host(on, { mod: json(MIAMI_NIGHTS), sessionId: SESSION })
    await $.session.start(INTERACTIVE)
    const text = `Here: ${paste('alpha line\nbeta line')} and after.`
    expect(await drawn($, text)).toEqual(['Here:', 'alpha line', 'beta line', 'and after.'])
  })

  test('a nested paste: the outer wrapper goes, the inner one is unescaped and drawn whole', async ($, on) => {
    host(on, { mod: json(MIAMI_NIGHTS), sessionId: SESSION })
    await $.session.start(INTERACTIVE)
    expect(await drawn($, NESTED)).toEqual(['1. a) and log the error', '2. b) retry once'])
  })

  test('two pastes in one message each draw on lines of their own', async ($, on) => {
    host(on, { mod: json(MIAMI_NIGHTS), sessionId: SESSION })
    await $.session.start(INTERACTIVE)
    const text = `Compare ${paste('one\ntwo')} with ${paste('three\nfour')}`.trimEnd()
    expect(await drawn($, text)).toEqual(['Compare', 'one', 'two', 'with', 'three', 'four'])
  })

  test('typed text around a one-line paste keeps it on the line', async ($, on) => {
    host(on, { mod: json(MIAMI_NIGHTS), sessionId: SESSION })
    await $.session.start(INTERACTIVE)
    expect(await drawn($, `open ${paste('src/main.ts')} now`)).toEqual(['open src/main.ts now'])
    expect(await drawn($, `open (${paste('src/main.ts')}) now`)).toEqual(['open (src/main.ts) now'])
  })

  test('a paste spanning lines breaks the typed line it sits in', async ($, on) => {
    host(on, { mod: json(MIAMI_NIGHTS), sessionId: SESSION })
    await $.session.start(INTERACTIVE)
    expect(await drawn($, `fix ${paste('a\nb')}please`)).toEqual(['fix', 'a', 'b', 'please'])
  })

  test('in ctrl+o the outline shows the paste unwrapped', async ($, on) => {
    host(on, { mod: json(MIAMI_NIGHTS), sessionId: SESSION })
    await $.session.start(INTERACTIVE)
    const ui = await $.ui.mount(at({ text: `Here: ${paste('alpha\nbeta')}`, isExpanded: true }))
    const tree = await ui.drawn()
    expect(borders(tree).length).toBe(1)
    const strings = walk(tree).flatMap(n => (n.children ?? []).filter((c): c is string => typeof c === 'string'))
    expect(strings).toContain('Here:\nalpha\nbeta')
    expect(strings.join('')).not.toContain('pasted_content')
    await ui.unmount()
  })
})

describe('text that is not a whole paste of the session draws as it came', () => {
  const asTyped: [string, string][] = [
    ['a message with no paste', 'Keep Miami Nights, and put my messages on its gradient.'],
    ['an opening tag never closed', `look ${paste('half a paste').replace(/<\/pasted_content[^>]*>\n*$/, '')}`],
    ['a paste closed under another id', `look \n\n<pasted_content id="${ID}">\nbody\n</pasted_content id="beef">\n`],
    ['tags the person typed, with an id not the session\'s', 'see <pasted_content id="abcd">\nmine\n</pasted_content id="abcd"> here'],
    ['a bare tag the person typed', 'what does <pasted_content> mean?'],
  ]
  for (const [name, text] of asTyped) {
    test(`${name}: every character drawn as today`, async ($, on) => {
      host(on, { mod: json(MIAMI_NIGHTS), sessionId: SESSION })
      await $.session.start(INTERACTIVE)
      expect(shownText(text, ID)).toBe(text)
      expect(await drawn($, text)).toEqual(text.split('\n').map(line => line.trimEnd()))
    })
  }

  test('a long paste past the box\'s bound is left to Claude Code', async ($, on) => {
    host(on, { mod: json(MIAMI_NIGHTS), sessionId: SESSION })
    await $.session.start(INTERACTIVE)
    const ui = await $.ui.mount(at({ text: `see ${paste('line of a long log\n'.repeat(400))}` }))
    expect(await ui.drawn()).toMatchObject({ type: 'Text' })
    await ui.unmount()
  })

  test('a typed block of another id beside a drawn paste is kept exactly', async ($, on) => {
    host(on, { mod: json(MIAMI_NIGHTS), sessionId: SESSION })
    await $.session.start(INTERACTIVE)
    const typed = 'mine <pasted_content id="abcd">\nx\n</pasted_content id="abcd">'
    expect(await drawn($, `${typed} and ${paste('p1\np2')} end`)).toEqual(['mine <pasted_content id="abcd">', 'x', '</pasted_content id="abcd"> and', 'p1', 'p2', 'end'])
  })
})

describe('without the session paste id, a message with a paste is Claude Code\'s', () => {
  test('$.session.id() rejecting: the paste goes to Claude Code, a message with none keeps the box', async ($, on) => {
    host(on, { mod: json(MIAMI_NIGHTS), sessionId: null })
    await $.session.start(INTERACTIVE)
    const ui = await $.ui.mount(at({ text: `Here: ${paste('a\nb')}` }))
    expect(await ui.drawn()).toMatchObject({ type: 'Text' })
    await ui.unmount()
    expect(await drawn($, 'no paste here')).toEqual(['no paste here'])
  })

  // The hooks run in an environment of their own, which a test cannot reach to
  // make `crypto.subtle.digest` reject; the hub's lookup takes the digest as an
  // argument instead. A null from it is the rejection path the test above drives.
  test('the digest rejecting: the lookup answers null and keeps nothing', async () => {
    const $ = { session: { id: async () => 'session-3' } } as never
    expect(await sessionPasteId($, () => Promise.reject(new Error('no digest')))).toBe(null)
    expect(await sessionPasteId($)).toBe('eb27')
  })

  test('a new session id gets its own paste id', async ($, on) => {
    const world: World = { mod: json(MIAMI_NIGHTS), sessionId: SESSION }
    host(on, world)
    await $.session.start(INTERACTIVE)
    expect(await drawn($, `x ${paste('a\nb')}`)).toEqual(['x', 'a', 'b'])
    world.sessionId = 'session-2'
    const old = `x ${paste('a\nb')}`
    expect(await drawn($, old)).toEqual(old.split('\n').map(line => line.trimEnd()))
    expect(await drawn($, `x ${paste('a\nb', '5d90')}`)).toEqual(['x', 'a', 'b'])
  })
})

/** `body` wrapped as a paste the copied text carried: its tags escaped, as
 *  Claude Code escapes them at every depth. */
const carried = (id: string, body: string) => `<\\pasted_content id="${id}">\n${body}\n<\\/pasted_content id="${id}">`
/** `core` inside `levels` carried pastes, a000 the innermost. */
const nest = (levels: number, core = 'core') => Array.from({ length: levels }, (_, i) => `a${String(i).padStart(3, '0')}`).reduce((body, id) => carried(id, body), core)

describe('pastes inside a paste', () => {
  test('two deep: each inner paste loses only its tag lines', () => {
    const body = ['a', carried('1111', ['b', carried('2222', 'c'), 'd'].join('\n')), 'e'].join('\n')
    expect(shownText(paste(body), ID)).toBe('a\nb\nc\nd\ne')
  })

  test('three deep', () => {
    const body = ['a', carried('1111', ['b', carried('2222', ['c', carried('3333', 'X'), 'd'].join('\n')), 'e'].join('\n')), 'f'].join('\n')
    expect(shownText(paste(body), ID)).toBe('a\nb\nc\nX\nd\ne\nf')
  })

  test(`unwrapped to ${MAX_DEPTH} levels; deeper tags stay, unescaped`, () => {
    expect(shownText(paste(nest(MAX_DEPTH - 1)), ID)).toBe('core')
    expect(shownText(paste(nest(MAX_DEPTH)), ID)).toBe('<pasted_content id="a000">\ncore\n</pasted_content id="a000">')
  })

  test('a fenced example of the tag format keeps its fence, its line breaks and its spaces', async ($, on) => {
    host(on, { mod: json(MIAMI_NIGHTS), sessionId: SESSION })
    await $.session.start(INTERACTIVE)
    const body = ['```', carried('1234', '  example'), '```'].join('\n')
    expect(await drawn($, `look ${paste(body)}`)).toEqual(['look', '```', '  example', '```'])
  })

  test('blank lines around an inner paste stay', () => {
    expect(shownText(paste(`one\n\n${carried('1234', 'two')}\n\nthree`), ID)).toBe('one\n\ntwo\n\nthree')
  })
})

describe('the split', () => {
  test('a paste takes up to two newlines either side, and its raw keeps them', () => {
    const text = `a\n\n\n<pasted_content id="${ID}">\nx\n</pasted_content id="${ID}">\n\n\nb`
    expect(pieces(text)).toEqual([
      { kind: 'text', text: 'a\n' },
      { kind: 'paste', id: ID, body: 'x', raw: `\n\n<pasted_content id="${ID}">\nx\n</pasted_content id="${ID}">\n\n` },
      { kind: 'text', text: '\nb' },
    ])
  })

  test('a closing tag inside a paste is the paste\'s text: it arrives escaped and closes nothing', () => {
    const body = 'before\n<\\/pasted_content id="' + ID + '">\nafter'
    expect(shownText(`x ${paste(body)}`, ID)).toBe(`x\nbefore\n</pasted_content id="${ID}">\nafter`)
  })

  test('an empty paste draws as nothing, with the typed text kept', () => {
    expect(shownText(`a \n\n<pasted_content id="${ID}">\n</pasted_content id="${ID}">\n\n b`, ID)).toBe('a  b')
  })
})
