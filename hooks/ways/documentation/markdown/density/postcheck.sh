#!/usr/bin/env bash
# Postcheck for documentation/markdown/density.
#
# Reads the PostToolUse input on stdin. Exit 0 = "the prose just written is
# dense with decoration tics — fire the way." Exit 1 = no match, the default and
# the right outcome on anything unfamiliar.
#
# Same inversion as the sibling reflow postcheck: exit 0 means "fire" here,
# while lint convention elsewhere reserves 0 for "clean".
#
# Why this is a postcheck and not a prompt-triggered way: the constraint being
# enforced is one the writer cannot self-check while drafting. Style directives
# delivered before writing decay across a long drafting session — measured in
# this repo at 118 em-dashes in an 11.5k-word document written 50 epochs after
# the writing way last fired. Counting after the write is the only path that
# reports a number instead of an intention.
#
# State writes follow the ADR-123 amendment (2026-07-29): session scope, this
# way's own directory, bounded, and never an input to the exit code.
set -euo pipefail

source "$(dirname "$0")/../../../sessions-root.sh"

INPUT=$(cat)

# One jq pass — this runs on every Edit/Write PostToolUse, so bail early and
# keep forks minimal.
{ read -r tool; read -r path; read -r session; } < <(
  printf '%s' "$INPUT" | jq -r '.tool_name // "", (.tool_input.file_path // ""), (.session_id // "")'
)

case "$tool" in Write | Edit | MultiEdit) ;; *) exit 1 ;; esac
[[ "$path" == *.md ]] || exit 1
[[ -n "$session" ]] || exit 1

# Don't fire on this way's own files: the body necessarily quotes the patterns
# it counts.
[[ "$path" == */documentation/markdown/* ]] && exit 1

STATE_DIR="${SESSIONS_ROOT}/${session}/markdown-density"
SEEN="${STATE_DIR}/seen"
PENDING="${STATE_DIR}/pending"

# Already surfaced this file this session — stay quiet. `refire` keys on the
# way, not the file, so without this a permissive cadence re-nags about prose
# the operator deliberately kept.
[[ -f "$SEEN" ]] && grep -qxF "$path" "$SEEN" 2>/dev/null && exit 1

# Inspect the text just written, not the file on disk. Reading the file would
# fire on pre-existing density in any file merely touched — false attribution,
# and a nag with no exit.
CONTENT=$(printf '%s' "$INPUT" \
  | jq -r '[.tool_input.content, .tool_input.new_string, (.tool_input.edits[]?.new_string)]
           | map(select(. != null)) | join("\n")' 2>/dev/null || true)
[[ -n "$CONTENT" ]] || exit 1

# Strip everything that is not running prose. Tables, code, and headings carry
# their own conventions and would skew every count — a comparison table is
# allowed to be dense, and a fenced block may contain any character at all.
PROSE=$(printf '%s\n' "$CONTENT" | awk '
  /^```/          { fence = !fence; next }
  fence           { next }
  /^[[:space:]]*\|/ { next }
  /^#{1,6} /      { next }
  /^[[:space:]]*$/ { print ""; next }
  {
    gsub(/`[^`]*`/, "CODE"); gsub(/\]\([^)]*\)/, "]")
    # Drop the definition separator on a list or bold-lead-in line. The
    # corpus-wide "- name(domain) — description" and "**Term** — gloss" forms
    # are structure, not prose, and counting them makes every way file and every
    # See Also section read as em-dash dense. Only the first is structural; a
    # second on the same line is the writer reaching for one.
    if ($0 ~ /^[[:space:]]*[-*] / || $0 ~ /^\*\*/) sub(/ — /, ": ")
    print
  }
')

WORDS=$(printf '%s' "$PROSE" | wc -w | tr -d ' ')

# Below this a single sentence swings the rate past any threshold. Short edits
# are the common case and must stay silent.
(( WORDS >= 150 )) || exit 1

# A clause whose job is to tell the reader the previous clause mattered. This
# is the check that discriminates: measured at 3.4 per 1k words in freshly
# drafted prose against 0.5 in prose that had been reviewed.
SIG=$(printf '%s' "$PROSE" | grep -oE \
  "That is (the|what|why|precisely|not|exactly)\b|This is (the|not|why|what|precisely|exactly)\b|which is (exactly|precisely|why|the point)\b|(is|are|was|were) worth (stating|noting|dwelling|keeping|having|flagging)\b|matters? more than\b|The (tell|point|interesting part|important thing|key thing) is\b|is the (mark|signature|shape|tell) of\b|It is worth (noting|stating|saying)\b" \
  2>/dev/null | wc -l | tr -d ' ')

DASH=$(printf '%s' "$PROSE" | grep -o "—" 2>/dev/null | wc -l | tr -d ' ')

# Rates per thousand words, integer arithmetic to avoid a bc dependency.
SIG_RATE=$(( SIG * 1000 / WORDS ))
DASH_RATE=$(( DASH * 1000 / WORDS ))

# Thresholds are deliberately loose. A surface that nags trains its reader to
# ignore it, so a fire has to mean something. Calibrated against measurements in
# this repo: significance clauses at 3/1k sits above reviewed prose and below
# the drafting average; em-dashes at 15/1k is roughly one every 67 words, which
# is dense by any reading and well above where the punctuation earns its keep.
(( SIG_RATE >= 3 || DASH_RATE >= 15 )) || exit 1

mkdir -p "$STATE_DIR"
printf '%s\n' "$path" >>"$SEEN"
printf '%s\t%s\t%s\t%s\t%s\t%s\n' \
  "$(date +%s)" "$path" "$WORDS" "$SIG" "$SIG_RATE" "$DASH_RATE" >>"$PENDING"

# Bound both files. Unconsumed entries are normal — the inward gate may deny the
# fire this request is asking for. The temp name carries $$ because parallel
# Edit calls mean parallel PostToolUse hooks, and a shared temp name lets two
# trimmers truncate the same file and mv a partial result over the state.
for f in "$SEEN" "$PENDING"; do
  if [[ -f "$f" ]] && (( $(wc -l <"$f") > 200 )); then
    tmp="${f}.trim.$$"
    tail -n 100 "$f" >"$tmp" 2>/dev/null && mv -f "$tmp" "$f"
    rm -f "$tmp"
  fi
done

exit 0
