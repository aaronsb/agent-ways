---
description: writing and refreshing agent instruction files such as CLAUDE.md and AGENTS.md navigation hubs, using a fixed per-directory template, keeping them lean with no status narrative, recording the IDs and locations that save rediscovery, describing only commands that run today, and syncing them before a commit
vocabulary: claude.md agents.md instruction file navigation hub directory map per-directory hub contents table subdirectories relationships conventions warnings reorg reorganize refresh init context engineering too long bloated stale instructions agent onboarding project memory pruning status narrative update log
pattern: claude\.md|agents\.md|directory map|navigation hub
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: convention -->
# Instruction Files

An instruction file such as CLAUDE.md or AGENTS.md is the first thing an agent reads in a directory. It is worth its context cost only when every line saves the agent work it could not do from the files alone.

## Check Before Writing

- Name the commands an instruction describes and confirm they run today. Do not document a step before it has been tested.
- If fixing the step is smaller than describing a workaround, fix the step.
- When observed behavior contradicts an instruction file, say so in one line and offer to fix the file or the script.

## Hub Template

Give every meaningful directory a hub with the same sections:

| Section | Holds |
|---|---|
| Purpose | One line |
| Contents | A `file | purpose` table |
| Subdirectories | Each child directory and what it holds |
| Relationships | What this directory feeds or depends on |
| Conventions | Hand-written rules for working here |
| Warnings | Dated gotchas, stale pointers, and supersession notes |

The root hub adds a directory map table. Code repos may add the tech stack, development commands, and deploy steps.

## Keep It Lean

| Do | Don't |
|----|-------|
| Stay short, roughly 200 lines, holding only what an agent cannot infer | Append status narrative or dated update logs |
| Move the chronicle to a log file and procedures to skills | Record file counts or other details that go stale |
| Record what saves rediscovery: gitignored directories, IDs that took research to find, where outputs land | Drop existing rules when pruning |

Record key facts about the repo itself in its instruction file. Copy them to cross-project memory only when other projects need them.

## When to Refresh

- Generate hubs (with `/init` or a hub-generation skill) on any repo that has none.
- At a stopping point, after a reorganization, or at the end of a deliverable cycle, propose a refresh. Check the root directory map once it runs.
- Before a commit, bring the affected hubs and the root map to the final working state.
- For a new repo, set up a light structure first (requirements, raw materials, docs, outputs), then generate the hubs.

## Skills and Way Files

Update a skill only after the new method has worked. Finish the task first, then record the most token-efficient pattern that worked. For skill conventions see skills(meta), and for way files see knowledge/authoring(meta). Keep personal names and IDs out of shared instruction files.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "A changelog section helps the next agent" | It grows every session and goes stale. Put history in a log file. |
| "I'll document the deploy step now and test it later" | An untested step reads as fact. Test it first. |
| "More context is safer" | Every line costs context in every session. Keep only what the agent cannot infer. |
| "I'll trim the old rules while I'm here" | Pruning removes narrative, not rules. Keep the rules. |

## See Also

- delivery/groundwork(softwaredev) — instructions describe only processes that run today
- skills(meta) — writing skills and choosing between a skill and a way
- knowledge/authoring(meta) — authoring way files
- readme(documentation) — the human front door, distinct from agent instructions
