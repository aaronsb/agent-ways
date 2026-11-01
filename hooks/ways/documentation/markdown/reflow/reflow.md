---
macro: prepend
requires: ["Bash(date:*)", "Bash(id:*)", "Bash(mv:*)", "Bash(rm:*)", "Bash(uname:*)"]
refire: 0.1
---
<!-- epistemic: heuristic -->
# Markdown You Just Wrote May Be Hard-Wrapped

A check ran over the text you just wrote and found prose that looks wrapped to a column. The convention is one line per paragraph (markdown(documentation)), because a wrapped paragraph makes the next `Edit` reproduce its line breaks exactly and turns a three-word change into a five-line diff.

You wrote it moments ago, so it is cheap to fix now and only gets more expensive later.

## Two ways to fix it

The first is usually better. Fix it yourself: the text is already in context, joining the paragraph is an ordinary edit, and you can see whether a break was mechanical or meant.

Or hand it to the tool:

```bash
ways author reflow --fix <file>
```

It copies the original aside first and prints that path, repairs only the paragraphs it detected, then reparses the result and compares. If anything moved beyond line breaks inside a paragraph, it writes nothing and reports what diverged.

## If you already fixed this file and it is still flagging

The detector is a heuristic. It cannot tell column wrapping from verse or other text whose lines are deliberate units. When a second flag lands on lines like that, keep the lines and end each one with a hard break (`\` or two trailing spaces). The detector and the wrap check accept a line that ends in a hard break. When it lands on a paragraph of prose, join the paragraph.

## When to leave it alone entirely

Line breaks that carry structure stay: list items, table rows, blockquote label lines (`**Status:**`), deliberate hard breaks (two trailing spaces), and verse or other text where each line is its own unit. markdown(documentation) has the full convention.
