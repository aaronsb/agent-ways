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

For multi-platform binaries, build per-platform and attach all of them to a single GitHub Release with a `checksums.txt`.

## Two-Step Release Under Branch Protection

A protected `main` splits the release in two, and this is common enough to plan for. The bump — version file, lockfile, changelog — goes through a PR like any other change. Only after it merges does the tag land on `main`.

Tagging is then the single outward step, and CI usually takes it from there: a tag-triggered workflow builds each platform and creates the release. Check for that workflow before hand-building artifacts.

Signed tags stop an agent cold. If the project signs (`tag.gpgsign`, or a `-s` in the release script), the tag command needs a passphrase from a terminal the agent doesn't own. Do everything up to that point, then hand the exact command to the operator rather than retrying into a timeout.

## Do Not

- Explain what semantic versioning is — just apply it
- List human process steps (deploy, announce) — produce artifacts Claude can generate
- Write publishing commands without checking `make release` first

## See Also

- delivery/commits(softwaredev) — changelog generated from commits
