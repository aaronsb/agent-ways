---
contract: adr/v1
kind: evidence
capability: disclosure
status: accepted
date: 2026-06-22
deciders:
  - aaronsb
related: []
---

# ADR-183: Single-language localization tuning: the English anchor as a peer

> **Cites:** ADR-139 (adopter-run localization), ADR-125 (coordinate-alias model)
> **Motivates:** `ways tune --lang` implementation; the `ways-localize` skill's acceptance gate

## What this note is

ADR-139 makes localization adopter-run: a native speaker activates **one** language
and Claude re-hydrates every way's `description`+`vocabulary` into it, with `ways tune`
as the language-agnostic acceptance gate. This note settles a problem that only
surfaces in that adopter-run world: **`ways tune` as built cannot measure a single
language.** It proposes the fix and the `ways tune --lang` surface the workflow needs.

## The problem

`ways tune` (`tune.rs:measure_way`) scores each locale alias against the multilingual
corpus and derives two numbers:

- **Fidelity** = `min cosine` between this alias and its **same-way peers** — *other
  languages of the same way*. It measures whether all translations of a way embed in
  the same neighborhood.
- **Discrimination** = `min_peer − top_confuser.score` — whether the way's own peers
  outrank the best-scoring alias of any *other* way.

Both are defined in terms of **peers**, and a peer is another *non-English language*
of the same way (the ~1.0 self-match is excluded; the English frontmatter is not in
the multilingual corpus at all). `emit_report` only counts and flags entries with
`peer_count > 0`.

In the maintainer world (18 languages shipped together) every way had 17 peers, so
this worked. In the adopter world the common case is **one** active language. Then:

- `peer_scores` is empty for every entry → `peer_count == 0`,
- fidelity and discrimination are both `NaN`,
- every entry is dropped from the report → `ways tune` prints **"0/0 entries flagged"**
  and silently measures nothing.

A gate that passes by measuring nothing is worse than no gate — it reads as "clean"
while verifying nothing. This is the blind spot that has to close before `ways-localize`
can claim "tune clean" means anything.

## The governing principle: English is the root, the source of truth

The thing a single-language adopter most needs verified is *not* cross-language
agreement (there are no other languages) — it is **"does my French translation of way
X still mean way X?"** That is a comparison against the **English original**.

This is not merely a convenient peer to fill the empty-peer hole — it is the
architecture. **English (the way's `.md` frontmatter) is the root: the single source of
truth. Localizations (the `.locales.jsonl` aliases) are subordinate derivations,
validated by their alignment *to the root*, never against each other.** If translations
were instead tuned for agreement among themselves, a cluster of bad translations that
happen to agree would pass while drifting collectively away from the English meaning —
a multilingual free-for-all where there is no longer a fixed source of truth. The root
must be the anchor every localization is measured against.

**Why English specifically.** The choice is pragmatic, not a claim of linguistic
primacy. Computer science and engineering are still conducted predominantly in English —
the terms of art, the library names, the docs — so grounding the source of truth there
is where the field already operates, and it is what a single (English-speaking)
maintainer can realistically keep coherent. A project with a different center of gravity
could legitimately pick a different root. What the architecture *requires* is that there
be exactly **one** root, not that it be English. Here it is English; the only real
alternative is no fixed source of truth at all — the free-for-all this note exists to
prevent.

This corrects the metric, not just plugs a hole:

- **Fidelity is alignment to the English root**, measured per-language and
  independently: `cosine(localized alias, English root)`. A French stub is valid iff it
  embeds near the English original. It is *not* `min` over sibling translations — that
  would let a bad sibling poison a good one, and let a drifting cluster self-certify.
  Sibling translations are at most diagnostic; they are never the gate.
- **Discrimination** = `localized alias − top_confuser` stays as-is → catches a stub
  that collides with a *different* way's stub (e.g. `commits` vs `branching`).
  Orthogonal to root-alignment; both must hold.
- **One language or N**, the rule is identical: every localized alias must align to the
  English root and not collide. N languages add diagnostics, not a different gate.

No "single-language degenerate mode," no discrimination-only fallback, no peer-democracy.
The English root is structurally present in the multilingual space as the anchor, and
every localization is measured against it.

## Decision

1. **The agent-ways config is the mode switch that routes the whole intl pipeline.**
   The switch is the resolved `Config.language`, which is *layered*: `output_language`
   in `~/.claude/ways.json` (Layer 1) is **overridden** by `language` in the user-scope
   `~/.config/ways/config.yaml` (Layer 2, default `auto`). So the *effective* switch is
   the user-scope `language` — and that is the layer **`ways-localize` writes** to
   activate intl (writing `ways.json` alone would be masked by the `auto` default).
   Read once, upstream — not sniffed for per-component.
   - **English mode** (`output_language: en` / `auto` — the default): the *intl* pipeline
     is not engaged. No multilingual corpus is built, **the matcher never loads the
     768-dim multilingual model** (see §5), no English-root anchoring in a multi corpus,
     and **no locale tuning** (`ways tune` is not in the flow). The mode wins over data
     presence: a stray multi corpus is ignored, not run. *English corpus tuning is
     unaffected — it is always on:* a new or materially-changed English way recomputes
     its embedding and re-verifies match/discrimination regardless of mode.
   - **Localized mode** (`output_language` = a non-English code): the intl pipeline turns
     on — multilingual corpus built with the English root as anchor, multilingual
     matching enabled, root-anchored tuning runs.

   **This is distinct from Claude Code's `settings.json` `language`** (CC's *response*
   language). Two flags, one bridge: the detection macro compares them — CC set to `es`
   but ways `output_language` still `en` means "not localized yet" → nudge; once
   `ways-localize` flips `output_language`, the macro self-silences. Merely setting CC to
   Spanish must *not* trigger a 127MB-model download + translate-all-ways; localization
   is explicit work `ways-localize` does, and only then does it flip the ways flag.

2. **The English root is the anchor in the multilingual space** (localized mode only).
   Each way contributes its English frontmatter (`description`+`vocabulary`) to the
   multilingual corpus, embedded with the multilingual model, as the reference `en`
   entry. Every localized alias is scored *against this root*. The root is the fixed
   source of truth, not a co-equal peer — translations align to it; it never averages
   with them.

3. **`ways tune` runs in localized mode, scoped by `--lang <code>`** (the workflow passes
   the adopter's language; default is the active languages). It is simply never invoked
   in English mode, so it has no dormant/empty path to special-case.

4. **Acceptance semantics for the `ways-localize` gate:** a language passes when, for
   every way, the localized alias (a) has fidelity ≥ threshold **against the English
   root** (it still means what the source of truth means) **and** (b) has a non-negative
   discrimination gap (it does not collide with another way). Re-author the flagged
   stubs and re-run until clean — meaningful at N=1.

5. **Matching compute is gated on the mode switch.** Both modes match by embedding
   cosine; the difference is the model, not the method. English mode runs the 384-dim
   English model only; localized mode adds the 768-dim multilingual model as a *second
   lane* (the English lane still runs). The matcher (`scan/scoring.rs`) currently runs
   that multilingual lane whenever a multi corpus is *present* (`run_if_ready`) —
   presence-gated, not mode-gated. Change it to consult `output_language`: in English
   mode it **never loads the 768-dim multilingual model**, a per-match saving on every
   prompt for the default install, instead of relying on corpus-file absence to skip it.
   In localized mode it runs as today. (Phase A removed the multi corpus, so English mode
   is already cheap *incidentally*; this makes it explicit and robust against a stray
   corpus.)

## Resolution: the English root lives in the multilingual space

Given the source-of-truth principle, the English root must be **structurally present**
in the multilingual space as the anchor — so localizations are always measured against
the truth, not reconstructed from each other at measure time. That means English
entries go into the **multi corpus** (a `corpus.rs` change), embedded with the
multilingual model, generated whenever a language is localized.

This is decided, not a fork — it follows from "English root first." The runtime side
effect is a *bonus that fits the architecture*, not the justification: because the multi
corpus is also the runtime retrieval index, a non-English query gets cross-lingual
fallback routing via the English anchors the moment the user switches language — exactly
the un-localized state the detection macro fires in. The change is bounded to the
multilingual lane (English-only installs never build it), and it keeps `ways tune` a
thin read-over-the-corpus rather than an on-the-fly embedder.

The rejected alternative — injecting the English anchor at measure time only (`tune.rs`
embeds English on the fly, runtime corpus untouched) — keeps tuning and runtime separate
but reconstructs the root per-run instead of letting it *be* the corpus's anchor, and
forgoes the fallback. It contradicts "the root is structurally primary," so it is out.

## Consequences

- `ways tune` is meaningful at one active language — the adopter-run common case.
- The acceptance gate the `ways-localize` skill leans on verifies real translation
  quality (alignment to the English original + non-collision), not cross-language
  agreement that doesn't exist yet.
- Non-English queries gain English-anchor fallback routing in the multilingual lane —
  useful in precisely the un-localized state the nudge targets.
- **English-mode matching never loads the heavier 768-dim multilingual model** — gated
  on the mode switch rather than on corpus-file absence. A per-match compute saving on
  every prompt for the default install (the 99% case).
- The maintainer-era tuning way (`meta/knowledge/optimization/tuning`) describes the
  old peers-only model and must be rewritten to this one (tracked with the Phase B docs
  pass).

## See also

- ADR-139 — adopter-run localization (the world this note operates in)
- ADR-125 — coordinate-alias model (`description`+`vocabulary` as embedding-space alias)
- `docs/explanation/localization/` (catalog `01.009.E`–`01.013.E`) — the user-facing
  scenarios this note is the mechanics for
- `meta/knowledge/optimization/tuning` way — to be rewritten to this model
