---
description: filing an issue for an idea or feature set aside with enough context to resume from it alone, closing an issue with a comment that links the commit, confirming after a merge that the merged tree passed CI once, on the pull request or on main, before calling the work landed, and opening a one-fix PR against an external repository using its template
vocabulary: set aside deferred not now file an issue closing comment links commit sha actions run on main pages green after merging tree passed once cancelled duplicate run external repository pr template one fix per pr reviewer request
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: convention -->
# Following Through on GitHub

## After the Merge

Before calling the work landed, confirm the merged tree passed CI once. Either run counts: the pull request's run on that same tree, or main's run after the merge. A run on main that was cancelled because it duplicated a tree that already passed is not a failure. A workflow that only runs on main, such as Pages or a deploy, has no earlier run, so confirm it finished green (delivery/ci).

## Issues

- When parking an idea, draft the issue before any build, with enough context, files, and decisions to resume from the issue alone, and file it. Parking applies to ideas and features. A review or test finding the change owns is fixed in the change, not parked (delivery/merge).
- When you close an issue, close it with a comment that says what was done and links the commit.

## Contributing to Another Project

One fix per PR. Follow the target repository's PR template. When the user names a reviewer, request their review.

## See Also

- delivery/github(softwaredev) — parent: PRs, review before merge, merge strategy, cleanup
- delivery/issues(softwaredev) — issues mirrored into the session task list
- delivery/ci(softwaredev) — one CI run per tree
