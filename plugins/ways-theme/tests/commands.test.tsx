import { describe, expect, test } from 'claude-code/testing'
import type { TestBody } from 'claude-code/testing'
import type { On, RenderElement } from 'claude-code'

import { copyPieces, fenceCopy, holdsCopy } from '../hooks/skins/commands.ts'
import type { Copy } from '../hooks/skins/commands.ts'
import { parseExport, setupOf } from '../hooks/skins/config.ts'
import { MAX_DRAWN } from '../hooks/skins/detail.ts'
import { splitReply, tableArt, tableCsv } from '../hooks/skins/markdown.ts'
import type { Segment, Table } from '../hooks/skins/markdown.ts'
import { inlineMath } from '../hooks/skins/math.ts'
import { replySegments } from '../hooks/skins/reply.ts'
import { codeSvg } from '../hooks/skins/svg-code.ts'
import { diffSvg } from '../hooks/skins/svg-diff.ts'
import { MAX_SVG, measure } from '../hooks/skins/svg-kit.ts'
import { terminalSvg } from '../hooks/skins/svg-terminal.ts'
import { MIAMI_NIGHTS } from './fixtures.ts'
import { INTERACTIVE, borders, host, json, shows } from './host.tsx'
import type { Node, World } from './host.tsx'

const PREFS = setupOf(MIAMI_NIGHTS as never).prefs as never
const at = (text: string, surface = 'terminal', columns = 80, isFullscreen = false) =>
  ({ plugin: 'ways-theme', surface, component: 'AssistantMessage', requestId: 'r1', viewport: { columns, rows: 40, isFullscreen }, props: { text, isFirstOfReply: true } }) as never

/** The command from the operator's screenshot, wider than any line it was drawn on. */
const COMMAND = '! git worktree remove ../vtium-glyph-fix36 && git worktree remove ../vtium-glyph-s4 && git worktree remove ../vtium-heads-adr && git branch -d feat/glyph-fix36 feat/glyph-s4 docs/heads-adr && git push origin --delete docs/heads-adr feat/deferred'
const SCREENSHOT = `You can run it yourself:\n\n${COMMAND}`

/** Whether a segment is a suggestion, shell or slash. */
const isSuggestion = (segment: Segment) => segment.kind === 'copy' && (segment.form === 'shell' || segment.form === 'slash')
const commands = (text: string) => copyPieces(text).flatMap(piece => (isSuggestion(piece) ? [(piece as Copy).text] : []))
const prose = (text: string) => copyPieces(text).flatMap(piece => (piece.kind === 'text' ? [piece.text] : []))

describe('which suggestions match', () => {
  test('a line of its own, after a lead-in paragraph', () => {
    expect(copyPieces(SCREENSHOT)).toEqual([
      { kind: 'text', text: 'You can run it yourself:' },
      { kind: 'copy', form: 'shell', text: COMMAND, raw: COMMAND },
    ])
  })

  test('a line straight after a lead-in sentence on the line before', () => {
    expect(copyPieces('Run this:\n! git status  \nThen look.')).toEqual([
      { kind: 'text', text: 'Run this:' },
      { kind: 'copy', form: 'shell', text: '! git status', raw: '! git status  ' },
      { kind: 'text', text: 'Then look.' },
    ])
  })

  test('a list item, its marker dropped', () => {
    expect(commands('- ! git status\n* ! git diff\n2. ! make test\n  3) ! make lint')).toEqual(['! git status', '! git diff', '! make test', '! make lint'])
  })

  test('a line that ends in a continuation takes the lines after it, to the first that does not continue', () => {
    expect(copyPieces('! git fetch &&\n  git rebase origin/main |\n  tee log \\\n  more\nnext')).toEqual([
      { kind: 'copy', form: 'shell', text: '! git fetch &&\n  git rebase origin/main |\n  tee log \\\n  more', raw: '! git fetch &&\n  git rebase origin/main |\n  tee log \\\n  more' },
      { kind: 'text', text: 'next' },
    ])
    expect(commands('! git fetch &&\n\nnext')).toEqual(['! git fetch &&'])
  })

  test('a `|` continuation stops at a table row', () => {
    expect(copyPieces('! git log --oneline |\n| a | b |\n|---|---|')).toEqual([
      { kind: 'copy', form: 'shell', text: '! git log --oneline |', raw: '! git log --oneline |' },
      { kind: 'text', text: '| a | b |\n|---|---|' },
    ])
  })

  test('a command word may start with a digit, a dot, a slash, a tilde, a dollar, a bracket or a quote', () => {
    const words = ['7z x a.7z', './build.sh', '/usr/bin/env true', '~/bin/sync', '$EDITOR notes', '(cd x && make)', '"$HOME/run"', "'quoted'"]
    expect(commands(words.map(word => `! ${word}`).join('\n'))).toEqual(words.map(word => `! ${word}`))
  })

  test('a command that opens with an environment assignment or a test bracket, as a line and as a span', () => {
    const lines = ['! CLAUDE_CODE_TYPES=/x make test-plugins', '! VAR=1 cmd', '! RUST_LOG=debug cargo run', '! [ -f x ] && echo y']
    expect(commands(lines.join('\n'))).toEqual(lines)
    expect(commands('Run `! VAR=1 cmd` or `! [ -f x ] && echo y`.')).toEqual(['! VAR=1 cmd', '! [ -f x ] && echo y'])
  })

  test('a paragraph indented four columns inside a list item is the item\'s, and a command', () => {
    expect(commands('1. Build it:\n\n    ! make dev-install\n\n2. Next')).toEqual(['! make dev-install'])
    expect(commands('- Build it:\n\n    ! make dev-install')).toEqual(['! make dev-install'])
    expect(prose('1. Build it:\n\n    ! make dev-install\n\n2. Next')).toEqual(['1. Build it:', '2. Next'])
  })

  test('a `<!--` inside a code span opens no comment', () => {
    expect(commands('Write `<!--` to open a comment.\n\n! git status')).toEqual(['! git status'])
  })

  test('a line after prose with no blank line between keeps its four-space indent and is a command', () => {
    expect(commands('- item\n    ! git status')).toEqual(['! git status'])
  })

  test('an inline code span: the prose before it, the command, the prose after it', () => {
    expect(copyPieces('Run `! git status` to see what moved.')).toEqual([
      { kind: 'text', text: 'Run' },
      { kind: 'copy', form: 'shell', text: '! git status', raw: '`! git status`' },
      { kind: 'text', text: 'to see what moved.' },
    ])
  })

  test('a span with CommonMark padding, a double-backtick span, and punctuation after a span', () => {
    expect(commands('Try `` ! echo `date` `` or ` ! ls `.')).toEqual(['! echo `date`', '! ls'])
    expect(prose('Try `` ! echo `date` `` or ` ! ls `.')).toEqual(['Try', 'or'])
  })

  test('a list item that is only a span keeps no empty bullet', () => {
    expect(copyPieces('- `! git status`')).toEqual([{ kind: 'copy', form: 'shell', text: '! git status', raw: '`! git status`' }])
  })

  test('a fence of one command, with no language or a shell\'s', () => {
    for (const fence of ['```\n! git push\n```', '```bash\n! git push\n```', '~~~sh\n! git push\n~~~', '```console\n  ! git push\n\n```']) {
      expect(commands(`Then:\n\n${fence}\n\nDone.`)).toEqual(['! git push'])
    }
  })

  test('a fence of one logical command keeps its lines as written', () => {
    expect(commands('```bash\n! git fetch && \\\n  git rebase\n```')).toEqual(['! git fetch && \\\n  git rebase'])
  })

  test('several suggestions in one reply, each its own', () => {
    const text = ['Three steps:', '', COMMAND, '', 'then `! git status`, and last:', '', '```', '! git log -1', '```'].join('\n')
    expect(commands(text)).toEqual([COMMAND, '! git status', '! git log -1'])
    expect(prose(text)).toEqual(['Three steps:', 'then', ', and last:'])
  })

  test('a reply on the terminal carries them as command segments, fences split out as cards included', () => {
    const text = `Lead:\n\n${COMMAND}\n\n\`\`\`\n! git status\n\`\`\`\n\n| a | b |\n|---|---|\n| 1 | 2 |`
    const kinds = replySegments(text, PREFS, 'terminal')?.map(segment => segment.kind)
    expect(kinds).toEqual(['text', 'copy', 'copy', 'table'])
  })

  test('off the terminal a reply keeps no command segment', () => {
    expect(replySegments(SCREENSHOT, PREFS, 'desktop')?.some(isSuggestion) ?? false).toBe(false)
  })
})

describe('what does not match', () => {
  const asProse: [string, string][] = [
    ['a `!` ending a sentence', "That's it! Now run the tests."],
    ['a line starting with `!=`', 'a\n!= b means not equal'],
    ['`!=` inside a span', 'use `a != b` here'],
    ['`!!` and `!important`', '!! twice\n!important'],
    ['an image', '![diagram](docs/flow.png)'],
    ['`!` with no space after it', '!git status'],
    ['`!` and a space with nothing after it', '! '],
    ['emphasis around it', '**! git status**'],
    ['a line in a quote', '> ! git status'],
    ['a span that does not start with `! `', 'run `git status !`'],
    ['a line inside a fence of another language', '```python\n! pip install x\n```'],
    ['a fence of two commands', '```\n! git add .\n! git commit\n```'],
    ['a fence whose command does not start the code', '```\ncd x\n! make\n```'],
    ['a fence still streaming', '```\n! git push'],
    ['a line after a fence still streaming', '```\ncode\n! git push'],
    ['a span in a quote', '> run `! git status` first'],
    ['a span in link text', 'see [`! ls`](https://example.com) and [run `! ls` here][ref]'],
    ['a span in a table row', '| step | `! git status` |'],
    ['a line inside an HTML comment', 'note\n<!-- for later\n! git push\n-->'],
    ['a span on a line with a whole HTML comment', 'run `! ls` <!-- later -->'],
    ['an indented code block after a blank line', 'Example:\n\n    ! git status\n\n    ! git diff'],
    ['an indented code block at the start', '\t! git status'],
    ['an indented code block inside a list item, four columns past its content', '1. Build it:\n\n       ! make dev-install'],
    ['an indented code block after a list has closed', '- a\n\npara\n\n    ! git status'],
    ['prose that starts with `! ` and a capital', '! This is not a command'],
    ['`! =`', '! = y'],
    ["a TeX log's error line", '! Undefined control sequence.\nl.12 \\foo'],
    ['a fence holding a TeX error line', '```\n! Undefined control sequence.\n```'],
  ]
  for (const [name, text] of asProse) {
    test(`${name} stays prose`, () => {
      const isFenced = /^[ \t]*(?:`{3,}|~{3,})/m.test(text)
      expect(copyPieces(text).every(piece => piece.kind === 'text' || (isFenced && piece.form === 'code'))).toBe(true)
    })
  }

  test('the quick test passes prose by', () => {
    expect(holdsCopy("That's it! Done.\na != b\n![x](y)")).toBe(false)
    expect(holdsCopy(SCREENSHOT)).toBe(true)
    expect(holdsCopy('see `! ls`')).toBe(true)
  })

  test('a fence of another language is its code, not a command', () => {
    expect(fenceCopy('python', '! pip install x', '')).toEqual({ kind: 'copy', form: 'code', lang: 'python', text: '! pip install x', raw: '' })
    expect(fenceCopy('', '! git push', 'raw')).toEqual({ kind: 'copy', form: 'shell', text: '! git push', raw: 'raw' })
  })
})

describe('a fence closes only on a line of its mark, at least as long', () => {
  const nested: [string, string, string][] = [
    ['a ```` fence holding ``` lines', '````md\nRun:\n```\n! git status\n```\n````', 'md'],
    ['a ~~~ fence holding ``` lines', '~~~\nnote\n```\n! git push\n```\n~~~', ''],
  ]
  for (const [name, text, lang] of nested) {
    test(`${name} is one code segment, with no command in it`, () => {
      const segments = splitReply(text)
      expect(segments.length).toBe(1)
      expect(segments[0]).toMatchObject({ kind: 'code', lang, raw: text })
      expect(replySegments(text, PREFS, 'terminal')?.some(isSuggestion) ?? false).toBe(false)
      expect(copyPieces(text).map(piece => piece.kind === 'copy' && piece.form)).toEqual(['code'])
    })
  }

  test('a nested fence still streaming keeps a table inside it as text', () => {
    const text = 'Here:\n\n````md\n```\n| a | b |\n|---|---|\n| 1 | 2 |'
    expect(splitReply(text)).toEqual([{ kind: 'text', text }])
  })

  test('inline math is left alone inside a fence a line of another mark does not close', () => {
    const drawn = inlineMath('~~~\n```\nlet $x^2$ be\n~~~\nafter $y^2$').split('\n')
    expect(drawn.slice(0, 4)).toEqual(['~~~', '```', 'let $x^2$ be', '~~~'])
    expect(drawn[4]).not.toContain('$')
    expect(drawn[4]!.startsWith('after ')).toBe(true)
  })

  test('a closing line shorter than the opening one does not close it', () => {
    expect(splitReply('````\n! git push\n```').every(segment => segment.kind === 'text')).toBe(true)
  })
})

const strings = (node: unknown): string => {
  if (typeof node === 'string') return node
  if (node === null || typeof node !== 'object') return ''
  return ((node as Node).children ?? []).map(strings).join('')
}

/** Each Text that draws exactly `text`, with the elements around it, outermost first. */
function drawnAt(tree: RenderElement, text: string): Node[][] {
  const out: Node[][] = []
  const visit = (node: unknown, above: Node[]) => {
    if (node === null || typeof node !== 'object') return
    const n = node as Node
    if (n.type === 'Text' && strings(n) === text) out.push(above)
    for (const child of n.children ?? []) visit(child, n.type === undefined ? above : [...above, n])
  }
  visit(tree, [])
  return out
}

/** Every node under `tree` that `test` holds for. */
function nodes(tree: unknown, test: (n: Node) => boolean): Node[] {
  if (tree === null || typeof tree !== 'object') return []
  const n = tree as Node
  return [...(test(n) ? [n] : []), ...(n.children ?? []).flatMap(child => nodes(child, test))]
}

/** The host, and every text a copy put on the clipboard. */
function hostWithClipboard(on: On, world: World) {
  const h = host(on, world)
  const copies: string[] = []
  on('ui.copy', ($, e) => {
    copies.push(e.text)
    return { value: { isCopied: true } } as never
  })
  return { ...h, copies }
}

async function drawn($: Parameters<TestBody>[0], text: string, surface?: string): Promise<RenderElement> {
  const ui = await $.ui.mount(at(text, surface))
  const tree = await ui.drawn()
  await ui.unmount()
  return tree
}

describe('a command draws as one string outside every frame', () => {
  test('the screenshot: the lead-in in its frame, the command under it in no frame, whole in one Text', async ($, on) => {
    hostWithClipboard(on, { mod: json(MIAMI_NIGHTS) })
    await $.session.start(INTERACTIVE)
    const tree = await drawn($, SCREENSHOT)
    expect(borders(tree).length).toBe(1)
    expect(shows(tree, 'You can run it yourself:')).toBe(true)
    const found = drawnAt(tree, COMMAND)
    expect(found.length).toBe(1)
    expect(found[0]!.some(n => n.props?.borderStyle !== undefined)).toBe(false)
    expect(found[0]!.some(n => typeof n.props?.paddingLeft === 'number' || typeof n.props?.paddingX === 'number')).toBe(false)
  })

  test('prose after the command takes a frame of its own', async ($, on) => {
    hostWithClipboard(on, { mod: json(MIAMI_NIGHTS) })
    await $.session.start(INTERACTIVE)
    const tree = await drawn($, `${SCREENSHOT}\n\nThat clears the branches.`)
    expect(borders(tree).length).toBe(2)
    expect(drawnAt(tree, COMMAND)[0]!.some(n => n.props?.borderStyle !== undefined)).toBe(false)
  })

  test('Copy puts the command on the clipboard exactly, with its `! `, no line break and no frame glyph', async ($, on) => {
    const h = hostWithClipboard(on, { mod: json(MIAMI_NIGHTS) })
    await $.session.start(INTERACTIVE)
    const ui = await $.ui.mount(at(SCREENSHOT))
    const button = await ui.find({ type: 'Button', text: /^Copy$/ })
    expect(button?.key).toBeDefined()
    await ui.press({ key: button!.key! })
    await ui.unmount()
    expect(h.copies).toEqual([COMMAND])
    expect(h.toasts).toContainEqual(expect.objectContaining({ text: 'Copied' }))
  })

  test('each of several commands copies its own text', async ($, on) => {
    const h = hostWithClipboard(on, { mod: json(MIAMI_NIGHTS) })
    await $.session.start(INTERACTIVE)
    const ui = await $.ui.mount(at('One:\n\n! git status\n\nand `! git diff --stat`.'))
    for (const button of await ui.findAll({ type: 'Button', text: /^Copy$/ })) await ui.press({ key: button.key! })
    await ui.unmount()
    expect(h.copies).toEqual(['! git status', '! git diff --stat'])
  })

  test('with copy off the command still draws whole outside the frame, with no control', async ($, on) => {
    hostWithClipboard(on, { mod: json({ ...MIAMI_NIGHTS, render: { ...MIAMI_NIGHTS.render, copy: false } }) })
    await $.session.start(INTERACTIVE)
    const ui = await $.ui.mount(at(SCREENSHOT))
    expect(await ui.findAll({ type: 'Button' })).toEqual([])
    const tree = await ui.drawn()
    await ui.unmount()
    expect(drawnAt(tree, COMMAND)[0]!.some(n => n.props?.borderStyle !== undefined)).toBe(false)
  })

  test('with frames off the command still draws as its own block', async ($, on) => {
    hostWithClipboard(on, { mod: json({ ...MIAMI_NIGHTS, render: { ...MIAMI_NIGHTS.render, frames: false } }) })
    await $.session.start(INTERACTIVE)
    const tree = await drawn($, SCREENSHOT)
    expect(borders(tree).length).toBe(0)
    expect(drawnAt(tree, COMMAND).length).toBe(1)
  })

  test('under another Claude Code theme than custom:ways no frame draws, and the command is its own block', async ($, on) => {
    hostWithClipboard(on, { mod: json(MIAMI_NIGHTS), selected: 'dark' })
    await $.session.start(INTERACTIVE)
    const tree = await drawn($, SCREENSHOT)
    expect(borders(tree).length).toBe(0)
    expect(drawnAt(tree, COMMAND).length).toBe(1)
  })

  test('with no ways look loaded the engine draws the reply', async ($, on) => {
    hostWithClipboard(on, {})
    await $.session.start(INTERACTIVE)
    const tree = await drawn($, SCREENSHOT)
    expect(shows(tree, 'engine')).toBe(true)
    expect(drawnAt(tree, COMMAND)).toEqual([])
  })

  test('a reply with no command is framed whole, as before', async ($, on) => {
    hostWithClipboard(on, { mod: json(MIAMI_NIGHTS) })
    await $.session.start(INTERACTIVE)
    expect(borders(await drawn($, "That's it! The worktrees are gone.")).length).toBe(1)
  })
})

/** The slash command from the operator's second screenshot, the last line of a reply. */
const SLASH = '/compact keep the continuation prompt in .claude/handoff.wrap.md and the task list as it stands; drop the exploratory back-and-forth'
const SLASH_REPLY = `The handoff is written. When you are ready:\n\n\`${SLASH}\``
const slashes = (text: string) => copyPieces(text).flatMap(piece => (piece.kind === 'copy' && piece.form === 'slash' ? [piece.text] : []))

describe('slash commands', () => {
  test('an inline span: the prose before it, the command, the prose after it', () => {
    expect(copyPieces('Run `/compact keep the handoff` now.')).toEqual([
      { kind: 'text', text: 'Run' },
      { kind: 'copy', form: 'slash', text: '/compact keep the handoff', raw: '`/compact keep the handoff`' },
      { kind: 'text', text: 'now.' },
    ])
  })

  test('a bare name on a line of its own or as a list item, and a fence of one line with no language', () => {
    expect(copyPieces('Then:\n/compact  \nDone.')).toEqual([
      { kind: 'text', text: 'Then:' },
      { kind: 'copy', form: 'slash', text: '/compact', raw: '/compact  ' },
      { kind: 'text', text: 'Done.' },
    ])
    expect(slashes('- /review\n2. /mcp')).toEqual(['/review', '/mcp'])
    expect(copyPieces('```\n/compact keep it\n```')).toEqual([{ kind: 'copy', form: 'slash', text: '/compact keep it', raw: '```\n/compact keep it\n```' }])
  })

  test('a name may hold digits, `:` and `-`, and a bare name ends the span', () => {
    expect(slashes('`/plugin:skill-2 go` and `/clear`.')).toEqual(['/plugin:skill-2 go', '/clear'])
  })

  test('arguments count in a span or a fence, never on a line of their own', () => {
    expect(slashes('`/compact keep it`\n\n```\n/review 12\n```')).toEqual(['/compact keep it', '/review 12'])
    expect(slashes('/compact keep it\n- /review 12')).toEqual([])
  })

  test('the screenshot reply: the lead-in stays prose and the span is the command, whole', () => {
    expect(copyPieces(SLASH_REPLY, 76)).toEqual([
      { kind: 'text', text: 'The handoff is written. When you are ready:' },
      { kind: 'copy', form: 'slash', text: SLASH, raw: `\`${SLASH}\`` },
    ])
  })

  test('the quick test sees a slash line, a slash span and a fence', () => {
    expect(holdsCopy('/compact')).toBe(true)
    expect(holdsCopy('see `/help`')).toBe(true)
    expect(holdsCopy('```rust\nfn x() {}\n```')).toBe(true)
  })

  const asProse: [string, string][] = [
    ['a path on a line of its own', '/home/aaron/Projects/x'],
    ['a path in a span', 'look in `/tmp/x` first'],
    ['a path that starts a line of prose', '/etc/hosts maps the names'],
    ['`a/b` in prose', 'the a/b split and and/or'],
    ['a `/` inside a sentence', 'use the /compact command when the context is full'],
    ['a URL', 'https://example.com/compact x\n`https://example.com/clear`'],
    ['a line comment', '// compact this'],
    ['a capital after the slash', '/Compact keep it'],
    ['a digit after the slash', '/9 x'],
    ['a slash with a space after it', '/ compact'],
    ['a slash command in a quote', '> /compact keep it'],
    ['a slash command in a table row', '| step | `/compact` |'],
    ['a slash command in link text', 'see [`/compact`](https://example.com)'],
    ['a slash command in an HTML comment', '<!--\n/compact\n-->'],
    ['a slash command in an indented code block', 'Example:\n\n    /compact keep it'],
    ['a fence still streaming', '```\n/compact keep it'],
    ['a list item that names a command and explains it', '- /compact — summarize the context'],
    ['a list item that names a command before a colon', '- /clear: wipe it'],
    ['a sentence that opens on two directories', '/dev and /proc are virtual filesystems'],
    ['a sentence that opens on a command name', '/compact is useful when the context fills'],
    ['a numbered item that opens on a directory', '1. /opt holds it'],
    ['a command and its arguments on a line of their own', '/compact keep the handoff'],
  ]
  for (const [name, text] of asProse) {
    test(`${name} stays prose`, () => {
      expect(copyPieces(text, 76).every(piece => piece.kind === 'text')).toBe(true)
    })
  }

  const ROOTS = ['bin', 'boot', 'dev', 'etc', 'home', 'lib', 'lib64', 'media', 'mnt', 'nix', 'opt', 'private', 'proc', 'root', 'run', 'sbin', 'srv', 'sys', 'tmp', 'usr', 'var']
  for (const root of ROOTS) {
    test(`/${root} is a directory while the command list is unread, in a span, on a line of its own and in a fence`, () => {
      for (const text of [`look in \`/${root}\` first`, `/${root}`, `- /${root}`, `\`/${root} x\``]) {
        expect(copyPieces(text, 76).every(piece => piece.kind === 'text')).toBe(true)
      }
      expect(copyPieces(`\`\`\`\n/${root}\n\`\`\``).map(piece => piece.kind === 'copy' && piece.form)).toEqual(['code'])
    })
  }

  test("a fence with a language holding a slash line is that language's code", () => {
    expect(copyPieces('```text\n/compact keep it\n```').map(piece => piece.kind === 'copy' && piece.form)).toEqual(['code'])
  })

  // `/compact` and `/review` are commands; `/health` and `/users` are routes.
  const LISTED = new Set(['compact', 'review'])
  const listed = (name: string) => LISTED.has(name)
  const listedSlashes = (text: string) => copyPieces(text, 80, undefined, listed).flatMap(piece => (piece.kind === 'copy' && piece.form === 'slash' ? [piece.text] : []))

  test('with the command list read, a name it lists is a command', () => {
    expect(listedSlashes('Run `/review` on it.\n\n/compact\n\n```\n/compact keep it\n```')).toEqual(['/review', '/compact', '/compact keep it'])
  })

  for (const text of ['The handler for `/health` returns 200.', 'A GET on `/users` lists them.', '- `/health`: liveness probe', '/health', '- /users', '```\n/health\n```']) {
    test(`with the command list read, a route it does not list stays where it is: ${JSON.stringify(text)}`, () => {
      expect(copyPieces(text, 80, undefined, listed).some(piece => piece.kind === 'copy' && piece.form === 'slash')).toBe(false)
      expect(holdsCopy(text, 80, listed) && copyPieces(text, 80, undefined, listed).some(piece => piece.kind === 'copy' && piece.form !== 'code')).toBe(false)
    })
  }

  test('a reply drawn after the list is read lifts only the commands it lists', async ($, on) => {
    const h = hostWithClipboard(on, { mod: json(MIAMI_NIGHTS), commands: ['compact', 'review'] })
    await $.session.start(INTERACTIVE)
    const ui = await $.ui.mount(at('The handler for `/health` returns 200. Run `/review` on it.'))
    for (const button of await ui.findAll({ type: 'Button', text: /^\s*Copy\s*$/ })) await ui.press({ key: button.key! })
    const tree = await ui.drawn()
    await ui.unmount()
    expect(h.copies).toEqual(['/review'])
    expect(shows(tree, '/health')).toBe(true)
  })

  test('where the list cannot be read, a name that is no directory counts', async ($, on) => {
    const h = hostWithClipboard(on, { mod: json(MIAMI_NIGHTS), commands: null })
    await $.session.start(INTERACTIVE)
    const ui = await $.ui.mount(at('Run `/review` on it, not `/nix`.'))
    for (const button of await ui.findAll({ type: 'Button', text: /^\s*Copy\s*$/ })) await ui.press({ key: button.key! })
    await ui.unmount()
    expect(h.copies).toEqual(['/review'])
  })
})

/** A fence's code with what a copy must keep: indentation, a tab, trailing spaces, blank lines. */
const CODE = 'fn main() {\n    let x = 1;   \n\n\tprintln!("{x}");\n\n}'
const FENCED = `Here it is:\n\n\`\`\`rust\n${CODE}\n\`\`\`\n\nThat prints 1.`

describe('a CRLF line ending reads as LF', () => {
  for (const lf of ['a\n/clear\nb', '! ls -la\n', '```python\ndef f():\n    return 1\n```', '| a | b |\n|---|---|\n| `x` | y |\n']) {
    test(JSON.stringify(lf), () => {
      const crlf = lf.replace(/\n/g, '\r\n')
      expect(copyPieces(crlf, 80)).toEqual(copyPieces(lf, 80))
      expect(splitReply(crlf)).toEqual(splitReply(lf))
    })
  }

  test('a CRLF fence copies its code with no carriage return', () => {
    expect(copyPieces('```python\r\ndef f():\r\n    return 1\r\n```')).toEqual([{ kind: 'copy', form: 'code', lang: 'python', text: 'def f():\n    return 1', raw: '```python\ndef f():\n    return 1\n```' }])
    expect(commands('a\r\n/clear\r\nb')).toEqual(['/clear'])
    expect(commands('! ls -la\r\n')).toEqual(['! ls -la'])
  })
})

describe('a fenced block is a copy block', () => {
  test('its code as written, its language kept', () => {
    const copy = { kind: 'copy', form: 'code', lang: 'rust', text: CODE, raw: `\`\`\`rust\n${CODE}\n\`\`\`` }
    expect(copyPieces(FENCED)).toEqual([{ kind: 'text', text: 'Here it is:' }, copy, { kind: 'text', text: 'That prints 1.' }])
    expect(replySegments(FENCED, PREFS, 'terminal')?.find(segment => segment.kind === 'copy')).toEqual(copy)
  })

  test('a fence with no code, a Mermaid chart and a diff keep their drawing', () => {
    expect(copyPieces('```\n\n```').every(piece => piece.kind === 'text')).toBe(true)
    const kinds = (text: string) => replySegments(text, PREFS, 'terminal')?.map(segment => segment.kind)
    expect(kinds('```mermaid\nflowchart LR\n  A --> B\n```')).toEqual(['code'])
    expect(kinds('```diff\n--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+b\n```')).toEqual(['code'])
  })

  test('off the terminal a fence stays a code card', () => {
    expect(replySegments(FENCED, PREFS, 'desktop')?.some(segment => segment.kind === 'copy') ?? false).toBe(false)
  })
})

/** A span no 80-column frame holds on one line. */
const LONG = 'git worktree remove ../vtium-glyph-fix36 && git branch -d feat/glyph-fix36 feat/glyph-s4 docs/heads-adr'
/** A row wide enough that LONG stays in its sentence. */
const ROOMY = Math.ceil(LONG.length / 0.6)

describe('a code span wider than its row', () => {
  test('leaves its sentence as a span block; a narrower one stays', () => {
    const text = `Then \`${LONG}\` clears them, and \`git status\` shows it.`
    expect(copyPieces(text, 76)).toEqual([
      { kind: 'text', text: 'Then' },
      { kind: 'copy', form: 'span', text: LONG, raw: `\`${LONG}\`` },
      { kind: 'text', text: 'clears them, and `git status` shows it.' },
    ])
    expect(copyPieces(text, ROOMY).every(piece => piece.kind === 'text')).toBe(true)
    expect(copyPieces(text).every(piece => piece.kind === 'text')).toBe(true)
    expect(holdsCopy(text, 76)).toBe(true)
    expect(holdsCopy(text, ROOMY)).toBe(false)
  })

  test('a span wider than 60% of its row leaves it, so one that starts late in its line does not wrap', () => {
    const late = (span: string) => `${'word '.repeat(10)}\`${span}\` ends it.`
    expect(copyPieces(late('x'.repeat(50)), 76).map(piece => piece.kind)).toEqual(['text', 'copy', 'text'])
    expect(copyPieces(late('x'.repeat(45)), 76).every(piece => piece.kind === 'text')).toBe(true)
    expect(holdsCopy(late('x'.repeat(50)), 76)).toBe(true)
    expect(holdsCopy(late('x'.repeat(45)), 76)).toBe(false)
  })

  test("an open list item's indentation comes off the row", () => {
    const span = 'x'.repeat(60)
    const spans = (text: string) => copyPieces(text, 100).filter(piece => piece.kind === 'copy').map(piece => (piece as Copy).text)
    expect(spans(`Then \`${span}\` clears them.`)).toEqual([])
    expect(spans(`- Then \`${span}\` clears them.`)).toEqual([span])
    expect(spans(`10. First:\n\n    then \`${span}\` clears them.`)).toEqual([span])
    expect(holdsCopy(`- Then \`${span}\` clears them.`, 100)).toBe(true)
    expect(holdsCopy(`Then \`${span}\` clears them.`, 100)).toBe(false)
  })
})

describe('a fence in a list item', () => {
  const PYTHON = '1. Do this:\n   ```python\n   def total(xs):\n       for x in xs:\n           yield x\n\n   ```'
  const WRITTEN = 'def total(xs):\n    for x in xs:\n        yield x'

  test('copies and draws without the list\'s indentation, its own kept', () => {
    expect(copyPieces('1. Do this:\n   ```sh\n   ls -la\n   ```').find(piece => piece.kind === 'copy')).toMatchObject({ form: 'code', text: 'ls -la' })
    expect(copyPieces(PYTHON).find(piece => piece.kind === 'copy')).toMatchObject({ form: 'code', lang: 'python', text: WRITTEN })
    expect(replySegments(PYTHON, PREFS, 'terminal')?.find(segment => segment.kind === 'copy')).toMatchObject({ text: WRITTEN })
    expect(splitReply(PYTHON).find(segment => segment.kind === 'code')).toMatchObject({ code: `${WRITTEN}\n` })
  })

  test('a line indented less than the fence loses only what it has', () => {
    expect(copyPieces('   ```\n   a\n b\n```').find(piece => piece.kind === 'copy')).toMatchObject({ text: 'a\nb' })
  })

  test('Copy takes the code as written, and each line draws at its own indentation', async ($, on) => {
    const h = hostWithClipboard(on, { mod: json(MIAMI_NIGHTS) })
    await $.session.start(INTERACTIVE)
    const ui = await $.ui.mount(at(PYTHON))
    const tree = await ui.drawn()
    expect(drawnAt(tree, '    for x in xs:').length).toBeGreaterThan(0)
    expect(drawnAt(tree, '       for x in xs:')).toEqual([])
    await ui.press({ key: (await ui.find({ type: 'Button', text: /^\s*Copy\s*$/ }))!.key! })
    await ui.unmount()
    expect(h.copies).toEqual([WRITTEN])
  })
})

describe('a fence copy block', () => {
  test('keeps no trailing blank line, in the copy or the drawing', () => {
    expect(fenceCopy('rust', 'fn x() {}\n\n  \n', 'raw')).toEqual({ kind: 'copy', form: 'code', lang: 'rust', text: 'fn x() {}', raw: 'raw' })
  })

  test('with no language and no Copy, draws no line above its code', async ($, on) => {
    hostWithClipboard(on, { mod: json({ ...MIAMI_NIGHTS, render: { ...MIAMI_NIGHTS.render, copy: false } }) })
    await $.session.start(INTERACTIVE)
    const tree = await drawn($, 'Here:\n\n```\nmake dev-install\n```')
    expect(nodes(tree, n => n.props?.justifyContent === 'space-between')).toEqual([])
    expect(drawnAt(tree, 'make dev-install').length).toBeGreaterThan(0)
  })

  test("a diff fence's long line wraps under its code, its numbers and sign in a gutter of their own", async ($, on) => {
    hostWithClipboard(on, { mod: json(MIAMI_NIGHTS) })
    await $.session.start(INTERACTIVE)
    const long = `const value = ${'compute(1, 2) + '.repeat(8)}0`
    const tree = await drawn($, `See:\n\n\`\`\`diff\n--- a/x.ts\n+++ b/x.ts\n@@ -1 +1 @@\n-const value = 0\n+${long}\n\`\`\``)
    const [above] = drawnAt(tree, long)
    expect(above).toBeDefined()
    const boxes = above!.filter(n => n.type === 'Box')
    const code = boxes.at(-1)!
    const row = boxes.at(-2)!
    expect(code.props?.flexShrink).toBe(1)
    expect(row.props?.flexDirection).toBe('row')
    const gutter = (row.children ?? []).flat(Infinity as 1)[0] as Node
    expect(gutter.props?.flexShrink).toBe(0)
    expect(strings(gutter)).toBe('  1 + ')
  })

  test('with tables off, a diff and a Mermaid chart keep their drawing', () => {
    const prefs = { ...(PREFS as object), tables: 'off' } as never
    const diff = 'See:\n\n```diff\n--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+b\n```'
    const chart = 'See:\n\n```mermaid\nnot a chart\n```'
    expect(replySegments(diff, prefs, 'terminal')?.some(segment => segment.kind === 'copy') ?? false).toBe(false)
    expect(replySegments(chart, prefs, 'terminal')?.some(segment => segment.kind === 'copy') ?? false).toBe(false)
    expect(replySegments('See:\n\n```rust\nfn x() {}\n```', prefs, 'terminal')?.some(segment => segment.kind === 'copy')).toBe(true)
  })
})

/** Whether a drawn Text sits clear of every frame: no border, padding or ground around it. */
const isClear = (above: Node[]) =>
  !above.some(n => n.props?.borderStyle !== undefined || ['paddingX', 'paddingLeft', 'paddingRight'].some(pad => typeof n.props?.[pad] === 'number') || n.props?.backgroundColor !== undefined)

/** What a selection or a copy must never carry: a frame glyph, a padding space at a line's end, a line break. */
const isClean = (text: string) => !/[│╭╮╰╯─]/.test(text) && !/ $/m.test(text) && !text.includes('\n')

describe('a copy block draws clear of every frame and copies its text alone', () => {
  test('the screenshot at 80 columns: the slash command outside the frame, copied whole', async ($, on) => {
    const h = hostWithClipboard(on, { mod: json(MIAMI_NIGHTS) })
    await $.session.start(INTERACTIVE)
    const ui = await $.ui.mount(at(SLASH_REPLY))
    const tree = await ui.drawn()
    expect(borders(tree).length).toBe(1)
    const found = drawnAt(tree, SLASH)
    expect(found.length).toBe(1)
    expect(isClear(found[0]!)).toBe(true)
    const button = await ui.find({ type: 'Button', text: /^Copy$/ })
    await ui.press({ key: button!.key! })
    await ui.unmount()
    expect(h.copies).toEqual([SLASH])
  })

  test('a wide span copies what its rows draw: one Text, clear of every frame, with no glyph, padding or break', async ($, on) => {
    const h = hostWithClipboard(on, { mod: json(MIAMI_NIGHTS) })
    await $.session.start(INTERACTIVE)
    const ui = await $.ui.mount(at(`Then \`${LONG}\` clears them, and \`${LONG.replace('worktree', 'branch')}\` too.`))
    const tree = await ui.drawn()
    for (const button of await ui.findAll({ type: 'Button', text: /^Copy$/ })) await ui.press({ key: button.key! })
    await ui.unmount()
    expect(h.copies.length).toBe(2)
    for (const copy of h.copies) {
      // The span's Text and the one coloured run inside it both draw it.
      const found = drawnAt(tree, copy)
      expect(found.length).toBeGreaterThan(0)
      expect(found.every(isClear)).toBe(true)
      expect(isClean(copy)).toBe(true)
    }
  })

  test('a fence: language and Copy above, each line clear of every frame, the code copied exactly', async ($, on) => {
    const h = hostWithClipboard(on, { mod: json(MIAMI_NIGHTS) })
    await $.session.start(INTERACTIVE)
    const ui = await $.ui.mount(at(FENCED))
    const tree = await ui.drawn()
    expect(borders(tree).length).toBe(2)
    expect(drawnAt(tree, 'rust').length).toBe(1)
    for (const line of ['fn main() {', '    let x = 1;   ', '  println!("{x}");', '}']) {
      // The line's Text and its one token's Text both draw it.
      const found = drawnAt(tree, line)
      expect(found.length).toBeGreaterThan(0)
      expect(found.every(isClear)).toBe(true)
    }
    await ui.press({ key: (await ui.find({ type: 'Button', text: /^\s*Copy\s*$/ }))!.key! })
    await ui.unmount()
    expect(h.copies).toEqual([CODE])
  })

  test('a 40-row table, a 60-line fence, a 40-line diff and 200 lines of shell output each draw whole', async ($, on) => {
    const h = hostWithClipboard(on, { mod: json(MIAMI_NIGHTS) })
    await $.session.start(INTERACTIVE)
    const table = ['| n | name |', '|---|---|', ...Array.from({ length: 40 }, (_, n) => `| ${n + 1} | row-${n + 1} |`)].join('\n')
    const code = Array.from({ length: 60 }, (_, n) => `line ${n + 1}`).join('\n')
    const diff = ['--- a/x', '+++ b/x', '@@ -0,0 +1,40 @@', ...Array.from({ length: 40 }, (_, n) => `+added ${n + 1}`)].join('\n')
    const ui = await $.ui.mount(at(`${table}\n\n\`\`\`\n${code}\n\`\`\`\n\n\`\`\`diff\n${diff}\n\`\`\``, 'terminal', 80, true))
    const tree = await ui.drawn()
    expect(shows(tree, 'added 40')).toBe(true)
    expect(drawnAt(tree, 'row-40').length).toBeGreaterThan(0)
    expect(drawnAt(tree, 'line 60').length).toBeGreaterThan(0)
    expect(await ui.find({ type: 'Button', text: /more|less/ })).toBe(undefined)
    for (const button of await ui.findAll({ type: 'Button', text: /^\s*Copy\s*$/ })) await ui.press({ key: button.key! })
    await ui.unmount()
    expect(h.copies).toContain(code)

    const stdout = Array.from({ length: 200 }, (_, n) => `out ${n + 1}`).join('\n')
    const result = { plugin: 'ways-theme', surface: 'terminal', component: 'ToolResult', requestId: 't2', viewport: { columns: 80, rows: 40 }, props: { tool_use_id: 't2', tool: 'Bash', output: { stdout, stderr: '', interrupted: false, isImage: false }, isErrored: false } } as never
    const card = await $.ui.mount(result)
    const drawn = await card.drawn()
    await card.unmount()
    for (const line of ['out 1', 'out 100', 'out 200']) expect(drawnAt(drawn, line).length).toBeGreaterThan(0)
    expect(shows(drawn, 'lines hidden')).toBe(false)
  })

  test('a `!` command still copies as before beside a slash command and a fence', async ($, on) => {
    const h = hostWithClipboard(on, { mod: json(MIAMI_NIGHTS) })
    await $.session.start(INTERACTIVE)
    const ui = await $.ui.mount(at('One:\n\n! git status\n\nthen `/clear`, and:\n\n```sh\nls -la\n```'))
    for (const button of await ui.findAll({ type: 'Button', text: /^\s*Copy\s*$/ })) await ui.press({ key: button.key! })
    await ui.unmount()
    expect(h.copies).toEqual(['! git status', '/clear', 'ls -la'])
  })
})

/** A reply holding one block of each kind `copy_as` decides for. */
const TABLE = ['| name | note |', '|---|---|', '| **a, b** | say "hi" |', '| [link](https://x.example) | *it*<br>two |'].join('\n')
const BLOCKS = ['! git status', '', 'then `/clear`.', '', '```sh\nls -la\n```', '', TABLE, '', `Then \`${LONG}\` clears them.`].join('\n')

/** Every Copy on the reply pressed in order, then `as text` and `copy reply`. */
async function copiesOf($: Parameters<TestBody>[0], on: On, copyAs: 'content' | 'markdown' | undefined) {
  const render = copyAs === undefined ? MIAMI_NIGHTS.render : { ...MIAMI_NIGHTS.render, copy_as: copyAs }
  const h = hostWithClipboard(on, { mod: json({ ...MIAMI_NIGHTS, render }) })
  await $.session.start(INTERACTIVE)
  const ui = await $.ui.mount(at(BLOCKS))
  for (const button of await ui.findAll({ type: 'Button', text: /^\s*Copy\s*$/ })) await ui.press({ key: button.key! })
  const blocks = [...h.copies]
  await ui.press({ key: (await ui.find({ type: 'Button', text: /as text/ }))!.key! })
  await ui.press({ key: (await ui.find({ type: 'Button', text: /copy reply/ }))!.key! })
  await ui.unmount()
  return { blocks, art: h.copies.at(-2), reply: h.copies.at(-1) }
}

describe('copy_as', () => {
  const art = tableArt(splitReply(TABLE)[0] as Table)

  test('content: a command as typed, a fence its code, a table as CSV, a span its text', async ($, on) => {
    const copied = await copiesOf($, on, 'content')
    expect(copied.blocks).toEqual(['! git status', '/clear', 'ls -la', 'name,note\n"a, b","say ""hi"""\nlink,"it\ntwo"', LONG])
    expect(copied.art).toBe(art)
    expect(copied.reply).toBe(BLOCKS)
  })

  test('markdown: a command as typed, a fence, a table and a span as written', async ($, on) => {
    const copied = await copiesOf($, on, 'markdown')
    expect(copied.blocks).toEqual(['! git status', '/clear', '```sh\nls -la\n```', TABLE, `\`${LONG}\``])
    expect(copied.art).toBe(art)
    expect(copied.reply).toBe(BLOCKS)
  })

  test('an export without copy_as copies content', async ($, on) => {
    const copied = await copiesOf($, on, undefined)
    expect(copied.blocks[2]).toBe('ls -la')
    expect(parseExport(JSON.stringify({ ...MIAMI_NIGHTS, render: { ...MIAMI_NIGHTS.render, copy_as: 'html' } }))).toBe('render.copy_as is not content or markdown')
  })

  test('a CSV cell holding a comma, a quote or a line break is quoted, its quotes doubled', () => {
    const table: Table = { kind: 'table', header: ['a', 'b'], align: ['left', 'left'], rows: [['x, y', 'say "hi"'], ['plain', 'one<br/>two'], ['~~gone~~ `kept`', 'snake_case _em_']] }
    expect(tableCsv(table)).toBe('a,b\n"x, y","say ""hi"""\nplain,"one\ntwo"\ngone kept,snake_case em')
  })

  const csvOf = (row: string) => tableCsv(splitReply(`| a | b |\n|---|---|\n${row}`)[0] as Table).split('\n')[1]
  const spans: [string, string, string][] = [
    ['dunders and a double star', '| `__init__` | `**kwargs` |', '__init__,**kwargs'],
    ['underscores', '| `_private_` | x |', '_private_,x'],
    ['tildes', '| `~~x~~` | x |', '~~x~~,x'],
    ['backslashes', '| `C:\\temp\\*` | x |', 'C:\\temp\\*,x'],
    ['a link', '| `[a](b)` | x |', '[a](b),x'],
    ['a line break', '| `a<br>b` | x |', 'a<br>b,x'],
    ['an escaped pipe', '| `a \\| b` | x |', 'a | b,x'],
  ]
  for (const [name, row, csv] of spans) {
    test(`a code span keeps its content verbatim: ${name}`, () => {
      expect(csvOf(row)).toBe(csv)
    })
  }

  test('outside a code span the markdown comes off: bold, a link with parentheses in its address, an autolink', () => {
    expect(csvOf('| **bold** `**kept**` | __b__ |')).toBe('bold **kept**,b')
    expect(csvOf('| [a](https://x.com/(p)) | <https://x.com> |')).toBe('a,https://x.com')
    expect(csvOf('| [`run`](https://x.com) | *it* `*it*` |')).toBe('run,it *it*')
  })

  test("a cell holding the private-use marks keeps them, beside a code span of its own", () => {
    const cell = 'it\uE0000\uE001s \uE0000\uE000'
    const table = splitReply(`| a | b |\n|---|---|\n| ${cell} | \`x\` ${cell} |`)[0] as Table
    expect(table.rows).toEqual([[cell, `x ${cell}`]])
    expect(tableCsv(table).split('\n')[1]).toBe(`${cell},x ${cell}`)
  })

  test('the drawn cell keeps a code span as written and drops bold around it', () => {
    const table = splitReply('| a | b |\n|---|---|\n| `**kwargs` | **bold** `__init__` |')[0] as Table
    expect(table.rows).toEqual([['**kwargs', 'bold __init__']])
  })
})

describe('nothing folds and nothing is cut', () => {
  const PALETTE = setupOf(MIAMI_NIGHTS as never).skin.palette
  const lines = (n: number, line = (k: number) => `line ${k}`) => Array.from({ length: n }, (_, k) => line(k + 1)).join('\n')

  test('an opened answer of 120 lines draws every line, wrapped, with no `show all`', async ($, on) => {
    hostWithClipboard(on, { mod: json(MIAMI_NIGHTS) })
    await $.session.start(INTERACTIVE)
    const failed = { tool_use_id: 't1', tool: 'Read', input: { file_path: '/work/proj/a.ts' }, isRunning: false, isErrored: true, isInterrupted: false, isExpanded: false, output: lines(120, k => `error ${k} ${'x'.repeat(k === 120 ? 200 : 0)}`.trimEnd()) }
    const ui = await $.ui.mount({ plugin: 'ways-theme', surface: 'terminal', component: 'ToolUse', requestId: 'r1', viewport: { columns: 80, rows: 40 }, props: failed } as never)
    const tree = await ui.drawn()
    expect(await ui.find({ type: 'Button', text: /show all|show less/ })).toBe(undefined)
    await ui.unmount()
    for (const line of ['error 1', 'error 21', 'error 119', `error 120 ${'x'.repeat(200)}`]) expect(drawnAt(tree, line).length).toBeGreaterThan(0)
    expect(nodes(tree, n => n.props?.wrap === 'truncate-end' && strings(n).startsWith('error '))).toEqual([])
  })

  test('an opened answer past one drawing keeps Claude Code\'s result under the row', async ($, on) => {
    // Claude Code's own result draws the output it was given.
    on('ui.render', { component: 'ToolResult' }, ($, e) => {
      const { Text } = $.ui.resolve(e)
      return <Text>{String((e.props as { output?: unknown }).output)}</Text>
    })
    hostWithClipboard(on, { mod: json(MIAMI_NIGHTS) })
    await $.session.start(INTERACTIVE)
    // Past what an opened row draws (MAX_DRAWN), inside what the stand-in's one Text draws.
    const output = lines(2000, k => `error ${k} ${'x'.repeat(36)}`)
    expect(output.length).toBeGreaterThan(MAX_DRAWN)
    expect(output.length).toBeLessThan(100000)
    const last = `error 2000 ${'x'.repeat(36)}`
    const call = { tool_use_id: 't1', tool: 'Read', input: { file_path: '/work/proj/a.ts' }, isRunning: false, isErrored: true, isInterrupted: false, isExpanded: false, output }
    const row = await $.ui.mount({ plugin: 'ways-theme', surface: 'terminal', component: 'ToolUse', requestId: 't1', viewport: { columns: 80, rows: 40 }, props: call } as never)
    const result = await $.ui.mount({ plugin: 'ways-theme', surface: 'terminal', component: 'ToolResult', requestId: 't1', viewport: { columns: 80, rows: 40 }, props: { tool_use_id: 't1', tool: 'Read', output, isErrored: true } } as never)
    const trees = [await row.drawn(), await result.drawn()]
    await row.unmount()
    await result.unmount()
    expect(trees.some(tree => shows(tree, last))).toBe(true)
  })

  const LONG_COMMAND = `rsync -av --exclude node_modules --exclude target ${'/work/proj/some/deep/folder/ '.repeat(6)}/backup/`.trim()
  const bash = (surface: string) =>
    ({ plugin: 'ways-theme', surface, component: 'ToolUse', requestId: 't7', viewport: { columns: 80, rows: 40 }, props: { tool_use_id: 't7', tool: 'Bash', input: { command: LONG_COMMAND }, isRunning: false, isErrored: true, isInterrupted: false, isExpanded: false, output: { stdout: '', stderr: 'rsync: failed', interrupted: false, isImage: false } } }) as never

  test("an opened shell card's `$ command` header draws the whole command and wraps", async ($, on) => {
    hostWithClipboard(on, { mod: json(MIAMI_NIGHTS) })
    await $.session.start(INTERACTIVE)
    const ui = await $.ui.mount(bash('terminal'))
    const tree = await ui.drawn()
    await ui.unmount()
    const header = nodes(tree, n => n.type === 'Text' && strings(n) === `$ ${LONG_COMMAND}`)
    expect(header.length).toBe(1)
    expect(header[0]!.props?.wrap).toBe('wrap')
  })

  test('on the desktop a tool row draws its whole target and wraps', async ($, on) => {
    hostWithClipboard(on, { mod: json(MIAMI_NIGHTS) })
    await $.session.start({ ...INTERACTIVE, surface: 'desktop' })
    const ui = await $.ui.mount(bash('desktop'))
    const tree = await ui.drawn()
    await ui.unmount()
    const row = nodes(tree, n => n.type === 'Text' && strings(n).startsWith('Bash') && strings(n).includes(LONG_COMMAND))
    expect(row.length).toBeGreaterThan(0)
    expect(row.every(n => n.props?.wrap === 'wrap')).toBe(true)
  })

  test('a code card draws every line and wraps a long one, with no `more lines`', () => {
    const card = codeSvg(`${lines(200)}\n${'y'.repeat(300)}`, 'ts', PALETTE, 800)!
    expect(card.source).toContain('>201<')
    expect(card.source).not.toMatch(/more line|…/)
    expect(card.source.match(/y/g)!.length).toBeGreaterThanOrEqual(300)
  })

  test('a code card wraps a line of wide characters by their width', () => {
    const text = '漢'.repeat(100)
    const card = codeSvg(text, '', PALETTE, 600)!
    const rows = [...card.source.matchAll(/font-size="12.5"[^>]*xml:space="preserve">(.*?)<\/text>/g)].map(m => m[1]!.replace(/<[^>]+>/g, ''))
    // The card's width less the gutter of a one-digit line number and the right margin.
    const room = 600 - (8 + 24) - 24
    expect(rows.join('')).toBe(text)
    expect(rows.every(row => measure(row, true, 12.5) <= room)).toBe(true)
    expect(rows.map(row => [...row].length)).toEqual([36, 36, 28])
  })

  test('a diff card draws every line and wraps a long one, with no `more lines`', () => {
    const diff = { path: 'a.ts', isNewFile: false, hunks: [{ oldStart: 1, newStart: 1, lines: [...Array.from({ length: 100 }, (_, k) => `+added ${k + 1}`), `+${'y'.repeat(300)}`] }] }
    const card = diffSvg(diff, 'a.ts', PALETTE, 800)!
    expect(card.source).toContain('added 100<')
    expect(card.source).not.toMatch(/more line|…/)
    expect(card.source.match(/y/g)!.length).toBeGreaterThanOrEqual(300)
  })

  test('a terminal card wraps a long line', () => {
    const card = terminalSvg({ stdout: 'y'.repeat(300), stderr: '', interrupted: false }, false, PALETTE, 800)!
    expect(card.source).not.toContain('…')
    expect(card.source.match(/y/g)!.length).toBeGreaterThanOrEqual(300)
  })

  test('a card past what the Svg element takes is null', () => {
    const big = lines(4000)
    expect(codeSvg(big, 'ts', PALETTE, 800)).toBe(null)
    expect(diffSvg({ path: 'a.ts', isNewFile: true, hunks: [{ oldStart: 0, newStart: 1, lines: big.split('\n').map(line => `+${line}`) }] }, 'a.ts', PALETTE, 800)).toBe(null)
    expect(terminalSvg({ stdout: big, stderr: '', interrupted: false }, false, PALETTE, 800)).toBe(null)
    expect(codeSvg(lines(40), 'ts', PALETTE, 800)!.source.length).toBeLessThan(MAX_SVG)
  })

  test('on the desktop a fence too big for a card keeps Claude Code\'s drawing, whole', async ($, on) => {
    hostWithClipboard(on, { mod: json(MIAMI_NIGHTS) })
    await $.session.start({ ...INTERACTIVE, surface: 'desktop' })
    const fence = `\`\`\`ts\n${lines(900, () => 'abcdefgh')}\n\`\`\``
    expect(fence.length).toBeLessThan(9000)
    const tree = await drawn($, `Here:\n\n${fence}\n\nDone.`, 'desktop')
    expect(nodes(tree, n => n.type === 'Svg')).toEqual([])
    expect(nodes(tree, n => n.type === 'Markdown' && n.props?.text === fence).length).toBe(1)
  })
})
