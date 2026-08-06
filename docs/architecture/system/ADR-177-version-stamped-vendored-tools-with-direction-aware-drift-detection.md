---
status: Accepted
date: 2026-08-06
deciders:
  - aaronsb
  - claude
related:
  - 138
---

# ADR-177: Version-stamped vendored tools with direction-aware drift detection

## Context

Three tools ship inside ways and get **vendored** — copied, never symlinked —
into consuming repositories: `adr-tool` (documentation/adr), `doc-tool`
(documentation/linting), and `chart-tool` (softwaredev/visualization/charts).
ADR-138 put the vendoring procedure in skills; the copies it produces are
snapshots that never hear about upstream changes.

None of the three carries a version marker. The one drift check that exists —
the adr way's macro `diff -q` between the repo copy and the installed
template — is **direction-blind**: a stale copy and a deliberately customized
copy produce the same byte difference, and the macro's note ("differs from the
universal template; expected for customized setups") actively reassures the
reader in the stale case. As upstream tools grow features (e.g. `adr archive`,
issue #438), every vendored copy silently falls behind while the disclosure
says all is well.

## Decision

**Every vendorable tool carries its own version, and drift disclosure reads
direction from it.**

1. **Per-tool semver constant** — a `TOOL_VERSION = "X.Y.Z"` line near the top
   of each tool, plus a `--version` flag. The version is per-tool and bumps
   only when the tool changes. It is deliberately *not* the ways release
   version: coupling to releases would mark every vendored copy stale on every
   release even when the tool never moved.

2. **Direction-aware macro disclosure** — a way macro that surfaces a vendored
   tool extracts `TOOL_VERSION` from both the repo copy and the installed
   template (a `grep`, not an execution) and compares with `sort -V`. Five
   states replace the direction-blind diff note:

   | Local copy vs installed | Disclosure |
   |---|---|
   | no version marker, installed stamped | predates versioning — out of date; re-vendor |
   | lower than installed | stale — re-vendor to pick up the newer tool |
   | equal, bytes differ (version line excluded) | customized — expected, say so |
   | higher than installed | repo is ahead — the agent-ways install is stale; update it |
   | stamped, installed unversioned | same as above — the install is stale |

   The version line is excluded from the customization diff so the stamp
   cannot trigger the very warning it exists to disambiguate. The extracted
   stamp is shape-restricted to a version string — a stamp that doesn't parse
   as one is treated as unversioned, never echoed into disclosed context.

3. **Re-vendoring is a documented move** — the skill that owns each tool's
   vendoring procedure (ADR-138) also documents the update: plain copy when
   the local tool is unmodified; when customized, diff first so local changes
   are carried forward rather than clobbered.

The unversioned era is bounded by the rule itself: a copy with no
`TOOL_VERSION` marker is by definition older than every stamped release, so
"no marker" needs no special casing beyond "out of date."

## Consequences

### Positive

- Drift disclosure gains direction: stale, customized, and ahead-of-install
  are distinct states with distinct remedies, instead of one reassuring note.
- Tool changes (like #438's `adr archive`) propagate — the next session in a
  consuming repo is told its copy is behind, rather than nobody ever noticing.
- "Repo ahead of install" is surfaced too, catching the stale-install case the
  old diff attributed to customization.

### Negative

- Version bumps are a manual discipline — a tool change without a bump defeats
  the mechanism. Mitigated by review: the stamp lives in the same file as any
  change to it.

### Neutral

- Requires stamping all three tools, upgrading the adr way's macro, and adding
  update guidance to the owning skills; other tool-surfacing macros adopt the
  same comparison as they grow one.
- Already-vendored copies in the wild have no marker and will read as "out of
  date" on first contact with the new macro — which is accurate.

## Alternatives Considered

- **Keep the byte-diff note.** Rejected: direction-blind. It cannot
  distinguish the case that needs action (stale) from the case that needs none
  (customized), and its wording suppresses the one signal it does emit.
- **Stamp tools with the ways release version.** Rejected: false staleness —
  every release would mark every vendored copy out of date even when the tool
  is byte-identical.
- **Checksum registry (manifest of known tool hashes per version).** Rejected:
  heavier machinery for the same answer; requires the installed side to carry
  history, and any local edit breaks the lookup entirely — exactly the
  customized case the mechanism must keep distinguishable.
- **Auto-update on detection.** Rejected: a vendored copy may be deliberately
  customized; overwriting on sight destroys local changes. Disclosure names
  the state; the operator (or a skill-guided session) decides.
