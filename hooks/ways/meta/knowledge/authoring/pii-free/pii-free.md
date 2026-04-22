---
description: stripping personal information from ways when decomposing personal skills or configurations into shared reusable guidance
vocabulary: pii personal information names emails accounts strip anonymize decompose persona
files: \.claude/(hooks/)?ways/.*way\.md$
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: constraint -->
# PII-Free Ways

When decomposing personal configurations, skills, or personas into reusable ways, strip all personal information before writing.

## What to Remove

| PII Type | Example in Source | Replacement |
|----------|-------------------|-------------|
| Names | "Check if Sarah replied" | "Check for replies" |
| Email addresses | "scan inbox for alice@corp.com" | "scan inbox across configured accounts" |
| Account identifiers | "workspace ID: 12345" | (omit entirely) |
| Organization names | "Acme Corp's Jira instance" | "the configured Jira instance" |
| Phone numbers, URLs with tokens | Specific endpoints or contact info | Generic references or omit |

## Why This Matters

Ways are injected as system context. They may be shared across projects, published as plugins, or visible to teammates. Personal information embedded in guidance text becomes a leak vector that's hard to audit after the fact.

## The Pattern

Source material (a personal skill, a runbook, a workflow description) often contains specific names and identifiers because it was written for one person's use. The decomposition step is where abstraction happens — the way should describe the *pattern*, not the *instance*.

```
(source_material)-[:DECOMPOSE {strip PII}]->(way_draft)-[:REVIEW]->(published_way)
```

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "It's just a first name, not sensitive" | Names establish identity. Combined with role context in the way, it's enough to identify someone. |
| "This is a private repo" | Ways migrate. Global ways sync across machines. Plugin ways publish. Assume eventual exposure. |
| "I'll clean it up later" | You won't find every instance. Strip at authoring time when the source material is in front of you. |
