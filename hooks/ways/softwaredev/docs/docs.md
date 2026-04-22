---
description: documentation philosophy, markdown conventions, when to write docs
vocabulary: documentation markdown technical prose project docs
pattern: readme|documentation|docs|document.*project|explain.*repo
files: README\.md$|docs/.*\.md$
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: convention -->
# Documentation Way

We write documentation in markdown, use language-appropriate docstrings in code, and use Mermaid for diagrams. These aren't arbitrary choices — markdown renders everywhere, docstrings live with the code they describe, and Mermaid diagrams are version-controllable text that renders in GitHub, VS Code, and most documentation tooling.

## Principles

- **Progressive disclosure** — Overview → Details → Deep dives
- **Task-oriented** — Organize by what people want to do
- **Keep README current** — Outdated README = broken front door
- **Scale to complexity** — Simple project = simple README. Complex project = README + docs tree

## See Also

- docs/readme(softwaredev) — README as the front door
- docs/api(softwaredev) — API documentation patterns
- docs/standards(softwaredev) — documentation standards
