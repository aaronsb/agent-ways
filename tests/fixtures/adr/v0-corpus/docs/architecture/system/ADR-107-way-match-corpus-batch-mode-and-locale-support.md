---
status: Accepted
date: 2026-04-02
superseded_by:
  - ADR-125
deciders:
  - aaronsb
  - claude
related:
  - ADR-108
  - ADR-110
  - ADR-111
  - ADR-125
---

# ADR-107: Corpus, Matching Pipeline, and Locale Support

> **Note (2026-04-17):** The matching pipeline architecture described here (two-tier embedding → BM25 → keyword/regex) has been superseded by [ADR-125: Authored Disclosure Graph and Removal of BM25](ADR-125-authored-disclosure-graph-and-removal-of-bm25.md). BM25 is removed; the embedding model is the sole retrieval tier. Locale stubs remain as specified (packed `.locales.jsonl`, one file per way), but the per-locale `embed_threshold` field is removed — thresholds are per-node, in English frontmatter. Sections below describing the two-tier pipeline, BM25 stemmer selection, and the `bm25_stemmer` field in `languages.json` are historical; refer to ADR-125 for the current model.

## Context

This ADR was originally drafted when the matching system was a C binary (`way-match`) called N times per prompt by shell scanners. Since then, ADR-111 consolidated everything into a single Rust binary (`ways`). This rewrite reflects the shipped architecture and defines the locale support plan within it.

### What shipped (Phases 1 & 2)

**External corpus** — `ways corpus` generates `ways-corpus.jsonl` in `~/.cache/claude-ways/user/`. The corpus is a cache artifact, regenerated on demand, read-only at runtime. IDF is computed across the full corpus (85+ ways), not a hardcoded seed.

**Batch scoring** — `ways scan prompt --query "..." --session ID` scores all ways in one call. The Rust binary loads the corpus once, tokenizes the query once, and scores every way. Scanner hooks call this instead of N separate invocations.

**Matching pipeline (historical)** — ADR-108 added embedding (all-MiniLM-L6-v2, 98% accuracy). The original shipped pipeline was embedding → BM25 fallback → keyword/regex patterns with automatic engine selection by model availability. ADR-125 removed BM25; the current pipeline is embedding-only, with explicit `pattern:` / `commands:` regex triggers as a separate override surface.

**Security boundary preserved** — runtime scanners never write to `~/.claude/`. The corpus and embedding model live in `~/.cache/claude-ways/user/` (XDG cache). Regeneration is an explicit authoring operation (`ways corpus`, `make setup`).

### What remains: locale support

The matching pipeline is English-only:
- BM25 stemmer: hardcoded `Algorithm::English` in `bm25.rs:175`
- Stopwords: English-only array in `bm25.rs:12-21`
- Embedding model: `all-MiniLM-L6-v2` is English-only
- Way content: all 85+ ways are written in English

Claude Code has a `language` setting. Users in non-English locales type prompts in their language, but matching operates on English vocabulary. The gap: a Japanese user's prompt produces zero BM25 tokens (no whitespace boundaries) and low embedding similarity (English-only model).

## Decision

### Language resolution

A new `agents/` module provides a resolution cascade for output language:

1. `ways.json` `output_language` — explicit user override
2. Claude Code `settings.json` `language` — agent-level config (project then user scope)
3. System locale (`$LC_ALL` → `$LC_MESSAGES` → `$LANG`) — parsed from locale strings like `ja_JP.UTF-8`
4. Default: `en`

The `agents/` module defines an `AgentConfig` trait. `claude_code.rs` implements it for Claude Code. This is the abstraction point for supporting other CLI agents — each gets its own module implementing the same trait.

The resolved language affects:
- **Output directive**: `core.md`'s "must be in English" line is substituted at render time with the configured language. The agent writes commit messages, comments, and docs in the user's language.
- **BM25 stemmer selection**: `languages.json` maps language codes to `rust_stemmers::Algorithm` names.
- **Status display**: `ways status` shows the resolved language.

### Language configuration resource

`languages.json` is embedded at compile time. It defines the 52 languages supported by the multilingual embedding model (`paraphrase-multilingual-MiniLM-L12-v2`), even though the current shipping model is English-only. Each entry contains:

```json
{
  "ja": {
    "name": "Japanese",
    "native": "日本語",
    "bm25_stemmer": null
  },
  "de": {
    "name": "German",
    "native": "Deutsch",
    "bm25_stemmer": "German"
  }
}
```

- `name` / `native`: display and normalization (accepts codes, English names, or native names)
- `bm25_stemmer`: the `rust_stemmers::Algorithm` variant name, or `null` if BM25 cannot support this language

The `null` stemmer field is the honest signal. Languages where `bm25_stemmer` is null (CJK, Thai, Arabic, etc.) require the embedding engine for matching. BM25 cannot tokenize them — no whitespace boundaries, no suffix-stripping morphology. This is an architectural limitation, not a tuning problem.

### Matching: language coverage by engine

The matching pipeline runs: embedding → BM25 → keyword/regex. Each engine has different language coverage:

**Embedding (primary)** — with the multilingual model (`paraphrase-multilingual-MiniLM-L12-v2`), covers all 52 languages in `languages.json`. Cross-language matching works natively — a Japanese prompt about security produces a vector near the English `description: security vulnerability scanning`. This is the primary matching path for all non-English users.

**BM25 (fallback)** — covers the ~15 languages where Snowball stemmers exist (Romance, Germanic, Slavic, Turkic, Finnic). `languages.json` `bm25_stemmer` field identifies these. BM25 is the fallback when the embedding engine is unavailable (model not downloaded, `way-embed` binary missing).

For languages with `bm25_stemmer: null`, BM25 is architecturally incapable — not "stemmer not yet added." These fall into two categories:

- **No word boundaries**: Japanese, Chinese, Thai have no whitespace between words. BM25's tokenizer (`split on whitespace`) produces whole sentences as single tokens. A segmenter (MeCab, jieba, ICU) would be needed before BM25 could operate at all.
- **Non-concatenative morphology**: Arabic and Hebrew build words from consonant roots with vowel patterns interleaved (k-t-b → kataba, kitāb, maktūb). Snowball's suffix-stripping approach cannot extract these roots. The concept of "stemming" doesn't apply — these languages need root extraction, a fundamentally different operation.

These languages require the embedding engine. There is no BM25 path and adding one would mean replacing the tokenizer and morphological analyzer — at which point you've built a search engine, not a fallback.

**Keyword/regex (always)** — language-independent. Technical terms borrowed into all languages (`git commit`, `npm install`, file paths, error codes) match regardless of prompt language. This tier fires even when both embedding and BM25 miss.

**The practical implication:** for languages BM25 can't handle, the embedding engine is not optional — it's required. `ways status` should surface this: if the resolved language has `bm25_stemmer: null` and the embedding engine is unavailable, warn that matching will be limited to keyword/regex patterns only.

### Way content stays English

Way body content (the guidance injected into agent context) is NOT translated. Rationale:

- The agent reads English perfectly regardless of output language
- 85+ way files × N languages is a maintenance nightmare with divergence risk
- The guidance is for the agent's reasoning, not displayed to the user
- Cross-language injection is well-understood: English instructions → non-English output

### Native language stubs (shipped)

The original ADR-107 Draft proposed a tiered file model (`{name}-{lang}.md`). This was initially deferred in favor of cross-language embedding. However, evaluation data showed that native-language stubs dramatically outperform cross-language matching:

| Language | EN model × EN desc | Multi model × cross-lang | Multi model × native stub |
|----------|-------------------:|------------------------:|-------------------------:|
| ja       | -0.03              | 0.69                    | **0.93**                 |
| ar       | 0.04               | 0.40                    | **0.96**                 |
| de       | 0.08               | 0.62                    | **0.82**                 |
| es       | 0.44               | 0.79                    | **0.84**                 |

Native stubs are now the primary multilingual matching strategy. Each stub provides a `description` and `vocabulary` in the target language, scored by the multilingual embedding model.

### Packed locale storage (.locales.jsonl)

Stubs are stored as **packed JSONL**, one file per way, co-located with the way it belongs to:

```
ea/briefing/
  briefing.md              # the way (English)
  briefing.locales.jsonl   # all language stubs
```

```jsonl
{"lang":"ja","description":"朝のブリーフィング、昨夜の要約","vocabulary":"朝礼 ブリーフィング 要約 優先事項"}
{"lang":"de","description":"Morgendliches Briefing, Tagesübersicht","vocabulary":"Morgenbriefing Tagesübersicht Zusammenfassung"}
```

Design constraints:
- **No `embed_threshold` per locale entry** — per ADR-125, thresholds are per-node (English frontmatter only). Locale entries are coordinate aliases; they do not carry their own gates. The corpus generator uses the node's threshold (or system default) for all of a node's aliases.
- **No `embed_model`** in packed format — always `"multilingual"` for locale stubs.
- **Override mechanism**: if `briefing.ja.md` exists as a real file on disk, it supersedes the `ja` entry in `briefing.locales.jsonl`. This allows graduating any stub to a full native-language way with body content.
- **Co-location over aggregation**: one `.locales.jsonl` per way (not per language, not one global file). Way deletion = directory deletion, translations go with it.

This replaces the individual `{name}.{lang}.md` stub files (which would grow to 4,000+ files at full language coverage). The packed format keeps the training corpus version-controlled, diffable, and lintable while eliminating file sprawl.

### Dual embedding model (shipped)

Both models ship simultaneously. `make setup` downloads both:

| Model | Size | Languages | Use case |
|-------|------|-----------|----------|
| all-MiniLM-L6-v2 | 21MB | English | Precise EN matching (default) |
| paraphrase-multilingual-MiniLM-L12-v2 | 127MB | 52 | Native-language stub matching |

`ways corpus` splits entries by `embed_model` field into two corpora (`ways-corpus-en.jsonl`, `ways-corpus-multi.jsonl`). The scanner queries both and merges results. Each way's English entry is scored by the EN model; each locale stub is scored by the multilingual model.

`languages.json` defines the supported language set for the multilingual model. Adding a language means verifying it's in the model's training data and adding the entry — no code changes.

### Embedding model language verification

`languages.json` declares what languages we *intend* to support. The embedding model determines what we *actually* support. These must be verified to match.

A test fixture per language validates that the model produces meaningful cross-language similarity. Each fixture contains a prompt in the target language and an English way description expressing the same intent. The test embeds both and checks that cosine similarity exceeds a minimum threshold (e.g., 0.25 — well below the matching threshold but above random noise).

```jsonl
{"lang": "ja", "prompt": "依存関係の脆弱性をチェックして", "description": "dependency vulnerability scanning", "min_similarity": 0.25}
{"lang": "de", "prompt": "Abhängigkeiten auf Schwachstellen prüfen", "description": "dependency vulnerability scanning", "min_similarity": 0.25}
{"lang": "ko", "prompt": "의존성 취약점 검사", "description": "dependency vulnerability scanning", "min_similarity": 0.25}
```

When the test runs against the current English-only model (`all-MiniLM-L6-v2`), most non-English languages will fail — that's expected and informative. It tells us exactly which languages gain support when we swap to the multilingual model. When we do swap, the same tests validate the new model's coverage without manual verification.

The test is run as: `ways embed-test-languages` or as part of `make test`. It reads `languages.json`, loads the model, and reports per-language pass/fail. Any language that fails gets flagged — either the model doesn't support it, or the test fixture needs revision.

This makes model selection empirical: run the tests against candidate models, pick the one that passes the languages you need at the size you can tolerate.

## Consequences

### Positive

- Output language works immediately for all languages — no model or matching changes required
- `agents/` module provides the abstraction point for multi-agent support
- Language resolution cascade respects user intent at every level
- `languages.json` as embedded resource means the language list is data, not code
- BM25 stemmer selection is a one-line change per language in `bm25.rs`
- Embedding model upgrade is a config change, not an architecture change

### Negative

- Multilingual matching requires a 6x larger embedding model (21MB → 120MB)
- BM25 fallback quality varies significantly across language families
- CJK/Thai users get no BM25 matching — embedding engine is required, not optional
- Cross-language embedding similarity is lower than same-language — thresholds may need per-language tuning

### Neutral

- Way content stays English — no translation infrastructure needed
- Packed `.locales.jsonl` replaces per-language stub files — same data, fewer files
- Override mechanism (`{name}.{lang}.md` supersedes JSONL entry) allows gradual migration from stubs to full native-language ways
- `ways.json` `output_language: "en"` is the default — zero behavior change for existing users

## References

- ADR-108: Embedding-Based Way Matching with all-MiniLM-L6-v2
- ADR-110: Way File Separation and Graph-Compatible Structure
- ADR-111: Unified Ways CLI — Single Binary Tool Consolidation
- ADR-125: Authored Disclosure Graph and Removal of BM25 (supersedes the matching pipeline described here)
- `tools/ways-cli/src/agents/` — Agent config module (Claude Code, system locale)
- `tools/ways-cli/languages.json` — Supported language definitions
- [paraphrase-multilingual-MiniLM-L12-v2](https://huggingface.co/sentence-transformers/paraphrase-multilingual-MiniLM-L12-v2) — Multilingual embedding model
- [rust_stemmers](https://docs.rs/rust-stemmers/) — Snowball stemmer implementations (removed with BM25 per ADR-125)
