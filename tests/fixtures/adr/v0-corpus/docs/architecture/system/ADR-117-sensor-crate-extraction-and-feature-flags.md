---
status: Accepted
date: 2026-04-10
deciders:
  - aaronsb
  - claude
related:
  - ADR-113
  - ADR-115
---

# ADR-117: Sensor Crate Extraction and Feature Flags

## Context

attend (ADR-113) ships with four built-in sensors: context, git, peers, and processes. These are compiled directly into the attend binary as modules under `src/sensors/`. The ScriptSensor runner (shipped in PR #4) provides an extensibility path for shell-script sensors, but built-in sensors have no separation from the orchestrator.

This creates several problems:

- **No isolation** — sensor code shares the same module tree as the tick loop, state management, and CLI. Changes to one sensor can break another through shared internal interfaces.
- **No selective compilation** — a user who only wants git and context awareness still compiles peer discovery and process scanning. On constrained systems or custom builds, this matters.
- **No clear trait contract** — the `Sensor` trait lives in `sensors/mod.rs` alongside the `SensorSlot` runtime scaffolding. The boundary between "what a sensor must implement" and "how the orchestrator runs it" is blurred.
- **Future daemon model** — if attend becomes long-running (paralleling the direction noted for ways-cli), hot-reloading sensors or dynamically enabling them requires a cleaner separation than in-binary modules.

Meanwhile, the workspace already demonstrates the crate extraction pattern: `agent-fmt` was extracted from ways-cli (PR #3) and is consumed by both tools. The same pattern applies here.

## Decision

Extract each built-in sensor into its own workspace crate. Introduce a `sensor-trait` crate that defines the `Sensor` trait, `Focus` struct, and supporting types. Wire sensors into attend via Cargo feature flags.

### Workspace Structure

```
tools/
  agent-fmt/           # shared terminal formatting
  sensor-trait/        # Sensor trait, Focus, SensorSlot
  sensor-git/          # GitSensor
  sensor-context/      # ContextSensor
  sensor-peers/        # PeerSensor
  sensor-processes/    # ProcessSensor
  attend/              # orchestrator — depends on sensor crates via features
  ways-cli/            # ways CLI
```

### Trait Crate

`sensor-trait` defines the contract between attend and any sensor:

```rust
pub trait Sensor: Send {
    fn name(&self) -> &str;
    fn poll(&mut self, focus: &Focus) -> Vec<(f64, String)>;
    fn emission_threshold(&self) -> f64;
    fn base_interval(&self) -> Duration;
    fn min_interval(&self) -> Duration;
    fn export_state(&self) -> Vec<(String, String)> { Vec::new() }
    fn import_state(&mut self, _state: &[(String, String)]) {}
}
```

The `Send` bound prepares for the daemon model where sensors may run on separate threads. `Focus` and `SensorSlot` also move to this crate since they define how the orchestrator interacts with sensors.

### Feature Flags

attend's `Cargo.toml`:

```toml
[features]
default = ["sensor-git", "sensor-context", "sensor-peers", "sensor-processes"]
sensor-git = ["dep:sensor-git"]
sensor-context = ["dep:sensor-context"]
sensor-peers = ["dep:sensor-peers"]
sensor-processes = ["dep:sensor-processes"]
```

The orchestrator uses `#[cfg(feature = "sensor-git")]` guards around sensor registration. A minimal build with `--no-default-features` compiles only the orchestrator + script sensor runner.

### Two Sensor Paths

This ADR formalizes the two-path model that emerged from PR #4:

| Path | Implementation | Performance | Extensibility | Compilation |
|------|---------------|-------------|---------------|-------------|
| **Crate sensors** | Rust, compiled in | Native | Requires recompile | Feature-flagged |
| **Script sensors** | Shell, external | Process fork per poll | No recompile | Config-driven |

Both paths are controlled by the same declarative config (ADR-115). A crate sensor and a script sensor with the same name: the crate sensor wins (compiled-in takes precedence). This allows a script sensor to prototype behavior that later graduates to a crate sensor.

### Config as Control Plane

The `attend.yaml` config (ADR-115) controls all sensors uniformly:

```yaml
sensors:
  git:
    interval: 30
    threshold: 2.0
  -processes:            # disable a built-in sensor
  +disk-pressure:        # add a script sensor
    script: scripts/check-disk.sh
    interval: 120
```

Disabling a crate sensor via config (`-processes`) means it's never instantiated at runtime even though the code is compiled in. Disabling via feature flag (`--no-default-features`) means the code isn't compiled at all. Config is the user-facing control plane; features are the build-time control plane.

## Consequences

### Positive

- **Clear contracts** — `sensor-trait` is the documented interface. Any crate implementing `Sensor` can be wired into attend.
- **Selective builds** — custom attend binaries for constrained environments.
- **Isolation** — sensor bugs don't leak across module boundaries. Each sensor has its own dependency tree.
- **Testability** — sensors can be unit-tested in isolation against a mock `Focus`.
- **Graduation path** — script sensor → crate sensor is a defined workflow: prototype in shell, promote to Rust when performance matters.
- **Daemon-ready** — `Send` bound and crate isolation prepare for concurrent sensor polling.

### Negative

- **More crates** — workspace goes from 3 to 7 members. Cargo handles this well but it's more manifests to maintain.
- **Cross-crate changes** — modifying the `Sensor` trait requires updating all sensor crates. Mitigated by keeping the trait stable and small.
- **Initial migration effort** — moving existing sensor code is mechanical but touches every sensor file.

### Neutral

- **No behavioral change** — the default feature set compiles all sensors, reproducing current behavior exactly.
- **ScriptSensor stays in attend** — it's part of the orchestrator (runs any script), not a specific sensor implementation.
- **Config format unchanged** — ADR-115's config works identically before and after extraction.

## Implementation Plan

1. Create `sensor-trait` crate with `Sensor`, `Focus`, `SensorSlot`, `AdaptiveInterval`, `DeltaAccumulator`
2. Create `sensor-git` crate, move `sensors/git.rs` content, depend on `sensor-trait`
3. Create `sensor-context` crate, move `sensors/context.rs`
4. Create `sensor-peers` crate, move `sensors/peer.rs` (largest, has signal reading)
5. Create `sensor-processes` crate, move `sensors/process.rs`
6. Update attend `Cargo.toml` with feature flags, `#[cfg]` guards in sensor registration
7. Update `sensors/mod.rs` to re-export from crates (compatibility shim, removable later)
8. Verify `make lint`, `make attend-rebuild`, all existing behavior preserved
9. Test minimal build: `cargo build -p attend --no-default-features`
10. Update CI workflow to test both default and minimal feature sets
