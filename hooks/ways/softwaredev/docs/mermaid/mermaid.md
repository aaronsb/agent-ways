---
description: Mermaid diagrams, flowcharts, sequence diagrams, state diagrams, diagram styling
vocabulary: mermaid diagram flowchart sequence state class gantt chart gitgraph timeline svg
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

## Styling

Diagrams should be legible in both dark and light mode with good color saturation.

**Color principles:**
- Use **mid-saturation colors** — vivid enough to differentiate, not so bright they strain
- Avoid pure white (`#fff`) or pure black (`#000`) fills — they break in one mode or the other
- Use **consistent text colors** that contrast against their fill in both modes

**Recommended palette:**

```
Fills (mid-saturation, dark/light safe):
  #2D7D9A  — teal/process blue
  #7B2D8E  — purple/integration
  #2D8E5E  — green/success/data
  #C2572A  — burnt orange/warning/external
  #5A6ABF  — slate blue/internal
  #8E6B2D  — amber/config/state

Text: #FFFFFF on dark fills, #1A1A2E on light fills
Borders: #4A5568 (neutral gray, works both modes)
```

**Avoid:**
- Default unstyled diagrams when 3+ actors or concerns are present — add color
- Neon or pastel fills that disappear against white or dark backgrounds
- Text-on-fill combinations that require squinting
