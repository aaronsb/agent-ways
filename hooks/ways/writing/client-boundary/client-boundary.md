---
description: building or checking an outbound document for a client, customer, partner or the public from internal notes, a pasted chat thread or working files; find what they shouldn't see and leave out internal margins, costs, staffing, stakeholder quotes and another organization's material, anonymize people and names when publishing, and state limitations and risks plainly in what remains
vocabulary: client customer outbound public share send publish case study summary draft internal notes slack thread pasted shouldn't see should not see ok to share confidential sanitize anonymize redact margin margins pricing costs staffing headcount stakeholder quote engagement other client employer limitations risks rate card team channel complaints recap
pattern: client.?facing|customer.?facing|\bnda\b|case stud(y|ies)|llms\.txt|make (the )?repo public
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: constraint -->
# Internal Material in Outbound Documents

When building a document a client, partner or the public will read from internal material, decide what stays internal before drafting. Build the outbound copy from a sanitized version of the source, never straight from raw notes or a pasted thread.

## Stays internal

| Material | In the outbound copy |
|---|---|
| Stakeholder quotes | The finding, unattributed |
| Headcounts and per-person figures | A share or percent, when the point needs a number |
| Margin, internal costs, hours, pricing not yet offered, internal deadlines and staffing | Omit |
| Another client's or employer's names, material and code | Omit |
| Internal tool names and mechanism terms | The outcome, plainly |
| Scheduling or capability not yet confirmed | Only what the user has confirmed |

Review the whole draft against this table before it goes out, including anything pasted in from chat or email. Run the project's own document checks if it has them.

## Limitations stay in

Leaving out confidential material is a confidentiality call. It never extends to framing. Known gaps, risks and open questions that bear on the reader's decisions are stated plainly, as the writing way's proposal row requires. Do not recast a limitation as a strength or a risk as a next step. Where an exclusion changes what the reader can rely on, such as a scope cut, say what is out of scope.

## Publishing

For a case study, an article, a public issue, site copy, `llms.txt` or a repository made public:

- Remove or anonymize people, client names and private sources unless the user says otherwise. Get consent before a client or the public sees an identifiable person, photos included.
- Scan for secrets, personal emails, webhooks and usernames, and replace user-specific configuration with placeholders (code/security/secrets).
- Do not attribute motives to real people.

## Keep organizations apart

When the user works for more than one organization, keep each one's material out of the others' documents and repositories. Strip references to the other organization when porting code between them.

## See Also

- writing(writing) — parent: audience, genre, and stating limitations
- research/provenance(research) — the open-items list stays with the author
- code/security/secrets(softwaredev) — credential scanning before anything is shared
- knowledge/authoring/pii-free(meta) — the same stripping applied to way content
- trust/delegation(meta) — outbound artifacts carry the user's name
