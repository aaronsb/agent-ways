# adr-way: an architecture-decision prompt discloses the ADR way, and the
# answer reflects it. The project has no ADR tooling, so the way's macro
# offers to install it.

fired documentation/adr

rubric "names ADRs"                      'ADR|architecture decision record'
rubric "covers context"                  'context'
rubric "covers consequences"             'consequence'
rubric "covers alternatives considered"  'alternative'
rubric "mentions ADR tooling or scaffold" 'docs/scripts/adr|adr new|scaffold|tooling'
rubric_threshold 4

if [[ -z "$(cd "$PROJ" && git status --porcelain)" ]]; then
  ok "no files created, as the prompt asked"
else
  fail "no files created, as the prompt asked" "$(cd "$PROJ" && git status --porcelain)"
fi
