---
description: injection prevention — SQL injection, XSS, command injection; validate and escape untrusted input before it reaches a query, a shell, or an HTML context
vocabulary: injection sql xss innerHTML parameterized sanitize escape shell command untrusted input template interpolation eval exec
scope: agent, subagent
refire: 0.2
pattern: sql.?injection|command.?injection|\bxss\b|cross.?site.?scripting
---
<!-- epistemic: constraint -->
# Injection Prevention Way

## Detection and Action Rules

| If You See | Do This |
|------------|---------|
| String concatenation in SQL | Replace with parameterized queries |
| `innerHTML` with user input | Use `textContent` or sanitize |
| User input in shell command | Use parameterized execution, never string interpolation |
| Template string with unsanitized input | Escape for the output context (HTML, URL, SQL) |
| `eval()` or `exec()` with external input | Remove. Find a safer alternative. |
| Model output passed to a shell, query, or file call | Same boundary, same rule: validate first. See the prompt child way. |

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "This input comes from a trusted source" | Trust boundaries shift. Today's internal API becomes tomorrow's public endpoint. |
| "I'll sanitize at the boundary" | Defense in depth. Sanitize at every layer touching untrusted data. |
| "This is just a prototype" | Prototypes become production. Security debt compounds. |
| "The ORM handles it" | ORMs have raw query escape hatches. Verify you're using the safe path. |
| "It's only used internally" | Internal tools get exposed, shared, and repurposed. Secure by default. |

## See Also

- code/security/injection/prompt(softwaredev) — prompt injection, model output as untrusted input, tool hijacking, exfiltration
- code/security/guards(softwaredev) — the allowlist or schema an input is validated against
