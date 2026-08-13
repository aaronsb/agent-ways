# Ways-Driven Development Config

Contextual ways of working, disclosed by hooks, with GitHub-first collaboration. Architecture decisions are recorded as ADRs, which is one thread among several in the method.

**Guidance is injected via hooks rather than through this file.** It loads on **SessionStart** (including after compaction), so the relevant ways stay live in the conversation window instead of sitting as a distant, always-on system prompt.

The live guidance is installed under `~/.claude/`, outside this repo's working tree. See `~/.claude/hooks/ways/core.md` for the base posture, and the other `~/.claude/hooks/ways/**` files for the contextual ways that disclose themselves when triggered.

The active output style governs register, meaning how output reads. The ways corpus carries method. `hooks/ways/core.md` is prepended to every session, so its own prose is a style sample the model imitates, and `scripts/check-register.sh` holds it to plain construction. See ADR-178.
