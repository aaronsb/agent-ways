---
contract: adr/v1
kind: decision
verb: change
capability: [authoring, cli, install]
amends: [ADR-111#1]
basis:
  - operator: aaronsb
    level: directed
    said: "let's work on an improvement spike to reduce the number of commands and tools necessary to manage the lifecycle of ways, and work in a lint/check for sizes. any of these very large ones (like authorship) deserve a progressive disclosure shape"
    via: session 2026-09-29, the request that started the way-lifecycle spike
  - operator: aaronsb
    level: directed
    said: "the third leg of this is to update the skills that bring the lifecycle management to claude"
    via: session 2026-09-29, the same request
  - operator: aaronsb
    level: directed
    said: "a fourth research task is to see if claude code has added a feature that looks like ways"
    via: session 2026-09-29, the same request
  - operator: aaronsb
    level: guided
    said: "let's do that"
    via: "session 2026-09-29, approving the agent-written plan, after the spike, to fix the broken references and draft this record; the plan was written by the agent"
  - evidence: "the way-lifecycle spike against main d55b94b3 (v1.22.0): adding a way and checking that it matches takes 8 distinct commands (template, lint, corpus, match, siblings, suggest, tree, tools/scripts/probe-measure.py); /ways-tests has 14 modes, five of them (budget, crowding, compare, metrics, check) with no command behind them; there are three update paths (ways update, make update, and the ways-update skill's own pull, make update-binaries and reconcile)"
  - evidence: "broken references at d55b94b3: commands/ways.md runs `ways embed --query`, a flag that does not exist; skills/ways-tests and the optimization way teach `suggest --apply` and `--all`, which do not exist; ways-localize reads ~/.claude/tools/, which is not projected (ADR-142); /sync-to-home serves the subdirectory topology that docs/install-guide.md marks superseded; the authoring way's body is 14,233 chars"
  - evidence: "of 143 ways, 22 declare `files:`, and two trigger on a file glob with no other channel (meta/knowledge/authoring and its child tool-agnostic); both exist to fire when Claude edits a way file"
  - upstream: 'Claude Code memory docs, path-specific rules: "Path-scoped rules trigger when Claude reads files matching the pattern, not on every tool use."'
  - upstream: "Claude Code hooks reference, JSON output: a hook's additionalContext is capped at 10,000 characters; over the limit, Claude Code saves the output to a file and replaces it with the file path and a preview of up to the first 2,000 characters, and does not ask Claude to read the file"
  - precedent: ADR-311
  - precedent: ADR-111
agent:
  name: Claude
  model: claude-opus-5-5
status: proposed
date: 2026-09-29
deciders:
  - aaronsb
related:
  - ADR-105
  - ADR-111
  - ADR-139
  - ADR-140
  - ADR-142
  - ADR-144
  - ADR-160
  - ADR-183
  - ADR-185
  - ADR-311
---

# ADR-501: ways check is the single authoring gate

## Summary

- **Decided:** `ways check` is the one command that validates a way. It runs lint, the size rule, hard-wrap detection, the See Also reference check, sibling overlap, the tree budget, a corpus rebuild when stale, and scoring of should-fire and should-not-fire prompts. `lint`, `siblings`, `tree` and `suggest` are removed, with no aliases, and `template` becomes `ways new`. Authoring is `ways new`, then `ways check` until it is clean. One `ways` skill covers creating, revising, splitting and retiring a way; `ways-tests` and `/sync-to-home` are deleted. `ways update` is the only update path. The only size error is a body over 10,000 chars, the point where Claude Code stops delivering the way in full.
- **Trades away:** single-purpose verbs a script could call one at a time, the `siblings all` corpus matrix as its own command, and warnings on token counts below the delivery cap. `ways reflow` stays as its own verb, because the markdown hook runs it on every markdown file, not only on ways.
- **One-way?** No. It is reversible now: a removed verb can come back as a thin wrapper over `check`. It becomes expensive once adopters script against `ways check --json`.
- **Probes:** *Confident (fewer-commands):* you wanted fewer commands to manage ways; is one `check` verb plus one `ways` skill the right shape? *Not confident (size-shape):* you asked that very large ways get a progressive-disclosure shape; is an error only past the point where Claude Code truncates a way, with the skill teaching the split, enough, or did you want ways flagged well before that point?
- **Inversion:** at one end, many small verbs, each composable, with the author deciding which to run and in what order. At the other, one opaque gate that hides which check failed. This takes one gate whose output names each check, and gives scripts the same result as JSON.

## Context

The ways CLI grew one verb per concern (ADR-111): `lint`, `reflow`, `siblings`, `tree`, `suggest`, `match`, `embed`, `corpus`, `template`. Adding a way and confirming that it matches takes 8 distinct commands. The `/ways-tests` skill wraps them in 14 modes, and five of those modes have no command behind them, so the model computes budgets and crowding by hand. The same flow is documented with conflicting syntax in three places, and several of those references are broken: a flag that does not exist, a `suggest --apply` that was never built, a skill that reads a directory the install does not project. Updating the install has three paths, and one skill serves an install topology the install guide marks superseded.

Size has no check. The authoring way's body is 14,233 chars. Claude Code caps a hook's `additionalContext` at 10,000 chars: past it, the model receives a file path and a 2,000-char preview and is not asked to read the file. A way over the cap is therefore not delivered, which is a defect and not a style concern. ADR-105's token budgets (about 1,200 tokens on a realistic path, about 4,000 for a whole tree) are targets for shaping a tree, and nothing breaks when a way passes them.

Claude Code now has path-scoped rules and skills with `paths:` frontmatter. Its documentation says "Path-scoped rules trigger when Claude reads files matching the pattern, not on every tool use." Only two of 143 ways trigger on a file glob alone, and both must fire when Claude edits a way file, not when it reads one.

## Decision

### 1. One authoring gate

`ways check [path] [-p PROMPT]... [-n PROMPT]... [--fix] [--json]` validates the way, directory or tree at `path`. Path resolution is the one `lint` uses today: project ways inside a project, otherwise global ways, with `--global` to force global. It runs, in order:

1. **Lint:** frontmatter against the schema, pattern hygiene, locale stubs, provenance.
2. **Size:** an error for any way whose body is over 10,000 chars.
3. **Hard-wrap detection:** the detector `ways reflow` uses, run on way files.
4. **References:** every `See Also` target resolves to a way that exists.
5. **Tree:** sibling vocabulary overlap, and the token totals of the tree containing the path.
6. **Corpus:** `corpus --if-stale`.
7. **Prompts:** for each `-p` prompt, whether the way fires under the live matcher (ADR-160), its rank, and the top competitors; for each `-n` prompt, whether it fires when it should not.
8. **Vocabulary:** the gaps `suggest` reports today, for semantic ways.

A check fails only on a defect it can name: a lint error, a body over the cap, hard-wrapped prose, a `See Also` target that does not resolve, a `-p` prompt the way does not fire on, or a `-n` prompt it fires on. Overlap, tree token totals against ADR-105's targets, competitors and vocabulary gaps are reported and do not fail. Exit codes follow the lint convention: 0 clean, 1 defects found, 2 the check could not run. `--json` follows ADR-185. `--fix` applies lint's fixes and repairs hard-wrapped prose, scoped by `path`, and refuses without a path unless `--all` is given, as `lint --fix` does today.

With no embedding model installed, the static checks (1 to 4) still run, and the model-backed checks report that they were skipped. A `-p` or `-n` prompt given with no model exits 2, since the author asked for a result the check cannot produce.

### 2. Verbs removed, and the one that stays

`lint`, `siblings`, `tree` and `suggest` are removed. `template` is renamed `ways new`. None keeps an alias. Every caller of them is in this repository: the CI step `ways lint --check hooks/ways` in `.github/workflows/build-ways.yml`, the optimization way's `macro.sh` (`ways suggest`), and help text in `init.rs`, `template.rs` and `lint`. They change in the same change that removes the verbs.

`ways reflow` stays. Its caller is outside way authoring: the `documentation/markdown/reflow` postcheck pipes every markdown write through `ways reflow --json`, on any `.md` file, and the markdown way teaches `ways reflow --fix <file>` for any markdown. `check` runs the same detector on way files.

This amends the verb list in ADR-111 §1. `ways match` stays as the diagnostic for how a single query fires. `ways embed` is outside this record.

### 3. The authoring flow

Authoring a way is two commands: `ways new <path> -d "..."`, then `ways check <path> -p "..." -n "..."`, repeated until it is clean. Retiring or moving a way is `git rm` or `git mv`, then `ways check` on the tree, which reports any `See Also` that now dangles.

### 4. Skills

- **ways** moves from `commands/ways.md` to `skills/ways/SKILL.md` and is rewritten as the one entry point to create, revise, split or retire a way, project or global. It interviews, runs `ways new`, drafts the way with the operator, and runs `ways check -p/-n`. It carries a short section on reading scores and the remedy loop.
- **Splitting.** When `check` reports a body over the cap, or a way covers two or more sub-topics, the skill teaches the ADR-105 split: a parent with the overview and `See Also`, children with disjoint vocabularies, then `ways check` on the directory. The size error names that remedy.
- **ways-tests** merges into `ways` and is deleted.
- **ways-update** keeps its pre-flight guard (a legacy in-place clone, a dirty tree, diverged history) and then calls `ways update`.
- **ways-localize** stays. Its paths change to the app source under `$XDG_DATA_HOME/agent-ways`.
- **/sync-to-home** is deleted, with the `sync-to-home`, `sync-to-home-link` and `sync-to-home-test` make targets and the two scripts behind them. The subdirectory topology they serve (ADR-140) was replaced by native projection (ADR-142, ADR-144).

Each lifecycle skill relies on Claude Code's native description matching. Each description names what the skill is not for, so the three do not compete.

### 5. One update path

`ways update` is the only way to update the install. `make update` builds `ways` if it is missing and then calls `ways update`. The ways-update skill calls it after its guard.

### 6. Build on Claude Code, not into it

No way moves to `.claude/rules/` or to a skill with `paths:`. Path-scoped rules trigger on reads, and the two glob-only ways must fire on edits. Moving them would also lose re-firing after a way drifts out of the window, the parent boost for children, and the fire telemetry that `stats`, `introspect` and `tune-precision` read. Every other way triggers on prompts, commands or state, which Claude Code has no equivalent for. The `ways` skill gets no `paths:`; the authoring way already fires when a way file is edited and points to the skill.

### 7. Size

`ways check` reports an error only for a way whose body is over 10,000 chars, the delivery cap above. There is no warning on a way's token count. ADR-105's budgets stay authoring guidance, and `check` reports a tree's totals against them without failing. A check names a defect or it reports; it does not warn on taste (ADR-311).

## Consequences

### Positive

- Adding a way goes from 8 commands to 2. The corpus rebuild, overlap and budget run inside `check`.
- The lifecycle skills go from five (`/ways`, `ways-tests`, `ways-update`, `ways-localize`, `/sync-to-home`) to three, each with one job.
- A way too large to be delivered fails its check, and the failure names the split.
- A retired or moved way no longer leaves a dangling `See Also` unnoticed.
- The broken references go with the surfaces that held them.

### Negative

- The authoring way fails the size check when this lands. Its split into a parent and children has to land first or in the same release.
- A script that called a removed verb breaks with no alias period. No such caller outside this repository is known; one that exists finds out from the error.
- `check` is a larger command than any verb it replaces, and its output has to stay readable when several checks report at once.
- The per-way cap does not cover several ways delivered together in one `additionalContext`; a combined delivery over 10,000 chars is still possible and is not checked here.

### Neutral

- ADR-105's text cites `/ways-tests budget` as the validator; `ways check` reports that budget from here on.
- `docs/reference/ways-cli.md`, `docs/hooks-and-ways/`, the authoring and optimization ways, and `frontmatter-schema.yaml`'s header name the new verbs.
- ADR-139 and ADR-183's localization flow is unchanged; only the skill's paths move.

## Alternatives Considered

- **Keep the verbs and fix the documentation.** It repairs the broken references, but leaves 8 commands in the flow and five skill modes with nothing behind them. The operator asked for fewer commands.
- **Keep the removed verbs as hidden aliases for one release.** An alias earns its keep when a caller outside the repository depends on the old name. None is known, and every known caller changes with the verbs, so an alias would only extend the surface.
- **Fold `reflow` into `check` as well.** `reflow` checks any markdown file and is called by a hook on every markdown write. Folding it would move a general markdown tool behind a way-authoring command.
- **Warn on token counts, as well as erroring on the cap.** A warning on a count below the cap names no defect; the way is delivered in full. ADR-105's targets remain in the authoring guidance and in `check`'s report.
- **Move glob-triggered ways to `.claude/rules/` or `paths:` skills.** Rules trigger on reads, and the only glob-only ways need edits. The move would also drop re-firing, parent boost and fire telemetry.
