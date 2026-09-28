---
status: Accepted
date: 2026-03-30
deciders:
  - aaronsb
  - claude
related:
  - ADR-014
  - ADR-107
  - ADR-108
  - ADR-110
---

# ADR-111: Unified `ways` CLI — Single Binary Tool Consolidation

## Context

The ways tooling has grown organically across multiple languages and entry points:

| Tool | Language | Lines | Function |
|------|----------|-------|----------|
| `way-match` | C | ~920 | BM25 scoring |
| `way-embed` | C++ | ~920 | Embedding match (ONNX/GGUF) |
| `generate-corpus.sh` | Bash | ~200 | Corpus generation |
| `lint-ways.sh` | Bash | ~530 | Frontmatter validation |
| `way-tree-analyze.sh` | Bash | ~300 | Tree structure analysis |
| `embed-lib.sh` | Bash | ~200 | Shared utilities |
| `embed-suggest.sh` | Bash | ~100 | Embedding suggestions |
| `provenance-scan.py` | Python | ~150 | Provenance scanning |
| `governance.sh` | Bash | ~540 | Governance orchestration |
| Various others | Bash | ~500 | Misc utilities |

Every tool re-walks the same directory tree and re-parses the same frontmatter. Adding a new feature (e.g., the graph generator from ADR-110) means writing yet another script that duplicates file discovery, YAML extraction, and JSON emission. The bash scripts also carry macOS bash 3.2 compatibility constraints that a compiled binary eliminates.

The `gh` CLI, `aws` CLI, and `gcloud` CLI demonstrate the pattern: one binary, subcommands for everything, shared infrastructure for common operations.

## Decision

### 1. Create a `ways` CLI binary

A single `ways` binary replaces all current tooling with subcommands:

```
ways lint [path]           # frontmatter validation (lint-ways.sh)
ways corpus [--global]     # corpus generation (generate-corpus.sh)
ways match <query>         # BM25 scoring (way-match)
ways embed <query>         # embedding match (way-embed)
ways siblings <id>         # way-vs-way cosine scoring (new, ADR-110 §5)
ways graph [--format jsonl]# graph export (new, ADR-110 §4)
ways tree <path>           # tree analysis (way-tree-analyze.sh)
ways provenance            # provenance scanning (provenance-scan.py)
```

### 2. Pure Rust implementation

The entire CLI is pure Rust — no FFI, no C/C++ compilation. BM25 scoring was reimplemented natively (~176 lines in `bm25.rs`). Embedding matching delegates to the existing `way-embed` binary via subprocess (the embedding engine requires GGUF/ONNX runtime which remains a separate C++ binary).

The original ADR planned FFI wrappers via the `cc` crate, but the BM25 C code was small enough to port directly. This eliminated cross-compilation complexity entirely — `cargo build` produces the binary with no native toolchain required beyond Rust.

### 3. Project structure

```
tools/ways-cli/
├── Cargo.toml
├── src/
│   ├── main.rs              # clap dispatcher (19 subcommands)
│   ├── cmd/
│   │   ├── scan/            # prompt/command/file/state matching
│   │   ├── show/            # session-aware way display
│   │   ├── governance/      # 9 governance query modes (7 files)
│   │   ├── lint.rs          # frontmatter validation
│   │   ├── corpus.rs        # corpus generation
│   │   ├── match_bm25.rs    # BM25 scoring (pure Rust)
│   │   ├── embed.rs         # embedding match (delegates to way-embed)
│   │   ├── list.rs          # session way list with forecast
│   │   ├── context.rs       # token usage from transcript
│   │   ├── reset.rs         # session state recovery
│   │   └── ...              # graph, tree, provenance, stats, etc.
│   ├── bm25.rs              # BM25 engine (Porter2 stemming, IDF)
│   ├── scanner.rs           # shared: file discovery by frontmatter
│   ├── frontmatter.rs       # shared: YAML frontmatter parsing
│   ├── session.rs           # session state (directory-per-session)
│   ├── table.rs             # ANSI-aware table formatting
│   └── util.rs              # shared utilities (home_dir, project detection)
├── tests/
│   └── session_sim.rs       # 8 integration scenarios
└── download-ways.sh         # pre-built binary installer
```

### 4. Incremental delivery

Subcommands ship independently. The order follows dependency:

| Phase | Subcommands | Replaces | Status |
|-------|-------------|----------|--------|
| 1 | `lint`, `corpus`, `graph` | lint-ways.sh, generate-corpus.sh, new | Shipped |
| 2 | `match`, `embed`, `siblings` | way-match (ported to Rust), way-embed (subprocess) | Shipped |
| 3 | `tree`, `provenance`, `scan`, `show`, `governance`, `context`, `list`, `stats`, `reset`, `init`, `status`, `suggest` | All remaining scripts | Shipped |

All three phases delivered as pure Rust. BM25 was ported rather than wrapped via FFI. Embedding delegates to the existing `way-embed` binary. The `scan` and `show` subcommands absorbed the hook orchestration that was previously spread across show-core.sh, show-way.sh, match-way.sh, and check-prompt.sh.

### 5. Installation and distribution

The binary installs to `~/.claude/bin/ways` with a symlink to `~/.local/bin/ways`. Three install paths:

1. **Download** — `download-ways.sh` pulls pre-built binary from GitHub Releases (no toolchain needed)
2. **Build from source** — `cargo build --release` (requires Rust toolchain)
3. **`make install`** — tries download first, falls back to build

CI builds 4 platforms (linux-x86_64, linux-aarch64, darwin-x86_64, darwin-arm64) via `cargo-zigbuild` for ARM cross-compilation. Tagged releases (`ways-v*`) create GitHub Releases with checksums.

The 10 remaining hook scripts are thin dispatchers — they parse hook JSON input and call `ways scan` or read session state. The orchestration, matching, and display logic lives entirely in the binary.

### 6. Remaining C/C++ (embedding engine)

BM25 was ported to pure Rust (176 lines in `bm25.rs`). The embedding engine (`way-embed`) remains a separate C++ binary because it depends on llama.cpp for GGUF model inference. The `ways embed` subcommand delegates to `way-embed` via subprocess.

Future option: the `ort` crate could replace the C++ embedding binary with a pure Rust ONNX path, eliminating the last subprocess dependency. This is not planned — the current approach works and the embedding binary is stable.

## Consequences

### Positive

- Single binary, single install, single update path
- Shared file scanning — one tree walk serves all subcommands
- Shared frontmatter parsing — one YAML parser, tested once
- New features (graph, siblings) are subcommands, not new scripts
- macOS bash 3.2 compatibility concerns eliminated for ported logic
- `ways --help` gives discoverability across all tooling
- Shell completion for free via `clap`

### Negative

- Rust toolchain required for development (not for end users — pre-built binaries available)
- CI cross-compiles for 4 platforms via `cargo-zigbuild` (working, but adds build complexity)
- Porting bash to Rust took more lines for the same functionality (mitigated: type system caught bugs the bash scripts silently swallowed)

### Neutral

- `governance.sh` (543 lines), `provenance-verify.sh`, and `context-usage.sh` were deleted — fully replaced by `ways governance`, `ways governance lint`, and `ways context`
- 10 bash hook scripts remain as thin dispatchers (15-30 lines each) — they parse hook JSON and call `ways scan`
- `Makefile` has `make ways`, `make ways-rebuild`, `make install`, `make release` targets
- CI builds changed from "compile C, compile C++, run bash" to "cargo build, run tests"
- Session state moved from flat `/tmp/.claude-*-{uuid}` markers to per-user `/tmp/.claude-sessions-{uid}/{session_id}/` directories

## Alternatives Considered

### Pure Go

Go's `cobra` library is excellent for CLIs and cross-compilation is normally trivial. However, ONNX Runtime is a C library — Go requires CGo to call it. CGo cross-compilation for 4 platforms requires Docker-based toolchains or zig-cc as a C cross-compiler, negating Go's primary advantage. The CGo boundary is also more awkward than Rust's `cc` crate integration.

### Pure Rust (rewrite everything)

Porting the C/C++ inference code to Rust risks subtle behavioral differences in numerics-sensitive paths (BM25 scoring, embedding normalization). The `ort` crate for ONNX is solid but static linking of ONNX Runtime remains uneven across platforms. The incremental approach (Rust shell + C/C++ FFI) gets to a working binary faster and keeps the option open.

### Extend C++

C++ is the right language for the inference engine but the wrong language for directory walking, YAML parsing, CLI dispatch, and JSON emission — which is 80% of the work. Libraries like `yaml-cpp` and `CLI11` exist but are a step down from `serde_yaml` and `clap`. The bash scripts exist precisely because C++ was too high-friction for the scripting layer.

### Go + C/C++ library (CGo)

Same FFI benefit as Rust + `cc`, but CGo cross-compilation is harder than Rust's `cargo-zigbuild`, and maintaining three languages (Go + C + C++) is worse than two (Rust + C/C++).

## Interaction with Other ADRs

| ADR | Interaction |
|-----|-------------|
| ADR-014 | `way-match` binary becomes `ways match` subcommand. BM25 algorithm unchanged |
| ADR-107 | Corpus generation becomes `ways corpus`. Locale support (Phase 3) becomes a flag |
| ADR-108 | `way-embed` binary becomes `ways embed` subcommand. ONNX/GGUF loading unchanged |
| ADR-110 | Graph export (`ways graph`) and sibling scoring (`ways siblings`) ship as subcommands rather than standalone scripts |

## Extension: `attend` adopts the same pattern (2026-05-09)

The `attend` binary (ADR-113) initially shipped a hand-rolled argv dispatcher with per-command help text written as free-form `println!` calls. As the surface grew to ~14 subcommands, the lack of a uniform help/argument-parsing layer started to bite — `--help` worked on some commands, errored as "unknown subcommand" on others, and silently ran the command on a third group. Rather than introduce a parallel CLI convention, `attend` adopts the clap-derive structure established here: a `Cli` struct, a `Commands` enum, doc comments as the source of truth for help text, and the same `agent_fmt::Banner` special-casing for bare invocation. External reference docs (`docs/cli/attend.md`) are generated from the same `Cli` definition via `clap-markdown`, wired into the project Makefile so the runtime help text and the published reference cannot drift. This decision retroactively confirms the ADR-111 pattern as the canonical CLI shape across the workspace.
