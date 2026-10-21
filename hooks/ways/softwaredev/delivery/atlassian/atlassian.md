---
description: moving a Jira ticket to its next status with a comment, or editing a page in a Confluence space, in a shared project through the REST API or a connector; touch only the tickets you own, close a ticket with a comment that links its supporting pages, publish a page under the parent page the user names, and check the rendered result before calling it done
vocabulary: jira confluence atlassian ticket tickets ticket key move to in review transition ticket status comment linking the pr wiki page confluence space update the page parent page shared project owned keys bulk update notifications relabel rendered page rest api connector
pattern: \bjira\b|\bconfluence\b|atlassian|\bjql\b
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: convention -->
# Jira and Confluence

A tracker or wiki write is visible to everyone in the project and is attributed to the user. Work so each change is scoped, traceable, and checked where people will read it.

## Read the API before calling it

Before the first REST call in a session, read the endpoint's documentation and any API notes the project keeps. Know the pagination and the response shape. Do not learn the API by trial writes. Pass the same instruction to any subagent that will call it.

## Touch only what you own

- In a shared project, keep the list of ticket keys this work owns and act only on those. Never run a bulk change across the whole project key.
- Leave tickets unassigned unless the user says otherwise.
- Suppress notifications on a bulk change.
- Relabel or move an existing ticket instead of creating a duplicate.
- Report any change to a date or scope to the user, so they can announce it to the people who track it.

## Closing a ticket

1. Post a comment that says what was done and links every supporting page, pull request, or document.
2. Transition the ticket to its next status.
3. Give the user the ticket URL.

A bulk close-out still gets a full comment on each ticket.

## Publishing a page

- Publish under the parent page the user names, and follow the space's existing page conventions.
- Send the body in the format the API expects for rich content, not raw markdown pasted as text.
- A successful response does not prove the page reads correctly. Open the rendered page, or fetch it back, and check the formatting before calling it done.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "The API returned 200, so the page is fine" | Connectors and format mismatches mangle bodies. Check the rendered page. |
| "A project-wide update is simpler than tracking keys" | A shared project holds other teams' work. Act only on owned keys. |
| "The transition is enough, the comment is extra" | The comment is the record of what closed it. Link the evidence. |

## See Also

- delivery/issues(softwaredev) — GitHub issues as the session task list
- trust/delegation(meta) — tracker and wiki writes are attributed to the human
