---
description: Mermaid diagrams, flowcharts, sequence diagrams, state diagrams, diagram styling, palette and color choices for light and dark themes
vocabulary: mermaid diagram flowchart sequence state class gantt chart gitgraph timeline svg styling palette color fill stroke contrast light dark mode theme legible opaque subgraph legend hue translucent theme-independent ascii
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: convention -->
# Mermaid Way

Use Mermaid diagrams instead of ASCII art. Mermaid is diffable, renderable, and doesn't break when you need to add a box in the middle.

## Choose the Right Diagram Type

| Content | Diagram Type | Not |
|---------|-------------|-----|
| Temporal sequences, request/response flows | `sequenceDiagram` | flowchart |
| State transitions, lifecycles | `stateDiagram-v2` | flowchart |
| Decision logic, branching paths | `flowchart` | sequence |
| Class/entity relationships | `classDiagram` | flowchart |
| Timelines, project phases | `gantt` or `timeline` | flowchart |
| Git branching strategies | `gitgraph` | flowchart |

The most common mistake is using flowchart for everything. If the content has a time axis, it's a sequence diagram. If things transition between states, it's a state diagram.

## GitHub Compatibility

**Line breaks in node labels:** GitHub's Mermaid renderer does not support `\n` for line breaks — it renders the literal text `\n`. Use `<br>` or `<br/>` inside quoted strings instead:

```mermaid
%% Wrong — GitHub renders literal \n
flowchart LR
    A["First line\nSecond line"]

%% Correct — GitHub renders a line break
flowchart LR
    A["First line<br>Second line"]
```

When reviewing or writing Mermaid diagrams, replace any `\n` in node labels with `<br>`.

**Literal angle brackets in labels:** escape `<` and `>` as `&lt;` and `&gt;`. Otherwise the renderer reads `<env>` as an HTML tag and silently drops it.

**…but not in sequence-diagram messages — there, avoid the brackets entirely.** The `&lt;`/`&gt;` fix is for *node labels* (flowchart, state). In a `sequenceDiagram` message the trailing `;` of the entity is parsed as a statement separator, so `A->>B: deploy &lt;env&gt;` is a parse error. Rephrase to drop the brackets instead — `deploy(env)`, `deploy [env]`, or just `deploy env`.

## Styling for Both Themes

A diagram on GitHub is painted against a white **or** near-black page, depending on the viewer's theme setting — you don't control which. So the question that governs every color choice is: **where does this color land — on a node, or on the page?** Those two cases want opposite treatments.

**Text on a node fill → contrast is theme-independent, so pair brightness to the fill.** Make node fills fully *opaque* and choose the text color *per fill*, against the node's own background rather than the page:
- Dark text (`#1a1a1a`) on **bright** fills (orange, amber, teal)
- White text (`#ffffff`) on **deep** fills (violet, slate, deep blue)

Because an opaque node hides the page behind it, that pairing reads the same in both themes. This is also why "white text everywhere" (or black everywhere) breaks — some fills are light and some are dark, so one rule can't serve both.

**Subgraph titles and borders → they land on the *page*, so they must clear both backgrounds.** A subgraph label or stroke paints over the page itself, not over a node. A full-saturation brand color that looks crisp in dark mode can fall below ~2.9:1 on white. Use **mid-tone** strokes/labels that clear ~3:1 on *both* backgrounds, and give the container a translucent fill (alpha `1a`) so it tints the region without hiding the opaque nodes inside it.

**A starting palette** — the specific hues don't matter; the brightness-and-text pairing does. Swap colors freely, keep the structure:

```
Opaque node fills + matched text:
  bright  #f6821f orange  text #1a1a1a   |  #fbbf24 amber  text #1a1a1a
  bright  #2d7d9a teal    text #ffffff   |  (teal is dark enough for white)
  deep    #7c3aed violet  text #ffffff   |  #475569 slate  text #ffffff
  deep    #2d8e5e green   text #ffffff

Subgraph (over the page — mid-tone, ~3:1 in both modes):
  stroke #d97706 fill #f6821f1a   |   stroke #8b5cf6 fill #7c3aed1a

Borders: a neutral mid-gray (e.g. #4a5568 / #94a3b8) reads in both modes
```

**Map color to intent, then stay consistent.** A reader learns the legend once from context and then reads color as meaning — so a hue should signal the same *role* in every node and every diagram in a repo. Pick a mapping up front; these are common conventions, not rules:

| Color family | Reads as | Typical use |
|---|---|---|
| Green | healthy / done / data-at-rest | success states, stores, terminal "done" nodes |
| Blue / teal | neutral process / compute | the default "a step happens here" node |
| Violet / indigo | core / internal service | your own services, the system's heart |
| Amber / yellow | caution / waiting / config | pending states, queues, config & secrets |
| Orange / red | warning / external / boundary | third-party edges, error paths, things outside your control |
| Slate / gray | inert / out-of-scope | the browser, the user, anything you don't own |

Two anchors keep it legible: **one hue = one role** (don't reuse green for both "success" and "database" in the same diagram unless they're genuinely the same idea), and **brand or domain cues stay fixed** — if orange means "the edge tier" in one diagram, it means that in all of them. When a diagram has a recurring entity (a provider, a tier, a lane), give it its own hue and never lend that hue to anything else.

**Avoid:**
- One text color for every node — match it to each fill's brightness instead
- Translucent or default node fills — the page bleeds through and breaks in one mode
- Full-saturation brand colors on subgraph labels/strokes — fine on dark, washed out on white
- Unstyled diagrams when 3+ actors or concerns are present — add color

**Validate before committing.** Render the diagram (`mmdc -i diagram.mmd -o /tmp/out.svg`, or the terminal `mmaid` way) — a clean render means the syntax parsed. Eyeball it in both a light and a dark preview.
