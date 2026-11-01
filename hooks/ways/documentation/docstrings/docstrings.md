---
description: adding doc comments or docstrings to code, such as JSDoc, TSDoc, Godoc, rustdoc and Python docstrings, whose sentences explain what the code does, and deciding which code needs one
vocabulary: docstring doc comment jsdoc tsdoc javadoc godoc pydoc rustdoc explain what it does exported public helpers google-style getters idiomatic self-evident
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: convention -->
# Docstrings Way

Match the docstring density of the surrounding code, in the language's idiomatic style:

| Language | Style | Example |
|----------|-------|---------|
| Python | Google-style docstrings | `"""Summary.\n\nArgs:\n    param: Description.\n"""` |
| JavaScript/TypeScript | JSDoc | `/** @param {string} name - Description */` |
| Rust | Doc comments | `/// Summary of the function.` |
| Go | Godoc | `// FunctionName does X.` |
| Shell/Bash | Header comment block | `# Description of what this script does` |

**When to write docstrings:**
- Public APIs, exported functions, classes, modules
- Complex internal logic where intent isn't obvious from the name
- Not needed for trivial getters, one-line helpers, or self-evident code

## See Also

- writing/plain(writing) — a comment states what the code does now, in plain sentences
