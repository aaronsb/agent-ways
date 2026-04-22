---
description: supply chain security, repository trust assessment, evaluating code from untrusted sources
vocabulary: supply chain trust assessment forked repo untrusted repo audit repo hygiene dependency scan vulnerability malicious backdoor provenance clone fork grab use try found shared unfamiliar external third party
commands: git\ clone
scope: agent, subagent
curve:
  type: Exponential
  half_life: 200000
---
<!-- epistemic: premise -->
# Supply Chain Trust Assessment

Scan before you run. Don't install dependencies or execute code from an unfamiliar source until you've looked at it.

## Assessment Tiers

Work from fast and cheap to slow and thorough. Most repos only need the first two.

| Tier | What | Time | When |
|------|------|------|------|
| 1. Repo audit | Git history, size, leaked secrets | seconds | Any unfamiliar repo |
| 2. Source audit | Dangerous code patterns | minutes | Before running anything |
| 3. Dep scan | Known vulnerabilities in dependencies | minutes | Before installing |
| 4. Automation | CI/Makefile scanning integration | varies | Established projects |
| 5. History sever | Flatten or delete tainted history | minutes | When history is the threat |

## Principles

- **Scan before you run.** `pip install` and `npm install` execute arbitrary code. Scan first.
- **Containers aren't a security boundary.** A malicious setup.py in Docker still has network access.
- **Match the tool to the project.** Manual `osv-scanner` for a hobby project. GitHub Actions for a team repo. Don't skip levels, don't overbuild.
- **Responsible disclosure over silence.** If you find leaked secrets in someone else's repo, report it — masked values, remediation hints, not a public callout.

## See Also

- code/security(softwaredev) — supply chain is a security concern
- code/supplychain/depscan(softwaredev) — scanning dependencies
- code/supplychain/sourceaudit(softwaredev) — auditing source before execution
- environment/deps(softwaredev) — dependency management workflow
