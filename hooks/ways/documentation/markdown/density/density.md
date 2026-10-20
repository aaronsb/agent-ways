---
macro: prepend
requires: ["Bash(awk:*)", "Bash(date:*)", "Bash(grep:*)", "Bash(jq:*)", "Bash(mkdir:*)", "Bash(mv:*)", "Bash(rm:*)", "Bash(tail:*)", "Bash(wc:*)"]
refire: 0.1
---
<!-- epistemic: heuristic -->
# The Prose You Just Wrote Is Dense With Decoration

A check counted decoration patterns in the text you just wrote. The numbers above are for that file.

You wrote it moments ago, so the text is still in context and cutting is cheap. It only gets more expensive later.

## What was counted

**Significance clauses** — a clause whose job is to tell the reader that the previous clause mattered. `That is the…`, `which is exactly…`, `worth noting`, `X matters more than Y`. Reviewed prose in this repo sits near 0.5 per thousand words; fresh drafts run several times that.

**Em-dash density** — not wrong individually, a tic at volume. Counted, not banned.

## The fix is cutting, not rewriting

Search the file for the flagged patterns and delete them. A significance clause almost never carries information, so the sentence around it survives the cut intact:

> The audit found three dead triggers. ~~That is the mark of a system with no linting.~~

Then reread, and cut whatever you added back to preserve rhythm.

## When the count is fine

A comparison table, a quoted passage, or a document *about* these patterns will score high for legitimate reasons. This check reads the text you just wrote rather than the file on disk, so it can't see that context. If the count is explained by the subject matter, say so and move on — it won't fire again for this file this session.

## When the user bans a mark outright

Some users set a stricter rule than density, such as no dashes in prose, no prose semicolons, or a short list of stock phrases. Under that rule any hit gets fixed, not only a high count. Restructure the sentence instead of swapping in a different dash or a string of colons. The dash characters below are written as escapes so this file stays clean.

```bash
grep -nE $'\xe2\x80\x94|\xe2\x80\x93| -- ' FILE
grep -n ';' FILE
grep -niE 'seamless|delve|circling back|hope this finds' FILE
```

Semicolons inside code are fine. Replace the phrase list with the one the user gave.

## Leave verbatim material alone

Do not edit transcripts, frozen or submitted copies, archives, quotes, or the user's own prose and titles. If a hit sits in that text, keep it and say so in one line.

## Text this check cannot see

The check reads only Markdown writes. For HTML, LaTeX, Word, or PDF deliverables, extract the text first (`pdftotext file.pdf -`, `pandoc -t plain file.docx`) and run the same search before handoff. Chat posts, emails, and commit messages never pass through a write hook, so search their final text before they go out.

## Run the project gates

Where the project has a prose or register lint, run it and fix each blocking finding at its cause. Never add an allowlist entry to make a gate pass. Then reread for what a search cannot catch: fragments, length over a stated cap, and facts the rewrite dropped.

## See Also

- trust/prose(meta) — the full account of what decoration is and why it survives to turn 50
- markdown/reflow(documentation) — the sibling check, for hard-wrapped prose
