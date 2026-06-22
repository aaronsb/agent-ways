---
status: Accepted
date: 2026-06-22
deciders:
  - aaronsb
  - claude
related:
  - "[[ADR-138]]"
  - "[[ADR-139]]"
---

# ADR-140: Two install topologies: in-place repo and subdirectory projection

## Context

Since its first release agent-ways has held one identity invariant: **the repo
*is* your config.** `~/.claude` is the git clone. That invariant is load-bearing
and almost everything in the install/update machinery quietly depends on it:

- `git pull` (or `make update`) *is* the whole update — files are updated in place.
- `git status` in `~/.claude` tells the truth, because the installed files and the
  source files are the same files.
- The update checker (`hooks/check-config-updates.sh`) just runs
  `git -C ~/.claude rev-parse` and classifies the result (clone / fork /
  renamed_clone / plugin) to decide whether to nudge.
- There is **zero drift** — nothing can be "installed but stale" relative to source.

That invariant is also a wall for a specific, common adopter: someone who already
has a `~/.claude` they care about — existing sessions, credentials, their own
`settings.json` (model, theme, plugins) — and is unwilling to turn that directory
into a clone of someone else's repo. Today the installer offers them only
`--dangerously-clobber` (back up and replace) or "move it aside." For an **org
rollout to non-technical colleagues**, where the safe-by-default path matters most,
that is the wrong and only choice on offer.

The subdirectory *copy* approach is not new — it has been informal practice for a
while. PR #182 (an external contributor) is the first time it has been **packaged**:
the repo lives in a subdirectory (e.g. `~/.claude/directory`), `~/.claude` stays the
user's own directory, and a new `/sync-to-home` command **projects** the repo's
outputs — skills, agents, commands, hooks, binaries — up into `~/.claude`, merging
only the hooks block and ways permissions into the existing `settings.json`. This
ADR formalizes existing practice rather than inventing a topology.

This is not a small feature; it breaks the in-place invariant and forks the
deployment model. The break has a measurable blast radius, ranked:

1. **Update detection goes dark.** `check-config-updates.sh` runs git against
   `~/.claude`, which in this topology is *not* a repo. It falls through every
   classifier and writes no cache, so the out-of-date nudge **never fires** — for
   precisely the topology with the most fragile update path (a two-step the user
   must remember: `git pull` *then* `/sync-to-home`).
2. **Divergent update workflows, single-topology messaging.** `update_status_text()`
   and several docs hardcode `make update`; the subdirectory path is
   `git pull && /sync-to-home`. The advice is wrong for these users.
3. **Drift becomes a first-class failure mode.** Copy-projection means "pulled but
   didn't re-sync" is a new, silent, wrong state. Canonical never had it; nothing
   detects it.
4. **The installer doesn't offer the gentle path.** `install.sh` knows only
   clone-in-place and clobber. The safe option for existing configs exists only in
   prose.
5. **Two `settings.json` writers** (`make install` and `/sync-to-home`) must agree
   on what they own, or a topology-switcher is surprised.

The decision underneath all five symptoms is **how** the projection happens: by
**copy** (what #182 does) or by **symlink** (`~/.claude/hooks → …/directory/hooks`,
etc.). That choice is too consequential to settle inside a command's bash, which is
why it is captured here.

## Decision

**Support two install topologies as first-class, with copy-based projection for the
subdirectory topology — made observable, so it reaches parity with the in-place
topology on update detection and messaging without taking on symlink fragility.**

1. **Two named topologies.**
   - **In-place** (default, unchanged): `~/.claude` *is* the repo. `make update`.
   - **Subdirectory**: repo at `~/.claude/<dir>`, projected into `~/.claude` by
     `/sync-to-home`. `git pull && /sync-to-home`.

2. **Copy, not symlink, is the subdirectory default.** The target audience spans
   mixed OSes including Windows (the #182 author already added Windows path-quoting);
   symlinks there require developer-mode/admin and must be followed correctly by
   Claude Code's hook execution. Copy is the form a non-technical adopter can
   reason about ("it copied files in") and is robust everywhere. Symlink-projection
   is recorded as a rejected alternative below, available to advanced users who opt
   in, but it is not the blessed default.

3. **The feature is delivered across all three ADR-138 substrates — way / skill /
   make — each owning its proper layer:**
   - **Mechanism (`make sync-to-home`).** A real script `scripts/sync-to-home.sh`,
     surfaced as a Makefile target, mirroring the `make update` → `scripts/update.sh`
     pattern of the in-place topology. It owns topology detection, backup, copy, the
     `settings.json` merge, and the marker/stamp writes below — deterministically,
     with no agent. This is why the `settings.json` merge shipped broken in #182:
     living in command prose it was un-testable; as a script behind a `make` target
     it is CI- and human-runnable and gets a smoke test.
   - **How (`/sync-to-home` skill).** The instructional layer (*skills own the how*).
     It tells Claude *what* to do and *to use the `make` target to do it*, carrying
     the judgment the mechanism can't — confirm this is the subdirectory topology,
     get consent before mutating `~/.claude`, invoke `make sync-to-home`, then report.
     It no longer carries the bash.
   - **Why (a topology way, `meta/deployment`).** Ways own the 5W. A new way carries
     the *rationale* — why you'd pick in-place vs subdirectory, and copy vs symlink —
     and fires when the topology question surfaces: install talk, an existing
     `~/.claude` conflict, "how do I update," or helping a colleague deploy. It arms
     Claude (often the very agent a non-technical adopter is pasting `curl | bash`
     into) with the *judgment behind the choice*, not just the command to run. The
     decision tree it discloses: existing `~/.claude` you value → subdirectory;
     greenfield / want zero-drift simplicity → in-place; Windows / symlink-shy →
     copy, never symlink.

4. **Make the copy topology observable** so the blast radius closes:
   - **Source marker.** `make sync-to-home` writes a `~/.claude/.claude-source`
     marker recording the absolute path of the projecting repo. This reuses the
     existing marker mechanism the update checker already honors for `renamed_clone`
     (`.claude-upstream`). `check-config-updates.sh` learns a new branch: if
     `~/.claude` is not itself a repo but a source marker points at one, run git
     *there*, classify against upstream, and on "behind" emit advice tailored to the
     topology: `cd <repo> && git pull && make sync-to-home`.
   - **Synced-HEAD stamp.** `make sync-to-home` records the repo HEAD it last
     projected (e.g. in the marker). A lightweight check compares the repo's current
     HEAD to the last-synced HEAD; when they differ, surface a **"pulled but not
     synced — run `make sync-to-home`"** nudge. This makes drift detectable rather
     than silent.

5. **Installer offers the topology as a conflict-menu option.** When `install.sh`
   detects an existing, non-agent-ways `~/.claude`, it already presents a menu of
   choices — today: (1) back up and clobber, (2) merge manually, (3) start fresh
   (move aside). Every one of those either *destroys* the existing config or *punts*
   to manual work; none keeps it. Subdirectory projection is added as a new,
   **non-destructive** menu entry — "install alongside: clone into a subdir, keep
   your `~/.claude`, run `make sync-to-home`" — defaulting to copy, with the symlink
   model available as the advanced variant. This makes the safe path a first-class
   choice at the exact moment of conflict, rather than something buried in the docs.

6. **Messaging parity.** `update_status_text()` and the install/update docs gain a
   subdirectory branch so every surface speaks the right update command for the
   topology in use.

PR #182 is the **starting point** for the copy-mode implementation. Its inline bash
is extracted into `scripts/sync-to-home.sh` (point 3); the script then gains the
marker and synced-HEAD writes; `make sync-to-home` wraps the script and the
`/sync-to-home` command is reduced to a narrating wrapper. The installer offer and
the messaging branch are the remaining follow-on work this ADR authorizes.

## Consequences

### Positive

- Colleagues with an existing `~/.claude` get a safe, non-destructive install path —
  the rollout's most common objection is answered without `--dangerously-clobber`.
- The dark-detection defect is closed: subdirectory installs get the same emphatic
  out-of-date nudge as in-place ones, plus a drift nudge in-place installs never need.
- The copy approach keeps Windows and symlink-shy environments working.
- The projection becomes deterministic and testable: a real script behind
  `make sync-to-home`, runnable without an agent, in CI, or by a human — closing the
  class of defect (the broken `settings.json` merge) that only existed because the
  logic lived in un-testable command prose.
- The decision and its alternatives are captured before the implementation hardens,
  so the copy-vs-symlink fork is a recorded choice rather than an accident of bash.

### Negative

- Two topologies is genuinely more surface to maintain: every install/update change
  must now be reasoned about twice, and tested on both.
- Copy-projection keeps drift as a real state; we mitigate it (synced-HEAD nudge) but
  do not eliminate it the way symlink or in-place would.
- A new marker file and a new detection branch add moving parts to the update checker,
  which is security-sensitive (it runs at session start).

### Neutral

- `make sync-to-home` and `make install` both touch `settings.json`; both now live in
  the Makefile and this ADR makes their ownership explicit (hooks block + ways
  permissions) but does not otherwise unify them.
- Leaves room for an opt-in symlink mode later without re-deciding the topology
  question — only the projection mechanism would change.

## Alternatives Considered

- **Symlink-projection** (`~/.claude/hooks → …/directory/hooks`, same for `bin`,
  `skills`, …). Architecturally the cleanest: `git pull` becomes the whole update,
  **zero drift**, no sync step, and the update checker can `readlink` to the repo and
  run git there — it restores every property the in-place topology has. Rejected as
  the *default* because the rollout audience includes Windows and low-privilege
  environments where symlinks are fragile (dev-mode/admin, and hook execution must
  follow them), and because it interleaves repo-owned trees into the user's own
  directory. Retained as a documented advanced opt-in.

- **Keep subdirectory as a doc-only escape hatch** (merge #182, label it
  "advanced/manual," invest nothing in detection/installer parity). Rejected: it
  leaves update detection dark for the exact users who most need the nudge, which is a
  defect, not a documentation gap — and undercuts the rollout goal of a safe,
  *supported* path.

- **Status quo — clobber or move-aside only.** Rejected: forces a destructive choice
  on adopters with valuable existing config, which is the wall this whole effort
  exists to remove.
