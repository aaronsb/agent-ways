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
capture defects-accept-dup  accept 150

# --- adr/v1 (ADR-304): a clean corpus with one unmigrated v0 record, and a
# corpus with one defect per v1 rule plus a malformed adr.yaml -----------------

fresh v1
capture v1-lint        lint
capture v1-lint-check  lint --check
capture v1-list        list
capture v1-view-spec   view 102
capture v1-lint-precedent-relative lint docs/architecture/system/ADR-109-precedent-chain.md
# adr new under adr/v1 (ADR-306 §3): a decision with its fields given, a spec,
# a bare decision whose empty fields lint names, and an unknown kind.
fresh v1
capture v1-new-decision new system "Stream exports" --verb change --capability ingest --agent Claude --model fixture-model
keep v1-new-decision-file.md docs/architecture/system/ADR-115-stream-exports.md
capture v1-new-decision-lint lint docs/architecture/system/ADR-115-stream-exports.md
capture v1-new-spec new system "Export format" --kind spec --capability ingest
keep v1-new-spec-file.md docs/architecture/system/ADR-116-export-format.md
capture v1-new-spec-lint lint docs/architecture/system/ADR-116-export-format.md
capture v1-new-bare new system "Bare decision"
keep v1-new-bare-file.md docs/architecture/system/ADR-117-bare-decision.md
capture v1-new-bare-lint lint docs/architecture/system/ADR-117-bare-decision.md
capture v1-new-unknown-kind new system "Policy thing" --kind policy
worktree v1-new-unknown-kind-status.txt
capture v1-new-refused new system "Spec with a verb" --kind spec --verb add --agent Claude --capability nosuch
# A value with a line that reads as the frontmatter fence is written on one
# line and reads back intact.
capture v1-new-multiline new system "Multiline model" --verb add --capability ingest --agent Claude --model "$(printf 'a\n---\nb')"
keep v1-new-multiline-file.md docs/architecture/system/ADR-118-multiline-model.md
capture v1-new-multiline-lint lint docs/architecture/system/ADR-118-multiline-model.md
# A placeholder left in a record that has left proposed is an error.
edit docs/architecture/system/ADR-115-stream-exports.md "s.replace('status: proposed', 'status: accepted')"
capture v1-new-placeholder-accepted lint docs/architecture/system/ADR-115-stream-exports.md

# The kind's schema, not its name, decides a new record's fields: a custom
# kind that takes a verb, requires targets and has a Summary section.
fresh v1
edit docs/architecture/adr.yaml "s.replace('  spec:\n', '  policy:\n    verb: required\n    requires: [capability, targets]\n    sections: [Summary]\n  spec:\n', 1)"
capture v1-new-custom-kind new system "Retention policy" --kind Policy --verb constrain --capability ingest
keep v1-new-custom-kind-file.md docs/architecture/system/ADR-115-retention-policy.md
# A v1 contract with no kinds: new says why it cannot write.
fresh v1-empty
capture v1-empty-new new system "Anything"

# Baseline capabilities (ADR-305). export joins the vocabulary as a baseline
# capability adopted on 2025-05-10: it needs no add decision.
baseline_fresh() {
  fresh v1
  edit docs/architecture/adr.yaml "s.replace('surfaces:', 'baseline:\\n  adopted: 2025-05-10\\n  capabilities: [export]\\n\\nsurfaces:', 1).replace('  search: Query over the store\\n', '  search: Query over the store\\n  export: Export from the store\\n')"
}
# export_change NUMBER SLUG DATE [EXTRA-FRONTMATTER-LINE]
export_change() {
  (cd "$WORK/repo" && printf -- '---\ncontract: adr/v1\nkind: decision\nverb: change\ncapability: export\n%sstatus: proposed\ndate: %s\ndeciders: [developer]\nagent: {name: Claude, model: m}\nbasis:\n  - evidence: export measured\n---\n\n# ADR-%s: %s\n\n## Summary\n\n- **Probes:** *Confident:* a. *Not confident:* b.\n- **Inversion:** c.\n' "${4:+$4\n}" "$3" "$1" "$2" > "docs/architecture/system/ADR-$1-$2.md")
}
baseline_fresh
capture v1-lint-baseline lint
# Malformed baseline config: a bare list, then an unknown name and a bad date.
edit docs/architecture/adr.yaml "s.replace('baseline:\\n  adopted: 2025-05-10\\n  capabilities: [export]', 'baseline: [export]')"
capture v1-lint-baseline-list lint
edit docs/architecture/adr.yaml "s.replace('baseline: [export]', 'baseline:\\n  adopted: May 2025\\n  capabilities: [export, exprt]')"
capture v1-lint-baseline-unknown lint
# A change before adoption stands on the baseline, and so does the first
# change after it. ADR-118 is numbered lower but dated later than ADR-119,
# so it is the second post-adoption change and must name ADR-119.
baseline_fresh
export_change 117 pre-adoption-export 2025-05-09
export_change 119 stream-exports 2025-05-20
export_change 118 compress-exports 2025-05-22
capture v1-lint-baseline-change lint
edit docs/architecture/system/ADR-118-compress-exports.md "s.replace('capability: export\n', 'capability: export\nsupersedes: [ADR-119]\n')"
capture v1-lint-baseline-change-names-prior lint
# A rejected post-adoption change is no prior.
edit docs/architecture/system/ADR-118-compress-exports.md "s.replace('supersedes: [ADR-119]\n', '')"
edit docs/architecture/system/ADR-119-stream-exports.md "s.replace('status: proposed', 'status: rejected')"
capture v1-lint-baseline-change-rejected-prior lint
fresh v1
fresh v1
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

# A record accepted on a review branch and revised there freezes where it
# merges, not at the branch's first accepted commit; editing it after the
# merge still fails.
fresh v1
(cd "$WORK/repo" && git switch -q -c review)
edit docs/architecture/system/ADR-113-operator-proposed.md "s.replace('status: proposed\n', 'status: accepted\nconsidered: [{operator: developer, said: \"yes\", via: PR 13}]\n')"
commit_all accept
edit docs/architecture/system/ADR-113-operator-proposed.md "s.replace('basis:\n', 'basis:\n  - evidence: a review fix\n', 1)"
commit_all "review fix"
(cd "$WORK/repo" && git switch -q - && git merge -q --no-ff -m "merge review" review)
capture v1-frozen-merged-branch lint docs/architecture/system/ADR-113-operator-proposed.md
edit docs/architecture/system/ADR-113-operator-proposed.md "s.replace('  - evidence: a review fix\n', '')"
capture v1-frozen-merged-branch-edited lint docs/architecture/system/ADR-113-operator-proposed.md

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
capture v1-accept-then-lint     lint --check docs/architecture/system/ADR-114-open-concern.md

fresh v1
capture v1-reject               reject 107 --reason "Superseded by the batching work before it landed"
keep v1-reject-file.md docs/architecture/system/ADR-107-ingest-notes.md
capture v1-abandon-no-reason    abandon 108 --reason "  "
capture v1-reject-then-lint     lint --check docs/architecture/system/ADR-107-ingest-notes.md
capture v1-abandon-no-flag      abandon 108

# A record another record rests on: rejecting it is refused, naming the
# dependent, and the dependent cannot be accepted while its precedent is
# still proposed.
fresh v1
(cd "$WORK/repo" && printf -- '---\ncontract: adr/v1\nkind: decision\nverb: change\ncapability: ingest\nsupersedes: [ADR-103]\nstatus: proposed\ndate: 2025-05-16\ndeciders: [developer]\nagent: {name: Claude, model: m}\nbasis:\n  - precedent: ADR-114\n---\n\n# ADR-116: Rests on ADR-114\n\n## Summary\n\n- **Probes:** *Confident:* a. *Not confident:* b.\n- **Inversion:** c.\n' > docs/architecture/system/ADR-116-rests-on-114.md)
capture v1-reject-precedent     reject 114 --reason "Not needed"
capture v1-accept-pending-precedent accept 116

# Line endings survive a status rewrite.
fresh v1
edit docs/architecture/system/ADR-114-open-concern.md "s.replace(chr(10), chr(13)+chr(10))"
capture v1-accept-crlf          accept 114
(cd "$WORK/repo" && python3 -c "import sys; d=open(sys.argv[1],'rb').read(); print('crlf kept' if b'\\r\\n' in d and d.count(b'\\n')==d.count(b'\\r\\n') else 'crlf lost', '|', [l for l in d.split(b'\\r\\n') if l.startswith(b'status:')])" docs/architecture/system/ADR-114-open-concern.md) > "$ACTUAL/v1-accept-crlf-file.txt"

# --- adr import (ADR-306) ---------------------------------------------------------

# keep_sheets PREFIX — keep every sheet scan wrote, one golden each
keep_sheets() {
  local sheet
  for sheet in "$WORK/repo/docs/architecture/.import/"ADR-*.yaml; do
    [[ -e "$sheet" ]] && keep "$1-$(basename "$sheet")" "docs/architecture/.import/$(basename "$sheet")"
  done
}

# Scan the v0 corpus: one sheet per record, the archive left out, a record
# without frontmatter skipped. The sources stay untouched and the sheet
# directory ignores itself, so git sees no change.
fresh corpus
capture import-scan-corpus import scan docs/architecture
worktree import-scan-corpus-status.txt
keep import-scan-corpus-gitignore docs/architecture/.import/.gitignore
keep_sheets import-scan-corpus

# In the v1 fixture: a v1 record scanned as itself, a v0 record, a v0 record
# with a preamble above its H1, an unmapped key and no Summary, and a
# Deprecated v0 record with no successor.
import_fresh() {
  fresh v1
  (cd "$WORK/repo" && printf -- '---\nstatus: Accepted\ndate: 2025-04-01\ndeciders: [developer]\nrevised: 2025-04-02\n---\n\n> Moved here from the ops wiki.\n\n# ADR-115: Nightly ingest window\n\n## Context\n\nIngest ran nightly.\n\n## Decision\n\nIngest runs in a nightly window.\n' > docs/architecture/system/ADR-115-nightly-ingest-window.md \
    && printf -- '---\nstatus: Deprecated\ndate: 2025-04-05\ndeciders: [developer]\n---\n\n# ADR-116: Hourly ingest\n\n## Context\n\nIngest ran hourly.\n' > docs/architecture/system/ADR-116-hourly-ingest.md)
  commit_all "v0 records to import"
}
IMPORTED="docs/architecture/system/ADR-101-ingest.md docs/architecture/system/ADR-110-old-v0-record.md docs/architecture/system/ADR-115-nightly-ingest-window.md docs/architecture/system/ADR-116-hourly-ingest.md"
import_fresh
# shellcheck disable=SC2086
capture import-scan-v1 import scan $IMPORTED
keep_sheets import-scan-v1
# Unedited sheets: the v1 record applies unchanged, the v0 ones are skipped.
capture import-apply-open import apply
worktree import-apply-open-status.txt
# --partial writes them anyway, and lint names what is missing. The
# preamble and the unmapped key are carried; the Deprecated record's note is
# one lint cannot find again, so --partial does not write past it.
capture import-apply-partial import apply --partial
worktree import-apply-partial-status.txt
keep import-apply-partial-110.md docs/architecture/system/ADR-110-old-v0-record.md
keep import-apply-partial-115.md docs/architecture/system/ADR-115-nightly-ingest-window.md
# An imported record with no Summary warns rather than fails (ADR-306 §4).
capture import-lint-no-summary lint docs/architecture/system/ADR-115-nightly-ingest-window.md
# Committed with empty fields, then completed: filling what the import left
# empty and adding an opening Summary is not an edit of a frozen decision.
commit_all "partial import"
edit docs/architecture/system/ADR-115-nightly-ingest-window.md "s.replace('verb: ~', 'verb: add').replace('capability: ~', 'capability: ingest').replace('basis: []', 'basis:\n  - evidence: ingest logs').replace('name: ~', 'name: Claude').replace('# ADR-115: Nightly ingest window\n', '# ADR-115: Nightly ingest window\n\n## Summary\n\n- **Probes:** *Confident:* a. *Not confident:* b.\n- **Inversion:** c.\n')"
capture import-lint-completed lint docs/architecture/system/ADR-115-nightly-ingest-window.md
# Fill once: after the fill is committed, changing it is an edit.
commit_all "complete the import"
edit docs/architecture/system/ADR-115-nightly-ingest-window.md "s.replace('capability: ingest', 'capability: search').replace('name: Claude', 'name: Other')"
capture import-lint-refilled lint docs/architecture/system/ADR-115-nightly-ingest-window.md
# Changing a field the import did fill is an edit.
edit docs/architecture/system/ADR-115-nightly-ingest-window.md "s.replace('capability: search', 'capability: ingest').replace('name: Other', 'name: Claude').replace('date: 2025-04-01', 'date: 2025-04-03')"
capture import-lint-completed-edited lint docs/architecture/system/ADR-115-nightly-ingest-window.md
# A Summary added anywhere but first is still a body edit.
edit docs/architecture/system/ADR-115-nightly-ingest-window.md "s.replace('date: 2025-04-03', 'date: 2025-04-01').replace('## Decision\n', '## Summary\n\nLate.\n\n## Decision\n')"
capture import-lint-summary-not-opening lint docs/architecture/system/ADR-115-nightly-ingest-window.md

# A completed sheet applies without --partial.
import_fresh
capture import-scan-complete import scan docs/architecture/system/ADR-110-old-v0-record.md
edit docs/architecture/.import/ADR-110.yaml "(lambda t: t[:t.index('todo:')] + 'todo: []\n' + t[t.index('candidates:'):])(s.replace('verb: ~', 'verb: add').replace('capability: ~', 'capability: ingest').replace('basis: []', 'basis:\n    - evidence: migrated from v0').replace('name: ~', 'name: Claude'))"
capture import-apply-complete import apply docs/architecture/.import/ADR-110.yaml
keep import-apply-complete-file.md docs/architecture/system/ADR-110-old-v0-record.md
capture import-apply-complete-lint lint docs/architecture/system/ADR-110-old-v0-record.md

# A source edited after the scan is refused, and the sheet is kept.
import_fresh
capture import-scan-changed import scan docs/architecture/system/ADR-110-old-v0-record.md
edit docs/architecture/system/ADR-110-old-v0-record.md "s.replace('They follow.', 'They follow, mostly.')"
capture import-apply-changed import apply --partial
worktree import-apply-changed-status.txt
# A rescan keeps a sheet that differs from a fresh scan; --force replaces it.
capture import-rescan-kept import scan docs/architecture/system/ADR-110-old-v0-record.md
capture import-rescan-forced import scan --force docs/architecture/system/ADR-110-old-v0-record.md
# The source now has uncommitted changes: refused unless --force.
capture import-apply-uncommitted import apply --partial docs/architecture/.import/ADR-110.yaml
capture import-apply-uncommitted-forced import apply --partial --force docs/architecture/.import/ADR-110.yaml

# Sheets apply cannot use: each is refused and the batch goes on.
# Among them: in-tree records renumbered or moved to another domain, and
# the Deprecated ADR-116, which --partial does not write past.
import_fresh
capture import-scan-bad import scan $IMPORTED
edit docs/architecture/.import/ADR-115.yaml "s.replace('  number: 115', '  number: 117')"
(cd "$WORK/repo/docs/architecture/.import" \
  && sed 's/^  number: 110$/  number: 101.10/' ADR-110.yaml > float.yaml \
  && sed 's#^  number: 110$#  number: "110.1/../../x"#' ADR-110.yaml > escape.yaml \
  && sed 's/^  domain: system$/  domain: legacy/' ADR-110.yaml > domain.yaml \
  && sed 's/^  domain: system$/  domain: storage/' ADR-110.yaml > nodomain.yaml \
  && python3 -c "s=open('ADR-110.yaml').read(); open('listbody.yaml','w').write(s[:s.index('body: |')] + 'body: [not, text]\n')" \
  && python3 -c "s=open('ADR-110.yaml').read(); open('nobody.yaml','w').write(s[:s.index('body: |')] + 'body: \"\"\n')" \
  && printf 'sheet: adr-import/v1\n  bad: [\n' > broken.yaml)
capture import-apply-bad import apply --partial docs/architecture/.import/float.yaml docs/architecture/.import/escape.yaml docs/architecture/.import/listbody.yaml docs/architecture/.import/nobody.yaml docs/architecture/.import/broken.yaml docs/architecture/.import/domain.yaml docs/architecture/.import/nodomain.yaml docs/architecture/.import/ADR-115.yaml docs/architecture/.import/ADR-116.yaml docs/architecture/.import/ADR-101.yaml
worktree import-apply-bad-status.txt

# A foreign source numbered outside its target domain's range is refused.
fresh v1
mkdir -p "$WORK/repo/foreign"
printf -- '---\nstatus: Accepted\ndate: 2025-04-01\ndeciders: [developer]\n---\n\n# ADR-042: Foreign record\n\n## Context\n\nFrom elsewhere.\n' > "$WORK/repo/foreign/ADR-042-foreign-record.md"
capture import-scan-foreign import scan foreign/ADR-042-foreign-record.md
edit docs/architecture/.import/ADR-042.yaml "s.replace('  domain: legacy', '  domain: system')"
capture import-apply-out-of-range import apply --partial docs/architecture/.import/ADR-042.yaml
edit docs/architecture/.import/ADR-042.yaml "s.replace('  number: 42', '  number: 150')"
capture import-apply-foreign import apply --partial docs/architecture/.import/ADR-042.yaml
worktree import-apply-foreign-status.txt

# A byte order mark is named as one.
fresh v1
printf '\357\273\277---\nstatus: Accepted\ndate: 2025-04-01\ndeciders: [developer]\n---\n\n# ADR-117: With a BOM\n' > "$WORK/repo/docs/architecture/system/ADR-117-with-a-bom.md"
capture import-scan-bom import scan docs/architecture/system/ADR-117-with-a-bom.md

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
