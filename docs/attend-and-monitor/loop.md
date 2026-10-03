# The attend loop

Attend is a single long-lived process that runs a timer-driven sensor loop. It has no threads, no async runtime, no event subscriptions. Each iteration looks at a priority queue of upcoming sensor polls, sleeps until the next one is due, polls everything that's ready, decides whether to emit, and goes back to sleep.

This page covers the rhythm of one iteration, the phases a sensor passes through inside that iteration, and how observations leave attend and reach whoever is listening. It's the substrate document — every other page in this directory refers back to something here.

The loop serves one Claude session: `attend run` prints its lines to stdout, and Monitor delivers each line as a notification. A human reads the same message bus through attend-chat ([`tui.md`](tui.md)), which watches the signal files directly and does not run this loop.

## The rhythm

Attend's loop is **pull-based, not push-based**. Nothing wakes it up — it wakes itself up at timestamps it already knows about, does one unit of work, and goes back to sleep. The only external input is the filesystem: signal files other agents write, git state, process list, transcript changes. Sensors read these during their own scheduled polls.

The implication for latency: attend's response time to an external change is bounded by the polling interval of the sensor that watches the relevant thing, not by the loop's "tick rate" (which doesn't really exist). A peer message arrives on disk immediately, but the peer sensor won't see it until its next poll — `min_interval` 10 seconds by default.

The implication for cost: when nothing is happening, attend is asleep almost all the time. A quiet terminal with nothing changing is essentially free.

## Startup

Before the loop begins, `cmd_run_with_catchup` builds up the context it needs:

1. **Focus** — resolve the working directory and human-readable description (`Focus::default_focus()`).
2. **Config** — load `~/.config/attend/config.yaml`, then overlay `<cwd>/.claude/attend.yaml` on top (ADR-115 pattern).
3. **Groups manager** — construct a `Groups` handle for the signals base and the current session ID, the handle channels ride on.
4. **Sensor registration** — `sensors::register_sensors()` walks the config and feature flags, instantiating each enabled sensor with its configured intervals and thresholds. The peers sensor receives a closure that lists the joined channel directories, so it picks up joins and leaves on every scan.
5. **Engagement state** — apply ADR-123 action-potential parameters to every slot. Refractory behavior is per-sensor but the parameters are shared.
6. **State restore** — if `$XDG_CACHE_HOME/attend/state/<session-id>.state` exists from a previous run, import the saved seen-signals and disclosed thresholds so a restart is continuous. A run started after `/clear` first moves the old session id's state to the new one (see [`delivery.md`](delivery.md#when-the-session-id-changes)).
7. **Banner** — print a startup line unless the fingerprint (version + commit + sensor list + focus) matches the last one written to `_last_banner`, in which case print `[attend] restarted (unchanged)` to keep noisy Monitors quiet.
8. **Governors** — build the event lane's `DisclosureGovernor` from the configured cooldown, rate window and maximum per window, and the message lane's permissive one (limits in [`delivery.md`](delivery.md#the-monitor-line)).
9. **Priority queue** — push every sensor slot into a `BinaryHeap<ScheduledSensor>` keyed by `fire_at`.
10. **Timers** — record startup `Instant`s for checkpoint, cleanup, and self-reload checks.

Then the loop begins.

## One iteration

```mermaid
flowchart TD
    Start([loop iteration])
    RekeyCheck{session id<br/>changed?}
    Rekey[flush held message lines<br/>checkpoint, exec self]
    ReloadCheck{binary mtime<br/>changed?}
    Exec[checkpoint state<br/>exec self]
    PeekQueue{queue<br/>empty?}
    Break([break])
    Sleep[sleep until<br/>next fire_at]
    Poll[poll every due sensor<br/>accumulate, reschedule]
    Lane{lane}
    MsgReady{anything<br/>accumulated?}
    EvtReady{threshold crossed<br/>and not refractory?}
    MsgGov{message governor<br/>permits?}
    EvtGov{event governor<br/>permits?}
    Emit[emit batch to stdout<br/>event lane: record engagement]
    Hold[hold, magnitude stays<br/>on the accumulator]
    Housekeeping[checkpoint if due<br/>cleanup if due]
    Continue([next iteration])

    Start --> RekeyCheck
    RekeyCheck -- yes --> Rekey
    RekeyCheck -- no --> ReloadCheck
    ReloadCheck -- yes --> Exec
    ReloadCheck -- no --> PeekQueue
    PeekQueue -- yes --> Break
    PeekQueue -- no --> Sleep
    Sleep --> Poll
    Poll --> Lane
    Lane -- "peers, keepwarm" --> MsgReady
    Lane -- "context, git, processes, disclosure" --> EvtReady
    MsgReady -- yes --> MsgGov
    EvtReady -- yes --> EvtGov
    MsgReady -- no --> Housekeeping
    EvtReady -- no --> Housekeeping
    MsgGov -- yes --> Emit
    EvtGov -- yes --> Emit
    MsgGov -- no --> Hold
    EvtGov -- no --> Hold
    Emit --> Housekeeping
    Hold --> Housekeeping
    Housekeeping --> Continue

    classDef terminal fill:#475569,stroke:#4a5568,color:#ffffff
    classDef decision fill:#fbbf24,stroke:#4a5568,color:#1a1a1a
    classDef process fill:#2d7d9a,stroke:#4a5568,color:#ffffff
    classDef boundary fill:#f6821f,stroke:#4a5568,color:#1a1a1a
    classDef store fill:#2d8e5e,stroke:#4a5568,color:#ffffff

    class Start,Break,Continue terminal
    class RekeyCheck,ReloadCheck,PeekQueue,Lane,MsgReady,EvtReady,MsgGov,EvtGov decision
    class Sleep,Poll,Emit,Hold process
    class Exec,Rekey boundary
    class Housekeeping store
```

The session-id check runs every iteration. When Claude Code's `/clear` has given the session a new id, the run prints any message line its governor still holds, checkpoints, and re-executes itself; the new process moves the state to the new id before reading it.

The reload branch is a hard exit — `execve(2)` replaces the current process image with a fresh copy of the binary. State is checkpointed first, and the new process restores from that checkpoint during startup, so observed signals, engagement history, and disclosed context thresholds survive the reload.

The queue-empty branch is a defensive break. In practice it's unreachable because every poll reschedules the sensor, but a future sensor that explicitly retires could drop out of the queue.

## The sensor queue

`BinaryHeap<ScheduledSensor>` is a min-heap ordered by `fire_at` — the soonest-scheduled sensor is always at the top. The loop peeks it to find the next wakeup, sleeps precisely that long (no jitter, no coarse tick), then drains *every* sensor whose `fire_at` has passed.

Draining is important: if two sensors both came due during the sleep, they both poll in this iteration. This keeps the loop from falling behind during bursts.

After each poll, the sensor's `schedule_next()` computes its next `fire_at` based on whether it observed anything, the action-potential engagement state, and its `min_interval` floor. The updated entry gets pushed back into the heap.

Scheduling dynamics:

- **Quiet sensor**: interval grows toward `base_interval()`. Polls become less frequent.
- **Active sensor**: interval shrinks toward `min_interval()`. Polls become more frequent.
- **Refractory sensor**: interval still runs, but the effective threshold is elevated so polls that would normally cross threshold are suppressed. See [`engagement.md`](engagement.md).

## One sensor's lifecycle within an iteration

Each drained sensor walks through a short state machine before control returns to the loop body:

```mermaid
stateDiagram-v2
    [*] --> Resting
    Resting --> Polling: fire_at ≤ now
    Polling --> Quiet: sensor reports no change
    Polling --> Changed: sensor reports change

    Quiet --> Rescheduled
    Changed --> GovernorRecord: record_event()
    GovernorRecord --> ReadyCheck: ready_to_disclose()?

    ReadyCheck --> Ready: threshold crossed<br/>not in refractory
    ReadyCheck --> Held: threshold crossed<br/>in absolute refractory
    ReadyCheck --> Accumulating: below threshold

    Ready --> Rescheduled: appended to<br/>ready_indices
    Held --> Rescheduled: magnitude preserved<br/>for next poll
    Accumulating --> Rescheduled: magnitude preserved<br/>for next poll

    Rescheduled --> [*]: schedule_next()<br/>pushed back into heap

    classDef resting fill:#475569,stroke:#4a5568,color:#ffffff
    classDef process fill:#2d7d9a,stroke:#4a5568,color:#ffffff
    classDef ok fill:#2d8e5e,stroke:#4a5568,color:#ffffff
    classDef waiting fill:#fbbf24,stroke:#4a5568,color:#1a1a1a
    classDef held fill:#f6821f,stroke:#4a5568,color:#1a1a1a

    class Resting resting
    class Polling,GovernorRecord,ReadyCheck,Rescheduled process
    class Quiet,Ready ok
    class Changed,Accumulating waiting
    class Held held
```

**Quiet** is the happy path when nothing changed. The sensor's interval grows, its accumulator stays at zero, the loop logs nothing, and it sleeps until next fire.

**Changed but below threshold** means the sensor saw something but the magnitude isn't high enough to emit yet. The event is accumulated in the sensor's `DeltaAccumulator` and will be combined with future events if they arrive before the accumulator decays.

**Changed, above threshold, but held in absolute refractory** means the action potential (ADR-123) is actively suppressing this sensor after a recent burst. The magnitude stays on the accumulator but the sensor is not added to `ready_indices`. The log line `held in absolute refractory` marks this case.

**Ready** means the sensor crossed threshold and engagement state permits disclosure. The sensor index is appended to `ready_indices`, which the loop processes in a batch after draining.

The state machine above is the event lane's. A message-lane sensor (`peers`, `keepwarm`) is ready whenever anything has accumulated: it has no threshold check and no refractory. The peers sensor also checkpoints at once after any poll that found a message, so the Stop-hook drain sees what it consumed.

## Disclosure and the governor

After all due sensors are polled, the ready ones split by lane, and each lane asks its own `DisclosureGovernor`. The governor is alarm management (ISA-18.2) applied to the notification channel: it bounds how often attend may interrupt, no matter how much the sensors want to say. Each enforces two limits:

| | Event lane | Message lane |
|---|---|---|
| Cooldown between disclosures | `governor.base_cooldown` (default 15 s) | fixed, see [`delivery.md`](delivery.md#the-monitor-line) |
| Disclosures per window | `governor.max_per_window` per `governor.rate_window` (default 3 per 120 s) | fixed, see [`delivery.md`](delivery.md#the-monitor-line) |

If the lane's governor permits, the loop builds a batch:

1. For each ready sensor, compute a priority from its accumulated magnitude: `high` at 5.0, `medium` at 3.0, `low` below.
2. Drain the sensor's events into a list of observations.
3. Append `(sensor_name, priority, observations)` to the batch.
4. Hand the batch to `emit::emit_batch()` which formats each event as a single Monitor-visible line.

Medium and high events print to stdout as `[attend sensor=<name> priority=<p>] <event>`, one line each, and Monitor delivers each line as a notification. Low events go to stderr; when a batch has both, one `[attend] also: N quiet event(s) from …` line counts the low ones, and a batch of only low events wakes nothing. Monitor truncates lines around 400 characters, which is why the peers sensor splits long messages.

After emission, the loop records a disclosure on that lane's governor. On the event lane it also records engagement for every sensor whose magnitude reached 3.0, so quiet filler does not escalate the refractory. The message lane records no engagement.

If the governor rejects the batch, the magnitudes stay on the accumulators and the loop logs `N sensors ready but governor holding (X/Y in window)`. The next time the governor window rolls, those accumulated events will fire together.

## Signal flow end-to-end

```mermaid
sequenceDiagram
    autonumber
    participant World as External state<br/>(git, peers, processes)
    participant Sensor
    participant Slot as SensorSlot
    participant Engagement as EngagementState
    participant Governor as DisclosureGovernor
    participant Emit as emit::emit_batch
    participant Monitor as Claude Code Monitor
    participant Agent as Conversation

    rect rgba(45,125,154,0.12)
    World->>Sensor: filesystem / process state changes
    Note right of Sensor: waits until next poll

    Slot->>Sensor: poll(focus)
    Sensor->>Slot: (changed, events)
    Slot->>Slot: accumulate magnitude
    Slot->>Governor: record_event() if changed
    Slot->>Engagement: ready_to_disclose()?
    Engagement-->>Slot: yes / held / absolute refractory
    end

    rect rgba(217,119,6,0.12)
    Slot->>Governor: can_disclose()?
    Governor-->>Slot: yes / rate-limited
    end

    rect rgba(45,142,94,0.12)
    Slot->>Emit: batch of (name, priority, events)
    Emit->>Monitor: println! — one line per event
    Monitor->>Agent: async notification

    Note over Governor,Engagement: Governor.record_disclosure()<br/>Engagement.record_fire(epoch_secs, 1.0) per sensor
    end
```

This is the one-way data flow. Signals enter via sensors (reading the environment) and leave via stdout (intercepted by Monitor). Nothing in attend's runtime is event-driven — every transition is a poll that happened to find something new.

## Timers running in parallel

All of these tick inside the same single-threaded loop using `Instant::now()` comparisons against stored last-time values:

| Timer | Interval | Purpose |
|---|---|---|
| Per-sensor poll | `slot.next_fire` (varies) | Primary loop rhythm — when to poll each sensor |
| Session-id check | every iteration | Follow `/clear` to the new session id |
| Self-reload check | 10s | Detect binary change, exec self |
| Checkpoint | 30s, and at once after a peers poll that found a message | Save sensor state snapshot for restart continuity and for the drain |
| Cleanup sweep | `cleanup.interval` (default 600s) | Prune stale signal files + empty project dirs |
| Sensor min_interval | per-sensor (default 10–20s) | Floor on polling frequency |
| Sensor base_interval | per-sensor (default 30–60s) | Ceiling — rest-state polling frequency |
| Event governor cooldown | `governor.base_cooldown` (default 15s) | Minimum gap between event-lane disclosures |
| Event governor rate window | `governor.rate_window` (default 120s) | Rolling window for the event-lane rate limit |
| Message governor | fixed ([`delivery.md`](delivery.md#the-monitor-line)) | The message lane's limits |
| Absolute refractory | `engagement.absolute_refractory` (default 60s) | Full suppression after burst — the `Curve::ActionPotential` hard gate |
| Multiplier half-life | derived from `engagement.decay_per_minute` (default 0.1 → ~395s) | Exponential half-life of the relative-refractory multiplier's decay back toward 1.0 (ADR-123) |

Attend's engagement parameters are all in wall-clock seconds because attend's progression axis is `sensor_trait::epoch_secs()`. Under ADR-123 the burst window is implicit in the multiplier's half-life rather than a parameter of its own; see [`engagement.md`](engagement.md#event-count-burst-detection).

The loop doesn't coordinate these timers explicitly — each is a separate "has enough time passed since the last time we did this?" check at the natural point in the iteration. The loop body walks through them in a fixed order so behavior is deterministic.

## Loop termination

Four ways the loop ends:

1. **`execve` via self-reload.** The current process image is replaced and control never returns to the loop. State is checkpointed first so the replacement process restores cleanly.
2. **`execve` after a session-id change.** The same, after flushing held message lines, so the new process starts under the new id. If the exec fails, the run says so on the Monitor and exits.
3. **Queue empty.** Defensive `break` — currently unreachable because every poll reschedules the sensor, but a future explicit-retire mechanism could drop out.
4. **External signal (SIGTERM, etc).** Default handling — the process exits. The most recent checkpoint is the restore point; any accumulated-but-not-disclosed magnitude since then is lost. This is acceptable because accumulation is ephemeral by design.

There's no graceful shutdown hook. The `attend run` process is meant to be started via Monitor, live as long as the session lives, and die when the session ends. The checkpoint timer is aggressive enough (30s) that losing <30s of accumulation is the worst case.

## Where to go from here

- **Writing new sensors**: [`authoring-sensors.md`](authoring-sensors.md) — how to implement a crate sensor or external script sensor that plays nicely with this loop.
- **Human mode**: [`tui.md`](tui.md) — the `attend chat` interactive TUI, same signal bus as the agent side.
- **Sensors individually**: [`sensors.md`](sensors.md) covers what each built-in observes and how it emits.
- **Engagement curve**: [`engagement.md`](engagement.md) covers the action potential model — why event-lane sensors go quiet after bursts.
- **Message delivery**: [`delivery.md`](delivery.md) covers the Monitor line and the Stop-hook drain.
- **Signal format**: [`signals.md`](signals.md) covers the wire format, on-disk layout, and lifecycle.
- **Configuration**: [`configuration.md`](configuration.md) covers the YAML overlay and how to reshape any of the timers above.
