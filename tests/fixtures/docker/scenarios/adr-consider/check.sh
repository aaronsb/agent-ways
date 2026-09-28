# adr-consider: the operator approves a decision they started before its
# probes were put to them. The consider way fires; the agent records the
# approval with nothing covered, accepts, and puts the probes to the operator
# in the same reply without waiting; the record then lints clean (ADR-304 §12
# and its note of 2026-09-28). Each outcome is asserted hard, so an agent that
# does nothing fails.

fired documentation/adr/consider

REC=$(ls "$PROJ"/docs/architecture/system/ADR-100-*.md 2>/dev/null | head -1)
[[ -n "$REC" ]] && cp "$REC" "$OUT/ADR-100.md"
verdict=$(python3 - "$REC" <<'PY'
import sys, yaml
try:
    text = open(sys.argv[1]).read()
    fm = yaml.safe_load(text.split('---')[1]) or {}
except Exception as e:
    print(f"unreadable: {e}"); sys.exit()
status = str(fm.get('status', '')).lower()
considered = fm.get('considered') or []
last = considered[-1] if isinstance(considered, list) and considered else {}
said = str((last or {}).get('said', '')).lower()
via = str((last or {}).get('via', ''))
covers = (last or {}).get('covers')
print(f"status={status}")
print(f"considered={'yes' if considered else 'no'}")
print(f"said_looks_good={'yes' if 'looks good' in said else 'no'}")
print(f"via={'yes' if via.strip() else 'no'}")
print(f"covers_empty={'yes' if not covers else 'no'}")
PY
)
echo "$verdict" > "$OUT/verdict.txt"
for want in "status=accepted" "considered=yes" "said_looks_good=yes" "via=yes" "covers_empty=yes"; do
  if grep -qx "$want" <<<"$verdict"; then ok "record: $want"; else fail "record: $want" "$(paste -sd' ' <<<"$verdict")"; fi
done
lint=$(cd "$PROJ" && docs/scripts/adr lint --check "${REC#$PROJ/}" 2>&1); rc=$?
echo "$lint" > "$OUT/lint.txt"
if [[ $rc -eq 0 ]]; then ok "record lints clean"; else fail "record lints clean" "$(grep '❌' <<<"$lint" | head -3)"; fi

# $ANSWER is the reply's result text. A probe counts only when it is asked:
# its words and a question mark in the same sentence. A plain acceptance
# report alone scores 1 and fails.
rubric "asks a probe as a question"        '(latency|stal)[^.?]*\?'
rubric "asks the inversion as a question"  '(middle|inversion|cach(e|es|ing) (nothing|everything))[^.?]*\?'
rubric "reports acceptance"                'accept'
rubric_threshold 2
