---
description: dependency management, package installation, library evaluation, security auditing of third-party code
vocabulary: dependency package library install upgrade outdated audit vulnerability license bundle npm pip cargo
pattern: dependenc|package|library|npm.?install|pip.?install|upgrade.*version
commands: npm\ install|yarn\ add|pip\ install|cargo\ add|go\ get
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: heuristic -->
# Dependencies Way

## Before Adding a Dependency

Pause and check:

| Question | How to Check |
|----------|-------------|
| Do we really need this? | Could we write it in <50 lines? |
| Is it maintained? | `npm info <pkg>` or `gh repo view <org/repo>` — last publish, open issues |
| How big is it? | `npm pack --dry-run <pkg>` for size |
| What's the license? | `npm info <pkg> license` |
| Is it trivial? | Don't add packages for `is-odd`, `left-pad`, etc. |

## When Updating

- `npm outdated` / `pip list --outdated` to see what's behind
- Read the changelog before updating — check for breaking changes
- Update one package at a time when debugging compatibility
- Run tests after each update

## Security

- `npm audit` / `pip-audit` / `cargo audit` after adding or updating
- Don't ignore vulnerability warnings — fix or document the exception
- Flag dependencies more than 2 major versions behind

## See Also

- code/supplychain(softwaredev) — security scanning for dependencies
- code/supplychain/depscan(softwaredev) — automated vulnerability scanning
