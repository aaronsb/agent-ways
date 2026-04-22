---
description: creating and applying patch files, git diff generation, patch series management
vocabulary: patch diff apply hunk unified series format-patch
pattern: patch|\.diff|apply.*change
files: \.(patch|diff)$
commands: git\ apply|git\ diff.*\>
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: convention -->
# Patch Creation Way

**Golden Rule:** Never hand-write patch content. Always generate patches from real file changes using `git diff`.

## Single Patch

```bash
# 1. Make the actual edit to the file
# 2. Generate the patch
git diff path/to/file > change.patch
# 3. Revert if you only needed the patch
git checkout path/to/file
```

## Modifying an Existing Patch

```bash
# 1. Apply the patch first
git apply existing.patch
# 2. Make your modifications to the file
# 3. Regenerate the patch
git diff path/to/file > existing.patch
# 4. Revert
git checkout path/to/file
```

## Patch Series

Patches in a series are **cumulative** - each assumes prior patches are applied.

```bash
# To modify patch N in a series (e.g., 0003-foo.patch):
git apply 0001-*.patch 0002-*.patch  # Apply predecessors
git apply 0003-*.patch               # Apply target patch
# Make your changes
git diff > 0003-foo.patch            # Regenerate
# Verify downstream patches still apply:
git apply --check 0004-*.patch       # Check only, don't apply
```

**If downstream patches fail after your change:**
- You've changed context they depend on
- Regenerate them too (apply each, capture, repeat)

## Key Commands

| Task | Command |
|------|---------|
| Generate patch | `git diff [file] > name.patch` |
| Apply patch | `git apply name.patch` |
| Check if applies | `git apply --check name.patch` |
| Revert file | `git checkout path/to/file` |
| Compare two files | `git diff --no-index old new` |
