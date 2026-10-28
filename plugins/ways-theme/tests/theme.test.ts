import { describe, expect, test } from 'claude-code/testing'
import type { On, ProcessRunResult } from 'claude-code'

import { register } from '../hooks/register.ts'
import { activeArgv, configDir, exportArgv, parseActive, parseTheme, plan, preference, themeFile, themeText } from '../hooks/lib.ts'
import { passedOverHits, registrationsOf } from './passed-over.ts'

const INTERACTIVE = { cwd: '/work/proj', surface: 'terminal', isInteractive: true } as const
const HEADLESS = { cwd: '/work/proj', surface: null, isInteractive: false } as const

const MIAMI = {
  name: 'Miami Nights',
  base: 'dark',
  overrides: { claude: '#ff4d93', text: '#f4eaff', success: '#3dffa2', userMessageBackground: '#3d1466' },
}
const FILE = '/home/dev/.claude/themes/ways-miami-nights.json'

const ok = (stdout: string, exitCode = 0): ProcessRunResult => ({ exitCode, stdout, stderr: '', isStdoutTruncated: false, isStderrTruncated: false })

/** What `ways` does for one call: print and exit, or fail to run at all
 *  (missing binary, a call past its bound). */
type Answer = ProcessRunResult | Error

type World = {
  active?: Answer
  exported?: Answer
  /** Claude Code's theme row now. */
  theme?: string
  /** Files on disk, by path. */
  files?: Record<string, string>
  env?: Record<string, string>
}

/** The host beneath the mod: the ways CLI, the file system, the
 *  environment and the /config rows answered from `world`, every call
 *  recorded. */
function host(on: On, world: World) {
  const argvs: string[][] = []
  const timeouts: (number | undefined)[] = []
  const writes: { path: string; text: string }[] = []
  const sets: unknown[] = []
  const logs: string[] = []
  const toasts: unknown[] = []
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
    writes.push({ path: e.path, text: e.text })
    files[e.path] = e.text
    return { value: undefined } as never
  })
  on('config.list', () => ({ value: [{ key: 'theme', label: 'Theme', kind: 'choice', value: world.theme ?? 'dark', provider: { kind: 'engine' }, isLocked: false }] }) as never)
  on('config.set', ($, e) => {
    sets.push({ key: e.key, value: e.value })
    return { value: e.value }
  })
  return { argvs, timeouts, writes, sets, logs, toasts, files }
}

describe('an agent-ways theme is active', () => {
  test('the export is written to the theme folder and selected', async ($, on) => {
    const h = host(on, { active: ok('miami-nights\n'), exported: ok(JSON.stringify(MIAMI)) })
    await $.session.start(INTERACTIVE)
    expect(h.argvs).toEqual([activeArgv(), exportArgv('miami-nights')])
    expect(h.timeouts.every(t => t !== undefined && t <= 2000)).toBe(true)
    expect(h.writes).toEqual([{ path: FILE, text: themeText(MIAMI) }])
    expect(h.sets).toEqual([{ key: 'theme', value: 'custom:ways-miami-nights' }])
    expect(h.toasts).toEqual([])
  })

  test('CLAUDE_CONFIG_DIR names the theme folder', async ($, on) => {
    const h = host(on, { active: ok('miami-nights\n'), exported: ok(JSON.stringify(MIAMI)), env: { HOME: '/home/dev', CLAUDE_CONFIG_DIR: '/cfg/cc' } })
    await $.session.start(INTERACTIVE)
    expect(h.writes.map(w => w.path)).toEqual(['/cfg/cc/themes/ways-miami-nights.json'])
  })

  test('a current file and a selected theme are left as they are', async ($, on) => {
    const h = host(on, { active: ok('miami-nights\n'), exported: ok(JSON.stringify(MIAMI)), theme: 'custom:ways-miami-nights', files: { [FILE]: themeText(MIAMI) } })
    await $.session.start(INTERACTIVE)
    expect(h.argvs.length).toBe(2)
    expect(h.writes).toEqual([])
    expect(h.sets).toEqual([])
  })

  test('a theme the person picked is followed over at the next session start', async ($, on) => {
    const h = host(on, { active: ok('miami-nights\n'), exported: ok(JSON.stringify(MIAMI)), theme: 'light', files: { [FILE]: themeText(MIAMI) } })
    await $.session.start(INTERACTIVE)
    expect(h.writes).toEqual([])
    expect(h.sets).toEqual([{ key: 'theme', value: 'custom:ways-miami-nights' }])
  })

  test('a headless session changes nothing', async ($, on) => {
    const h = host(on, { active: ok('miami-nights\n'), exported: ok(JSON.stringify(MIAMI)) })
    await $.session.start(HEADLESS)
    expect(h.argvs).toEqual([])
    expect(h.writes).toEqual([])
    expect(h.sets).toEqual([])
  })
})

describe('the terminal theme is active', () => {
  test("the mod's own theme goes back to dark", async ($, on) => {
    const h = host(on, { active: ok('terminal\n'), theme: 'custom:ways-miami-nights' })
    await $.session.start(INTERACTIVE)
    expect(h.argvs).toEqual([activeArgv()])
    expect(h.sets).toEqual([{ key: 'theme', value: 'dark' }])
    expect(h.writes).toEqual([])
  })

  for (const theme of ['light', 'custom:dracula', 'dark']) {
    test(`a theme of the person's own (${theme}) stays`, async ($, on) => {
      const h = host(on, { active: ok('terminal\n'), theme })
      await $.session.start(INTERACTIVE)
      expect(h.sets).toEqual([])
      expect(h.writes).toEqual([])
    })
  }
})

describe('a ways that fails changes nothing', () => {
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
  for (const [name, world] of cases) {
    test(name, async ($, on) => {
      const h = host(on, { ...world, theme: 'custom:ways-miami-nights' })
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

  test('the file and the preference share one slug', () => {
    expect(themeFile('/home/dev/.claude/', 'nord')).toBe('/home/dev/.claude/themes/ways-nord.json')
    expect(preference('nord')).toBe('custom:ways-nord')
    expect(configDir(undefined, '/home/dev')).toBe('/home/dev/.claude')
    expect(configDir('/cfg', '/home/dev')).toBe('/cfg')
    expect(configDir(undefined, undefined)).toBeUndefined()
  })

  test('the plan follows a theme, resets only its own, and leaves the rest', () => {
    expect(plan('nord', 'light')).toEqual({ kind: 'follow', name: 'nord' })
    expect(plan('terminal', 'custom:ways-nord')).toEqual({ kind: 'reset' })
    expect(plan('terminal', 'custom:nord')).toEqual({ kind: 'none' })
    expect(plan('terminal', undefined)).toEqual({ kind: 'none' })
  })
})

describe('what the mod hooks reaches it on every host', () => {
  test("no hook is on an event an organization's policy plugin passes over, and none is classic", () => {
    const regs = registrationsOf(register as never)
    // The recording saw the module's hook: the control for the empty lists below.
    expect(regs.map(r => r.pattern)).toEqual(['session.start'])
    expect(regs.filter(r => r.pattern.startsWith('classic.'))).toEqual([])
    expect(passedOverHits(regs)).toEqual([])
  })
})
