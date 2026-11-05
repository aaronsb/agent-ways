declare module 'claude-code' {
  interface PluginState {
    images: {
      /** Claude Code's own answer to whether this terminal draws kitty
       *  graphics with Unicode placeholders, as the probe learned it; null
       *  while unknown. */
      kittyGraphics: boolean | null
      /** Whether Read's description carries the steer, decided at the first
       *  ask of the session and kept, so a later ask answers the same. */
      steersRead: boolean
      /** The images pasted with a prompt, written to the cache as data URI
       *  files, by the prompt row's uuid. */
      pasted: StateFamily<string[]>
    }
  }
}
