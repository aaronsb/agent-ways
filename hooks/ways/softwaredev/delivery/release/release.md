---
description: software releases, changelog generation, version bumping, semantic versioning, tagging
vocabulary: release changelog version bump semver tag publish ship major minor breaking
refire: 0.15
pattern: release|changelog|semver|git.?tag|release.?(notes|candidate)|npm.?publish|cargo.?publish
pattern_keep: release  # measured (ADR-155 §5): load-bearing ('github release with binaries' g=0.46, keyword-only); noise floor-gated (median g=0.01)
scope: agent, subagent
---
<!-- epistemic: heuristic -->
# Release Way

## First: Check for `make release`

Before writing ad-hoc release commands, check if the project has a Makefile with a `release` or `dist` target:

```bash
make help 2>/dev/null | grep -iE 'release|dist|publish|deploy'
# or just: grep -E '^(release|dist|publish)' Makefile 2>/dev/null
```

If it exists, **use it**. The Makefile is the canonical release interface — it knows the project's packaging, signing, and publishing steps.

## When There's No `make release`

### Generate Changelog

```bash
git log --oneline $(git describe --tags --abbrev=0 2>/dev/null || echo "HEAD~20")..HEAD
```

Format using Keep a Changelog:
```
## [X.Y.Z] - YYYY-MM-DD
### Added
### Changed
### Fixed
### Removed
```

### Infer Version Bump

From commit messages since last tag:
- Any `feat!:` or `BREAKING CHANGE` → **major**
- Any `feat:` → **minor**
- Only `fix:`, `docs:`, `chore:` → **patch**

### Update Version

Detect the version file (package.json, Cargo.toml, pyproject.toml, version.txt) and update it.

## Reconcile the Issue Tracker

A release is the moment "fixed in X" becomes a public claim, so the tracker is reconciled before the tag, not after. This is the release-time sibling of the ADR status flip in `delivery/merge` — same failure, different ledger: nothing breaks while it drifts, and the correction arrives later as a bulk audit.

Find what the release claims:

```bash
git log $(git describe --tags --abbrev=0)..HEAD --format='%s%n%b' \
  | grep -oiE '(clos|fix|resolv)(e[sd])? +#?[A-Z]+-?[0-9]+'
```

The pattern is deliberately tracker-agnostic — it catches `#123` and `PROJ-456` equally. Three things to settle with the hits:

- Items the commits closed get the released version recorded, where the tracker has a fix-version field.
- Items referenced without a closing keyword get checked against what the release actually does.
- An item the changelog names as fixed while the tracker shows it open means one of the two is wrong.

Act through whatever CLI the project already uses — `gh issue`, `glab`, `jira`, an MCP tool, a checklist in a file. Detect it from the repo rather than assuming. **A project with no tracker is a valid outcome**: say so and move on.

## Publishing Artifacts

| Destination | How |
|---|---|
| GitHub Releases | `gh release create vX.Y.Z --notes-file CHANGELOG.md <binaries>` |
| npm | `npm publish` (in `make release`) |
| PyPI | `python -m build && twine upload dist/*` |
| Cargo | `cargo publish` |
| AUR | Update PKGBUILD, `makepkg --printsrcinfo > .SRCINFO`, push to AUR |
| Container registry | `docker build -t repo:vX.Y.Z . && docker push` |

For multi-platform binaries (like ways, mmaid), build per-platform, attach all to a single GitHub Release with checksums.

## This Project

Two steps, because `main` is branch-protected (ADR-150). `make cut-release COMPONENT=<c> LEVEL=<patch|minor|major>` opens a version-bump PR; after it merges, `make publish-release COMPONENT=<c> PUSH=1` tags `<c>-vX.Y.Z` on main and pushes it.

Pushing the tag is the one outward step. CI (`build-<c>.yml`) then builds every platform and creates the GitHub Release with per-platform artifacts and `checksums.txt`. Tags are annotated and GPG-signed, so `publish-release` needs the operator's passphrase — an agent cannot complete that step.

## Do Not

- Explain what semantic versioning is — just apply it
- List human process steps (deploy, announce) — produce artifacts Claude can generate
- Write publishing commands without checking `make release` first

## See Also

- delivery/commits(softwaredev) — changelog generated from commits
