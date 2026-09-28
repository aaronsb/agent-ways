---
status: Accepted
date: 2026-07-06
deciders:
  - aaronsb
  - claude
related:
  - "[[ADR-142]]"
  - "[[ADR-147]]"
  - "[[ADR-162]]"
---

# ADR-163: Config separation — dotfiles as source-of-truth feeding the settings fragment store

## Context

ADR-147 built the user-scope settings fragment store (`$XDG_CONFIG_HOME/agent-ways/settings/`)
and its projector (`ways settings project`). It defined how fragments *compile and
merge* into `~/.claude/settings.json`, but left two things open:

1. **Where fragments come from** — the store is a per-host directory. Nothing said how
   an operator's config *travels* across their machines.
2. **File artifacts** — ADR-147's Context named `statusline` (and hook scripts) as file
   artifacts, but the built machinery projects only settings.json *keys*, never files.

The gap is not academic. Auditing one operator's two hosts (call them north and slab)
surfaced silent drift that no CI catches:

- `statusline.sh` present on north, **missing on slab** — the settings *pointer* had
  propagated (legacy residue) but the *script* never did, so slab's status line was
  broken while its `settings.json` still referenced it.
- `model` differed (`opus[1m]` vs `opus`); a `permissions.deny` guarding gh/docker
  credentials was present on north, **absent on slab** — a security drift.
- Session-link (`Claude-Session:`) suppression worked on north **only because a per-host
  `~/.claude` memory told the agent to disobey the harness prompt** — slab lacked the
  memory and leaked the URL into commits/PRs. (This ADR originally recorded ADR-162's
  reason: that `attribution.sessionUrl` was *broken upstream*, leaving a mechanical deny
  hook as the real fix. That premise was false — [[ADR-167]] proves the key governs the
  link and supersedes ADR-162. The observation above is unaffected: the control, whatever
  it is, only defends the hosts it reaches. *Distributing* it is this ADR's concern, and
  the drift is the point.)

Separately, the framework repo was **force-claiming a user-scoped key**: `statusLine` sat
inert in the repo-tracked `settings.json` (reconcile co-owns only `hooks` +
`permissions`, so it was never projected) — exactly the anti-pattern ADR-147 set out to
end.

## Decision

**dotfiles is the operator's cross-host source-of-truth; agent-ways compiles and projects
what dotfiles feeds it.** The layering:

```
dotfiles (VCS, per-operator)
  ├─ settings fragments ──deploy──► $XDG_CONFIG_HOME/agent-ways/settings/  (ADR-147 store)
  │                                    └─ ways settings project ──► ~/.claude/settings.json  (KEYS)
  └─ file artifacts (statusline.sh, …) ──deploy──► ~/.claude/                (FILES)
```

Four commitments:

1. **Artifact-ownership split.** The fragment store owns settings.json **keys**; dotfiles
   owns **file artifacts** and deploys them directly to `~/.claude`. One owner per
   artifact — no file has two writers. (`statusLine` the *key* → fragment;
   `statusline.sh` the *file* → dotfiles.)

2. **The framework de-claims user-scoped keys.** The repo-tracked `settings.json` ships
   only what `ways reconcile` co-owns (`hooks` + `permissions`). `statusLine` was removed
   (PR #347, merged).

3. **The fragment store is the projection boundary.** dotfiles never writes `~/.claude`'s
   `settings.json` directly — it deploys the store, and `ways settings project` performs
   the three-way merge. agent-ways stays the single `settings.json` writer (alongside
   reconcile's two co-owned slices), so the operator's own keys survive.

4. **Projector-base hygiene is part of the contract.** The projector's per-host
   last-applied base (`$XDG_STATE/agent-ways/settings-fragments-<scope>.json`) is
   **host-local ephemeral state**, not config. It can carry *ghosts* — keys a prior
   projection managed but the store no longer declares — which cause surprising
   cross-host retractions. (Observed: a stale base recording `model: opus` would, if
   carried to another host, silently retract that host's model to the default.) The base
   is reset when the store's ownership set changes; dotfiles never deploys it.

## Consequences

- Cross-host config becomes reproducible: clone dotfiles → deploy → `ways settings
  project`, and any host converges to a coherent Claude Code config.
- Session-link suppression (ADR-162) stops being a per-host memory hack: the deny hook
  becomes a primitive distributed through this same pipeline.
- The two disjoint settings writers (`ways reconcile` vs `ways settings project`) that
  ADR-147 left unreconciled remain disjoint here; unifying them is out of scope
  (follow-up).
- File-artifact projection stays *outside* agent-ways (owned by dotfiles). If the file
  set grows, revisit building projection into the fragment store (see Alternatives).

## Alternatives Considered

- **Plain dotfiles writes `~/.claude/settings.json` directly.** Rejected: two owners
  (dotfiles + `ways settings project`/reconcile) fighting one file — the exact drift that
  produced the slab breakage.
- **Personal config lives in the framework repo.** Rejected: revives ADR-147's
  force-claiming anti-pattern (the inert `statusLine` is the cautionary case).
- **Build file-artifact projection into agent-ways so it owns `statusline.sh` too.**
  Deferred, not rejected: cleaner single-owner end-state, but net-new machinery. The
  dotfiles-owns-files split ships today and proves the loop; revisit when the file set
  justifies it.

## Validating implementation (the statusline pilot)

`statusLine` was the pilot that exercised every seam:

- **De-claimed** from the repo (`statusLine` removed from tracked `settings.json`, PR #347, merged).
- **Authored** as a user-scope fragment (`10-statusLine.md`) in the store.
- **Projected** cleanly — the stale projector base was reset first to clear ghosts, after
  which `ways settings project` reported `already up to date`.
- **Distributed** — both the fragment store and `statusline.sh` are now dotfiles-managed
  symlink entries (`claude-settings`, `claude-statusline`), committed and pushed, so a
  second host reproduces the config with `dotfiles pull && dotfiles deploy && ways
  settings project`.

Attribution/session-link suppression (ADR-162) is the second, higher-value pass over this
same pipeline.
