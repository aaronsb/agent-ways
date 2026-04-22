---
description: lockfile hygiene, version pinning, preventing supply chain attacks through dependency resolution
vocabulary: lockfile package-lock.json yarn.lock pnpm-lock pin exact version caret semver npm-ci transitive resolution
files: package\.json|package-lock\.json|yarn\.lock|pnpm-lock\.yaml
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: lesson-learned -->
# Lockfile Hygiene

Pin lockfiles to trusted versions. Know what you pinned so you can chase things down.

Caret ranges (`^1.2.3`) tell the package manager "give me the latest compatible version" — which means a compromised patch release published for even a few hours can silently land in your project on the next install. Lockfiles are the defense: they record exactly what resolved, so you only get new versions when you deliberately update.

## When generating package.json

- **Prefer exact versions** for direct dependencies: `"foo": "1.2.3"` not `"foo": "^1.2.3"`
- If the project already uses caret ranges, follow existing convention but flag the trade-off

## When adding or updating dependencies

- Use `npm ci` (respects lockfile exactly) not `npm install` (resolves new versions) in CI and when reproducing builds
- After `npm install`, review the lockfile diff: `git diff package-lock.json` — look for packages you didn't ask for
- New transitive dependencies appearing in a lockfile update are a signal to investigate before committing

## Practical defaults

| Context | Approach |
|---------|----------|
| CI/CD pipelines | `npm ci --ignore-scripts` always |
| Local development | `npm install`, but review lockfile diff before committing |
| New project scaffolding | Exact versions in package.json |
| Existing project | Follow convention, flag if using caret ranges on security-critical deps |
| Brand-new package version (<48h old) | Delay adoption — let the community vet it first |
