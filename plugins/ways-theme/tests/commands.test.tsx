import { describe, expect, test } from 'claude-code/testing'
import type { TestBody } from 'claude-code/testing'
import type { On, RenderElement } from 'claude-code'

import { commandPieces, fenceCommand, holdsCommand } from '../hooks/skins/commands.ts'
import { setupOf } from '../hooks/skins/config.ts'
import { splitReply } from '../hooks/skins/markdown.ts'
import { inlineMath } from '../hooks/skins/math.ts'
import { replySegments } from '../hooks/skins/reply.ts'
import { MIAMI_NIGHTS } from './fixtures.ts'
import { INTERACTIVE, borders, host, json, shows } from './host.tsx'
import type { Node, World } from './host.tsx'

const PREFS = setupOf(MIAMI_NIGHTS as never).prefs as never
const at = (text: string, surface = 'terminal', columns = 80) =>
  ({ plugin: 'ways-theme', surface, component: 'AssistantMessage', requestId: 'r1', viewport: { columns, rows: 40 }, props: { text, isFirstOfReply: true } }) as never

/** The command from the operator's screenshot, wider than any line it was drawn on. */
const COMMAND = '! git worktree remove ../vtium-glyph-fix36 && git worktree remove ../vtium-glyph-s4 && git worktree remove ../vtium-heads-adr && git branch -d feat/glyph-fix36 feat/glyph-s4 docs/heads-adr && git push origin --delete docs/heads-adr feat/deferred'
const SCREENSHOT = `You can run it yourself:\n\n${COMMAND}`

const commands = (text: string) => commandPieces(text).flatMap(piece => (piece.kind === 'command' ? [piece.command] : []))
const prose = (text: string) => commandPieces(text).flatMap(piece => (piece.kind === 'text' ? [piece.text] : []))

describe('which suggestions match', () => {
  test('a line of its own, after a lead-in paragraph', () => {
    expect(commandPieces(SCREENSHOT)).toEqual([
      { kind: 'text', text: 'You can run it yourself:' },
      { kind: 'command', command: COMMAND, raw: COMMAND },
    ])
  })

  test('a line straight after a lead-in sentence on the line before', () => {
    expect(commandPieces('Run this:\n! git status  \nThen look.')).toEqual([
      { kind: 'text', text: 'Run this:' },
      { kind: 'command', command: '! git status', raw: '! git status  ' },
      { kind: 'text', text: 'Then look.' },
    ])
  })

  test('a list item, its marker dropped', () => {
    expect(commands('- ! git status\n* ! git diff\n2. ! make test\n  3) ! make lint')).toEqual(['! git status', '! git diff', '! make test', '! make lint'])
  })

  test('a line that ends in a continuation takes the lines after it, to the first that does not continue', () => {
    expect(commandPieces('! git fetch &&\n  git rebase origin/main |\n  tee log \\\n  more\nnext')).toEqual([
      { kind: 'command', command: '! git fetch &&\n  git rebase origin/main |\n  tee log \\\n  more', raw: '! git fetch &&\n  git rebase origin/main |\n  tee log \\\n  more' },
      { kind: 'text', text: 'next' },
    ])
    expect(commands('! git fetch &&\n\nnext')).toEqual(['! git fetch &&'])
  })

  test('a `|` continuation stops at a table row', () => {
    expect(commandPieces('! git log --oneline |\n| a | b |\n|---|---|')).toEqual([
      { kind: 'command', command: '! git log --oneline |', raw: '! git log --oneline |' },
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
    expect(commandPieces('Run `! git status` to see what moved.')).toEqual([
      { kind: 'text', text: 'Run' },
      { kind: 'command', command: '! git status', raw: '`! git status`' },
      { kind: 'text', text: 'to see what moved.' },
    ])
  })

  test('a span with CommonMark padding, a double-backtick span, and punctuation after a span', () => {
    expect(commands('Try `` ! echo `date` `` or ` ! ls `.')).toEqual(['! echo `date`', '! ls'])
    expect(prose('Try `` ! echo `date` `` or ` ! ls `.')).toEqual(['Try', 'or'])
  })

  test('a list item that is only a span keeps no empty bullet', () => {
    expect(commandPieces('- `! git status`')).toEqual([{ kind: 'command', command: '! git status', raw: '`! git status`' }])
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
    expect(kinds).toEqual(['text', 'command', 'command', 'table'])
  })

  test('off the terminal a reply keeps no command segment', () => {
    expect(replySegments(SCREENSHOT, PREFS, 'desktop')?.some(segment => segment.kind === 'command') ?? false).toBe(false)
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
      expect(commandPieces(text).every(piece => piece.kind === 'text')).toBe(true)
    })
  }

  test('the quick test passes prose by', () => {
    expect(holdsCommand("That's it! Done.\na != b\n![x](y)")).toBe(false)
    expect(holdsCommand(SCREENSHOT)).toBe(true)
    expect(holdsCommand('see `! ls`')).toBe(true)
  })

  test('a fence of another language is not a command', () => {
    expect(fenceCommand('python', '! pip install x', '')).toBe(null)
    expect(fenceCommand('', '! git push', 'raw')).toEqual({ kind: 'command', command: '! git push', raw: 'raw' })
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
      expect(replySegments(text, PREFS, 'terminal')?.some(segment => segment.kind === 'command') ?? false).toBe(false)
      expect(commandPieces(text).every(piece => piece.kind === 'text')).toBe(true)
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
    expect(/[\n│╭╮╰╯]/.test(h.copies[0]!)).toBe(false)
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
