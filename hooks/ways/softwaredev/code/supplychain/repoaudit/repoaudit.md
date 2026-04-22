---
description: git history audit, leaked secrets detection, binary blob discovery in repositories
vocabulary: git history large objects leaked secrets committed gitignored binary blob git rev-list repo size secret scan AKIA ghp_ glpat xox api key token password private key credentials
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: heuristic -->
# Repo Audit

Quick checks you can run in seconds on any unfamiliar repo.

## Size Smell

```bash
du -sh .git && du -sh . --exclude=.git
```

If `.git` is significantly larger than the working tree, something is hiding in history — binary blobs, database dumps, zip archives.

## Large Objects in History

```bash
git rev-list --objects --all \
  | git cat-file --batch-check='%(objecttype) %(objectsize) %(rest)' \
  | awk '/^blob/ && $2 > 1048576 {print $2, $3}' \
  | sort -rn | head -20
```

## Committed Then Gitignored

The most common secret leak pattern — developer commits a file, realizes the mistake, adds it to `.gitignore`. File disappears from working tree but remains in git history forever.

```bash
git ls-files -i --exclude-standard
```

## Secrets in History

Search diffs for common credential patterns:

```bash
# AWS, OpenAI, GitHub, GitLab, Slack, PEM keys
git log -p --all | grep -E '(AKIA[A-Z0-9]{16}|sk-[a-zA-Z0-9]{20,}|ghp_[a-zA-Z0-9]{36}|glpat-[a-zA-Z0-9-]{20}|xox[bpas]-|-----BEGIN.*(PRIVATE|RSA))' | head -30
```

If you find leaked credentials in someone else's repo, file a responsible disclosure — mask the actual values, describe where and how to fix.
