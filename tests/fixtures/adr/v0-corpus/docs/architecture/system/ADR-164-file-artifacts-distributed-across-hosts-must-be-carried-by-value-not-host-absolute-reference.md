---
status: Accepted
date: 2026-07-07
deciders:
  - aaronsb
  - claude
related:
  - "[[ADR-163]]"
  - "[[ADR-147]]"
  - "[[ADR-142]]"
---

# ADR-164: File artifacts distributed across hosts must be carried by value not host-absolute reference

## Context

ADR-163 split config artifact ownership: the settings fragment store owns
settings.json **keys**, and dotfiles owns **file artifacts** and deploys them to
`~/.claude`. Its validating pilot claimed the `statusline.sh` file artifact was
"distributed" across hosts.

It was not. The dotfiles-tracked artifact was itself a **symlink whose target was an
absolute path into a per-host directory** — `/home/<authoring-user>/.local/share/
agent-ways/statusline.sh`. That link resolved only on the host that authored it. On
a second host with a different `$HOME` (a different username) the target did not
exist, so the deployed `~/.claude/statusline.sh` dangled and the status line was
broken — the very cross-host drift ADR-163 set out to end, reintroduced one layer
down. The keys half of the pipeline never had this problem: the fragment store holds
real YAML, so it already travelled by value.

The failure mode generalizes beyond statusline: any artifact distributed **by
reference**, where the reference encodes a host-local absolute path (a `$HOME`, an
app dir, a cache dir, a username), silently fails to reproduce on a host whose layout
differs.

## Decision

**A file artifact carried through the config pipeline is distributed by value — its
content — never by a reference that encodes a host-local absolute path.**

Concretely, the dotfiles-tracked artifact is the **real file**. `dotfiles deploy`
then symlinks `~/.claude/<artifact>` to that dotfiles copy — a link whose target is
host-relative under `$HOME`, so it resolves identically regardless of username or
where the application happens to be installed. No artifact anywhere in the pipeline
may be a symlink whose target is an absolute path into a per-host application, cache,
or home directory.

Corollary: dotfiles is the source of truth for a managed artifact's content
(consistent with ADR-163) — which also lets the operator customize it. Managing an
app-shipped default in dotfiles means dotfiles wins; the artifact is no longer an
alias of the app's copy.

## Consequences

### Positive

- Distributed artifacts reproduce on any host regardless of username or install
  layout; ADR-163's cross-host distribution claim actually holds.
- Operators can customize a managed artifact, because dotfiles owns its content
  rather than pointing at an app-shipped file.

### Negative

- App updates to a shipped default (e.g. a new `statusline.sh`) no longer flow
  automatically to a host that manages it via dotfiles — the operator re-syncs the
  content when they want the newer default. A by-value file is opaque to which app
  version produced it.

### Neutral

- This brings file artifacts to parity with the keys half of the pipeline, which was
  already by-value (the fragment store holds real YAML).
- A future "file-artifact projection built into agent-ways" (ADR-163 Alternatives,
  deferred) would supersede the dotfiles-owns-files mechanism, but must honour this
  same by-value principle — it too cannot distribute a host-absolute reference.

## Alternatives Considered

- **Relative symlink into the app dir** (e.g. `../../.local/share/agent-ways/
  statusline.sh`). Rejected: still by-reference. It aliases the app's copy rather
  than owning content, so the application stays the true owner (contra ADR-163); it
  breaks if the dotfiles store or the app dir moves relative to `$HOME`; and it
  cannot carry operator customization.
- **A symlink target with `$HOME`/env expansion.** Rejected: a symlink stores literal
  bytes; the OS does not expand environment variables in a link target.
- **Leave the artifact host-absolute and re-author it per host.** Rejected: that is
  exactly the silent cross-host drift ADR-163 set out to end.
