# adr-consider: the operator approves a decision they started before its
# intent check was put to them. The consider way fires; the agent records the
# approval with nothing covered, accepts, and asks or offers the intent check
# in the same reply without waiting; the record then lints clean (ADR-304 §12
# and its notes of 2026-09-28). Each record outcome is asserted hard, so an
# agent that does nothing, or halts to ask first, fails.

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

# $ANSWER is the reply's result text. The intent check counts when the reply
# asks it or offers it: its words and a question mark in the same sentence,
# or its words and an offer such as "if you have a view, say so" on the same
# line (grep matches per line). A bare "let me know" is not an offer, since a
# plain report ends with one. The inversion is not
# required: the way puts it to the operator only when it turns on what they
# want. A plain acceptance report alone scores 1 and fails.
rubric "puts the intent check to the operator" '(stal|30 ?s|seconds old|\bfresh)[^.?]*\?|(stal|30 ?s|seconds old|\bfresh).*(say so|if you (have|want to weigh|disagree|see)|your (view|take))|(say so|if you (have|want to weigh|disagree|see)|your (view|take)).*(stal|30 ?s|seconds old|\bfresh)'
rubric "reports acceptance"                    'accept'
rubric_threshold 2
