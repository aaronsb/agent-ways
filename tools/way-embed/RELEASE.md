Pre-built binaries and model for ADR-108 embedding-based way matching.

## Quick Install

One command — downloads the right binary for your platform + model, generates corpus:

```bash
cd "${XDG_DATA_HOME:-$HOME/.local/share}/agent-ways" && make setup
```

This will:
1. Download the pre-built binary for your OS/arch from this release
2. Download the Q5_K_M model (21MB)
3. Regenerate the corpus with embedding vectors
4. Run verification tests

If no pre-built binary exists for your platform, it builds from source automatically.

### Manual install

```bash
# Download binary + model separately
bash ${XDG_DATA_HOME:-$HOME/.local/share}/agent-ways/tools/way-embed/download-binary.sh
bash ${XDG_DATA_HOME:-$HOME/.local/share}/agent-ways/tools/way-embed/download-model.sh

# Regenerate corpus
ways corpus

# Verify
bash ${XDG_DATA_HOME:-$HOME/.local/share}/agent-ways/tools/way-embed/test-embedding.sh
```

## Available platforms

| Binary | Platform |
|--------|----------|
| `way-embed-linux-x86_64` | Linux x86_64 |
| `way-embed-linux-aarch64` | Linux ARM64 |
| `way-embed-darwin-x86_64` | macOS Intel |
| `way-embed-darwin-arm64` | macOS Apple Silicon |

## Build from source

If your platform isn't listed:

```bash
cd "${XDG_DATA_HOME:-$HOME/.local/share}/agent-ways" && make setup
```

Requires: cmake, C++ compiler, git (for submodule).

## Verify model provenance

Download the model directly from HuggingFace instead of the release:

```bash
bash ${XDG_DATA_HOME:-$HOME/.local/share}/agent-ways/tools/way-embed/download-model.sh --upstream
```

Both paths verify against the same SHA-256 checksum.

## Engine

Retrieval is embedding-only — the `way-embed` binary and the GGUF model are hard dependencies of `ways` after ADR-125. There is no engine toggle.
