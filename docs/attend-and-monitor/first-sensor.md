# Your first sensor file

This walkthrough writes sensor files by hand, sees attend pick them up, and sends test results through them. It tunes a shipped sensor for yourself, switches one off for a project, reads what a mistake looks like, and starts a command sensor from the shipped example. [`authoring-sensors.md`](authoring-sensors.md) is the spec behind each step.

You need an attend that reads sensor files (ADR-403): `attend sensor list` prints `Kind` and `Origin` columns. You do not need Rust. The paths below use `~/.config` for `$XDG_CONFIG_HOME`.

## Step 1 — See what runs

```
$ attend sensor list
  Sensor          Kind    Origin  State  Lane    Interval   Source                 File
  ─────────────────────────────────────────────────────────────────────────────────────
  context         crate   builtin active event   60s / 20s  attend@0.33.0          -
  ...
  processes       crate   builtin active event   30s / 5s   sensor-processes@0.6.0 -
```

Each row is one sensor and its files merged. Every row here is `builtin`: the files attend ships, under `attend/sensors/` in the repository. `processes` reports build tools that finish, at a threshold of 2.0.

## Step 2 — Tune a shipped sensor for yourself

Make the user directory and write a file named after the sensor:

```bash
mkdir -p ~/.config/attend/sensors
cat > ~/.config/attend/sensors/processes.yaml <<'EOF'
threshold: 3.0
watch: [cargo, rustc, make]
EOF
```

The file carries only what it changes. It merges over the shipped `processes.yaml`, so `kind`, `crate` and the intervals come from there. `watch` is a field of the compiled processes sensor: these three tools get build enrichment, and the default list is replaced.

```
$ attend sensor list
  ...
  processes       crate   builtin · user extends active event   30s / 5s   sensor-processes@0.6.0 ~/.config/attend/sensors/processes.yaml
```

`Origin` is now `builtin · user extends` and `File` names your file. A running `attend run` sees the change and reloads.

## Step 3 — Send a test through it

`attend sensor fire` passes a synthetic result through the sensor's real threshold and lane, and delivers what gets through to the session. `--dry-run` stops before delivery.

```
$ echo "2.5|cargo finished" | attend sensor fire processes --input - --dry-run
processes: held on the event lane: 2.5 accumulated of a 3.0 threshold
```

One observation at 2.5 is under the new threshold, so it waits in the accumulator. Two smaller ones add up:

```
$ printf '1.5|cargo finished\n1.5|make finished\n' | attend sensor fire processes --input -
processes: delivered on the event lane to session 3f2a…: processes-1795443401432392982-0, processes-1795443401432392983-1
```

Without `--dry-run` each line becomes a signal from `system:processes`, marked `[test]` and addressed to the session, as the sensor host writes a real report. The session reads it as it reads a message, and `attend message inbox` lists it with `from_kind` `system`. The session is the one the shell runs in; from anywhere else, name it with `--session <id>`, and with no session at all the fire refuses to deliver. Magnitude is the lever: what reaches the threshold goes out together, and what does not waits. On the event lane a batch that reaches the threshold also needs 3.0 to be written; under that, attend logs it and writes nothing, as the host does. A fire writes nothing to the sensor's store unless given `--commit`.

## Step 4 — Switch a sensor off for one project

In a project you work in, write a project file:

```bash
mkdir -p .claude/attend/sensors
printf 'enabled: false\n' > .claude/attend/sensors/git.yaml
```

Run from that project, `attend sensor list` shows `git` with `Origin` `project` and `State` `off`, and `attend run` there does not poll it. A project file that changes a `crate` sensor needs no permission. A project's `command` or `mcp` file runs code the repository chose, so it loads only once you list the project in `project_sensors` in `~/.config/attend/config.yaml`.

## Step 5 — Read a mistake

Change the threshold in your user file to a word:

```bash
sed -i 's/^threshold: .*/threshold: high/' ~/.config/attend/sensors/processes.yaml
```

`attend sensor list` now shows `processes` as `invalid`, and the line under the table says why:

```
  processes: invalid: threshold: threshold is a number from 0 to 100
```

An invalid sensor does not run, and the shipped file does not stand in for it. `ways settings lint` names the file and the line, beside the findings of every other settings file. Set the threshold back to `3.0` and the sensor is `active` again.

## Step 6 — Start a command sensor from the example

`example-command.yaml` is a complete command sensor, shipped off, with every field commented. It reports systemd user units that fail. Copy it under a name of your own, from the app dir or from `attend/sensors/` in a checkout:

```bash
cp "${XDG_DATA_HOME:-$HOME/.local/share}/agent-ways/attend/sensors/example-command.yaml" \
   ~/.config/attend/sensors/failed-units.yaml
```

A sensor's name is its file's stem, so set `name: failed-units` in the copy, and `enabled: true`. Read the comments while you are there: `argv` and `output` say what runs and how its output reads, `state` says what attend keeps between polls, and each trigger says which change wakes the agent.

```
$ attend sensor list
  ...
  failed-units    command user    active   event   300s / 60s sh  ~/.config/attend/sensors/failed-units.yaml
  ...
```

`attend sensor poll failed-units` runs the command once and prints each trigger's verdict, `quiet` or `fired`, against this session's store, and writes nothing to it. `ways settings lint` checks the file: change a trigger's `on: added` to `on: appeared` and lint names the line.

## Clean up

```bash
rm ~/.config/attend/sensors/processes.yaml ~/.config/attend/sensors/failed-units.yaml
rm .claude/attend/sensors/git.yaml
```

With the files gone, each sensor reads from its shipped file again.

## Where to go next

- **[`authoring-sensors.md`](authoring-sensors.md)** — every field of every kind, triggers, state, lanes and the project allow-list.
- **[`sensors.md`](sensors.md)** — what each shipped sensor observes and the magnitudes it emits.
- **`example-mcp.yaml`** beside the command example — the pattern for a sensor that reads an MCP tool.
