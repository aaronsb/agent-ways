---
status: Accepted
date: 2026-06-22
deciders:
  - aaronsb
  - claude
related:
  - ADR-107
  - ADR-125
  - ADR-138
supersedes_in_part: ADR-125
---

# ADR-139: Shelve maintainer i18n; adopter-run localization via ways-localize

> Decision record for GitHub issue #160. Supersedes the maintainer-side
> obligations of the locale layer introduced in ADR-107 and elaborated (still in
> Draft) in ADR-125. The retrieval machinery those ADRs describe is **kept** — only
> the maintainer's standing obligation to author and ship translations is shelved.

## Context

agent-ways ships a maintainer-maintained multilingual layer so non-English Claude
Code users can get ways' benefits in their own language. The intent was sound; the
economics and architecture are not:

- **Zero validated users.** 222 sessions / 10,019 way fires over ~4.5 months — one
  English speaker. No non-English usage signal exists. We built and maintain the
  layer against a hypothetical adopter.
- **Heavy, compounding maintenance cost.** A scout inventory (2026-06-22) measured
  the real surface:
  - **91 `.locales.jsonl` files, ~3,094 locale entries** across 18 active languages
    (the issue's "~1,547" estimate undercounted).
  - A **second 127MB embedding model** (`paraphrase-multilingual-MiniLM-L12-v2`)
    downloaded *unconditionally* in every `make setup` alongside the 21MB
    English-only model.
  - A whole audit subsystem (`ways language`, `ways tune`) plus three CI targets
    (`test-lang`, `test-locales`, `test-multilingual`) gating `make test`.
  - A **17×-per-way authoring tax**: `template.rs` pre-populates every new way with
    17 translation stubs (18 entries incl. English).
- **Architecturally leaky, and getting leakier.** Per ADR-138 (skills own the *how*,
  ways own the *5W*), value is actively migrating *into* skills — and skills are
  English-only. A localized way now hands off to an English skill: we localized the
  *shrinking* half of the value surface.
- **Unverified substrate.** Claude Code's own non-English skill surfacing/matching
  has never been measured. We localized ways on top of an unmeasured foundation.

A genuinely complete non-English experience needs localized **skills** *and*
verification of Claude Code's native non-English behavior — a much larger job that
should be triggered by a real adopter, not pre-built against a hypothetical one. And
the people best positioned to judge translation fidelity and idiom are **native
speakers**, not the (English-speaking) maintainer. So both the *cost* and the
*quality* of localization belong with the adopter, not the maintainer.

## Decision

**Stop pre-shipping translations. Move localization to an adopter-run, autonomous
flow.** Concretely, draw a hard line between the localization *engine* (kept,
dormant) and the localization *data* + maintainer *obligations* (removed):

| Layer | Fate | Why |
|-------|------|-----|
| Localization **data** (~3,094 entries / 91 `.locales.jsonl`) | **Delete** | Stale, unused, regenerable on demand; git history is the backstop |
| Rust intl/locale code paths (`corpus.rs` split, `tune.rs`, `language.rs`, `match_cmd.rs` multi-column) | **Keep, dormant** | The engine the skill drives; cheap to carry |
| Multilingual embedding model management/download | **Keep, dormant** | Activated on demand, never for English installs |
| Per-way localization in `template.rs` (the 17× tax) | **Remove** | New/edited ways are English-only by default |
| `ways tune` / `ways language` audit commands | **Keep, don't run by default** | Become the acceptance gate for adopter-run localization |
| Multilingual model in default `make setup` | **Make on-demand** | English install never fetches the 127MB model |

**English is fully dormant.** A default (English) install fetches one model, builds
one corpus, runs no locale audit, and pays no authoring tax. The multilingual engine
exists but is never touched until an adopter explicitly localizes.

**Three new components replace the maintainer obligation:**

1. **`ways-localize` skill** (`ways-*` family — kin to `ways-tests`, `ways-update`).
   The operator-facing orchestrator. It:
   - **Interviews** the operator (human) for the target language — conversational,
     and recognizable from a request *in that language* (resolving the bootstrap
     paradox: a non-English user shouldn't need to read English to get a non-English
     experience).
   - **Delegates the fan-out** to a re-hydrate-and-tune workflow (below).
   - **Sets Claude Code's own response language** by writing the `language` key in
     `settings.json` (the one supported mechanism — `{"language": "french"}`,
     persistent, effective next session). No env var or CLI flag exists.
   - Surfaces progress and the final "N ways localized, `ways tune` clean" summary
     **in the adopter's language**.

2. **Re-hydrate-and-tune workflow** (per the `meta/workflows` way). Given a target
   language, it fans out across ways: translate each way's `description` +
   `vocabulary` (Claude is multilingual), pack the stubs (`pack-locales.sh`), rebuild
   the corpus, and **verify fidelity/discrimination with `ways tune`**, iterating on
   flagged stubs until clean. `ways tune` is the objective, language-agnostic
   acceptance gate (it audits embedding geometry, not prose).

3. **State-triggered detection macro** (a `state`-triggered way). At
   SessionStart/PreCompact it reads the `language` field in `settings.json`. The
   check itself always runs — it must read the field to know the state — but only
   one state produces output; the cost on English installs is a single cheap file
   read, no model/embedding/output:
   - English or unset → emits nothing. Silenced on *output*, not on *existence*:
     the detector runs, finds nothing to do, and injects zero guidance (the ~99%
     case).
   - Non-English **and** no regenerated locale data exists for that language →
     injects a recommendation to invoke `ways-localize`. Because CC is already
     responding in that language, the nudge reaches the operator in their language
     for free.
   - **Self-silences**: once `ways-localize` rehydrates that language, the locale
     data exists → the condition is false → the macro goes quiet. (Cleaner than
     fire-count decay; the presence of localized data *is* the "done" signal.)

**Scope note.** This localizes *ways* only. A complete non-English experience also
needs localized **skills** and verification of Claude Code's native non-English skill
matching — tracked as a larger follow-on, not part of this decision.

## Validation

The central decision (shelve maintainer i18n) rests on *measured* facts: the usage
data (222 sessions / one English speaker) and the 2026-06-22 scout inventory (91
files, ~3,094 entries, 127MB second model, 17× tax, 3 CI targets). Those are read
directly from the repo and corpus — no external bet.

One premise is about Claude Code's *own* behavior and was probed before acceptance,
per the prototype-before-accept way:

- **Readable half (verified).** The detection macro reads the top-level `language`
  string in `settings.json` via `jq`. Probed 2026-06-22: the field is absent on this
  install → the macro correctly resolves to the silent (English/unset) state. The
  detector keys off a real, machine-readable field, not a hallucinated one.
- **Behavioral half (assumed, deferred).** That writing `{"language": "<lang>"}`
  actually switches CC's response language (docs: v2.1.176+, effective next session)
  is sourced from official docs, not observed — testing it requires a settings change
  + restart, intrusive to the authoring session. It is **secondary to the shelve**
  (it powers the skill's step 5 and the macro's trigger, not the decision to shelve),
  and has documented fallbacks (output-style, CLAUDE.md instruction) if it disproves.
  To be confirmed when `ways-localize` is built.

## Consequences

### Positive

- **Lighter default install.** English users skip a 127MB model download, a second
  corpus + embedding pass, the locale audit, and three CI targets.
- **Zero authoring tax.** New/edited ways are English-only; no 17 stub lines per way.
- **Cost and quality land on the beneficiary.** The native-speaker adopter pays the
  translation/tuning tokens *and* is the real expert on fidelity and idiom — better
  output than maintainer-authored stubs, at no maintainer cost.
- **Fully reversible engine.** Deleting data, not code, means re-enabling
  localization is `ways-localize`, not a rebuild.
- **The leak stops mattering.** We no longer maintain translations for the shrinking
  (ways) half while the growing (skills) half stays English.

### Negative

- **Non-English ways stop shipping out of the box.** Until an adopter runs
  `ways-localize`, only English ways match. (Mitigated: there are no measured
  non-English users today, and the detection macro surfaces the fix immediately.)
- **First-run localization cost moves to the adopter** — model download + a
  translate/tune token spend across all ways. (By design: the beneficiary pays.)
- **New moving parts to build and maintain** — a skill, a workflow, and a macro —
  replacing static data with orchestration.

### Neutral

- The Rust engine carries dormant code (corpus split, `tune`, `language`,
  multi-column match). It compiles and is tested but unexercised on English installs.
- The deleted `.locales.jsonl` data remains in git history; "delete" is recoverable.
- ADR-125 (Draft) keeps its *retrieval* model (coordinate-alias embedding, per-node
  thresholds); only its maintainer-side authoring obligation is superseded here.

## Alternatives Considered

- **Leave the stubs dormant in git, delete nothing (issue #160's original framing).**
  Rejected: the ~3,094 stale entries stay a maintenance and review-noise liability
  (they confused our own tuning pass), and the authoring tax persists if `template.rs`
  still emits them. Deleting the data while keeping the engine gets the same
  reversibility (git history) with none of the ongoing drag.
- **Delete the engine too (Rust intl code + model management).** Rejected: the engine
  is the cheap part and deleting it makes `ways-localize` a from-scratch rebuild
  rather than a re-hydration. Dormant code is a smaller liability than lost capability.
- **Keep maintainer-maintained translations.** Rejected on all four counts above:
  zero validated users, compounding cost, architectural leak, unverified substrate —
  and lower quality than a native speaker + Claude would produce.
- **Ship an English quick-start guide for localization.** Rejected as a bootstrap
  paradox: an English-only guide gates exactly the non-English users it's meant to
  serve. The conversational, in-language entry point (the skill + detection macro)
  replaces it.
