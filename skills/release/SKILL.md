---
name: release
description: Cut a versioned release — infer the version bump, update the changelog, tag, and publish artifacts (GitHub Release, package registries). The heavy sibling of /merge. Use when the user says "cut a release", "publish a release", "tag a version", "ship a release", or invokes /release.
allowed-tools: Bash, Read, Grep, Glob
---

# Release — publish a versioned release

The occasional, heavier tail: turning merged work into a published, versioned
artifact. This is **not** landing an increment (that's `/merge`) — it's the one-way
door of putting a version into the world. Confirm before anything publishes.

The doctrine lives in the `delivery/release` way; this skill runs it.

## First: check for `make release`

```bash
make help 2>/dev/null | grep -iE 'release|dist|publish|deploy'
grep -E '^(release|dist|publish):' Makefile 2>/dev/null
```

If a target exists, **use it** — the Makefile is the canonical release interface; it
knows the project's packaging, signing, and publishing steps. Confirm with the user,
then run it.

## When there's no `make release`

### 1. Infer the version bump

From commits since the last tag:

```bash
git log --oneline "$(git describe --tags --abbrev=0 2>/dev/null || echo HEAD~20)"..HEAD
```

- `feat!:` / `BREAKING CHANGE` → **major**
- `feat:` → **minor**
- only `fix:` / `docs:` / `chore:` → **patch**

State the inferred bump and the resulting version; let the user override.

### 2. Update the version file

Detect and update: `package.json`, `Cargo.toml`, `pyproject.toml`, `version.txt`, etc.

### 3. Changelog

Keep a Changelog format, grouped Added / Changed / Fixed / Removed.

### 4. Reconcile the issue tracker

A release is when "fixed in X" becomes a true statement. Find what this release claims
to close, before the tag makes the claim public:

```bash
git log $(git describe --tags --abbrev=0)..HEAD --format='%s%n%b' \
  | grep -oiE '(clos|fix|resolv)(e[sd])? +#?[A-Z]+-?[0-9]+'
```

That catches `#123` and `PROJ-456` alike. What to do with the hits depends on what the
project's tracker offers:

- Items the commits closed — stamp the released version, if there's a fix-version field.
- Items referenced without a closing keyword — check whether this release resolves them.
- An item named in the changelog that the tracker still shows open — one of the two is wrong.

Use whatever CLI the project already uses (`gh issue`, `glab`, `jira`, an MCP tool, a
`TODO.md`). Detect it; don't assume one. **No tracker is a valid answer** — say so and
move on rather than inventing one.

### 5. Tag

```bash
git tag -a vX.Y.Z -m "summary"
git push origin main --tags
```

### 6. Publish artifacts (confirm first — one-way door)

| Destination | How |
|---|---|
| GitHub Release | `gh release create vX.Y.Z --generate-notes <artifacts>` |
| npm | `npm publish` |
| PyPI | `python -m build && twine upload dist/*` |
| Cargo | `cargo publish` |
| Container | `docker build -t repo:vX.Y.Z . && docker push` |

For multi-platform binaries, build per-platform and attach all to one GitHub Release
with a `checksums.txt`.

## Key Principles

- **`make release` first** — don't hand-roll what the project already encodes.
- **Never auto-publish** — every publish target is a one-way door; confirm each.
- **Apply semver, don't explain it** — infer the bump, state it, proceed on confirm.
- **Release follows merge** — the work should already be on main before you release it.

## Not for

- Landing an increment to main — that's `/merge`.
- Writing or reviewing the code being released — that's `/develop` and `code-review`.

## See Also

- `merge` — land the work first; release publishes what's already merged.
- delivery/release(softwaredev) — the release doctrine this runs.
