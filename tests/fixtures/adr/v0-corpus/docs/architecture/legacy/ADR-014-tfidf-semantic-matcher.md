---
status: Accepted
date: 2026-02-16
deciders:
  - aaronsb
  - claude
related:
  - ADR-013
---

# ADR-014: TF-IDF/BM25 Binary for Semantic Way Matching

## Context

Ways use three matching modes: regex, semantic (gzip NCD), and state triggers. A fourth mode — model-based matching via `claude -p` subprocess (`model-match.sh`) — is wired up but unused, and increasingly fragile due to Anthropic's January 2026 crackdown on third-party tools invoking Claude subscriptions programmatically (see [References](#references)).

The semantic mode (`semantic-match.sh`) combines two techniques:

1. **Keyword counting** — count how many vocabulary words appear in the prompt (match if >= 2)
2. **Gzip NCD** — information-theoretic similarity via compression ratio

This works but has known weaknesses:

- **Gzip NCD is surface-level**: it detects shared byte patterns, not shared meaning. "optimize database queries" and "speed up SQL" share no bytes but are semantically close. NCD misses this.
- **Threshold tuning is fragile**: each way needs a hand-tuned NCD threshold (0.52–0.58), and the score is sensitive to prompt length — a long prompt dilutes the signal from a short description.
- **Keyword counting is brittle**: requires manually curated vocabulary lists per way. Misses synonyms, abbreviations, and natural phrasing.
- **No term weighting**: "test" in a prompt about "testing frameworks" and "test" in "put it to the test" are treated identically. No concept of term importance.

Seven ways currently use semantic matching (api, config, debugging, design, security, testing, adr-context). As the corpus grows, false positives and missed matches will increase.

TF-IDF and BM25 address these issues with term-frequency weighting and inverse-document-frequency discrimination — without requiring any ML model, GPU, or external service.

## Decision

Build a single portable binary (`way-match`) using Cosmopolitan Libc (APE format) that:

1. Accepts way descriptions + vocabulary as a **corpus** and a **prompt** as query
2. Computes **BM25 relevance scores** between the prompt and each way
3. Returns ranked matches above a configurable threshold
4. Runs on Linux (amd64/arm64), macOS (amd64/arm64), and Windows (amd64) from a single binary

### Interface

```
# Batch mode: score prompt against all ways at once
way-match score --corpus ways.jsonl --query "optimize my database queries"

# Output: ranked matches, one per line
# way_id<TAB>score<TAB>description_snippet
testing    0.12    writing unit tests...
debugging  0.31    debugging code issues...
design     0.87    software system design...

# Single pair mode (drop-in for semantic-match.sh)
way-match pair --description "software system design..." \
               --vocabulary "architecture pattern database..." \
               --query "optimize my database queries" \
               --threshold 0.4

# Exit 0 if match, 1 if not (compatible with current script interface)
```

### Corpus file format

```jsonl
{"id":"design","description":"software system design architecture patterns...","vocabulary":"architecture pattern database schema..."}
{"id":"testing","description":"writing unit tests, test coverage...","vocabulary":"unittest coverage mock tdd..."}
```

Generated at install time or on first run by scanning way frontmatter. Cached and regenerated when way files change (mtime check).

### BM25 parameters

| Parameter | Default | Notes |
|-----------|---------|-------|
| k1        | 1.2     | Term frequency saturation |
| b         | 0.75    | Document length normalization |
| threshold | 0.4     | Minimum score to report (tunable per-way via frontmatter override) |

These are the standard BM25 defaults from the literature. The threshold replaces the current NCD threshold and can be carried forward in way frontmatter.

### IDF computation

IDF is computed over the way corpus itself (currently ~33 documents). This is a small corpus, but the IDF still provides signal: "test" appears in many way descriptions (low IDF), "owasp" appears in one (high IDF). Terms from the vocabulary field are indexed alongside the description to enrich the document representation.

The query (user prompt) is tokenized and scored against each way's combined description+vocabulary document.

### Integration with existing matching

```
check-prompt.sh
  ├── regex ways     → pattern match (unchanged)
  ├── semantic ways  → way-match binary (replaces semantic-match.sh)
  │                    falls back to gzip NCD if binary absent
  └── state ways     → check-state.sh (unchanged)
```

The `semantic-match.sh` script is replaced by a call to `way-match pair` with the same interface contract (exit 0/1). The `match: semantic` frontmatter field continues to work unchanged. The `threshold:` field is reinterpreted as a BM25 threshold instead of NCD threshold (values will differ and need one-time recalibration).

The `match: model` mode (`model-match.sh`) remains in the codebase but is not invested in further. No ways use it today, and Anthropic's tightening of `claude -p` subprocess usage makes it an unreliable foundation. If LLM-level matching is needed in the future, a local embedding approach (see Alternatives) is a more sustainable path than depending on subscription-gated CLI invocations.

Batch mode (`way-match score --corpus`) is available for future optimization: score all semantic ways in one invocation instead of N separate calls.

### Repo organization

Source and binary live in the claude-code-config repo (not a submodule). The tool has exactly one consumer — the ways matching system — and is ~500-1000 lines of C. A separate repo adds git submodule overhead with no real benefit for a single-file, single-purpose tool.

```
tools/
  way-match/
    way-match.c          # BM25 implementation (~500-1000 lines)
    Makefile              # cosmocc build targets
bin/
  way-match              # Built APE fat binary (checked in, ~100-300KB)
```

`.gitignore` additions:
```gitignore
# Tools (source + build)
!tools/
!tools/way-match/
!tools/way-match/*

# Built binaries (checked in, cross-platform APE)
!bin/
!bin/way-match
```

### Build and distribution

- Source: C, ~500-1000 lines estimated
- Compiler: `cosmocc` (Cosmopolitan toolchain — bundles its own gcc/clang, no system compiler needed)
- Build deps: cosmocc only (download + unzip, ~60MB). Only needed to rebuild — not to use.
- Output: single APE binary, estimated ~100-300KB
- No dynamic linking, no runtime dependencies
- Checked into repo at `bin/way-match` (fat binary covers linux/macos/windows, amd64/arm64)
- Fallback: if binary missing or fails, fall back to `semantic-match.sh` (gzip NCD)

```makefile
# Makefile sketch
COSMOCC ?= $(HOME)/.cosmocc/bin/cosmocc

bin/way-match: tools/way-match/way-match.c
	$(COSMOCC) -O2 -o $@ $<

verify: tools/way-match/way-match.c
	$(COSMOCC) -O2 -o /tmp/way-match-verify $<
	@if cmp -s bin/way-match /tmp/way-match-verify; then \
		echo "PASS: binary matches source"; \
	else \
		echo "MISMATCH: checked-in binary differs from source build"; \
		echo "  checked-in: $$(sha256sum bin/way-match)"; \
		echo "  from source: $$(sha256sum /tmp/way-match-verify)"; \
	fi
	@rm -f /tmp/way-match-verify

clean:
	rm -f bin/way-match
```

### Trust and verification

The binary is checked in for convenience, but **users should never need to trust it blindly**. The trust model:

1. **Source is adjacent**: `tools/way-match/way-match.c` is a single readable C file, ~500-1000 lines, no obfuscation, no vendored blobs
2. **Build is trivial**: one `cosmocc` invocation, no configure step, no fetched dependencies
3. **`make verify`**: builds from source and compares against the checked-in binary
4. **`make bin/way-match`**: users can always build their own and ignore the checked-in copy
5. **CI can enforce**: a GitHub Action can run `make verify` on PRs that touch `bin/way-match` to ensure the binary matches the source

The checked-in binary is a convenience for users who don't want to install cosmocc. It is never the only option. Anyone uncomfortable with it can build from source in under 10 seconds.

### Trust chain principle

The ways system is deliberately built on tools already present on the machine — bash, gzip, jq, bc. This is a design principle, not an accident. Every layer of the matching system must be auditable and replaceable:

```
Trust tier 0 (always available):  bash, gzip, wc, bc  →  NCD fallback
Trust tier 1 (source-auditable):  way-match binary     →  BM25 scoring
Trust tier 2 (external service):  model-match.sh       →  LLM classification
```

The fallback path (`semantic-match.sh`) runs on POSIX builtins — it would work on a busybox instance. The binary is an upgrade in quality, not a hard dependency. If `bin/way-match` is missing, corrupt, or untrusted, the system degrades to gzip NCD automatically. No way ever fails to match because the binary isn't there.

### Tokenization

Simple whitespace + punctuation splitting, lowercased, with the existing stopword list. No stemming in v1 — keeps the implementation minimal and the binary small. Stemming (Porter or similar) is a future enhancement if needed.

## Consequences

### Positive
- **Better matching quality**: BM25 handles term importance — rare domain terms score higher than common ones
- **Length-insensitive**: BM25's length normalization handles short descriptions vs long prompts naturally (gzip NCD degrades here)
- **Single threshold semantic**: one scoring model instead of keyword-count OR NCD, reducing dual-path complexity
- **Cross-platform**: one binary for Linux/macOS/Windows, amd64/arm64
- **Fast**: BM25 over 33 documents is microseconds, well under the current gzip NCD latency (which forks gzip 3x per way)
- **No model, no API, no GPU**: pure computation, fully offline
- **Graceful fallback**: gzip NCD script remains as fallback

### Negative
- **New build dependency**: Cosmopolitan toolchain for compilation (though output is a static binary)
- **Binary in repo**: ~100-300KB checked-in binary (acceptable for a cross-platform fat binary)
- **Threshold recalibration**: existing per-way thresholds need one-time adjustment from NCD scale to BM25 scale
- **Still not embeddings**: BM25 won't catch "make it faster" → "performance optimization" without shared terms. Vocabulary field partially mitigates this.

### Neutral
- Batch mode enables future architectural changes (score all ways per prompt in one call) but doesn't require them immediately
- The `vocabulary` field becomes more valuable as BM25 document enrichment rather than a flat keyword list
- The `model` matching mode remains in codebase but is not actively invested in

## Alternatives Considered

### Keep gzip NCD, tune thresholds
Rejected: fundamental limitation — NCD measures byte-level redundancy, not term importance. No amount of threshold tuning fixes "optimize SQL" vs "speed up database queries".

### Embeddings (fastembed, llama.cpp)
Deferred: would require shipping a model file (25-130MB) alongside the binary. Dramatically better semantic understanding but violates the "zero dependencies, tiny binary" constraint. Could be a future ADR-015 if BM25 proves insufficient.

### Python implementation (scikit-learn TF-IDF)
Rejected: adds Python runtime dependency. The ways system is currently pure bash + coreutils. A compiled binary preserves the "just works" property.

### TF-IDF instead of BM25
Rejected: BM25 is strictly better for this use case — it adds term frequency saturation (diminishing returns for repeated terms) and document length normalization, both relevant when matching short descriptions against variable-length prompts. Implementation complexity is nearly identical.

### WASM binary instead of Cosmopolitan APE
Considered: would require a WASM runtime (wasmtime, wasmer). APE is more self-contained — the binary IS the runtime.

## Validation

A test harness will compare BM25 against gzip NCD on the actual way corpus to validate the upgrade. Tests live alongside the source in `tools/way-match/` and cover:

### Correctness tests
- **True positives**: prompts that should match a specific way do match (e.g., "add unit tests for the auth module" → testing way)
- **True negatives**: prompts that should not match a way don't (e.g., "what's for lunch" → no match)
- **Synonym/paraphrase coverage**: prompts using different words for the same concept (e.g., "speed up SQL" should match the same way as "optimize database queries")

### Comparative tests (BM25 vs gzip NCD)
- Run both scorers against a shared test fixture of prompt/expected-way pairs
- Report match/miss matrix: cases where BM25 matches but NCD misses (expected wins), and vice versa (regressions to investigate)
- Measure score distributions to calibrate BM25 thresholds against real prompts

### Performance tests
- Latency comparison: BM25 single invocation vs gzip NCD (which forks gzip 3x per way, ~7 ways = ~21 subprocess spawns)
- Batch mode throughput: score all semantic ways in one invocation

Test fixtures are a JSONL file of `{"prompt": "...", "expected_way": "...", "should_match": true}` entries, curated from real usage patterns and known NCD failure cases.

## References

- [Stop using Claude for OpenClaw and OpenCode](https://generativeai.pub/stop-using-claudes-api-for-moltbot-and-opencode-52f8febd1137) — Jan 2026, context on Anthropic restricting programmatic Claude subscription usage
- [You might be breaking Claude's ToS without knowing it](https://blog.devgenius.io/you-might-be-breaking-claudes-tos-without-knowing-it-228fcecc168c) — Jan 2026, ToS implications of `claude -p` subprocess patterns
- [Please stop using OpenClaw](https://www.xda-developers.com/please-stop-using-openclaw/) — coverage of Anthropic cease-and-desist and account bans
- [Cosmopolitan Libc](https://github.com/jart/cosmopolitan) — Actually Portable Executable toolchain
- [llamafile](https://github.com/Mozilla-Ocho/llamafile) — proof of Cosmopolitan APE at scale (multi-GB ML binaries)
