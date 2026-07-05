---
description: going over the stuff you have added to a machine over time and deciding what you no longer need — auditing a workstation for forgotten, unused, or leftover software you installed and stopped using, reviewing what accumulated, grouping it by subject, and trimming the cruft. The retrospective audit-and-cull side of installing software, distinct from project dependency management.
vocabulary: stuff I added installation machine workstation unneeded dont need anymore no longer use forgotten unused leftover smelly leftovers cruft accumulated go over review audit clean up trim prune get rid of remove uninstall what did I add what have I installed group by subject still need AUR yay go over the stuff I added go over what I installed
pattern: stuff i.?ve added|what (did|have) i install|(unneeded|leftover|forgotten|unused) (things|stuff|packages|software)|(don.?t|do not) (need|want) (it|them|this|that|th[eo]se|any ?more)|no longer (use|need)|(clean up|get rid of|trim|prune|cull).{0,30}(cruft|leftover|smelly|packages)|smelly .{0,20}leftover
scope: agent
refire: 0.12
---
<!-- epistemic: heuristic -->
# Package History as a Behavioral Journal

The package manager's log is an *involuntary journal*. The user never sat down to write it, but every line is timestamped intent they couldn't fake in retrospect. When someone asks "what have I added that I don't need anymore?", don't just dump a package list — read the log as a time series. The shape is the answer.

## Three signatures

| Signature | What it looks like | What it means |
|-----------|--------------------|---------------|
| **Burst** | Several related packages on the *same day* | One focused sitting with a goal (set up remote desktop, rice the desktop, tune the CPU). Burst density ≈ how deep the flow state was. |
| **Thread** | A theme recurring across months | Durable identity, not a whim. A persistent interest the log *proves*. Keep it. |
| **Orphan** | A cluster that appeared once and never recruited a neighbor or got revisited | Abandoned. **This is the strong cull signal — not age.** Old ≠ unused; a stable tool used daily can be a year untouched. A theme that *stopped recruiting* is done. |

The builder layer is the sharpest marker: packages the user *authored themselves* (their own AUR/local builds) mark a need no existing tool fit. They cluster around whatever the user was deep in at the time.

## What the log cannot see

- **Install ≠ use.** No telemetry on what actually runs. The log shows intent, not habit.
- **Survivorship.** `pacman -Qm` / `brew leaves` show only what *survived*. For the true series — including removals and upgrade churn — read the transaction log, not the current state.

## Extraction

| Manager | Deliberate adds | Dated history (incl. removals) |
|---------|-----------------|-------------------------------|
| pacman/yay | `pacman -Qm` (foreign), `pacman -Qet` (explicit leaves) | `/var/log/pacman.log` (`installed`/`removed`/`upgraded`, all timestamped) |
| apt | `apt-mark showmanual` | `/var/log/apt/history.log*` |
| dnf | `dnf history userinstalled` | `dnf history` |
| brew | `brew leaves` | `~/Library/Logs/Homebrew` / `brew info --json` install times |

Dated AUR list (pacman): `expac --timefmt='%Y-%m-%d' '%l\t%n' $(pacman -Qmq) | sort`

Focus pass 1 on **deliberate adds** (AUR/manual) — native repo leaves are mostly DE/base noise. Group by *subject*, then call out redundancy clusters *inside* each group (two remote-desktop stacks, three themes only one of which can be active, a build-tool cluster left over from one project). That's where the cull candidates hide.

## Render the timeline

Phases over time are sequential — show, don't narrate. Pipe a Mermaid `timeline` to `mmaid` for terminal art:

```bash
mmaid <<'EOF'
timeline
    title host — what the install log remembers
    2025-04 Bootstrap : shell, grub, mounts (provisioning, not exploring)
    2025-07 Infra sprint : whole remote-access stack in one sitting
    2026-05 Re-provision : browser+editor+toolchain in one burst
EOF
```

## Don't

- Flag "unneeded" by install date. Age is a weak signal; recruitment is the strong one.
- Persist a curated inventory before the cull — documenting cruft that's about to be culled wastes the write. Cull first, then offer to record the post-cull state.
- Remove without a reverse-dependency check (`pactree -r` / `pacman -Rsp` dry run). Let the user drive every "kill it".

## See Also

- visualization/diagrams(softwaredev) — `mmaid` timeline/structural rendering
- workstation/shell/tools(workstation) — the install side; this is the audit side
