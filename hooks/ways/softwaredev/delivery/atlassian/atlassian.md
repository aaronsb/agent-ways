---
description: working in Jira and Confluence through their REST APIs or a connector, reading the API reference before calling it, keeping a production site read-only behind a guard hook, touching only owned items in a shared project, closing tickets with linked comments, publishing pages in Atlassian Document Format, and writing a client delivery backlog
vocabulary: jira confluence atlassian ticket issue transition close comment page publish adf atlas_doc_format toc macro jql backlog epic story sprint sow sandbox production prod runbook guard hook read-only owned keys shared project critical path dependencies rest api v2 v3
pattern: \bjira\b|\bconfluence\b|\badf\b|atlas_doc_format|\bjql\b|atlassian
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: constraint -->
# Jira and Confluence

## Read the reference first

Before any Atlassian REST call, read the project's API notes and the endpoint's documentation. Know the pagination, the response shape, and quirks such as the JQL search endpoint returning no total count. Do not learn API behavior by trial and error. Put this same directive in every subagent prompt.

## Client production is off limits

- Do recon with read-only GET calls. Build in a sandbox site.
- Production changes belong to a human working from a written runbook with verification URLs.
- Enforce this in the harness, not with instructions alone. A PreToolUse hook can deny the production host and any PUT, DELETE, PATCH, or POST outside the allowed search endpoints. Never edit a guard hook unless its test matrix passes.
- Allow a production exception only when the user scopes it explicitly, such as "the project backlog and nothing else in prod". Leave no story partly complete. Roll back cleanly if blocked. Give before and after URLs, and record how the change was made.

## Shared client projects

Keep a list of the item keys you own. Operate only on those items, never on the whole project key. Leave items unassigned unless told otherwise, and suppress notifications on bulk changes. Announce any re-date or rescope openly, for example at the next status meeting. Relabel items instead of re-importing them.

## Closing a ticket

1. Post a well-formatted comment that links every supporting Confluence page. Jira Cloud's v3 API takes comments in ADF.
2. Transition or close the ticket.
3. Give the user the ticket URL so they can check it.

Bulk close-outs still get a full comment on each ticket.

## Confluence pages

- Send the body in `atlas_doc_format` JSON, not raw markdown or HTML.
- If a connector wraps the body as plain text, call the Confluence REST API v2 directly with the `atlas_doc_format` representation.
- Follow the space's page conventions, such as a table of contents macro at the top.
- Publish under the parent page the user names. Check the rendered page before calling it done.

## Client backlogs

For multi-week delivery plans:

| Do | Don't |
|----|-------|
| Classic story format that is simple, transparent, trackable, and committable | Overstuffed design content |
| Map dependencies and the critical path | Hour estimates |
| Spread completion dates across the delivery period | Name internal assignees (assign the team generically) |
| Tie each item in plain terms to the statement of work and its business value | Per-person resourcing models |

Same-day or event-day execution plans are the exception. Those still get per-stage time estimates.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "One quick fix in prod is faster than the runbook" | Production belongs to the human with the runbook. Ask for an explicit scope. |
| "The API call returned 200, the page is fine" | Connectors mangle bodies. Open the rendered page. |
| "Editing the whole project is simpler than tracking keys" | Shared projects hold other teams' work. Touch only owned keys. |

## See Also

- delivery/github(softwaredev) — the same review discipline on GitHub
- trust/delegation(meta) — tracker and wiki writes are attributed to the human
