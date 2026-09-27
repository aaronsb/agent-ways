#!/usr/bin/env bash
# Golden-output test for the adr tool (#560, the ADR-304 rollout baseline).
#
# Runs the tool's read and write commands against the fixture corpus in
# tests/fixtures/adr/corpus and diffs each output against
# tests/fixtures/adr/golden. Later steps in the rollout (#561 onward) must keep
# these outputs byte-identical for v0 corpora.
#
# Usage:
#   tests/adr-golden-test.sh            diff against the goldens
#   tests/adr-golden-test.sh --update   rewrite the goldens from this run
#
# ADR_TOOL overrides the tool under test (default: docs/scripts/adr), so an
# assembled or rewritten tool can be checked against the same goldens.

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
ADR_TOOL="${ADR_TOOL:-$REPO_ROOT/docs/scripts/adr}"
CORPUS="$SCRIPT_DIR/fixtures/adr/corpus"
GOLDEN="$SCRIPT_DIR/fixtures/adr/golden"
UPDATE=0
[[ "${1:-}" == "--update" ]] && UPDATE=1

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
ACTUAL="$WORK/actual"
mkdir -p "$ACTUAL"

# A fresh git repo per run: the tool finds its root through git, and archive
# uses git mv. Commit identity is fixed so nothing reads the host's config.
fresh_corpus() {
  rm -rf "$WORK/repo"
  cp -r "$CORPUS" "$WORK/repo"
  (cd "$WORK/repo" && git init -q && git add -A \
    && git -c user.name=fixture -c user.email=fixture@example.invalid commit -qm fixture)
}

# Replace what varies between machines and days with fixed tokens.
normalize() {
  sed -e "s#$WORK/repo#<ROOT>#g" -e "s#$(date +%Y-%m-%d)#<TODAY>#g"
}

# capture NAME CMD...  — run the tool in the corpus, keep stdout+stderr and
# the exit code, both normalized
capture() {
  local name="$1"; shift
  local rc
  (cd "$WORK/repo" && "$ADR_TOOL" "$@") > "$ACTUAL/$name.out" 2>&1
  rc=$?
  { cat "$ACTUAL/$name.out"; printf '[exit %d]\n' "$rc"; } | normalize > "$ACTUAL/$name.tmp"
  mv "$ACTUAL/$name.tmp" "$ACTUAL/$name.out"
}

# --- read commands on the untouched corpus ------------------------------------

fresh_corpus
capture list                 list
capture list-group           list --group
capture list-all             list --all
capture list-archived        list --archived
capture list-status-accepted list --status Accepted
capture list-domain-system   list --domain system
capture view-101             view 101
capture view-legacy-005      view 005
capture view-missing         view 999
capture lint                 lint
capture lint-check           lint --check
capture lint-one             lint docs/architecture/system/ADR-104-hook-priorities.md
capture domains              domains
capture config               config

# --- write commands, each on a fresh corpus, capturing the files they write --

fresh_corpus
capture index-write index -y
normalize < "$WORK/repo/docs/architecture/INDEX.md" > "$ACTUAL/index-file.md"

fresh_corpus
capture new-system new system "Queue backpressure"
# The tool numbers from the lowest free slot in the domain (#549 tracks that),
# so find the file it wrote rather than assuming a number.
created=$(cd "$WORK/repo" && git ls-files --others --exclude-standard -- docs/architecture)
if [[ $(wc -w <<<"$created") -eq 1 ]]; then
  { echo "path: $created"; normalize < "$WORK/repo/$created"; } > "$ACTUAL/new-system-file.md"
else
  echo "adr new wrote $(wc -w <<<"$created") files, expected 1: $created" > "$ACTUAL/new-system-file.md"
fi

fresh_corpus
capture new-unknown-domain new nosuchdomain "Nowhere"

fresh_corpus
capture rename-103 rename 103 "Read-through cache layer"
(cd "$WORK/repo" && git status --porcelain | normalize) > "$ACTUAL/rename-103-status.txt"

# --- compare or update ----------------------------------------------------------

if [[ $UPDATE -eq 1 ]]; then
  rm -rf "$GOLDEN"
  mkdir -p "$GOLDEN"
  cp "$ACTUAL"/* "$GOLDEN/"
  echo "goldens written: $(ls "$GOLDEN" | wc -l) files in tests/fixtures/adr/golden"
  exit 0
fi

PASS=0
FAIL=0
for f in "$GOLDEN"/*; do
  name=$(basename "$f")
  if diff -u "$f" "$ACTUAL/$name" > "$WORK/diff" 2>&1; then
    PASS=$((PASS + 1))
  else
    FAIL=$((FAIL + 1))
    echo "  FAIL: $name"
    sed 's/^/    /' "$WORK/diff" | head -40
  fi
done
for f in "$ACTUAL"/*; do
  [[ -e "$GOLDEN/$(basename "$f")" ]] || { FAIL=$((FAIL + 1)); echo "  FAIL: no golden for $(basename "$f")"; }
done

echo ""
echo "=== ADR Golden Tests: $PASS passed, $FAIL failed ==="
[[ $FAIL -eq 0 ]]
