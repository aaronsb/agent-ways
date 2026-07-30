#!/usr/bin/env bash
# Postcheck for documentation/markdown/reflow (#415).
#
# Reads the PostToolUse input on stdin. Exit 0 = "freshly written markdown is
# hard-wrapped — fire the way." Exit 1 = no match, which is the default and the
# right outcome on anything unfamiliar.
#
# Note the inversion: this file's exit 0 means "fire", while `ways reflow`
# follows lint convention where exit 0 means "clean". One flag can't serve both
# conventions, so the inversion lives here, in the wrapper that exists anyway.
#
# ADR-123's amendment (2026-07-29) permits the state writes below: a postcheck
# may record session-scoped findings for its own way's macro.sh to read, because
# check-post.sh discards postcheck stdout and macro.sh gets no stdin. The
# constraints there are honored — session scope, own way, bounded, and never an
# input to this predicate. The exit code is a pure function of the observed
# post-state; .seen only suppresses *re-disclosure*, never detection.
set -euo pipefail

source "$(dirname "$0")/../../../sessions-root.sh"

INPUT=$(cat)

# One jq pass: this runs on every Edit/Write/Bash/Task PostToolUse, so keep
# forks minimal and bail before doing anything expensive.
{ read -r tool; read -r path; read -r session; } < <(
  printf '%s' "$INPUT" | jq -r '.tool_name // "", (.tool_input.file_path // ""), (.session_id // "")'
)

case "$tool" in Write | Edit | MultiEdit) ;; *) exit 1 ;; esac
[[ "$path" == *.md ]] || exit 1
[[ -n "$session" ]] || exit 1

# Don't fire on this way's own files: the way body and these scripts necessarily
# discuss wrapped prose, and the fixtures contain it deliberately.
[[ "$path" == */documentation/markdown/* ]] && exit 1

WAYS_BIN="${HOME}/.claude/bin/ways"
[[ -x "$WAYS_BIN" ]] || exit 1

STATE_DIR="${SESSIONS_ROOT}/${session}/markdown-reflow"
SEEN="${STATE_DIR}/seen"
PENDING="${STATE_DIR}/pending"

# Already surfaced this file this session — stay quiet. `refire` keys on the way,
# not the file, so without this a permissive cadence re-nags about a file the
# operator deliberately left wrapped.
[[ -f "$SEEN" ]] && grep -qxF "$path" "$SEEN" 2>/dev/null && exit 1

# Inspect the text that was just written, not the file on disk. Reading the file
# would fire on pre-existing drift in any file merely touched — false
# attribution, and a nag with no exit.
CONTENT=$(printf '%s' "$INPUT" \
  | jq -r '[.tool_input.content, .tool_input.new_string, (.tool_input.edits[]?.new_string)]
           | map(select(. != null)) | join("\n")' 2>/dev/null || true)
[[ -n "$CONTENT" ]] || exit 1

# Lint convention: 0 = clean, 1 = wrapped prose found. Check for exactly 1 —
# anything else is the binary failing (an install predating `ways reflow` exits
# 2 on the unknown subcommand), and an error must read as "no finding" rather
# than firing the way on every markdown write.
set +e
printf '%s' "$CONTENT" | "$WAYS_BIN" reflow --quiet 2>/dev/null
verdict=$?
set -e
(( verdict == 1 )) || exit 1

mkdir -p "$STATE_DIR"
printf '%s\n' "$path" >>"$SEEN"
printf '%s\t%s\n' "$(date +%s)" "$path" >>"$PENDING"

# Bound both files. Unconsumed entries are normal, not exceptional: the inward
# gate may deny the fire this request is asking for.
for f in "$SEEN" "$PENDING"; do
  if [[ -f "$f" ]] && (( $(wc -l <"$f") > 200 )); then
    tail -n 100 "$f" >"${f}.trim" && mv -f "${f}.trim" "$f"
  fi
done

exit 0
