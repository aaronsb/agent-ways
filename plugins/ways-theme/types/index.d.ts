// The ways-theme mod's types contract: the skins layer's shapes and the
// session state its hooks keep.
//
// Adapted from skins by hellosverre (MIT; hooks/skins/LICENSE),
// https://github.com/hellosverre/claude-skins at 985ad63, types/index.d.ts.
// The prefs no longer come from a settings store or a slash command: the
// skins layer builds them from `ways settings theme export --mod`
// (hooks/skins/config.ts). The `$.skins` API for other mods and the
// skin-code bridge are gone, and the session state lives under this mod's
// own name.

// Every colour a skin names. `fg` is body text, `muted` secondary text, `surface` the
// band behind a table header, `zebra` every other table row.
export type SkinSlot =
  | 'read'
  | 'write'
  | 'run'
  | 'search'
  | 'web'
  | 'mcp'
  | 'other'
  | 'user'
  | 'fg'
  | 'muted'
  | 'surface'
  | 'zebra'
  | 'ok'
  | 'err'
  | 'warn'

// What the drawing reads. `skin` is the ways theme's name.
export type Prefs = {
  skin: string
  icons: 'unicode' | 'ascii'
  // Which side the outline around a typed prompt sits on. 'plain' drops the outline for a
  // leading marker.
  prompt: 'left' | 'right' | 'plain'
  // Whose words the spinner and the turn footer show: 'claude' keeps Claude Code's own;
  // 'skin' draws the theme's.
  words: 'claude' | 'skin'
  rail: boolean
  // 'text' keeps the skin but draws tables and code as text the desktop can select.
  tables: 'on' | 'text' | 'off'
  shimmer: boolean
  band: boolean
  // When each plan limit resets, beside its meter in the band.
  countdown: boolean
  markdown: boolean
  quiet: boolean
  charts: boolean
  // Display formulas as cards and inline TeX as Unicode.
  math: boolean
  // Tables and code in slash-command output drawn the way replies are.
  commands: boolean
  // Shell output as a card: the command, its exit status, stderr apart.
  shell: boolean
  // Fenced code on the terminal and shell output in the skin's colours.
  highlight: boolean
  // Bare URLs and paths as links: OSC 8 where the terminal draws it, anchors on the desktop.
  links: boolean
  // Copy buttons on tables, code, diagrams and whole replies.
  copy: boolean
  // How a copy control draws its forms: as words, or as glyphs.
  copyStyle: 'verbose' | 'compact'
  // A tool row's target path too long for its line: on lines of its own under the tool's
  // name, or cut short with an ellipsis.
  paths: 'wrap' | 'truncate'
  // Runs of reads and searches drawn open, as if each had been expanded, not as one line.
  expand: boolean
  // Kept for the drawing, which reads it for motion; always null here.
  calm: null
}

// How full the context window is and how much of each plan limit is spent, in percent,
// as the band above the prompt shows them, and when each limit resets (epoch ms) where known.
export type UsageSnap = { context: number | null; limits: { label: string; percent: number; resetsAt?: number }[] }

// What one turn did, shown in its footer.
export type TurnStats = { tools: number; added: number; removed: number }

// What this turn did to a file: made it, or changed one that was there.
export type Touch = 'created' | 'edited'

// How a tool row stands: opened or closed by the person, or `auto`, open only when the
// call failed.
export type Disclosure = 'auto' | 'open' | 'closed'

declare module 'claude-code' {
  interface PluginState {
    'ways-theme': {
      // The look the export gave, as JSON: the prefs and the skin; '' while none.
      look: string
      startedAt: number
      frame: number
      turns: Record<string, TurnStats>
      duration: StateFamily<number>
      usage: UsageSnap
      // The usage band's countdowns by limit label, as last drawn.
      countdowns: Record<string, string>
      images: StateFamily<boolean>
      quiet: StateFamily<boolean>
      // A shell call's command, for its output card's header: ToolResult does not carry it.
      command: StateFamily<string>
      // Each tool row's chevron.
      disclosure: StateFamily<Disclosure>
      // Files the turn created or edited, by absolute path, for their colour in rows and prose.
      touched: Record<string, Touch>
      settle: number
    }
  }
}
