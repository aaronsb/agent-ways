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

# The contrast shapes ANTITHESIS misses, because the negation leads the clause
# instead of trailing it. The split-sentence form needs both halves on one line,
# which the corpus line-handling rule guarantees for a paragraph.
CONTRAST="\bnot (just|only|merely|simply) [^.!?]{0,60} but\b|\bit['’]?s not [^.!?]{0,60}, it['’]?s\b|\bthis does not mean\b[^.!?]{0,120}[.!?] it means\b"

# Assistant-voice residue that survives a copy-paste out of a chat reply.
# `Certainly` is anchored to its punctuation so the ordinary adverb is left be.
CHATBOT="Great question|I hope this helps|\bCertainly[!,]|Let me know if\b|\bAs an AI\b"

# A rebuttal to an objection nobody raised. `to be clear` needs its discourse
# punctuation, so "the goal is to be clear about scope" does not count.
ARGUING="\bto be clear[,:]|\bthis (is not|isn['’]t) (really |mainly |just )?about\b|\bmake no mistake\b|\blet['’]s be honest\b|\bdon['’]t get me wrong\b|\bI['’]m not saying\b"

# A line opening with a bolded label, as a list item or on its own. Three in a
# row turns a paragraph into a spec sheet.
BOLD_LABEL="^[[:space:]]*([-*+]|[0-9]+[.)])?[[:space:]]*[*][*][^*]+[*][*]:?"

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

# Runs of three or more consecutive bold-label lines, counted on the file rather
# than the prose because the shape is layout. Fenced blocks are skipped.
bold_runs() {
  awk -v pat="$BOLD_LABEL" '
    /^```/    { fence = !fence; run = 0; next }
    fence     { next }
    $0 ~ pat  { run++; if (run == 3) n++; next }
              { run = 0 }
    END       { print n + 0 }
  ' "$1"
}

echo "Register lint — always-on surface"

if [[ ! -f "$CORE" ]]; then
  fail "$CORE not found"
  exit 1
fi

PROSE="$(prose_of "$CORE")"
WORDS=$(printf '%s' "$PROSE" | wc -w | tr -d ' ')
ANTI=$(count "$ANTITHESIS" "$PROSE")
SIG=$(count "$SIGNIFICANCE" "$PROSE")
CONTRA=$(count "$CONTRAST" "$PROSE")
CHAT=$(count "$CHATBOT" "$PROSE")
ARGUE=$(count "$ARGUING" "$PROSE")
BOLDRUN=$(bold_runs "$CORE")
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
(( CONTRA == 0 )) && ok "no leading-negation contrasts"     || fail "$CORE: $CONTRA leading-negation contrast(s) — state the positive half alone"
(( CHAT == 0 ))   && ok "no chatbot residue"                || fail "$CORE: $CHAT chatbot phrase(s) — cut the assistant voice"
(( ARGUE == 0 ))  && ok "no arguing with no one"            || fail "$CORE: $ARGUE pre-emptive rebuttal(s) — drop the objection nobody raised"
(( BOLDRUN == 0 )) && ok "no bold-label runs"               || fail "$CORE: $BOLDRUN run(s) of 3+ bold-label lines — write them as sentences"

if (( ANTI > 0 || SIG > 0 || SLOGAN > 0 || ASIDE > 0 || CONTRA > 0 || CHAT > 0 || ARGUE > 0 )); then
  echo
  note "Offending lines:"
  grep -nEi "$ANTITHESIS|$SIGNIFICANCE|^\*\*[^*]+\*\*\.?( |\$)| — [^—]{3,80} — |$CONTRAST|$CHATBOT|$ARGUING" "$CORE" \
    | sed 's/^/    /' | head -20
fi

# Corpus context. Advisory only — these fire on triggers, so their register
# reaches a fraction of sessions. ADR-178 records the baseline and leaves the
# sweep unscheduled.
if [[ "${1:-}" == "--corpus" ]]; then
  echo
  echo "Corpus baseline (advisory, never fails):"
  tw=0; ta=0; tc=0; th=0; tg=0; tb=0
  while IFS= read -r f; do
    p="$(prose_of "$f")"
    w=$(printf '%s' "$p" | wc -w | tr -d ' ')
    (( w >= 200 )) || continue
    a=$(count "$ANTITHESIS" "$p")
    tw=$(( tw + w )); ta=$(( ta + a ))
    tc=$(( tc + $(count "$CONTRAST" "$p") ))
    th=$(( th + $(count "$CHATBOT" "$p") ))
    tg=$(( tg + $(count "$ARGUING" "$p") ))
    tb=$(( tb + $(bold_runs "$f") ))
    (( a * 1000 / w >= 8 )) && printf '    %3d/1k  %s\n' "$(( a * 1000 / w ))" "${f#hooks/ways/}"
  done < <(find hooks/ways -name '*.md' | sort)
  if (( tw > 0 )); then
    printf '  corpus: %s per 1k across %s words\n' "$(( ta * 1000 / tw ))" "$tw"
    printf '  contrast %s, chatbot %s, arguing %s, bold-label runs %s\n' \
      "$tc" "$th" "$tg" "$tb"
  fi
fi

if (( FAIL )); then
  echo
  printf '\033[0;31mRegister lint failed. See ADR-178.\033[0m\n'
  exit 1
fi

exit 0
