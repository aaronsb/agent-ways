---
macro: prepend
requires: ["Bash(date:*)", "Bash(id:*)", "Bash(rm:*)", "Bash(uname:*)"]
refire: 0.1
---
<!-- epistemic: convention -->
# Hard-Wrapped Markdown Just Written

The file above was written with prose hard-wrapped to a column. Our convention is flat prose — one line per paragraph — because a wrapped paragraph makes the next `Edit` reproduce interior line breaks exactly, and turns a three-word change into a five-line diff.

You just wrote it, so it's cheap to fix now and it only gets more expensive later.

```bash
ways reflow --fix <file>     # flattens, backs the original up, prints the backup path
```

**Leave it as-is if the breaks are deliberate.** One-clause-per-line prose, a field list, and anything with an intentional `<br>` are all legitimate — the detector is a heuristic and this is advisory. If you meant the line breaks, ignore this.

See markdown(documentation) for the full convention and the cases where a line break carries structure.
