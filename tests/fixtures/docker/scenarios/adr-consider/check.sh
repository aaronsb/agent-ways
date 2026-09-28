# adr-consider, turn 2: the operator answers the probes (prompt2.txt). The
# agent records that answer verbatim with the probes it covers, and accepts;
# the record then lints clean (ADR-304 §12). Turn 1 is checked in check1.sh.
# Each outcome is asserted hard, so an agent that does nothing fails.

fired documentation/adr/consider

REC=$(ls "$PROJ"/docs/architecture/system/ADR-100-*.md 2>/dev/null | head -1)
[[ -n "$REC" ]] && cp "$REC" "$OUT/ADR-100.md"
verdict=$(python3 - "$REC" "$dir/prompt2.txt" <<'PY'
import sys, yaml
try:
    text = open(sys.argv[1]).read()
    fm = yaml.safe_load(text.split('---')[1]) or {}
except Exception as e:
    print(f"unreadable: {e}"); sys.exit()
reply = ' '.join(open(sys.argv[2]).read().split())
status = str(fm.get('status', '')).lower()
considered = fm.get('considered') or []
last = considered[-1] if isinstance(considered, list) and considered else {}
said = ' '.join(str((last or {}).get('said', '')).split())
via = str((last or {}).get('via', ''))
covers = (last or {}).get('covers') or []
covers = [str(c) for c in covers] if isinstance(covers, list) else []
print(f"status={status}")
print(f"considered={'yes' if considered else 'no'}")
print(f"said_verbatim={'yes' if reply in said else 'no'}")
print(f"via={'yes' if via.strip() else 'no'}")
for probe in ('latency', 'staleness', 'inversion'):
    print(f"covers_{probe}={'yes' if probe in covers else 'no'}")
PY
)
echo "$verdict" > "$OUT/verdict.txt"
for want in "status=accepted" "considered=yes" "said_verbatim=yes" "via=yes" \
            "covers_latency=yes" "covers_staleness=yes" "covers_inversion=yes"; do
  if grep -qx "$want" <<<"$verdict"; then ok "record: $want"; else fail "record: $want" "$(paste -sd' ' <<<"$verdict")"; fi
done
lint=$(cd "$PROJ" && docs/scripts/adr lint --check "${REC#$PROJ/}" 2>&1); rc=$?
echo "$lint" > "$OUT/lint.txt"
if [[ $rc -eq 0 ]]; then ok "record lints clean"; else fail "record lints clean" "$(grep '❌' <<<"$lint" | head -3)"; fi

rubric "mentions the consideration"  "consider"
rubric "reports acceptance"          "accept"
rubric_threshold 1
