---
id: 01.024.R
domain: ways
mode: reference
related:
  - "[[ADR-138]]"
  - "[[ADR-178]]"
  - "[[ADR-143]]"
  - "[[01.026.E]]"
aliases: []
---

# Contribution kinds and the evidence that helps review

The shared ways corpus accepts two kinds of contribution, ways and skills. Two other kinds are often offered as ways and belong somewhere else. This page defines the four kinds, gives the phrasing rule for a way, and lists the evidence that helps review each kind. Why contributions are welcome and what happens to one after review are in [Where ways come from](../explanation/where-ways-come-from.md). The steps for preparing a pull request are in [Contribute a way or skill](../how-to/contribute-a-way-or-skill.md).

## The four kinds

| Kind | Definition | Home | Example |
|---|---|---|---|
| Way | How the agent performs a kind of work when that work comes up. Triggers inject it just in time. | `hooks/ways/{domain}/{way}/{way}.md` | A way for multi-account work: when acting across several accounts, confirm which account and check its auth before the first action. |
| Skill | A procedure the agent runs on request ([ADR-138](../architecture/practice/ADR-138-skills-own-the-how-ways-own-the-5w.md)). | `skills/{name}/SKILL.md` | `/merge`: branch, commit, push, pull request, review gate, merge and cleanup, in that order. |
| Tool definition or resource | Knowledge about one tool: its parameters, limits and quirks. | The tool's own instructions, such as an MCP server's instructions or its tool descriptions | A calendar server's date format, its page size and the fields its search accepts. |
| Posture | A person's standing preferences, such as approval gates, register and closing habits. | That person's user-scope ways, `$XDG_CONFIG_HOME/agent-ways/ways/` ([ADR-143](../architecture/practice/ADR-143-three-root-way-runtime-core-user-project.md)) | "Ask me before sending any message on my behalf." "End each reply with a one-line summary." |

### A mis-filed example

A way titled "Using the calendar server" lists the server's tool names, its date format and its rate limit, and fires whenever a prompt mentions a meeting. Its content is tool knowledge. In the shared corpus it fires in installs that use a different calendar tool, and in installs that have the server it repeats what the server's own instructions already put in every session. The tool knowledge moves to the server's instructions. A practice that holds across calendar tools, such as confirming the time zone before scheduling across zones, can stay as a way written without the tool's names ([tool-agnostic](../../hooks/ways/meta/knowledge/authoring/tool-agnostic/tool-agnostic.md)).

## Phrasing a way

Write a way as how the work is done when the situation arises. Name the situation, then the action.

| Phrased as an outcome | Phrased as the work |
|---|---|
| "Ensures actions land on the right account." | "When acting across several accounts, confirm which account and check its auth before the first action." |
| "Produces a clean commit history." | "When committing, make one logical change per commit and give it a conventional type." |
| "Keeps the user informed." | "When a long-running command starts, say what it is and how long it usually takes." |

The agent reads a way at the moment the situation arises, so the way has to say what to do at that moment. An outcome leaves the steps to the agent. The named situation also gives the trigger words to match on.

## Evidence that helps review

| Kind | Evidence that helps review |
|---|---|
| Way | Probe results against held-out prompts and, when the effect is in doubt, an ablation |
| Skill | A run of the skill on a representative task showing it completes the task: its transcript or the end state it left |
| Tool definition or resource | None; it goes to the tool |
| Posture | None; it stays in the person's user-scope ways |
| Docs that are never injected | `docs/scripts/doc lint` |

This evidence informs the maintainer's decision. It is not a gate. Whether a contribution enters the corpus is the maintainer's judgment.

### Why evidence helps, and its limits

Every way in the shared corpus is a candidate in every session of every install. A way costs tokens when it fires and competes with nearby ways for every prompt in its region. Probe results show the reviewer whether its trigger fires on everyday phrasing and whether it takes prompts from existing ways. An ablation shows whether behaviour changes when the way is present.

The judgment that a way would help brings in variety from outside the guidance system: the person who writes it is predicting where the agent's default falls short. A benchmark of typical scenarios rarely contains the atypical situations a way is written for. No measured effect on typical prompts is therefore weak evidence against a way meant for an atypical one.

### The ablation for a way

`make test-live-matrix` runs the tier 2 live fixture over several arms ([ADR-508](../architecture/platform/ADR-508-the-live-fixture-runs-locally-its-github-workflow-runs-on-manual-dispatch-only.md)). Each arm is a clone of this repository. A way's ablation uses three:

| Arm | Contents | What it measures |
|---|---|---|
| Without | The base branch, without the way | Baseline behaviour |
| With | The contribution branch | Behaviour with the way available to its trigger |
| Noise floor | The base branch again, under a second arm name | Run-to-run spread; a difference smaller than the spread between the two identical arms is no finding |

When the way's trigger may not be firing in the scenarios, add a forced arm. It is a branch with a commit that gives the way `trigger: session-start`, so the way is present in every scenario. Comparing it with the without-arm shows whether the text changes behaviour, separately from whether its trigger fires. The forced arm is a manual, committed branch, not a driver option.

The scenarios are held out: someone other than the way's author writes them from a one-sentence statement of the situation, without the way's wording ([evaluative/held-out](../../hooks/ways/evaluative/held-out/held-out.md)). Grading the transcripts is manual and follows [evaluative/ablation](../../hooks/ways/evaluative/ablation/ablation.md): a blind judge grades them without knowing which arm produced them and with the way's injected text stripped.
