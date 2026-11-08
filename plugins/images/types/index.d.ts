// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

declare module 'claude-code' {
  interface PluginState {
    images: {
      /** Claude Code's own answer to whether this terminal draws kitty
       *  graphics with Unicode placeholders, as the probe learned it; null
       *  while unknown. */
      kittyGraphics: boolean | null
      /** Whether Read's description carries the steer, decided at the first
       *  ask and kept, so an ask after a reload of the mod answers the same. */
      steersRead: boolean
      /** The images pasted with a prompt, written to the cache as data URI
       *  files, by the prompt row's uuid. */
      pasted: StateFamily<string[]>
    }
  }
}
