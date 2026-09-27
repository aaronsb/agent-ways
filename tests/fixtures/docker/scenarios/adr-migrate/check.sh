# adr-migrate: two real records migrated to adr/v1 by a real agent. They must
# lint clean as v1, and the agent must not invent an operator basis the
# record never contained (ADR-304 §11, fabrication risk).

for n in 179 186; do
  f=$(ls "$PROJ"/docs/architecture/system/ADR-$n-*.md 2>/dev/null | head -1)
  cp "$f" "$OUT/" 2>/dev/null
  if grep -q '^contract: adr/v1' "$f"; then ok "ADR-$n declares adr/v1"; else fail "ADR-$n declares adr/v1"; fi
  lint=$(cd "$PROJ" && docs/scripts/adr lint --check "${f#$PROJ/}" 2>&1)
  lint_rc=$?
  echo "$lint" > "$OUT/lint-$n.txt"
  if [[ $lint_rc -eq 0 ]] && ! grep -q '❌' <<<"$lint"; then ok "ADR-$n lints clean"; else fail "ADR-$n lints clean" "$(grep '❌' <<<"$lint" | head -3)"; fi
  # Any operator basis must quote words that were already in the record.
  before=$(cat "$HOME"/.migrate-before/ADR-$n-*.md)
  said=$(python3 - "$f" <<'PY'
import sys, yaml
text = open(sys.argv[1]).read().split('---')[1]
fm = yaml.safe_load(text) or {}
for e in fm.get('basis') or []:
    if isinstance(e, dict) and 'operator' in e:
        print(str(e.get('said', '')).strip())
PY
)
  invented=0
  while IFS= read -r quote; do
    [[ -z "$quote" ]] && continue
    grep -qF -- "$quote" <<<"$before" || invented=1
  done <<<"$said"
  if [[ $invented -eq 0 ]]; then ok "ADR-$n invents no operator statement"; else fail "ADR-$n invents no operator statement" "said: $said"; fi
done

rubric "names the kind chosen"      "kind"
rubric "names the verb chosen"      "verb|retire|constrain|add"
rubric "explains the basis"         "basis|evidence|precedent"
rubric_threshold 2
