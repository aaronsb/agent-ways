---
description: injection prevention, SQL injection, XSS, command injection, input sanitization
vocabulary: injection sql xss innerHTML parameterized sanitize escape shell command template interpolation
scope: agent, subagent
refire: 0.2
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

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "This input comes from a trusted source" | Trust boundaries shift. Today's internal API becomes tomorrow's public endpoint. |
| "I'll sanitize at the boundary" | Defense in depth. Sanitize at every layer touching untrusted data. |
| "This is just a prototype" | Prototypes become production. Security debt compounds. |
| "The ORM handles it" | ORMs have raw query escape hatches. Verify you're using the safe path. |
| "It's only used internally" | Internal tools get exposed, shared, and repurposed. Secure by default. |
