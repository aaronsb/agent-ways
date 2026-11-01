---
description: markdown authoring mechanics — line handling, hard wrapping versus flat prose, when a line break carries structure, tables and fences
vocabulary: markdown wrap unwrap reflow flatten line length column paragraph prose hard wrap fill width flow text file authoring plaintext txt rewrap hard-wrap unwrapped commonmark reparse
files: \.md$
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: convention -->
# Markdown Line Handling

Write each paragraph of prose on one line in the file. Give each list item, table row and blockquote line one line too. Don't hard-wrap prose to a column.

The convention exists for the tools that edit and compare files. Hard wrapping costs twice:

- An `Edit` replaces an exact string, so its `old_string` has to reproduce every line break inside a wrapped paragraph. A one-sentence change becomes a multi-line match to transcribe, and that match is where edits miss and get retried.
- When three words change, the rest of a wrapped paragraph reflows, so `git diff` shows five changed lines for one changed thought. The reviewer then reads in proportion to the wrap width.

Both costs recur on every later edit of the file.

## Line breaks that do carry structure

Keep a line break where the break is the structure. These stay on their own lines:

| Construct | Why it stays |
|---|---|
| List items | The marker is line-initial syntax |
| Table rows | Row boundary is the line boundary |
| Headings, thematic breaks | Block-level, line-initial |
| Blockquote label lines (`**Status:**`, `**Next:**`) | Reads as a field list; joining makes one run-on line |
| Deliberate hard breaks (two trailing spaces, or `\`) | The break renders as `<br>` — joining silently deletes it |
| Verse, an address, or other text where each line is its own unit | Each line is the unit, and joining changes the text. End each such line with a hard break (`\` or two trailing spaces), so the renderer keeps it on its own line and the wrap check accepts it |

A paragraph of prose is never one of these. When two lines of the same paragraph sit one under the other, join them.

## Wide content is allowed to be wide

Tables, long links, deep code lines and long fenced blocks may run past any comfortable reading width. The reader scrolls. Never rewrap a table's cells to narrow it.

## Other text formats are not markdown

The reasoning above depends on a renderer that reflows paragraphs. Where there is none, the wrap is the layout and stays:

- In a plain `.txt` file, nothing reflows the text, so a 72-column wrap there is correct.
- A commit message body keeps the conventional 72-column wrap, because git tooling assumes it.
- A code comment follows the language's own line-length convention.

## Repairing a wrapped file

`ways author reflow <file>` reports hard-wrapped paragraphs and exits non-zero when it finds any; `--fix` flattens them, backs the original up first, and prints the backup path.

It repairs the paragraph around each detection and leaves the rest of the file byte for byte as it was.

Before writing, it reparses the result and compares the document's structure with the original. If anything moved beyond the line breaks inside a paragraph, it writes nothing and says so. Read the diff anyway, because that comparison cannot see two things: a join that erased a deliberate line break, and non-CommonMark syntax such as `:::` directives.

The detector looks for line lengths clustered under a fill width, so it misses a paragraph wrapped by hand at uneven points. In this repository, `scripts/check-register.sh` fails on any two prose lines of one paragraph in a way body or SKILL.md.

## See Also

- mermaid(documentation) — the sibling convention-plus-tool pair, for diagrams
- diataxis(documentation) — which *mode* a page is written in
- standards(documentation) — how conventions like this one get established
- knowledge/authoring(meta) — authoring the way files themselves
- writing/plain(writing) — the sentences inside the paragraph
