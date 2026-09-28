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
#   v1-defects/  one broken record per v1 rule, a malformed adr.yaml, and
#             keys adr/v1 no longer reads
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
TOOL_VERSION="$(sed -nE 's/^TOOL_VERSION = "([^"]+)"$/\1/p' "$ADR_TOOL" | head -1)"
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
# The tool's version, where `adr contract` prints it in parentheses, becomes
# <VERSION>, so a version bump changes only version.out, which prints it bare.
normalize() {
  sed -e "s#$ADR_TOOL#<ADR_TOOL>#g" -e "s#$WORK/repo#<ROOT>#g" -e "s#$WORK#<WORK>#g" \
      -e "s#$TODAY#<TODAY>#g" -e "s#$(date +%Y-%m-%d)#<TODAY>#g" \
      -e "s#(adr-tool $TOOL_VERSION)#(adr-tool <VERSION>)#g"
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
# No command prints argparse's help, which varies across Python versions, then
# the module docstring's usage block, which does not. Keep the usage block.
capture usage
sed -n '/^Usage:$/,$p' "$ACTUAL/usage.out" > "$ACTUAL/usage.tmp" && mv "$ACTUAL/usage.tmp" "$ACTUAL/usage.out"
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
# accept checks the record's own rules and refuses on an error (ADR-311 §3).
capture v1-accept-own-errors accept 117
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

# Observables (ADR-307): loose shapes pass, malformed ones fail.
fresh v1
edit docs/architecture/system/ADR-101-ingest.md "s.replace('date: 2025-05-02\n', 'date: 2025-05-02\nobservable:\n  - ingest of a 10 MB file finishes under a second\n  - see: the queue drains\n    run: make drain-check\n  - url: https://example.com/dashboard\n', 1)"
capture v1-observable-added lint docs/architecture/system/ADR-101-ingest.md
edit docs/architecture/system/ADR-101-ingest.md "s.replace('  - url: https://example.com/dashboard\n', '  - \"\"\n  - {}\n  - 42\n', 1)"
capture v1-observable-malformed lint docs/architecture/system/ADR-101-ingest.md
edit docs/architecture/system/ADR-101-ingest.md "__import__('re').sub(r'observable:\n(?:  .*\n)+', 'observable: soon\n', s, count=1)"
capture v1-observable-not-list lint docs/architecture/system/ADR-101-ingest.md
edit docs/architecture/system/ADR-101-ingest.md "s.replace('observable: soon\n', 'observable: []\n', 1)"
capture v1-observable-empty lint docs/architecture/system/ADR-101-ingest.md
# A change may list the capabilities it alters (ADR-308).
fresh v1
(cd "$WORK/repo" && printf -- '---\ncontract: adr/v1\nkind: decision\nverb: change\ncapability: [ingest, search]\nsupersedes: [ADR-103]\nstatus: proposed\ndate: 2025-05-21\ndeciders: [developer]\nagent: {name: Claude, model: m}\nbasis:\n  - evidence: both paths share one queue\n---\n\n# ADR-116: Shared queue for ingest and search\n\n## Summary\n\n- **Probes:** *Confident:* a. *Not confident:* b.\n- **Inversion:** c.\n' > docs/architecture/system/ADR-116-shared-queue.md)
capture v1-change-list lint docs/architecture/system/ADR-116-shared-queue.md
# Past three capabilities, a listed change draws a warning (ADR-308 §3).
edit docs/architecture/adr.yaml "s.replace('  search: Query over the store\n', '  search: Query over the store\n  export: Export from the store\n  audit: Audit trail\n', 1)"
edit docs/architecture/system/ADR-116-shared-queue.md "s.replace('capability: [ingest, search]', 'capability: [ingest, search, export, audit]')"
capture v1-change-list-long lint docs/architecture/system/ADR-116-shared-queue.md

# A heading that only starts with "Summary" is another section, not the Summary.
fresh v1
edit docs/architecture/system/ADR-108-ingest-over-v0.md "s.replace('## Summary\n', '## Summary Nudge\n', 1)"
capture v1-summary-prefix-heading lint docs/architecture/system/ADR-108-ingest-over-v0.md
edit docs/architecture/system/ADR-108-ingest-over-v0.md "s.replace('## Summary Nudge\n', '## Summary: the short version\n', 1)"
capture v1-summary-colon-heading lint docs/architecture/system/ADR-108-ingest-over-v0.md

# An evidence kind (ADR-309): a basis that cites a record as evidence must
# reach an evidence or spec record.
fresh v1
edit docs/architecture/adr.yaml "s.replace('basis: [decision, spec] }', 'basis: [decision, spec, evidence] }').replace('    edges: { supersedes: spec, decided_by: decision }\n', '    edges: { supersedes: spec, decided_by: decision }\n  evidence:\n    verb: forbidden\n    requires: [capability]\n    edges: { supersedes: evidence }\n', 1)"
(cd "$WORK/repo" && printf -- '---\ncontract: adr/v1\nkind: evidence\ncapability: ingest\nstatus: accepted\ndate: 2025-05-20\ndeciders: [developer]\n---\n\n# ADR-117: Ingest throughput survey\n\nMeasured 40 MB/s on the reference host.\n' > docs/architecture/system/ADR-117-ingest-throughput-survey.md)
(cd "$WORK/repo" && printf -- '---\ncontract: adr/v1\nkind: decision\nverb: change\ncapability: ingest\nsupersedes: [ADR-103]\nstatus: proposed\ndate: 2025-05-21\ndeciders: [developer]\nagent: {name: Claude, model: m}\nbasis:\n  - evidence: ADR-117\n  - evidence: ADR-101\n  - evidence: ADR-999\n---\n\n# ADR-118: Raise the ingest batch size\n\n## Summary\n\n- **Probes:** *Confident:* a. *Not confident:* b.\n- **Inversion:** c.\n' > docs/architecture/system/ADR-118-raise-the-ingest-batch-size.md)
capture v1-evidence-kind lint docs/architecture/system/ADR-117-ingest-throughput-survey.md docs/architecture/system/ADR-118-raise-the-ingest-batch-size.md

fresh v1
capture v1-cite        cite
capture v1-cite-no-inventory cite --no-inventory

# Lifecycle commands (ADR-304 §2, §11, §12)
fresh v1
capture v1-accept-not-proposed  accept 102
capture v1-accept-v0            accept 110
capture v1-accept-dry-run       accept 106 --dry-run
worktree v1-accept-dry-run-status.txt
# --whatif is an alias of --dry-run: the same output, nothing written.
capture v1-accept-whatif        accept 106 --whatif
worktree v1-accept-whatif-status.txt
cmp -s "$ACTUAL/v1-accept-dry-run.out" "$ACTUAL/v1-accept-whatif.out" \
  && echo "same as --dry-run" > "$ACTUAL/v1-accept-whatif-same.txt" \
  || echo "differs from --dry-run" > "$ACTUAL/v1-accept-whatif-same.txt"
capture v1-accept-concern       accept 114
keep v1-accept-concern-file.md docs/architecture/system/ADR-114-open-concern.md
capture v1-accept-then-lint     lint --check docs/architecture/system/ADR-114-open-concern.md
# An operator-started record is accepted without a considered entry.
capture v1-accept-no-considered accept 113

fresh v1
capture v1-reject               reject 107 --reason "Superseded by the batching work before it landed"
keep v1-reject-file.md docs/architecture/system/ADR-107-ingest-notes.md
capture v1-abandon-no-reason    abandon 108 --reason "  "
capture v1-reject-then-lint     lint --check docs/architecture/system/ADR-107-ingest-notes.md
capture v1-abandon-no-flag      abandon 108

# A record another record rests on: rejecting it and accepting the dependent
# each check only the record acted on, not the corpus (ADR-311 §3).
fresh v1
(cd "$WORK/repo" && printf -- '---\ncontract: adr/v1\nkind: decision\nverb: change\ncapability: ingest\nsupersedes: [ADR-103]\nstatus: proposed\ndate: 2025-05-16\ndeciders: [developer]\nagent: {name: Claude, model: m}\nbasis:\n  - precedent: ADR-114\n---\n\n# ADR-116: Rests on ADR-114\n\n## Summary\n\n- **Probes:** *Confident:* a. *Not confident:* b.\n- **Inversion:** c.\n' > docs/architecture/system/ADR-116-rests-on-114.md)
capture v1-reject-precedent     reject 114 --reason "Not needed"
capture v1-accept-dependent     accept 116

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
# A directory of records that scan cannot read: each file it passes over is
# named, with the reason, and counted as skipped.
mkdir -p "$WORK/repo/notes/decisions"
printf '# 1. Use Postgres\n\nStatus: Accepted\n' > "$WORK/repo/notes/decisions/0001-use-postgres.md"
printf '# ADR-2: Inline\n\nStatus: Accepted\n' > "$WORK/repo/notes/decisions/ADR-002-inline.md"
printf 'Decisions live here.\n' > "$WORK/repo/notes/decisions/README.md"
printf -- '---\nstatus: Accepted\ndate: 2026-01-15\ndeciders: [a]\nrelated: []\n---\n\n# ADR-003: No slug\n\n## Context\nx\n' > "$WORK/repo/notes/decisions/ADR-003.md"
capture import-scan-flat import scan notes/decisions
# A broad folder: each Markdown file passed over is named, a hidden folder
# is named and not entered, other files are counted by extension, and the
# tool's own files are left out only at the top of the folder scanned.
mkdir -p "$WORK/repo/notes/.drafts" "$WORK/repo/notes/archive" "$WORK/repo/notes/sub" "$WORK/repo/notes/img"
printf 'x\n' > "$WORK/repo/notes/.drafts/ADR-003-draft.md"
printf 'x\n' > "$WORK/repo/notes/.notes.md"
printf 'x\n' > "$WORK/repo/notes/INDEX.md"
printf 'x\n' > "$WORK/repo/notes/sub/INDEX.md"
printf 'k: v\n' > "$WORK/repo/notes/sub/adr.yaml"
printf 'x\n' > "$WORK/repo/notes/archive/README.md"
printf 'x\n' > "$WORK/repo/notes/archive/old.txt"
printf 'x\n' > "$WORK/repo/notes/Makefile"
for i in 1 2 3; do printf 'png' > "$WORK/repo/notes/img/p$i.png"; done
capture import-scan-broad import scan notes
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
# Committed with empty fields, then completed: the filled record lints clean.
commit_all "partial import"
edit docs/architecture/system/ADR-115-nightly-ingest-window.md "s.replace('verb: ~', 'verb: add').replace('capability: ~', 'capability: ingest').replace('basis: []', 'basis:\n  - evidence: ingest logs').replace('name: ~', 'name: Claude').replace('# ADR-115: Nightly ingest window\n', '# ADR-115: Nightly ingest window\n\n## Summary\n\n- **Probes:** *Confident:* a. *Not confident:* b.\n- **Inversion:** c.\n')"
capture import-lint-completed lint docs/architecture/system/ADR-115-nightly-ingest-window.md

# A completed sheet applies without --partial.
import_fresh
capture import-scan-complete import scan docs/architecture/system/ADR-110-old-v0-record.md
edit docs/architecture/.import/ADR-110.yaml "(lambda t: t[:t.index('todo:')] + 'todo: []\n' + t[t.index('candidates:'):])(s.replace('verb: ~', 'verb: add').replace('capability: ~', 'capability: ingest').replace('basis: []', 'basis:\n    - evidence: migrated from v0').replace('name: ~', 'name: Claude'))"
capture import-apply-complete import apply docs/architecture/.import/ADR-110.yaml
keep import-apply-complete-file.md docs/architecture/system/ADR-110-old-v0-record.md
capture import-apply-complete-lint lint docs/architecture/system/ADR-110-old-v0-record.md
# An observable on the sheet is written in its place in the key order.
import_fresh
capture import-scan-observable import scan docs/architecture/system/ADR-110-old-v0-record.md
edit docs/architecture/.import/ADR-110.yaml "(lambda t: t[:t.index('todo:')] + 'todo: []\n' + t[t.index('candidates:'):])(s.replace('verb: ~', 'verb: add').replace('capability: ~', 'capability: ingest').replace('basis: []', 'basis:\n    - evidence: migrated from v0').replace('name: ~', 'name: Claude').replace('  status: accepted', '  observable:\n    - the record lints clean\n  status: accepted', 1))"
capture import-apply-observable import apply docs/architecture/.import/ADR-110.yaml
keep import-apply-observable-file.md docs/architecture/system/ADR-110-old-v0-record.md

# A dry run lints what it would write inside the corpus, prints each issue,
# then restores every file and keeps every sheet.
import_fresh
capture import-scan-dryrun import scan docs/architecture/system/ADR-110-old-v0-record.md
edit docs/architecture/.import/ADR-110.yaml "(lambda t: t[:t.index('todo:')] + 'todo: []\n' + t[t.index('candidates:'):])(s.replace('verb: ~', 'verb: change').replace('capability: ~', 'capability: ingest').replace('basis: []', 'basis:\n    - evidence: migrated from v0').replace('name: ~', 'name: Claude'))"
capture import-apply-dryrun import apply --dry-run
worktree import-apply-dryrun-status.txt

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
edit docs/architecture/.import/ADR-115.yaml "s.replace(\"  number: '115'\", '  number: 117')"
(cd "$WORK/repo/docs/architecture/.import" \
  && sed "s/^  number: '110'$/  number: 101.10/" ADR-110.yaml > float.yaml \
  && sed "s#^  number: '110'\$#  number: '110.1/../../x'#" ADR-110.yaml > escape.yaml \
  && sed "s/^  number: '110'$/  number: 0156/" ADR-110.yaml > octal.yaml \
  && sed "s/^  number: '110'$/  number: 0x6E/" ADR-110.yaml > hex.yaml \
  && sed 's/^  domain: system$/  domain: legacy/' ADR-110.yaml > domain.yaml \
  && sed 's/^  domain: system$/  domain: storage/' ADR-110.yaml > nodomain.yaml \
  && python3 -c "s=open('ADR-110.yaml').read(); open('listbody.yaml','w').write(s[:s.index('body: |')] + 'body: [not, text]\n')" \
  && python3 -c "s=open('ADR-110.yaml').read(); open('nobody.yaml','w').write(s[:s.index('body: |')] + 'body: \"\"\n')" \
  && printf 'sheet: adr-import/v1\n  bad: [\n' > broken.yaml)
capture import-apply-bad import apply --partial docs/architecture/.import/float.yaml docs/architecture/.import/escape.yaml docs/architecture/.import/octal.yaml docs/architecture/.import/hex.yaml docs/architecture/.import/listbody.yaml docs/architecture/.import/nobody.yaml docs/architecture/.import/broken.yaml docs/architecture/.import/domain.yaml docs/architecture/.import/nodomain.yaml docs/architecture/.import/ADR-115.yaml docs/architecture/.import/ADR-116.yaml docs/architecture/.import/ADR-101.yaml
worktree import-apply-bad-status.txt

# A source outside the repo, numbered outside its target domain's range, is
# refused; renumbered into the range it is written, named by its file name.
fresh v1
rm -rf "$WORK/outside" && mkdir -p "$WORK/outside"
printf -- '---\nstatus: Accepted\ndate: 2025-04-01\ndeciders: [developer]\n---\n\n# ADR-042: Foreign record\n\n## Context\n\nFrom elsewhere.\n' > "$WORK/outside/ADR-042-foreign-record.md"
capture import-scan-foreign import scan "$WORK/outside/ADR-042-foreign-record.md"
edit docs/architecture/.import/ADR-042.yaml "s.replace('  domain: legacy', '  domain: system')"
capture import-apply-out-of-range import apply --partial docs/architecture/.import/ADR-042.yaml
edit docs/architecture/.import/ADR-042.yaml "s.replace(\"  number: '42'\", \"  number: '150'\")"
# A dry run that would write a new file removes it again.
capture import-apply-foreign-dryrun import apply --partial --dry-run docs/architecture/.import/ADR-042.yaml
worktree import-apply-foreign-dryrun-status.txt
capture import-apply-foreign import apply --partial docs/architecture/.import/ADR-042.yaml
worktree import-apply-foreign-status.txt
keep import-apply-foreign-file.md docs/architecture/system/ADR-150-foreign-record.md

# A status with no v1 mapping, or none at all, holds the sheet back even
# under --partial: lint could not tell what it was once written.
fresh v1
(cd "$WORK/repo" && printf -- '---\nstatus: WIP pending review\ndate: 2025-04-01\ndeciders: [developer]\n---\n\n# ADR-117: Work in progress\n\n## Context\n\nText.\n' > docs/architecture/system/ADR-117-work-in-progress.md \
  && printf -- '---\ndate: 2025-04-01\ndeciders: [developer]\n---\n\n# ADR-118: No status\n\n## Context\n\nText.\n' > docs/architecture/system/ADR-118-no-status.md)
commit_all "records with odd statuses"
capture import-scan-status import scan docs/architecture/system/ADR-117-work-in-progress.md docs/architecture/system/ADR-118-no-status.md
keep_sheets import-scan-status
capture import-apply-status import apply --partial

# A byte order mark is named as one.
fresh v1
printf '\357\273\277---\nstatus: Accepted\ndate: 2025-04-01\ndeciders: [developer]\n---\n\n# ADR-117: With a BOM\n' > "$WORK/repo/docs/architecture/system/ADR-117-with-a-bom.md"
capture import-scan-bom import scan docs/architecture/system/ADR-117-with-a-bom.md

# --- adr domain (ADR-306 §6) ------------------------------------------------------
# A record's number is its identity and never changes. Under adr/v1 its folder
# decides its domain; a domain's range only allocates new numbers.

# add: the entry is written into adr.yaml's domains block as text, so the
# file's comments and layout stay. Overlapping ranges, a taken folder or name
# and a backwards range are refused.
fresh v1
capture domain-add domain add docs --range 300-399 --folder documentation --label Documentation --description "Guides and references"
keep domain-add-config.yaml docs/architecture/adr.yaml
capture domain-add-overlap domain add ops --range 150-250 --folder operations
capture domain-add-refused domain add docs --range 400-300 --folder system
worktree domain-add-refused-status.txt
fresh corpus
capture domain-add-v0 domain add api --range 400-499 --folder api --label "API surface"
keep domain-add-v0-config.yaml docs/architecture/adr.yaml
capture domain-add-legacy-overlap domain add early --range 50-60 --folder early

# rename: the domain key, its folder moved with git mv, every path into the
# folder rewritten, and a catalog page's domain: key. Links between records
# keep their shape.
rename_fresh() {
  fresh v1
  mkdir -p "$WORK/repo/docs/guide"
  printf -- '---\ndomain: system\n---\n\n# Hooks guide\n\nSee [no network](../architecture/system/ADR-104-no-network-in-hooks.md) and ADR-104.\nRecords live in docs/architecture/system/ and [the folder](../architecture/system/).\n' > "$WORK/repo/docs/guide/hooks.md"
  edit docs/architecture/system/ADR-109-precedent-chain.md "s + '\n## 3. Notes\n\nSee [the constraint](../system/ADR-104-no-network-in-hooks.md).\n'"
  edit src/search.py "s + '# ADR-104 hooks stay offline: docs/architecture/system/ADR-104-no-network-in-hooks.md\n'"
  commit_all "references"
}
rename_fresh
capture domain-rename-dry domain rename system platform --dry-run
worktree domain-rename-dry-status.txt
capture domain-rename domain rename system platform
worktree domain-rename-status.txt
keep domain-rename-config.yaml docs/architecture/adr.yaml
keep domain-rename-guide.md docs/guide/hooks.md
keep domain-rename-109.md docs/architecture/platform/ADR-109-precedent-chain.md
keep domain-rename-search.py src/search.py
capture domain-rename-lint lint
capture domain-rename-list list --group
rename_fresh
capture domain-rename-unknown domain rename nosuch other
capture domain-rename-refused domain rename system system --folder archive
capture domain-rename-same domain rename system system
capture domain-rename-folder domain rename system core --folder kernel
worktree domain-rename-folder-status.txt

# move: the file goes to the target domain's folder with its number and slug;
# path references are rewritten and ADR-N citations are not. ADR-104 then
# sits outside the docs range, which is valid under adr/v1.
move_fresh() {
  fresh v1
  (cd "$WORK/repo" && "$ADR_TOOL" domain add docs --range 300-399 --folder documentation --description "Guides" > /dev/null \
    && "$ADR_TOOL" index -y > /dev/null)
  mkdir -p "$WORK/repo/docs/guide"
  printf -- '---\ndomain: system\n---\n\n# Hooks guide\n\nSee [no network](../architecture/system/ADR-104-no-network-in-hooks.md) and ADR-104#1.\n' > "$WORK/repo/docs/guide/hooks.md"
  edit docs/architecture/system/ADR-109-precedent-chain.md "s + '\n## 3. Notes\n\nADR-104 applies; see [the constraint](ADR-104-no-network-in-hooks.md) and [ingest](./ADR-101-ingest.md).\n'"
  edit docs/architecture/system/ADR-111-cut-search.md "s + '\nSee [ADR-104](ADR-104-no-network-in-hooks.md).\n'"
  # The record that moves links to a sibling that stays, by bare file name.
  edit docs/architecture/system/ADR-104-no-network-in-hooks.md "s + '\nSee [the chain](ADR-109-precedent-chain.md).\n'"
  edit src/search.py "s + '# ADR-104 hooks stay offline: docs/architecture/system/ADR-104-no-network-in-hooks.md\n# ADR-1040 and ADR-104.1 are other numbers.\n'"
  commit_all "references"
}
move_fresh
capture domain-move-dry domain move 104 docs --dry-run
worktree domain-move-dry-status.txt
capture domain-move domain move ADR-104 docs
worktree domain-move-status.txt
keep domain-move-104.md docs/architecture/documentation/ADR-104-no-network-in-hooks.md
keep domain-move-109.md docs/architecture/system/ADR-109-precedent-chain.md
keep domain-move-guide.md docs/guide/hooks.md
keep domain-move-search.py src/search.py
keep domain-move-index.md docs/architecture/INDEX.md
capture domain-move-lint lint docs/architecture/documentation/ADR-104-no-network-in-hooks.md docs/architecture/system/ADR-109-precedent-chain.md docs/architecture/system/ADR-111-cut-search.md
capture domain-move-list list --group
capture domain-move-cite cite --no-inventory src/search.py docs/guide
capture domain-move-scan import scan docs/architecture/documentation/ADR-104-no-network-in-hooks.md
capture domain-move-again domain move 104 docs
# Numbers stay allocated by range, across every record wherever it sits.
capture domain-move-new-docs new docs "Style guide"
capture domain-move-new-system new system "Queue limits"
capture domain-move-refused domain move 999 nowhere

# A plan applies several moves at once: ADR-104 and ADR-111 move together, so
# ADR-111's link to ADR-104 stays a sibling link.
move_fresh
printf -- '- {record: 104, domain: docs}\n- {record: ADR-111, domain: docs}\n' > "$WORK/plan.yaml"
capture domain-move-plan domain move --plan "$WORK/plan.yaml"
worktree domain-move-plan-status.txt
keep domain-move-plan-111.md docs/architecture/documentation/ADR-111-cut-search.md
keep domain-move-plan-109.md docs/architecture/system/ADR-109-precedent-chain.md
move_fresh
printf -- '- {record: 104, domain: docs, number: 300}\n' > "$WORK/plan.yaml"
capture domain-move-plan-number domain move --plan "$WORK/plan.yaml"
printf -- '- {record: 104, domain: docs}\n- {record: ADR-104, domain: docs}\n' > "$WORK/plan.yaml"
capture domain-move-plan-twice domain move --plan "$WORK/plan.yaml"
worktree domain-move-plan-refused-status.txt

# Under adr/v0 the number decides the domain, so a move is refused.
fresh corpus
capture domain-move-v0 domain move 104 docs

# --- what a relocation rewrites (#603) ------------------------------------------------
# A rewrite reaches a path that resolves to what moved: a relative link, a
# path from the repo root, or a URL into this repository's origin. Another
# repository's URL, prose, and a code constant naming a folder stay. The dry
# run prints each line it would change.
#
# ADR-104's basis evidence and body name ADR-101's path, and the rewrite
# reaches both.
safety_fresh() {
  fresh v1
  (cd "$WORK/repo" && git remote add origin git@github.com:fixture/corpus.git)
  printf 'TEMPLATE_DIR = "architecture/system"\nRECORDS = "docs/architecture/system"\n' > "$WORK/repo/src/app.py"
  printf '# Changes\n\n- Another repo: https://github.com/someone/else/tree/main/docs/architecture/system/ADR-101-ingest.md\n- This repo: https://github.com/fixture/corpus/blob/main/docs/architecture/system/ADR-101-ingest.md\n- The vendor layout uses architecture/system and a system/ADR-101-ingest.md file.\n- See docs/architecture/system/ADR-101-ingest.md.\n' > "$WORK/repo/CHANGELOG.md"
  edit docs/architecture/system/ADR-104-no-network-in-hooks.md "s.replace('  - evidence: fixture measurement\n', '  - evidence: \"the survey at docs/architecture/system/ADR-101-ingest.md\"\n') + '\nSee [ingest](ADR-101-ingest.md), [the guide](https://example.com/v1/guide), [the notes](../api/README.md), and/or the spec.\n'"
  (cd "$WORK/repo" && "$ADR_TOOL" domain add docs --range 300-399 --folder documentation --description "Guides" > /dev/null)
  (cd "$WORK/repo" && git add -A && git commit -q --amend -m fixture)
}
safety_fresh
capture relocate-move-dry domain move 101 docs --dry-run
capture relocate-rename-dry domain rename system platform --dry-run
capture relocate-rename domain rename system platform
keep relocate-rename-changelog.md CHANGELOG.md
keep relocate-rename-app.py src/app.py
keep relocate-rename-104.md docs/architecture/platform/ADR-104-no-network-in-hooks.md
capture relocate-rename-lint lint

# A move of a record that another record cites by path.
safety_fresh
capture relocate-move domain move 101 docs
keep relocate-move-104.md docs/architecture/system/ADR-104-no-network-in-hooks.md
capture relocate-move-lint lint docs/architecture/system/ADR-104-no-network-in-hooks.md
S="docs/architecture/system"

# ADR-104 cites ADR-101 by a sibling link, in its basis and in its body.
# Moving 104, then 101, then 104 again, each committed, leaves the link
# reaching ADR-101 from a third folder.
chain_fresh() {
  fresh v1
  (cd "$WORK/repo" && "$ADR_TOOL" domain add docs --range 300-399 --folder documentation --description Guides > /dev/null \
    && "$ADR_TOOL" domain add ops --range 400-499 --folder operations --description Operations > /dev/null)
}
chain_moves() {
  (cd "$WORK/repo" && "$ADR_TOOL" domain move 104 docs > /dev/null); commit_all "move 104"
  (cd "$WORK/repo" && "$ADR_TOOL" domain move 101 ops > /dev/null); commit_all "move 101"
  (cd "$WORK/repo" && "$ADR_TOOL" domain move 104 ops > /dev/null); commit_all "move 104 again"
}
chain_fresh
edit $S/ADR-104-no-network-in-hooks.md "s.replace('  - evidence: fixture measurement', '  - evidence: \"see [ingest](ADR-101-ingest.md)\"')"
(cd "$WORK/repo" && git add -A && git commit -q --amend -m fixture)
chain_moves
keep relocate-chain-basis-104.md docs/architecture/operations/ADR-104-no-network-in-hooks.md
chain_fresh
edit $S/ADR-104-no-network-in-hooks.md "s + '\nSee [ingest](ADR-101-ingest.md).\n'"
(cd "$WORK/repo" && git add -A && git commit -q --amend -m fixture)
chain_moves
keep relocate-chain-body-104.md docs/architecture/operations/ADR-104-no-network-in-hooks.md

# --- which repository a URL names, and what a rewrite leaves (#603) -------------
# adr.yaml's repository: names this repository, so the rewrite reads the
# same URLs as this repository's with or without an origin remote.
fresh v1
edit docs/architecture/adr.yaml "s + 'repository: github.com/fixture/corpus\n'"
edit $S/ADR-104-no-network-in-hooks.md "s + '\nSee https://github.com/fixture/corpus/blob/main/docs/architecture/system/ADR-101-ingest.md.\n'"
(cd "$WORK/repo" && git add -A && git commit -q --amend -m fixture)
(cd "$WORK/repo" && "$ADR_TOOL" domain add docs --range 300-399 --folder documentation --description Guides > /dev/null)
capture relocate-repository-move domain move 101 docs
keep relocate-repository-104.md $S/ADR-104-no-network-in-hooks.md
edit docs/architecture/adr.yaml "s.replace('repository: github.com/fixture/corpus', 'repository: [3]')"
capture relocate-repository-bad lint $S/ADR-104-no-network-in-hooks.md

# A URL at a commit or a tag is a permalink and stays; one at a branch, a
# branch with a slash, or on GitHub's raw host is rewritten. A path written
# with backslashes, or inside a fenced code block (indented in a list too), stays.
fresh v1
(cd "$WORK/repo" && git remote add origin git@github.com:fixture/corpus.git && git tag v1.0 && git branch feature/x)
printf -- '# Notes\n\n- https://github.com/fixture/corpus/blob/0123456789abcdef0123456789abcdef01234567/docs/architecture/system/ADR-101-ingest.md\n- https://github.com/fixture/corpus/blob/abc1234/docs/architecture/system/ADR-101-ingest.md\n- https://github.com/fixture/corpus/blob/ABC1234/docs/architecture/system/ADR-101-ingest.md\n- https://github.com/fixture/corpus/blob/v1.0/docs/architecture/system/ADR-101-ingest.md\n- https://github.com/fixture/corpus/blob/main/docs/architecture/system/ADR-101-ingest.md\n- https://github.com/fixture/corpus/blob/feature/x/docs/architecture/system/ADR-101-ingest.md\n- https://raw.githubusercontent.com/fixture/corpus/main/docs/architecture/system/ADR-101-ingest.md\n- docs\\architecture\\system\\ADR-101-ingest.md\n\n```sh\ngit mv docs/architecture/system/ADR-101-ingest.md docs/architecture/documentation/ADR-101-ingest.md\n```\n\n1. Step one\n\n    ```sh\n    cat docs/architecture/system/ADR-101-ingest.md\n    ```\n\nSee docs/architecture/system/ADR-101-ingest.md.\n' > "$WORK/repo/NOTES.md"
edit $S/ADR-109-precedent-chain.md "s + '\nOn Windows: docs\\\\architecture\\\\system\\\\ADR-101-ingest.md\n'"
commit_all notes
(cd "$WORK/repo" && "$ADR_TOOL" domain add docs --range 300-399 --folder documentation --description Guides > /dev/null)
capture relocate-refs-dry domain move 101 docs --dry-run
capture relocate-refs domain move 101 docs
keep relocate-refs-notes.md NOTES.md
keep relocate-refs-109.md $S/ADR-109-precedent-chain.md

# --- record edits: consider, set, supersede, enact ----------------------------------
#
# Each edit keeps the file it wrote, so the goldens show that only the touched
# field's lines changed.

S="docs/architecture/system"
# named_probes — an accepted record whose Summary names its probes
named_probes() {
  (cd "$WORK/repo" && printf -- '---\ncontract: adr/v1\nkind: decision\nverb: add\ncapability: adr\nstatus: accepted\ndate: 2025-05-20\ndeciders: [developer, agent]\nagent: {name: Claude, model: fixture-model}\nbasis:\n  - evidence: fixture measurement\n---\n\n# ADR-115: Named probes\n\n## Summary\n\n- **Decided:** the decision.\n- **Probes:** *Confident (identity-stable):* a. *Not confident (band-hint):* b.\n- **Inversion:** c.\n\n## 1. Decision\n\nThe decision.\n' > "$S/ADR-115-named-probes.md")
  commit_all "named probes"
}

fresh v1
named_probes
capture record-consider         consider 115 --said '"Fine" (Recommended)' --via "session 2025-05-20, selected from agent-written options" --operator developer --covers band-hint inversion --canary caught
keep record-consider-file.md "$S/ADR-115-named-probes.md"
capture record-consider-append  consider 100 --said "ok, as it stands" --via "PR #2" --operator developer --covers
keep record-consider-append-file.md "$S/ADR-100-adopt-v1.md"
capture record-consider-new     consider 113 --said "add it back" --via call --operator developer --paraphrase
keep record-consider-new-file.md "$S/ADR-113-operator-proposed.md"
worktree record-consider-status.txt
capture record-consider-unknown-probe consider 115 --said ok --via PR --operator developer --covers identity-stable no-such-probe
capture record-consider-no-names      consider 101 --said ok --via PR --operator developer --covers edge-case
capture record-consider-no-said       consider 115 --via PR --operator developer
capture record-consider-no-via        consider 115 --said ok --via "  " --operator developer
capture record-consider-v0            consider 110 --said ok --via PR --operator developer

fresh v1
capture record-set              set 106 "capability=[search, ingest]" "related=[ADR-100, ADR-101]" extends-=ADR-100 date=2025-05-17
keep record-set-file.md "$S/ADR-106-search-change.md"
capture record-set-remove-block set 106 related-=ADR-100
keep record-set-remove-block-file.md "$S/ADR-106-search-change.md"
capture record-set-append-block set 114 "basis+={evidence: a second load test}" "concern+={said: Retries may double, resolve: Count retries}"
keep record-set-append-block-file.md "$S/ADR-114-open-concern.md"
capture record-set-mutable      set 103 superseded_by+=ADR-107
keep record-set-mutable-file.md "$S/ADR-103-ingest-batching.md"
capture record-set-v0           set 110 status=Superseded
keep record-set-v0-file.md "$S/ADR-110-old-v0-record.md"
worktree record-set-status.txt
capture record-set-accepted     set 101 capability=search "related=[ADR-100]"
capture record-set-status-cmd   set 106 status=accepted
capture record-set-status-other set 106 status=proposed
capture record-set-remove-missing set 106 related-=ADR-9
capture record-set-bad-yaml     set 106 "related=[ADR-1"
capture record-set-bad-form     set 106 related
capture record-set-unknown-key  set 106 capabilty=search

fresh v1
capture record-set-dry-run      set 106 verb=add --dry-run
worktree record-set-dry-run-status.txt

# Line endings survive a field edit.
fresh v1
edit "$S/ADR-114-open-concern.md" "s.replace(chr(10), chr(13)+chr(10))"
capture record-set-crlf         set 114 "basis+={evidence: a second load test}"
(cd "$WORK/repo" && python3 -c "import sys; d=open(sys.argv[1],'rb').read(); print('crlf kept' if b'\\r\\n' in d and d.count(b'\\n')==d.count(b'\\r\\n') else 'crlf lost')" "$S/ADR-114-open-concern.md") > "$ACTUAL/record-set-crlf-file.txt"

fresh v1
capture record-list-field       list --field capability=ingest
capture record-list-capability  list --capability adr
capture record-list-kind        list --kind spec
capture record-list-verb        list --verb change --field status=proposed
capture record-list-edge        list --field amends=ADR-101
capture record-list-present     list --field supersedes
capture record-list-group-by    list --group-by capability
capture record-list-json        list --json --verb retire
capture record-list-json-group  list --json --group-by verb --field enacted
fresh corpus
capture record-list-group-by-v0 list --group-by status

fresh v1
capture record-supersede        supersede 101 --by 106
keep record-supersede-new-file.md "$S/ADR-106-search-change.md"
keep record-supersede-old-file.md "$S/ADR-101-ingest.md"
worktree record-supersede-status.txt

fresh v1
capture record-supersede-amends supersede 100 --by 107 --amends 2
keep record-supersede-amends-file.md "$S/ADR-107-ingest-notes.md"
worktree record-supersede-amends-status.txt
capture record-supersede-no-section   supersede 100 --by 108 --amends 9
capture record-supersede-kind         supersede 102 --by 106
capture record-supersede-old-proposed supersede 113 --by 106
capture reject-for-supersede          reject 108 --reason "Dropped"
capture record-supersede-new-rejected supersede 101 --by 108

fresh v1
capture record-supersede-accepted supersede 101 --by 104
keep record-supersede-accepted-file.md "$S/ADR-104-no-network-in-hooks.md"

fresh v1
capture record-enact            enact 111 ABC1234
keep record-enact-file.md "$S/ADR-111-cut-search.md"
capture record-enact-again      enact 105 3f9c2a1dead
keep record-enact-again-file.md "$S/ADR-105-retire-legacy-ingest.md"
capture record-enact-wrong-verb enact 101 abc1234
capture record-enact-not-hash   enact 111 HEAD~1
capture set-for-enact           set 111 status=superseded --force
capture record-enact-wrong-status enact 111 abc1234

# --- adr contract (#614) -----------------------------------------------------------

# A v0 config: contract names both contracts; --upgrade appends the contract
# line and the v1 blocks, leaving every existing line as it was; a second
# --upgrade does nothing.
fresh corpus
capture contract-current  contract --current
capture contract-v0       contract
capture contract-upgrade  contract --upgrade
keep contract-upgrade-config.yaml docs/architecture/adr.yaml
(cd "$WORK/repo" && git diff --stat -- docs/architecture/adr.yaml | normalize) > "$ACTUAL/contract-upgrade-diffstat.txt"
commit_all "upgrade"
capture contract-upgrade-noop contract --upgrade
worktree contract-upgrade-noop-status.txt
capture contract-after    contract
# An explicit contract line keeps its quotes and comment; the comments
# around it stay where they were.
fresh corpus
edit docs/architecture/adr.yaml "s.replace('project_name: ADR Fixture\n', 'project_name: ADR Fixture\n\n# Records stay on v0 for now.\ncontract: \"adr/v0\"  # decided in review\n', 1)"
capture contract-upgrade-line-whatif contract --upgrade --whatif
capture contract-upgrade-line contract --upgrade
keep contract-upgrade-line-config.yaml docs/architecture/adr.yaml
# A contract the tool does not know is refused, and adr.yaml is untouched.
fresh corpus
edit docs/architecture/adr.yaml "s.replace('project_name: ADR Fixture\n', 'project_name: ADR Fixture\ncontract: adr/v9\n', 1)"
commit_all "unknown contract"
capture contract-unknown         contract
capture contract-upgrade-unknown contract --upgrade
worktree contract-upgrade-unknown-status.txt
# A v1 config is current.
fresh v1
capture contract-v1 contract
# adr/v1 declared by hand without kinds or capabilities (#589): contract
# names what is missing, --upgrade appends it and leaves the contract line,
# and a second --upgrade does nothing.
fresh v1-empty
capture contract-incomplete         contract
capture contract-incomplete-upgrade contract --upgrade
keep contract-incomplete-config.yaml docs/architecture/adr.yaml
capture contract-incomplete-lint    lint
commit_all "completed"
capture contract-incomplete-noop    contract --upgrade
worktree contract-incomplete-noop-status.txt
# An empty contract line is filled with a space after the colon.
fresh corpus
edit docs/architecture/adr.yaml "s.replace('project_name: ADR Fixture\n', 'project_name: ADR Fixture\ncontract:\n', 1)"
capture contract-upgrade-empty contract --upgrade
keep contract-upgrade-empty-config.yaml docs/architecture/adr.yaml
# --upgrade --whatif prints the contract line and the blocks, seeds included,
# and writes nothing; --dry-run without --upgrade is refused.
fresh corpus
capture contract-upgrade-whatif contract --upgrade --whatif
worktree contract-upgrade-whatif-status.txt
capture contract-whatif-alone   contract --whatif
# Capabilities are seeded from domains (ADR-312); the corpus upgrade above
# seeds system, ops and docs. A domain named process is not seeded twice: the
# template's process wins. A domain with no description is seeded from its
# name.
fresh corpus
edit docs/architecture/adr.yaml "s.replace('  docs:\n', '  process:\n', 1).replace('    description: Runtime, hooks and storage\n', '', 1)"
capture contract-seed-process contract --upgrade
keep contract-seed-process-config.yaml docs/architecture/adr.yaml
# No domains: the template's block, with the placeholder core.
fresh corpus
(cd "$WORK/repo" && printf 'project_name: ADR Fixture\n\n# No domains yet.\ndomains: {}\n' > docs/architecture/adr.yaml)
capture contract-seed-none contract --upgrade
keep contract-seed-none-config.yaml docs/architecture/adr.yaml
commit_all "no domains"
capture contract-seed-none-noop contract --upgrade
worktree contract-seed-none-noop-status.txt
# Records ahead of a v0 config: lint warns on adr.yaml and names the command.
fresh corpus
(cd "$WORK/repo" && printf -- '---\ncontract: adr/v1\nkind: decision\nverb: add\ncapability: core\nstatus: proposed\ndate: 2025-05-21\ndeciders: [developer]\nagent: {name: Claude, model: m}\nbasis:\n  - evidence: a finding\n---\n\n# ADR-107: A v1 record in a v0 repo\n\n## Summary\n\n- **Probes:** *Confident:* a. *Not confident:* b.\n- **Inversion:** c.\n' > docs/architecture/system/ADR-107-a-v1-record.md)
capture contract-behind-lint lint
# import apply into a v0 config says the config is behind and does not edit it.
fresh corpus
capture contract-import-scan  import scan docs/architecture/system/ADR-102-hook-ordering.md
capture contract-import-apply import apply --partial
(cd "$WORK/repo" && git status --porcelain -- docs/architecture/adr.yaml | normalize) > "$ACTUAL/contract-import-apply-config-status.txt"

# --- vocabulary shape --------------------------------------------------------------

# gen_v0 FOLDER FIRST COUNT — COUNT small v0 records numbered from FIRST
gen_v0() {
  local n
  for ((n = $2; n < $2 + $3; n++)); do
    printf -- '---\nstatus: Accepted\ndate: 2025-05-01\ndeciders: [developer]\n---\n\n# ADR-%d: Generated record %d\n\n## Context\n\nA generated record.\n' \
      "$n" "$n" > "$WORK/repo/docs/architecture/$1/ADR-$n-generated.md"
  done
}
# gen_v1 CAPABILITY FIRST COUNT — COUNT small v1 evidence records in system/
gen_v1() {
  local n
  for ((n = $2; n < $2 + $3; n++)); do
    printf -- '---\ncontract: adr/v1\nkind: evidence\ncapability: %s\nstatus: accepted\ndate: 2025-05-01\ndeciders: [developer]\n---\n\n# ADR-%d: Generated finding %d\n\nA generated finding.\n' \
      "$1" "$n" "$n" > "$WORK/repo/docs/architecture/system/ADR-$n-generated.md"
  done
}

# Fat corpus, skinny domains: 47 of 52 records in system. domains and
# contract print a notice; --whatif prints it with the seeds; v0 lint does not.
fresh corpus
gen_v0 system 120 40
commit_all "fat system"
capture shape-thin-domains        domains
capture shape-thin-contract       contract
capture shape-thin-upgrade-whatif contract --upgrade --whatif
capture shape-thin-v0-lint        lint
# Six declared domains, three holding records: the notice counts the six
# seeds, not the three domains in use.
edit docs/architecture/adr.yaml "s.replace('\nstatuses:', '  api:\n    range: [400, 499]\n    name: API\n    description: Endpoints\n    folder: api\n\n  ui:\n    range: [500, 599]\n    name: UI\n    description: Interfaces\n    folder: ui\n\n  ai:\n    range: [600, 699]\n    name: AI\n    description: Models\n    folder: ai\n\nstatuses:', 1)"
capture shape-thin-six-domains-whatif contract --upgrade --whatif
# Balanced: 52 records across three domains; no notice.
fresh corpus
gen_v0 system 120 14
gen_v0 runbooks 220 13
gen_v0 documentation 320 13
commit_all "balanced"
capture shape-balanced-domains domains
capture shape-balanced-contract contract
# adr/v1, 40 records on one capability: lint warns (vocabulary-thin), contract
# prints the notice.
fresh v1
gen_v1 ingest 120 40
commit_all "fat ingest"
capture shape-thin-v1-lint     lint
capture shape-thin-v1-contract contract
# adr/v1, 40 records across three capabilities in one domain: the capability
# axis is balanced, so lint does not warn about the single domain.
fresh v1
gen_v1 adr 120 14
gen_v1 ingest 134 13
gen_v1 search 147 13
commit_all "balanced capabilities"
capture shape-balanced-v1-lint lint

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
