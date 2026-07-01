---
status: Draft
date: 2026-07-01
deciders:
  - aaronsb
  - claude
related:
  - "[[ADR-142]]"
  - "[[ADR-143]]"
  - "[[ADR-144]]"
  - "[[ADR-145]]"
---

# ADR-147: projectable user config layer

## Context

Before 1.0, this repository *was* the user's `~/.claude` — cloning it in place made
it a version-controlled, deployable Claude Code configuration. In effect the repo
was a **dotfiles repo for Claude Code** that happened to also carry the ways
framework. 1.0 (ADR-142) split those two roles: the framework became an XDG
application, and `~/.claude` became a thin **projection** of it. The reconciler
(ADR-144) projects the framework roots (`skills/`, `agents/`, `commands/`,
`hooks/ways/`, binaries) and treats everything else in `~/.claude` as **user
remainder** — left untouched by construction (ADR-145).

"Left untouched" is correct but incomplete. Users accumulate *their own* Claude Code
config — a statusline command and its script, custom slash commands, output styles,
keybindings, `settings.json` preferences — the little things that make an install
theirs. Today those live only in the live `~/.claude` on one machine. They are not
version-controlled, do not survive a `--dangerously-clobber` reinstall, and do not
travel to a second machine or a fresh Claude Code install. The one thing the pre-1.0
repo did well for its owner — *carry my config so I don't lose it* — 1.0 dropped on
the floor.

A concrete failure made the gap visible: agent-ways' `settings.json` force-injected
a `statusLine` pointing at a repo-root `statusline.sh`, but that script isn't a
projected root, so under the projection it pointed at a file that didn't exist and
the statusline broke. Two lessons: (1) the framework should not force-claim a
**user-scoped** key like `statusLine`, and (2) a user's own config artifacts want to
be *managed and projectable*, not orphaned in one machine's `~/.claude`.

## Decision

Add an optional **user config layer**: a user-owned, version-controllable set of
Claude Code config artifacts that the reconciler **projects into `~/.claude`
alongside the framework** — refresh-safe across updates and deployable to any
machine. This elevates ADR-145's passive "user remainder" into an explicit,
*managed* third source, and re-grants the pre-1.0 repo's original value (carry my
config) without re-coupling it to the framework.

Principles (the affordance, not a mandated structure):

**1. User scope, mirroring the ways precedent.** ADR-143 already gives a user their
own *ways* at `$XDG_CONFIG_HOME/agent-ways/ways/`, projected and update-safe. Extend
the same pattern to *config*: a user-scoped location under
`$XDG_CONFIG_HOME/agent-ways/` holds the user's Claude Code config artifacts. What
lives there is the user's to decide — agent-ways provides the mechanism, not an
opinion about content.

**2. Two kinds of artifact, two projection modes.**
   - *Whole files/dirs* (a `statusline.sh`, a `commands/` tree, `output-styles/`,
     images) project into `~/.claude` the same idempotent way framework roots do
     (symlink/copy via the reconciler).
   - *`settings.json` fragments* are the exception: `settings.json` is a single
     doubly-claimed file (ADR-145's shared-write seam). The user's settings keys are
     **merged** into it, never overwrite it — the same three-way merge the framework
     already uses for its own block, but for the user's keys.

**3. The framework owns only its keys; user-scoped keys belong to the user layer.**
agent-ways' `settings.json` contribution is limited to what it genuinely owns (the
hooks block, its permissions). It must stop force-injecting user-scoped keys
(`statusLine`, `model`, `theme`, `env`, output style, …). Those are either the
user's to set directly or belong in the user config layer. A convenience the
framework *ships* (like a sample `statusline.sh`) becomes something the user *opts
into* by placing it in their layer — not something force-wired and then broken.

**4. One reconciler, three sources.** This is ADR-145's convergence made whole:
`~/.claude` converges toward (CC baseline) ∪ (agent-ways framework manifest) ∪
(**user config layer**). The three are disjoint by construction; the user layer is
projected by the same reconcile pass, so `install` / `update` / `repair` deploy it
for free and it survives a clobber-reinstall (it lives in `$XDG_CONFIG`, not
`~/.claude`).

**5. Non-prescriptive and opt-in.** Absent a user layer, nothing changes — the
reconciler projects only the framework, exactly as today. The layer is an
affordance for the user who wants their config carried; it never imposes a structure
or content.

Deliberately left open for the debate this ADR anchors (not yet decided):

- **Exact location and shape** of the user layer — a single overlay directory whose
  tree mirrors `~/.claude` (convention-by-directory), vs. an explicit manifest of
  what to project. Convention-by-directory is the simpler dotfiles-like model; a
  manifest is more explicit about intent.
- **Declaration of `settings.json` fragments** — a dedicated `settings.user.json`
  the merge folds in, vs. keys placed in the config the loader already reads.
- **Precedence** when the same path exists in more than one source (framework vs
  user layer vs CC baseline) — likely user-over-framework for user-scoped artifacts,
  but the settings merge needs an explicit rule.
- **The audit** of which keys agent-ways currently injects that are actually
  user-scoped and should be de-scoped (statusLine is the first; enumerate the rest).
- **Bootstrapping / capture** — is there a command to *seed* the user layer from an
  existing `~/.claude` (so an adopter's current config becomes carried), mirroring
  how migration seeds other state?

## Consequences

### Positive

- A user's own Claude Code config becomes version-controllable, survives a reinstall,
  and deploys to a new machine or a fresh Claude Code install — the pre-1.0 repo's
  best property, restored without re-coupling config to the framework.
- Fixes the `statusLine` class of bug at the root: the framework stops force-claiming
  user-scoped keys; the convenience script becomes opt-in user config that the
  reconciler actually projects.
- Completes ADR-145: the "user remainder" stops being an untouched blind spot and
  becomes a first-class, managed source the reconciler converges.
- Zero cost when unused — no layer, no change; purely additive.

### Negative

- More surface in the reconciler and the `settings.json` merge (a third contributor
  to reconcile, and user-key merging distinct from framework-key merging).
- A precedence model is now genuinely three-way; conflicts (same path in framework
  and user layer) need a defined, documented rule rather than "framework only".
- Risk of scope creep toward a general dotfiles manager. The guardrail: this carries
  *Claude Code* config projected into `~/.claude`, nothing broader.

### Neutral

- Depends on the `settings.json` three-way merge already implemented for the
  framework block (ADR-145 / reconcile) — extends it rather than inventing new
  machinery.
- Interacts with the de-scoping audit: shipping this is the natural moment to stop
  force-injecting user-scoped settings keys.

## Alternatives Considered

- **Keep "user remainder" untouched (status quo).** Rejected: it drops the pre-1.0
  value (carry my config) and leaves the `statusLine` class of bug — the framework
  force-injects user-scoped keys with nowhere durable for the user's own to live.
- **Carry user config as a way or a skill.** Rejected (this was tried in spirit with
  the injected `statusLine`): ways match into context and skills load at startup, so
  they tax *every* session for *every* user to serve one user's config. Config is
  data to project, not guidance to disclose — the projection layer is the right
  mechanism, and it never loads into a session.
- **A separate, standalone dotfiles tool.** Rejected as the primary path: the
  reconciler already projects into `~/.claude` idempotently and already merges
  `settings.json`; a second tool would duplicate that and re-introduce drift. The
  affordance belongs where the projection already happens.
- **Force a prescribed config structure.** Rejected: the user explicitly wants an
  affordance, not an agenda. Convention or manifest must stay opt-in and content-
  agnostic.
