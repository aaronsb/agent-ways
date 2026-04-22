---
description: source code security audit for dangerous patterns, obfuscation, exfiltration
vocabulary: eval exec obfuscated base64 pickle deserialize exfiltration shell injection subprocess os.system innerHTML dangerous pattern code audit source review
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: heuristic -->
# Source Code Audit

Scan for dangerous patterns before running unfamiliar code. Language-agnostic — the patterns show up everywhere.

## What to Grep For

| Pattern | Risk | Quick check |
|---------|------|-------------|
| `eval()` / `exec()` | Arbitrary execution | `grep -rn 'eval\|exec'` |
| `base64` decode | Obfuscated payloads | `grep -rn 'b64decode\|atob\|base64'` |
| `pickle` / `marshal` | Deserialization RCE | `grep -rn 'pickle\|marshal\|shelve\|yaml.load'` |
| `shell=True` / `os.system` | Shell injection | `grep -rn 'shell=True\|os\.system\|os\.popen'` |
| Outbound HTTP in setup | Data exfiltration | `grep -rn 'requests\.\|urllib\|fetch(' setup.py __init__.py` |
| Single-char variables | Intentional obfuscation | Clusters of `a=`, `b=`, `x=` with hex/encoded strings |

## Context Matters

Not every `eval()` is malicious. A template engine uses eval. A REPL uses exec. What's suspicious is eval/exec in:
- `setup.py` or `__init__.py` (runs on import/install)
- `postinstall` scripts (runs on `npm install`)
- Code that decodes a string then executes it
- Files with no obvious connection to the project's purpose
