# way-embed: Embedding-based semantic matcher for the ways system
# Builds against llama.cpp (git submodule) via CMake.
#
# Per-platform native binaries. No cross-compilation — build on each target.
# Platform detection at setup time selects the right binary.

ifeq ($(OS),Windows_NT)
    SHELL := C:/Program Files/Git/usr/bin/bash.exe
    .SHELLFLAGS := -c
endif

BUILDDIR  = build
BIN       = ../../bin/way-embed
CMAKE_FLAGS = -DCMAKE_BUILD_TYPE=Release

# Platform detection
OS    := $(shell uname -s | tr '[:upper:]' '[:lower:]')
ARCH  := $(shell uname -m)
PLATFORM = $(OS)-$(ARCH)

# Platform-tagged binary (e.g., way-embed-linux-x86_64)
PLATFORM_BIN = ../../bin/way-embed-$(PLATFORM)

# On Windows (MSYS/MinGW), MSVC multi-config generators put the output in
# a config subdirectory and add .exe. Detect and handle both cases.
IS_WINDOWS := $(if $(filter mingw% cygwin% msys%,$(OS)),yes,$(if $(findstring nt-,$(OS)),yes,))
ifeq ($(IS_WINDOWS),yes)
  BUILT_BIN      = $(BUILDDIR)/Release/way-embed.exe
  CMAKE_BUILD_FLAGS = --config Release
else
  BUILT_BIN      = $(BUILDDIR)/way-embed
  CMAKE_BUILD_FLAGS =
endif

# Default: build for this platform and install as way-embed
$(BIN): cmake-build
	cp $(BUILT_BIN) $(PLATFORM_BIN)
	cp $(PLATFORM_BIN) $(BIN)
	@echo ""
	@echo "Built: $(PLATFORM_BIN) ($$(ls -lh $(PLATFORM_BIN) | awk '{print $$5}'))"

cmake-build: CMakeLists.txt way-embed.cpp | llama.cpp/CMakeLists.txt
	@command -v cmake >/dev/null 2>&1 || { \
		echo "ERROR: cmake not found — required to build way-embed (the semantic matcher) from source."; \
		echo "  Install the build toolchain from the agent-ways app dir, then retry:"; \
		echo "    make deps    # auto-detects your package manager (pacman/apt/dnf/zypper/brew)"; \
		echo "  Or install manually: cmake + a C++ compiler"; \
		echo "    Arch: pacman -S cmake gcc   Debian: apt install cmake g++   Fedora: dnf install cmake gcc-c++   macOS: brew install cmake"; \
		exit 1; }
	@mkdir -p $(BUILDDIR)
	@cd $(BUILDDIR) && cmake $(CMAKE_FLAGS) .. 2>&1 | tail -5
	cmake --build $(BUILDDIR) --target way-embed $(CMAKE_BUILD_FLAGS) -j$$(nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 4)

# Ensure submodule is initialized
llama.cpp/CMakeLists.txt:
	@echo "Initializing llama.cpp submodule..."
	git -C ../.. submodule update --init --depth 1 tools/way-embed/llama.cpp

# Build from source and compare against checked-in binary
verify: cmake-build
	@if cmp -s $(PLATFORM_BIN) $(BUILDDIR)/way-embed; then \
		echo "PASS: binary matches source ($(PLATFORM))"; \
	else \
		echo "MISMATCH: checked-in binary differs from source build"; \
		exit 1; \
	fi

# Model download (XDG cache location). Prefer the 1.0 cache name; reuse the
# legacy claude-ways dir if it already holds a model (un-migrated install) so we
# don't re-download. Matches paths::cache_root() in the binary.
MODEL_DIR  = $(shell C="$${XDG_CACHE_HOME:-$$HOME/.cache}"; if [ -d "$$C/agent-ways/user" ]; then echo "$$C/agent-ways/user"; elif [ -d "$$C/claude-ways/user" ]; then echo "$$C/claude-ways/user"; else echo "$$C/agent-ways/user"; fi)
MODEL_EN   = $(MODEL_DIR)/minilm-l6-v2.gguf
MODEL_MULTI = $(MODEL_DIR)/multilingual-minilm-l12-v2-q8.gguf
SRCDIR     = $(CURDIR)/

model:
	@[ -f "$(MODEL_EN)" ] || bash "$(SRCDIR)download-model.sh" "$(MODEL_DIR)"

model-multilingual:
	@[ -f "$(MODEL_MULTI)" ] || bash "$(SRCDIR)download-model.sh" --multilingual "$(MODEL_DIR)"

# Download directly from HuggingFace (for provenance verification)
model-upstream:
	bash $(SRCDIR)download-model.sh --upstream "$(MODEL_DIR)"

# Full setup: download (or build) binary + English model, regenerate corpus.
# Tries pre-built binary from GitHub Releases first, builds from source if unavailable.
# The multilingual model (127MB) is on-demand per ADR-139 — fetched via
# `make model-multilingual` by the ways-localize skill, not at default setup.
setup: setup-binary
	@[ -f "$(MODEL_EN)" ] || bash "$(SRCDIR)download-model.sh" "$(MODEL_DIR)"
	@echo "Regenerating corpus with embeddings..."
	../../bin/ways corpus --quiet
	@echo ""
	@echo "Setup complete. Running verification..."
	bash "$(SRCDIR)test-embedding.sh"
	@echo ""
	@echo "To localize ways into another language: invoke the ways-localize skill"

# Try download, fall back to build
setup-binary:
	@if bash "$(SRCDIR)download-binary.sh" "$(MODEL_DIR)"; then \
		echo "Pre-built binary installed."; \
	else \
		echo "No pre-built binary available, building from source..."; \
		$(MAKE) $(BIN); \
	fi

# Force re-download (or rebuild) of the platform binary. Removes the
# cached install so download-binary.sh's "already installed" early-
# return does not skip a refresh after an upstream update.
rebuild-binary:
	@rm -f "$(MODEL_DIR)/way-embed"
	@$(MAKE) setup-binary

# Run basic smoke test
test: $(BIN)
	@echo "Testing way-embed ($(PLATFORM))..."
	$(BIN) --version
	@echo "PASS: version check"

# Publish pre-built binaries + model to GitHub Releases
# Run on each platform, then create release with all artifacts.
# Usage: make release VERSION=v0.1.0
VERSION ?= v0.1.0
RELEASE_TAG = way-embed-$(VERSION)
GH_REPO = $(shell git -C ../.. remote get-url origin | sed 's|.*github.com[:/]||;s|\.git$$||')

# Build and tag the platform binary for release
release-build: $(PLATFORM_BIN)
	@echo "Built $(PLATFORM_BIN) for release"
	@ls -lh $(PLATFORM_BIN)

# Expected platform binaries for release
RELEASE_PLATFORMS = linux-x86_64 linux-aarch64 darwin-x86_64 darwin-arm64
RELEASE_BINS = $(addprefix ../../bin/way-embed-,$(RELEASE_PLATFORMS))

# Create the release (run after CI builds all platforms)
release:
	@echo "Publishing $(RELEASE_TAG) to $(GH_REPO)..."
	@echo "Expected binaries:"; for b in $(RELEASE_BINS); do \
		[ -f "$$b" ] && ls -lh "$$b" || echo "  MISSING: $$b"; \
	done
	gh release create $(RELEASE_TAG) \
		--repo $(GH_REPO) \
		--title "way-embed $(VERSION)" \
		--notes-file $(SRCDIR)RELEASE.md \
		$(wildcard $(RELEASE_BINS)) $(MODEL)
	@echo "Published: https://github.com/$(GH_REPO)/releases/tag/$(RELEASE_TAG)"

clean:
	rm -rf $(BUILDDIR)

.PHONY: cmake-build verify test model model-multilingual model-upstream setup setup-binary release-build release clean
