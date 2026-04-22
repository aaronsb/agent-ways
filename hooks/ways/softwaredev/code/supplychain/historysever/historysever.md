---
description: git history flattening, fork severing, BFG cleanup, removing secrets from git history
vocabulary: orphan branch sever history flatten history fork sever bfg git reflog gc prune standalone delete fork
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: convention -->
# History Severing

When git history itself is the problem — leaked secrets baked into commit objects, malicious payloads spread across tree history, or a fork relationship you want to break cleanly.

## Orphan Branch (Full Sever)

Keep only the current tree state, discard all history:

```bash
git checkout --orphan clean-main
git add -A
git commit -m "Initial commit (history severed)"
git branch -D main
git branch -m main
git gc --aggressive --prune=now
```

## BFG (Targeted Cleanup)

When you want to keep history but remove specific secrets or large files:

```bash
# Remove specific files from all history
bfg --delete-files '*.sql' --no-blob-protection

# Replace secret strings
bfg --replace-text secrets.txt  # file with one secret per line

# Clean up after BFG
git reflog expire --expire=now --all
git gc --prune=now --aggressive
```

BFG is orders of magnitude faster than `git filter-branch`. Install: `pacman -S bfg` / `brew install bfg`.

## Fork Severing

GitHub forks share git objects with upstream through the fork network. To break that relationship entirely:

1. Clone your fork locally
2. Delete the fork on GitHub
3. Create a new standalone repo with the same name
4. Push your local clone to the new repo

This eliminates the upstream trust chain — your repo is no longer connected to the original author's git objects.

## When to Sever

- You found secrets in history that can't be rotated (revoked keys still in git objects)
- The upstream repo has suspicious content in its commit history
- You want a clean provenance break from an untrusted fork
- The `.git` directory is bloated with artifacts you'll never need
