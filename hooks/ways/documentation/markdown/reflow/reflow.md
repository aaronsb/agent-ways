---
macro: prepend
requires: ["Bash(date:*)", "Bash(id:*)", "Bash(mv:*)", "Bash(rm:*)", "Bash(uname:*)"]
refire: 0.1
---
<!-- epistemic: heuristic -->
# Markdown You Just Wrote May Be Hard-Wrapped

A check ran over the text you just wrote and found prose that looks wrapped to a column. Our convention is flat prose — one line per paragraph — because a wrapped paragraph makes the next `Edit` reproduce interior line breaks exactly, and turns a three-word change into a five-line diff.

You wrote it moments ago, so it is cheap to fix now and only gets more expensive later.

## Two ways to fix it, and the first is usually better

**Fix it yourself.** The text is already in context. Rewriting the paragraph flat is an ordinary edit, and you can see what the prose is doing — whether a break was mechanical or meant.

**Or hand it to the tool:**

```bash
ways reflow --fix <file>
```

It copies the original aside first and prints that path, repairs only the paragraphs it detected, then reparses the result and compares. If anything moved beyond line breaks inside a paragraph, it writes nothing and reports what diverged.

## If you already fixed this file and it is still flagging

**Trust your prose over the check.** The detector is a heuristic. It cannot tell column wrapping from one-clause-per-line prose broken on purpose, and that style is one this convention explicitly permits.

A second flag on a file you have already looked at is therefore far more likely to be a false positive than a missed repair. Say so and move on. Don't rewrite prose you believe is correct in order to satisfy a check — that is how a heuristic starts degrading the thing it was meant to protect.

## When to leave it alone entirely

Line breaks that carry structure are correct and stay: list items, table rows, blockquote label lines (`**Status:**`), deliberate hard breaks (two trailing spaces), and parallel one-clause-per-line prose. See markdown(documentation) for the full convention.
