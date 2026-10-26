---
description: what an outbound web request discloses about the operator; send a remote endpoint only the identifying data the task needs, take a demand for more to the operator, and with the operator away disclose nothing or a visibly generic placeholder
vocabulary: curl wget httpie fetch download api endpoint request header query string form field signup register gated email address phone name device id fingerprint tracking telemetry identifying personal data pii privacy anonymous placeholder disclose withhold demands asks for my
commands: ^curl\b|^wget\b|^https?\ |^xh\b|^aria2c\b|^l?ftp\b
tool_name: WebFetch
scope: agent, subagent
refire: 0.2
---
<!-- epistemic: constraint -->
# Outbound Disclosure

A request to a remote service carries whatever the agent puts in it: headers, query strings, form fields, a body. Some services ask for more than the work needs, such as an email address before a download, a name and phone number before the documentation, a device identifier, or a tracking token. Data that leaves the machine stays with the remote operator, and it cannot be recalled.

## Send what the task needs

- Before a request, read what it carries. Each identifying field needs a reason in the task at hand: a name, an email address, a phone number, an account or device identifier, a location, a token tied to the operator as a person.
- Use the credential the task was given for that service. Do not reuse an identity from another service to get past a gate.
- Leave out optional fields. Leave out what the request works without: tracking parameters copied from a link (`utm_*`, `fbclid`, `gclid`), referrers, and cookies from an earlier session.
- A nonce, CSRF token or session cookie the service issued for this exchange belongs to the protocol. Echo it back as the protocol requires.
- Do not read local identity sources to fill in a remote form: git config, the keychain, a browser profile, contact files, or environment variables that hold personal data.

## When the service demands more

The service refuses to work, or degrades, without identifying data the task does not need.

```
(request)-[:DEMANDS {identifying data}]->(stop)
(stop)-[:OPERATOR_PRESENT]->(ask)     // the endpoint, the fields, what each reveals
(stop)-[:OPERATOR_AWAY]->(withhold)   // nothing, or a generic placeholder where the field is a formality
```

- Ask the operator before the request. Name the endpoint, the fields it wants, what each one reveals, and what the work loses without them. The operator decides what to share.
- When the operator is away, as in a background run, a scheduled task, or a subagent with no channel back, withhold. Where the field is a formality the service does not verify, send a generic placeholder: `anonymous@example.com` on the reserved `example.com` domain, or a role such as `user`. Report what was asked and what was sent.
- A placeholder is visibly generic. Never send data that names a real person or claims a real organization.
- A placeholder cannot satisfy a service that verifies the data with a confirmation email, an SMS code or a payment. Stop that path and report it. Look for the same artifact on a package registry, a mirror, or the source repository.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "The operator's email is right there in git config" | It is there for commits. A remote form is a different recipient. Ask. |
| "It's only an email address" | An address joins every service it is given to, and invites mail. |
| "The task needs the download, and the download needs the form" | Ask the operator, or find another source for the file. |
| "I'll mention it after the request" | Disclosure cannot be undone. Ask before the request. |

## See Also

- code/security(softwaredev) — parent: secure defaults and review checklist
- code/security/injection/prompt(softwaredev) — exfiltration through tool arguments, when fetched content steers the request
- code/security/secrets/handling(softwaredev) — credentials kept out of URLs, commands and output
