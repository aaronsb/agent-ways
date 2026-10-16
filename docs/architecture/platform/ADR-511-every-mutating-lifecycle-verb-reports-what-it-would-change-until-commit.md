---
contract: adr/v1
kind: decision
verb: change
capability: [install, cli]
amends: [ADR-150#Decision, ADR-144#Decision, ADR-184#Decision, ADR-509#Decision]
basis:
  - operator: aaronsb
    level: directed
    said: "a small update to 'ways manage update' - the status scroll is a bit 'techy' and it would be nice to have at least some semantic coloring of the output or some visual/color breaks in whats going on. additionally, printing the release notes for each item would be really helpful too. finally, we could have a switch on update behavior, where update defaults on whatif, and 'use --commit' or somethjing like that as a flag to go from whatif mode to actually updating. ways (and attend) is complex enough that respecting a development environment config to allow the operator to know whats changing is probably a good thing. ideally, we'd collect the delta between the last update and the current update and read all that out in whatif mode"
    via: session chat, 2026-10-16
  - operator: aaronsb
    level: directed
    said: "the other lifecycle flows for ways (and attend etc) could use the same spec/convention"
    via: session chat, 2026-10-16
  - evidence: 'at main 17b500e0, `ways manage update` with no flag pulls, downloads or builds binaries, relinks PATH and reprojects every target; `--dry-run` printed a fixed list of steps that named no version, commit or note (tools/ways-cli/src/cmd/update.rs run, the dry_run branch)'
  - evidence: '`make dev-install` points ~/.local/bin/<bin> and ~/.claude/bin/<bin> into a checkout; an update runs `make relink` and a repair relinks ~/.claude/bin, so either undoes it without a word (.claude/CLAUDE.md, Build cadence)'
  - evidence: '`ways reconcile` is spawned with no flag by the previous release''s updater and by scripts/install.sh, whose bin/ways can predate a new flag (ADR-509 §4; scripts/install.sh lines 394 and 460)'
  - evidence: '`uninstall` and `reinstall` already print a plan and change nothing without `--yes`; `target add` takes `--dry-run`; `target plan` previews activation'
  - precedent: ADR-150
  - precedent: ADR-509
  - precedent: ADR-144
  - precedent: ADR-184
  - precedent: ADR-198
  - precedent: ADR-504
agent:
  name: Claude
  model: claude-opus-5-5
considered:
  - operator: aaronsb
    said: "Report, like the rest"
    via: chat choice, 2026-10-16
    covers: [scope]
status: accepted
date: 2026-10-16
deciders:
  - aaronsb
related: [ADR-150, ADR-509, ADR-144, ADR-184, ADR-198, ADR-504, ADR-142]
observable:
  - "`ways manage update` on an install one release behind prints each component's version installed and at the target, the release notes in between with the CI tail dropped, the commits grouped by area, and the actions --commit would take; HEAD, the working tree and every binary link are as they were"
  - "With a `make dev-install` active, the what-if names each link that points into the checkout and says --commit would relink it to the app"
  - "Offline, the what-if says the release notes could not be fetched and prints the rest"
  - "`ways manage update --commit` prints one header per stage (Pull, Binaries, Corpus, Reproject, Judge, Done) with ✓, ⚠ or ✗ per outcome, and a command's raw output only under --verbose or when it fails"
  - "In a pipe or under NO_COLOR the report and the step scroll carry the same text with no escapes"
  - "`ways manage repair` and `ways manage target add|enable|disable|remove <dir>` change nothing without --commit; `ways reconcile` still applies with no flag"
---

# ADR-511: Every mutating lifecycle verb reports what it would change until --commit

## Summary

- **Decided:** every `ways manage` verb that changes the install prints what it would change and changes nothing until `--commit` is given. The report has one shape for every verb: a title, sections under ruled headers, a mark per line (↑ moves, = unchanged, + added, * fixed, → an action, ⚠ needs attention), coloured by role on a terminal and plain in a pipe. `update` reports versions, release notes, the source delta, local changes, the actions and dev links. A commit run prints one header per stage with ✓, ⚠ or ✗ per outcome and keeps raw command output for `--verbose` or a failure. `--yes` and `--dry-run` stay accepted. `install.manage_mode: commit` lets an operator restore apply-by-default for `update`, `repair` and `target`. The hidden `reconcile` keeps applying, and the installer keeps installing in one step.
- **Trades away:** every caller that means to apply must now say `--commit`: scripts, skills, hints, the settings screens and the operator's habit. An operator who types `ways manage update` and walks away has updated nothing.
- **One-way?** No: reversible. The default is one function and one setting; flipping it back is a one-line change, and `install.manage_mode: commit` does it per machine today.
- **Probes:** *Confident (default):* you want a bare `ways manage update` to change nothing on disk except a fetch, and to tell you what `--commit` would do. *Not confident (scope):* you want `repair` and `target` to follow the same default, so that `ways manage repair` on its own reports, even though a repair is usually run to fix something now.
- **Inversion:** one end keeps every verb applying at once and adds a richer `--dry-run`, so habits and callers stay as they are and the operator has to remember to look first. The other end makes every lifecycle path report first with no way to opt out, including the installer one-liner and the updater's cross-release call. This decision reports first for every verb a person types, keeps one setting to opt out, and leaves the two calls that come from another release, or that are themselves the explicit request, applying.

## Context

The install changes in several places at once: the app checkout, six suite binaries and way-embed, the corpus, each target's projection and `settings.json`, the attend mod, and the PATH links. A contributor's `make dev-install` adds a seventh: links that point into a checkout other than the app. Before this decision the update applied on a bare invocation, and its scroll was the raw output of git, make, the download script and cargo. Its `--dry-run` printed a fixed list of steps, the same on every machine, that named no version, no commit and no release note.

The operator asked for the update to report before it acts: what each component moves to, the release notes in between, and the delta since the last update, with colour and visual breaks in place of the raw scroll. The operator then asked for the other lifecycle flows to follow the same convention.

Some verbs already had half of it. `uninstall` and `reinstall` print a plan without `--yes` (ADR-198, ADR-509 §3), `target add` takes `--dry-run`, and `target plan` previews activation (ADR-184). `repair`, `target enable|disable|remove` and `update` applied at once.

Two callers cannot pass a new flag. The previous release's updater spawns `reconcile` on the newly installed binary (ADR-509 §4), and `scripts/install.sh` runs `bin/ways reconcile` on a binary that `make setup` may have downloaded from an older release.

## Decision

1. **What-if is the default.** A mutating `ways manage` verb without `--commit` changes nothing except a read-only fetch, and prints what it would change. The verbs are `update`, `repair`, `target add|enable|disable|remove`, `uninstall` and `reinstall`. A what-if exits 0 even when it found something to change: a script that runs it under `set -e` must not fail on the report.
2. **One apply flag.** `--commit` applies, on every verb. `--what-if` names the default explicitly. `--yes` (from `uninstall` and `reinstall`) and `--dry-run` (from `update`, `repair` and `target add`) stay accepted on every verb as hidden aliases of `--commit` and `--what-if`, so existing docs and habits keep working. The two flags conflict.
3. **One setting.** `install.manage_mode` in the user config takes `whatif` (the default) or `commit`. With `commit`, `update`, `repair` and `target` apply without the flag; a flag on the command line wins over the setting. `uninstall` and `reinstall` remove the install and always need `--commit`. `ways settings` lists the key on the install tab.
4. **One report.** The report and the step scroll live in one module of the `ways` binary (`cmd/lifecycle.rs`) that every verb uses. They draw from the theme engine's roles (ADR-504): a ruled header per section, a mark and role per line, the terminal palette on a terminal and plain text in a pipe, under `NO_COLOR`, or with `TERM=dumb`. The plain text is the coloured text with the escapes removed. A commit run prints one header per stage and ✓, ⚠ or ✗ per outcome. The output of the commands it runs is captured, shown when a command fails, and streamed under `--verbose`.
5. **Versions and notes where a verb moves versions.** `update` reports, for each suite binary, way-embed and the attend mod, the version installed and the version at the target. For each component that moves it prints the GitHub release notes of every `<component>-vX.Y.Z` release after the installed version up to the target, without the heading and without the CI tail after the closing `---`. The notes come from the GitHub Releases API through `curl`, the tool the installer one-liner already needs, with a token from `GH_TOKEN` or `GITHUB_TOKEN` when set. `gh` is not required. Unreachable notes are named as unreachable, and the report goes on. `WAYS_RELEASES_JSON` names a file to read in place of the API, for a mirror or a test.
6. **The source delta.** `update` fetches the app checkout and summarises `HEAD..<target>`: a count per area (the component a conventional scope names, else the area most of the commit's paths fall in), the feats and fixes listed, the rest counted, and the merged pull requests. `--verbose` lists every commit. It also lists local changes the pull's autostash would carry, and the actions `--commit` would take on this machine.
7. **Dev links are detected.** A verb that relinks binaries reads each suite binary's link in `$XDG_BIN_HOME` (else `~/.local/bin`) and in each enabled target's `bin/`. A link whose target lies outside the app and inside a git checkout is a dev link. The report names each one, says the verb would point it back at the app, and names `make dev-install` as the way back.
8. **Calls from another release keep applying.** `ways reconcile`, the hidden fixed name of ADR-509 §4, applies with no flag, as before, and takes `--what-if`/`--dry-run` to report. The previous release's updater and the installer call it, and neither can pass a flag the binary it calls may lack. Everything a person reads names `ways manage repair --commit`.
9. **The installer is the explicit request.** `curl … | bash` and `scripts/install.sh` install in one step: running the one-liner is the request to apply, and a what-if there would leave a fresh machine with nothing. It prints the same staged report in shell, with the same headers, marks and plain fallback, since no `ways` binary exists when it starts. The installer calls `reconcile`, so item 8 keeps it working with any binary.
10. **Make targets are out of scope.** `make dev-install`, `make dev-restore` and `make purge-attend-state` are contributor and recovery tools whose interface is make's: a target does its work, and `make -n` previews it. attend has no lifecycle verb of its own; its binaries, its mod and its state are installed, updated and removed through `ways manage`, so it follows this convention through them.
11. **Internal callers say `--commit`.** The Makefile's `update` target, the ways-update skill, the settings screens' actions, hints that tell the operator to run a verb, and the tests that apply pass `--commit`. In-process callers (the updater's reproject, `reinstall`'s repair, `uninstall`'s withdrawal) call the apply path directly.

A what-if of `ways manage update`, one release behind (trimmed):

```
ways manage update: what-if
app ~/.local/share/agent-ways · at d3fe7f7d · target 17b500e0

── Versions ────────────────────────────────────────────────────
  ↑ ways         1.41.0 → 1.41.1
  = ways-audit   1.0.3 unchanged
  ↑ attend mod   0.28.0 → 0.28.1

── Release notes ───────────────────────────────────────────────
  ways 1.41.0 → 1.41.1
    ways-v1.41.1
      ### Fixed
      - **Entry dialogs wrap a long error.** … (#1021).

── Changes ─────────────────────────────────────────────────────
  23 commits, 6 pull requests merged
  attend (4 fix, 2 docs, 1 release)
    fix      fix(attend): a session with no turn yet is wakeable

── Actions ─────────────────────────────────────────────────────
  → pull: fast-forward d3fe7f7d → 17b500e0 (origin/main)
  → ways: download the ways-v1.41.1 release
  → reproject ~/.claude (ways manage repair)

· Nothing changed. Run `ways manage update --commit` to apply.
```

This amends ADR-150's Decision (the self-update applies only under `--commit`, and reports first), ADR-144's Decision (the repair verb reports first; the reconciler's own entry, `reconcile`, still applies), ADR-184's Decision (`target add|enable|disable|remove` report first; `target plan` is unchanged), and ADR-509's Decision (item 1's table: every verb takes `--commit`; item 3's `--yes` becomes an alias of `--commit`; item 5's one-release aliases `ways update`, `ways uninstall` and `ways target` report like the verbs they forward to).

## Consequences

### Positive

- An operator sees what an update would do, and why, before anything moves: versions, notes, commits, local changes, actions and dev links in one page.
- A contributor with `make dev-install` active is told that an update or a repair would undo it, before it does.
- The step scroll reads as stages with outcomes, and the raw output is still there for a failure or `--verbose`.
- One module holds the report and the scroll, so a new verb takes the convention by using it.

### Negative

- Every applying caller changes: docs, skills, hints, the settings screens, tests and habits. A caller left on the bare form silently reports instead of applying. The call-site test checks two ways. In Rust, shell, Makefiles and the TypeScript mod it fails a `ways manage <verb>` without `--commit` that follows a cue word such as "run" or "then", or that the settings screens queue; a hint with no cue word still gets past it. In the docs, skills and ways it fails every bare mutating verb unless its form makes it a description (a heading, a fenced command whose comment says it only reports, a describing verb right after it, or a word such as "whether" or "as" right before it) or it is on an allowlist with its reason; an instruction written in one of those forms still gets past it.
- The release notes need the network and an unauthenticated API call per page of 100 releases, against GitHub's limit of 60 an hour per address.
- `ways manage repair` run to fix something now needs `--commit`; the operator who wants the old behaviour sets `install.manage_mode: commit`.
- `reconcile` and `manage repair` now differ in their default, which a reader of the source has to keep apart.

### Neutral

- `install.manage_mode` is user scope only. A target's or a project's file cannot make a verb apply.
- The exit code of a what-if does not say whether an update is available. A script that needs to know can compare versions itself; a distinct code would break `set -e` callers.

## Alternatives Considered

- **Keep applying by default and enrich `--dry-run`.** Rejected: the operator asked for the default to be the report, and a richer flag that has to be remembered is what the old `--dry-run` already was.
- **Report first on `update` only.** Rejected after the operator's second message: the verbs share one install, and a convention that holds for one verb teaches nothing about the others.
- **Make `reconcile` report first too.** Rejected: the previous release's updater and the installer call it with no flag, so every update from the previous release and every fresh install would stop projecting.
- **Read the notes with `gh`.** Rejected: `gh` is optional on an install, and its absence would hide the notes on exactly the machines without a toolchain. `curl` is already required by the one-liner.
- **A distinct exit code when an update is available.** Rejected for now (see Neutral).
