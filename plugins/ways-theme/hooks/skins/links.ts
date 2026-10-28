// Ported from skins by hellosverre (MIT; LICENSE beside this file),
// https://github.com/hellosverre/claude-skins at 985ad63, hooks/links.ts.

import type { Touch } from './types.ts'
import { shortenPath } from './format'

// Links and touched files: which terminals draw OSC 8 hyperlinks, which URLs the engine's
// `Link` takes, and how a path in a reply or a tool row finds the file this turn changed.

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

  return (url.protocol === 'https:' || isLocal) && href.length <= 2048 && /^[\x21-\x7e]+$/.test(href) && !href.includes('@') ? href : null
}

const slashed = (path: string): string => path.replace(/\\/g, '/')

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
