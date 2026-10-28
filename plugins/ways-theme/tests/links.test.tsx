import { describe, expect, mock, test } from 'claude-code/testing'
import type { Engine } from 'claude-code/testing'
import type { On, ProcessRunResult, RenderElement } from 'claude-code'

import { MOD_EXPORT_ARGV } from '../hooks/skins/config.ts'
import { MIAMI_NIGHTS } from './fixtures.ts'

const ok = (stdout: string): ProcessRunResult => ({ exitCode: 0, stdout, stderr: '', isStdoutTruncated: false, isStderrTruncated: false })

// Claude Code's export of the active theme, for the register module's frames.
const CLAUDE_CODE_MIAMI = { name: 'Miami Nights', base: 'dark', overrides: { claude: '#ff4d93', userMessageBackground: '#3d1466' } }

type Env = Readonly<Record<string, string>>

const KITTY: Env = { TERM: 'xterm-kitty', HOME: '/home/dev' }

/** The host a reply draws in: Miami Nights exported and selected, the session in
 *  `cwd`, the environment `env`, every other render the engine's. */
function host(on: On, env: Env, cwd = '/work/proj') {
  mock.clock(on, { now: 1_000 })
  on('session.start', ($, e) => ({ cwd: e.cwd }))
  on('session.cwd', () => ({ value: cwd }) as never)
  on('session.usage', () => ({ value: { startedAt: 0, context: { window: 200000, percent: 42 }, rateLimits: [] } }) as never)
  on('ui.log', () => ({ value: undefined }) as never)
  on('process.run', ($, e) => {
    const argv = e.argv.join(' ')
    if (argv === MOD_EXPORT_ARGV.join(' ')) return { value: ok(JSON.stringify(MIAMI_NIGHTS)) } as never
    if (argv === 'ways settings get theme.active') return { value: ok('miami-nights\n') } as never
    return { value: ok(JSON.stringify(CLAUDE_CODE_MIAMI)) } as never
  })
  on('env.get', ($, e) => ({ value: env[e.name] }) as never)
  on('fs.read', () => {
    throw new Error('ENOENT')
  })
  on('fs.write', () => ({ value: undefined }) as never)
  on('config.list', () => ({ value: [{ key: 'theme', label: 'Theme', kind: 'choice', value: 'custom:ways', provider: { kind: 'engine' }, isLocked: false }] }) as never)
  on('ui.render', ($, e) => {
    const { Text } = $.ui.resolve(e)
    return <Text>engine</Text>
  })
  return { start: { cwd, surface: 'terminal', isInteractive: true } as const }
}

type Node = { type?: string; props?: Record<string, unknown>; children?: unknown[] }

function walk(tree: unknown, out: Node[] = []): Node[] {
  if (tree === null || typeof tree !== 'object') return out
  const node = tree as Node
  if (typeof node.type === 'string') out.push(node)
  for (const child of node.children ?? []) walk(child, out)
  return out
}

const hrefs = (tree: RenderElement): string[] => walk(tree).filter(n => n.type === 'Link').map(n => String(n.props?.href))

const VIEW = { columns: 100, rows: 30 }
const at = (component: string, props: Record<string, unknown>) => ({ plugin: 'ways-theme', surface: 'terminal', component, requestId: 'r1', viewport: VIEW, props }) as never
const reply = (text: string) => at('AssistantMessage', { text, isFirstOfReply: true })

/** The links a reply of `text` draws, in a session with `env` under `cwd`. */
async function linksOf($: Engine, on: On, text: string, env: Env = KITTY, cwd?: string) {
  const h = host(on, env, cwd)
  await $.session.start(h.start)
  const ui = await $.ui.mount(reply(text))
  const tree = await ui.drawn()
  await ui.unmount()
  return { links: hrefs(tree), tree }
}

describe('paths in a reply link to the file', () => {
  test('a bare absolute path links to it as file:///', async ($, on) => {
    const { links } = await linksOf($, on, 'The theme lives in /etc/skins/miami.json now.')
    expect(links).toEqual(['file:///etc/skins/miami.json'])
  })

  test('a relative path resolves against the session folder', async ($, on) => {
    const { links } = await linksOf($, on, 'See tools/ways-cli/src/main.rs for the entry point.')
    expect(links).toEqual(['file:///work/proj/tools/ways-cli/src/main.rs'])
  })

  test('a backtick path with :line links with the line as the fragment', async ($, on) => {
    const { links, tree } = await linksOf($, on, 'The check is in `tools/x.rs:12`, and in `./src/lib.rs:40:7`.')
    expect(links).toEqual(['file:///work/proj/tools/x.rs#12', 'file:///work/proj/src/lib.rs#40'])
    // The code span keeps its ground and shows the text as written.
    const spans = walk(tree).filter(n => n.type === 'Text' && typeof n.props?.backgroundColor === 'string')
    expect(spans.length).toBe(2)
    expect(JSON.stringify(tree)).toContain('tools/x.rs:12')
  })

  test('a home path resolves against HOME, and ../ and ./ fold away', async ($, on) => {
    const { links } = await linksOf($, on, 'Copy ~/.claude/themes/ways.json over ../shared/./a.ts first.')
    expect(links).toEqual(['file:///home/dev/.claude/themes/ways.json', 'file:///work/shared/a.ts'])
  })

  test('a folder holding spaces, # or non-ASCII is percent-encoded', async ($, on) => {
    const { links } = await linksOf($, on, 'Open `src/a.ts:3` and @scope/pkg/index.d.ts.', KITTY, '/work/my proj#2/naïve%')
    expect(links).toEqual(['file:///work/my%20proj%232/na%C3%AFve%25/src/a.ts#3', 'file:///work/my%20proj%232/na%C3%AFve%25/@scope/pkg/index.d.ts'])
  })

  test('a URL keeps its own link, underlined as before', async ($, on) => {
    const { links, tree } = await linksOf($, on, 'Read https://example.com/docs/a.md and src/b.ts.')
    expect(links).toEqual(['https://example.com/docs/a.md', 'file:///work/proj/src/b.ts'])
    const underlined = walk(tree).filter(n => n.type === 'Text' && n.props?.underline === true)
    expect(underlined.length).toBe(1)
  })

  test('a code span that is not a whole path is not linked', async ($, on) => {
    const { links } = await linksOf($, on, 'Run `make lint`, then `cargo test -p ways`, then `git diff src/a.ts`.')
    expect(links).toEqual([])
  })

  const NO_LINKS: [string, Env][] = [
    ['a terminal not known to draw them', { HOME: '/home/dev' }],
    ['tmux', { ...KITTY, TMUX: '/tmp/tmux-1000/default,1,0' }],
    ['FORCE_HYPERLINK=0', { ...KITTY, FORCE_HYPERLINK: '0' }],
  ]
  for (const [name, env] of NO_LINKS) {
    test(`under ${name} a path stays text, drawn as before`, async ($, on) => {
      const { links, tree } = await linksOf($, on, 'See tools/x.rs and `src/lib.rs:4`.', env)
      expect(links).toEqual([])
      expect(JSON.stringify(tree)).toContain('tools/x.rs')
      expect(JSON.stringify(tree)).toContain('src/lib.rs:4')
    })
  }

  test('a link stays under 2048 characters, or the path stays text', async ($, on) => {
    const long = `${'a'.repeat(60)}/`.repeat(40)
    const { links } = await linksOf($, on, `See ${long}end.rs and src/ok.rs.`)
    expect(links).toEqual(['file:///work/proj/src/ok.rs'])
  })
})

describe('an opened tool row links its path', () => {
  test('the path field of an opened Read links to the file', async ($, on) => {
    const h = host(on, KITTY)
    await $.session.start(h.start)
    const READ = { tool_use_id: 't1', tool: 'Read', input: { file_path: '/work/proj/src/a.ts' }, isRunning: false, isErrored: true, isInterrupted: false, isExpanded: false, output: 'File does not exist.' }
    const ui = await $.ui.mount(at('ToolUse', READ))
    expect(hrefs(await ui.drawn())).toEqual(['file:///work/proj/src/a.ts'])
    await ui.unmount()
  })
})
