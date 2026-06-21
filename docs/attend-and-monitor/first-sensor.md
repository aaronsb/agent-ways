# Your first sensor — a walkthrough

This is the "sit down and build one" companion to [`authoring-sensors.md`](authoring-sensors.md). The reference doc is organized around questions: *what's the subprocess contract, what goes in the config block, what do the magnitudes mean*. It's the right shape when you already know you want to write a sensor and need to look up the details. It's the wrong shape if you've never seen attend before and want to go from zero to a working sensor in one sitting.

This tutorial builds a sensor from nothing, top to bottom, and explains each piece as it lands. Target reader: you know bash and Linux, you've read [`loop.md`](loop.md) so you understand attend is watching something for you, and now you want to make it watch one more thing.

You'll need `attend` already built and installed (`make attend && make install` from the agent-ways repo). You do not need Rust.

## What we're building

A sensor that notifies you when a file you care about gets modified.

Pick a file you actually want to watch — a notes file, a personal TODO list, a config file you share across machines. This walkthrough uses `$HOME/notes.md` as the example, but you can substitute anything. The goal isn't the target, it's the pattern: the pattern generalizes to *any single-file observation*, and the shape you end up with is how you'd write a watcher for a database file, a cert that rotates, a deploy lock, or anything else that changes on a timescale attend can poll.

## Step 1 — Write the first version

Open a scratch file and put this in it:

```bash
#!/usr/bin/env bash
set -euo pipefail

TARGET="$HOME/notes.md"

if [[ -f "$TARGET" ]]; then
  mtime=$(stat -c %Y "$TARGET")
  echo "2.0|notes.md mtime is $mtime"
fi
```

Save it as `notes-watch.sh`, make it executable (`chmod +x notes-watch.sh`), and run it by hand:

```
$ bash notes-watch.sh
2.0|notes.md mtime is 1744589321
```

Congratulations, that's a sensor. It doesn't do anything useful yet, but it respects the subprocess contract the sensor loop needs:

- **Exits 0 even if the target doesn't exist.** The `if` guard means the script stays quiet when `notes.md` is missing, and silence is how the sensor loop says "nothing to report this tick."
- **One line of output per event,** shaped as `magnitude|description`. The pipe is the delimiter. Whitespace around it gets trimmed. The magnitude is a floating-point number.
- **No arguments needed.** Attend will invoke the script as `bash /path/to/notes-watch.sh` with no extra arguments each poll. Any state you want to keep across polls has to live on disk (we'll get to that).

See [`authoring-sensors.md` § "The subprocess contract"](authoring-sensors.md) for the full list of rules. The short version: exit 0, write events on stdout, don't hang for more than 10 seconds.

## Step 2 — Declare it in config

A sensor script does nothing until attend knows about it. Copy the script to a trusted location — if you don't already have one, `$XDG_DATA_HOME/attend/sensors/` (usually `~/.local/share/attend/sensors/`) is the XDG-spec convention:

```
mkdir -p "$HOME/.local/share/attend/sensors"
cp notes-watch.sh "$HOME/.local/share/attend/sensors/"
```

Then open `~/.config/attend/config.yaml` and add a block under the `sensors:` section:

```yaml
sensors:
  +notes-watch:
    script: $XDG_DATA_HOME/attend/sensors/notes-watch.sh
    enabled: true
    interval: 30
    min_interval: 10
    threshold: 1.0
```

The important details:

- **The `+` prefix** declares a *new* sensor, on top of the built-ins. `-` would *disable* a built-in. Names in the config don't include the `+`/`-` after they're parsed — it's a create/disable marker, not part of the name.
- **`script:`** is the path to the script. `$XDG_*` variables get expanded at load time, so this config is portable across machines.
- **`interval: 30`** means "poll every 30 seconds when quiet." `min_interval: 10` is the floor during bursts, which we'll come back to.
- **`threshold: 1.0`** is the emission threshold. Every time the sensor fires an event with magnitude ≥ 1.0, the loop considers disclosing it. We used `2.0` in the script, so every poll will trip the threshold right now.

Now restart attend (stop the Monitor that's running `attend run`, start a new one — config changes need a fresh load) and watch for the notification. Within 30 seconds you should see one land in the conversation:

```
[attend sensor=notes-watch] notes.md mtime is 1744589321
```

It'll fire again every 30 seconds. That's not useful — we want a notification *when it changes*, not *every time we look at it* — but now you've seen the full loop end to end, from script to notification. From here on, every change is a refinement.

## Step 3 — Fire on change, not on every poll

The marker-file pattern is attend's standard trick for "remember what we saw last time." Write the old value to a file under `$XDG_STATE_HOME/attend/`, read it back on the next poll, compare, and only emit when the value has changed:

```bash
#!/usr/bin/env bash
set -euo pipefail

TARGET="$HOME/notes.md"

STATE_DIR="${XDG_STATE_HOME:-$HOME/.local/state}/attend"
mkdir -p "$STATE_DIR"
MARKER="$STATE_DIR/notes-watch.marker"

[[ -f "$TARGET" ]] || exit 0

mtime=$(stat -c %Y "$TARGET")

prev=""
[[ -f "$MARKER" ]] && prev=$(cat "$MARKER" 2>/dev/null || printf '')

# First run — record state and emit nothing. Otherwise attend restarts
# would re-notify for any mtime that already existed when the sensor
# came up.
if [[ -z "$prev" ]]; then
  printf '%s\n' "$mtime" > "$MARKER"
  exit 0
fi

if [[ "$mtime" != "$prev" ]]; then
  printf '%s\n' "$mtime" > "$MARKER"
  echo "2.0|notes.md modified"
fi
```

Two new pieces to notice:

1. **First-run silence.** If we didn't short-circuit when the marker is missing, the first time the sensor ever ran it would see `prev=""`, compare to the current mtime, and decide "changed." That's not what a user wants — the file hasn't *changed*, the sensor just doesn't know its history yet. First-run silence is a convention you'll see in every shipped sensor for the same reason.
2. **Update the marker on change only.** If mtime is unchanged, the marker stays the same; if it changed, we overwrite with the new value. This rolling "high-water mark" is what lets attend restarts pick up where they left off.

Restart attend again. This time you'll see **nothing** — the sensor ran, compared the mtime to the marker, found no change, and exited silent. Now touch the file in another terminal:

```
$ touch "$HOME/notes.md"
```

Within 30 seconds (the `interval`), a notification lands:

```
[attend sensor=notes-watch] notes.md modified
```

That's the shape. Now let's tune it.

## Step 4 — Tier the magnitudes

Right now every modification emits at magnitude 2.0 regardless of what changed. That's fine for a single file, but the "author's lever" — the place where you encode *how much each kind of change matters* — is the magnitude column. Let's split modifications by size:

```bash
size_prev=""
[[ -f "$STATE_DIR/notes-watch.size" ]] && size_prev=$(cat "$STATE_DIR/notes-watch.size")

size_now=$(stat -c %s "$TARGET")
printf '%s\n' "$size_now" > "$STATE_DIR/notes-watch.size"

if [[ "$mtime" != "$prev" ]]; then
  printf '%s\n' "$mtime" > "$MARKER"

  delta=$(( size_now - ${size_prev:-$size_now} ))
  delta_abs=${delta#-}

  if (( delta_abs == 0 )); then
    echo "1.5|notes.md touched (size unchanged)"
  elif (( delta_abs < 100 )); then
    echo "2.0|notes.md edited (+/- $delta_abs bytes)"
  else
    echo "3.0|notes.md large edit (+/- $delta_abs bytes)"
  fi
fi
```

What changed:

- **A second marker file** (`.size`) tracks the byte size at the last poll. You can use one marker with multiple fields (common shape: `mtime|size` joined with a delimiter), but two files keep this example readable.
- **Three magnitude tiers.** A touch with no size change is likely a `mv`-in-place or tool-driven re-save (1.5, below the default 2.0 threshold — accumulator only). A small edit is normal activity (2.0, barely crosses). A big edit is worth knowing about (3.0, breaks through comfortably).

That tiering is the whole point. Read [`engagement.md`](engagement.md) to understand how the loop uses those numbers: the accumulator, refractory behaviour after a burst, and the way low-magnitude events get suppressed after enough high-magnitude noise. The short version: **pick numbers that reflect your actual priority, not round numbers that feel nice.** Attend will handle the rest.

## Step 5 — Handle failure gracefully

What happens if the target file vanishes mid-session? Right now the `exit 0` guard covers it, but let's be explicit about *all* the places things can go wrong:

```bash
# Dependencies — silent no-op if anything is missing. The sensor
# contract is "no output = quiet poll," so we never emit errors.
command -v stat >/dev/null 2>&1 || exit 0

TARGET="$HOME/notes.md"

# Target may have been deleted, unmounted, or never existed. Any of
# those is the same as "nothing to observe."
[[ -f "$TARGET" ]] || exit 0

STATE_DIR="${XDG_STATE_HOME:-$HOME/.local/state}/attend"
mkdir -p "$STATE_DIR" || exit 0     # disk full, permissions, etc.

# stat can fail if the file disappears between the -f check and the
# read, or if its inode gets swapped mid-poll. || true keeps set -e
# from killing us; we treat a bad stat as "no observation this tick."
mtime=$(stat -c %Y "$TARGET" 2>/dev/null || true)
[[ -z "$mtime" ]] && exit 0
```

The pattern across every shipped sensor is the same: **a quiet poll is a valid outcome.** Users don't want error lines in their notification stream — they want attend to notice things, and if it can't notice anything this tick, they want it to shut up until it can. A sensor that emits `ERROR: stat failed` on a broken symlink is a sensor that gets disabled within an hour.

See `tools/attend/examples/xdg-downloads.sh` for a different take on the same pattern (`xdg-user-dirs` might not exist, fallback to `$HOME/Downloads`), or `gh-notifications.sh` for how to handle network failure and authentication issues without leaking them into the conversation.

## Step 6 — Tune intervals and decay

Go back to the config block:

```yaml
  +notes-watch:
    script: $XDG_DATA_HOME/attend/sensors/notes-watch.sh
    enabled: true
    interval: 30
    min_interval: 10
    threshold: 1.0
    decay_threshold: 4
```

The knobs:

- **`interval`** — base polling rate when nothing's happening. If your target file changes on minute-scale (notes, TODOs, config), 30–60 seconds is right. If it changes on hour-scale (cert files, nightly exports), bump it to 300 or more. API-backed sensors might sit at 120–180 to respect rate limits. **Pick the rate at which you'd actually want to find out.**
- **`min_interval`** — the ramp floor during bursts. When the sensor is actively emitting, the loop polls it faster (down to `min_interval`) to catch the back end of a burst quickly. Leave this at roughly `interval / 3` unless you have a reason to change it.
- **`threshold`** — the accumulator floor. Events whose magnitude crosses this get disclosed; events below just add to the accumulator until the sum crosses. Our `1.5` touch events accumulate, our `2.0`/`3.0` edit events fire on their own.
- **`decay_threshold`** — how many quiet polls before the loop relaxes the elevated refractory threshold after a burst. If your sensor produces natural bursts (file edited rapidly, then goes quiet), 4–5 is a good value. See [`engagement.md`](engagement.md) for the full model.

There's no tuning ritual at this stage — pick sensible defaults, run it for a day, adjust if you're getting too many or too few notifications. Attend's [`attend tune`](configuration.md#attend-tune) command can also derive some parameters from session history.

## Step 7 — Ship it

You already did the install. For completeness, here's the deploy checklist:

1. **Script lives in a trusted location.** `$XDG_DATA_HOME/attend/sensors/`, `~/bin/`, a project-scoped `.claude/sensors/`, or any absolute path you trust. Attend doesn't care where — it only cares that the path resolves and the script respects the contract.
2. **Script is executable.** `chmod +x`.
3. **Config block is in `~/.config/attend/config.yaml`** (user scope) or `<project>/.claude/attend.yaml` (project scope — overlays on top of user-scope). See [`configuration.md`](configuration.md) for the overlay semantics.
4. **`enabled: true`.** The shipped examples ship disabled so you read them before turning them on; your own sensor can start enabled.
5. **Restart attend.** Stop the running Monitor task, start a new one. Config changes need a fresh process; binary changes get picked up automatically via self-reload.
6. **Watch the startup banner.** The first notification after attend restarts lists the active sensors. If your sensor isn't in the list, the config didn't parse — check indentation and re-read [`configuration.md`](configuration.md) § "Parser notes".

## What you just learned

You now know every piece of the sensor-authoring pattern:

- **Subprocess contract** (stdout as events, `magnitude|description`, exit 0 on silence, 10 second timeout)
- **Marker-file state tracking** so your sensor survives attend restarts
- **First-run silence** so a fresh marker doesn't flood the session
- **Graceful degradation** — a quiet poll is a valid outcome for every failure mode
- **Magnitude as the author's lever** — the one place you encode *how much each kind of change matters*
- **Config plumbing** — declaring the sensor, choosing intervals, understanding threshold and decay

Everything else — the shipped examples under `tools/attend/examples/`, the GitHub Project walkthrough in [`authoring-sensors.md`](authoring-sensors.md#walkthrough-github-project-sensor), the built-in crate sensors at [`sensors.md`](sensors.md) — is variations on this same shape. Read a few and the pattern will start to feel natural.

## Where to go next

- **[`authoring-sensors.md`](authoring-sensors.md)** — the reference doc. Everything this tutorial glossed over lives there, plus a walkthrough of a more involved network-API sensor (the GitHub Project example).
- **[`sensors.md`](sensors.md)** — what each built-in sensor observes and how it chose its magnitude table. Good for calibrating your own numbers against a known surface.
- **[`engagement.md`](engagement.md)** — the action potential model, which is what ultimately decides whether your event becomes a notification or gets eaten by refractory. Read this *after* you've shipped a sensor and started wondering why it stopped firing.
- **`tools/attend/examples/xdg-downloads.sh`, `gh-notifications.sh`** — the two shipped reference sensors. Each demonstrates a different shape: local filesystem scan, and remote API stream keyed on a rolling timestamp. (Watching one PR's CI is intentionally a Monitor concern, not a sensor — see ADR-137.)
- **[`configuration.md`](configuration.md)** — when you want to declare your sensor at project scope, use the permissions audit, or understand the overlay semantics.
