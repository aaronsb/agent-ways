---
description: Shipping code — commits, pull requests, releases, migrations, and the path from local changes to production
vocabulary: ship deliver deploy release commit push merge pull request pr land code review changelog version tag branch workflow ci cd pipeline promote stage production
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: premise -->
# Delivery

**Before ad-hoc commands, check `make help`.** If the project has a Makefile, it likely has `make release`, `make dist`, `make deploy`, or similar targets that encode the project's actual publishing workflow. Use those.

Children of this way cover the journey from local changes to production:

| Stage | Way |
|-------|-----|
| Commits, messages | `delivery/commits` |
| PRs, issues, review, merge strategy | `delivery/github` |
| Landing an increment — review gate → merge → cleanup | `delivery/merge` |
| Releases, tagging, publishing | `delivery/release` |
| Patch creation | `delivery/patches` |
| Implementation planning | `delivery/implement` |

Deploying a schema migration is a delivery step, but the migration *discipline*
— design, numbering, idempotency, consolidation — lives in its own domain now:
see `data/migrations`.

## See Also

- delivery/commits(softwaredev) — commit structure and messages
- delivery/github(softwaredev) — PR workflow
- delivery/merge(softwaredev) — the review gate and landing an increment
- delivery/implement(softwaredev) — implementation planning
- data/migrations(data) — schema migration discipline (moved out of delivery)
