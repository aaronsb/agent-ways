#!/usr/bin/env bash
# Test `adr archive`, active-set semantics, and supersession reading (ADR-303 / issue #438)
#
# Runs entirely against a throwaway git repo in a temp dir — never the host repo.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$SCRIPT_DIR/.."
ADR_TOOL="$REPO_ROOT/hooks/ways/documentation/adr/adr-tool"

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

PASS=0
FAIL=0

pass() { echo "  PASS: $1"; PASS=$((PASS + 1)); }
fail() { echo "  FAIL: $1"; shift; for line in "$@"; do echo "    $line"; done; FAIL=$((FAIL + 1)); }

assert_contains() {
  local desc="$1" haystack="$2" pattern="$3"
  if echo "$haystack" | grep -qE "$pattern"; then pass "$desc"
  else fail "$desc" "expected pattern: $pattern" "got: $(echo "$haystack" | head -5)"; fi
}

assert_not_contains() {
  local desc="$1" haystack="$2" pattern="$3"
  if echo "$haystack" | grep -qE "$pattern"; then
    fail "$desc" "should NOT match: $pattern" "got: $(echo "$haystack" | grep -E "$pattern" | head -3)"
  else pass "$desc"; fi
}

assert_exit() {
  local desc="$1" expected="$2"; shift 2
  local actual=0
  "$@" >/dev/null 2>&1 || actual=$?
  if [[ "$actual" -eq "$expected" ]]; then pass "$desc"
  else fail "$desc" "expected exit $expected, got $actual"; fi
}

# ---------------------------------------------------------------------------
# Fixture: throwaway repo with three ADRs
# ---------------------------------------------------------------------------
cd "$TMPDIR"
git init -q repo
cd repo
git config user.email test@example.com
git config user.name Test

mkdir -p docs/architecture/system docs/scripts
cat > docs/architecture/adr.yaml <<'EOF'
project_name: Archive Test
domains:
  system:
    name: System
    description: Test domain
    range: [100, 199]
    folder: system
statuses:
  - Draft
  - Proposed
  - Accepted
  - Superseded
  - Deprecated
defaults:
  status: Draft
EOF

make_adr() {
  local num="$1" title="$2" status="$3" extra="${4:-}"
  cat > "docs/architecture/system/ADR-${num}-test.md" <<EOF
---
status: ${status}
date: 2026-01-01
deciders:
  - tester
${extra}---

# ADR-${num}: ${title}

## Context

Original context for ${num}.
$(for i in $(seq 1 30); do echo "Context filler line ${i} — realistic ADRs are long enough that git rename detection survives the banner."; done)

## Decision

Original decision for ${num}.
EOF
}

make_adr 101 "Old decision" Accepted
make_adr 102 "New decision" Accepted
make_adr 103 "Partially superseded decision" Accepted 'superseded_by:
  - "ADR-102#4"
'
git add -A && git commit -qm 'fixture'

echo "== Round trip"
out=$("$ADR_TOOL" archive 101 --reason "Replaced wholesale" --superseded-by ADR-102 2>&1)
assert_contains "archive reports the move" "$out" 'system/ADR-101-test.md -> .*archive/system/ADR-101-test.md'
[[ -f docs/architecture/archive/system/ADR-101-test.md ]] && pass "file exists at archive path" || fail "file exists at archive path"
[[ ! -f docs/architecture/system/ADR-101-test.md ]] && pass "file gone from active path" || fail "file gone from active path"
list_out=$("$ADR_TOOL" list 2>&1)
assert_not_contains "plain list excludes archived" "$list_out" 'ADR-101'
assert_contains "plain list keeps active" "$list_out" 'ADR-102'
archived_out=$("$ADR_TOOL" list --archived 2>&1)
assert_contains "list --archived finds it" "$archived_out" 'ADR-101'
assert_not_contains "list --archived excludes active" "$archived_out" 'ADR-102.*New decision'
all_out=$("$ADR_TOOL" list --all 2>&1)
assert_contains "list --all shows archived marker" "$all_out" 'ADR-101.*\[archived\]'
assert_contains "list --all shows active" "$all_out" 'ADR-102'

echo "== History"
porcelain=$(git status --porcelain)
# R = staged rename; the banner/status rewrite shows as a trailing M — both fine
assert_contains "git sees a rename (staged R)" "$porcelain" '^RM? docs/architecture/system/ADR-101-test.md -> docs/architecture/archive/system/ADR-101-test.md'
# --follow reports nothing on a merely-staged rename — assert after committing
git commit -qam 'archive ADR-101'
follow=$(git log --follow --oneline -- docs/architecture/archive/system/ADR-101-test.md)
[[ $(echo "$follow" | wc -l) -ge 2 ]] && pass "git log --follow reaches past the rename" \
  || fail "git log --follow reaches past the rename" "got: $follow"

echo "== Honesty"
archived_file=docs/architecture/archive/system/ADR-101-test.md
content=$(cat "$archived_file") || { fail "archived file readable"; content=""; }
assert_contains "status rewritten to Superseded" "$content" '^status: Superseded$'
assert_contains "frontmatter superseded_by written" "$content" 'ADR-102'
assert_contains "banner carries the reason" "$content" '\*\*Why:\*\* Replaced wholesale'
assert_contains "banner carries superseded-by" "$content" '\*\*Superseded by:\*\* ADR-102'
first_body_line=$(grep -vE '^(---|status:|date:|deciders:|  -|superseded_by:)' "$archived_file" | grep -v '^$' | head -1)
[[ "$first_body_line" == '# ADR-101: Old decision' ]] && pass "banner sits after the H1 (title reads first)" \
  || fail "banner sits after the H1 (title reads first)" "first body line: $first_body_line"
tail_archived=$(sed -n '/^## Context/,$p' "$archived_file")
tail_original=$(git show HEAD~1:docs/architecture/system/ADR-101-test.md | sed -n '/^## Context/,$p')
[[ "$tail_archived" == "$tail_original" ]] && pass "body below banner is byte-identical" \
  || fail "body below banner is byte-identical"

echo "== Refusals"
assert_exit "missing --reason refuses (non-zero)" 2 "$ADR_TOOL" archive 102
assert_exit "unknown status refuses" 1 "$ADR_TOOL" archive 102 --reason x --status Bogus
assert_exit "unknown ADR number refuses" 1 "$ADR_TOOL" archive 999 --reason x
assert_exit "unresolvable --superseded-by refuses" 1 "$ADR_TOOL" archive 102 --reason x --superseded-by ADR-999
assert_exit "second archive is a no-op (exit 0)" 0 "$ADR_TOOL" archive 101 --reason x
out=$("$ADR_TOOL" archive 103 --reason x 2>&1) && rc=0 || rc=$?
[[ $rc -ne 0 ]] && pass "partially superseded doc refuses to archive" \
  || fail "partially superseded doc refuses to archive" "exit was 0" "$out"
assert_contains "partial refusal names the state" "$out" 'partially superseded'

echo "== Index"
"$ADR_TOOL" index -y >/dev/null 2>&1
index=$(cat docs/architecture/INDEX.md)
main_tables=$(sed -n '1,/^## Archived/p' docs/architecture/INDEX.md)
assert_not_contains "archived ADR absent from main tables (old path)" "$main_tables" 'system/ADR-101-test.md'
assert_not_contains "archived ADR absent from main tables (archive path too)" "$main_tables" 'archive/system/ADR-101-test.md'
assert_contains "archived section lists it at the archive path" "$index" 'archive/system/ADR-101-test.md'
assert_contains "partial supersession surfaced in index" "$index" 'partially superseded by ADR-102 §4'

echo "== Lint"
# malformed archived ADR must still fail lint
cat > docs/architecture/archive/system/ADR-150-broken.md <<'EOF'
# ADR-150: Broken archived ADR

No frontmatter at all.
EOF
lint_out=$("$ADR_TOOL" lint --check 2>&1) && rc=0 || rc=$?
[[ $rc -ne 0 ]] && pass "malformed archived ADR still fails lint" || fail "malformed archived ADR still fails lint"
assert_contains "lint names the archived file" "$lint_out" 'ADR-150-broken'
rm docs/architecture/archive/system/ADR-150-broken.md
# unresolvable superseded_by fails; non-reciprocal warns
make_adr 104 "Dangling reference" Accepted 'superseded_by:
  - ADR-777
'
lint_out=$("$ADR_TOOL" lint 2>&1)
assert_contains "unresolvable superseded_by is an error" "$lint_out" "superseded_by: 'ADR-777' resolves to no known ADR"
rm docs/architecture/system/ADR-104-test.md
lint_out=$("$ADR_TOOL" lint 2>&1)
assert_contains "non-reciprocal link warns" "$lint_out" 'ADR-102 does not declare the reciprocal supersedes'

echo "== Dry run"
before=$(git status --porcelain; ls docs/architecture/system)
out=$("$ADR_TOOL" archive 102 --reason "dry" --dry-run 2>&1)
after=$(git status --porcelain; ls docs/architecture/system)
assert_contains "dry run reports the move" "$out" 'Would archive ADR-102'
[[ "$before" == "$after" ]] && pass "dry run changes nothing" || fail "dry run changes nothing"

echo "== Review regressions (PR #442)"
# 1: adr new must not reissue an archived ADR's number (101 is archived)
new_out=$("$ADR_TOOL" new system "Reissue check" 2>&1)
assert_not_contains "adr new does not reissue archived number" "$new_out" 'ADR-101'
rm -f docs/architecture/system/ADR-*-reissue-check.md

# 2: a YAML '# comment' in frontmatter must not attract the banner
cat > docs/architecture/system/ADR-105-test.md <<'EOF'
---
status: Accepted
# yaml comment that looks like an H1 scan target
date: 2026-01-01
deciders:
  - tester
---

# ADR-105: Comment in frontmatter

## Context

Body.
EOF
"$ADR_TOOL" archive 105 --reason "comment fixture" >/dev/null 2>&1
arch105=docs/architecture/archive/system/ADR-105-test.md
fm_block=$(awk '/^---$/{n++; next} n==1' "$arch105")
assert_not_contains "banner not injected into frontmatter" "$fm_block" 'ARCHIVED'
assert_contains "banner present in body" "$(cat "$arch105")" 'ARCHIVED'
assert_contains "comment fixture still has valid status" "$(cat "$arch105")" '^status: Superseded$'

# 4: an ADR with no status field gains one on archive
cat > docs/architecture/system/ADR-106-test.md <<'EOF'
---
date: 2026-01-01
deciders:
  - tester
---

# ADR-106: No status field

## Context

Body.
EOF
"$ADR_TOOL" archive 106 --reason "status fixture" >/dev/null 2>&1
assert_contains "missing status is appended on archive" \
  "$(cat docs/architecture/archive/system/ADR-106-test.md)" '^status: Superseded$'

# 9: --all and --archived are mutually exclusive
assert_exit "list --all --archived refuses" 2 "$ADR_TOOL" list --all --archived

# 3: a repo living under a directory named 'archive' still sees its active set
mkdir -p "$TMPDIR/archive"
cd "$TMPDIR/archive"
git init -q repo2 && cd repo2
git config user.email test@example.com && git config user.name Test
mkdir -p docs/architecture/system
cp "$TMPDIR/repo/docs/architecture/adr.yaml" docs/architecture/adr.yaml
cat > docs/architecture/system/ADR-110-test.md <<'EOF'
---
status: Accepted
date: 2026-01-01
deciders:
  - tester
---

# ADR-110: Repo under an archive dir

## Context

Body.
EOF
list_out=$("$ADR_TOOL" list 2>&1)
assert_contains "repo under archive/ parent still lists active ADRs" "$list_out" 'ADR-110'
cd "$TMPDIR/repo"

echo
echo "Results: $PASS passed, $FAIL failed"
[[ $FAIL -eq 0 ]]
