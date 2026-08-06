#!/usr/bin/env bash
# Run all automated tests. Exit non-zero if any fail.
#
# Usage: tests/run-all.sh [--verbose]

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$SCRIPT_DIR/.."

PASS=0
FAIL=0

run_suite() {
  local name="$1"
  shift
  echo ""
  echo "=== $name ==="
  echo ""
  if "$@" 2>&1; then
    PASS=$((PASS + 1))
  else
    FAIL=$((FAIL + 1))
    echo "^^^ FAILED: $name"
  fi
}

# Session simulation tests (Rust integration)
if command -v cargo &>/dev/null; then
  run_suite "Session Simulation Tests" \
    cargo test --manifest-path "$REPO_ROOT/tools/ways-cli/Cargo.toml" --test session_sim -- --test-threads=1
else
  echo ""
  echo "=== Session Simulation Tests ==="
  echo "SKIP: cargo not found (install Rust toolchain)"
fi

# Ways lint (frontmatter validation)
WAYS_BIN="$REPO_ROOT/bin/ways"
if [[ -x "$WAYS_BIN" ]]; then
  run_suite "Ways Frontmatter Lint" "$WAYS_BIN" lint --global --check
else
  echo ""
  echo "=== Ways Frontmatter Lint ==="
  echo "SKIP: bin/ways not found (run 'make setup')"
fi

# Embedding engine tests (if way-embed available)
EMBED_BIN="$HOME/.cache/claude-ways/user/way-embed"
if [[ -x "$EMBED_BIN" ]]; then
  run_suite "Embedding Engine Tests" bash "$REPO_ROOT/tools/way-embed/test-embedding.sh"
else
  echo ""
  echo "=== Embedding Engine Tests ==="
  echo "SKIP: way-embed not found (run 'make setup')"
fi

# Multilingual way matching tests
if [[ -x "$WAYS_BIN" ]]; then
  run_suite "Multilingual Way Matching" bash "$SCRIPT_DIR/test-multilingual.sh"
fi

# ADR lint tests (frontmatter detection, field validation)
if command -v python3 &>/dev/null; then
  run_suite "ADR Lint Tests" bash "$REPO_ROOT/tests/adr-lint-test.sh"
  run_suite "ADR Archive Tests" bash "$REPO_ROOT/tests/adr-archive-test.sh"
else
  echo ""
  echo "=== ADR Lint Tests ==="
  echo "SKIP: python3 not found"
fi

# Doc-graph link integrity
run_suite "Doc-Graph Link Integrity" bash "$REPO_ROOT/scripts/doc-graph.sh" --stats

# Governance provenance lint
if [[ -x "$WAYS_BIN" ]]; then
  run_suite "Governance Provenance Lint" "$WAYS_BIN" governance lint
fi

echo ""
echo "=== Summary ==="
echo "Passed: $PASS  Failed: $FAIL"
[[ $FAIL -eq 0 ]]
