// Ported from skins by hellosverre (MIT; LICENSE beside this file),
// https://github.com/hellosverre/claude-skins at 985ad63, hooks/skin.ts.

import type { SkinSlot } from './types.ts'

// Shapes shared by the skins and the row builders.

// What a tool call is, by colour. `other` only shows in group rows.
export type Kind = 'read' | 'write' | 'run' | 'search' | 'web' | 'mcp' | 'other'

// The kinds a skin draws a tool row for; the rest keep Claude Code's own row.
export type ToolKind = Exclude<Kind, 'other'>

export type Slot = SkinSlot

export type Palette = Readonly<Record<Slot, string>>

export type Skin = {
  readonly name: string
  readonly label: string
  readonly palette: Palette
  // Gerunds for the spinner, and past-tense words for the turn footer.
  readonly spinner: readonly string[]
  readonly done: readonly string[]
  // Its own palette for a light background; without one, light.ts derives it.
  readonly light?: Palette
  readonly ansi?: Palette
}

export type IconSet = 'unicode' | 'ascii'

export type Icons = Readonly<
  Record<'running' | 'ok' | 'err' | 'interrupted' | 'done' | 'rail' | 'branch' | 'prompt', string>
> & { readonly frames: readonly string[] }

// Single-cell glyphs with no emoji presentation; each status has its own shape, not only a colour.
export const ICONS: Readonly<Record<IconSet, Icons>> = {
  unicode: {
    running: '○',
    ok: '●',
    err: '✕',
    interrupted: '◌',
    done: '◆',
    rail: '┃',
    branch: '─',
    prompt: '▍',
    frames: ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'],
  },
  ascii: {
    running: 'o',
    ok: '*',
    err: 'x',
    interrupted: '-',
    done: '>',
    rail: '|',
    branch: '-',
    prompt: '>',
    frames: ['|', '/', '-', '\\'],
  },
}
