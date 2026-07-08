---
description: How we write Claude Code skills in this repo — skill naming and scope conventions, the global-scope caveat, and choosing a skill as the authoring surface for a procedure; defers SKILL.md mechanics to the official docs
vocabulary: skill slash command SKILL.md create author write invoke user-invocable plugin convention scope global
pattern: SKILL\.md|skill.?(creation|author|write)|claude.?code.?skill|~\/\.claude\/skills
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: convention -->
# Skills Way

This way is *our convention* for writing skills — not a SKILL.md tutorial. The
mechanics (every frontmatter field, location precedence, progressive-disclosure
layout, argument/shell substitution) live in the canonical reference and change
faster than any copy here would. Read it for the "how":

> **Canonical mechanics:** https://code.claude.com/docs/en/skills.md

Don't restate that doc in a skill or a way. If you catch yourself writing a
frontmatter-fields table, stop — link the doc instead. What follows is only the
judgment the doc can't make for you in *this* repo.

## First decide: skill, way, or slash command?

These three overlap, and reaching for the wrong one is the most common mistake.

| Want | Use | Because |
|------|-----|---------|
| Guidance that fires when a tool/file/prompt matches, injected mid-session | **a way** (`hooks/ways/…`) | Hooks disclose it just-in-time; no user action; participates in embedding match |
| A capability the user (or Claude) invokes by name to *do* a task | **a skill** (`skills/…`) | Self-contained, can carry scripts and `allowed-tools`, runnable on demand |
| A throwaway reusable prompt with no logic | a plain slash command | Lighter than a skill; no directory, no tools |

Rule of thumb: **a way teaches Claude how to behave; a skill gives Claude something to run.** If the answer is "inject advice when X happens," it's a way — and most of *this repo's* value is ways, so default there and only reach for a skill when there's a concrete procedure to execute. (`ways-update`, `ways-tests`, `merge`, `attend` are skills because each *runs a procedure*; `meta/knowledge`, `softwaredev/code/quality` are ways because they *shape behavior*.)

## The scope caveat — `skills/` here projects into personal scope

`skills/` in this repo is **projected into `~/.claude/skills/`** (ADR-142) — the
live personal scope. Once a skill here reaches your install (via `ways reconcile` /
update, or immediately if you're editing the symlinked projection), it is available
in **every** project on this machine. Two consequences:

- **Triggers must be tight.** A loose `description` on a global skill hijacks
  unrelated requests everywhere. Name the specific task and the words a user would
  actually say, and say what it's *not* for. (`ways-update` ends its description
  with "Not for editing or authoring individual ways… or upgrading project
  dependencies" precisely to stay in its lane.)
- **A global skill must be location-independent.** It can't assume cwd. Resolve the
  target up front — e.g. `ROOT="${CLAUDE_CONFIG_DIR:-$HOME/.claude}"` — and verify
  it's the repo you expect before acting.

If a capability only makes sense inside one project, it belongs in *that* project's
`.claude/skills/`, not here.

## House conventions

- **Naming.** Lowercase, hyphenated, directory matches `name`. Group related skills
  into a family prefix that already exists rather than inventing a sibling vocabulary:
  `ways-*` (tests, update), `think-*`, `project-*`. A new `ways-foo` reads as kin to
  `ways-tests`; a bare `foo` reads as orphaned.
- **Self-contained and honest about side effects.** Skills here lean on real tooling
  (`make`, `ways`, `gh`) rather than reimplementing it. If a skill mutates anything —
  git state, the working tree, remote — make it ask before destructive moves, the same
  bar the delivery ways hold.
- **Defer, don't duplicate.** Point at the canonical doc for mechanics and at sibling
  ways/skills for adjacent concerns; keep the skill about its one job.

## Worked example

`skills/ways-update/SKILL.md` is the reference for the conventions above: a tight
single-purpose description with an explicit "not for" clause, `CLAUDE_CONFIG_DIR`
resolution so it runs from anywhere, real `make`/`ways` tooling instead of
hand-rolled steps, and a pre-flight check before it touches git. Copy its shape.

## Validate before shipping

A skill is picked up at Claude Code startup — there's no corpus rebuild (that's
ways). After adding or editing one:

- Confirm `name` matches the directory and the frontmatter parses.
- **Restart Claude Code**, then ask "what skills are available?" and trigger it with
  a realistic phrasing to confirm the description fires when it should — and doesn't
  fire on the near-miss requests you wrote it to avoid.

## See Also

- knowledge/authoring(meta) — authoring ways (the other half of the skill-vs-way call)
- Canonical SKILL.md reference — https://code.claude.com/docs/en/skills.md
