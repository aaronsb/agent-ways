// The host the skins tests run against: `ways` answered from a world the test
// holds, a clock it moves, and readers for the trees the mod draws.

import { mock } from 'claude-code/testing'
import type { On, ProcessRunResult, RenderElement } from 'claude-code'

import { MOD_EXPORT_ARGV } from '../hooks/skins/config.ts'

export const ok = (stdout: string, exitCode = 0): ProcessRunResult => ({ exitCode, stdout, stderr: '', isStdoutTruncated: false, isStderrTruncated: false })
export const json = (v: unknown) => ok(JSON.stringify(v))

// What the register module's own calls read: Miami Nights active and exported
// for Claude Code, so its frames are themed.
export const CLAUDE_CODE_MIAMI = { name: 'Miami Nights', base: 'dark', overrides: { claude: '#ff4d93', userMessageBackground: '#3d1466' } }

export type Answer = ProcessRunResult | Error

export type World = {
  /** `ways settings theme export --mod`; ways missing when not given. */
  mod?: Answer
  verbose?: boolean
  /** Claude Code's `theme` row: `custom:ways` when not given, null for no
   *  row. A `config.set` of the row changes it, as the engine's writer does. */
  selected?: string | null
  /** `$.config.list` rejects. */
  listFails?: boolean
}

export const INTERACTIVE = { cwd: '/work/proj', surface: 'terminal', isInteractive: true } as const

/** The host: `ways` answered from `world` (the mod export may change between
 *  reads), a clock the test moves, every log, toast, registration and store
 *  call recorded. */
export function host(on: On, world: World) {
  const clock = mock.clock(on, { now: 1_000 })
  const modRuns: (number | undefined)[] = []
  const logs: string[] = []
  const toasts: unknown[] = []
  const stripped: string[] = []
  on('session.start', ($, e) => ({ cwd: e.cwd }))
  on('session.cwd', () => ({ value: '/work/proj' }) as never)
  on('session.usage', () => ({ value: { startedAt: 0, context: { window: 200000, percent: 42 }, rateLimits: [] } }) as never)
  on('ui.log', ($, e) => {
    if (String(e.text).startsWith('ways-theme skins:')) logs.push(String(e.text))
    return { value: undefined } as never
  })
  on('ui.toast', ($, e) => {
    toasts.push(e)
    return { value: undefined } as never
  })
  for (const noun of ['command.register', 'tool.register', 'store.get', 'store.set', 'ui.open', 'http.fetch'] as const) {
    on(noun, () => {
      stripped.push(noun)
      return { value: undefined } as never
    })
  }
  on('process.run', ($, e) => {
    const argv = e.argv.join(' ')
    if (argv === MOD_EXPORT_ARGV.join(' ')) {
      modRuns.push(e.init?.timeoutMs)
      if (world.mod === undefined) throw new Error('spawn ways ENOENT')
      if (world.mod instanceof Error) throw world.mod
      return { value: world.mod } as never
    }
    if (argv === 'ways settings get theme.active') return { value: ok('miami-nights\n') } as never
    return { value: json(CLAUDE_CODE_MIAMI) } as never
  })
  on('env.get', ($, e) => ({ value: e.name === 'HOME' ? '/home/dev' : undefined }) as never)
  on('fs.read', () => {
    throw new Error('ENOENT')
  })
  on('fs.write', () => ({ value: undefined }) as never)
  on('config.set', ($, e) => {
    if (e.key === 'theme' && typeof e.value === 'string') world.selected = e.value
    return { value: e.value }
  })
  on('config.list', () => {
    if (world.listFails) throw new Error('config unavailable')
    const verbose = { key: 'verbose', label: 'Verbose', kind: 'boolean', value: world.verbose ?? false, provider: { kind: 'engine' }, isLocked: false }
    const selected = world.selected === undefined ? 'custom:ways' : world.selected
    const theme = selected === null ? [] : [{ key: 'theme', label: 'Theme', kind: 'choice', value: selected, provider: { kind: 'engine' }, isLocked: false }]
    return { value: [verbose, ...theme] } as never
  })
  on('ui.render', ($, e) => {
    // The dialog's tree holds the engine's own node.
    if (e.component === 'AskUserQuestion') return { type: 'engine', ref: 0 } as never
    const { Text } = $.ui.resolve(e)
    return <Text>engine</Text>
  })
  return { clock, modRuns, logs, toasts, stripped }
}

export type Node = { type?: string; props?: Record<string, unknown>; children?: unknown[] }

/** Every element of a drawn tree, outermost first. */
export function walk(tree: unknown, out: Node[] = []): Node[] {
  if (tree === null || typeof tree !== 'object') return out
  const node = tree as Node
  if (typeof node.type === 'string') out.push(node)
  for (const child of node.children ?? []) walk(child, out)
  return out
}

export const borders = (tree: RenderElement) => walk(tree).filter(n => n.type === 'Box' && n.props?.borderStyle !== undefined)
export const grounds = (tree: RenderElement) => walk(tree).filter(n => n.type === 'Text' && typeof n.props?.backgroundColor === 'string').map(n => n.props!.backgroundColor as string)
export const shows = (tree: RenderElement, text: string) => JSON.stringify(tree).includes(text)
