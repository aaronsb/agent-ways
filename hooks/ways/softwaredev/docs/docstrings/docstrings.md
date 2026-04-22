---
description: code documentation, docstrings, JSDoc, Godoc, rustdoc, inline comments
vocabulary: docstring jsdoc godoc pydoc rustdoc comment annotation type hint documentation
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: convention -->
# Docstrings Way

Use docstrings in every language, following the idiomatic style:

| Language | Style | Example |
|----------|-------|---------|
| Python | Google-style docstrings | `"""Summary.\n\nArgs:\n    param: Description.\n"""` |
| JavaScript/TypeScript | JSDoc | `/** @param {string} name - Description */` |
| Rust | Doc comments | `/// Summary of the function.` |
| Go | Godoc | `// FunctionName does X.` |
| Shell/Bash | Header comment block | `# Description of what this script does` |

**When to write docstrings:**
- Public APIs, exported functions, classes, modules — always
- Complex internal logic where intent isn't obvious from the name
- Not needed for trivial getters, one-line helpers, or self-evident code
