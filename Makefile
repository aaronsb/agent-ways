# agent-ways
# Top-level Makefile — build, install, and release.
#
# Quick start:   make setup
# Full install:  make install
# Update:        make update

.DEFAULT_GOAL := help
.PHONY: setup install uninstall update clean help ways ways-rebuild attend attend-rebuild attend-chat attend-chat-rebuild lint test test-unit test-sim test-lang test-locales test-multilingual release

WAYS_BIN = bin/ways
ATTEND_BIN = bin/attend
ATTEND_CHAT_BIN = bin/attend-chat
XDG_BIN = $(or $(XDG_BIN_HOME),$(HOME)/.local/bin)

# --- Primary targets ---

help:
	@echo "agent-ways"
	@echo ""
	@echo "  make setup        Build ways CLI + attend + fetch embedding model + corpus"
	@echo "  make install      Full first-time setup (hooks + tools + PATH)"
	@echo "  make update       Pull latest changes and re-run install"
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
	@echo "  make release      Build release binary for current platform"
	@echo "  make uninstall    Remove ways from PATH"
	@echo "  make clean        Remove build artifacts"
	@echo ""

# Build ways CLI + set up embedding engine + generate initial corpus.
setup: ways attend attend-chat
	@echo "Setting up embedding engine..."
	$(MAKE) -C tools/way-embed setup
	@echo ""
	@echo "Setting up mmaid diagram renderer..."
	@bash tools/mmaid/download-mmaid.sh || echo "  (mmaid optional — skipping)"
	@echo ""
	@echo "Generating corpus..."
	@$(WAYS_BIN) corpus --quiet

# Full install: build, setup, symlink to PATH.
install: hooks-executable setup
	@mkdir -p $(XDG_BIN)
	@ln -sf $(CURDIR)/$(WAYS_BIN) $(XDG_BIN)/ways
	@ln -sf $(CURDIR)/$(ATTEND_BIN) $(XDG_BIN)/attend
	@ln -sf $(CURDIR)/$(ATTEND_CHAT_BIN) $(XDG_BIN)/attend-chat
	@echo ""
	@echo "Install complete."
	@echo "  ways binary:        $(XDG_BIN)/ways → $(CURDIR)/$(WAYS_BIN)"
	@echo "  attend binary:      $(XDG_BIN)/attend → $(CURDIR)/$(ATTEND_BIN)"
	@echo "  attend-chat binary: $(XDG_BIN)/attend-chat → $(CURDIR)/$(ATTEND_CHAT_BIN)"
	@echo "  Restart Claude Code for ways to take effect."

# Remove symlink from PATH.
uninstall:
	@rm -f $(XDG_BIN)/ways $(XDG_BIN)/attend $(XDG_BIN)/attend-chat
	@echo "Removed $(XDG_BIN)/ways $(XDG_BIN)/attend $(XDG_BIN)/attend-chat"

# Pull upstream and re-setup.
update:
	git pull --ff-only
	$(MAKE) install

# --- Build ---

# Get the ways binary: try existing → download → build from source.
ways:
	@if [ -x $(WAYS_BIN) ] && $(WAYS_BIN) --version >/dev/null 2>&1; then \
		echo "ways already installed: $$($(WAYS_BIN) --version)"; \
	elif bash tools/ways-cli/download-ways.sh 2>/dev/null; then \
		echo "Pre-built binary installed."; \
	elif command -v cargo >/dev/null 2>&1; then \
		echo "No pre-built binary, building from source..."; \
		cargo build --release --manifest-path tools/Cargo.toml -p ways; \
		mkdir -p bin; \
		ln -sf $(CURDIR)/tools/target/release/ways $(WAYS_BIN); \
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
	cargo build --release --manifest-path tools/Cargo.toml -p ways
	@mkdir -p bin
	@ln -sf $(CURDIR)/tools/target/release/ways $(WAYS_BIN)
	@echo "Built: $(WAYS_BIN) ($$(ls -lh $(WAYS_BIN) | awk '{print $$5}'))"

# Build attend binary from workspace.
attend:
	@if [ -x $(ATTEND_BIN) ] && $(ATTEND_BIN) --help >/dev/null 2>&1; then \
		echo "attend already built."; \
	elif command -v cargo >/dev/null 2>&1; then \
		echo "Building attend..."; \
		cargo build --release --manifest-path tools/Cargo.toml -p attend; \
		mkdir -p bin; \
		ln -sf $(CURDIR)/tools/target/release/attend $(ATTEND_BIN); \
		echo "Built: $(ATTEND_BIN) ($$(ls -lh $(ATTEND_BIN) | awk '{print $$5}'))"; \
	else \
		echo "error: cargo not found. Install Rust: https://rustup.rs/"; \
		exit 1; \
	fi

# Force rebuild attend from source.
attend-rebuild:
	@if ! command -v cargo >/dev/null 2>&1; then \
		echo "error: cargo not found. Install Rust: https://rustup.rs/"; \
		exit 1; \
	fi
	cargo build --release --manifest-path tools/Cargo.toml -p attend
	@mkdir -p bin
	@ln -sf $(CURDIR)/tools/target/release/attend $(ATTEND_BIN)
	@echo "Built: $(ATTEND_BIN) ($$(ls -lh $(ATTEND_BIN) | awk '{print $$5}'))"

# Build attend-chat binary from workspace.
attend-chat:
	@if [ -x $(ATTEND_CHAT_BIN) ] && $(ATTEND_CHAT_BIN) --help >/dev/null 2>&1; then \
		echo "attend-chat already built."; \
	elif command -v cargo >/dev/null 2>&1; then \
		echo "Building attend-chat..."; \
		cargo build --release --manifest-path tools/Cargo.toml -p attend-chat; \
		mkdir -p bin; \
		ln -sf $(CURDIR)/tools/target/release/attend-chat $(ATTEND_CHAT_BIN); \
		echo "Built: $(ATTEND_CHAT_BIN) ($$(ls -lh $(ATTEND_CHAT_BIN) | awk '{print $$5}'))"; \
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
	@ln -sf $(CURDIR)/tools/target/release/attend-chat $(ATTEND_CHAT_BIN)
	@echo "Built: $(ATTEND_CHAT_BIN) ($$(ls -lh $(ATTEND_CHAT_BIN) | awk '{print $$5}'))"

# --- Test ---

test: lint test-smoke test-unit test-sim test-lang test-locales test-multilingual
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
	assert len(active) > 0, 'No active locales found in corpus'" \
	&& echo "  Language coverage: PASS"

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

# --- Supporting ---

hooks-executable:
	@find hooks -name '*.sh' -exec chmod +x {} + 2>/dev/null || true
	@echo "Hooks marked executable."

clean:
	$(MAKE) -C tools/way-embed clean
	cargo clean --manifest-path tools/ways-cli/Cargo.toml 2>/dev/null || true
	cargo clean --manifest-path tools/Cargo.toml 2>/dev/null || true
	rm -rf dist/
