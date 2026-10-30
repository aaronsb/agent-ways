import { describe, expect, test } from 'claude-code/testing'
import type { RenderElement } from 'claude-code'

import { register } from '../hooks/register.tsx'
import { gradientAt, mix, paletteOf, parseExport, setupOf, toOklab } from '../hooks/skins/config.ts'
import { PICKUP_MS, WAYS_TIMEOUT_MS } from '../hooks/skins/hub.tsx'
import { MIN_WIDTH, PAD_X, VERTICAL_FROM_LINES, axisFor, boxLines, canMeasure, cellsOf, gradientRuns, graphemes, wrapCells } from '../hooks/skins/prompt-box.tsx'
import { MIAMI_NIGHTS, TAMPA, TERMINAL_THEME } from './fixtures.ts'
import { registrationsOf } from './passed-over.ts'
import { INTERACTIVE, borders, grounds, host, json, ok, shows, walk } from './host.tsx'
import type { Answer, World } from './host.tsx'

const VIEW = { columns: 100, rows: 30 }
const at = (surface: string, component: string, props: Record<string, unknown>, requestId = 'r1') => ({ plugin: 'ways-theme', surface, component, requestId, viewport: VIEW, props }) as never

const REPLY = { text: 'A reply with **bold** text.', isFirstOfReply: true }
const READ = { tool_use_id: 't1', tool: 'Read', input: { file_path: '/work/proj/a.ts' }, isRunning: false, isErrored: false, isInterrupted: false, isExpanded: false, output: undefined }
const BASH_OUT = { stdout: 'one\ntwo', stderr: '', interrupted: false, isImage: false }
const BASH = { tool_use_id: 't2', tool: 'Bash', input: { command: 'make check' }, isRunning: false, isErrored: false, isInterrupted: false, isExpanded: false, output: BASH_OUT }
const GROUP = { calls: [{ tool: 'Read', input: { file_path: '/work/proj/a.ts' }, isRunning: false, isErrored: false, isInterrupted: false }], isActive: false, isExpanded: false }
const PERSON = { text: 'Keep Miami Nights, and put my messages on its gradient.', origin: { kind: 'composer' }, isExpanded: false }

describe('the export contract', () => {
  test('each fixture parses whole', () => {
    for (const fixture of [MIAMI_NIGHTS, TAMPA, TERMINAL_THEME]) expect(parseExport(JSON.stringify(fixture))).toEqual(fixture)
  })

  const broken: [string, unknown][] = [
    ['not JSON', '{"theme": '],
    ['an array', []],
    ['a key missing', { ...MIAMI_NIGHTS, render: undefined }],
    ['an extra key', { ...MIAMI_NIGHTS, extra: 1 }],
    ['a theme that is not a name', { ...MIAMI_NIGHTS, theme: '../evil' }],
    ['a kind that is neither', { ...MIAMI_NIGHTS, kind: 'dim' }],
    ['a role short of #rrggbb', { ...MIAMI_NIGHTS, roles: { ...MIAMI_NIGHTS.roles, body: '#fff' } }],
    ['a role missing', { ...MIAMI_NIGHTS, roles: { ...MIAMI_NIGHTS.roles, subtle: undefined } }],
    ['a role extra', { ...MIAMI_NIGHTS, roles: { ...MIAMI_NIGHTS.roles, glow: '#ffffff' } }],
    ['null roles on a theme', { ...MIAMI_NIGHTS, roles: null }],
    ['roles on terminal', { ...TERMINAL_THEME, roles: MIAMI_NIGHTS.roles }],
    ['a gradient axis that is neither', { ...MIAMI_NIGHTS, gradient: { axis: 'radial', stops: ['#000000', '#ffffff'] } }],
    ['one gradient stop', { ...MIAMI_NIGHTS, gradient: { axis: 'vertical', stops: ['#000000'] } }],
    ['a gradient stop by name', { ...MIAMI_NIGHTS, gradient: { axis: 'vertical', stops: ['#000000', 'pink'] } }],
    ['a word list that is not a list', { ...MIAMI_NIGHTS, words: { spinner: 'Cruising', done: [] } }],
    ['a word with a control character', { ...MIAMI_NIGHTS, words: { spinner: ['Cru\u001bising'], done: ['Docked'] } }],
    ['a word with a line separator', { ...MIAMI_NIGHTS, words: { spinner: ['Cru\u2028ising'], done: ['Docked'] } }],
    ['a word with a bidi override', { ...MIAMI_NIGHTS, words: { spinner: ['\u202eCruising'], done: ['Docked'] } }],
    ['a word with a bidi isolate', { ...MIAMI_NIGHTS, words: { spinner: ['Cruising'], done: ['\u2067Docked'] } }],
    ['a word that is only spaces', { ...MIAMI_NIGHTS, words: { spinner: ['   '], done: ['Docked'] } }],
    ['render.copy that is null', { ...MIAMI_NIGHTS, render: { ...MIAMI_NIGHTS.render, copy: null } }],
    ['tables that are neither', { ...MIAMI_NIGHTS, render: { ...MIAMI_NIGHTS.render, tables: 'cards' } }],
    ['icons that are neither', { ...MIAMI_NIGHTS, render: { ...MIAMI_NIGHTS.render, icons: 'emoji' } }],
    ['a prompt box that is neither', { ...MIAMI_NIGHTS, render: { ...MIAMI_NIGHTS.render, prompt_box: 'glow' } }],
    ['a switch that is a string', { ...MIAMI_NIGHTS, render: { ...MIAMI_NIGHTS.render, rail: 'yes' } }],
    ['a render key missing', { ...MIAMI_NIGHTS, render: { ...MIAMI_NIGHTS.render, frames: undefined } }],
  ]
  for (const [name, value] of broken) {
    test(`${name} is refused`, () => {
      expect(typeof parseExport(typeof value === 'string' ? value : JSON.stringify(value))).toBe('string')
    })
  }

  test('a refused export draws nothing of its own: the engine draws, inside the frames the module had already', async ($, on) => {
    host(on, { mod: json({ ...MIAMI_NIGHTS, kind: 'dim' }) })
    await $.session.start(INTERACTIVE)
    for (const [component, props] of [['UserMessage', PERSON], ['ToolGroup', GROUP], ['Spinner', { word: 'Working', message: null, suffix: '', mode: 'requesting' }]] as const) {
      const ui = await $.ui.mount(at('terminal', component, props))
      expect(shows(await ui.drawn(), 'engine')).toBe(true)
      await ui.unmount()
    }
  })

  test('render.copy may be left out, and reads as true', () => {
    const { copy, ...rest } = MIAMI_NIGHTS.render
    expect(copy).toBe(true)
    const parsed = parseExport(JSON.stringify({ ...MIAMI_NIGHTS, render: rest }))
    expect(typeof parsed !== 'string' && parsed.render.copy).toBe(true)
    expect(typeof parseExport(JSON.stringify({ ...MIAMI_NIGHTS, render: { ...MIAMI_NIGHTS.render, copy: 'no' } }))).toBe('string')
  })
})

describe('a ways that fails is a no-op', () => {
  const cases: [string, World][] = [
    ['ways is missing', {}],
    ['the export exits non-zero', { mod: ok('', 2) }],
    ['the export runs past its bound', { mod: new Error(`timed out after ${WAYS_TIMEOUT_MS} ms`) }],
    ['the export is not JSON', { mod: ok('error: no such verb') }],
    ['the export has the wrong shape', { mod: json({ theme: 'miami-nights' }) }],
  ]
  for (const [name, world] of cases) {
    test(name, async ($, on) => {
      const h = host(on, world)
      await $.session.start(INTERACTIVE)
      expect(h.modRuns).toEqual([WAYS_TIMEOUT_MS])
      for (const [component, props] of [['UserMessage', PERSON], ['ToolUse', READ], ['ToolGroup', GROUP], ['TurnDuration', { word: 'Baked', durationMs: 4000, isExpanded: false }]] as const) {
        const ui = await $.ui.mount(at('terminal', component, props))
        expect(walk(await ui.drawn()).some(n => n.type === 'Text' && JSON.stringify(n).includes('engine'))).toBe(true)
        await ui.unmount()
      }
      expect(h.toasts).toEqual([])
      expect(h.logs.length).toBe(1)
    })
  }

  test('a failure repeated on later reads logs once', async ($, on) => {
    const h = host(on, { mod: ok('', 2) })
    await $.session.start(INTERACTIVE)
    await h.clock.advance(PICKUP_MS * 3)
    expect(h.modRuns.length).toBe(4)
    expect(h.logs.length).toBe(1)
  })

  test('a headless session reads nothing', async ($, on) => {
    const h = host(on, { mod: json(MIAMI_NIGHTS) })
    await $.session.start({ cwd: '/work/proj', surface: null, isInteractive: false })
    expect(h.modRuns).toEqual([])
  })
})

describe('roles map to the skin', () => {
  test('each slot takes its role', () => {
    const r = MIAMI_NIGHTS.roles
    const p = paletteOf(r)
    expect([p.user, p.fg, p.muted, p.ok, p.err, p.warn]).toEqual([r.accent, r.body, r.muted, r.ok, r.err, r.warn])
    expect([p.read, p.write, p.run, p.search, p.web, p.mcp, p.other]).toEqual([r.info, r.hot, r.warn, r.alt, r.accent_dim, r.muted, r.rule])
  })

  test('the table bands step from bg toward subtle, the zebra less far', () => {
    const r = MIAMI_NIGHTS.roles
    const p = paletteOf(r)
    const L = (hex: string) => toOklab(hex)[0]
    expect(L(r.bg)).toBeLessThan(L(p.zebra))
    expect(L(p.zebra)).toBeLessThan(L(p.surface))
    expect(L(p.surface)).toBeLessThan(L(r.subtle))
  })

  test('a light theme keeps its dark body; terminal takes the terminal palette', () => {
    expect(setupOf(TAMPA as never).skin.palette.fg).toBe(TAMPA.roles.body)
    const terminal = setupOf(TERMINAL_THEME as never)
    expect(terminal.skin.palette.fg).toBe('text')
    expect(terminal.themed).toBe(false)
    expect(terminal.ground).toBeNull()
  })

  test('the words are the theme\'s, or Claude Code\'s when it has none', () => {
    expect(setupOf(MIAMI_NIGHTS as never).skin.spinner).toEqual(MIAMI_NIGHTS.words.spinner)
    expect(setupOf(MIAMI_NIGHTS as never).prefs.words).toBe('skin')
    expect(setupOf(TAMPA as never).prefs.words).toBe('claude')
  })

  test('a drawn row carries the role colours', async ($, on) => {
    host(on, { mod: json(MIAMI_NIGHTS) })
    await $.session.start(INTERACTIVE)
    const ui = await $.ui.mount(at('terminal', 'ToolGroup', GROUP))
    const drawn = JSON.stringify(await ui.drawn())
    expect(drawn).toContain(MIAMI_NIGHTS.roles.info)
    expect(drawn).toContain(MIAMI_NIGHTS.roles.ok)
    await ui.unmount()
  })

  test('an empty word list keeps Claude Code\'s words for that slot, and null for both', async ($, on) => {
    const world: World = { mod: json({ ...MIAMI_NIGHTS, words: { spinner: MIAMI_NIGHTS.words.spinner, done: [] } }) }
    const h = host(on, world)
    await $.session.start(INTERACTIVE)
    const footer = await $.ui.mount(at('terminal', 'TurnDuration', { word: 'Baked', durationMs: 4000, isExpanded: false }))
    expect(JSON.stringify(await footer.drawn())).toContain('Baked')
    await footer.unmount()
    const spinner = await $.ui.mount(at('terminal', 'Spinner', { word: 'Working', message: null, suffix: '', mode: 'requesting' }))
    const spun = JSON.stringify(await spinner.drawn()).replace(/[^A-Za-z]/g, '')
    expect(MIAMI_NIGHTS.words.spinner.some(w => spun.includes(w))).toBe(true)
    await spinner.unmount()
    world.mod = json({ ...MIAMI_NIGHTS, words: null })
    await h.clock.advance(PICKUP_MS)
    const plain = await $.ui.mount(at('terminal', 'Spinner', { word: 'Working', message: null, suffix: '', mode: 'requesting' }))
    expect(JSON.stringify(await plain.drawn()).replace(/[^A-Za-z]/g, '')).toContain('Working')
    await plain.unmount()
    const done = await $.ui.mount(at('terminal', 'TurnDuration', { word: 'Baked', durationMs: 4000, isExpanded: false }))
    expect(JSON.stringify(await done.drawn())).toContain('Baked')
    await done.unmount()
  })

  test('the spinner and the footer take the theme\'s words', async ($, on) => {
    host(on, { mod: json(MIAMI_NIGHTS) })
    await $.session.start(INTERACTIVE)
    const spinner = await $.ui.mount(at('terminal', 'Spinner', { word: 'Working', message: null, suffix: '', mode: 'requesting' }))
    const text = JSON.stringify(await spinner.drawn()).replace(/[^A-Za-z]/g, '')
    expect(MIAMI_NIGHTS.words.spinner.some(w => text.includes(w))).toBe(true)
    await spinner.unmount()
    const footer = await $.ui.mount(at('terminal', 'TurnDuration', { word: 'Baked', durationMs: 4000, isExpanded: false }))
    const done = JSON.stringify(await footer.drawn())
    expect(MIAMI_NIGHTS.words.done.some(w => done.includes(w))).toBe(true)
    await footer.unmount()
  })
})

describe('the menus are gone', () => {
  test('the module registers no command, tool, pane, store, prompt section or classic hook', () => {
    const patterns = registrationsOf(register as never).map(r => r.pattern)
    for (const gone of ['command.run', 'engine.create', 'prompt.compose', 'skins.markdown']) expect(patterns).not.toContain(gone)
    expect(patterns.filter(p => p.startsWith('classic.'))).toEqual([])
    const matchers = registrationsOf(register as never).map(r => JSON.stringify(r.matcher ?? {}))
    expect(matchers.some(m => m.includes('Pane') || m.includes('design'))).toBe(false)
  })

  test('a session registers no slash command or tool, opens no pane, keeps no store and fetches nothing', async ($, on) => {
    const h = host(on, { mod: json(MIAMI_NIGHTS) })
    await $.session.start(INTERACTIVE)
    await h.clock.advance(PICKUP_MS)
    const ui = await $.ui.mount(at('terminal', 'AbovePrompt', { hasSurvey: false, isWorking: false, maxRows: 4, bodyColumns: 100 }))
    expect(await ui.find({ type: 'Button', text: /Compact/ })).toBeUndefined()
    await ui.unmount()
    expect(h.stripped).toEqual([])
    expect(h.toasts).toEqual([])
  })
})

describe('one frame per row', () => {
  test('a reply, a tool row and a tool group are each framed once', async ($, on) => {
    host(on, { mod: json(MIAMI_NIGHTS) })
    await $.session.start(INTERACTIVE)
    for (const [component, props] of [['AssistantMessage', REPLY], ['ToolUse', READ], ['ToolGroup', GROUP]] as const) {
      const ui = await $.ui.mount(at('terminal', component, props))
      const tree = await ui.drawn()
      expect(borders(tree).length).toBe(1)
      // The frame wraps skins' drawing, which opens on the blank line it takes back.
      expect(shows(tree, 'engine')).toBe(false)
      await ui.unmount()
    }
  })

  test('inside a frame the rail draws no connector', async ($, on) => {
    host(on, { mod: json(MIAMI_NIGHTS) })
    await $.session.start(INTERACTIVE)
    const ui = await $.ui.mount(at('terminal', 'ToolUse', READ))
    expect(shows(await ui.drawn(), '┃')).toBe(false)
    await ui.unmount()
  })

  // The frame and the row inside it read one predicate of one look: a frame
  // always holds a row without its rail, and an unframed row keeps its rail.
  const framedOnce = (tree: RenderElement) => borders(tree).length === 1 && !shows(tree, '┃')
  const bare = (tree: RenderElement) => borders(tree).length === 0 && shows(tree, '┃')

  test('under verbose, rows that report expanded are framed once, the rail stripped', async ($, on) => {
    host(on, { mod: json(MIAMI_NIGHTS), verbose: true })
    await $.session.start(INTERACTIVE)
    const line = await $.ui.mount(at('terminal', 'TurnDuration', { word: 'Baked', durationMs: 4000, isExpanded: true }, 'turn1'))
    const row = await $.ui.mount(at('terminal', 'ToolUse', { ...READ, isExpanded: true }))
    expect(framedOnce(await row.drawn())).toBe(true)
    const reply = await $.ui.mount(at('terminal', 'AssistantMessage', REPLY, 'm1'))
    expect(borders(await reply.drawn()).length).toBe(1)
    await reply.unmount()
    await row.unmount()
    await line.unmount()
  })

  const switches: [string, Answer, Answer, boolean, boolean][] = [
    ['a themed session switched to terminal', json(MIAMI_NIGHTS), json(TERMINAL_THEME), true, false],
    ['a terminal session switched to a theme', json(TERMINAL_THEME), json(MIAMI_NIGHTS), false, true],
    ['an export that failed at start and then loads', new Error('timed out after 1500 ms'), json(MIAMI_NIGHTS), false, true],
  ]
  for (const [name, before, after, wasFramed, isFramedAfter] of switches) {
    test(`${name} reframes the rows it has drawn`, async ($, on) => {
      const world: World = { mod: before }
      const h = host(on, world)
      await $.session.start(INTERACTIVE)
      const row = await $.ui.mount(at('terminal', 'ToolUse', READ))
      const first = await row.drawn()
      expect(wasFramed ? framedOnce(first) : borders(first).length === 0).toBe(true)
      world.mod = after
      await h.clock.advance(PICKUP_MS)
      const second = await row.drawn()
      expect(isFramedAfter ? framedOnce(second) : bare(second)).toBe(true)
      await row.unmount()
    })
  }

  test('a shell call is its card: the row goes unframed and the card holds the one border', async ($, on) => {
    host(on, { mod: json(MIAMI_NIGHTS) })
    await $.session.start(INTERACTIVE)
    const row = await $.ui.mount(at('terminal', 'ToolUse', BASH, 't2'))
    expect(borders(await row.drawn()).length).toBe(0)
    await row.unmount()
    const card = await $.ui.mount(at('terminal', 'ToolResult', { tool_use_id: 't2', tool: 'Bash', output: BASH_OUT, isErrored: false }, 't2'))
    expect(borders(await card.drawn()).length).toBe(1)
    await card.unmount()
  })

  test('render.frames off draws every row unframed', async ($, on) => {
    host(on, { mod: json({ ...MIAMI_NIGHTS, render: { ...MIAMI_NIGHTS.render, frames: false } }) })
    await $.session.start(INTERACTIVE)
    for (const [component, props] of [['AssistantMessage', REPLY], ['ToolUse', READ], ['ToolGroup', GROUP]] as const) {
      const ui = await $.ui.mount(at('terminal', component, props))
      expect(borders(await ui.drawn()).length).toBe(0)
      await ui.unmount()
    }
  })

  test("the person's message: the gradient box has no border, the frame one", async ($, on) => {
    const world: World = { mod: json(MIAMI_NIGHTS) }
    const h = host(on, world)
    await $.session.start(INTERACTIVE)
    const box = await $.ui.mount(at('terminal', 'UserMessage', PERSON))
    expect(borders(await box.drawn()).length).toBe(0)
    await box.unmount()
    world.mod = json({ ...MIAMI_NIGHTS, render: { ...MIAMI_NIGHTS.render, prompt_box: 'frame' } })
    await h.clock.advance(PICKUP_MS)
    const frame = await $.ui.mount(at('terminal', 'UserMessage', PERSON))
    expect(borders(await frame.drawn()).length).toBe(1)
    await frame.unmount()
    world.mod = json({ ...MIAMI_NIGHTS, render: { ...MIAMI_NIGHTS.render, prompt_box: 'off' } })
    await h.clock.advance(PICKUP_MS)
    const engine = await $.ui.mount(at('terminal', 'UserMessage', PERSON))
    expect(await engine.drawn()).toMatchObject({ type: 'Text' })
    await engine.unmount()
  })
})

describe('the gradient prompt box', () => {
  const V = MIAMI_NIGHTS.gradient
  const H = TAMPA.gradient

  test('lines wrap to the box, padded with ground on every side', () => {
    const lines = boxLines('one two three four five six', 16)
    expect(lines.every(l => [...l].length === 16)).toBe(true)
    expect(lines[0]!.trim()).toBe('')
    expect(lines[lines.length - 1]!.trim()).toBe('')
    expect(lines.slice(1, -1).every(l => l.startsWith(' '.repeat(PAD_X)))).toBe(true)
    expect(wrapCells('abcdefghij', 4)).toEqual(['abcd', 'efgh', 'ij'])
    expect(boxLines('a\n\nb', 20).length).toBe(5)
  })

  test('a paragraph keeps its indentation; a line a wrap began drops its opening spaces', () => {
    expect(wrapCells('    return 1', 40)).toEqual(['    return 1'])
    expect(boxLines('def f():\n\treturn 1', 30)[2]).toBe(`${' '.repeat(PAD_X)}  return 1${' '.repeat(30 - PAD_X * 2 - 10)}${' '.repeat(PAD_X)}`)
    expect(wrapCells('aaa    bbb', 3)).toEqual(['aaa', 'bbb'])
  })

  test('an emoji sequence is one grapheme two cells wide, never split across runs', () => {
    const family = '\u{1f468}\u200d\u{1f469}\u200d\u{1f467}'
    expect(graphemes(family)).toEqual([family])
    expect(cellsOf(family)).toBe(2)
    expect(cellsOf('\u2764\ufe0f')).toBe(2)
    const text = `${family.repeat(6)} \u{1f44d}\u{1f3fd} \u{1f1ef}\u{1f1f5}`
    const lines = boxLines(text, 24)
    const runs = gradientRuns(lines, 24, V.stops, 'horizontal')
    expect(runs.map(line => line.map(r => r.text).join(''))).toEqual(lines)
    expect(runs.flat().every(r => !/^[\u200d\ufe0f\u{1f3fb}-\u{1f3ff}]/u.test(r.text))).toBe(true)
    expect(lines.every(l => graphemes(l).reduce((n, g) => n + cellsOf(g), 0) === 24)).toBe(true)
  })

  test('without grapheme segmentation, text holding an emoji sequence takes the outline', () => {
    expect(canMeasure('plain words', false)).toBe(true)
    expect(canMeasure('a \u2764\ufe0f', false)).toBe(false)
    expect(canMeasure('a \u{1f1ef}\u{1f1f5}', false)).toBe(false)
    expect(canMeasure('a \u2764\ufe0f', true)).toBe(true)
  })

  test('a vertical gradient runs down the lines, one ground a line', () => {
    const lines = boxLines('first\nsecond\nthird', 30)
    const runs = gradientRuns(lines, 30, V.stops, 'vertical')
    expect(runs.every(line => line.length === 1)).toBe(true)
    const ls = runs.map(line => toOklab(line[0]!.ground)[0])
    expect(new Set(runs.map(line => line[0]!.ground)).size).toBe(lines.length)
    expect(ls[0]).toBeLessThan(ls[ls.length - 1]!)
  })

  test('a horizontal gradient runs across the cells, each line alike', () => {
    const lines = boxLines('hello there', 40)
    const runs = gradientRuns(lines, 40, H.stops, 'horizontal')
    expect(runs[0]!.length).toBeGreaterThan(3)
    expect(runs.map(line => line.map(r => r.ground))).toEqual(runs.map(() => runs[0]!.map(r => r.ground)))
    expect(runs[1]!.map(r => r.text).join('')).toBe(lines[1])
    expect(runs[0]![0]!.ground).not.toBe(runs[0]![runs[0]!.length - 1]!.ground)
  })

  test('the mix is OKLab: halfway is half the lightness, not half the sRGB value', () => {
    expect(mix('#000000', '#ffffff', 0)).toBe('#000000')
    expect(mix('#000000', '#ffffff', 1)).toBe('#ffffff')
    expect(Math.abs(toOklab(mix('#000000', '#ffffff', 0.5))[0] - 0.5)).toBeLessThan(0.01)
    expect(mix('#000000', '#ffffff', 0.5)).not.toBe('#808080')
    expect(gradientAt(['#000000', '#ffffff', '#000000'], 0.5)).toBe('#ffffff')
    expect(gradientAt(['#3d1466'], 0.7)).toBe('#3d1466')
  })

  test(`under ${VERTICAL_FROM_LINES} lines the gradient runs across, from ${VERTICAL_FROM_LINES} down`, () => {
    expect(axisFor(1)).toBe('horizontal')
    expect(axisFor(VERTICAL_FROM_LINES - 1)).toBe('horizontal')
    expect(axisFor(VERTICAL_FROM_LINES)).toBe('vertical')
  })

  test('drawn on the terminal: the body colour on each line\'s ground', async ($, on) => {
    host(on, { mod: json(MIAMI_NIGHTS) })
    await $.session.start(INTERACTIVE)
    const ui = await $.ui.mount(at('terminal', 'UserMessage', PERSON))
    const tree = await ui.drawn()
    expect(new Set(grounds(tree)).size).toBeGreaterThanOrEqual(3)
    expect(walk(tree).filter(n => typeof n.props?.backgroundColor === 'string').every(n => n.props!.color === MIAMI_NIGHTS.roles.body)).toBe(true)
    const text = walk(tree).flatMap(n => (n.children ?? []).filter(c => typeof c === 'string')).join('')
    expect(text).toContain(PERSON.text)
    await ui.unmount()
  })

  /** Each drawn line's grounds, left to right. */
  const lineGrounds = (tree: RenderElement) =>
    walk(tree)
      .filter(n => n.type === 'Text' && n.props?.backgroundColor === undefined && (n.children ?? []).some(c => typeof c === 'object'))
      .map(line => walk(line).filter(n => typeof n.props?.backgroundColor === 'string').map(n => n.props!.backgroundColor as string))

  test('one line on a vertical theme still runs across, padding rows alike', async ($, on) => {
    host(on, { mod: json(MIAMI_NIGHTS) })
    await $.session.start(INTERACTIVE)
    const ui = await $.ui.mount(at('terminal', 'UserMessage', PERSON))
    const lines = lineGrounds(await ui.drawn())
    expect(lines.length).toBe(3)
    expect(lines[0]!.length).toBeGreaterThan(10)
    expect(lines[1]!.length).toBeGreaterThan(10)
    expect(lines[0]).toEqual(lines[2])
    await ui.unmount()
  })

  test('five lines on a horizontal theme run down, one ground a line', async ($, on) => {
    host(on, { mod: json(TAMPA) })
    await $.session.start(INTERACTIVE)
    const ui = await $.ui.mount(at('terminal', 'UserMessage', { ...PERSON, text: 'one\ntwo\nthree\nfour\nfive' }))
    const lines = lineGrounds(await ui.drawn())
    expect(lines.length).toBe(7)
    expect(lines.every(l => l.length === 1)).toBe(true)
    expect(new Set(lines.map(l => l[0])).size).toBe(7)
    await ui.unmount()
  })

  const fallbacks: [string, Record<string, unknown>, Record<string, unknown>?][] = [
    ['a pasted block', { text: 'look at [Pasted text #1 +40 lines]' }],
    ['the row expanded with ctrl+o', { isExpanded: true }],
    ['a view too narrow for the box', {}, { columns: MIN_WIDTH - 2, rows: 30 }],
  ]
  for (const [name, props, viewport] of fallbacks) {
    test(`${name} takes the plain frame`, async ($, on) => {
      host(on, { mod: json(MIAMI_NIGHTS) })
      await $.session.start(INTERACTIVE)
      const target = at('terminal', 'UserMessage', { ...PERSON, ...props }) as { viewport: unknown }
      const ui = await $.ui.mount({ ...target, viewport: viewport ?? VIEW } as never)
      const tree = await ui.drawn()
      expect(borders(tree).length).toBe(1)
      expect(grounds(tree)).toEqual([])
      await ui.unmount()
    })
  }

  test('an image in the prompt takes the plain frame, with the engine drawing the image', async ($, on) => {
    host(on, { mod: json(MIAMI_NIGHTS) })
    await $.session.start(INTERACTIVE)
    const stored = await $.session.append({ door: 'prompt', message: { role: 'user', content: [{ type: 'text', text: PERSON.text }, { type: 'image', source: { type: 'base64', media_type: 'image/png', data: 'iVBORw0KGgo=' } }] }, uuid: 'stored-1' } as never)
    const ui = await $.ui.mount(at('terminal', 'UserMessage', PERSON, (stored as { uuid: string }).uuid))
    const tree = await ui.drawn()
    expect(borders(tree).length).toBe(1)
    expect(shows(tree, 'engine')).toBe(true)
    await ui.unmount()
  })

  test('the transcript in ctrl+o, read from the turn line, takes the plain frame', async ($, on) => {
    host(on, { mod: json(MIAMI_NIGHTS) })
    await $.session.start(INTERACTIVE)
    const line = await $.ui.mount(at('terminal', 'TurnDuration', { word: 'Baked', durationMs: 4000, isExpanded: true }, 'turn1'))
    const ui = await $.ui.mount(at('terminal', 'UserMessage', PERSON))
    expect(grounds(await ui.drawn())).toEqual([])
    await ui.unmount()
    await line.unmount()
  })

  test('under verbose, where every row and turn line reports expanded, the box keeps its gradient', async ($, on) => {
    host(on, { mod: json(MIAMI_NIGHTS), verbose: true })
    await $.session.start(INTERACTIVE)
    const line = await $.ui.mount(at('terminal', 'TurnDuration', { word: 'Baked', durationMs: 4000, isExpanded: true }, 'turn1'))
    const ui = await $.ui.mount(at('terminal', 'UserMessage', { ...PERSON, isExpanded: true }))
    const tree = await ui.drawn()
    expect(borders(tree).length).toBe(0)
    expect(new Set(grounds(tree)).size).toBeGreaterThanOrEqual(3)
    await ui.unmount()
    await line.unmount()
  })

  test("another Claude Code theme than custom:ways leaves the person's message to the engine, live", async ($, on) => {
    host(on, { mod: json(MIAMI_NIGHTS), selected: 'dark' })
    await $.session.start(INTERACTIVE)
    const ui = await $.ui.mount(at('terminal', 'UserMessage', PERSON))
    expect(await ui.drawn()).toMatchObject({ type: 'Text' })
    await $.config.set({ key: 'theme', value: 'custom:ways' } as never)
    const tree = await ui.drawn()
    expect(borders(tree).length).toBe(0)
    expect(new Set(grounds(tree)).size).toBeGreaterThanOrEqual(3)
    await ui.unmount()
  })

  test('a theme with no gradient draws a flat box in its selection colour', async ($, on) => {
    host(on, { mod: json({ ...MIAMI_NIGHTS, gradient: null }) })
    await $.session.start(INTERACTIVE)
    const ui = await $.ui.mount(at('terminal', 'UserMessage', PERSON))
    const tree = await ui.drawn()
    expect(borders(tree).length).toBe(0)
    expect([...new Set(grounds(tree))]).toEqual([MIAMI_NIGHTS.roles.selection_bg])
    await ui.unmount()
  })

  test('the terminal theme, which has no colours, takes the plain frame', async ($, on) => {
    host(on, { mod: json({ ...TERMINAL_THEME, render: { ...TERMINAL_THEME.render, prompt_box: 'gradient' } }) })
    await $.session.start(INTERACTIVE)
    const ui = await $.ui.mount(at('terminal', 'UserMessage', PERSON))
    expect(borders(await ui.drawn()).length).toBe(1)
    await ui.unmount()
  })

  test('a control character leaves the row to the engine', async ($, on) => {
    host(on, { mod: json(MIAMI_NIGHTS) })
    await $.session.start(INTERACTIVE)
    const ui = await $.ui.mount(at('terminal', 'UserMessage', { ...PERSON, text: 'raw \u001b[31mred' }))
    expect(await ui.drawn()).toMatchObject({ type: 'Text' })
    await ui.unmount()
  })

  test("another party's message is the engine's", async ($, on) => {
    host(on, { mod: json(MIAMI_NIGHTS) })
    await $.session.start(INTERACTIVE)
    const ui = await $.ui.mount(at('terminal', 'UserMessage', { ...PERSON, origin: { kind: 'peer' } }))
    expect(await ui.drawn()).toMatchObject({ type: 'Text' })
    await ui.unmount()
  })
})

describe('the copy controls', () => {
  const TABLE_REPLY = { text: 'Two:\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\nAnd more.', isFirstOfReply: true }

  test("render.copy on draws the table's forms and the reply's", async ($, on) => {
    host(on, { mod: json(MIAMI_NIGHTS) })
    await $.session.start(INTERACTIVE)
    const ui = await $.ui.mount({ ...(at('terminal', 'AssistantMessage', TABLE_REPLY) as object), viewport: { ...VIEW, isFullscreen: true } } as never)
    const keys = (await ui.findAll({ type: 'Button' })).map(button => button.key)
    expect(keys).toEqual(['copy-1-markdown', 'copy-1-native', 'copy-1-text', 'copy-reply-markdown', 'copy-reply-text'])
    await ui.unmount()
  })

  test('render.copy off draws none of them and leaves no row where they were', async ($, on) => {
    host(on, { mod: json({ ...MIAMI_NIGHTS, render: { ...MIAMI_NIGHTS.render, copy: false } }) })
    await $.session.start(INTERACTIVE)
    const ui = await $.ui.mount(at('terminal', 'AssistantMessage', TABLE_REPLY))
    expect(await ui.findAll({ type: 'Button' })).toEqual([])
    const empty = walk(await ui.drawn()).filter(n => n.type === 'Box' && n.props?.justifyContent === 'flex-end')
    expect(empty).toEqual([])
    await ui.unmount()
    const card = await $.ui.mount(at('terminal', 'ToolResult', { tool_use_id: 't2', tool: 'Bash', output: BASH_OUT, isErrored: false }, 't2'))
    expect(await card.find({ type: 'Button', text: /Copy/ })).toBeUndefined()
    await card.unmount()
  })
})

describe('live pickup', () => {
  test('a changed export lands within one pickup, with no restart', async ($, on) => {
    const world: World = { mod: json(MIAMI_NIGHTS) }
    const h = host(on, world)
    await $.session.start(INTERACTIVE)
    const before = await $.ui.mount(at('terminal', 'UserMessage', PERSON))
    expect(JSON.stringify(await before.drawn())).toContain(MIAMI_NIGHTS.roles.body)
    await before.unmount()
    world.mod = json(TAMPA)
    await h.clock.advance(PICKUP_MS)
    const after = await $.ui.mount(at('terminal', 'UserMessage', PERSON))
    const drawn = JSON.stringify(await after.drawn())
    expect(drawn).toContain(TAMPA.roles.body)
    expect(drawn).not.toContain(MIAMI_NIGHTS.roles.body)
    await after.unmount()
    expect(h.modRuns.length).toBe(2)
  })

  test('a mounted row redraws when the export changes', async ($, on) => {
    const world: World = { mod: json(MIAMI_NIGHTS) }
    const h = host(on, world)
    await $.session.start(INTERACTIVE)
    const ui = await $.ui.mount(at('terminal', 'ToolGroup', GROUP))
    expect(JSON.stringify(await ui.drawn())).toContain(MIAMI_NIGHTS.roles.info)
    world.mod = json(TAMPA)
    await h.clock.advance(PICKUP_MS)
    expect(JSON.stringify(await ui.drawn())).toContain(TAMPA.roles.info)
    await ui.unmount()
  })

  test('a failed read keeps the look it had', async ($, on) => {
    const world: World = { mod: json(MIAMI_NIGHTS) }
    const h = host(on, world)
    await $.session.start(INTERACTIVE)
    world.mod = ok('', 1)
    await h.clock.advance(PICKUP_MS)
    const ui = await $.ui.mount(at('terminal', 'ToolGroup', GROUP))
    expect(JSON.stringify(await ui.drawn())).toContain(MIAMI_NIGHTS.roles.info)
    await ui.unmount()
    expect(h.logs.length).toBe(1)
  })

  test('an unchanged export writes nothing and redraws nothing', async ($, on) => {
    const h = host(on, { mod: json(MIAMI_NIGHTS) })
    await $.session.start(INTERACTIVE)
    const ui = await $.ui.mount(at('terminal', 'ToolGroup', GROUP))
    const first = await ui.drawn()
    await h.clock.advance(PICKUP_MS * 2)
    expect(await ui.drawn()).toEqual(first)
    await ui.unmount()
  })
})

describe('surfaces other than the terminal', () => {
  const SITES: [string, Record<string, unknown>][] = [
    ['UserMessage', PERSON],
    ['AssistantMessage', REPLY],
    ['ToolUse', READ],
    ['ToolGroup', GROUP],
    ['ToolResult', { tool_use_id: 't2', tool: 'Bash', output: BASH_OUT, isErrored: false }],
    ['CommandOutput', { command: 'cost', args: '', text: 'Total cost: $0.00\nDuration: 4s', isErrored: false }],
    ['Spinner', { word: 'Working', message: null, suffix: '', mode: 'requesting' }],
    ['TurnDuration', { word: 'Baked', durationMs: 4000 }],
    ['AbovePrompt', { hasSurvey: false, isWorking: false, maxRows: 4, bodyColumns: 100 }],
    ['AskUserQuestion', { tool: 'AskUserQuestion', questions: [{ header: 'Theme', question: 'Which?', options: [] }] }],
  ]
  for (const [component, props] of SITES) {
    test(`${component} draws on the desktop, with no frame and no gradient`, async ($, on) => {
      const h = host(on, { mod: json(MIAMI_NIGHTS) })
      await $.session.start({ ...INTERACTIVE, surface: 'desktop' })
      const ui = await $.ui.mount(at('desktop', component, props))
      const tree = await ui.drawn()
      expect(walk(tree).filter(n => n.props?.borderStyle === 'round' && n.props?.marginTop === 1)).toEqual([])
      expect(grounds(tree).filter(g => g.startsWith('#0d') || g.startsWith('#24') || g.startsWith('#4a'))).toEqual([])
      await ui.unmount()
      expect(h.toasts).toEqual([])
    })
  }

  for (const surface of ['vscode', 'mobile'] as const) {
    test(`${surface}: every site draws, and the person's message is the engine's`, async ($, on) => {
      host(on, { mod: json(MIAMI_NIGHTS) })
      await $.session.start({ ...INTERACTIVE, surface })
      for (const [component, props] of SITES) {
        const ui = await $.ui.mount(at(surface, component, props))
        const tree = await ui.drawn()
        if (component === 'UserMessage') expect(tree).toMatchObject({ type: 'Text' })
        await ui.unmount()
      }
    })
  }

  test('the bookkeeping hooks run the same on the desktop', async ($, on) => {
    host(on, { mod: json(MIAMI_NIGHTS) })
    await $.session.start({ ...INTERACTIVE, surface: 'desktop' })
    const ran = await $.tool.call({ tool: 'Bash', tool_use_id: 't9', command: 'ls', description: 'list' } as never).catch((e: unknown) => e)
    expect(ran).toBeDefined()
  })
})
