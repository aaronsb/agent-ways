# adr-migrate: two real records migrated to adr/v1 by a real agent. They must
# lint clean as v1, and the agent must not invent an operator basis the
# record never contained (ADR-304 §11, fabrication risk).

for n in 179 186; do
  f=$(ls "$PROJ"/docs/architecture/*/ADR-$n-*.md 2>/dev/null | head -1)
  cp "$f" "$OUT/" 2>/dev/null
  if grep -q '^contract: adr/v1' "$f"; then ok "ADR-$n declares adr/v1"; else fail "ADR-$n declares adr/v1"; fi
  lint=$(cd "$PROJ" && docs/scripts/adr lint --check "${f#$PROJ/}" 2>&1)
  lint_rc=$?
  echo "$lint" > "$OUT/lint-$n.txt"
  if [[ $lint_rc -eq 0 ]] && ! grep -q '❌' <<<"$lint"; then ok "ADR-$n lints clean"; else fail "ADR-$n lints clean" "$(grep '❌' <<<"$lint" | head -3)"; fi
  # Neither record quotes the operator, so any operator basis, considered or
  # concern entry is invented (ADR-304 §11).
  invented=$(python3 - "$f" <<'PY'
import sys, yaml
fm = yaml.safe_load(open(sys.argv[1]).read().split('---')[1]) or {}
found = [k for k in ('considered', 'concern') if fm.get(k)]
found += ['operator basis' for e in fm.get('basis') or [] if isinstance(e, dict) and 'operator' in e]
print(', '.join(found))
PY
)
  if [[ -z "$invented" ]]; then ok "ADR-$n invents no operator statement"; else fail "ADR-$n invents no operator statement" "found: $invented"; fi
  # The original body survives: every original heading is still there.
  missing=$(grep -E '^#{1,3} ' "$HOME"/.migrate-before/ADR-$n-*.md | while IFS= read -r h; do grep -qxF -- "$h" "$f" || echo "$h"; done)
  if [[ -z "$missing" ]]; then ok "ADR-$n keeps its original sections"; else fail "ADR-$n keeps its original sections" "$missing"; fi
done

rubric "names the kind chosen"      "kind: decision|as a decision"
rubric "names the verb chosen"      "\\b(retire|constrain|add|change|cut)\\b"
rubric "explains the basis"         "\\b(evidence|precedent)\\b"
rubric_threshold 3
