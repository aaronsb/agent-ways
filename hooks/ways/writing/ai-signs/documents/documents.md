---
description: Signs of generic, padded writing in documents such as reports, decks, proposals, exec summaries, one-pagers, memos, Confluence pages, and READMEs. Cut brochure words, trailing -ing clauses, coverage padding, and the challenges-and-outlook formula, open with the main finding, and fix heading case, heading levels, tiny tables, and bold on every line before the draft goes to its reader.
vocabulary: proposal memo exec-summary one-pager whitepaper confluence-page deck heading sentence-case title-case brochure puffery promotional vibrant groundbreaking renowned legacy trailing-clause outlook challenges roadmap-line bold-everywhere
pattern: \b(brochure|promotional|marketing.?speak)\b.{0,40}\b(proposal|report|deck|memo|one.?pager|page|doc|summary)\b|\b(proposal|report|deck|memo|one.?pager|page|doc|summary)\b.{0,40}\b(reads?|sounds?) like (a |an )?(brochure|press release|marketing copy|ad)\b
files: \.(docx|pptx|odt|rtf)$
commands: \bpandoc\b.{0,120}\.(docx|pptx|pdf)\b|\bmarp\b.{0,80}\.md\b
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: convention -->
# Signs in documents

Use these rules for reports, decks, proposals, and wiki pages.

## Words to cut

| Kind | Examples |
|---|---|
| Brochure words | vibrant, rich history, renowned, groundbreaking, showcasing, commitment to |
| Trailing -ing clauses | ", highlighting", ", ensuring", ", fostering" |
| Coverage padding | featured in, profiled in, trade publications, an active social media presence |
| Legacy filler | testament to, plays a key role, reflects broader trends, evolving landscape, sets the stage for |

## Structure

- End a sentence at the fact.
- Open with the main finding, not a line that defines the document.
- Put method, sources, and caveats in notes or an appendix. On the page, show only what the reader needs, and skip diagrams of your own build.
- Do not define a term the reader already knows or never needs.
- Delete roadmap lines like "In this section, we will discuss" and notes to the requester.
- Cut the "Despite its strengths, X faces challenges" formula and endings like "continues to thrive". If you must cover a risk, name its owner and next step.

## Formatting

- Match the last approved version or the reader's template if one exists.
- Write headings in sentence case.
- Use one top-level heading and skip no levels. Do not restate the title in the next heading.
- Turn a 2- or 3-row table into a sentence when that reads better.
- Do not bold a key phrase in every sentence.
- In Google Docs, Word, and Confluence, use editor formatting, not literal ** or ## marks.

## See Also

- writing(writing) — genre structure and revision passes
- trust/prose(meta) — significance clauses in long prose
- markdown(documentation) — Markdown conventions for files that stay as Markdown
