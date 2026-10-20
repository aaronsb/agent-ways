---
description: Writing a personal session posture way, a short always-on way in your own ways directory that loads once at session start and holds the standing rules you want in every session, such as approval gates, register, trigger words, and how to close a work turn
vocabulary: posture always-on session-start personal standing rules gates register trigger words house rules every session my own way global preferences once refire constraint
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: convention -->
# Personal Posture Way

Some rules apply to every session a user runs, in every project. Examples are "never push without being asked", "drafts stay unsent until I say send", and "answer short". A personal posture way holds them. It lives in your own ways directory (`$XDG_CONFIG_HOME/agent-ways/ways/`), so it reaches every project and survives updates to the shipped ways.

## Frontmatter

```markdown
---
trigger: session-start
refire: once
scope: agent
---
<!-- epistemic: constraint -->
```

Use `trigger: session-start` with no description or vocabulary. The way fires from session state, not from a prompt match, so it needs no golden prompts. `refire: once` keeps it from repeating as the session grows. Add `subagent` to scope only if subagents get the same rules some other way and you want them restated.

## What goes in it

Keep the body short. It costs context in every session, so aim for well under 3,000 characters. Use one line per rule. Sections that work well:

| Section | Holds |
|---|---|
| Register | How replies should read: length, sentence shape, banned phrases |
| Gates | Actions that need an explicit go: commit, push, deploy, send, writes to systems of record |
| Context | What to confirm before acting, such as the repo, remote, and active accounts |
| Trigger words | A table of phrases the user types and what each one means |
| Working rules | Run tools yourself, verify before asserting, model choice for subagents |
| Closing a work turn | What to state after changing files or systems |

Write each gate as a bound ("only when asked, and only the step named"), not a preference. Keep a constraint at the strength the user stated it.

## What stays out

- Detail that a topic way already carries. Link to that way under See Also and keep one line here.
- Anything `core.md` already says. The posture way adds to the base posture. It does not restate it.
- Names, account handles, client lists, and secrets. Refer to secrets by name or path only.
- Status, history, or project facts. Those belong in the project instructions or a log file.

## Trigger words

A trigger-word table lets one short phrase stand for a longer instruction. Pick phrases that rarely appear by accident. Each row says what the phrase allows and what it rules out, for example "answer inline, reading is fine, make no writes or sends".

## Check it

Run `ways author lint` on your ways directory. Start a new session and confirm the way appears once at the top. If a rule keeps getting missed mid-session, move it into a topic way that fires when the risky action is near, because a once-only way fades as the session grows.

## See Also

- knowledge/authoring(meta) — parent: way format and matching
- knowledge/authoring/refire(meta) — the `once` preset and other cadences
- knowledge/authoring/pii-free(meta) — keeping personal details out of shared ways
- memory(meta) — what belongs in memory instead
