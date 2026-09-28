---
status: Accepted
date: 2026-07-01
deciders:
  - aaronsb
  - claude
related:
  - ADR-142
---

# ADR-150: Version-truth and downgrade-safe self-update

## Context

`ways update` (the self-update command, ADR-142's projection lifecycle) refreshes
the app source and its binaries "pre-built first": it downloads the latest
GitHub Release binary and only falls back to building from source when no
toolchain is present. This is correct for an end user pinned to a release — but
it **silently downgraded a live install** in practice:

- The app checkout tracks `main`. At the time of the failure `main` was **78
  commits ahead of the `ways-v1.0.0` tag** — those 78 commits included the
  entire `settings` command and `ways update` itself.
- `tools/ways-cli/Cargo.toml` had stayed at `version = "1.0.0"` across all 78
  commits, and the newest published release was also `ways-v1.0.0`. So the
  pre-built binary and the source **reported the same version string while being
  78 commits apart**.
- `ways update` downloaded the stale `ways-v1.0.0` binary over the current one.
  `ways update` and `ways settings` vanished (`unrecognized subcommand`), and a
  second `ways update` was impossible — the command that would fix it had been
  removed by the update.

The machinery to *avoid* this already exists; the gap is **version-truth**, not
tooling:

- **The release pipeline is complete.** `.github/workflows/build-ways.yml` (and
  the sibling `build-attend`/`build-way-embed` workflows) trigger on `ways-v*`
  tags, build all four platforms, and the tag-gated `release` job runs
  `gh release create`. Cutting a release is `bump + tag + push`; CI does the
  rest.
- **The binary already bakes provenance.** `build.rs` sets `WAYS_COMMIT` from
  `git rev-parse --short HEAD`, and `banner.rs` shows `v1.0.0 (c595437)`.

But that provenance never reaches the surfaces that decide anything:

- `ways --version` is wired to clap's `version` = `CARGO_PKG_VERSION` alone
  (`1.0.0`), with **no commit**. `download-ways.sh`'s "already installed" check
  and every human sanity-check see only the frozen string. The one place the
  commit appears — the bare-invocation banner — is human-only and unread by
  tooling.
- **Nothing compares versions before replacing a binary.** `ways update`'s
  `refresh_component` renames-then-reverts on *build failure*, but a *successful*
  download of an older binary is treated as success. There is no "is this
  candidate actually newer than what I'm replacing?" gate.

The result: staleness is invisible where it counts, and the self-updater will
downgrade whenever the tracked source is ahead of the latest cut release — which,
for a checkout that follows `main`, is the *normal* state between releases.

## Decision

Make the version *truthful and staleness-detectable*, and make self-update
*refuse to move backward* — leveraging the existing tag→CI→release pipeline
rather than adding a new one. Four changes:

1. **Bake `git describe`, not just the short hash.** Extend `build.rs` to emit
   `WAYS_BUILD = git describe --tags --always --dirty` (e.g.
   `ways-v1.0.0-78-gc595437`) alongside `WAYS_COMMIT`. This single string
   encodes the nearest release tag, the commits-ahead count, the commit, and a
   dirty flag — everything needed to order two builds.

2. **Surface it in `ways --version`.** Set clap's `long_version` so
   `ways --version` prints the full provenance
   (`ways 1.0.0 (ways-v1.0.0-78-gc595437)`), making a dev build visibly distinct
   from a release build to both humans and scripts. The banner is unaffected.

3. **A downgrade guard in `ways update`.** Before installing a candidate
   pre-built binary, compare its embedded `WAYS_BUILD` against the pulled
   source's `git describe`. Install the pre-built **only if it is at least as new
   as the source** (same commit, or the source is an ancestor of the release).
   Otherwise:
   - **toolchain present →** build from source (the checkout is the authority
     and can't be behind itself);
   - **no toolchain →** keep the current binary and warn loudly ("a pre-built
     matching this source hasn't been published yet; install a toolchain or wait
     for the release"). **Never replace a binary with an older one.** This is the
     belt-and-suspenders that keeps `ways update` safe *between* releases,
     independent of whether anyone remembered to cut one.

4. **A release-cut helper so the version bump can't be forgotten.** The root
   cause was human: 78 commits merged with no bump and no tag. Add a single
   entry point — `scripts/release.sh <component> <patch|minor|major>` (surfaced
   as `make cut-release COMPONENT=ways LEVEL=patch`) — that bumps the component
   `Cargo.toml` version, commits it, creates the `<component>-vX.Y.Z` tag, and
   pushes. CI takes it from there. The policy: **a release is cut from `main`
   whenever a user-affecting change to a shipped binary lands** (not every PR,
   but no long silent runs). Between releases, `git describe` — now surfaced and
   guarded — carries the truth, so a missed release degrades to "build from
   source / keep current," never to a downgrade.

Semver stays per-component (`ways-vX.Y.Z`, `attend-vX.Y.Z`, …), matching the
existing tag scheme and independent CI workflows.

## Consequences

### Positive

- The self-updater can never silently downgrade: the worst case between releases
  is "built from source" or "kept current with a warning," both of which leave a
  working, current-or-newer binary.
- Staleness is legible everywhere — `ways --version`, `download-ways.sh`, CI logs,
  bug reports — because the build string names its exact provenance.
- The release pipeline that already exists gets *used* on a discipline, closing
  the gap that let 78 commits sit unpublished.
- `git describe` ordering is free and reliable — no version-string parsing games,
  no dependence on anyone bumping Cargo.toml for *correctness* (the bump is for
  human-facing semver; the guard keys on commit ancestry).

### Negative

- `build.rs` now depends on `git describe` succeeding in the build environment;
  a tarball build with no `.git` yields `WAYS_BUILD = unknown` (handled: the
  guard treats `unknown` as "cannot prove newer" → build/keep-current, never
  downgrade).
- The downgrade guard needs the source checkout's `git describe` at update time —
  fine for the app checkout (always a git clone per ADR-142), and the guard
  degrades safely when it can't be computed.
- One more release step to remember — mitigated by the `make cut-release` helper,
  which makes the correct path the easy path.

### Neutral

- Requires touching `build.rs`, `main.rs` (clap `long_version`),
  `update.rs` (`refresh_component` gains the version comparison), and a new
  `scripts/release.sh` + `make cut-release` target — the build slice tracked
  separately from this ADR.
- The `attend`/`attend-chat` binaries still have no published pre-builts; the
  guard's "no candidate newer than source → build/keep" path already covers them
  (they are toolchain-gated today), so nothing regresses.

## Alternatives Considered

- **Always build from source when a toolchain is present (drop pre-built-first).**
  Rejected: it discards the explicit "not everyone has build tools" requirement
  that motivated pre-built-first, and it's slower for the common case where the
  source *is* a released tag. The downgrade guard achieves the same safety while
  keeping download-first for users on releases.
- **Compare `CARGO_PKG_VERSION` strings only (bump-per-PR discipline).** Rejected
  as the *primary* mechanism: it's exactly what failed — a human forgot to bump,
  and string equality can't see commit ancestry. Version bumps remain for
  human-facing semver, but *correctness* rides on `git describe`, which can't be
  forgotten.
- **Auto-cut a release on every merge to `main` (CI bumps + tags).** Rejected as
  over-complex for now ("only complexify to the amount necessary"): it turns
  every merge into a published release with version churn and 4-platform builds.
  The `make cut-release` helper keeps cutting cheap and deliberate; auto-cut can
  be revisited if the cadence proves too manual.
- **Embed a monotonic build number instead of `git describe`.** Rejected: it
  needs external state (a counter), where `git describe` derives ordering from
  the repo itself for free and is human-legible.

## Amendment (2026-07-05): the downgrade guard does not apply to `--ref`

The guard above governs the **release channel**: `ways update` pulls the tracked
branch and must never replace a binary with an older published one. `ways update
--ref <branch|tag|sha>` (ADR-142's amendment) is a different lifecycle — an
explicit pin to a chosen ref, built from source — and the guard is
**intentionally bypassed** there, for two reasons:

- **Download-first cannot apply.** An unpublished ref has no GitHub Release
  binary, so `--ref` always builds from source. There is no downloaded candidate
  to compare — which is the only thing the guard gates.
- **"Never move backward" is the wrong invariant for an explicit pin.** The guard
  exists because a *channel* update should be monotonic. Choosing to deploy a
  specific commit — including one behind the current build, to reproduce or
  bisect — is a deliberate act, not an accidental downgrade. The guard's job is to
  stop *silent* regressions; a named `--ref` is neither silent nor accidental.

The safety that remains is structural, not guard-based: `--ref` still touches only
app-scope (`$XDG_DATA` plus the regenerable projection) per ADR-142 §5, and
returning to the release channel is one command (`ways update --ref main`), after
which the guard governs again as normal.
