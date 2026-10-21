---
description: Writing, trimming or refreshing the CLAUDE.md or AGENTS.md file that loads for every session in a repository, keeping only what every session needs and cannot infer from the repository, sending conventions for one kind of work to project ways, and keeping status and history out of the file
vocabulary: claude.md agents.md guidance file at the root of the repo project instructions file loaded every session what belongs too long bloated trim prune stale agents ignore it forget how to run tests conventions commands gotchas
pattern: \b(claude|agents)\.md\b
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: convention -->
# Instruction Files

An instruction file such as CLAUDE.md or AGENTS.md loads into every session that opens in its directory. Each line costs context in every one of those sessions, so a line earns its place only when it saves the agent work it could not do from the files alone.

## What goes where

| Content | Home |
|---|---|
| How to build, test and run the project, and where outputs land | The instruction file |
| Facts that took research to find: ignored directories, IDs, external locations | The instruction file |
| A convention that applies to one kind of work, such as migrations or releases | A project way, which fires when that work comes up |
| A procedure run on request | A skill |
| Open work, session state, recaps | Tasks and issues, and the wrap skill at handoff |
| History and status narrative | Version control and the issue tracker |

## Check before writing

Run each command an instruction describes and confirm it works today (delivery/groundwork). Where fixing the step is smaller than documenting a workaround, fix the step. When observed behaviour contradicts the file, say so in one line and offer to fix the file or the script.

## Keep it lean

| Do | Don't |
|----|-------|
| Keep only what an agent cannot infer from the repository | Append status, dated update logs or file counts |
| Remove narrative when trimming | Drop existing rules when trimming |
| Keep personal names and credentials out of shared files | Copy a repository fact into cross-project memory |

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "A changelog section helps the next agent" | It grows every session and goes stale. Git and the tracker hold history. |
| "I'll document the step now and test it later" | An untested step reads as fact. Test it first. |
| "More context is safer" | Every line costs context in every session. Keep only what the agent cannot infer. |
| "I'll trim the old rules while I'm here" | Trimming removes narrative. Keep the rules. |

## See Also

- delivery/groundwork(softwaredev) — instructions describe only processes that run today
- knowledge/authoring(meta) — writing a project way for a convention
- skills(meta) — choosing between a skill and a way
- readme(documentation) — the human front door, distinct from agent instructions
