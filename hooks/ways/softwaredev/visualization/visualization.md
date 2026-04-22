---
description: Suggest visual representations when explaining, walking through, or describing systems, flows, and relationships
vocabulary: walk me through explain how show me describe overview understand flow process step by step architecture workflow pipeline relationship lifecycle sequence interaction dependency diagram visual
pattern: walk.*through|explain.*how|show.*how|describe.*flow|step.by.step|how.*work|overview.*system
embed_threshold: 0.28
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: heuristic -->
# Visualization Opportunity

When explaining something structural — a workflow, a lifecycle, how components interact — consider whether a diagram would communicate it better than prose.

**This is a nudge, not a mandate.** If you're about to write a multi-paragraph textual walkthrough of something spatial or sequential, a visual is probably clearer.

| Signal | Consider |
|---|---|
| "Walk me through..." | Sequence diagram or flowchart |
| "How does X work?" | Flowchart or state diagram |
| "Show me the data" | Chart (bar, line, histogram) |
| "What's the relationship between..." | ER diagram or class diagram |
| Comparing options | Table or chart |

Render to the terminal with the tools in the children of this way:
- `diagrams/` — `mmaid` for structural diagrams (Mermaid syntax)
- `charts/` — `chart-tool` for quantitative data (JSON piped)

## See Also

- visualization/charts(softwaredev) — quantitative data rendering
- visualization/diagrams(softwaredev) — structural diagram rendering
- docs/mermaid(softwaredev) — Mermaid syntax for diagrams
