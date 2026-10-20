---
description: The user asks you to double check, verify first, or make sure before answering. Check current official docs for the exact product and version, confirm what was actually deployed or received, read the project's own reference before an external call, and test each claim against real evidence before you state it.
vocabulary: double check triple check verify first make sure are you sure don't assume current docs exact version edition model id actually deployed received confirm evidence hypothesis working example source unverified
pattern: (double|triple).?check|are you sure\b|verify (it |this |that )?first|search to (validate|verify|confirm)|(don.?t|do not) (just )?assume|which version (is|are|do)
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: convention -->
# Verify Before Claiming

A request to double check, verify first, or make sure means go back to the logs, scripts, and evidence before you answer. This matters most for debugging, final commits, and anything that goes to a client. "Triple check" means re-read the evidence. It does not mean restate the earlier conclusion. The reply still comes back short and plain.

## Learn external behavior from current docs

- Match the exact product, edition, and version. A self-hosted edition and the cloud edition of the same product often differ.
- Get model IDs, API parameters, and CLI flags from current vendor docs. Do not assume the version you remember is current.
- When the user names a new model or tool, use exactly that one, not a familiar older one.
- When the user says something exists or disputes a claim, search and check before you argue.
- A working example from the user outranks your assumptions. If their call works without a token, find out why before you insist it needs one.

## Read the reference before the call

Before an external API call, a production change, or a data ingest, read the project's own reference: the README, API notes, or playbook. Do not learn API behavior by trial and error against a live system. Put the same directive in subagent briefs.

## Confirm instead of assuming

- Confirm that CI deployed, that a rebuilt artifact really changed, and that a payment or message really arrived.
- When someone reports a problem, first check whether it is true. Name a bug's cause only after reading the code or the log. If the user challenges a claim, rule the alternative in or out with evidence.
- When you cannot check directly, write a small diagnostic script or query the user can run.
- Break a claim into separate statements and check each one. List what you could not verify. Never present unverified data, such as prices or availability, as real.
- For a claim about how an external system behaves, probe the real system before you build on it.

Docs, design notes, memory, and your own earlier statements are claims too. freshness/groundtruth(softwaredev) covers checking them against the executable artifacts.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "I checked this earlier in the session" | Things change. Re-read the evidence the user asked about. |
| "The docs I remember say X" | Your memory has a cutoff. Fetch the current page. |
| "The deploy command returned zero" | Exit status is not the live state. Check the running system. |
| "It's probably the same in this edition" | Editions diverge. Confirm the one in use. |

## See Also

- freshness/groundtruth(softwaredev) — prose is a claim, checked against code and config
- architecture/design/prototype(softwaredev) — probing an external system before deciding
- delivery/groundwork/claims(softwaredev) — reports and descriptions as claims to test
- environment/debugging(softwaredev) — finding the cause from evidence
