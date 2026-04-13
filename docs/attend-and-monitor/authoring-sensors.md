# Authoring sensors

Sensor authorship is a first-class design surface in attend. A sensor is not a log tail — it's a module that translates raw environmental change into *magnitude-weighted observations* that feed attend's engagement model (ADR-119) and disclosure governor. A well-designed sensor encodes how much each kind of change matters in the magnitude, and lets the loop handle when to fire, how often, and whether to suppress.

This page is for people building sensors, in either of attend's two implementations.

## The sensor-layer contract

Sensors in attend refuse to editorialize. Each sensor reports a **fact the framework can verify** rather than a guess the sensor produced. The token ledger is authoritative; the signal file is authoritative; the working tree is authoritative. The sensor surfaces state transitions; the consuming agent does whatever synthesis it wants on top.

The discipline was crystallized during a live peer-agent validation of the `sensor-disclosure` crate, when a second Claude instance observing the same mechanism wrote:

> "The discipline is refusing to editorialize at the sensor layer and letting the consuming agent do whatever synthesis it wants on top. Word-boundary chunking preserves that contract across the transport boundary, which is the only place prose fidelity actually matters."
>
> — *claude//home/aaron/temp, during sensor-disclosure validation, 2026-04-13*

When you build a sensor, that's the bar: emit measurable transitions, not simulated judgment. If your description text is about to contain a verb like "should," "probably," or "likely" — stop and reframe as observation. The sensor does not decide whether the state is good, bad, urgent, or ignorable; the consuming agent does. The sensor's job is to hand that agent a verifiable fact at the right moment, nothing more.

This constraint is what makes the rest of the document tractable. Magnitudes, thresholds, engagement states, the disclosure governor — all of that machinery is how attend turns a stream of honest observations into a useful attention signal. If sensors start editorializing, the machinery is compensating for corrupted input, and the whole stack's predictability collapses. Keep the sensor layer honest and the rest of the system can do its job.

## Two implementations

| | Crate sensor | External script sensor |
|---|---|---|
| **Language** | Rust | Any executable (usually bash) |
| **Linkage** | Compiled into the attend binary via a feature flag | Spawned as a subprocess on each poll |
| **State** | Shared process memory; can hold arbitrary Rust state across polls | Stateless between polls (unless the script persists state externally) |
| **Startup cost** | Free (already in memory) | Process spawn per poll — ~5–30 ms |
| **Timeout** | None (sensor owns its poll duration) | 10 seconds, enforced by attend |
| **Good for** | Built-in system sensors, high-frequency polling, complex state | Integrations with CLI tools, per-project sensors, quick experiments |
| **Example** | `sensor-context`, `sensor-git`, `sensor-peers`, `sensor-processes` | `gh`-cli GitHub Project watcher, `kubectl` pod status, `docker events` tail |

Both implementations land at the same place in the loop — attend calls `poll()` on a schedule, reads back a `Vec<(f64, String)>` of observations, and feeds them into the accumulator. The only difference is *how the code gets loaded and run*. Design-wise they're identical; pick the one that matches your integration and performance needs.

## What sensor authors are designing around

Before you write a single line of code, understand what your events will encounter once they leave your poll function. Every observation you emit goes through the same pipeline:

1. **Accumulator.** Your `(magnitude, description)` pair is added to the sensor's `DeltaAccumulator`. Magnitudes accumulate across ticks until the sensor is drained or decays.
2. **Emission threshold.** A per-sensor threshold — the accumulator has to exceed this before the sensor is a candidate to disclose. Low-magnitude events accumulate silently until several of them add up; a single high-magnitude event may cross threshold on its own.
3. **Engagement / refractory (ADR-119).** After the sensor recently fired a disclosure, its effective threshold is temporarily elevated (relative refractory) or it's fully suppressed (absolute refractory, ~60s by default). During refractory, new events still accumulate but don't fire until the cooldown passes — unless their magnitude is high enough to break through the elevated threshold. This is how attend models "disengagement after a burst": low-magnitude follow-ups get swallowed, truly urgent events still get through.
4. **Disclosure governor.** Even after the sensor is ready, a global governor rate-limits disclosures across the whole loop (default: 3 per 120s, with a 15s cooldown between them). If a burst of sensors all want to fire at once, some get held until the window rolls.
5. **Monitor delivery.** Whatever survives all of the above gets printed as one stdout line per event and picked up by Monitor (or the `attend chat` TUI) as an async notification into the conversation.

**The design implication**: *magnitude is the author's main lever*. Don't emit uniform-magnitude events; think carefully about which changes are loud and which are quiet. A sensor that emits magnitude 5.0 for every tick will flood the governor; one that emits 0.1 for everything will never fire. The right shape is usually a hierarchy: cheap background changes at 0.5–1.0, routine notable events at 2.0–3.0, things you want to break through refractory at 5.0+.

## Crate sensors — the `Sensor` trait

A crate sensor implements `sensor_trait::Sensor`:

```rust
pub trait Sensor: Send {
    fn name(&self) -> &str;
    fn poll(&mut self, focus: &Focus) -> Vec<(f64, String)>;
    fn emission_threshold(&self) -> f64;
    fn base_interval(&self) -> Duration;
    fn min_interval(&self) -> Duration;
    fn decay_threshold(&self) -> u32;

    // Optional — for sensors that need state across process restarts
    fn export_state(&self) -> Vec<(String, String)> { Vec::new() }
    fn import_state(&mut self, _state: &[(String, String)]) {}
}
```

The contract:

- **`name()`** — stable identifier used in logs, config, and state keys.
- **`poll(focus)`** — called whenever the sensor's turn comes up in the priority queue. Returns a list of `(magnitude, description)` pairs. Empty vec means "nothing changed this tick." The `Focus` argument carries the working directory, a human-readable description of current work, and any keywords attend knows about — use it to scope observations or skip irrelevant ones.
- **`emission_threshold()`** — the accumulator floor this sensor must cross before it's a disclosure candidate. Typical values 1.5–2.5.
- **`base_interval()`** — the slowest polling interval, used when the sensor has been quiet for a while. Typical 30–60 seconds.
- **`min_interval()`** — the fastest polling interval, used when the sensor is actively observing change. Typical 5–20 seconds.
- **`decay_threshold()`** — number of consecutive quiet polls before the interval grows back toward `base_interval`. Typical 3–5.
- **`export_state` / `import_state`** — if your sensor needs to remember anything across an attend restart (e.g., "which signals have I already seen?"), implement these. Attend checkpoints every 30 seconds.

To wire a new crate in, add it to `tools/attend/Cargo.toml` as an optional dep + feature, then register it in `tools/attend/src/sensors/mod.rs` with `register_builtin!`. See the existing sensors for reference.

## External script sensors — the subprocess protocol

An external sensor is declared in attend config:

```yaml
sensors:
  +github-project:
    script: $XDG_DATA_HOME/attend/sensors/github-project.sh
    interval: 120        # base interval in seconds
    min_interval: 30     # fastest interval
    threshold: 2.0       # emission threshold
    decay_threshold: 4   # quiet ticks before decay
    requires:
      - Bash(gh:*)       # permission audit (ADR-116)
```

The `+` prefix declares a new sensor beyond the built-ins.

**Where the script lives is up to you.** Attend deliberately imposes no single "sensors dir." A script path can be:

- **User-global** under `$XDG_DATA_HOME/attend/sensors/` — the XDG-convention default, survives across projects, lives in your own trusted data home
- **Project-scoped** at `<project>/.claude/sensors/*.sh` — only active when attend runs from that repo; useful for sensors tied to a specific codebase
- **An absolute path** to anywhere on disk — a personal tools repo, a team-shared scripts directory, `~/bin`, `/usr/local/lib/my-sensors/`, wherever you keep trusted executables

The config parser expands `$HOME`, `~`, `$XDG_CONFIG_HOME`, `$XDG_DATA_HOME`, `$XDG_STATE_HOME`, and `$XDG_CACHE_HOME` at load time, so `$XDG_DATA_HOME/attend/sensors/foo.sh` becomes an absolute path regardless of how attend is launched. This keeps configs portable across machines.

The script is executed via `bash` with cwd set to the working directory attend was launched in. No arguments are passed.

**Trust is the author's responsibility.** External sensors run arbitrary shell under your user — read every sensor before you enable it. Attend's design assumes sensors come from locations you already trust: your own data home, your own project dirs, your own tool repos. The parser will happily resolve any path, but if you point it at something you haven't audited, that's on you.

**The shipped examples.** Attend ships three reference external sensors in the agent-ways repo under `tools/attend/examples/`. Each demonstrates a different shape of sensor so authors have a range to compare before writing their own:

- **`xdg-downloads.sh`** — **local filesystem scan with count-based tiers.** Scans the XDG Downloads directory, diffs against a marker, emits once with magnitude tied to how many new files appeared. ~80 lines, most of them comments walking through the contract. Good first read — you only need to understand the diff-against-marker pattern to follow the whole script.
- **`gh-pr-checks.sh`** — **per-branch aggregate state machine.** Polls `gh pr checks` for the current branch's PR and emits only on terminal state transitions (pass / fail), keyed on repo-root + branch + PR number so closing and reopening a PR on the same branch doesn't carry stale state. Demonstrates aggregate-state modelling, marker-based state persistence, silent no-op when there's no PR, and magnitude tiers designed to break refractory on regressions without spamming on routine pushes.
- **`gh-notifications.sh`** — **network API stream keyed on a rolling timestamp.** Queries `gh api notifications?since=<marker>` each poll and emits one line per returned item, tiered by GitHub's `reason` field (review_requested loudest, comment quietest). Demonstrates the stream shape (no aggregation — every distinct event becomes a distinct emission), wall-clock-marker pattern (no need to remember seen IDs; the server does the filtering), and graceful degradation on auth / network failure.

Taken together the three cover the failure modes an author will hit in the wild: local state (xdg-downloads), remote state that needs rollup (gh-pr-checks), and remote state that arrives as a stream (gh-notifications). All three are disabled by default and all three have commented-out stub blocks in the user-scope default config. The intended workflow for any of them:

1. Copy the script from the agent-ways repo into `$XDG_DATA_HOME/attend/sensors/` (or any trusted location — `~/bin`, `.claude/sensors/`, a tools repo, absolute paths all work)
2. Read the script — it's all bash, most of it comments explaining the pattern
3. Open `$XDG_CONFIG_HOME/attend/config.yaml`, find the sensor's stub block, uncomment it if needed, and flip `enabled: true`
4. Restart attend

**The subprocess contract:**

- **Execution:** `bash <script>` each poll. No arguments passed in; the script can read env vars if it needs any (e.g., `$PWD`, `$HOME`).
- **Timeout:** 10 seconds. Attend kills the process and discards its output if the script runs longer.
- **Exit status:** Non-zero exit → attend silently discards any output. The sensor just reports "nothing observed this poll."
- **Stdout format:** One event per line, as `magnitude|description`. Whitespace around the pipe is trimmed. Unparseable lines are silently dropped (they don't fail the poll; they just don't become events).
- **Stderr:** Ignored by attend. Useful for debug logs that shouldn't leak into notifications.

Minimal example:

```bash
#!/usr/bin/env bash
# sensors/cargo-watch.sh — observe cargo build state

if pgrep -x cargo >/dev/null; then
  echo "1.0|cargo is running"
fi

if [[ -f target/debug/build-stamp ]]; then
  age=$(( $(date +%s) - $(stat -c %Y target/debug/build-stamp) ))
  if (( age < 60 )); then
    echo "2.5|build completed $age seconds ago"
  fi
fi
```

Each time attend polls this sensor, it gets 0, 1, or 2 events depending on state. The magnitudes encode the relative importance: a running cargo process is mildly notable (1.0), a fresh build is more so (2.5).

## Walkthrough: GitHub Project sensor

This is the canonical external sensor — a clean illustration of how *event magnitude is the author's design lever*. It's a feasible, useful sensor that most people would actually benefit from (unlike the more esoteric peer-messaging machinery). We're documenting the shape; writing the full implementation is a separate task.

### Motivation

A common workflow: you're coding, Claude is helping, and you have a GitHub Project board where issues move between columns (backlog → in progress → review → done). When an issue moves, something changed about *what you should be working on*, but you wouldn't know unless you alt-tab to the browser. A sensor can watch the board and surface those moves as attend signals without interrupting the conversation.

### Config

```yaml
sensors:
  +github-project:
    script: .claude/sensors/github-project.sh
    interval: 300           # poll every 5 minutes by default
    min_interval: 60        # speed up to 1 min when activity is detected
    threshold: 2.5          # assigned-to-me moves cross this on a single event
    decay_threshold: 3
    requires:
      - Bash(gh:*)
```

The sensor runs in the project's working directory. The script uses `gh` CLI and assumes the user has authenticated (`gh auth login`).

### Script skeleton

```bash
#!/usr/bin/env bash
# .claude/sensors/github-project.sh
#
# Watches the GitHub Project associated with this repo.
# Emits magnitude-weighted events for card state changes.

set -euo pipefail

STATE_DIR="$HOME/.cache/attend/sensors/github-project"
mkdir -p "$STATE_DIR"

# Identify the repo and current git user
REPO=$(gh repo view --json nameWithOwner -q .nameWithOwner 2>/dev/null) || exit 0
ME=$(gh api user -q .login 2>/dev/null) || exit 0
STATE_FILE="$STATE_DIR/$(echo "$REPO" | tr '/' '-').state"

# Fetch the project board for this repo (first linked project, if any)
BOARD_JSON=$(gh project item-list --owner "@me" --format json --limit 100 2>/dev/null) || exit 0

# Extract (item_id, status, assignee) tuples
CURRENT=$(echo "$BOARD_JSON" | jq -c '.items[] | {id, status: .status, assignee: (.assignees[0].login // "")}')

# Compare against last snapshot
if [[ -f "$STATE_FILE" ]]; then
  PREV=$(cat "$STATE_FILE")
  # For each item, check if status changed
  echo "$CURRENT" | while read -r item; do
    id=$(echo "$item" | jq -r .id)
    cur_status=$(echo "$item" | jq -r .status)
    cur_assignee=$(echo "$item" | jq -r .assignee)
    prev_status=$(echo "$PREV" | jq -r "select(.id == \"$id\") | .status" 2>/dev/null || echo "")

    if [[ -n "$prev_status" && "$cur_status" != "$prev_status" ]]; then
      # State transition detected — encode magnitude based on assignment
      if [[ "$cur_assignee" == "$ME" ]]; then
        # Assigned to me — strong signal, single event should break through
        echo "3.0|issue #$id moved: $prev_status → $cur_status (assigned to you)"
      else
        # Unassigned or someone else — weak signal, accumulates
        echo "0.8|issue #$id moved: $prev_status → $cur_status"
      fi
    fi
  done
fi

# Save current snapshot for next poll
echo "$CURRENT" > "$STATE_FILE"
```

### Magnitude design — the nuance

The interesting design decision is in the two `echo` lines at the end. Both are emitting an event for the same category of change (a card moved on the board), but with very different magnitudes:

- **Assigned to me → 3.0.** Your emission threshold is 2.5. A single event with magnitude 3.0 clears the bar on its own, immediately. If you're the assignee and the card moved, you probably want to know right now.
- **Not assigned to me → 0.8.** A single event sits below threshold and accumulates silently. One unassigned move is noise; attend won't bother you. But if the board has four unassigned moves in a polling window, they add up to 3.2 — now the threshold is crossed and attend discloses "several items moved on the board" collectively. That's the shape of useful awareness: individual moves are ignorable, aggregate activity is informative.

This is the engagement model working as designed. The author didn't have to write any threshold logic, rate limiting, or refractory handling. They just encoded *how much each kind of change matters* into the magnitude, and attend handled the rest.

**Further refinements the author could add** (all by adjusting magnitudes, not by touching attend):

- A PR review requested on your own PR → 4.0 (break through refractory even during quiet focus)
- A CI failure on your in-progress branch → 5.0 (never suppress)
- A label change on any card → 0.3 (barely nudges accumulator; aggregate only)
- A new card created in the backlog → 0.5 (background awareness)

Each of these is a single-line change. The magnitude number is the entire design surface.

### What attend does with the events

Once the script's stdout reaches attend:

1. Each valid `magnitude|description` line becomes one event.
2. Events accumulate in the sensor's `DeltaAccumulator` between polls.
3. When the accumulator exceeds the emission threshold (2.5 for this sensor), the sensor becomes a disclosure candidate.
4. The disclosure governor decides whether to actually fire (respecting cooldown, rate window, and any active refractory from ADR-119).
5. If fired, each event becomes one Monitor notification line delivered into the conversation (or rendered in `attend chat` if the consumer is a human).

The sensor author never touches any of that machinery. They just write `gh`-CLI glue and pick magnitudes.

## Design rules of thumb

Written as heuristics, not commandments:

1. **Design magnitudes as a hierarchy, not a scale.** Don't linearly interpolate magnitudes across your event types. Pick a few discrete tiers (background, notable, urgent, critical) and assign each event type to one.
2. **Your background events should sum to something useful.** If your sensor only ever emits 0.5s, those 0.5s should aggregate to meaningful disclosure when enough of them happen. Check: does 3–5 background events add up to cross threshold?
3. **Reserve the top tier for actual emergencies.** If your sensor can emit a 5.0 event, it should be the kind of thing that needs to break through an ongoing conversation about something else. Abuse of the top tier trains the user to ignore attend.
4. **Prefer empty polls to noisy polls.** When nothing is happening, return an empty vec. The loop handles quiet — it slows the sensor's polling frequency, lets the engagement state relax, and stays out of the user's way. An always-chatty sensor is fighting the loop.
5. **Make poll idempotent.** The script should produce the same output for the same state regardless of when it's called. Attend handles "what changed" via the accumulator; you just report the current situation. State tracking inside your script is fine (as in the GH example), but use it to detect transitions, not to throttle.
6. **Fail silently and cheaply.** If your upstream is unavailable (network down, `gh` not authed, API rate-limited), exit 0 with no output. Don't emit error events — they pollute the notification stream. Use stderr if you need to log for debugging.
7. **Respect the 10-second timeout.** External sensors have a hard wall. If your work might take longer, cache aggressively or split into a fast poll + an async background job that writes a state file the poll reads.

## Related

- [`loop.md`](loop.md) — the substrate your sensor plugs into
- [`engagement.md`](engagement.md) *(planned)* — the action potential model in detail
- [`sensors.md`](sensors.md) *(planned)* — reference for the built-in sensors
- [`configuration.md`](configuration.md) *(planned)* — config schema for declaring sensors
- **ADR-117** — sensor crate extraction and feature flags
- **ADR-119** — action potential engagement model
- **ADR-116** — permission requirements for sensors
