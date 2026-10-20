---
description: building finished deliverables for clients or leadership, such as slide decks with one beat per slide and no clutter, glanceable status visuals regenerated from data, legible diagrams, simple PDFs in the project's agreed toolchain, compact office documents, and spreadsheets formatted for people, then rendering and inspecting every page
vocabulary: deck slide slides presentation pdf report one-pager docx google doc spreadsheet excel xlsx pptx chart roadmap burnup status visual diagram svg png branded print style guide toolchain pandoc latex render inspect overflow overlap margins whitespace executive client deliverable frozen header
pattern: (build|make|create|draft|export|generate|render).{0,30}\b(decks?|pdfs?|slides|one.?pagers?)\b|\bxlsx\b|\bpptx\b|burn.?up chart
commands: \b(pandoc|pdflatex|xelatex|lualatex|latexmk|pdftoppm|soffice|libreoffice)\b
files: \.(tex|docx|xlsx|pptx)$|(^|/)(decks?|slides)/
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: convention -->
# Decks, PDFs, Docs, and Sheets

These go straight to executives and clients. Write the deliverable itself, not commentary on the notes. Lead with validated data and make it visual. Match the established style. Where the brief conflicts with the established work, follow the established work and flag the difference. Every number traces to a source.

## Decks

- One slide per major beat. Plain, common-sense headers carry the story.
- Start sparse. First drafts tend to carry far more text than the final version.
- No text greebles: eyebrow labels, stat headers, or slide metadata nobody reads.
- Write numerals ("9", not "nine").
- Keep text inside the margins, with generous whitespace.
- Use motion on at most one element per slide, if at all.
- Record every style rule the user sets, such as how definitions are formatted or whether dates appear, in the deck README so the next build follows it.

## Status visuals

- Use labels of two to four words, large text, a simple status label (for example a green "On Track"), and checkbox next steps.
- Show the whole agreed scope, not one phase, and make what is due and when easy to see.
- Regenerate from a data file, and refresh the data first. A re-dated chart with stale numbers is worse than no chart.

## Diagrams

- Prefer SVG plus PNG from the repo's build pipeline when the output must look polished. Go vertical when a wide layout gets cramped. Skip legends that the arrows already explain.
- Write image-model prompts as one plain paragraph with no bullets or headers. Check the output for accuracy.
- If generated imagery misses the tone, use sourced public-domain imagery with license credits.

## By Format

| Format | Do | Don't |
|---|---|---|
| PDF | Use the project's agreed toolchain (for example pandoc plus LaTeX, or HTML printed to PDF) and any supplied style guide. Keep branding, modest conventional type, enough white space, simple non-overlapping charts, a short sources section, and strict page limits. | Switch toolchains. Build callout-heavy or oversized layouts. Edit an approved source file in place (copy it first). |
| Doc | Compact, scannable rich text: bold, color, tables with working borders, links inline where needed. Little white space. | Add "prepared for" boilerplate or a links appendix. |
| Spreadsheet | Sized and formatted cells, a frozen header row, merged cells for hierarchy, full text instead of codes like "P1", blank cells when empty, concise linked sources. | Create renamed copies. Put instructions to the reader inside cells. |

White space differs by format on purpose.

Gitignore generated PDFs unless told to commit them. Delete one-off helper scripts when the deliverable is done.

## Then Check It

Render and inspect every page or slide for overlap, overflow, and blank areas before you call it done. Run a prose check on the extracted text as well.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "The build succeeded, so the deck is fine" | A clean build can still overflow a margin. Look at every page. |
| "More detail shows more work" | The reader wants the point. Cut until each slide holds one beat. |
| "I'll just update the date on the chart" | Refresh the data first or leave the old date. |
| "This other toolchain is easier" | The project agreed on one. Switching breaks the established look. |

## See Also

- writing(writing) — audience, structure, and genre before formatting
- visualization/charts(softwaredev) — quantitative charts from data
- visualization/diagrams(softwaredev) — structural diagrams
- mermaid(documentation) — Mermaid syntax when a quick diagram is enough
