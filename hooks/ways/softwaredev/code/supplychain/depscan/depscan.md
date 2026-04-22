---
description: dependency vulnerability scanning, lockfile auditing, package security
vocabulary: osv-scanner pip-audit npm audit cargo audit govulncheck dependency scan vulnerability CVE lockfile requirements package-lock Cargo.lock go.sum SBOM
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: heuristic -->
# Dependency Scanning

Scan lockfiles before installing. The install step itself runs arbitrary code.

## Tool Selection

Pick the right tool for the job. Prefer tools that are already available or trivial to install.

| Tool | Ecosystems | Install | Notes |
|------|-----------|---------|-------|
| **osv-scanner** | All (Python, Node, Go, Rust, Java, ...) | `go install github.com/google/osv-scanner/cmd/osv-scanner@latest` | Google-backed, queries OSV.dev, best all-rounder |
| **pip-audit** | Python | `pip install pip-audit` | PyPA official, uses PyPI advisory DB |
| **npm audit** | Node | built-in | Ships with npm, zero install |
| **cargo audit** | Rust | `cargo install cargo-audit` | RustSec advisory DB |
| **govulncheck** | Go | `go install golang.org/x/vuln/cmd/govulncheck@latest` | Official Go team tool |

**Start with osv-scanner** if you're not sure — it handles multiple ecosystems from one tool.

## Scan Before Install

```bash
# Scan the lockfile, not the installed packages
osv-scanner --lockfile=requirements.txt
osv-scanner --lockfile=package-lock.json

# NOT this order:
# pip install -r requirements.txt  ← too late, setup.py already ran
# pip-audit                        ← scanning after the fact
```

## What the Results Mean

Not every CVE is a showstopper. Check:
- **Does this project actually use the vulnerable code path?**
- **Is there a patched version available?** Update if so.
- **Is it a dev dependency?** Lower risk than a runtime dependency.
- **Is the severity critical/high?** Prioritize those.
