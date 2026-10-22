---
description: filing an issue for work set aside with enough context to resume from it alone, closing an issue with a comment that links the commit, confirming the CI workflow on main finished green after a merge before calling the work landed, and opening a one-fix PR against an external repository using its template
vocabulary: set aside deferred not now file an issue closing comment links commit sha actions run on main pages green after merging external repository pr template one fix per pr reviewer request
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: convention -->
# Following Through on GitHub

## After the Merge

When a workflow runs on main (CI, Pages, a deploy), confirm it finished green before calling the work landed.

## Issues

- When parking an idea, draft the issue before any build, with enough context, files, and decisions to resume from the issue alone, and offer to file it.
- Close an issue with a comment that says what was done and links the commit.

## Contributing to Another Project

One fix per PR. Follow the target repository's PR template. When the user names a reviewer, request their review.

## See Also

- delivery/github(softwaredev) — parent: PRs, review before merge, merge strategy, cleanup
- delivery/issues(softwaredev) — issues mirrored into the session task list
