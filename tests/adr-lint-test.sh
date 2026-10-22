#!/usr/bin/env bash
# Test ADR lint frontmatter detection
#
# Tests that `adr lint` correctly identifies:
# - Missing YAML frontmatter with inline metadata
# - Missing YAML frontmatter without inline metadata
# - Valid YAML frontmatter (no false positives)
# - Unclosed frontmatter delimiters
# - Field-level errors in valid frontmatter

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$SCRIPT_DIR/.."
ADR_TOOL="$REPO_ROOT/docs/scripts/adr"

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

PASS=0
FAIL=0

assert_lint_contains() {
  local desc="$1"
  local file="$2"
  local pattern="$3"

  local output
  output=$("$ADR_TOOL" lint "$file" 2>&1) || true

  if echo "$output" | grep -qE "$pattern"; then
    echo "  PASS: $desc"
    PASS=$((PASS + 1))
  else
    echo "  FAIL: $desc"
    echo "    Expected pattern: $pattern"
    echo "    Got: $output"
    FAIL=$((FAIL + 1))
  fi
}

assert_lint_not_contains() {
  local desc="$1"
  local file="$2"
  local pattern="$3"

  local output
  output=$("$ADR_TOOL" lint "$file" 2>&1) || true

  if echo "$output" | grep -qE "$pattern"; then
    echo "  FAIL: $desc"
    echo "    Should NOT match: $pattern"
    echo "    Got: $output"
    FAIL=$((FAIL + 1))
  else
    echo "  PASS: $desc"
    PASS=$((PASS + 1))
  fi
}

assert_lint_exit() {
  local desc="$1"
  local file="$2"
  local expected_exit="$3"

  local actual_exit=0
  "$ADR_TOOL" lint --check "$file" >/dev/null 2>&1 || actual_exit=$?

  if [[ "$actual_exit" -eq "$expected_exit" ]]; then
    echo "  PASS: $desc"
    PASS=$((PASS + 1))
  else
    echo "  FAIL: $desc (expected exit $expected_exit, got $actual_exit)"
    FAIL=$((FAIL + 1))
  fi
}

# --- Fixtures ---

# Inline metadata (old pattern, no frontmatter)
cat > "$TMPDIR/ADR-001-inline.md" << 'EOF'
# ADR-001: Inline Metadata Test

Status: Accepted
Date: 2026-01-01
Deciders: @alice, @bob

## Context
Some context.

## Decision
Some decision.
EOF

# No metadata at all
cat > "$TMPDIR/ADR-002-bare.md" << 'EOF'
# ADR-002: Bare Document

## Context
Just a markdown file with no metadata.
EOF

# Valid YAML frontmatter
cat > "$TMPDIR/ADR-003-valid.md" << 'EOF'
---
status: Accepted
date: 2026-01-15
deciders:
  - alice
  - bob
related: []
---

# ADR-003: Valid Frontmatter

## Context
Properly formatted ADR.
EOF

# Unclosed frontmatter
cat > "$TMPDIR/ADR-004-unclosed.md" << 'EOF'
---
status: Draft
date: 2026-01-20

# ADR-004: Unclosed Frontmatter

## Context
Missing closing --- delimiter.
EOF

# Valid frontmatter but missing fields
cat > "$TMPDIR/ADR-005-partial.md" << 'EOF'
---
status: Draft
---

# ADR-005: Partial Frontmatter

## Context
Has frontmatter but missing date and deciders.
EOF

# Valid frontmatter with invalid status
cat > "$TMPDIR/ADR-006-badstatus.md" << 'EOF'
---
status: Approved
date: 2026-02-01
deciders:
  - alice
---

# ADR-006: Bad Status Value

## Context
Status value not in valid list.
EOF

# A basis entry with two via: keys. YAML loading keeps the second and drops
# the first, so lint must name the key and both lines (#850).
cat > "$TMPDIR/ADR-007-dupkey.md" << 'EOF'
---
contract: adr/v1
kind: decision
verb: change
capability: adr
basis:
  - operator: alice
    level: directed
    said: "keep the first answer"
    via: "chat, quoted context the author wrote"
    via: "chat"
agent:
  name: Claude
  model: claude-opus-5-5
status: proposed
date: 2026-02-01
deciders:
  - alice
---

# ADR-007: Duplicate Key In A Basis Entry

## Context
A basis entry repeats via.
EOF

# The same record with one via: is not flagged.
sed '/^    via: "chat"$/d' "$TMPDIR/ADR-007-dupkey.md" > "$TMPDIR/ADR-008-onekey.md"

# Merge keys: an override of a merged key is not a duplicate, and a real
# duplicate beside a merge key still is.
cat > "$TMPDIR/ADR-009-merge.md" << 'EOF'
---
status: proposed
date: 2026-02-01
deciders:
  - alice
defaults: &defaults
  level: directed
  via: "chat"
other: &other
  said: "base"
child:
  <<: *defaults
  via: "override"
pair:
  <<: [*defaults, *other]
  said: "override"
twice:
  <<: *defaults
  said: "first"
  said: "second"
---

# ADR-009: Merge Keys

## Context
Merge overrides and one real duplicate.
EOF

# --- Tests ---

echo "Inline metadata detection"
assert_lint_contains \
  "should detect inline metadata pattern" \
  "$TMPDIR/ADR-001-inline.md" \
  "No YAML frontmatter.*found inline metadata"

assert_lint_not_contains \
  "should not report field-level errors when frontmatter missing" \
  "$TMPDIR/ADR-001-inline.md" \
  "Missing (status|date) in frontmatter"

echo ""
echo "Bare document (no metadata)"
assert_lint_contains \
  "should report missing frontmatter" \
  "$TMPDIR/ADR-002-bare.md" \
  "No YAML frontmatter found"

assert_lint_not_contains \
  "should not mention inline metadata" \
  "$TMPDIR/ADR-002-bare.md" \
  "inline metadata"

echo ""
echo "Valid frontmatter"
assert_lint_not_contains \
  "should report no frontmatter errors" \
  "$TMPDIR/ADR-003-valid.md" \
  "(No YAML frontmatter|Missing|❌)"

assert_lint_exit \
  "should exit 0 with --check" \
  "$TMPDIR/ADR-003-valid.md" \
  0

echo ""
echo "Unclosed frontmatter"
assert_lint_contains \
  "should detect unclosed frontmatter" \
  "$TMPDIR/ADR-004-unclosed.md" \
  "Opening.*no closing"

echo ""
echo "Partial frontmatter (missing fields)"
assert_lint_contains \
  "should report missing date" \
  "$TMPDIR/ADR-005-partial.md" \
  "Missing date in frontmatter"

assert_lint_contains \
  "should report missing deciders" \
  "$TMPDIR/ADR-005-partial.md" \
  "Missing deciders in frontmatter"

assert_lint_not_contains \
  "should not report missing frontmatter" \
  "$TMPDIR/ADR-005-partial.md" \
  "No YAML frontmatter"

echo ""
echo "Invalid status value"
assert_lint_contains \
  "should report invalid status" \
  "$TMPDIR/ADR-006-badstatus.md" \
  "Invalid status.*Approved"

echo ""
echo "Duplicate frontmatter key"
assert_lint_contains \
  "should report the duplicate via: with both lines as an error" \
  "$TMPDIR/ADR-007-dupkey.md" \
  "❌ Duplicate key basis\[0\]\.via in frontmatter \(lines 10 and 11\)"

assert_lint_exit \
  "should exit 1 for a duplicate key" \
  "$TMPDIR/ADR-007-dupkey.md" \
  1

assert_lint_not_contains \
  "should not flag the record with a single via:" \
  "$TMPDIR/ADR-008-onekey.md" \
  "Duplicate key"

assert_lint_not_contains \
  "should not flag a key that overrides a merged one" \
  "$TMPDIR/ADR-009-merge.md" \
  "Duplicate key (child|pair|defaults|other)"

assert_lint_contains \
  "should flag a real duplicate beside a merge key" \
  "$TMPDIR/ADR-009-merge.md" \
  "❌ Duplicate key twice\.said in frontmatter \(lines 19 and 20\)"

echo ""
echo "--check exit code"
assert_lint_exit \
  "should exit 1 for files with errors" \
  "$TMPDIR/ADR-001-inline.md" \
  1

echo ""
echo "=== ADR Lint Tests: $PASS passed, $FAIL failed ==="
[[ $FAIL -eq 0 ]]
