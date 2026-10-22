---
description: building a finished deliverable that goes to a client, a customer or leadership, such as a slide deck, a PDF report, a Word or Google document, a one-pager or a spreadsheet; one beat per slide, charts regenerated from fresh data, the established style followed and deviations flagged, and every page rendered and looked at before it goes out
vocabulary: deck slides presentation pdf docx word document google doc one pager spreadsheet excel xlsx pptx final report deliverable client customer leadership executive cio engagement send finished goes out status chart burnup roadmap visual diagram render inspect overflow margins template style guide toolchain convert export forward to their board sponsor polished
pattern: \bxlsx\b|\bpptx\b|\bdocx\b|\bone.?pager\b|burn.?up chart
commands: \b(pandoc|pdftoppm|soffice|libreoffice)\b
files: \.(docx|xlsx|pptx|odt)$
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: convention -->
# Finished Deliverables

A deck, PDF, document or sheet that goes to a client or to leadership is read without you there to explain it. Write the deliverable itself. Commentary on the notes stays in chat. Lead with checked figures (research/provenance) and show them visually where a picture reads faster.

## Follow the established work

- Match the last approved version, the reader's template, or the supplied style guide. Where the brief is specific, follow it, and flag to the user where it departs from the established work.
- Keep the project's agreed toolchain. A switch changes the look the reader knows.
- When the user wants a style kept, record each rule they set, such as how dates appear or whether numbers are numerals, where writing/register says standing style rules go, so the next build follows it.
- Copy an approved source file before editing it.

## Decks

- One slide per major beat. Plain headers carry the story.
- Start sparse. Cut until each slide holds its one point.
- Keep text inside the margins.

## Documents

- Open with the main finding. Put method, sources and caveats in notes or an appendix.
- Use one top-level heading and skip no levels. The first heading does not restate the title.
- Turn a two- or three-row table into a sentence when that reads better.
- In Google Docs, Word and Confluence, use the editor's formatting. Literal `**` and `##` show as text.

## Status visuals and charts

- Regenerate each chart from its data file, and refresh the data first. A re-dated chart with stale numbers misleads the reader.
- Show the whole agreed scope, and make what is due and when easy to see.

## Build and check

The pptx, docx, pdf and xlsx skills cover building each format. Before calling the deliverable done, render every page or slide and look at it for overflow, overlap and blank areas (code/testing/evaluation/handoff). The decoration check (markdown/density) sees only Markdown writes, so extract the text of an HTML, LaTeX, Word or PDF deliverable (`pdftotext file.pdf -`, `pandoc -t plain file.docx`) and read it for the same patterns before handoff. Gitignore generated files unless told to commit them. Delete one-off helper scripts when the deliverable is done.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "The build succeeded, so the deck is fine" | A clean build can still overflow a margin. Look at every page. |
| "More detail shows more work" | The reader wants the point. Cut until each slide holds one beat. |
| "I'll just update the date on the chart" | Refresh the data first, or leave the old date. |
| "This other toolchain is easier" | The project agreed on one. Switching breaks the established look. |

## See Also

- writing(writing) — parent: audience, structure, and genre before formatting
- research/provenance(research) — every figure on the page traced to its source
- writing/client-boundary(writing) — what internal material stays out of the client copy
- code/testing/evaluation/handoff(softwaredev) — render static output and look before handing it over
- visualization/charts(softwaredev) — quantitative charts from data
- visualization/diagrams(softwaredev) — structural diagrams
