---
description: code that runs on a system the agent cannot reach, such as a customer's self-hosted server, a scripting add-on inside a hosted product, a workflow automation server, or a remote browser, where the user deploys by hand and relays output; ship artifacts that report their own diagnostics, diff the deployed copy before changing code, respect the platform's edition and version, and treat the user's test as the gate
vocabulary: remote runtime cannot reach cannot access their environment customer instance client instance self-hosted on-prem data center server edition scripting add-on script console workflow automation webhook deployed script deployed copy paste output screenshot relay drift stale cache wrong version test script return value step status browser console log viewer plugin version syntax check walkthrough how the scripts relate
pattern: deployed (version|copy)|(can.?t|cannot) (reach|access) (the|their|that) (server|instance|environment)
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: convention -->
# Remote Runtimes

The code runs on a system you cannot reach. The user deploys it by hand and can only relay what they see. Every artifact you hand over has to report on itself, because you will not see it run.

## Make the Artifact Report Itself

- Hand over a ready-to-run script, not a description of one.
- Put step-tagged status in the script's return value, for example `{"step":"lookup-record","ok":false,"detail":"..."}`.
- Add light request and response logging to the browser console or the visible UI. Match any logging style the project already uses.
- Never tell the user to check a log viewer that does not exist in their environment. If the platform has no log viewer installed, send the output to the return value or the browser console.
- Work in a loop. The user runs it and pastes the output and screenshots. You analyze them and propose the next minimal change.

## Check Drift Before Code

When the user pastes the deployed copy, diff it against the local repo without editing anything. Report match or mismatch before you propose a fix. Suspect deployment drift, a webhook pointed at the wrong script version, or a stale cache before you blame the code.

## Respect the Platform

| Do | Don't |
|---|---|
| Use the docs for the edition and version they run | Cite the cloud or latest-release behavior |
| Use what is already installed | Propose a new plugin or add-on |
| Use the platform's own scripting language and built-ins | Borrow functions from another language that the platform lacks |
| Confirm the capability exists in that version | Assume a feature from a newer release |

Before you claim the data is missing, write a small test script that checks for it. Run it in the platform's script console, or with a local `curl` against an endpoint you can reach.

## Verification

Run a syntax check before handoff. A fix stays unverified until the user has tested it in their environment. Until they confirm, do not mark findings remediated, do not update findings files or config docs, and do not commit.

Preserve the working script. Put experiments in a new file, and keep each script to the simplest version that works.

## When They Ask What Things Do

When the user configures things themselves or asks what the scripts do, explain in chat what each piece does and how the pieces relate. Give step-by-step walkthroughs and plain commands, not opaque wrapper scripts.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "The code looks right, so the deploy must be wrong" | Diff the deployed copy first. Then you know. |
| "They can check the logs" | Confirm a log viewer exists there. If not, put the output in the return value. |
| "That function works in the cloud edition" | Check it in the edition and version they run. |
| "The syntax check passed, so it's fixed" | Only the user's test in their environment confirms it. |

## See Also

- environment/debugging(softwaredev) — the general debugging loop
- environment/hostparity(softwaredev) — a green result on your host says nothing about theirs
- environment/ssh(softwaredev) — when you can reach the remote host yourself
- code/testing/evaluation(softwaredev) — reproduce the failure as reported
