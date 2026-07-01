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

There is a second asymmetry the "left untouched" framing misses: `~/.claude` is not a
passive projection *sink*. Claude Code **configures itself** — the user subscribes to a
skill, a capability writes a statusline or an output-style, a slash command is added,
a `settings.json` key is flipped — and that lands *live* in `~/.claude`, authored
through CC rather than by hand in any store. One-way projection (store → `~/.claude`)
would either clobber that live config or let the durable store rot: config the user
genuinely acquired through CC would never make it back to the place that survives a
reinstall or travels to a second machine. So the store and `~/.claude` are two
authoring surfaces for the same user-scoped config, and the relationship between them
is **two-way**, not a one-directional deploy.

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

**5. Two-way, on the git model — deploy and capture, both explicit.** Because CC
authors user-scoped config live in `~/.claude`, the user layer syncs in both
directions: *deploy* (store → `~/.claude`, the reconcile pass above) and *capture*
(`~/.claude` → store, sweeping live user-scoped artifacts back into the durable
store). Both are **explicit, user-invoked** operations — like `git push`/`pull`, not
a background daemon reconciling continuously; continuous bidirectional sync is where
dotfiles managers drown in conflict resolution and "which machine is authoritative."
Capture is not new machinery: it is the ADR-145 `settings.json` three-way merge run
with `~/.claude` treated as an authoring *source* — track a last-synced base, diff
live against store, three-way merge, surface genuine conflicts. Where whole files
project as symlinks, editing the live file already writes the store (same inode) and
no capture is needed; capture bites only where symlinks can't reach — the *merged*
`settings.json` and fresh artifacts CC creates that the store has never seen.

The last-synced base is what keeps this legible. Everything that changed on only one
side since base auto-resolves silently; only a *genuine* conflict — the same key or
file changed on **both** sides since base — needs a human/agent call. No single
hardcoded rule ("latest wins", "richest wins", "store wins") resolves those reliably:
a deploy pass rewrites `~/.claude` mtimes so "latest" mis-reads, "richest" resurrects
keys a user deliberately pruned, and "store wins" discards CC's self-configuration —
the very thing capture exists to keep. So conflicts **surface** through the
framework's own disclosure machinery rather than being auto-resolved: a **macro**
computes the divergence signals at trigger time (which side is newer, which is richer,
what actually differs), a **way** guides the operator and Claude through the choice,
and a **skill** lets Claude drive the capture/deploy/merge. The heuristics become a
*suggested default* for the rare true conflict, never a silent verdict — straightforward
for both the operator and Claude to decide.

**6. Non-prescriptive and opt-in.** Absent a user layer, nothing changes — the
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
- **The audit runs both ways.** *De-scoping:* which keys agent-ways currently injects
  are actually user-scoped and should be un-claimed (statusLine is the first;
  enumerate the rest). *Classifying for capture:* the inverse — of the artifacts
  living in a real `~/.claude`, which are the user's to carry vs framework-owned vs
  ephemeral? The `skills/` case is the sharp one: a CC-subscribed skill is brand-new
  content the store never had, and capture must decide it is user-scoped.
- **Capture mechanics** — capture is ongoing, not a one-time seed (principle 5); the
  first capture *is* the seed. Open: the merge base for detecting real conflicts (a
  stored last-synced snapshot vs. content hashing), and whether capture is a distinct
  verb (`ways config capture`) or a `--capture` mode of reconcile.
- **Conflict surfacing (macro + way + skill).** True conflicts surface rather than
  auto-resolve (principle 5). Open: which signals the macro should compute and how it
  presents them; whether the guiding **way** is new or an extension of the existing
  deploy/migration ways (review those so this *composes* rather than duplicates); and
  the shape of the **skill** that lets Claude execute the chosen resolution. The
  heuristics (latest / richest / store-wins) are inputs the macro *reports*, not a
  rule the code silently applies.

## Consequences

### Positive

- A user's own Claude Code config becomes version-controllable, survives a reinstall,
  and deploys to a new machine or a fresh Claude Code install — the pre-1.0 repo's
  best property, restored without re-coupling config to the framework.
- Config CC *authors itself* (a subscribed skill, a capability-written statusline) is
  captured back to the durable store rather than being clobbered on the next deploy or
  lost on reinstall — the two-way sync closes the loop between CC's self-configuration
  and the store.
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
- Two-way sync adds a capture direction with its own conflict case (store and
  `~/.claude` both changed since the last sync). The explicit-pull / git model bounds
  this — conflicts surface at an invoked `capture`, never silently in the background —
  but a merge-base and conflict-presentation model still has to be designed.
- Risk of scope creep toward a general dotfiles manager, sharpened by the two-way
  sync (that is precisely what dotfiles managers do). The guardrail holds: this
  carries *Claude Code* config projected into `~/.claude`, nothing broader, and sync
  stays explicit rather than becoming a continuous reconciling daemon.

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
