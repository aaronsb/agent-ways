#!/usr/bin/env bash
# Golden-output test for the adr tool (#560, the ADR-304 rollout baseline).
#
# Runs the tool's read and write commands against two fixture corpora and diffs
# each output against tests/fixtures/adr/golden. Later steps in the rollout
# (#561 onward) must keep these outputs byte-identical for v0 corpora, and that
# guarantee reaches only the paths exercised here.
#
#   corpus/   a well-formed corpus: three domains (one with two folders), a
#             legacy range, a decimal sub-part, an archive, every lifecycle
#             status, whole and section-level supersession, two lint defects
#   defects/  lint findings the main corpus does not carry, and a duplicate
#             number for the multiple-match branch of view
#   v1/       an adr/v1 corpus (ADR-304): every kind and verb used correctly,
#             and one record still on v0
#   v1-defects/  one broken record per v1 rule, and a malformed adr.yaml
#   v1-empty/ adr/v1 declared with no kinds and no capabilities
#
# Usage:
#   tests/adr-golden-test.sh            diff against the goldens
#   tests/adr-golden-test.sh --update   rewrite the goldens from this run
#
# ADR_TOOL overrides the tool under test (default: docs/scripts/adr), so an
# assembled or rewritten tool can be checked against the same goldens.
#
# `--help` output is deliberately absent: argparse formats it differently
# across Python versions.

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
ADR_TOOL="${ADR_TOOL:-$REPO_ROOT/docs/scripts/adr}"
# capture runs the tool from inside the fixture repo, so a relative path
# given by the caller has to be resolved first.
[[ "$ADR_TOOL" = /* ]] || ADR_TOOL="$PWD/$ADR_TOOL"
[[ -x "$ADR_TOOL" ]] || { echo "ADR_TOOL is not executable: $ADR_TOOL" >&2; exit 2; }
FIXTURES="$SCRIPT_DIR/fixtures/adr"
GOLDEN="$FIXTURES/golden"
TODAY="$(date +%Y-%m-%d)"
UPDATE=0
[[ "${1:-}" == "--update" ]] && UPDATE=1

# The date normalization below replaces today's date wherever it appears. It is
# safe only while no fixture carries today's date as content.
if grep -rqF "$TODAY" "$FIXTURES/corpus" "$FIXTURES/defects" "$FIXTURES/v1" "$FIXTURES/v1-defects" "$FIXTURES/v1-empty"; then
  echo "a fixture file contains today's date ($TODAY); fixture dates must be in the past" >&2
  exit 2
fi

# The physical path: on macOS mktemp returns /var/..., while git reports the
# resolved /private/var/..., and <ROOT> has to match what the tool prints.
WORK="$(cd "$(mktemp -d)" && pwd -P)"
trap 'rm -rf "$WORK"' EXIT
ACTUAL="$WORK/actual"
mkdir -p "$ACTUAL"

# Git reads nothing from the host: no global or system config (signing,
# hooks, rename detection, excludes), and a fixed identity.
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1
export GIT_AUTHOR_NAME=fixture GIT_AUTHOR_EMAIL=fixture@example.invalid
export GIT_COMMITTER_NAME=fixture GIT_COMMITTER_EMAIL=fixture@example.invalid

# fresh CORPUS — a clean git repo holding a copy of the named corpus. The tool
# finds its root through git, and archive and rename use git mv.
fresh() {
  rm -rf "$WORK/repo"
  cp -r "$FIXTURES/$1" "$WORK/repo" \
    && (cd "$WORK/repo" && git init -q && git add -A && git commit -qm fixture) \
    || { echo "could not set up the $1 fixture repo" >&2; exit 2; }
}

# Replace what varies between machines and days with fixed tokens.
# The tool prints its own invocation path in some hints, and a rebuilt tool
# lives at a different path, so that path is normalized too.
# A run that crosses midnight sees two dates, so both the date the run
# started on and the current one become <TODAY>.
normalize() {
  sed -e "s#$ADR_TOOL#<ADR_TOOL>#g" -e "s#$WORK/repo#<ROOT>#g" \
      -e "s#$TODAY#<TODAY>#g" -e "s#$(date +%Y-%m-%d)#<TODAY>#g"
}

# capture NAME CMD... — run the tool in the fixture repo and keep its stdout,
# stderr and exit code, normalized. Stdin is empty, so a prompt reads EOF.
capture() {
  local name="$1"; shift
  local rc
  (cd "$WORK/repo" && "$ADR_TOOL" "$@") < /dev/null > "$ACTUAL/$name.out" 2>&1
  rc=$?
  { cat "$ACTUAL/$name.out"; printf '[exit %d]\n' "$rc"; } | normalize > "$ACTUAL/$name.tmp"
  mv "$ACTUAL/$name.tmp" "$ACTUAL/$name.out"
}

# edit PATH PY — rewrite a fixture-repo file with a Python expression over s.
# Portable where GNU and BSD sed disagree (newlines in replacements).
edit() {
  python3 - "$WORK/repo/$1" "$2" <<'PY'
import sys
path, expr = sys.argv[1], sys.argv[2]
s = open(path).read()
s = eval(expr)
open(path, 'w').write(s)
PY
}

# commit_all MSG — commit the fixture repo's working tree
commit_all() { (cd "$WORK/repo" && git add -A && git commit -qm "$1"); }

# keep NAME PATH — keep a file the tool wrote, normalized
keep() {
  if [[ -n "$2" && -f "$WORK/repo/$2" ]]; then
    { echo "path: $2"; normalize < "$WORK/repo/$2"; } > "$ACTUAL/$1"
  else
    echo "missing: $2" > "$ACTUAL/$1"
  fi
}

# worktree NAME — keep git's view of what the command changed
worktree() {
  (cd "$WORK/repo" && git add -A && git status --porcelain | normalize) > "$ACTUAL/$1"
}

# --- read commands on the untouched corpus ------------------------------------

fresh corpus
capture version              --version
capture list                 list
capture list-alias-ls        ls
capture list-group           list --group
capture list-all             list --all
capture list-archived        list --archived
capture list-status-accepted list --status Accepted
capture list-domain-system   list --domain system
capture list-domain-ops      list --domain ops
capture view-101             view 101
capture view-alias-v         v 101
capture view-alias-show      show ADR-012
capture view-decimal         view 101.1
capture view-section         view ADR-104#2
capture view-legacy-005      view 005
capture view-archived        view 107
capture view-missing         view 999
capture lint                 lint
capture lint-check           lint --check
capture lint-one             lint docs/architecture/system/ADR-104-hook-priorities.md
capture domains              domains
capture cite                 cite
capture cite-check           cite --check
capture cite-one-path        cite src/storage.py
capture cite-dir-path        cite src
capture cite-outside-path    cite ../..

# A family with one member in force and one superseded: a bare citation stays
# quiet, an explicit citation of the superseded member warns.
fresh corpus
(cd "$WORK/repo" && printf -- '---\nstatus: Superseded\ndate: 2025-06-02\ndeciders: [developer]\nsuperseded_by: [ADR-101]\n---\n\n# ADR-101.2: Old storage detail\n' > docs/architecture/system/ADR-101.2-old-detail.md \
  && printf '# ADR-101 as a family.\n# ADR-101.2 on its own.\n' > src/family.py)
capture cite-family cite src/family.py
capture config               config

# --- write commands, each on a fresh corpus, keeping the files they write ------

fresh corpus
capture index-new index -y
keep index-new-file.md docs/architecture/INDEX.md
capture index-current index
capture rename-for-index rename 103 "Read-through cache layer"
capture index-stale-skipped index
capture index-stale-updated index -y

fresh corpus
capture new-system new system "Queue backpressure"
worktree new-system-status.txt
# The tool numbers from the lowest free slot in the domain (#549 tracks that),
# so keep whichever file it wrote rather than assuming a number.
created=$(cd "$WORK/repo" && git diff --cached --name-only --diff-filter=A)
keep new-system-file.md "$created"

fresh corpus
capture new-docs new docs "Glossary conventions"
worktree new-docs-status.txt

fresh corpus
capture new-ops new ops "Rollback drill"
worktree new-ops-status.txt

fresh corpus
capture new-unknown-domain new nosuchdomain "Nowhere"

fresh corpus
capture rename-title rename 103 "Read-through cache layer"
worktree rename-title-status.txt
keep rename-title-file.md docs/architecture/system/ADR-103-read-through-cache-layer.md

fresh corpus
capture rename-slug rename 103 --slug cache
worktree rename-slug-status.txt

fresh corpus
capture rename-no-change rename 103
capture rename-missing rename 999 "Nothing"

fresh corpus
capture archive-rejected archive 103 --status Rejected --reason "Never built"
worktree archive-rejected-status.txt
keep archive-rejected-file.md docs/architecture/archive/system/ADR-103-cache-layer.md

fresh corpus
capture archive-superseded-dry archive 105 --superseded-by ADR-101 --reason "Folded into storage" --dry-run
worktree archive-superseded-dry-status.txt
capture archive-partial-refused archive 102 --reason "Only part of it is replaced"
capture archive-bad-successor archive 105 --superseded-by ADR-199 --reason "No such successor"

# --- the defects corpus ---------------------------------------------------------

fresh defects
capture defects-lint        lint
capture defects-lint-check  lint --check
capture defects-list        list
capture defects-view-dup    view 150

# --- adr/v1 (ADR-304): a clean corpus with one unmigrated v0 record, and a
# corpus with one defect per v1 rule plus a malformed adr.yaml -----------------

fresh v1
capture v1-lint        lint
capture v1-lint-check  lint --check
capture v1-list        list
capture v1-view-spec   view 102
capture v1-cite        cite
# The cut on search, enacted: citations of search records now fail.
(cd "$WORK/repo" && sed -i.bak 's/^verb: cut$/verb: cut\nenacted: abcdef1/' docs/architecture/system/ADR-111-cut-search.md \
  && rm docs/architecture/system/ADR-111-cut-search.md.bak)
capture v1-cite-enacted cite --check
capture v1-cite-no-inventory cite --no-inventory

# A cut undone by a later accepted add: search is present again.
fresh v1
(cd "$WORK/repo" && printf -- '---\ncontract: adr/v1\nkind: decision\nverb: add\ncapability: search\nstatus: accepted\ndate: 2025-05-20\ndeciders: [developer]\nagent: {name: Claude, model: m}\nbasis:\n  - evidence: demand returned\n---\n\n# ADR-115: Add search back\n' > docs/architecture/system/ADR-115-add-search-back.md)
capture v1-cite-readded cite --no-inventory

# A retire before enactment, with one target misspelled; then an inventory
# command that fails.
fresh v1
edit docs/architecture/system/ADR-105-retire-legacy-ingest.md "s.replace('enacted: 3f9c2a1\n', '').replace('cli:ingest-legacy', 'cli:ingest-legacy, cli:ingest-legcy')"
capture v1-cite-retire-pending cite
edit docs/architecture/adr.yaml "s[:s.index('  cli:')] + '  cli: { inventory: \"exit 3\" }' + s[s.index(chr(10), s.index('  cli:')):]"
capture v1-cite-inventory-fails cite

# A frozen decision edited after acceptance: a changed capability (an error),
# a body edited mid-text (a warning), and a mutable field (allowed).
fresh v1
edit docs/architecture/system/ADR-101-ingest.md "s.replace('capability: ingest\n', 'capability: search\n').replace('The decision.\n', 'The decision, rewritten.\n').replace('date: 2025-05-02\n', 'date: 2025-05-02\nconsidered: [{operator: developer, said: ok, via: PR 2}]\n')"
capture v1-frozen-lint lint docs/architecture/system/ADR-101-ingest.md

# The freeze follows a rename: renamed, committed, then edited.
fresh v1
(cd "$WORK/repo" && git mv docs/architecture/system/ADR-101-ingest.md docs/architecture/system/ADR-101-ingest-renamed.md)
commit_all rename
edit docs/architecture/system/ADR-101-ingest-renamed.md "s.replace('capability: ingest\n', 'capability: search\n')"
capture v1-frozen-renamed lint docs/architecture/system/ADR-101-ingest-renamed.md

# Proposed, then accepted in a later commit: accepting is not an edit.
fresh v1
edit docs/architecture/system/ADR-113-operator-proposed.md "s.replace('status: proposed\n', 'status: accepted\nconsidered: [{operator: developer, said: \"yes\", via: PR 13}]\n')"
commit_all accept
capture v1-frozen-accepted lint docs/architecture/system/ADR-113-operator-proposed.md

# An accepted v0 record migrated to v1 adds the v1 fields: the migration,
# not an edit of a frozen decision (ADR-304 §7).
fresh v1
edit docs/architecture/system/ADR-110-old-v0-record.md "s.replace('status: Accepted\n', 'contract: adr/v1\nkind: decision\nverb: add\ncapability: ingest\nstatus: accepted\nagent: {name: Claude, model: fixture-model}\nbasis:\n  - evidence: migrated from v0\n').replace('# ADR-110: An unmigrated v0 record\n', '# ADR-110: An unmigrated v0 record\n\n## Summary\n\n- **Probes:** *Confident:* a. *Not confident:* b.\n- **Inversion:** c.\n')"
capture v1-frozen-migrated lint docs/architecture/system/ADR-110-old-v0-record.md

# A non-UTF-8 blob in a record's history does not stop lint.
fresh v1
printf 'binary \377\376 junk\n' > "$WORK/repo/docs/architecture/system/ADR-102-ingest-spec.md"
commit_all "non-utf8"
cp "$FIXTURES/v1/docs/architecture/system/ADR-102-ingest-spec.md" "$WORK/repo/docs/architecture/system/ADR-102-ingest-spec.md"
commit_all restore
capture v1-frozen-nonutf8 lint docs/architecture/system/ADR-102-ingest-spec.md

# Lifecycle commands (ADR-304 §2, §11, §12)
fresh v1
capture v1-accept-refused       accept 113
capture v1-accept-not-proposed  accept 102
capture v1-accept-v0            accept 110
capture v1-accept-dry-run       accept 106 --dry-run
worktree v1-accept-dry-run-status.txt
capture v1-accept-concern       accept 114
keep v1-accept-concern-file.md docs/architecture/system/ADR-114-open-concern.md

fresh v1
capture v1-reject               reject 107 --reason "Superseded by the batching work before it landed"
keep v1-reject-file.md docs/architecture/system/ADR-107-ingest-notes.md
capture v1-abandon-no-reason    abandon 108 --reason "  "

fresh v1-defects
capture v1-defects-lint        lint
capture v1-defects-lint-check  lint --check

fresh v1-empty
capture v1-empty-lint  lint

# --- compare or update ----------------------------------------------------------

if [[ $UPDATE -eq 1 ]]; then
  for f in "$GOLDEN"/*; do
    [[ -e "$f" && ! -e "$ACTUAL/$(basename "$f")" ]] && echo "removed golden: $(basename "$f")"
  done
  rm -rf "$GOLDEN"
  mkdir -p "$GOLDEN"
  cp "$ACTUAL"/* "$GOLDEN/"
  echo "goldens written: $(ls "$GOLDEN" | wc -l | tr -d ' ') files in tests/fixtures/adr/golden"
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
