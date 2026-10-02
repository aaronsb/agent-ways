# agent-ways
# Top-level Makefile — build and test.
#
# Quick start:   make setup
# Install/update/uninstall/release live in the binary and scripts:
#   ways update | ways reconcile | ways uninstall | make cut-release

.DEFAULT_GOAL := help
.PHONY: setup link relink install update sync-to-home update-binaries clean help deps ways ways-rebuild ways-audit ways-audit-rebuild ways-mcp ways-mcp-rebuild ways-agent ways-agent-rebuild attend attend-rebuild attend-chat attend-chat-rebuild way-embed-rebuild lint test test-unit test-sim test-adr test-statusline test-hooks test-lang test-locales test-multilingual test-live purge-attend-state

ifeq ($(OS),Windows_NT)
    SHELL := C:/Program Files/Git/usr/bin/bash.exe
    .SHELLFLAGS := -c
    LINK := cp -f
    EXE := .exe
else
    SHELL := bash
    LINK := ln -sf
    EXE :=
endif

WAYS_BIN = bin/ways
WAYS_AUDIT_BIN = bin/ways-audit
WAYS_MCP_BIN = bin/ways-mcp
WAYS_AGENT_BIN = bin/ways-agent
ATTEND_BIN = bin/attend
ATTEND_CHAT_BIN = bin/attend-chat
WAY_EMBED_BIN = bin/way-embed
# The suite binaries `link` puts on PATH and `relink` installs when missing.
SUITE_BINS = ways ways-audit ways-mcp ways-agent attend attend-chat
XDG_BIN = $(or $(XDG_BIN_HOME),$(HOME)/.local/bin)
CLAUDE_BIN = $(HOME)/.claude/bin

# --- Primary targets ---

help:
	@echo "agent-ways"
	@echo ""
	@echo "  make setup        Build ways CLI + attend + fetch embedding model + corpus"
	@echo "                    (install, update and removal: ways update | reconcile | uninstall)"
	@echo "  make deps         Install the C++ build toolchain (only if a prebuilt binary"
	@echo "                    won't run on your platform and you must build from source)"
	@echo "  make ways         Get ways binary (download or build from source)"
	@echo "  make ways-rebuild Force rebuild ways from source"
	@echo "  make ways-audit   Get ways-audit compliance binary (download or build)"
	@echo "  make attend       Build attend binary"
	@echo "  make attend-rebuild Force rebuild attend from source"
	@echo "  make lint         Run clippy on Rust workspace (warnings = errors)"
	@echo "  make test         Run all tests (lint + smoke + unit + sim + adr + statusline + hooks)"
	@echo "  make test-unit    Run Rust unit tests"
	@echo "  make test-sim     Run session simulator (8 scenarios)"
	@echo "  make test-adr     Run adr tool tests (lint, archive, golden, import, macro)"
	@echo "  make test-statusline  Test statusline.sh segments against stub attend builds"
	@echo "  make test-hooks   Test hook scripts against a temp HOME and sessions root"
	@echo "  make test-lang    Validate active language coverage"
	@echo "  make test-locales Check locale files for gaps and duplicates"
	@echo "  make test-multilingual  Verify multilingual way matching (18 languages)"
	@echo "  make test-live TIER=1  Live install fixture in Docker (ADR-186; FLAVOR=branch|release)"
	@echo "  make docs         Regenerate docs/cli/attend.md from the clap definition"
	@echo "  make cut-release  Open a version-bump PR for a component (COMPONENT=ways LEVEL=patch)"
	@echo "  make publish-release  After the bump PR merges: tag + publish (COMPONENT=ways [PUSH=1])"
	@echo "  make clean        Remove build artifacts"
	@echo "  make purge-attend-state  Wipe all attend runtime cache (peers, signals,"
	@echo "                           channels, instance names, heartbeats, sensor"
	@echo "                           checkpoints). Manual recovery only — never"
	@echo "                           invoked by setup or update."
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
setup: ways ways-audit attend attend-chat
	@# The MCP server (ADR-501) is optional until a module depends on it: a
	@# missing prebuilt with no cargo warns instead of failing the install.
	@$(MAKE) -s --no-print-directory ways-mcp || echo "  ⚠ ways-mcp not installed; the agent-ways MCP server stays unregistered."
	@$(MAKE) -s --no-print-directory ways-agent || echo "  ⚠ ways-agent not installed; the relevance gate stays off."
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
	@# A failed embedding pass keeps the install going like a missing engine
	@# does above: ways fall back to keyword matching until `ways corpus` works.
	@# --ways-dir: the shipped ways, read from the app; the ~/.claude projection
	@# does not exist yet on a fresh install.
	@$(WAYS_BIN) corpus --quiet --ways-dir "$(CURDIR)/hooks/ways" || echo "  ⚠ Corpus built without embeddings (see above); keyword ways still work."

# Idempotent linking of the suite binaries onto PATH. Only links what exists in
# bin/, so it is safe to run before every binary is built and safe to re-run.
# The suite binaries (ways, ways-audit, ways-mcp, ways-agent, attend, attend-chat) link into
# $(XDG_BIN); way-embed lives in $(CLAUDE_BIN).
link:
	@mkdir -p "$(XDG_BIN)"
	@for b in $(SUITE_BINS); do \
		if [ -e "$(CURDIR)/bin/$$b" ]; then \
			$(LINK) "$(CURDIR)/bin/$$b" "$(XDG_BIN)/$$b"; \
		fi; \
	done
	@if [ -e "$(CURDIR)/$(WAY_EMBED_BIN)" ]; then \
		mkdir -p "$(CLAUDE_BIN)"; \
		if [ "$(CURDIR)/$(WAY_EMBED_BIN)" -ef "$(CLAUDE_BIN)/way-embed" ]; then :; \
		else $(LINK) "$(CURDIR)/$(WAY_EMBED_BIN)" "$(CLAUDE_BIN)/way-embed"; fi; \
	fi

# Install any suite binary the install lacks, then link. `ways update` runs this
# after every pull, from the freshly pulled Makefile, so a component added to the
# suite reaches an existing install even when the updater predates it: the 1.23.1
# updater refreshes only the components it knew of, and ways-mcp arrives here.
# Each component target is download-first, then cargo; a failure warns and the
# rest of the suite still links.
relink:
	@for b in $(SUITE_BINS); do \
		[ -e "$(CURDIR)/bin/$$b" ] || $(MAKE) -s --no-print-directory $$b \
			|| echo "  ⚠ $$b not installed; run 'make $$b' in $(CURDIR) to retry."; \
	done
	@$(MAKE) -s --no-print-directory link

# transition: removed by #717 (ADR-506)
# Retired targets, kept so an in-flight pre-#715 `make update` finishes: make has
# already parsed the old Makefile, so its recipe still runs `$(MAKE) install`
# against this file after `scripts/update.sh` pulls it.
install: link
	@echo "note: 'make install' is retired; use 'ways update'."

# transition: removed by #717 (ADR-506)
update:
	@echo "note: 'make update' is retired; running 'ways update'."
	@./bin/ways update

# transition: removed by #717 (ADR-506)
sync-to-home:
	@echo "'make sync-to-home' was removed; see docs/migration-1.0.md." >&2
	@exit 1

# Force-rebuild every binary `ways update` is responsible for refreshing.
update-binaries: ways-rebuild ways-audit-rebuild ways-mcp-rebuild ways-agent-rebuild attend-rebuild attend-chat-rebuild way-embed-rebuild

# --- Build ---

# Get the ways binary: try existing → download → build from source.
ways:
	@if [ -x $(WAYS_BIN) ] && $(WAYS_BIN) --version >/dev/null 2>&1; then \
		echo "ways already installed: $$($(WAYS_BIN) --version)"; \
	elif bash tools/ways-cli/download-ways.sh; then \
		echo "Pre-built binary installed."; \
	elif command -v cargo >/dev/null 2>&1; then \
		echo "Pre-built unavailable (see above) — building from source..."; \
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

# Get the ways-audit compliance binary: try existing → download → build. A
# first-class member of the suite (installed and updated like the others); it is
# *deliberately-invoked* in the sense that you run `ways-audit` when you want it,
# not that it's optional to install.
ways-audit:
	@if [ -x $(WAYS_AUDIT_BIN) ] && $(WAYS_AUDIT_BIN) --version >/dev/null 2>&1; then \
		echo "ways-audit already installed: $$($(WAYS_AUDIT_BIN) --version)"; \
	elif bash tools/ways-audit/download-ways-audit.sh; then \
		echo "Pre-built ways-audit binary installed."; \
	elif command -v cargo >/dev/null 2>&1; then \
		echo "Pre-built unavailable (see above) — building ways-audit from source..."; \
		bash scripts/check-rust.sh || exit 1; \
		cargo build --release --manifest-path tools/Cargo.toml -p ways-audit; \
		mkdir -p bin; \
		$(LINK) "$(CURDIR)/tools/target/release/ways-audit$(EXE)" $(WAYS_AUDIT_BIN); \
		echo "Built: $(WAYS_AUDIT_BIN) ($$(ls -lh $(WAYS_AUDIT_BIN) | awk '{print $$5}'))"; \
	else \
		echo "error: No pre-built binary and cargo not found."; \
		echo "Install Rust: https://rustup.rs/"; \
		exit 1; \
	fi

# Force rebuild ways-audit from source (ignores existing binary and download).
ways-audit-rebuild:
	@if ! command -v cargo >/dev/null 2>&1; then \
		echo "error: cargo not found. Install Rust: https://rustup.rs/"; \
		exit 1; \
	fi
	@bash scripts/check-rust.sh
	cargo build --release --manifest-path tools/Cargo.toml -p ways-audit
	@mkdir -p bin
	@$(LINK) "$(CURDIR)/tools/target/release/ways-audit$(EXE)" $(WAYS_AUDIT_BIN)
	@echo "Built: $(WAYS_AUDIT_BIN) ($$(ls -lh $(WAYS_AUDIT_BIN) | awk '{print $$5}'))"

ways-mcp:
	@if [ -x $(WAYS_MCP_BIN) ] && $(WAYS_MCP_BIN) --version >/dev/null 2>&1; then \
		echo "ways-mcp already installed: $$($(WAYS_MCP_BIN) --version)"; \
	elif bash tools/ways-mcp/download-ways-mcp.sh; then \
		echo "Pre-built ways-mcp binary installed."; \
	elif command -v cargo >/dev/null 2>&1; then \
		echo "Pre-built unavailable (see above) — building ways-mcp from source..."; \
		bash scripts/check-rust.sh || exit 1; \
		cargo build --release --manifest-path tools/Cargo.toml -p ways-mcp; \
		mkdir -p bin; \
		$(LINK) "$(CURDIR)/tools/target/release/ways-mcp$(EXE)" $(WAYS_MCP_BIN); \
		echo "Built: $(WAYS_MCP_BIN) ($$(ls -lh $(WAYS_MCP_BIN) | awk '{print $$5}'))"; \
	else \
		echo "error: No pre-built binary and cargo not found."; \
		echo "Install Rust: https://rustup.rs/"; \
		exit 1; \
	fi

# Force rebuild ways-mcp from source (ignores existing binary and download).
ways-mcp-rebuild:
	@if ! command -v cargo >/dev/null 2>&1; then \
		echo "error: cargo not found. Install Rust: https://rustup.rs/"; \
		exit 1; \
	fi
	@bash scripts/check-rust.sh
	cargo build --release --manifest-path tools/Cargo.toml -p ways-mcp
	@mkdir -p bin
	@$(LINK) "$(CURDIR)/tools/target/release/ways-mcp$(EXE)" $(WAYS_MCP_BIN)
	@echo "Built: $(WAYS_MCP_BIN) ($$(ls -lh $(WAYS_MCP_BIN) | awk '{print $$5}'))"

ways-agent:
	@if [ -x $(WAYS_AGENT_BIN) ] && $(WAYS_AGENT_BIN) --version >/dev/null 2>&1; then \
		echo "ways-agent already installed: $$($(WAYS_AGENT_BIN) --version)"; \
	elif bash tools/ways-agent/download-ways-agent.sh; then \
		echo "Pre-built ways-agent binary installed."; \
	elif command -v cargo >/dev/null 2>&1; then \
		echo "Pre-built unavailable (see above) — building ways-agent from source..."; \
		bash scripts/check-rust.sh || exit 1; \
		cargo build --release --manifest-path tools/Cargo.toml -p ways-agent; \
		mkdir -p bin; \
		$(LINK) "$(CURDIR)/tools/target/release/ways-agent$(EXE)" $(WAYS_AGENT_BIN); \
		echo "Built: $(WAYS_AGENT_BIN) ($$(ls -lh $(WAYS_AGENT_BIN) | awk '{print $$5}'))"; \
	else \
		echo "error: No pre-built binary and cargo not found."; \
		echo "Install Rust: https://rustup.rs/"; \
		exit 1; \
	fi

# Force rebuild ways-agent from source (ignores existing binary and download).
ways-agent-rebuild:
	@if ! command -v cargo >/dev/null 2>&1; then \
		echo "error: cargo not found. Install Rust: https://rustup.rs/"; \
		exit 1; \
	fi
	@bash scripts/check-rust.sh
	cargo build --release --manifest-path tools/Cargo.toml -p ways-agent
	@mkdir -p bin
	@$(LINK) "$(CURDIR)/tools/target/release/ways-agent$(EXE)" $(WAYS_AGENT_BIN)
	@echo "Built: $(WAYS_AGENT_BIN) ($$(ls -lh $(WAYS_AGENT_BIN) | awk '{print $$5}'))"

# Build attend binary from workspace.
attend:
	@if [ -x $(ATTEND_BIN) ] && $(ATTEND_BIN) --version >/dev/null 2>&1; then \
		echo "attend already built."; \
	elif bash tools/attend/download-attend.sh; then \
		echo "Pre-built attend binary installed."; \
		$(MAKE) -s --no-print-directory _attend_state_hint; \
	elif command -v cargo >/dev/null 2>&1; then \
		echo "Pre-built unavailable (see above) — building attend from source..."; \
		cargo build --release --manifest-path tools/Cargo.toml -p attend; \
		mkdir -p bin; \
		$(LINK) "$(CURDIR)/tools/target/release/attend$(EXE)" $(ATTEND_BIN); \
		echo "Built: $(ATTEND_BIN) ($$(ls -lh $(ATTEND_BIN) | awk '{print $$5}'))"; \
		$(MAKE) -s --no-print-directory _attend_state_hint; \
	else \
		echo "error: No pre-built binary and cargo not found. Install Rust: https://rustup.rs/"; \
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
	@if [ -x $(ATTEND_CHAT_BIN) ] && $(ATTEND_CHAT_BIN) --version >/dev/null 2>&1; then \
		echo "attend-chat already built."; \
	elif bash tools/attend-chat/download-attend-chat.sh; then \
		echo "Pre-built attend-chat binary installed."; \
		$(MAKE) -s --no-print-directory _attend_state_hint; \
	elif command -v cargo >/dev/null 2>&1; then \
		echo "Pre-built unavailable (see above) — building attend-chat from source..."; \
		cargo build --release --manifest-path tools/Cargo.toml -p attend-chat; \
		mkdir -p bin; \
		$(LINK) "$(CURDIR)/tools/target/release/attend-chat$(EXE)" $(ATTEND_CHAT_BIN); \
		echo "Built: $(ATTEND_CHAT_BIN) ($$(ls -lh $(ATTEND_CHAT_BIN) | awk '{print $$5}'))"; \
		$(MAKE) -s --no-print-directory _attend_state_hint; \
	else \
		echo "error: No pre-built binary and cargo not found. Install Rust: https://rustup.rs/"; \
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

test: lint test-smoke test-unit test-sim test-adr test-statusline test-hooks
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

test-adr:
	@echo "Running adr tool tests (lint, archive, golden output, import round trip)..."
	@python3 hooks/ways/documentation/adr/assemble --check
	@bash tests/adr-lint-test.sh
	@bash tests/adr-archive-test.sh
	@bash tests/adr-golden-test.sh
	@bash tests/adr-import-roundtrip.sh
	@bash tests/adr-macro-test.sh
	@bash tests/adr-conversion-check.sh
	@bash tests/adr-template-test.sh
	@docs/scripts/adr lint --check >/dev/null || { docs/scripts/adr lint; exit 1; }
	@docs/scripts/adr cite --check >/dev/null || { docs/scripts/adr cite; exit 1; }
	@echo "adr tool tests passed."

test-statusline:
	@echo "Running statusline tests..."
	@bash tests/statusline-test.sh

test-hooks:
	@echo "Running hook script tests..."
	@cargo build --manifest-path tools/Cargo.toml -p ways --quiet
	@bash tests/hooks-test.sh
	@bash tests/gh-tasks-test.sh

test-unit:
	@echo "Running Rust unit tests..."
	@# Every crate, including agent-theme (and its raw-colour lint over the workspace, ADR-504 §6)
	@# and ways-cli's piped_output test.
	@cargo test --manifest-path tools/Cargo.toml --workspace --quiet
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

# Live integration fixture (ADR-186): a Debian container with Claude Code,
# a seeded home, and the installer run unattended. TIER=1 needs no API key.
# The branch flavor mounts the checkout and the four suite binaries.
test-live:
	@TIER="$(or $(TIER),1)" bash tests/fixtures/docker/test-live.sh

# --- Release ---

# Release a Cargo-versioned component (ADR-150). Two steps, because main is
# branch-protected and PR-first:
#   make cut-release COMPONENT=ways LEVEL=patch      # 1) open a version-bump PR
#   (merge that PR, then: git checkout main && git pull)
#   make publish-release COMPONENT=ways [PUSH=1]     # 2) tag the merged bump + publish
# publish-release without PUSH=1 creates the tag locally and prints the push
# command; PUSH=1 pushes it (CI then builds all platforms + creates the Release).
cut-release:
	@bash scripts/release.sh bump "$(COMPONENT)" "$(LEVEL)"

publish-release:
	@bash scripts/release.sh tag "$(COMPONENT)" $(if $(PUSH),--push,)

# --- Supporting ---

clean:
	$(MAKE) -C tools/way-embed clean
	cargo clean --manifest-path tools/ways-cli/Cargo.toml 2>/dev/null || true
	cargo clean --manifest-path tools/Cargo.toml 2>/dev/null || true
	rm -rf dist/

# Wipe all attend / attend-chat runtime cache state under
# ~/.cache/attend/. Recovery target only — NEVER a dependency of
# setup, update-binaries, or any rebuild target.
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
