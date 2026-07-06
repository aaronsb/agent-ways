# Ways-Driven Development Config

Contextual ways of working, disclosed by hooks, with GitHub-first collaboration.
Architecture decisions are recorded as ADRs — one thread among several, not the
whole method.

**Guidance is injected via hooks, not this file.** It loads on **SessionStart**
(including after compaction), so the relevant ways stay live in the conversation
window instead of sitting as a distant, always-on system prompt.

The live guidance is installed under `~/.claude/`, not this repo's working tree.
See `~/.claude/hooks/ways/core.md` for the base posture, and the other
`~/.claude/hooks/ways/**` files for the contextual ways that disclose themselves
when triggered.
