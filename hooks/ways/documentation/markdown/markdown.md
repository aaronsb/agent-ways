---
description: markdown authoring mechanics — line handling, hard wrapping versus flat prose, when a line break carries structure, tables and fences
vocabulary: markdown wrap unwrap reflow flatten line length column paragraph prose hard wrap fill width flow text file authoring plaintext txt
files: \.md$
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: convention -->
# Markdown Line Handling

Write prose **one line per paragraph**. One line per list item, one line per table row, one line per blockquote line. Don't hard-wrap prose to a column.

This is a machine-ergonomics convention, not a style preference. Two concrete costs:

- **Hard wrapping breaks `Edit`.** Exact-string replacement means `old_string` has to reproduce every interior line break perfectly. A one-sentence change inside a wrapped paragraph becomes a multi-line match to transcribe; flat, it's a single line. This is where edits *miss* and get retried, not merely where they get long.
- **Hard wrapping makes diffs non-semantic.** Change three words and the rest of the paragraph reflows, so `git diff` shows five changed lines for one changed thought. Review cost starts scaling with wrap width instead of with the size of the change.

Neither cost is one-time. A flat file stays cheaper to edit for its whole life.

## Line breaks that do carry structure

A line break is fine — required, even — where the break *is* the structure. Keep these on their own lines:

| Construct | Why it stays |
|---|---|
| List items | The marker is line-initial syntax |
| Table rows | Row boundary is the line boundary |
| Headings, thematic breaks | Block-level, line-initial |
| Blockquote label lines (`**Status:**`, `**Next:**`) | Reads as a field list; joining makes one run-on line |
| Deliberate hard breaks (two trailing spaces, or `\`) | The break renders as `<br>` — joining silently deletes it |
| Parallel one-clause-per-line prose | Authored rhythm, not wrapping |

That last one is the judgment call. The test that separates it from hard wrapping: **does the break land at a clause boundary, or mid-phrase?** Wrapping breaks wherever the column runs out — mid-phrase, with line lengths all clustered just under the fill width. Authored breaks land where the thought turns, and their lengths vary freely.

## Wide content is allowed to be wide

Tables, long links, deep code lines, and long fenced blocks may run past any comfortable reading width. Readers scroll; authors shouldn't reflow. Never "fix" a table by rewrapping its cells.

## Other text formats are not markdown

The reasoning above depends on a renderer that reflows paragraphs. Where there is none, wrapping is the layout and should stay:

- **Plain `.txt`** — no renderer, so the wrap is the presentation. Leave it alone; 72 columns there is correct, not drift.
- **Commit message bodies** — keep the conventional 72-column wrap. Git tooling assumes it.
- **Code comments** — follow the language's own line-length convention.

## Repairing a wrapped file

`ways reflow <file>` reports hard-wrapped paragraphs and exits non-zero when it finds any; `--fix` flattens them, backs the original up first, and prints the backup path.

It repairs the *enclosing paragraph* of each detection and leaves everything else byte-identical, so it won't flatten authored one-clause-per-line prose elsewhere in the file. Read the diff anyway — the tool's token-stream check catches dropped words but cannot see a join that shouldn't have happened.

## See Also

- mermaid(documentation) — the sibling convention-plus-tool pair, for diagrams
- diataxis(documentation) — which *mode* a page is written in
- standards(documentation) — how conventions like this one get established
- knowledge/authoring(meta) — authoring the way files themselves
