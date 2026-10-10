---
description: a clean-room evaluative loop that installs the product from zero into a fresh disposable environment and exercising it the way a new user would; pinned base image and tool versions, branch and release flavours, seeded user config that must survive, idempotence checked by running twice, non-root artifacts
vocabulary: clean room fresh install from scratch from zero new user first run installer test install path disposable container throwaway machine pinned base image cache bust branch flavour release flavour published artifact seeded home someone else's existing settings config preserved survives untouched second config directory installed over idempotent install twice toolchain prebuilt first-run cache-bust unprivileged
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: heuristic -->
# Clean-Room Loop

This loop proves the install path. The product is installed from nothing into a fresh, disposable environment and used the way a new user would use it. Tests on the authoring machine run against a configured checkout, so the installer, the upgrade over an existing configuration, and the first run are what they never see.

It starts from an empty machine; the root's routing table sets it beside `sandboxed` and `rubric`.

## When it applies

An install script, setup instructions, a getting-started guide or upgrade steps that have only been run on the author's machine, which already had the tools, settings and leftovers the steps assume. The next person to run them starts from something else.

## A reproducible room

- **Pin the base image and every tool version** the install depends on, in one place the image and the runner both read. A moving target (a "latest" channel) gets a cache-bust keyed to the date, so a stale cached layer cannot stand in for it. Pinned versions keep full caching.
- **An unprivileged user** runs the install. Artifacts written to a bind-mounted host path are owned by the host user, so cleaning up never needs elevated rights (see environment/container-safety).
- **Nothing the installer would not find on a user's machine.** When a dependency is missing from the image on purpose, assert that the install fetched the prebuilt artifact. Adding a toolchain to make a failing run pass hides the defect the room exists to find.

## Two flavours

- **Branch:** the checkout's committed state and freshly built binaries. It answers whether this change installs.
- **Release:** the published artifact through the documented install command. It answers whether what users download works today, and catches drift between the main branch, the release, and upstream dependencies.

## The user was here first

Seed the home directory with what a real user has: their own configuration, their own hooks or plugins, a setting the installer must not overwrite, a second configuration directory the product should not touch. Assert afterwards that the user's files survive by identity, their hooks still run, and the untouched directory hashes the same as its seed.

## Run it twice

A dry-run twice prints identical output and reports nothing to do. A second real run changes nothing.

## Exercise the first run

After the install, drive the product's entry points the way the host application would: real event payloads piped to the hooks or commands, read from the configuration the install wrote. Assert the expected output appears and every entry point exits zero.

## When it runs

The room is slow, so it runs locally before a release and on demand in CI, with the release flavour on a schedule. A tier that spends money, such as live model calls, is opt-in and runs on top of a passing install tier. Report each failure by name with the installer output beside it, and fix the earliest failing section first, since later ones cascade from it.

## See Also

- evaluative(evaluative) — parent: author and judge, the shared core, choosing the loop
- environment/container-safety(softwaredev) — non-root users and narrow bind mounts
- environment/hostparity(softwaredev) — the room is a second host; name which one a result came from
- delivery/release(softwaredev) — the room runs before the tag
