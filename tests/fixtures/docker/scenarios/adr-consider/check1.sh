# adr-consider, turn 1: the operator approves before the probes were put to
# them. The consider way fires; the agent asks the probes and the inversion
# and leaves the record as it was (ADR-304 §12, note of 2026-09-28).

fired documentation/adr/consider

REC=$(ls "$PROJ"/docs/architecture/system/ADR-100-*.md 2>/dev/null | head -1)
[[ -n "$REC" ]] && cp "$REC" "$OUT/ADR-100.turn1.md"
if [[ -n "$REC" ]] && (cd "$PROJ" && git diff --quiet HEAD -- "${REC#$PROJ/}"); then
  ok "turn 1: record untouched"
else
  fail "turn 1: record untouched" "$(cd "$PROJ" && git diff --stat HEAD -- docs/architecture | tail -1)"
fi

printf '%s\n' "$ANSWER" > "$OUT/answer1.txt"
rubric "asks about the latency win"   'latency'
rubric "asks about staleness"         'stale|30 ?s(ec|econds)?'
rubric "asks the inversion"           'middle|inversion|cach(e|es|ing) (nothing|everything)'
rubric "puts a question"              '\?'
rubric_threshold 3
