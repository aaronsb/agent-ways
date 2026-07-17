---
status: Accepted
date: 2026-07-16
deciders:
  - aaronsb
  - claude
related:
  - "[[ADR-104]]"
  - "[[ADR-147]]"
  - "[[ADR-162]]"
  - "[[ADR-163]]"
supersedes: ADR-162
---

# ADR-167: Session-link suppression: attribution.sessionUrl as primary control, deny hook as backstop

## Context

The disclosure surface [[ADR-162]] identified is real and unchanged: Claude Code
appends a session link to commit messages and PR bodies, that link resolves to the
**full session transcript**, and a transcript routinely carries material that merely
scrolled through the conversation — file contents, environment values, tokens,
internal paths. On a public repository, publishing it puts one click between a commit
and whatever secret happened to pass through the session.

What has changed is the claim that nothing but a hook can stop it. ADR-162 concluded:

> The conclusion is that any control depending on the harness honoring a setting, or
> on the model choosing to obey it, is not load-bearing. Suppression must be
> mechanical and must live in a layer we own.

That conclusion was false when written, and it rested on two errors.

**A mis-citation.** ADR-162 attributed the failure to upstream `#18253`, described as
*marked not planned*. `#18253` is a different bug — "Attribution settings not honored
— Co-Authored-By and PR footer added despite empty attribution config"
([CLOSED/COMPLETED]) — and concerns the **footers**, never the session link. The issue
it meant is `#41873`, "attribution setting does not control session URL in commit
messages" (filed April 2026, [CLOSED/NOT_PLANNED] 2026-05-22).

**A superseded fact.** `#41873` was closed not-planned, and then upstream shipped the
control anyway. Claude Code **v2.1.183** (2026-06-19) added `attribution.sessionUrl`:

> Added `attribution.sessionUrl` setting to omit the claude.ai session link from
> commits and PRs in web and Remote Control sessions

ADR-162 is dated 2026-07-06 — seventeen days after the key shipped. It named
`attribution.sessionUrl` as "the key that does govern it" and then dismissed it as
unreliable on the strength of an issue about a different bug. The key is absent from
the official settings documentation (upstream `#69614`, open), which is why it stayed
unset here long after it was available; `#66504` (open) separately argues the link
should be opt-in rather than default-on.

### The evidence

A single-variable experiment on 2026-07-16 settles it. Same repository, same binary
(**v2.1.212**), same account; only `attribution.sessionUrl` differs. The question put
to each session: *does your system prompt instruct you to end git commit messages with
a Claude-Session trailer?*

| Run | `attribution.sessionUrl` | Trailer instruction in system prompt |
|-----|--------------------------|--------------------------------------|
| control | `true` | **present** — a `Claude-Session:` trailer naming a session URL |
| treatment | `false` | **absent** |

The setting governs the trailer. Two earlier attempts did **not** establish this and
were discarded: a headless `claude -p` pair (void — `-p` never injects the trailer, so
the control could not reproduce the baseline) and a run in a non-git directory (void —
no repository, therefore no commit instructions). Only the controlled pair is evidence.

### How the error persisted

The mis-citation propagated into five artifacts, each of which made the next look
corroborated:

1. **ADR-162** — the original wrong citation.
2. **`strip-session-link-pre.sh`** — its header comment repeats "the `attribution`
   setting does not reliably reach the model (upstream #18253)".
3. **The operator's memory entry** — repeats "attribution setting doesn't govern it
   (#18253)".
4. **[[ADR-163]]** — records the premise second-hand: "the `attribution.sessionUrl`
   config key is broken upstream, so a mechanical hook that denies the publishing
   command is the real fix". Its own decision does not rest on this, but an Accepted
   ADR restating the claim lends it the weight of a second source.
5. **The `softwaredev/delivery/github` macro** — reports `Session-link trailer: OFF`
   from `attribution.commit`/`.pr` alone, having never read `sessionUrl`.

The macro is the sharp end. It rendered the wrong fact as a reassuring green light in
every session, including sessions whose system prompt carried the trailer. In the
control run above, the session had to reason around its own status line contradicting
its system prompt.

The count itself makes the point. This ADR's first draft listed **four** carriers and
missed ADR-163 — a document it cites approvingly in its own Decision and lists in
`related:`. A reviewer found it. An argument about how unchecked claims propagate is
exactly the argument whose own citations go unchecked, and being the one making it
confers no immunity.

The lesson generalizes past this key: ADR-162 froze a **contingent upstream fact** as
though it were a durable architectural truth. Upstream behavior is evidence with a
shelf life. A decision that depends on it should say so, and should name the
observation that would overturn it.

## Decision

**`attribution.sessionUrl: false` is the primary control.** It is authored as a
fragment in the settings store ([[ADR-147]]) and reaches `~/.claude/settings.json`
through the dotfiles source-of-truth ([[ADR-163]]) — `20-attribution.md`, carrying
`commit: ""`, `pr: ""`, and `sessionUrl: false`. Suppression happens at the source: the
model is never instructed to emit the link, so there is nothing to catch.

**The ADR-162 hook is retained, demoted to backstop.** Its mechanism is unchanged and
its design rationale still holds — deny rather than rewrite (rewriting requires forcing
`permissionDecision: allow`, which would bypass the operator's own permission rules on
outward-facing `gh` commands), user scope rather than per-project, trailer/footer
position only so inline prose mentions pass. What changes is its standing: it is no
longer the sole defense.

It is retained for two reasons, and two only:

- **Undocumented** — absent from the official settings docs (`#69614`), so it cannot be
  rediscovered from primary sources and may change without notice. A key we found by
  reading a changelog is a key upstream never promised us.
- **Default-on, and delivered by an independent path** — a host that never receives the
  fragment leaks by default. This bites only because hook and fragment travel
  *separately*: the hook rides reconcile-owned `hooks`, the fragment rides dotfiles →
  fragment store → project. Were they one pipeline, the backstop would be absent exactly
  when it was needed and would cover nothing. [[ADR-163]] records a real instance —
  suppression held on one host and leaked on another.

A third reason was considered and **rejected as circular**: that the changelog
scope-qualifies the key to "web and Remote Control sessions". The qualifier cuts both
ways. If Remote Control is the likely reason the link reached commits at all — and this
operator runs `remoteControlAtStartup: true` — then the qualifier may describe *complete
coverage*, not a gap. Reasoning "the boundary is uncharacterized, therefore keep the
backstop" while also holding "the backstop stands, therefore the boundary needn't be
characterized" is ADR-162's error inverted: an unexamined boundary dressed as a known
risk. The boundary is genuinely uncharacterized. That is an open question, not evidence.
The two reasons above carry the decision without it.

The backstop costs nothing when the primary control works: the model never emits the
link, so the hook never fires. It is insurance against the fragment not being projected
and against the key changing upstream — not against a session-type gap we have never
observed.

**Status surfaces must read the key that governs.** The `github` macro reports the
session link off `attribution.sessionUrl` (absent means ON), and reports the
Co-Authored-By / "Generated with Claude Code" footers separately off `commit`/`pr`.
Conflating two independent controls under one label is the error this ADR corrects; a
surface that asserts a fact it never read is worse than no surface.

**Defense in depth is the posture, not a single mechanism.** Setting suppresses, hook
catches, macro reports. Each is independently insufficient.

## Consequences

### Positive

- The link is suppressed **before** the model is instructed to emit it, rather than
  denied after it is written. No retry round-trip, no reliance on the model adapting
  within a session.
- The hook stops being load-bearing, so a gap in its command coverage (ADR-162 lists
  them) is no longer a leak on its own.
- The operator's config carries the reason: the fragment body records why the key is
  set, what governs what, and why the hook remains.

### Negative

- The primary control is undocumented upstream. If Anthropic renames or removes
  `sessionUrl`, the fragment silently stops working — and the failure is invisible
  (a link appears where none did before). The backstop exists for exactly this.
- The key is not in the vendored SchemaStore schema, which declares `attribution` with
  `additionalProperties: false` and only `commit`/`pr`. `ways settings new
  attribution.sessionUrl` therefore refuses it, and the fragment is hand-authored.
  Tracked separately, with an upstream PR as the durable fix.
- Two mechanisms to maintain where ADR-162 had one.

### Neutral

- ADR-162's hook code is untouched by this decision; only its header comment needs its
  rationale corrected.
- The scope boundary of `sessionUrl` ("web and Remote Control sessions") is uncharted
  for other session types — it is not known whether such sessions receive the link at
  all. Characterizing it is cheap (one controlled run with `remoteControlAtStartup`
  off) and would either close the question or reveal a real gap. Left open
  deliberately, and named here so the omission is visible rather than assumed away.

## Alternatives Considered

- **Keep the hook as sole defense; ignore the setting.** Rejected: preserves the
  false premise, and pays a deny/retry round-trip on every commit for a leak the
  harness will suppress for free. Being wrong for a good reason is still wrong.
- **Adopt the setting; retire the hook.** Rejected. The setting is undocumented,
  default-on, and scope-qualified — three properties that individually argue for a
  backstop. ADR-162's instinct was half right: the harness honors the setting fine;
  the failure was in our reading of it. That is a reason to verify the setting, not to
  trust it alone.
- **Amend ADR-162 in place.** Rejected: the *decision* changes, not merely its
  context. The hook's role moves from sole defense to backstop, and a new control is
  introduced. Precedent ([[ADR-104]] → ADR-123) supersedes when the decision moves and
  preserves what remains correct — which is what this does.
- **Set `sessionUrl` directly in `~/.claude/settings.json`.** Rejected: unmanaged,
  host-local, and undocumented — the same shape of failure ADR-162 recorded, where a
  repository without local config leaked because the control never traveled. The
  fragment store carries it to every host by construction.
