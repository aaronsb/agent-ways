# Testing

Tests live here and in the tools they validate. This page covers all test types across the project.

```bash
# Run all automated tests from one place
tests/run-all.sh
```

## Way Matching Tests

Three layers, from fast/automated to slow/interactive.

### 1. Session Simulation Tests (Rust integration)

Rust-based tests that exercise the `ways` binary against fixture ways. Validates scan, matching, session state, and epoch tracking.

```bash
make test-sim
# or directly:
cargo test --manifest-path tools/ways-cli/Cargo.toml --test session_sim -- --test-threads=1
```

**What it covers**: End-to-end scan pipeline, embedding scoring against real frontmatter, session marker lifecycle, epoch distance calculations.

### 2. Embedding Engine Tests

Validates the embedding tier (all-MiniLM-L6-v2) on the shared fixture set.

```bash
# Embedding-specific validation (15 tests)
bash tools/way-embed/test-embedding.sh
```

**Current baseline**: Embedding 98.4% (63/64), 0 false negatives.

### 3. Activation Test (live agent + subagent)

Interactive test protocol that verifies the full hook pipeline in a running Claude Code session. Tests regex matching, semantic matching, co-activation, negative controls, and subagent injection.

**To run**: Start a fresh session from `~/.claude/` and type:

```
read and run the activation test at tests/way-activation-test.md
```

Claude reads the test file (avoiding prompt-hook contamination), then walks you through 9 steps:

| Step | Who | Tests |
|------|-----|-------|
| 1 | Claude | Session baseline (no premature domain activation) |
| 2 | User types prompt | Regex pattern matching (delivery/commits) |
| 3 | User types prompt | Semantic matching, established way (code/security) |
| 4 | User types prompt | Semantic matching, newer way (code/performance) |
| 5 | User types prompt | Co-activation of multiple related ways |
| 6 | User types prompt | Negative control (no false positives) |
| 7 | Claude | Subagent injection (Testing Way via SubagentStart) |
| 8 | Claude | Subagent negative (no fresh injection; parent context OK) |
| 9 | Claude | Summary table |

Takes about 5 minutes. **Current baseline**: 8/8 PASS (steps 1-8).

### 4. Multilingual Matching Test (live agent)

Interactive test protocol that verifies locale stubs fire correctly for non-English prompts across 12 languages and 5 script families.

**To run**: Start a fresh session from `~/.claude/` and type:

```
read and run the multilingual test at tests/multilingual-test.md
```

Claude reads the test file, then walks you through 16 steps:

| Steps | Script | Languages | Tests |
|-------|--------|-----------|-------|
| 1-4 | Latin | de, es, fr, pt-br | Locale vocabulary matching for familiar scripts |
| 5-7 | CJK | ja, ko, zh | Cross-script embedding matching |
| 8-9 | Cyrillic | ru, uk | Cyrillic vocabulary matching |
| 10 | Arabic | ar | Right-to-left script matching |
| 11-12 | Thai, Devanagari | th, hi | Southeast Asian and Indic scripts |
| 13-14 | Latin | en, it | Cross-language consistency (same concept, two languages) |
| 15 | Latin | nl (inactive) | Negative: inactive language falls back to English |
| 16 | — | — | Summary table |

Takes about 10 minutes. Tests all 18 active languages are reachable.

### Ad-Hoc Testing with /ways-tests

The `/ways-tests` skill and `ways` CLI provide targeted testing without writing scripts:

```bash
# Score a specific way against a prompt
/ways-tests score security "how do i hash passwords with bcrypt"

# Rank all ways against a prompt (check discrimination)
/ways-tests score-all "write some unit tests for this module"

# Vocabulary gap analysis
/ways-tests suggest security

# Validate frontmatter
/ways-tests lint --all

# Sibling vocabulary overlap
ways siblings softwaredev/code/supplychain/depscan/node
```

## Documentation Tests

### Doc-Graph (link integrity)

Builds a link graph from all git-tracked markdown files. Finds dead ends, orphans, and broken internal links.

```bash
bash scripts/doc-graph.sh --stats     # broken links, orphans, dead ends
bash scripts/doc-graph.sh --mermaid   # Mermaid diagram of link graph
bash scripts/doc-graph.sh --json      # JSON adjacency list
bash scripts/doc-graph.sh --all       # all outputs
```

**What it covers**: Every internal markdown link resolves to a real file. No orphaned docs (unreachable from any other doc). No dead ends (docs with no outgoing links to the rest of the tree).

### Governance Provenance Verification

Validates that provenance metadata is structurally sound: policy URIs point to real files, verified dates aren't stale, controls have justifications.

```bash
ways governance lint              # human-readable report
ways governance lint --json       # machine-readable
ways governance report            # full coverage report
```

**What it covers**: Provenance chain integrity — every `policy.uri` in provenance sidecars resolves, every control has justifications, verified dates are within staleness window.

## When to Run Which

| Scenario | Test |
|----------|------|
| Changed `ways` CLI source code | `make test-sim` |
| Changed a way's vocabulary or threshold | `/ways-tests score` + `/ways-tests score-all` |
| Changed locale stubs or active languages | `make test-locales` + multilingual test |
| Changed hook scripts (check-*.sh, inject-*.sh) | Activation test |
| Added a new way | `/ways-tests score` + `/ways-tests lint` + activation test |
| Restructured way directories | All three test layers |
| Changed embedding engine or model | `tools/way-embed/test-embedding.sh` |
| Renamed or moved documentation files | Doc-graph |
| Changed provenance metadata | Governance verification |
| Sanity check after merge | All of the above |
