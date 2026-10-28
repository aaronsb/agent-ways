import { describe, expect, test } from 'claude-code/testing'
import type { Engine } from 'claude-code/testing'
import type { On, RenderElement } from 'claude-code'

import { MIAMI_NIGHTS } from './fixtures.ts'
import { INTERACTIVE, host, json, walk } from './host.tsx'

type Env = Readonly<Record<string, string>>

const KITTY: Env = { TERM: 'xterm-kitty', HOME: '/home/dev' }

const hrefs = (tree: RenderElement): string[] => walk(tree).filter(n => n.type === 'Link').map(n => String(n.props?.href))

const VIEW = { columns: 100, rows: 30 }
const at = (component: string, props: Record<string, unknown>, surface = 'terminal') => ({ plugin: 'ways-theme', surface, component, requestId: 'r1', viewport: VIEW, props }) as never
const reply = (text: string) => at('AssistantMessage', { text, isFirstOfReply: true })

/** The links a reply of `text` draws, in a session with `env` under `cwd`. */
async function linksOf($: Engine, on: On, text: string, env: Env = KITTY, cwd: string = INTERACTIVE.cwd) {
  host(on, { mod: json(MIAMI_NIGHTS), env, cwd })
  await $.session.start({ ...INTERACTIVE, cwd })
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

  test('a lone home path is linked', async ($, on) => {
    const { links } = await linksOf($, on, 'Then reload ~/.bashrc')
    expect(links).toEqual(['file:///home/dev/.bashrc'])
  })

  const REMOTE: [string, string][] = [
    ['an scp target', 'scp user@host:/var/log/app.log .'],
    ['an rsync target', 'rsync box:/srv/data/a.db here'],
    ['a home path on another host', 'scp me@box:~/notes/a.txt .'],
    ['an s3 URL', 'Copy s3://bucket/dir/a.json down.'],
    ['an ftp URL', 'Fetch ftp://mirror.example.org/pub/x.tar.gz first.'],
    ['an ssh URL', 'Clone ssh://git@host/org/r.git now.'],
    ['a scheme-relative URL', 'Load //cdn.example.com/lib/x.js here.'],
  ]
  for (const [name, text] of REMOTE) {
    test(`${name} is not a local path`, async ($, on) => {
      const { links, tree } = await linksOf($, on, text)
      expect(links).toEqual([])
      // Nor is any part of it coloured as a path.
      expect(walk(tree).some(n => n.type === 'Text' && n.props?.color === MIAMI_NIGHTS.roles.info)).toBe(false)
    })
  }

  test('a drive path keeps its drive when .. folds past its root', async ($, on) => {
    const { links } = await linksOf($, on, 'See C:/x/../../y.txt and D:/a/./b.rs:7 now.')
    expect(links).toEqual(['file:///C:/y.txt', 'file:///D:/a/b.rs#7'])
  })

  test('a relative path under a network or WSL folder stays text', async ($, on) => {
    const { links, tree } = await linksOf($, on, 'See src/a.ts and `src/b.ts:4`.', KITTY, '\\\\wsl$\\Ubuntu\\home\\me\\proj')
    expect(links).toEqual([])
    expect(JSON.stringify(tree)).toContain('src/a.ts')
  })

  const NO_LINKS: [string, Env][] = [
    ['a terminal not known to draw them', { HOME: '/home/dev' }],
    ['tmux', { ...KITTY, TMUX: '/tmp/tmux-1000/default,1,0' }],
    ['FORCE_HYPERLINK=0', { ...KITTY, FORCE_HYPERLINK: '0' }],
    ['CI', { ...KITTY, CI: 'true' }],
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

describe('a tool row links its file', () => {
  const READ = { tool_use_id: 't1', tool: 'Read', input: { file_path: '/work/proj/src/a.ts' }, isRunning: false, isErrored: false, isInterrupted: false, isExpanded: false, output: undefined }
  const FAILED = { ...READ, isErrored: true, output: 'File does not exist.' }

  test('the collapsed row links its target, drawn as before', async ($, on) => {
    host(on, { mod: json(MIAMI_NIGHTS), env: KITTY })
    await $.session.start(INTERACTIVE)
    const ui = await $.ui.mount(at('ToolUse', READ))
    const tree = await ui.drawn()
    expect(hrefs(tree)).toEqual(['file:///work/proj/src/a.ts'])
    const link = walk(tree).find(n => n.type === 'Link')
    expect(link?.children).toEqual(['src/a.ts'])
    await ui.unmount()
  })

  test('an opened row links its target and its path field', async ($, on) => {
    host(on, { mod: json(MIAMI_NIGHTS), env: KITTY })
    await $.session.start(INTERACTIVE)
    const ui = await $.ui.mount(at('ToolUse', FAILED))
    expect(hrefs(await ui.drawn())).toEqual(['file:///work/proj/src/a.ts', 'file:///work/proj/src/a.ts'])
    await ui.unmount()
  })

  test('a row where links are off links nothing', async ($, on) => {
    host(on, { mod: json(MIAMI_NIGHTS), env: { ...KITTY, TMUX: '/tmp/tmux-1000/default,1,0' } })
    await $.session.start(INTERACTIVE)
    const ui = await $.ui.mount(at('ToolUse', FAILED))
    const tree = await ui.drawn()
    expect(hrefs(tree)).toEqual([])
    expect(JSON.stringify(tree)).toContain('  src/a.ts')
    await ui.unmount()
  })

  for (const surface of ['desktop', 'vscode', 'mobile'] as const) {
    test(`on ${surface} a row gets no file link: file:// names the terminal's machine`, async ($, on) => {
      host(on, { mod: json(MIAMI_NIGHTS), env: KITTY })
      await $.session.start({ ...INTERACTIVE, surface })
      const ui = await $.ui.mount(at('ToolUse', FAILED, surface))
      expect(hrefs(await ui.drawn()).filter(href => href.startsWith('file:'))).toEqual([])
      await ui.unmount()
    })
  }
})
