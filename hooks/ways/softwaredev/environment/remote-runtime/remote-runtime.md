---
description: changing code that runs on a system you cannot reach, such as a customer's server, a hosted product's script console, a cloud function in an account you cannot access, or an air-gapped box, while the user runs it and pastes back the output; compare the copy running there with the local source before changing anything, make the code report its own status, and call the fix unverified until the user's run confirms it
vocabulary: cannot reach no access can't give you the account no credentials lambda cloud function cannot access their environment customer server client instance self-hosted on-prem air-gapped hosted product script console remote machine runs only there i'll paste the output paste back screenshot relay edition step status return value console log debug blind
pattern: deployed (version|copy)|(can.?t|cannot) (reach|access) (the|their|that) (server|instance|environment|box|machine)|air.?gapped
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: convention -->
# Remote Runtimes

The code runs where you cannot reach it. The user runs it and relays what they see. You get one look per round trip, so each round has to carry as much evidence as it can.

## Compare their copy before changing code

When the code fails on their system, ask the user to paste the copy that is actually running there. Diff it against the local source without editing anything, and report match or mismatch first. A pasted copy that differs from the source, a trigger pointed at an old script, or a stale cache explains many failures that look like code bugs. Change the code only after the copy you are changing is known to be the one that runs.

## Make the code report its own status

- Hand over code that runs as given, not a description of it.
- Have it report each step's outcome where the user can see it: the return value, the script console, the browser console, or the visible UI. A step-tagged result such as `{"step":"lookup-record","ok":false,"detail":"..."}` names where it stopped.
- Send output to a place that exists on their system. Do not point the user at a log viewer they may not have.
- Before claiming data or a capability is missing, write a small check that tests for it and have the user run it.

## Match their platform

Work from the documentation for the edition and version they run, not the latest release. Use what is already installed there, in the platform's own language. Confirm a feature exists in their version before relying on it.

## Report it unverified until they run it

A syntax check or a passing local test shows the code parses and runs here. Report a fix as unverified on their system until the user confirms it worked there, and do not mark a finding resolved before then (environment/hostparity). Keep the last working version where the user can restore it, and put experiments in a separate copy.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "The code looks right, so their deploy must be wrong" | Diff the deployed copy first. Then you know. |
| "They can check the logs" | Confirm the log viewer exists there. Otherwise put the output in the return value. |
| "That function exists in the current release" | Check the edition and version they run. |
| "The syntax check passed, so it's fixed" | Only their run confirms it. Report it unverified until then. |

## See Also

- environment/debugging(softwaredev) — the general debugging loop
- environment/hostparity(softwaredev) — a green result on your host says nothing about theirs
- environment/ssh(softwaredev) — when you can reach the remote host yourself
