# agent-ways
# Top-level Makefile — build, install, and release.
#
# Quick start:   make setup
# Full install:  make install
# Update:        make update

.DEFAULT_GOAL := help
.PHONY: setup install uninstall update update-binaries sync-to-home sync-to-home-link sync-to-home-test clean help deps ways ways-rebuild attend attend-rebuild attend-chat attend-chat-rebuild hooks-install way-embed-rebuild lint test test-unit test-sim test-lang test-locales test-multilingual release purge-attend-state

ifeq ($(OS),Windows_NT)
    SHELL := C:/Program Files/Git/usr/bin/bash.exe
    .SHELLFLAGS := -c
    LINK := cp -f
    EXE := .exe
    # Copy hooks/ways contents into ~/.claude/hooks/ways/ (no symlinks without Developer Mode)
    INSTALL_HOOKS = mkdir -p "$(HOME)/.claude/hooks/ways" && cp -r "$(CURDIR)/hooks/ways/." "$(HOME)/.claude/hooks/ways/"
else
    SHELL := bash
    LINK := ln -sf
    EXE :=
    # Symlink hooks/ways into ~/.claude/hooks/ways
    INSTALL_HOOKS = mkdir -p "$(HOME)/.claude/hooks" && ln -sf "$(CURDIR)/hooks/ways" "$(HOME)/.claude/hooks/ways"
endif

WAYS_BIN = bin/ways
ATTEND_BIN = bin/attend
ATTEND_CHAT_BIN = bin/attend-chat
WAY_EMBED_BIN = bin/way-embed
XDG_BIN = $(or $(XDG_BIN_HOME),$(HOME)/.local/bin)
CLAUDE_BIN = $(HOME)/.claude/bin

# --- Primary targets ---

help:
	@echo "agent-ways"
	@echo ""
	@echo "  make setup        Build ways CLI + attend + fetch embedding model + corpus"
	@echo "  make install      Full first-time setup (hooks + tools + PATH)"
	@echo "  make deps         Install the C++ build toolchain (only if a prebuilt binary"
	@echo "                    won't run on your platform and you must build from source)"
	@echo "  make update       Pull + rebuild + relink in the app dir; then 'ways reconcile' to reproject"
	@echo "  make sync-to-home [legacy] ADR-140 subdirectory projection — superseded by 'ways reconcile'"
	@echo "  make sync-to-home-link  [legacy] symlink variant of sync-to-home"
	@echo "  make ways         Get ways binary (download or build from source)"
	@echo "  make ways-rebuild Force rebuild ways from source"
	@echo "  make attend       Build attend binary"
	@echo "  make attend-rebuild Force rebuild attend from source"
	@echo "  make lint         Run clippy on Rust workspace (warnings = errors)"
	@echo "  make test         Run all tests (lint + smoke + unit + sim + lang)"
	@echo "  make test-unit    Run Rust unit tests"
	@echo "  make test-sim     Run session simulator (8 scenarios)"
	@echo "  make test-lang    Validate active language coverage"
	@echo "  make test-locales Check locale files for gaps and duplicates"
	@echo "  make test-multilingual  Verify multilingual way matching (18 languages)"
	@echo "  make docs         Regenerate docs/cli/attend.md from the clap definition"
	@echo "  make release      Build release binary for current platform"
	@echo "  make cut-release  Bump+tag a component release (COMPONENT=ways LEVEL=patch [PUSH=1])"
	@echo "  make uninstall    Remove ways from PATH"
	@echo "  make clean        Remove build artifacts"
	@echo "  make purge-attend-state  Wipe all attend runtime cache (peers, signals,"
	@echo "                           channels, instance names, heartbeats, sensor"
	@echo "                           checkpoints). Manual recovery only — never"
	@echo "                           invoked by setup/install/update."
	@echo ""

# Install the C++ build toolchain (cmake + compiler + git) needed to build
# way-embed from source, for platforms with no pre-built binary (or where the
# prebuilt won't launch). This is the ONLY target that installs system packages;
# it uses sudo where the platform requires it and lets the package manager prompt
# for confirmation. `make setup` never calls this — the user runs it explicitly.
deps:
	@echo "Installing build toolchain (cmake, C++ compiler, git)."
	@echo "This installs system packages — you'll be prompted to confirm."
	@echo ""
	@if command -v pacman >/dev/null 2>&1; then \
		sudo pacman -S --needed cmake gcc git; \
	elif command -v apt-get >/dev/null 2>&1; then \
		sudo apt-get update && sudo apt-get install cmake g++ git; \
	elif command -v dnf >/dev/null 2>&1; then \
		sudo dnf install cmake gcc-c++ git; \
	elif command -v zypper >/dev/null 2>&1; then \
		sudo zypper install cmake gcc-c++ git; \
	elif command -v brew >/dev/null 2>&1; then \
		brew install cmake git; \
	else \
		echo "No supported package manager found (pacman/apt/dnf/zypper/brew)."; \
		echo "Install manually: cmake, a C++ compiler (g++ or clang++), and git."; \
		exit 1; \
	fi
	@echo ""
	@echo "Toolchain ready. Now run: make setup"

# Build ways CLI + set up embedding engine + generate initial corpus.
setup: ways attend attend-chat
	@echo "Setting up embedding engine..."
	@# Optional accelerator — if its build deps are missing, warn and continue so
	@# the rest of install (incl. the PATH symlinks) still completes. Without it,
	@# ways fall back to regex/keyword matching.
	@$(MAKE) -C tools/way-embed setup || { \
		echo ""; \
		echo "  ⚠ Embedding engine not built — semantic (meaning-based) matching is unavailable."; \
		echo "    Regex/keyword ways still work. To enable it, install the build toolchain"; \
		echo "    with 'make deps', then re-run 'make setup' (or use a platform with a prebuilt binary)."; \
	}
	@echo ""
	@echo "Setting up mmaid diagram renderer..."
	@bash tools/mmaid/download-mmaid.sh || echo "  (mmaid optional — skipping)"
	@echo ""
	@echo "Generating corpus..."
	@$(WAYS_BIN) corpus --quiet

# Full install: build, setup, symlink to PATH.
install: hooks-executable setup hooks-install
	@mkdir -p "$(XDG_BIN)"
	@$(LINK) "$(CURDIR)/$(WAYS_BIN)" "$(XDG_BIN)/ways"
	@$(LINK) "$(CURDIR)/$(ATTEND_BIN)" "$(XDG_BIN)/attend"
	@$(LINK) "$(CURDIR)/$(ATTEND_CHAT_BIN)" "$(XDG_BIN)/attend-chat"
	@mkdir -p "$(CLAUDE_BIN)"
	@# When the repo IS ~/.claude, $(CURDIR)/bin/way-embed already equals
	@# $(CLAUDE_BIN)/way-embed — linking a file to itself errors ("same file").
	@# Skip the link in that case; the binary is already where tools look for it.
	@if [ "$(CURDIR)/$(WAY_EMBED_BIN)" -ef "$(CLAUDE_BIN)/way-embed" ]; then \
		echo "  way-embed: already in place ($(CLAUDE_BIN)/way-embed) — no link needed"; \
	else \
		$(LINK) "$(CURDIR)/$(WAY_EMBED_BIN)" "$(CLAUDE_BIN)/way-embed"; \
	fi
	@echo ""
	@echo "Install complete."
	@echo "  ways binary:        $(XDG_BIN)/ways → $(CURDIR)/$(WAYS_BIN)"
	@echo "  attend binary:      $(XDG_BIN)/attend → $(CURDIR)/$(ATTEND_BIN)"
	@echo "  attend-chat binary: $(XDG_BIN)/attend-chat → $(CURDIR)/$(ATTEND_CHAT_BIN)"
	@echo "  way-embed binary:   $(CLAUDE_BIN)/way-embed → $(CURDIR)/$(WAY_EMBED_BIN)"
	@case ":$$PATH:" in \
		*":$(XDG_BIN):"*) ;; \
		*) printf '\n  %s\n  %s\n  %s\n' \
			"NOTE: $(XDG_BIN) is not on your PATH — ways/attend won't be found yet." \
			"Add it to your shell rc (~/.bashrc or ~/.zshrc), then restart your shell:" \
			"    export PATH=\"$(XDG_BIN):\$$PATH\"" ;; \
	esac
	@echo "  Restart Claude Code for ways to take effect."

hooks-install:
	@$(INSTALL_HOOKS)
	@echo "Hooks installed at $(HOME)/.claude/hooks/ways"

# Remove symlink from PATH.
uninstall:
	@rm -f "$(XDG_BIN)/ways" "$(XDG_BIN)/attend" "$(XDG_BIN)/attend-chat"
	@echo "Removed $(XDG_BIN)/ways $(XDG_BIN)/attend $(XDG_BIN)/attend-chat"

# Pull upstream and re-setup. scripts/update.sh wraps the pull so machine-local
# changes (settings.json, stale build artifacts) and merged branches don't abort
# the fast-forward — it autostashes, prunes, and surfaces conflicts instead of
# clobbering. See the script for recovery behavior.
update:
	@bash scripts/update.sh
	$(MAKE) update-binaries
	$(MAKE) install

# Subdirectory topology (ADR-140): the repo lives in a subdir of ~/.claude and is
# projected up into it. Copy is the default (robust everywhere); the -link variant
# symlinks instead (git pull then becomes the whole update — no re-sync). Both
# delegate to the same deterministic script and run a smoke test of the merge.
sync-to-home:
	@bash scripts/sync-to-home.sh

sync-to-home-link:
	@SYNC_MODE=symlink bash scripts/sync-to-home.sh

sync-to-home-test:
	@bash scripts/sync-to-home-test.sh

# Rebuild every binary `update` is responsible for refreshing.
# Indirected from `update:` so adding a new rebuild here takes effect
# on the same `make update` run that pulls the change — sub-makes
# re-read the Makefile, but the in-memory `update:` recipe is fixed at
# make-process startup.
update-binaries: ways-rebuild attend-rebuild attend-chat-rebuild way-embed-rebuild

# --- Build ---

# Get the ways binary: try existing → download → build from source.
ways:
	@if [ -x $(WAYS_BIN) ] && $(WAYS_BIN) --version >/dev/null 2>&1; then \
		echo "ways already installed: $$($(WAYS_BIN) --version)"; \
	elif bash tools/ways-cli/download-ways.sh 2>/dev/null; then \
		echo "Pre-built binary installed."; \
	elif command -v cargo >/dev/null 2>&1; then \
		echo "No pre-built binary, building from source..."; \
		bash scripts/check-rust.sh || exit 1; \
		cargo build --release --manifest-path tools/Cargo.toml -p ways; \
		mkdir -p bin; \
		$(LINK) "$(CURDIR)/tools/target/release/ways$(EXE)" $(WAYS_BIN); \
		echo "Built: $(WAYS_BIN) ($$(ls -lh $(WAYS_BIN) | awk '{print $$5}'))"; \
	else \
		echo "error: No pre-built binary and cargo not found."; \
		echo "Install Rust: https://rustup.rs/"; \
		exit 1; \
	fi

# Force rebuild from source (ignores existing binary and download).
ways-rebuild:
	@if ! command -v cargo >/dev/null 2>&1; then \
		echo "error: cargo not found. Install Rust: https://rustup.rs/"; \
		exit 1; \
	fi
	@bash scripts/check-rust.sh
	cargo build --release --manifest-path tools/Cargo.toml -p ways
	@mkdir -p bin
	@$(LINK) "$(CURDIR)/tools/target/release/ways$(EXE)" $(WAYS_BIN)
	@echo "Built: $(WAYS_BIN) ($$(ls -lh $(WAYS_BIN) | awk '{print $$5}'))"

# Build attend binary from workspace.
attend:
	@if [ -x $(ATTEND_BIN) ] && $(ATTEND_BIN) --help >/dev/null 2>&1; then \
		echo "attend already built."; \
	elif command -v cargo >/dev/null 2>&1; then \
		echo "Building attend..."; \
		cargo build --release --manifest-path tools/Cargo.toml -p attend; \
		mkdir -p bin; \
		$(LINK) "$(CURDIR)/tools/target/release/attend$(EXE)" $(ATTEND_BIN); \
		echo "Built: $(ATTEND_BIN) ($$(ls -lh $(ATTEND_BIN) | awk '{print $$5}'))"; \
		$(MAKE) -s --no-print-directory _attend_state_hint; \
	else \
		echo "error: cargo not found. Install Rust: https://rustup.rs/"; \
		exit 1; \
	fi

# Generate the attend CLI markdown reference from the same clap-derive
# `Cli` definition that drives runtime --help (ADR-111 extension). Output
# lives in docs/cli/ alongside other end-user reference material.
docs: attend
	@mkdir -p docs/cli
	@cargo build --release --manifest-path tools/Cargo.toml -p attend --bin gen-docs --quiet
	@./tools/target/release/gen-docs > docs/cli/attend.md
	@echo "Wrote docs/cli/attend.md ($$(wc -l < docs/cli/attend.md) lines)"

# Force rebuild attend from source.
attend-rebuild:
	@if ! command -v cargo >/dev/null 2>&1; then \
		echo "error: cargo not found. Install Rust: https://rustup.rs/"; \
		exit 1; \
	fi
	cargo build --release --manifest-path tools/Cargo.toml -p attend
	@mkdir -p bin
	@$(LINK) "$(CURDIR)/tools/target/release/attend$(EXE)" $(ATTEND_BIN)
	@echo "Built: $(ATTEND_BIN) ($$(ls -lh $(ATTEND_BIN) | awk '{print $$5}'))"
	@$(MAKE) -s --no-print-directory _attend_state_hint

# Build attend-chat binary from workspace.
attend-chat:
	@if [ -x $(ATTEND_CHAT_BIN) ] && $(ATTEND_CHAT_BIN) --help >/dev/null 2>&1; then \
		echo "attend-chat already built."; \
	elif command -v cargo >/dev/null 2>&1; then \
		echo "Building attend-chat..."; \
		cargo build --release --manifest-path tools/Cargo.toml -p attend-chat; \
		mkdir -p bin; \
		$(LINK) "$(CURDIR)/tools/target/release/attend-chat$(EXE)" $(ATTEND_CHAT_BIN); \
		echo "Built: $(ATTEND_CHAT_BIN) ($$(ls -lh $(ATTEND_CHAT_BIN) | awk '{print $$5}'))"; \
		$(MAKE) -s --no-print-directory _attend_state_hint; \
	else \
		echo "error: cargo not found. Install Rust: https://rustup.rs/"; \
		exit 1; \
	fi

# Force rebuild attend-chat from source.
attend-chat-rebuild:
	@if ! command -v cargo >/dev/null 2>&1; then \
		echo "error: cargo not found. Install Rust: https://rustup.rs/"; \
		exit 1; \
	fi
	cargo build --release --manifest-path tools/Cargo.toml -p attend-chat
	@mkdir -p bin
	@$(LINK) "$(CURDIR)/tools/target/release/attend-chat$(EXE)" $(ATTEND_CHAT_BIN)
	@echo "Built: $(ATTEND_CHAT_BIN) ($$(ls -lh $(ATTEND_CHAT_BIN) | awk '{print $$5}'))"
	@$(MAKE) -s --no-print-directory _attend_state_hint

# Internal: post-build advisory printed after every attend / attend-
# chat (re)build. Suggests `make purge-attend-state` for operators
# updating from older attends whose on-disk state schema may have
# drifted (signals format, instance registry, heartbeat layout).
# Phony so it always runs; not a dependency of any user target.
.PHONY: _attend_state_hint
_attend_state_hint:
	@echo ""
	@echo "  Note: if you are updating from an older attend, consider"
	@echo "        \`make purge-attend-state\` to reset cached runtime"
	@echo "        state for consistency. Skip it on a fresh install."

# Force re-fetch (or rebuild) of the way-embed binary. Delegates to the
# way-embed sub-Makefile's rebuild-binary target, which clears the
# cached install before download-binary.sh would short-circuit.
way-embed-rebuild:
	$(MAKE) -C tools/way-embed rebuild-binary

# --- Test ---

test: lint test-smoke test-unit test-sim
	@echo "All tests passed."

lint:
	@echo "Linting Rust workspace..."
	@cargo clippy --manifest-path tools/Cargo.toml -- -D warnings
	@echo "Lint passed."

test-smoke: ways
	@echo "Smoke testing ways binary..."
	@$(WAYS_BIN) --version
	@$(WAYS_BIN) lint --check --global && echo "  lint: PASS"
	@$(WAYS_BIN) match "write a unit test" >/dev/null && echo "  match: PASS"
	@$(WAYS_BIN) graph --output /dev/null && echo "  graph: PASS"
	@echo "Smoke tests passed."

test-unit:
	@echo "Running Rust unit tests..."
	@cargo test --manifest-path tools/ways-cli/Cargo.toml --bin ways --quiet
	@echo "Unit tests passed."

test-sim: ways
	@echo "Running session simulator (8 scenarios)..."
	@cargo test --manifest-path tools/ways-cli/Cargo.toml --test session_sim -- --test-threads=1
	@echo "Simulation tests passed."

test-lang: ways
	@echo "Validating active language coverage..."
	@$(WAYS_BIN) language --json | python3 -c "\
	import json,sys; d=json.load(sys.stdin); \
	active=d['locales_found']; \
	print(f'  Active locales in corpus: {len(active)}'); \
	print('  Language coverage: SKIP (no locale data -- dormant per ADR-139)' if len(active)==0 else '  Language coverage: PASS')"

test-locales:
	@echo "Checking locale files for gaps and duplicates..."
	@python3 scripts/test-locales.py

test-multilingual: ways
	@bash tests/test-multilingual.sh

# --- Release ---

# Build release binary for current platform with checksum.
# To publish: git tag ways-vX.Y.Z && git push --tags
# CI builds all 4 platforms and creates a GitHub Release.
release: ways-rebuild
	@mkdir -p dist
	@PLATFORM=$$(uname -s | tr '[:upper:]' '[:lower:]')-$$(uname -m | sed 's/arm64/aarch64/'); \
		cp $(WAYS_BIN) dist/ways-$$PLATFORM; \
		cd dist && sha256sum ways-$$PLATFORM > ways-$$PLATFORM.sha256; \
		echo "dist/ways-$$PLATFORM ($$(ls -lh ways-$$PLATFORM | awk '{print $$5}'))"; \
		cat ways-$$PLATFORM.sha256

# Cut a release for a Cargo-versioned component (ADR-150): bump version + tag
# locally. Publishing is a separate, explicit push (or PUSH=1). Examples:
#   make cut-release COMPONENT=ways LEVEL=patch          # local: bump 1.0.0 -> 1.0.1, tag
#   make cut-release COMPONENT=ways LEVEL=patch PUSH=1   # also push main + tag (publishes)
cut-release:
	@bash scripts/release.sh "$(COMPONENT)" "$(LEVEL)" $(if $(PUSH),--push,)

# --- Supporting ---

hooks-executable:
	@find hooks -name '*.sh' -exec chmod +x {} + 2>/dev/null || true
	@echo "Hooks marked executable."

clean:
	$(MAKE) -C tools/way-embed clean
	cargo clean --manifest-path tools/ways-cli/Cargo.toml 2>/dev/null || true
	cargo clean --manifest-path tools/Cargo.toml 2>/dev/null || true
	rm -rf dist/

# Wipe all attend / attend-chat runtime cache state under
# ~/.cache/attend/. Recovery target only — NEVER a dependency of
# setup, install, update, update-binaries, or any rebuild target.
# An advisory hint is printed at the end of attend / attend-chat
# build targets pointing operators here when they update.
#
# Removes: signals (peer messages), _groups.yaml (channel
# membership), state/ (sensor checkpoints), instances/ (per-cwd
# session naming, ADR-129), heartbeat/ (liveness sidecars,
# ADR-129), last_inbound (reply targeting).
#
# Does NOT touch ~/.claude/sessions/*.json or ~/.claude/projects/ —
# those are Claude Code's own session state, owned outside attend.
purge-attend-state:
	@echo "Wiping ~/.cache/attend/ (peers, signals, channels, instances, heartbeats, sensor state)"
	@if pgrep -f 'attend run' >/dev/null 2>&1; then \
		echo ""; \
		echo "WARNING: at least one 'attend run' is currently running."; \
		echo "         Purging cached state under a live attend leaves it"; \
		echo "         operating on stale in-memory views. Stop your"; \
		echo "         attend processes first, then re-run this target."; \
		echo ""; \
		echo "         Aborting."; \
		exit 1; \
	fi
	@rm -rf "$(HOME)/.cache/attend"
	@echo "Done. Next attend launch starts from a clean slate."
