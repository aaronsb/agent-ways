---
description: writing or editing an agent skill, agent definition, prompt, or project instruction file that describes how work is done here
vocabulary: skill agent definition prompt file instructions claude.md agents.md system prompt persona onboarding the agent
files: (SKILL\.md|CLAUDE\.md|AGENTS\.md|\.cursorrules|\.claude/(agents|commands|skills|ways)/.*\.md|/agents/[^/]+\.md)$
scope: agent
---

## anchor

You are about to describe how work gets done here. Instructions that describe a process which cannot run are a premature primitive: they look like progress and deliver nothing.

## check

Before writing this:

- **Does what it describes run today?** The build command, the test command, the deploy path, the rollback. Name them. If you cannot, the instruction is describing a wish.
- **Is fixing the missing step the smaller change?** A working `make test` beats a paragraph telling the agent to test carefully.
- **Is this a workaround for something you are not permitted to fix?** Then the finding is the permission boundary. Report it; do not encode it.
- **Once it works, capture it.** Write it down the moment the process runs.
