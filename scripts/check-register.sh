#!/usr/bin/env bash
# Register lint for the always-on guidance surface (ADR-178).
#
# `hooks/ways/core.md` is prepended to every session before any work begins. It
# is therefore the largest style *sample* in the context window, and in-context
# style transfer is imitative before it is instructed: when a file's register
# disagrees with its rules, the register wins. ADR-178 measured core.md at 11
# antithesis constructions per thousand words against a corpus baseline of 4,
# including two as section headers, while the file's own text banned the
# construction.
#
# The `documentation/markdown/density` postcheck cannot catch this. It counts
# significance clauses and em-dashes, and core.md passed both thresholds while
# demonstrating the tic. This lint counts the shape instead.
#
# Thresholds are zero for core.md, which is stricter than anything applied to
# the rest of the corpus. That is deliberate: a triggered way reaches one
# session in many, and core reaches all of them. The rest of the corpus is
# reported for context and never fails the build.
#
# Exit code: 0 = clean, 1 = core.md violates. Advisory corpus rows never set it.

set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT" || exit 2

CORE="hooks/ways/core.md"
FAIL=0
note() { printf '  %s\n' "$1"; }
fail() { printf '\033[0;31m[FAIL] %s\033[0m\n' "$1"; FAIL=1; }
ok()   { printf '\033[0;32m[ ok ] %s\033[0m\n' "$1"; }

# The counterweight bolted onto a finished clause. Deliberately narrow: bare
# `not` and `no` are ordinary English and over-fire on any prose that states a
# constraint ("do not hard-wrap"). Only the appended-contrast shapes count.
ANTITHESIS="[,;] (and )?not [a-z]|; it (doesn't|does not|isn't)|\b(is|was|are|were) not (a|an|the) |\bnot (a|an|the) [a-z]+, but\b"

# A clause whose job is to rank the previous clause. Same pattern the density
# postcheck uses, kept in sync deliberately so the two surfaces agree.
SIGNIFICANCE="That is (the|what|why|precisely|not|exactly)\b|This is (the|not|why|what|precisely|exactly)\b|which is (exactly|precisely|why|the point)\b|(is|are|was|were) worth (stating|noting|dwelling|keeping|having|flagging)\b|matters? more than\b|The (tell|point|interesting part|important thing|key thing) is\b|is the (mark|signature|shape|tell) of\b|It is worth (noting|stating|saying)\b"

# Strip frontmatter, fenced code, tables, and headings. Those carry their own
# conventions and would skew every count.
prose_of() {
  awk '
    NR == 1 && /^---$/ { fm = 1; next }
    fm && /^---$/      { fm = 0; next }
    fm                 { next }
    /^```/             { fence = !fence; next }
    fence              { next }
    /^[[:space:]]*\|/  { next }
    /^#{1,6} /         { next }
    {
      gsub(/`[^`]*`/, "CODE"); gsub(/\]\([^)]*\)/, "]")
      # The corpus-wide "- name(domain) — gloss" form is structure, not prose.
      if ($0 ~ /^[[:space:]]*[-*] /) sub(/ — /, ": ")
      print
    }
  ' "$1"
}

count() { printf '%s' "$2" | grep -oEi "$1" 2>/dev/null | wc -l | tr -d ' '; }

echo "Register lint — always-on surface"

if [[ ! -f "$CORE" ]]; then
  fail "$CORE not found"
  exit 1
fi

PROSE="$(prose_of "$CORE")"
WORDS=$(printf '%s' "$PROSE" | wc -w | tr -d ' ')
ANTI=$(count "$ANTITHESIS" "$PROSE")
SIG=$(count "$SIGNIFICANCE" "$PROSE")
# A bolded thesis slogan opening a paragraph. Eight of these led core.md's
# paragraphs before ADR-178, and two of them were the banned construction.
SLOGAN=$(grep -cE '^\*\*[^*]+\*\*\.?( |$)' "$CORE" | tr -d ' ')
# A parenthetical aside set off by a matched pair of em-dashes. ASD-STE100 turns
# these into their own sentence, and doing so is what made the constructions in
# core.md visible in the first place.
ASIDE=$(grep -oE ' — [^—]{3,80} — ' "$CORE" | wc -l | tr -d ' ')
DASH=$(printf '%s' "$PROSE" | grep -o '—' | wc -l | tr -d ' ')

if (( WORDS > 0 )); then
  printf '  %s: %s words, %s em-dashes (%s per 1k)\n' \
    "$CORE" "$WORDS" "$DASH" "$(( DASH * 1000 / WORDS ))"
fi

(( ANTI == 0 ))   && ok "no antithesis constructions"       || fail "$CORE: $ANTI antithesis construction(s) — state the claim without the counterweight"
(( SIG == 0 ))    && ok "no significance clauses"           || fail "$CORE: $SIG significance clause(s) — cut the clause that ranks the previous one"
(( SLOGAN == 0 )) && ok "no bolded thesis slogans"          || fail "$CORE: $SLOGAN bolded paragraph lead-in(s) — write the sentence plainly"
(( ASIDE == 0 ))  && ok "no paired em-dash asides"          || fail "$CORE: $ASIDE paired em-dash aside(s) — give the aside its own sentence"

if (( ANTI > 0 || SIG > 0 || SLOGAN > 0 || ASIDE > 0 )); then
  echo
  note "Offending lines:"
  grep -nE "$ANTITHESIS|$SIGNIFICANCE|^\*\*[^*]+\*\*\.?( |\$)| — [^—]{3,80} — " "$CORE" \
    | sed 's/^/    /' | head -20
fi

# Corpus context. Advisory only — these fire on triggers, so their register
# reaches a fraction of sessions. ADR-178 records the baseline and leaves the
# sweep unscheduled.
if [[ "${1:-}" == "--corpus" ]]; then
  echo
  echo "Corpus baseline (advisory, never fails):"
  tw=0; ta=0
  while IFS= read -r f; do
    p="$(prose_of "$f")"
    w=$(printf '%s' "$p" | wc -w | tr -d ' ')
    (( w >= 200 )) || continue
    a=$(count "$ANTITHESIS" "$p")
    tw=$(( tw + w )); ta=$(( ta + a ))
    (( a * 1000 / w >= 8 )) && printf '    %3d/1k  %s\n' "$(( a * 1000 / w ))" "${f#hooks/ways/}"
  done < <(find hooks/ways -name '*.md' | sort)
  (( tw > 0 )) && printf '  corpus: %s per 1k across %s words\n' "$(( ta * 1000 / tw ))" "$tw"
fi

if (( FAIL )); then
  echo
  printf '\033[0;31mRegister lint failed. See ADR-178.\033[0m\n'
  exit 1
fi

exit 0
