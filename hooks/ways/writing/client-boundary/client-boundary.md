---
description: Keeping internal material out of client-facing and public artifacts. Covers INTERNAL_ and CLIENT_ file prefixes, what never reaches a client, sanitizing names and client scope before sharing or publishing, keeping separate clients and employers apart, and never revealing that anything was filtered or excluded.
vocabulary: client-facing internal sanitize anonymize publish public share external partner case-study deliverable exclude scope headcount margin quotes nda confidential stakeholder engagement consultancy redact
files: (^|/)(CLIENT_|INTERNAL_)[^/]*$
pattern: client.?facing|\bsanitiz|anonymi[sz]|\bnda\b|client edition|case stud(y|ies)|llms\.txt|make (the )?repo public|stakeholder quote|quotes (of|from) stakeholders
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: constraint -->
# Internal Versus Client

Anyone who works for clients keeps two kinds of material. One is internal working notes. The other is what a client or the public will read. This way keeps them apart.

## Mark files by audience

- A filename prefix is a simple convention. `INTERNAL_` files never reach the client. `CLIENT_` files are deliberately client-facing. Unmarked files count as internal.
- Build `CLIENT_` files only from sanitized client editions, never from raw internal notes or registers.
- Review each file before a client drop. Run the project's client-document linter if it has one.

## Never in a client artifact

| Exclude | Use instead |
|---|---|
| Stakeholder quotes | The finding, unattributed |
| Raw headcounts, per-person dollar figures | Percent of the department |
| Exact dates (external audiences only) | Relative timeline |
| Hours, margin, costs, tokens, internal deadlines, resourcing, uncommitted commercial topics | Omit |
| Risk or concern language in next steps | Plain next actions |
| "Known Gaps", "Critical Files", file paths in sales documents | Strengths and configurability |
| Internal mechanism terms, such as manifest, canary, runbook, or guard hook | The outcome, plainly |
| Unconfirmed scheduling, automation still being proven | Only what the user has confirmed |

## Leave no trace of filtering

- Never note what was excluded. Never label anything "Client Edition".
- When the user removes something from scope, purge every trace of it, exclusion notes included. Then rebuild every derived output.

## Framing

- Be honest but favorable, with simple arguments that are hard to dispute. Do not boast. Internal tools are not "products".
- Disclose AI involvement only at the level the user has approved, in plain bullets.
- Notes for technical partners get the mechanism exactly right. Client incident notes stay short.

## Public or shared artifacts

This covers GitHub issues, articles, case studies, site copy, `llms.txt`, and shared repos.

- Remove or anonymize people, client names, private sources, and architecture unless told otherwise.
- Scan for secrets, personal emails, webhooks, and usernames. Replace user-specific config with placeholders.
- Do not attribute motives to real people. Ask what a procurement contact at each company would support saying.
- Sanitize internal thinking documents only when asked.

## Keep the orgs apart

- When the user works for more than one org, such as an employer and separate clients, keep each one's material off the others' sites and out of the others' deliverables.
- Strip references to the other client when porting code between client repos.
- Material mined from another repo is inspiration, not truth. This project's own design docs win. Report what you borrowed without naming the repo, client, use case, or code.

## Facilitators and people

- Answer keys, twists, and passwords for a workshop or game stay in the internal repo until the planned release. No routine push should be able to leak them.
- Get consent before a client sees identifiable people, photos included.

## See Also

- code/security/secrets(softwaredev) — credential scanning before anything is shared
- knowledge/authoring/pii-free(meta) — the same stripping applied to way content
- trust/delegation(meta) — outbound artifacts carry the user's name
