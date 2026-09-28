---
status: Accepted
date: 2026-02-17
deciders:
  - aaronsb
  - claude
related:
  - ADR-013
  - ADR-014
---

# ADR-100: Ways Scaffolding Wizard

## Context

The ways system has a cold-start problem. Creating a project-local way requires knowledge spread across multiple documents (extending.md, matching.md, authoring/way.md) and familiarity with directory conventions, YAML frontmatter, matching modes, and vocabulary design. This knowledge is well-documented but not discoverable at the moment a human thinks "I want Claude to do X differently in my project."

The current `/ways` skill shows which ways have fired in the current session — mildly interesting for debugging but not actionable. The `/test-way` skill handles vocabulary tuning and validation but assumes ways already exist. There is no guided path from intent ("our API uses GraphQL") to working way.

Meanwhile, the broader Claude Code ecosystem has developed patterns for agent steering (CLAUDE.md files, PROMPT.md, hooks) but these are either monolithic (everything in one file) or require manual setup. The ways system solves the architecture problem (event-driven, contextual injection) but lacks an entry point for humans who haven't read the documentation.

## Decision

Repurpose `/ways` from a session diagnostic into a scaffolding wizard that interviews the human and creates project-local ways.

### Design

The wizard is a Claude Code skill (`commands/ways.md`) that directs Claude through a conversational flow:

1. **Ground**: Read `docs/hooks-and-ways/matching.md` and `extending.md` to load the decision framework before engaging the human. This is explicit in the skill prompt — the agent needs the full matching mode landscape before it can recommend one.

2. **Detect**: Check project state — does `.claude/ways/` exist? Are there existing project-local ways? Show what's already there.

3. **Interview**: Use `AskUserQuestion` to gather intent conversationally:
   - "What should Claude know or do differently in this project?" (plain language)
   - Follow-up questions adapted to the answer — trigger timing, scope, specificity
   - Recommend matching mode with a one-sentence explanation of why

4. **Scaffold**: Create the directory and `way.md` with correct frontmatter. The body is the human's intent compressed to directive form, following the voice guidance from extending.md (collaborative, includes the why, writes for the innie).

5. **Validate**: Lint the new way. If semantic, score it against sample prompts from the conversation. Show the result.

6. **Handoff**: Point to `/ways-tests` for ongoing tuning. Explain that the way will fire automatically in future sessions when the trigger conditions are met.

### Session commitment

Invoking `/ways` is an intentional act — the human has decided to build or revise project steering. This isn't a casual diagnostic; it's dedicating the session to way work. The context cost of reading matching.md and extending.md upfront is justified because:

- The human expects informed recommendations, not guesswork
- Matching mode decisions are the first fork in the road — you can't interview well without understanding the options
- Revision of existing ways (vocabulary tuning, trigger changes, scope adjustments) requires the same foundational knowledge as creation

This framing also means the wizard handles both creation and revision. A human invoking `/ways` on a project with existing ways should be able to say "the API way isn't firing when I talk about endpoints" and get diagnostic help, not just a scaffold for a new way.

### Interaction with the ways system

The wizard leverages the existing trigger infrastructure recursively:

- Talking about ways fires `meta/knowledge` — system overview enters context
- Editing a way.md fires `meta/knowledge/authoring` — frontmatter spec enters context
- Discussing vocabulary fires `meta/knowledge/optimization` — tuning workflow enters context

The skill is the ignition; the ways system is the engine that keeps running as the conversation deepens. The skill prompt doesn't need to duplicate what the ways already provide — it just needs to start the conversation in the right neighborhood.

### Scope

- **Project-local ways** are the primary target — that's the user-facing use case
- **Global ways** remain an advanced/maintainer concern — the wizard can mention them but doesn't need to scaffold them
- The existing "which ways fired" diagnostic moves to a subcommand or gets folded into the wizard's detect phase

## Consequences

### Positive

- Humans can create project-local ways without reading documentation first
- The matching mode decision (regex vs semantic vs state) is guided rather than discovered
- The wizard naturally produces ways that follow conventions (correct directory structure, valid frontmatter, appropriate voice)
- Recursive trigger behavior means the wizard improves its own context as it works — each step loads more relevant guidance
- The `/ways` namespace becomes useful instead of decorative

### Negative

- The skill prompt needs to be rich enough to guide Claude but not so prescriptive that it scripts every branch — finding this balance requires iteration
- Wizard-created ways may need vocabulary tuning that the wizard can start but the human must finish (the discrimination judgment is inherently human)
- `/test-way` is renamed to `/ways-tests` to share the `/ways` root namespace (tab-completion discoverability), but remains a separate skill — creation/revision vs precision tuning are different tasks at different expertise levels

### Neutral

- The old "show fired ways" behavior could become `/ways status` or be dropped — low value either way
- This establishes a pattern for other scaffolding wizards (skills, hooks, ADR domains) if the approach works
- The wizard's grounding step (reading matching.md) means it consumes context tokens on the documentation — acceptable given the task is inherently documentation-heavy

## Alternatives Considered

- **Management hub with subcommands** (`/ways list`, `/ways new`, `/ways health`, `/ways test`): More comprehensive but spreads the skill thin. The cold-start problem is the acute pain; management features can be added later. A Swiss Army knife is less useful than a sharp blade when you need to cut one thing.

- **Dashboard focused on project state** (`/ways` shows coverage, gaps, health for this repo): Useful for maintainers but doesn't solve the creation problem. You can't show health metrics for ways that don't exist yet.

- **Template gallery** (pick from pre-built way templates for common patterns): Lower friction than a wizard but less adaptive. Templates assume the human's intent fits a category; the wizard discovers it through conversation. Templates could be a future enhancement within the wizard flow.
