---
description: Go dependency security, govulncheck, module verification, replace directives
vocabulary: govulncheck go.sum go.mod replace directive go install go get module proxy checksum
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: convention -->
# Go Dependency Security

## Scanning

```bash
# Official Go team tool — only reports vulns in code paths you actually call
govulncheck ./...

osv-scanner --lockfile=go.sum
```

## Go-Specific Risks

- **`replace` directives in `go.mod`** can point dependencies to arbitrary local paths or forks. Check for unexpected replacements.
- **`go generate` runs arbitrary commands.** `//go:generate` comments in source files execute whatever follows. Review before running `go generate`.
- **Go's checksum database (sum.golang.org)** provides transparency — modules can't be silently changed after publication. This is a genuine supply chain advantage.
- **`go install` from a URL** downloads and compiles in one step. Know the source.
