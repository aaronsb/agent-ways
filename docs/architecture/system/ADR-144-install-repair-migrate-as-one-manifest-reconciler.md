---
status: Accepted
date: 2026-06-29
deciders:
  - aaronsb
  - claude
related:
  - "[[ADR-142]]"
  - "[[ADR-140]]"
  - "[[ADR-133]]"
  - "[[ADR-128]]"
---

# ADR-144: Install / repair / migrate as one manifest reconciler

## Context

This is a child of ADR-142 (agent-ways 1.0). The spine declared **one reconciler engine,
four from-states** (fresh / drifted / out-of-date / legacy-in-place) and an **autonomy
gradient** (silent for update/repair, gated-and-loud for migration). This ADR records the
engine: the **manifest** it converges toward, the **bootstrap** that lets it run at all, and
the **migrator** plus its deprecation lifecycle.

The current reality is several scripts with overlapping jobs:

- `install.sh` — clone-in-place or `--dangerously-clobber` (ADR-140 §5).
- `scripts/update.sh` / `make update` — `git pull` for the in-place topology.
- `scripts/sync-to-home.sh` — the subdirectory projector. It already does most of what the
  engine needs: it has **both `copy` (default) and `symlink` modes** (`SYNC_MODE`, lines
  11-33), projects trees idempotently (`project_tree`, lines 68-90, which already skips when a
  link/dir already resolves to source), backs up before overwrite (lines 47-55), merges
  `settings.json` for hooks + ways permissions only (lines 167-198), and writes a source marker
  + synced-HEAD stamp (`.claude-source`, lines 189-199).
- `hooks/check-config-updates.sh` — the out-of-date detector that runs at session start.

Two facts from the real script are load-bearing for this ADR:

1. **A manifest already exists** — `.claude-source-manifest` (lines 143-165). In copy mode the
   script enumerates the source subtrees it projects (`build_manifest`, lines 144-156:
   `skills`, `agents`, `commands`, `hooks/ways`, two named hooks, four named binaries), diffs
   against the previous manifest, and **prunes only orphans it previously created** — explicitly
   leaving user-owned files in the same shared dirs untouched ("User-owned files in the same
   shared dirs are never in the manifest, so they are never touched," line 141). The
   app-vs-user disentangling ADR-142 frames as the manifest's "double duty" is **already the
   stated rationale of this code** — but the manifest is built by *filesystem enumeration of
   hardcoded source subtrees*, not from git.
2. **The manifest is copy-mode-only.** Symlink mode "reflects source live — no orphans" (line
   142), so it writes no manifest. There is today no single artifact that describes the desired
   `~/.claude` state across *both* materializations.

So the engine is less a green-field build than a **consolidation and a grounding correction**:
unify the scripts into one reconciler, lift the manifest from a copy-mode pruning side-effect to
*the* projection contract, and re-base it on git truth.

## Decision

**One idempotent reconciler that converges `~/.claude` toward a git-derived manifest, plus a
self-sufficient bootstrap that is exempt from the breakage it repairs, plus a gated crash-safe
migrator with an explicit deprecation lifecycle.**

### 1. Manifest format, derived from the git-tracked file set

The manifest is the list of projection entries — for each, the relative path in `~/.claude` and
the source path in `$XDG_DATA` it points back to. It is **derived from `git ls-files` in
`$XDG_DATA/agent-ways`**, not from filesystem enumeration of hardcoded subtrees. This is the
correction to today's `build_manifest`:

- **Git is the authoritative "what we ship."** A way/skill/command added or removed upstream is
  reflected automatically; the hardcoded `for t in skills agents commands hooks/ways` list
  (line 146) stops being a thing anyone can forget to update.
- **The set difference becomes exact.** "What is app vs. user" is `git ls-files` (app) vs. what's
  present in the shared dirs (everything); the complement is user scope. Today's manifest gets
  the *spirit* right (don't touch what we didn't write) but its membership test is "did a prior
  projection write it," which is weaker than "is it git-tracked-by-agent-ways."

Both materializations (symlink default, copy fallback — ADR-142 §2) are driven from the *same*
manifest; symlink mode now also has a manifest (it just materializes entries as links), closing
the "no single desired-state artifact" gap.

### 2. The four from-states are one convergence with different entry conditions

The engine computes (desired = manifest) vs. (actual = live `~/.claude`) and applies the delta
idempotently. The four from-states differ only in *starting actual* and *trust posture*:

- **fresh** — actual is empty → materialize all entries.
- **drifted** — actual is missing/broken entries → re-materialize just those (repair).
- **out-of-date** — `$XDG_DATA` HEAD advanced → re-derive manifest, materialize the delta, prune
  orphans (the existing `comm -23` orphan logic, lines 159-163, generalized).
- **legacy-in-place** — actual *is an old clone* → the migrator (§4) first relocates, then this
  same convergence runs.

Idempotence is already the discipline in `project_tree` (skip when already linked/resolved, lines
75-85); the engine generalizes it to every entry and to the prune step.

### 3. Bootstrap and the exemption (the engine's hardest problem)

**Chicken-and-egg:** the reconciler runs from a SessionStart hook; if the hooks are gone, nothing
runs to repair them. Resolution:

- The **installer detects Claude Code and writes the minimum self-sufficient hook** into
  `~/.claude` — the irreducible Claude-Code-owned floor (ADR-142's `~/.claude` row). On SessionStart
  that hook invokes the manifest check/repair.
- **CRITICAL exemption: the bootstrap/repair entrypoint must not be a symlink into the tree it
  manages.** If the healer is itself a link into `$XDG_DATA`, a broken `$XDG_DATA` (or a wiped link)
  takes out its own healer. So the bootstrap entrypoint is **the one deliberate copy** (a small
  copied script) *or* a call to a binary at a **stable absolute XDG path** — never a link into the
  managed tree. This is a deliberate, named exception to the symlink default, justified precisely
  because it is the thing that repairs the symlinks.
- **The restart seam.** `settings.json` is read once at session start, so a repaired hook **takes
  effect next session**. The first post-install session is therefore a one-time **bootstrap →
  "start a new session to pick up agent-ways" → restart** flow. The reconciler must make this
  legible (say it plainly), not silently leave the user in a half-armed session.

### 4. SessionStart check — escalating, silent-on-success

Mirroring the framework's existing convention (and today's silent `ways init` / `check-setup`):

- all entries satisfied → **silent**;
- fixable drift → repair autonomously, emit **"X was repaired"**;
- unfixable → emit **"things are broken"** loudly, pointing at the backup / re-install path.

This replaces `check-config-updates.sh`'s in-place-only detection with a manifest check that works
across all materializations; the in-place git classifier survives only for the legacy from-state
during the deprecation window (§5).

### 5. The migrator and its deprecation lifecycle

Migration (legacy-in-place → XDG) is the **higher trust tier** (ADR-142 §5): it rewrites the user's
actual `~/.claude`.

- **Gated, not silent.** Entered only via an explicit `migrate` skill / installer invocation — never
  a SessionStart side effect.
- **Back up first, announce loudly** with the backup path (the existing pre-overwrite backup, lines
  47-55, generalized from "the projected dirs" to "the whole `~/.claude`").
- **Crash-safe and resumable.** Marker-driven phases, back-up-before-mutate, atomic moves. A
  half-migrated `~/.claude` is worse than an un-migrated one; on crash the next run resumes from the
  last completed phase, and the backup is the escape hatch.
- **Migration ground truth is git.** What to *relocate to `$XDG_DATA`* (the app) vs. *lift to
  `$XDG_CONFIG`/`$XDG_STATE`* (the user's ways, sessions, settings) is the same `git ls-files` set
  difference as §1: app files are git-tracked-by-agent-ways; everything else in the old clone is the
  user's and must be preserved, not replaced.

**Deprecation lifecycle** (concrete, per ADR-142 §8). The governing property: **the transition is
per-user and update-gated, not a global flag day.** A user meets the 1.1.0 gate only when *they* update
to it, on their own clock — so the engine never needs to know how many in-place adopters exist (the
maintainer doesn't; ADR-142 Context). The migrator is invoked *by an update reaching the user*, not by a
date reaching the population.

The lifecycle is an **escalating-pressure curve across the 1.0.x line**, dormant at first so adopters
meet the migrator before they're pushed by it:

- **1.0.0** — the migrator ships **functional but dormant**: `ways migrate` (plan / what-if / execute)
  is available on demand, with **no SessionStart pressure**. The release exists to make the path real
  and prove it end-to-end; updating to it is idempotent-in-use to 0.9.x (the runtime behaves identically
  until the operator chooses to migrate). This is the verify-before-you-push release.
- **1.0.1 … 1.0.5** — the migrator stays available while a SessionStart nudge **escalates each release**,
  keyed to version and **silent for already-migrated installs**: a gentle offer at 1.0.1 ("agent-ways can
  migrate — want to?") → progressively more insistent → at **1.0.5**, the last-chance warning that the
  *next* release drops the migrator and that migrating after this point means checking out the 1.0.5 tag.
- **1.1.0** — migrator and in-place check **removed**. Safe by construction: releases are sequential, so
  reaching 1.1.0 means having passed through the escalation. Two properties make the removal humane:
  - **The escape hatch is the immutable tag.** A user still in-place migrates with
    `git clone --branch ways-v1.0.5 … && ways migrate` — the migrator lives *forever* at that tag, so
    "we removed it" never means "you can't." This is the concrete form of the never-strand clause.
  - **The cliff is a ramp.** "Remove assistance" is not "break the un-migrated." A 1.1.0 binary on an
    un-migrated `~/.claude` still **reads correctly** through the transition fallbacks — core ways from
    the clone's own `hooks/ways`, cache from the legacy `claude-ways`, events from `~/.claude/stats`. It
    loses assisted migration and in-place *checking*, not *function*. A user frozen pre-1.1.0 stays
    working and unsupported by their own choice; a user who updates *to* 1.1.0 while in-place keeps
    running via the fallbacks. The design never actively breaks them.

(The specific patch numbers above are the *cadence*, not a contract — the mechanism is "pressure level
keyed to release, escalating to a final 1.0.x before 1.1.0," and any 1.0.x spacing satisfies it.)

## Consequences

### Positive

- **One engine, one reason to change.** The convergence logic lives once; install/update/repair/
  migrate become entry conditions, not four scripts to keep in sync (the SOLID payoff ADR-142 §4
  claims, realized here).
- **The manifest stops being a copy-mode afterthought** and becomes the single desired-state contract
  for both materializations, git-grounded — which is also what makes the app-vs-user split exact
  rather than "whatever a prior sync happened to write."
- **Repair is real and idempotent**, reusing the already-idempotent `project_tree` discipline; a
  damaged projection self-heals at the next SessionStart, silently when it can.
- **Migration is auditable and recoverable** — gated, backed up, resumable — rather than a one-shot
  destructive script.
- Much of this is **consolidation of code that already exists and works** (copy/symlink modes,
  backup, settings merge, orphan prune, marker/stamp), lowering implementation risk.

### Negative

- **The bootstrap exemption is a permanent, deliberate violation of the symlink default** — a copied
  (or stable-path-binary) entrypoint that must be kept in sync with the engine it launches by some
  *other* mechanism than the manifest, because it can't be a managed link. If that copy goes stale,
  the healer and the thing it heals disagree. This is irreducible complexity, not an oversight.
- **The git-derived manifest assumes `$XDG_DATA` is a clean agent-ways checkout.** If a user has
  hand-edited core files in place (exactly what pre-1.0 forced them to do to customize), those edits
  are now "drift from git truth" and the reconciler may revert them — the migrator must detect and
  rescue such edits into user scope, or it silently destroys the customization it was meant to
  preserve. This is the migration's sharpest edge.
- **Migration crash-safety is genuinely hard to get right** and easy to get subtly wrong; resumable
  marker-driven phases over a directory the user also has open in live sessions is a concurrency
  problem, not just a sequencing one.
- **The `settings.json` merge remains the shared-write seam** (ADR-142 Negative) and is the one engine
  output that touches a user-owned file; it must be idempotent and key-scoped or the engine corrupts
  the very floor it depends on.
- **A long dual-mode window:** the in-place git classifier and the manifest reconciler both ship for
  all of 1.0.x.

### Neutral

- `.claude-source` (marker) and `.claude-source-manifest` already exist (ADR-140 observability); this
  ADR re-bases the manifest on git and extends it to symlink mode, but the marker mechanism is reused,
  not invented.
- The session substrate this engine must preserve across migration (ledger, focus, and possibly memory)
  is defined by ADR-142's `$XDG_STATE` boundary open question and ADR-128's memory ownership — the engine
  inherits whatever that boundary resolves to.

## Alternatives Considered

- **Keep separate install / update / sync / repair scripts; just fix the manifest.** Rejected: the four
  scripts already duplicate backup, projection, and settings-merge logic three ways (`install.sh`,
  `update.sh`, `sync-to-home.sh`), which is how the broken settings-merge shipped in #182 (ADR-140) —
  duplicated lifecycle logic is the defect generator. One engine is the structural fix.
- **Make the bootstrap hook a symlink like everything else** (no exemption). Rejected: a broken target
  takes out the healer; the entrypoint that repairs the tree cannot live inside the tree it repairs.
- **Keep the filesystem-enumeration manifest** (today's `build_manifest`). Rejected: it requires a
  hand-maintained subtree list, and its "is it ours" test ("did a prior sync write it") is weaker than
  git truth — it can't classify a file the user dropped into a shared dir that happens to match a name we
  later ship. Git ls-files is the authoritative membership test.
- **Auto-migrate silently at SessionStart** (no gate). Rejected outright by ADR-142's autonomy gradient:
  rewriting the user's home config without consent and without a loud backup announcement is exactly the
  blast radius the gradient exists to prevent.

- **Bootstrap via a thin marketplace plugin.** *Viable alternative, not the chosen approach* — recorded
  here so the reviewer can weigh it against §3's self-sufficient copied/stable-path entrypoint, which
  stands. (Research-grounded: Claude Code plugins-reference plus adversarial cross-verification of the
  named issues below.)

  - **The crux is documented to work.** A plugin can ship a `SessionStart` hook that auto-fires the
    moment the plugin is *enabled*, with **no `settings.json` edit by the user** (plugins-reference; the
    shipped `explanatory-output-style` plugin is an existing example of an auto-firing plugin hook). The
    hook file lives in Claude-Code's managed plugin cache
    (`~/.claude/plugins/cache/<marketplace>/<plugin>/<version>/`), which is **independent of agent-ways'
    own `~/.claude` projection** — a different tree, owned by a different system.

  - **Effect on the bootstrap exemption: it *relocates* the exemption, it does not eliminate it.** Because
    the hook is a real file in a Claude-Code-managed cache (loader-guaranteed to exist, *not* a symlink
    into the projected tree agent-ways manages), it satisfies §3's exemption requirement by construction.
    The burden of "guarantee a stable entry point" shifts **from agent-ways** (a copied file we keep in
    sync) **to Claude Code's plugin loader**. That is a genuinely cleaner exemption — but it is a
    **dependency on a third-party surface, not the removal of the dependency.**

  - **Corrected risk-ranking — the exemption is *not* the headline risk here.** The headline risk is
    **coupling a load-bearing bootstrap to Anthropic's plugin subsystem, which agent-ways does not control
    and which is visibly churning**: an ephemeral, version-stamped `CLAUDE_PLUGIN_ROOT` (#15642); a
    stale-marketplace-clone update bug (#35752); repeated Windows symlink regressions
    (#23819 / #24140 / #53948 / #50052); and an uninstall path that deletes the plugin *and* its
    `CLAUDE_PLUGIN_DATA` — which would make the projection persist while its **healer vanishes**. A
    secondary facet is *fit with the official model*: plugins are meant to *contribute artifacts*, not
    *mutate `~/.claude`*. A bootstrap hook that symlinks and merges `settings.json` into `~/.claude` is
    **technically allowed** (hooks run unsandboxed at full user privilege) but is **off-label** use of the
    plugin contract.

  - **Three hard constraints if pursued:** (1) the bootstrap must resolve `$XDG_DATA` **independently** and
    treat `CLAUDE_PLUGIN_ROOT` as ephemeral/throwaway (it rotates every plugin update); (2) any durable
    bootstrap state goes to `$CLAUDE_PLUGIN_DATA` (survives updates) or `$XDG_STATE` — **never**
    `CLAUDE_PLUGIN_ROOT`; (3) the plugin must be a genuinely self-contained **thin shim** (the hook plus a
    small launcher only). Plugins deliberately skip outside-pointing symlinks for security, so the shim
    **cannot symlink-bridge into `$XDG_DATA`**; and the binaries / model / corpus do not fit git-clone
    plugin delivery — so **"ship the whole app as a plugin" is not viable** (wrong shape), full stop.

  - **When it earns its keep — and when it doesn't.** The real payoff is **discoverability**:
    `claude plugin install agent-ways@aaronsb` via a self-published single-plugin marketplace, riding
    Anthropic's managed install channel, with the exemption-maintenance shift as a *bonus*. For the
    exemption *alone* it is a **bad trade** — standing up a whole marketplace plus lifecycle coupling to
    replace one copied file. It implies a **two-channel update model**: the thin plugin updates rarely via
    the marketplace, while the bulk app updates independently via the reconciler / `git` in `$XDG_DATA`.
    This is adjacent to **ADR-133**, which already shells `claude plugin list --json` at `SessionStart` to
    discover plugin-contributed ways — so the framework already touches the plugin surface, just for
    discovery rather than bootstrap.

## Open Questions

- **Bootstrap-exemption mechanism: copied script vs. stable-path binary call.** Both satisfy "not a link
  into the managed tree"; which is chosen affects how the entrypoint is kept current. Parked per ADR-142.
- **Windows materialization specifics** — copy fallback details, developer-mode requirement, path quoting
  (the #182 author already started this); how the reconciler detects "can't symlink here" and falls back.
- **Rescue policy for hand-edited core files** found during migration (revert to git, or lift the diff
  into user scope as a shadow — see ADR-143's user root).
- **Exact 1.0.x window length** before 1.1.0 (shared with ADR-142).
- **Whether this child stays separate or folds into ADR-142.** Recommendation: keep separate — the
  bootstrap exemption and the crash-safe migrator are each substantial enough to warrant their own
  recorded decision and their own Consequences.
