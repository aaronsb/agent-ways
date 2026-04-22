---
description: software releases, changelog generation, version bumping, semantic versioning, tagging
vocabulary: release changelog version bump semver tag publish ship major minor breaking
refire: 0.15
pattern: release|changelog|tag|version.?bump|bump.?version|npm.?publish|cargo.?publish
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

- Annotated tags: `git tag -a vX.Y.Z -m "summary"`
- Push tags explicitly: `git push origin main --tags`
- No CI release pipeline — tagging is the release
- Binary tools: GitHub Releases with per-platform artifacts + `checksums.txt`

## Do Not

- Explain what semantic versioning is — just apply it
- List human process steps (deploy, announce) — produce artifacts Claude can generate
- Write publishing commands without checking `make release` first

## See Also

- delivery/commits(softwaredev) — changelog generated from commits
