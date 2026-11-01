---
description: choosing a documentation mode — Diátaxis classification (tutorial / how-to / reference / explanation) for a catalog page
vocabulary: diataxis tutorial how-to reference explanation mode classify learning working practical theoretical study newcomer goal information understanding catalog page classification document quadrant learning-oriented understanding-oriented information-oriented recipe reclassify
pattern: di[aá]taxis|which (mode|kind of (doc|page))|tutorial vs|reference vs|explanation vs|what mode
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: convention -->
# Diátaxis — picking a documentation mode

Every catalog page is exactly **one** of four modes. The mode is the page's *pole*, the `P` in its `DD.NNN.P` id, and the linter enforces it. Choose the mode deliberately, because a page that mixes two modes fails the readers of both.

The tool computes the id and writes the frontmatter, but **it cannot choose the mode for you.** Decide the mode, then scaffold the page:

```bash
docs/scripts/doc new <domain> <mode> "<title>"   # mode: tutorial|how-to|reference|explanation
docs/scripts/doc lint                            # the page is lint-clean by construction
```

## Two questions, four modes

Two questions place every page in one of the four modes:

1. **Is the reader _studying_ or _working_?** Building understanding for later, or solving a task right now?
2. **Is the content _practical steps_ or _theoretical knowledge_?** Things to *do*, or things that *are* / *why*?

|                       | Practical steps (action) | Theoretical knowledge (cognition) |
|-----------------------|--------------------------|-----------------------------------|
| **Studying** (acquire) | **Tutorial** (`T`)       | **Explanation** (`E`)             |
| **Working** (apply)    | **How-to** (`H`)         | **Reference** (`R`)               |

- **Tutorial** — a guided lesson that takes a newcomer through doing something successfully. It is learning-oriented: the author teaches, and the reader follows the steps on trust.
- **How-to** — a recipe to achieve a specific goal for someone who already knows the basics. It is task-oriented and leaves out the teaching.
- **Reference** — a description of the machinery: APIs, flags, schemas, options. It is information-oriented, accurate and complete. Readers look things up in it instead of reading it through.
- **Explanation** — a discussion of a topic: why it works this way, the design rationale, the trade-offs, and how it fits the larger system. It is understanding-oriented.

## Rules

- **There is no fifth mode.** If a page does not fit one quadrant, split it into two pages.
- **One mode per page.** The common failure is a tutorial that keeps stopping to explain, or a reference padded with how-to steps. Split them into two pages and link each to the other with `related:`.
- **The mode tells the reader** what kind of help the page gives. Keep the page in its quadrant, and when its content drifts into another one, reclassify the page or split it.

## See Also

- documentation(documentation) — the typed-graph model these pages live in
- standards(documentation) — house style for the prose itself
