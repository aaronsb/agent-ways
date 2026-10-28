// Ported from skins by hellosverre (MIT; LICENSE beside this file),
// https://github.com/hellosverre/claude-skins at 985ad63, hooks/links.ts.

import type { Touch } from '../../types'
import { shortenPath } from './format'

// Links and touched files: which terminals draw OSC 8 hyperlinks, which URLs the engine's
// `Link` takes, the `file://` URL a path links to, and how a path in a reply or a tool row
// finds the file this turn changed.

// The environment variables the terminal check reads.
export type HyperlinkEnv = Partial<
  Record<'FORCE_HYPERLINK' | 'TERM_PROGRAM' | 'TERM_PROGRAM_VERSION' | 'TERM' | 'WT_SESSION' | 'VTE_VERSION' | 'KONSOLE_VERSION' | 'TMUX' | 'CI', string | undefined>
>

const PROGRAMS = new Set(['iTerm.app', 'WezTerm', 'vscode', 'ghostty', 'Hyper', 'WarpTerminal', 'Tabby', 'rio'])
const TERMS = /^(xterm-kitty|xterm-ghostty|alacritty|foot|wezterm|contour)/

// Whether the terminal draws OSC 8 links: `FORCE_HYPERLINK=1|0` decides outright; else the
// terminals known to draw them. tmux passes them on only when set up to, so it reads as no.
export function hyperlinksFrom(env: HyperlinkEnv): boolean {
  if (env.FORCE_HYPERLINK !== undefined && env.FORCE_HYPERLINK !== '') {
    return env.FORCE_HYPERLINK !== '0'
  }

  if (env.CI !== undefined || env.TMUX !== undefined) {
    return false
  }

  if (env.WT_SESSION !== undefined || env.KONSOLE_VERSION !== undefined) {
    return true
  }

  if (env.VTE_VERSION !== undefined && Number(env.VTE_VERSION) >= 5000) {
    return true
  }

  if (env.TERM_PROGRAM === 'Apple_Terminal') {
    // Terminal.app draws them from macOS 15's version 455 on.
    return Number((env.TERM_PROGRAM_VERSION ?? '').split('.')[0]) >= 455
  }

  return (env.TERM_PROGRAM !== undefined && PROGRAMS.has(env.TERM_PROGRAM)) || TERMS.test(env.TERM ?? '')
}

// The bound the engine's `Link` and the terminals hold an href to.
const MAX_HREF = 2048

// Printable ASCII only: no control character reaches the terminal inside an href.
const isPrintable = (href: string): boolean => /^[\x21-\x7e]+$/.test(href)

// The URL as the engine's `Link` takes it (https, or http on localhost; printable ASCII,
// no `@`, at most 2048 characters), or null: such a URL stays text.
export function safeHref(text: string): string | null {
  let url: URL

  try {
    url = new URL(text)
  } catch {
    return null
  }

  const href = url.href
  const isLocal = url.protocol === 'http:' && (url.hostname === 'localhost' || url.hostname === '127.0.0.1')

  return (url.protocol === 'https:' || isLocal) && href.length <= MAX_HREF && isPrintable(href) && !href.includes('@') ? href : null
}

const slashed = (path: string): string => path.replace(/\\/g, '/')

// Where a path in text is placed from: the session's folder, and home for `~/`.
export type PathBase = { cwd: string; home?: string }

// `/a/./b/../c` as `/a/c`; a `..` past the root, or past a drive's root, stays there.
function folded(path: string): string {
  const drive = /^\/([A-Za-z]:)(?:\/|$)/.exec(path)?.[1]
  const kept: string[] = []

  for (const segment of path.split('/').slice(drive === undefined ? 1 : 2)) {
    if (segment === '..') {
      kept.pop()
    } else if (segment !== '.' && segment !== '') {
      kept.push(segment)
    }
  }

  const parts = drive === undefined ? kept : [drive, ...kept]

  return `/${parts.join('/')}${path.endsWith('/') && kept.length > 0 ? '/' : ''}`
}

// A path as the `file://` URL a terminal opens, or null where it cannot be placed or the
// URL runs past the bound. Relative paths are under the session's folder, `~/` under home.
// A network path (`\\server\share`, a WSL folder) is not placed: its host is not this one.
// The host is left empty, which kitty and the other terminals read as this machine. A
// `:line` becomes `#line`, the fragment kitty's open actions match and ripgrep's kitty
// hyperlink format writes; a column is dropped. Each segment is percent-encoded.
export function fileHref(text: string, base: PathBase): string | null {
  const line = /:(\d+)(?::\d+)?$/.exec(text)
  const path = slashed(line === null ? text : text.slice(0, line.index))
  const home = base.home === undefined || base.home === '' ? null : slashed(base.home).replace(/\/+$/, '')
  const cwd = slashed(base.cwd).replace(/\/+$/, '')
  const isRooted = /^[A-Za-z]:\//.test(path) || path.startsWith('/')
  const full = isRooted ? path : path.startsWith('~/') ? (home === null ? null : `${home}/${path.slice(2)}`) : cwd === '' ? null : `${cwd}/${path}`

  if (full === null || full.startsWith('//')) {
    return null
  }

  // A drive path is `/C:/...` in a URL.
  const rooted = folded(/^[A-Za-z]:\//.test(full) ? `/${full}` : full)

  try {
    const encoded = rooted
      .split('/')
      .map(segment => encodeURIComponent(segment).replace(/%3A/gi, ':').replace(/%40/g, '@'))
      .join('/')
    const href = `file://${encoded}${line === null ? '' : `#${line[1]}`}`

    return href.length <= MAX_HREF && isPrintable(href) ? href : null
  } catch {
    // A lone surrogate does not encode.
    return null
  }
}

// What this turn did to the file a path names, if anything. The path may be absolute or
// relative to the session's folder and may carry `:line`.
export function touchOf(touched: Readonly<Record<string, Touch>>, text: string, cwd: string): Touch | undefined {
  const path = slashed(text.replace(/(?::\d+){1,2}$/, '')).replace(/^\.\//, '')

  if (path === '') {
    return undefined
  }

  for (const [file, touch] of Object.entries(touched)) {
    const full = slashed(file)

    if (full === path || slashed(shortenPath(file, cwd)) === path) {
      return touch
    }
  }

  return undefined
}
