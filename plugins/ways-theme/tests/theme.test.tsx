import { describe, expect, test } from 'claude-code/testing'
import type { TestBody } from 'claude-code/testing'
import type { On, ProcessRunResult } from 'claude-code'

import { register } from '../hooks/register.tsx'
import { DEFAULT_THEME, activeArgv, configDir, exportArgv, parseActive, parseTheme, themeFile, themeText } from '../hooks/lib.ts'
import { passedOverHits, registrationsOf } from './passed-over.ts'

const INTERACTIVE = { cwd: '/work/proj', surface: 'terminal', isInteractive: true } as const
const DESKTOP = { cwd: '/work/proj', surface: 'desktop', isInteractive: true } as const
const HEADLESS = { cwd: '/work/proj', surface: null, isInteractive: false } as const

const MIAMI = {
  name: 'Miami Nights',
  base: 'dark',
  overrides: { claude: '#ff4d93', text: '#f4eaff', success: '#3dffa2', userMessageBackground: '#3d1466' },
}
const FILE = '/home/dev/.claude/themes/ways.json'

const ok = (stdout: string, exitCode = 0): ProcessRunResult => ({ exitCode, stdout, stderr: '', isStdoutTruncated: false, isStderrTruncated: false })

/** What `ways` does for one call: print and exit, or fail to run at all
 *  (missing binary, a call past its bound). */
type Answer = ProcessRunResult | Error

type World = {
  active?: Answer
  exported?: Answer
  /** Files on disk, by path. */
  files?: Record<string, string>
  env?: Record<string, string>
  /** Claude Code's `verbose` row. */
  verbose?: boolean
  /** `$.fs.write` rejects, as a read-only folder does. */
  writeFails?: boolean
}

/** The host beneath the mod: the ways CLI, the file system and the
 *  environment answered from `world`, every call recorded. A config.set or
 *  a toast the mod made would be recorded too; it makes none. */
function host(on: On, world: World) {
  const argvs: string[][] = []
  const timeouts: (number | undefined)[] = []
  const writes: { path: string; text: string }[] = []
  const sets: unknown[] = []
  const logs: string[] = []
  const toasts: unknown[] = []
  /** How often the engine drew each component. */
  const draws: Record<string, number> = {}
  const files = { ...(world.files ?? {}) }
  const env = world.env ?? { HOME: '/home/dev' }
  on('session.start', ($, e) => ({ cwd: e.cwd }))
  on('ui.log', ($, e) => {
    logs.push(String((e as { text?: unknown }).text ?? JSON.stringify(e)))
    return { value: undefined } as never
  })
  on('ui.toast', ($, e) => {
    toasts.push(e)
    return { value: undefined } as never
  })
  on('process.run', ($, e) => {
    argvs.push([...e.argv])
    timeouts.push(e.init?.timeoutMs)
    const a = e.argv.join(' ') === activeArgv().join(' ') ? world.active : world.exported
    if (a === undefined) throw new Error('spawn ways ENOENT')
    if (a instanceof Error) throw a
    return { value: a } as never
  })
  on('env.get', ($, e) => ({ value: env[e.name] }) as never)
  on('fs.read', ($, e) => {
    const text = files[e.path]
    if (text === undefined) throw new Error(`ENOENT: ${e.path}`)
    return { value: text } as never
  })
  on('fs.write', ($, e) => {
    if (world.writeFails) throw new Error(`EACCES: ${e.path}`)
    writes.push({ path: e.path, text: e.text })
    files[e.path] = e.text
    return { value: undefined } as never
  })
  on('config.set', ($, e) => {
    sets.push({ key: e.key, value: e.value })
    return { value: e.value }
  })
  on('config.list', () => ({ value: [{ key: 'verbose', label: 'Verbose', kind: 'boolean', value: world.verbose ?? false, provider: { kind: 'engine' }, isLocked: false }] }) as never)
  // The engine's own drawing of every row.
  on('ui.render', ($, e) => {
    draws[e.component] = (draws[e.component] ?? 0) + 1
    const { Text } = $.ui.resolve(e)
    return <Text>engine</Text>
  })
  return { argvs, timeouts, writes, sets, logs, toasts, files, draws }
}

describe('the theme file follows theme.active', () => {
  test('a fill theme is exported to themes/ways.json', async ($, on) => {
    const h = host(on, { active: ok('miami-nights\n'), exported: ok(JSON.stringify(MIAMI)) })
    await $.session.start(INTERACTIVE)
    expect(h.argvs).toEqual([activeArgv(), exportArgv('miami-nights')])
    expect(h.timeouts.every(t => t !== undefined && t <= 2000)).toBe(true)
    expect(h.writes).toEqual([{ path: FILE, text: themeText(MIAMI) }])
    expect(h.sets).toEqual([])
    expect(h.toasts).toEqual([])
  })

  test('terminal writes Claude Code\'s own dark, with no export', async ($, on) => {
    const h = host(on, { active: ok('terminal\n'), files: { [FILE]: themeText(MIAMI) } })
    await $.session.start(INTERACTIVE)
    expect(h.argvs).toEqual([activeArgv()])
    expect(h.writes).toEqual([{ path: FILE, text: themeText(DEFAULT_THEME) }])
    expect(h.sets).toEqual([])
  })

  test('CLAUDE_CONFIG_DIR names the theme folder', async ($, on) => {
    const h = host(on, { active: ok('miami-nights\n'), exported: ok(JSON.stringify(MIAMI)), env: { HOME: '/home/dev', CLAUDE_CONFIG_DIR: '/cfg/cc' } })
    await $.session.start(INTERACTIVE)
    expect(h.writes.map(w => w.path)).toEqual(['/cfg/cc/themes/ways.json'])
  })

  test('a current file is left as it is', async ($, on) => {
    const h = host(on, { active: ok('miami-nights\n'), exported: ok(JSON.stringify(MIAMI)), files: { [FILE]: themeText(MIAMI) } })
    await $.session.start(INTERACTIVE)
    expect(h.argvs.length).toBe(2)
    expect(h.writes).toEqual([])
  })

  test('a headless session changes nothing', async ($, on) => {
    const h = host(on, { active: ok('miami-nights\n'), exported: ok(JSON.stringify(MIAMI)) })
    await $.session.start(HEADLESS)
    expect(h.argvs).toEqual([])
    expect(h.writes).toEqual([])
  })

  // The file is the surface's own to read; a surface that does not read it
  // is left as it was. The mod writes the same file whatever draws.
  test('a desktop session writes the same file and nothing else', async ($, on) => {
    const h = host(on, { active: ok('miami-nights\n'), exported: ok(JSON.stringify(MIAMI)) })
    await $.session.start(DESKTOP)
    expect(h.writes).toEqual([{ path: FILE, text: themeText(MIAMI) }])
    expect(h.sets).toEqual([])
    expect(h.toasts).toEqual([])
  })
})

describe('a ways that fails leaves the file as it was', () => {
  const cases: [string, World][] = [
    ['ways is missing', {}],
    ['theme.active exits non-zero', { active: ok('', 2) }],
    ['theme.active prints garbage', { active: ok('error: config unreadable\nsee log\n') }],
    ['theme.active runs past its bound', { active: new Error('timed out after 1500 ms') }],
    ['the export exits non-zero', { active: ok('miami-nights\n'), exported: ok('', 3) }],
    ['the export prints garbage', { active: ok('miami-nights\n'), exported: ok('{"name": "Miami Nights", "base": "dark", "overrides": {"claude": "pink"}}') }],
    ['the export is cut short', { active: ok('miami-nights\n'), exported: ok(JSON.stringify(MIAMI).slice(0, 40)) }],
    ['the export runs past its bound', { active: ok('miami-nights\n'), exported: new Error('timed out after 1500 ms') }],
  ]
  test('a refused write throws nothing and draws no frame', async ($, on) => {
    const h = host(on, { active: ok('miami-nights\n'), exported: ok(JSON.stringify(MIAMI)), writeFails: true })
    await $.session.start(INTERACTIVE)
    expect(h.writes).toEqual([])
    expect(h.logs.length).toBe(1)
    const ui = await $.ui.mount({ ...REPLY, surface: 'terminal' } as never)
    expect(await ui.drawn()).toMatchObject({ type: 'Text' })
    await ui.unmount()
  })

  for (const [name, world] of cases) {
    test(name, async ($, on) => {
      const h = host(on, { ...world, files: { [FILE]: themeText(MIAMI) } })
      await $.session.start(INTERACTIVE)
      expect(h.writes).toEqual([])
      expect(h.sets).toEqual([])
      expect(h.toasts).toEqual([])
      expect(h.logs.length).toBeLessThanOrEqual(1)
    })
  }
})

describe('the rules', () => {
  test('theme.active reads one name, and nothing else', () => {
    expect(parseActive(ok('nord\n'))).toBe('nord')
    expect(parseActive(ok('\n'))).toBe('terminal')
    expect(parseActive(ok('../evil\n'))).toBeUndefined()
    expect(parseActive(ok('nord', 1))).toBeUndefined()
  })

  test('an export is a whole theme of #rrggbb colours on a known base', () => {
    expect(parseTheme(ok(JSON.stringify(MIAMI)))).toEqual(MIAMI)
    expect(parseTheme(ok(JSON.stringify({ ...MIAMI, base: 'neon' })))).toBeUndefined()
    expect(parseTheme(ok(JSON.stringify({ ...MIAMI, overrides: {} })))).toBeUndefined()
    expect(parseTheme(ok(JSON.stringify({ ...MIAMI, overrides: { claude: '#fff' } })))).toBeUndefined()
    expect(parseTheme(ok('[]'))).toBeUndefined()
  })

  test('one file, in the config folder', () => {
    expect(themeFile('/home/dev/.claude/')).toBe('/home/dev/.claude/themes/ways.json')
    expect(configDir(undefined, '/home/dev')).toBe('/home/dev/.claude')
    expect(configDir('/cfg', '/home/dev')).toBe('/cfg')
    expect(configDir(undefined, undefined)).toBeUndefined()
  })
})

const REPLY = { plugin: 'ways-theme', component: 'AssistantMessage', requestId: 'm1', props: { text: 'hello', isFirstOfReply: true } } as const
const TOOL = { plugin: 'ways-theme', component: 'ToolUse', requestId: 't1', props: { tool: 'Bash', input: {}, isRunning: false, isErrored: false, isInterrupted: false, isExpanded: false } } as const
const GROUP = { plugin: 'ways-theme', component: 'ToolGroup', requestId: 'g1', props: { calls: [], isActive: false, isExpanded: false } } as const
const THEMED: World = { active: ok('miami-nights\n'), exported: ok(JSON.stringify(MIAMI)) }

describe('frames on the terminal', () => {
  test('a reply sits on the theme ground inside a border, in theme keys', async ($, on) => {
    host(on, THEMED)
    await $.session.start(INTERACTIVE)
    const ui = await $.ui.mount({ ...REPLY, surface: 'terminal' } as never)
    expect(await ui.drawn()).toMatchObject({ type: 'Box', props: { marginTop: 1, borderStyle: 'round', backgroundColor: 'userMessageBackground', borderColor: 'claude' } })
    expect((await ui.find({ type: 'Text', text: /engine/ }))?.text).toBe('engine')
    await ui.unmount()
  })

  test('a light theme frames a reply on the same ground', async ($, on) => {
    host(on, { active: ok('tampa\n'), exported: ok(JSON.stringify({ ...MIAMI, name: 'Tampa', base: 'light' })) })
    await $.session.start(INTERACTIVE)
    const ui = await $.ui.mount({ ...REPLY, surface: 'terminal' } as never)
    expect(await ui.drawn()).toMatchObject({ type: 'Box', props: { borderStyle: 'round', backgroundColor: 'userMessageBackground', borderColor: 'claude' } })
    await ui.unmount()
  })

  test('a tool call and a tool group take a border; an errored call takes the error colour', async ($, on) => {
    host(on, THEMED)
    await $.session.start(INTERACTIVE)
    for (const [site, colour] of [[TOOL, 'suggestion'], [GROUP, 'suggestion'], [{ ...TOOL, props: { ...TOOL.props, isErrored: true } }, 'error']] as const) {
      const ui = await $.ui.mount({ ...site, surface: 'terminal' } as never)
      expect(await ui.drawn()).toMatchObject({ type: 'Box', props: { borderStyle: 'round', borderColor: colour } })
      await ui.unmount()
    }
  })

  const unchanged: [string, World, Record<string, unknown>, unknown][] = [
    ['an expanded tool call', THEMED, TOOL, { isExpanded: true }],
    ['an expanded tool group', THEMED, GROUP, { isExpanded: true }],
    ['a reply past the length bound', THEMED, REPLY, { text: 'x'.repeat(20001) }],
    ['a block that does not open a reply', THEMED, REPLY, { isFirstOfReply: false }],
    ['a reply under verbose', { ...THEMED, verbose: true }, REPLY, {}],
    ['a reply under the terminal theme', { active: ok('terminal\n') }, REPLY, {}],
    ['a reply when ways fails', {}, REPLY, {}],
  ]
  for (const [name, world, site, props] of unchanged) {
    test(`${name} is the engine's own`, async ($, on) => {
      host(on, world)
      await $.session.start(INTERACTIVE)
      const s = site as typeof REPLY
      const ui = await $.ui.mount({ ...s, props: { ...s.props, ...(props as object) }, surface: 'terminal' } as never)
      expect(await ui.drawn()).toMatchObject({ type: 'Text' })
      await ui.unmount()
    })
  }

  describe('ctrl+o on the transcript', () => {
    // A turn's closing line reports whether the view is the ctrl+o
    // transcript, for the whole view.
    const TURN = { plugin: 'ways-theme', component: 'TurnDuration', requestId: 'd1', props: { word: 'Baked', durationMs: 3000, isExpanded: false } } as const
    const PERSON = { plugin: 'ways-theme', component: 'UserMessage', requestId: 'u1', props: { text: 'hi', origin: { kind: 'composer' }, isExpanded: false } } as const
    const sites = [REPLY, TOOL, GROUP]
    const drawnAs = async ($: Parameters<TestBody>[0], type: string) => {
      for (const site of sites) {
        const ui = await $.ui.mount({ ...site, surface: 'terminal' } as never)
        expect(await ui.drawn()).toMatchObject({ type })
        await ui.unmount()
      }
    }

    test('the ctrl+o view unframes replies and tool rows; leaving it frames them again', async ($, on) => {
      host(on, THEMED)
      await $.session.start(INTERACTIVE)
      const turn = await $.ui.mount({ ...TURN, surface: 'terminal' } as never)
      await drawnAs($, 'Box')
      await turn.redraw({ ...TURN.props, isExpanded: true } as never)
      await drawnAs($, 'Text')
      await turn.redraw({ ...TURN.props, isExpanded: false } as never)
      await drawnAs($, 'Box')
      await turn.unmount()
    })

    test("the turn line is the engine's own", async ($, on) => {
      host(on, THEMED)
      await $.session.start(INTERACTIVE)
      for (const isExpanded of [true, false]) {
        const ui = await $.ui.mount({ ...TURN, props: { ...TURN.props, isExpanded }, surface: 'terminal' } as never)
        expect(await ui.drawn()).toMatchObject({ type: 'Text' })
        await ui.unmount()
      }
    })

    test('two turn lines that disagree settle and do not flip the view back', async ($, on) => {
      const h = host(on, THEMED)
      await $.session.start(INTERACTIVE)
      const a = await $.ui.mount({ ...TURN, props: { ...TURN.props, isExpanded: true }, surface: 'terminal' } as never)
      const b = await $.ui.mount({ ...TURN, requestId: 'd2', surface: 'terminal' } as never)
      // b's first report is the latest change: collapsed.
      await drawnAs($, 'Box')
      // a drawn again with its value unchanged moves nothing.
      await a.redraw({ ...TURN.props, isExpanded: true } as never)
      await drawnAs($, 'Box')
      expect(h.draws.TurnDuration).toBeLessThanOrEqual(8)
      await a.unmount()
      await b.unmount()
    })

    test("a person's row does not move the view", async ($, on) => {
      host(on, THEMED)
      await $.session.start(INTERACTIVE)
      // A row under a speaker label reports its own isExpanded.
      const ui = await $.ui.mount({ ...PERSON, props: { ...PERSON.props, isExpanded: true }, surface: 'terminal' } as never)
      expect(await ui.drawn()).toMatchObject({ type: 'Text' })
      await drawnAs($, 'Box')
      await ui.unmount()
    })

    test('a composer row on another surface does not change terminal framing', async ($, on) => {
      host(on, THEMED)
      await $.session.start(INTERACTIVE)
      const ui = await $.ui.mount({ ...PERSON, props: { ...PERSON.props, isExpanded: true }, surface: 'desktop' } as never)
      await drawnAs($, 'Box')
      await ui.unmount()
    })

    test('two composer rows that disagree leave the view as it was', async ($, on) => {
      const h = host(on, THEMED)
      await $.session.start(INTERACTIVE)
      const open = await $.ui.mount({ ...PERSON, props: { ...PERSON.props, isExpanded: true }, surface: 'terminal' } as never)
      const shut = await $.ui.mount({ ...PERSON, requestId: 'u2', surface: 'terminal' } as never)
      await open.redraw({ ...PERSON.props, isExpanded: true } as never)
      await drawnAs($, 'Box')
      expect(h.draws.UserMessage).toBeLessThanOrEqual(4)
      await open.unmount()
      await shut.unmount()
    })

    test('a turn line on another surface does not change terminal framing', async ($, on) => {
      host(on, THEMED)
      await $.session.start(INTERACTIVE)
      const ui = await $.ui.mount({ ...TURN, props: { ...TURN.props, isExpanded: true }, surface: 'desktop' } as never)
      await drawnAs($, 'Box')
      await ui.unmount()
    })
  })

  test('turning verbose on in /config hands the rows back', async ($, on) => {
    host(on, THEMED)
    await $.session.start(INTERACTIVE)
    await $.config.set({ key: 'verbose', value: true } as never)
    const ui = await $.ui.mount({ ...REPLY, surface: 'terminal' } as never)
    expect(await ui.drawn()).toMatchObject({ type: 'Text' })
    await ui.unmount()
  })

  for (const surface of ['desktop', 'vscode', 'mobile'] as const) {
    test(`${surface} draws every row as the engine does`, async ($, on) => {
      host(on, THEMED)
      await $.session.start({ ...INTERACTIVE, surface })
      for (const site of [REPLY, TOOL, GROUP]) {
        const ui = await $.ui.mount({ ...site, surface } as never)
        expect(await ui.drawn()).toMatchObject({ type: 'Text' })
        await ui.unmount()
      }
    })
  }
})

describe('what the mod hooks reaches it on every host', () => {
  test("no hook is on an event an organization's policy plugin passes over, and none is classic", () => {
    const regs = registrationsOf(register as never)
    // The recording saw the module's hooks: the control for the empty lists below.
    expect(regs.map(r => r.pattern)).toEqual(['session.start', 'config.set', 'ui.render', 'ui.render', 'ui.render', 'ui.render'])
    expect(regs.filter(r => r.pattern.startsWith('classic.'))).toEqual([])
    expect(passedOverHits(regs)).toEqual([])
  })
})
