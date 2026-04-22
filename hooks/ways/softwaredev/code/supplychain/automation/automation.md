---
description: security scanning automation, GitHub Actions, Dependabot, CodeQL, Makefile audit targets
vocabulary: github action dependabot codeql security scanning automation ci pipeline sbom scorecard make audit workflow security policy
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: convention -->
# Scanning Automation

Match the scanning approach to the project's maturity. Don't propose a CodeQL pipeline for a repo without CI, and don't suggest manual `osv-scanner` for a team project with GitHub Actions already running.

## Pick the Right Level

| Project state | Scanning approach |
|---------------|-------------------|
| Just exploring / solo | Run `osv-scanner` or `pip-audit` manually when it matters |
| Has a Makefile | Add `make audit` wrapping the right scanner |
| Has CI (GitHub Actions) | Add a scanning workflow |
| Team with Dependabot | Let Dependabot handle updates, add CodeQL for code scanning |
| Needs compliance artifacts | Add SBOM generation to release workflow |

## Makefile Target

The simplest automation — a `make audit` target:

```makefile
audit: ## Run dependency vulnerability scan
	osv-scanner --lockfile=requirements.txt
	# or: pip-audit -r requirements.txt
	# or: npm audit
```

## GitHub Actions

Free, open source actions that work well:

```yaml
# .github/workflows/security.yml
name: Security Scan
on: [push, pull_request]
jobs:
  scan:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: google/osv-scanner-action@v1  # all ecosystems
      # or: - uses: pypa/gh-action-pip-audit@v1  # Python
```

## Dependabot

For repos on GitHub — zero config, just add the file:

```yaml
# .github/dependabot.yml
version: 2
updates:
  - package-ecosystem: pip    # or npm, cargo, gomod, etc.
    directory: /
    schedule:
      interval: weekly
```

Dependabot opens PRs for outdated/vulnerable deps. Review them; don't auto-merge blindly.

## When to Escalate

- **Manual → Makefile**: When you find yourself running the same scan command repeatedly
- **Makefile → GitHub Action**: When the project has collaborators or you want CI gates
- **Action → Dependabot/CodeQL**: When the project is production-facing and needs continuous monitoring
- **SBOM generation**: When someone asks for a software bill of materials (compliance, procurement)

Don't skip levels. Each level assumes the one below it is working.
