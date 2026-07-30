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

## See Also

- trust/prose(meta) — the full account of what decoration is and why it survives to turn 50
- `documentation/markdown/reflow` — the sibling check, for hard-wrapped prose
