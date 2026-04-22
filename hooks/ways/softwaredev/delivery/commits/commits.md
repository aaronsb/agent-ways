---
description: git commit messages, branch naming, conventional commits, atomic changes
vocabulary: commit message branch conventional feat fix refactor scope atomic squash amend stash rebase cherry
pattern: commit|push.*(remote|origin|upstream)
commands: git\ commit
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: convention -->
# Git Commits Way

## Conventional Commit Format

Scopes match the area of change: `ways`, `hooks`, `adr`, `docs`, `config`, `governance`, or the specific way/feature name.

- `feat(scope): description` - New features
- `fix(scope): description` - Bug fixes
- `docs(scope): description` - Documentation
- `refactor(scope): description` - Code improvements
- `test(scope): description` - Tests
- `chore(scope): description` - Maintenance

## Branch Names

- `adr-NNN-topic` - Implementing an ADR
- `feature/name` - New feature work
- `fix/issue` - Bug fixes
- `refactor/area` - Code improvements

## Rules

- Skip "Co-Authored-By" and emoji trailers
- Focus commit message on the "why" not the "what"
- Keep commits atomic - one logical change per commit

## See Also

- delivery/github(softwaredev) — commits feed into PRs
- delivery/release(softwaredev) — commit types drive version bumps
