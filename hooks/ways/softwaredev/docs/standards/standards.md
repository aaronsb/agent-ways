---
description: establishing team norms, coding conventions, testing philosophy, dependency policy, accessibility requirements
vocabulary: convention norm guideline accessibility style guide linting rule agreement philosophy
scope: agent, subagent
refire: 0.2
---
<!-- epistemic: convention -->
# Standards Way

Standards define how a team works. They're distinct from quality metrics (the Quality Way measures adherence) — this way is about establishing and documenting the norms themselves.

## When Standards Come Up

- Starting a new project and defining conventions
- Onboarding context: "what are our standards?"
- Policy decisions: dependency criteria, accessibility requirements
- Resolving disagreements about style or approach

## Writing Standards Documents

Structure standards as actionable rules, not aspirational prose:

```markdown
## [Category] Standards

### Rule: [Concise directive]
**Rationale**: Why this matters.
**Example**: What compliance looks like.
**Exception**: When this doesn't apply (if any).
```

Keep them scannable. A standard nobody reads is a standard nobody follows.

## Common Standards Areas

| Area | Covers | Not |
|------|--------|----|
| Coding style | Formatting, naming, file structure | Architecture patterns (Design Way) |
| Testing philosophy | When to test, coverage expectations | Test mechanics (Testing Way) |
| Dependency policy | Evaluation criteria, update cadence | Package management (Deps Way) |
| Accessibility | WCAG compliance level, testing requirements | UI implementation details |
| Documentation | What to document, where, format | How to write docs (Docs Way) |

## Establishing vs Enforcing

This way helps **establish** standards. Enforcement belongs elsewhere:
- Linters and formatters for style rules
- CI checks for coverage thresholds
- Review checklists for process compliance

## Avoid

- Standards without rationale (rules need "why")
- Standards that duplicate tooling (if the linter catches it, don't write a standard for it)
- Aspirational standards nobody plans to enforce
