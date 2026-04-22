---
description: Persistent memory system — MEMORY.md, topic files, what to record and when
vocabulary: remember memory save note forget recall persist session learning gotcha pattern
trigger: context-threshold
threshold: 80
pattern: remember|save.*(to|this|that).*memory|note.*(for|this).*(later|next)|don't forget|keep.*in.*mind
macro: prepend
scope: agent
requires: ["Bash(jq:*)", "Bash(sed:*)", "Bash(ways:*)", "Bash(wc:*)"]
refire: 0.15
---
<!-- epistemic: convention -->
# Memory

Memory is Claude Code's auto-memory — the `MEMORY.md` file and topic files in the project's memory directory. It persists across sessions. The first 200 lines of MEMORY.md load at every session start.

## When This Fires

This way fires in two contexts:

1. **User asks to remember something** — explicit request like "remember this" or "note this for later"
2. **Context threshold** — context is filling up and it's time to checkpoint before compaction

For explicit requests, just record what the user asked. For threshold checkpoints, apply the surprise test below.

## Surprise Test (threshold checkpoint only)

Did anything unexpected happen this session? A gotcha, a pattern that broke assumptions, a workaround you had to discover? If the session was routine — standard code, familiar patterns, no surprises — there's nothing new to record. Skip and keep working.

## What to Record

- Gotchas and workarounds specific to this codebase
- Patterns that worked (or didn't) for this project
- Project-specific tool/config quirks
- Decisions made and their rationale

**Not worth recording:**
- Generic knowledge you already have
- One-off context that won't recur
- Anything already captured in a way or project doc
- Way content — never duplicate a way's guidance, just reference it

## Memory as Way References

Memory entries about ways should be pointers, not copies. Ways are structured, curated guidance. Memory records *experience* with that guidance:

```markdown
## Useful Ways
- `softwaredev/code/testing` — relevant when editing integration tests,
  not just when "test" keywords appear. --forked flag is essential in this repo.
- `softwaredev/delivery/commits` — always check before pushing to repos
  with pre-commit hooks; caught formatting issues in sessions 2 and 3.
```

This is progressive disclosure: ways hold the knowledge, memory indexes when and why it matters. Claude can `Read` any way file on demand — memory tells it *which ones to reach for* in context the triggers wouldn't catch.

## Writing Memory

Follow the two-step save described in the harness's `# auto memory` block: write the topic file, then add a one-line pointer to `MEMORY.md`. Read the current `MEMORY.md` first so you update existing entries rather than duplicate them.
