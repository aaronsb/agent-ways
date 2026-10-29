declare module 'claude-code' {
  interface PluginState {
    images: {
      /** Claude Code's own answer to whether this terminal draws kitty
       *  graphics with Unicode placeholders, as the probe learned it; null
       *  while unknown. */
      kittyGraphics: boolean | null
      /** The images pasted with a prompt, written to the cache as data URI
       *  files, by the prompt row's uuid. */
      pasted: StateFamily<string[]>
    }
  }
}
