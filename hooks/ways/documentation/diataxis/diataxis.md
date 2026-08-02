---
description: choosing a documentation mode — Diátaxis classification (tutorial / how-to / reference / explanation) for a catalog page
vocabulary: diataxis tutorial how-to reference explanation mode classify learning working practical theoretical study newcomer goal information understanding catalog page classification document
pattern: di[aá]taxis|which (mode|kind of (doc|page))|tutorial vs|reference vs|explanation vs|what mode
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: convention -->
# Diátaxis — picking a documentation mode

Every catalog page is exactly **one** of four modes. The mode is the page's
*pole* (the `P` in its `DD.NNN.P` id) and it is enforced — pick it deliberately,
because a page that serves two modes serves neither.

The tool computes the id and writes the frontmatter; **this is the one judgment
it can't make for you.** Decide the mode, then scaffold:

```bash
docs/scripts/doc new <domain> <mode> "<title>"   # mode: tutorial|how-to|reference|explanation
docs/scripts/doc lint                            # the page is lint-clean by construction
```

## The closed 2×2

Two questions place every page:

1. **Is the reader _studying_ or _working_?** Building understanding for later, or solving a task right now?
2. **Is the content _practical steps_ or _theoretical knowledge_?** Things to *do*,
   or things that *are* / *why*?

|                       | Practical steps (action) | Theoretical knowledge (cognition) |
|-----------------------|--------------------------|-----------------------------------|
| **Studying** (acquire) | **Tutorial** (`T`)       | **Explanation** (`E`)             |
| **Working** (apply)    | **How-to** (`H`)         | **Reference** (`R`)               |

- **Tutorial** — a guided lesson that takes a newcomer through doing something successfully. Learning-oriented; you are the teacher, the reader trusts you.
- **How-to** — a recipe to achieve a specific goal for someone who already knows the basics. Task-oriented; assumes competence, omits the teaching.
- **Reference** — a description of the machinery: APIs, flags, schemas, options. Information-oriented; accurate, complete, consulted not read.
- **Explanation** — illuminates a topic: why it works this way, the design rationale, the trade-offs, the bigger picture. Understanding-oriented.

## Rules

- **There is no fifth mode.** If a page won't fit one quadrant, it is two pages.
- **One mode per page.** The classic failure is a tutorial that keeps stopping to explain, or a reference padded with how-to steps — each interrupts the other's job. Split them and `related:`-link across.
- **The mode is a promise to the reader** about what kind of help they're getting.
  Keep the page faithful to its quadrant; if it drifts, reclassify or split.

## See Also

- documentation(documentation) — the typed-graph model these pages live in
- standards(documentation) — house style for the prose itself
