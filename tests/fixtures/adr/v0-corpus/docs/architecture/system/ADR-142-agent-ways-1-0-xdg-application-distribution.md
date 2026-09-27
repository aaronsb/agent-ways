---
status: Accepted
date: 2026-06-29
deciders:
  - aaronsb
  - claude
related:
  - "[[ADR-140]]"
  - "[[ADR-141]]"
  - "[[ADR-112]]"
  - "[[ADR-128]]"
---

# ADR-142: agent-ways 1.0 — XDG application distribution

## Context

Since its first release agent-ways has held one identity invariant: **`~/.claude`
*is* agent-ways.** You install by cloning the repo into `~/.claude`; the installed
files and the source files are literally the same files. ADR-140 named this the
**in-place topology** and made it the default. Almost everything in the install/update
machinery quietly depends on it: `git pull` (or `make update`) *is* the whole update;
`git status` in `~/.claude` tells the truth; the update checker just runs
`git -C ~/.claude rev-parse`; there is zero drift, because there is nothing to drift
*from*.

That invariant is load-bearing, and it is also the source of every structural problem
1.0 exists to resolve. Three forces have converged:

1. **The application and the user's workspace are the same directory, so we cannot
   safely replace the application.** `~/.claude` holds agent-ways' shipped files (ways,
   skills, hooks, `bin/`) *and* the user's own `settings.json` (model, theme, plugins),
   their sessions, their credentials, and any ways they wrote themselves. Because these
   are interleaved in one tree with no manifest distinguishing them, "update agent-ways"
   cannot mean "replace the shipped files" — it can only mean "git pull and hope the
   merge is clean." This is precisely why **auto-update has never been safe**: today
   out-of-date is a *notification*, never auto-applied, because we cannot tell the
   application's files apart from the user's and so cannot mutate one without risking the
   other.

2. **ADR-140 forked the deployment model but left the fork unresolved.** Faced with the
   adopter who already has a `~/.claude` they value, ADR-140 introduced the
   **subdirectory topology**: clone into `~/.claude/<dir>`, then *project* the outputs up
   into `~/.claude`. It chose **copy** as the projection default (Windows / low-privilege
   robustness) and explicitly recorded **symlink-projection as a rejected default,
   retained as an advanced opt-in** — flagging in its own Neutral consequences that it
   "leaves room for an opt-in symlink mode later without re-deciding the topology
   question." Copy-projection, however, *creates drift as a first-class state* ("pulled
   but didn't re-sync"), which ADR-140 then had to detect and nag about. We now have two
   topologies, two update workflows, two `settings.json` writers, and a drift failure mode
   that exists only because of the copy choice.

3. **"global" conflates the application with the user.** The `ways` runtime today scans
   two root classes — **global** (everything shipped, under `~/.claude`) and **project**
   (per-repo ways). A way the *user* writes and a way agent-ways *ships* land in the same
   global root, indistinguishable. There is no way to shadow a shipped way without editing
   it in place — which the next update then clobbers. The "can't safely update" problem and
   the "can't tell app from user" problem are the same problem viewed from the runtime side.

The underlying realization: **agent-ways has outgrown being a dotfile and become an
application.** Applications that respect their host don't own the user's home directory;
they separate code from config from state from cache, install into well-known locations,
and replace their own code wholesale on update without touching the user's data. The XDG
Base Directory specification is exactly that contract. The cache half of it is already
here — derived corpus, embeddings, and the model live in `$XDG_CACHE_HOME`
(`~/.cache/claude-ways/` today, renamed to `agent-ways/` for one consistent name) and are regenerated, not version-controlled. 1.0 finishes the
job: it makes agent-ways a real XDG application and reduces `~/.claude` to the thin
**projection surface that Claude Code owns**.

This is a 1.0-scale change: it re-decides ADR-140's defaults, restructures the runtime's
root model (→ child ADR-143), and replaces the install/update/repair scripts with a single
reconciler engine (→ child ADR-144). This ADR is the spine; it records the identity shift,
the state taxonomy everything else falls out of, and the lifecycle for getting existing
users there without stranding them.

**A note on who this affects, and how much we know about them.** The migration-relevant
population is specifically the **in-place-clone adopters** — those running a `~/.claude`-*is*-the-repo
install that the migrator (§5, §8) must handle; everyone else is a fresh install. The maintainer
does **not** know that population's exact size. The passive signals available are weak and
ambiguous: roughly 19 stars and ~10 forks; release-asset downloads are low *and* undercounted
because adopters typically build from source (`make ways`) rather than pull a release artifact;
clone traffic is polluted by CI. The honest read is **small, real, and unknown-exact**, with the
forkers (~10+) the best proxy for in-place adopters. The deprecation lifecycle (§8) is therefore
designed to be **population-independent** — its safety must not, and does not, depend on knowing N.

## Decision

**Restructure agent-ways from "a clone that lives in `~/.claude`" into an XDG application
that *projects into* `~/.claude`.** `~/.claude` stops being the application and becomes a
thin, regenerable projection surface; the application, the operator's config, and the
durable session substrate move to their proper XDG homes.

### 1. The state taxonomy (the core decision — durability and repair fall out of it)

The single decision from which everything else derives is **classifying every file agent-ways
touches by its durability and ownership**, and putting each class in the XDG location whose
contract matches:

| Location | Holds | Durability contract |
|---|---|---|
| `$XDG_DATA_HOME/agent-ways/` | **The application** — exactly what is on GitHub: ways, skills, hooks, `bin/`, docs. | Read-only to the user; **replaced wholesale on update**. Losing it is a re-install, not data loss. |
| `$XDG_CONFIG_HOME/agent-ways/` | **The operator's own** ways and macros, plus `ways.json`. | Durable; **never touched by update**. Out of every mutation's blast radius. |
| `$XDG_STATE_HOME/agent-ways/` | **Session substrate** — ledger, memory, focus — that should survive a `~/.claude` wipe. | Durable; survives reinstall/repair. (Boundary vs. Claude-Code-owned state is an open question, below.) |
| `$XDG_CACHE_HOME/agent-ways/` | **Derived** — corpus, embeddings, model. (Exists today as `claude-ways/`; the reconciler renames it.) | Regenerable; safe to delete; rebuilt on demand. |
| `~/.claude/` | **The projection** — a merged `settings.json` (hooks + ways permissions only) and the projected tree. | The irreducible **Claude-Code-owned floor**; regenerable from the manifest. |

The payoff is that **durability and ownership are now structural, not conventional.** "Can we
auto-update?" stops being a judgment call and becomes a lookup: update mutates only
`$XDG_DATA` (replaceable by definition) plus one surgical merge into `settings.json`; it never
goes near `$XDG_CONFIG` or `$XDG_STATE`. Repair regenerates the projection from the manifest;
losing the projection loses nothing. This is the separation of concerns that the single-tree
model made impossible.

### 2. Manifest-driven projection (symlink default, copy fallback, git-derived)

A **manifest** is the contract describing what should exist in `~/.claude` and where each entry
points back into `$XDG_DATA`. The projection is *whatever materializes that manifest* — so the
ADR-140 "copy vs symlink" question stops being a topology fork and becomes an **implementation
detail of materialization**:

- **Symlink (default, Unix).** `~/.claude/hooks → $XDG_DATA/agent-ways/hooks`, etc. With symlinks,
  `git pull` in `$XDG_DATA` makes most updates **live with zero projection step and zero drift** —
  it restores every good property the original in-place topology had, without owning the user's home.
- **Copy (fallback, Windows / low-privilege).** Where symlinks require developer-mode/admin or are
  fragile, the same manifest is satisfied by copying. Drift is reintroduced here, but it is now the
  *exception*, confined to environments that can't symlink, and the reconciler (child ADR-144) detects
  and closes it.

**The manifest is derived from the agent-ways git-tracked file set.** Git is the authoritative answer
to "what does agent-ways ship." This makes the manifest do double duty as the **app-vs-user
disentangler**: a file present in a project-owned directory (e.g. `hooks/ways/...`) but **not
git-tracked-by-agent-ways** is, by set difference, user-scope. The thing we could never compute in the
single-tree model — *which of these files are mine and which are the user's* — is now a `git ls-files`
set difference against ground truth. (Format and mechanics: child ADR-144.)

### 3. Flip the default — supersede ADR-140's defaults (not ADR-140)

**Projection becomes the default, and within projection, symlink becomes the default
materialization.** This **supersedes the *defaults* ADR-140 decided** — its in-place-default and its
copy-default — while leaving ADR-140 *valid as the origin of the two-materialization idea*. ADR-140
explicitly parked "an opt-in symlink mode later, without re-deciding the topology question" in its
Neutral consequences; **this ADR is that future, promoted from opt-in to default.** The in-place model
does not vanish — it becomes the legacy from-state the migrator reconciles away from (§5, child ADR-144).

### 4. One reconciler engine, four from-states

There is **no separate installer, updater, repairer, and migrator.** There is **one state-reconciler**
that drives the live `~/.claude` tree toward the manifest, entered from four different source states:

| From-state | What reconciliation means |
|---|---|
| **fresh** (no prior install) | Materialize the manifest from nothing. |
| **drifted** (projection damaged) | Re-materialize the missing/broken entries. *Repair.* |
| **out-of-date** ($XDG_DATA behind) | Advance $XDG_DATA, re-derive manifest, re-materialize the delta. *Update.* |
| **legacy-in-place** (`~/.claude` *is* an old clone) | Relocate the app to $XDG_DATA, lift user files to $XDG_CONFIG/$XDG_STATE, replace the tree with the projection. *Migrate.* |

Collapsing four tools into one engine is the design's central SOLID claim: the *logic* (converge the
tree to the manifest, idempotently) has **one reason to change**; the four entrypoints differ only in
*starting state and trust posture*, not in mechanism. Detailed engine, manifest format, and bootstrap:
**child ADR-144**.

### 5. The autonomy gradient (the load-bearing safety model)

The same engine runs under **two trust postures, keyed on blast radius** — this is what makes "one
engine for both a silent repair and a destructive migration" safe rather than reckless:

- **update / repair → act silently and autonomously.** They touch only replaceable app-scope
  (`$XDG_DATA` + the regenerable projection) and the surgical `settings.json` merge. Reversible, low
  blast radius, no consent needed — consistent with the framework's silent-on-success convention.
- **migration → a higher tier.** Migration rewrites the user's actual `~/.claude`. It must: **back up
  the whole `~/.claude` first**; **announce loudly** (with the backup path); **gate the first run behind
  explicit consent** (a `migrate` skill / installer invocation — never a silent SessionStart side
  effect); and be **crash-safe and resumable** (marker-driven phases, back-up-before-mutate, atomic
  moves). A half-migrated `~/.claude` is worse than an un-migrated one; the kept backup is the escape
  hatch behind "things are broken."

### 6. Manifest check on every SessionStart (escalating, silent-on-success)

A SessionStart manifest check, mirroring the framework's existing escalation convention:

- **all good → silent.** (As `ways init` / check-setup are silent today when healthy.)
- **fixable drift → repair, then emit "X was repaired."**
- **unfixable → emit "things are broken" loudly**, pointing at the backup / re-install path.

The chicken-and-egg this raises (if hooks are gone, nothing runs to repair them) and its resolution
(the self-sufficient bootstrap hook, and why that one entrypoint must be *exempt* from the breakage it
repairs) are **child ADR-144's** core problem. The one cross-cutting fact the spine must state: because
`settings.json` is read once at session start, a repaired hook **takes effect next session** — so the
first post-install run is a one-time **bootstrap → "start a new session to pick up agent-ways" →
restart** flow.

### 7. Auto-update, finally safe — decomposed

Auto-update was unsafe because *any* update risked clobbering user customizations. The taxonomy removes
the risk by construction:

- User ways live in `$XDG_CONFIG` — **never in the blast radius** of an update.
- Under symlink projection, `git pull` in `$XDG_DATA` makes most updates **live with zero projection**.
- The **only** mutation to a user-owned file is the **surgical `settings.json` merge** (hooks block +
  ways permissions). That single shared-write seam is the entire remaining risk surface — and it is
  named, bounded, and testable rather than diffuse.

Therefore the stance flips: **out-of-date becomes auto-applied** (silently, per the gradient), not a
notification the user must act on. This retires ADR-140's drift-nudge machinery for the default
(symlink) path; it survives only as the copy-fallback's exception handling.

### 8. Deprecation lifecycle (concrete, and population-independent)

Existing in-place users must reach the new architecture without being stranded. The lifecycle is
designed so that **the absolute size of the adopter base is irrelevant to its safety** — the maintainer
does not know N exactly (small, real, unknown-exact; see Context), and the design must not depend on
knowing it.

The mechanism that makes this work: **1.1.0 is a per-user, update-gated transition, not a global flag
day.** Each in-place user encounters the 1.1.0 "last migration release" gate **only when *they* update
to it, on their own clock** — there is no calendar date at which anyone is cut off.

- **1.0** — new architecture ships; the migrator (legacy-in-place from-state) goes live. Migration path
  first viable.
- **1.0.x** (1.0.1 … 1.0.N, N may be large) — migration supported throughout. The in-place check and the
  assisted migrator run for everyone still in-place.
- **1.1.0** — the **final** migration release. *When a user updates to it*, it migrates them (or confirms
  they already are) and announces that the *next* release will no longer check or migrate: from 1.1.0's
  successor on, ensuring `~/.claude` is how you want it becomes the user's responsibility, and agent-ways
  manages only itself under the concern-separated architecture.
- **post-1.1.0** — XDG-only; no in-place checking or migration. We distinguish **"remove the migrator"**
  (stop assisted migration and the in-place check) from **forcibly breaking anyone still in-place**: the
  design must not strand un-migrated users — removing assistance is not the same as actively breaking them.

Two properties fall out of the per-user gate, and they are the whole point:

- **"Remove the migrator" is safe by construction, not by hope.** Releases are sequential, so a user
  cannot reach a post-1.1.0 release without having passed *through* 1.1.0's gate — which migrated them on
  the way. By the time the migrator is gone, everyone who got there was migrated *en route*. Safety never
  depends on "did everyone migrate by some date"; the update path itself enforces it. This is exactly why
  N is irrelevant.
- **A user who never updates past 1.1.0 stays frozen at 1.1.0 — working but unsupported.** This is an
  **explicitly acceptable outcome**, not a failure: their in-place install keeps functioning; they simply
  stop receiving updates and assisted migration. Nobody is stranded by a date — they self-select out by
  declining to update, and what they decline into is a working frozen version.

## Consequences

### Positive

- **Auto-update becomes safe and can be turned on**, because durability is now structural: the only
  user-owned write is one bounded `settings.json` merge, and user ways are physically outside the blast
  radius.
- **App, config, state, and cache are separable for the first time** — clean separation of concerns. A
  `~/.claude` wipe loses nothing durable; a re-install is a manifest re-materialization; corrupted cache
  is regenerated.
- **The copy-vs-symlink fork collapses** into one manifest with two materializations; symlink restores
  the original in-place model's zero-drift, zero-step update without owning the user's home.
- **Users can shadow a shipped way** (via `$XDG_CONFIG`, child ADR-143) instead of forking it in place and
  losing the edit on the next update.
- **Four lifecycle tools collapse into one reconciler** with a single reason to change; the four
  entrypoints are starting-state + trust-posture, not duplicated logic.
- **The app-vs-user question becomes computable** — a `git ls-files` set difference against ground truth,
  not a heuristic.

### Negative

- **The `settings.json` merge is a genuine shared-write seam and the design's leakiest boundary.** Two
  writers (Claude Code owns the file; agent-ways surgically merges hooks + ways permissions) write one
  file agent-ways does not own. This is the one place the otherwise-clean separation of concerns breaks,
  and it is exactly where a botched merge can corrupt a user-owned file. It must be idempotent,
  key-scoped (touch only hooks + ways permissions), and backed up before write. Calling it out honestly:
  this seam is the residual risk the whole taxonomy could not eliminate, only shrink.
- **Migration is the highest-risk operation agent-ways has ever performed** — it rewrites the user's home
  config tree. Crash-safety, resumability, and a full backup are mandatory, not optional; a half-migrated
  `~/.claude` is worse than not migrating. The risk is real even with the gradient.
- **More moving parts and more locations.** A user debugging their setup now reasons across four XDG roots
  plus the projection, rather than one directory. `git status` in `~/.claude` no longer tells the truth;
  the truth is in `$XDG_DATA`. This is a real loss of the in-place model's one-directory legibility.
- **Symlink-vs-copy divergence persists at the edges.** Windows / low-priv installs still get copy, still
  get drift, and still need the exception-handling path — the fork is narrowed to a fallback, not removed.
- **Bootstrap is a known race with a one-time restart UX cost** (§6); the first post-install session is a
  bootstrap-then-restart, which is friction at the worst possible moment (first impression).
- **A long 1.0.x tail of dual-mode support.** For the whole 1.0.x window we maintain both the in-place
  check / migrator and the XDG runtime — the very "reason about it twice" cost ADR-140 already flagged,
  extended across the deprecation window.

### Neutral

- `$XDG_CACHE_HOME/agent-ways/` already exists today as `claude-ways/` and already behaves as
  derived/regenerable state; 1.0 ratifies the convention rather than inventing it, and **harmonizes the
  name to `agent-ways`** so all four XDG tiers share one application name. The rename
  (`claude-ways` → `agent-ways`) is a one-time move the reconciler performs; because the tier is
  regenerable, a missed rename costs only a rebuild.
- The session ledger (ADR-112) and the KG evidential backend (ADR-141) become tenants of `$XDG_STATE`;
  their "survive a `~/.claude` wipe" expectation is exactly what the state tier provides. Memory routing
  (ADR-128) is unaffected in intent but its files' XDG-vs-Claude-Code ownership is an open question.
- The two-topology framing of ADR-140 is folded into a one-manifest / two-materialization framing; ADR-140
  is superseded only on its *defaults*, and remains the cited origin of the materialization choice.

## Alternatives Considered

- **Keep the single-tree model; make update smarter.** Try to teach the updater to diff and preserve user
  edits inside the shared tree. Rejected: this is the status quo's unwinnable problem — without a manifest
  grounding "what we ship" in git truth, app and user files are indistinguishable, so a "smart" merge is a
  heuristic that will eventually clobber something. The taxonomy makes the distinction structural instead
  of heuristic.
- **Copy-projection as the default (ADR-140's choice), just better instrumented.** Rejected as the
  *default*: copy makes drift a permanent first-class state, which then needs continuous detection and
  nagging. Symlink eliminates drift on the platforms that can symlink (the majority), so copy is correctly
  the *fallback*, not the default. ADR-140's instinct was right for its Windows-first rollout framing; 1.0
  re-weights for the whole population.
- **Put everything (app + config + state) under one new `~/.agent-ways/` directory.** Rejected: it would
  re-create the single-tree conflation in a new location and ignore the XDG contracts that *exactly* encode
  the durability classes we need. The value is in the *separation*, not in merely moving out of `~/.claude`.
- **Stay in-place forever; never migrate.** Rejected: it permanently forecloses safe auto-update and
  shadowable user ways, which are the two capabilities driving 1.0. But its *spirit* is honored in the
  deprecation lifecycle's "don't strand un-migrated users" clause.

## Open Questions

These are recorded deliberately undecided; they do not block the Draft but must be resolved before
Accepted:

- **`$XDG_STATE` vs. Claude-Code-owned boundary.** Which of sessions / memory are Claude-Code-owned (and
  stay in `~/.claude`) vs. agent-ways-owned (and move to `$XDG_STATE`)? The ledger and focus are clearly
  ours; auto-memory (ADR-128) sits in Claude Code's `projects/<slug>/memory/` and may not be ours to move.
- **Exact length of the 1.0.x migration window** before 1.1.0 retires assisted migration.
- **Whether to add a lightweight, privacy-respecting adoption signal** so future lifecycle decisions
  aren't blind. Note this is **explicitly not a prerequisite**: passive GitHub signals (forks, release
  downloads) already exist, and the per-user-gated deprecation (§8) is population-independent by design —
  so telemetry would only inform *future* judgment calls, never gate the safety of *this* lifecycle. Any
  such signal must clear the project's own anti-surveillance bar before it's worth adding.
- **Whether the two children stay separate ADRs or fold into this spine** — see ADR-143 / ADR-144;
  current recommendation is to keep them separate (below).
- Bootstrap-exemption mechanism and Windows materialization specifics are parked in **child ADR-144**.

## Amendment (2026-07-05): source-pinned deploys — `ways update --ref`

§4's **out-of-date → update** from-state advances `$XDG_DATA` to the *latest
release* on the tracked branch and refreshes binaries download-first (§7). A
development need sits alongside it: deploying an **unpublished ref** — a feature
branch, a tag, or a bare commit — onto a live install to dogfood it, without
publishing to `main` and cutting a release.

`ways update --ref <branch|tag|sha>` serves that need as a variant of the update
from-state, with three deliberate differences from the release-channel path:

- **Fetch + detached checkout of the ref**, not a pull of the tracking branch.
  The checkout lands on a detached HEAD at the ref; `ways update --ref main`
  returns to the release channel.
- **Build the whole suite from source** (`ways-rebuild` and the sibling
  `*-rebuild` targets, plus way-embed's source target) — an unpublished ref has
  no pre-built release binary to download, so download-first (§7) does not apply.
- **The ADR-150 downgrade guard is bypassed** (see that ADR's amendment): pinning
  an explicit ref is a deliberate choice, not a channel update, so "never move
  backward" is not the right invariant.

The reconciler tail is unchanged: after the source build it runs the same relink
+ corpus regen + `ways reconcile` projection as any other update, so the trust
posture (§5: app-scope plus the one `settings.json` merge) is identical. This
stays a developer/dogfooding affordance layered on the update from-state, not a
fifth reconciler mode.
