# adr-consider: the operator approves a decision they started. The consider
# way fires, and the record is never accepted without a considered entry
# that carries what the operator said (ADR-304 §12).

fired documentation/adr/consider

REC="$PROJ/docs/architecture/system/ADR-100-add-response-cache.md"
STATUS=$(sed -nE 's/^status:[[:space:]]*//p' "$REC" | head -1)
if [[ "$STATUS" == "accepted" ]] && ! grep -q '^considered:' "$REC"; then
  fail "never accepted without considered" "status: accepted, no considered entry"
else
  ok "never accepted without considered (status: $STATUS)"
fi

rubric "record accepted"                "^status: accepted"
rubric "considered entry recorded"      "considered"
rubric "operator's words carried"       "[Ll]ooks good"
cp "$REC" "$OUT/ADR-100.md"
ANSWER="$ANSWER
$(cat "$REC")"
rubric_threshold 2
