---
description: a sandboxed evaluative loop for behaviour inside a platform that is risky or hidden to run directly, such as a compositor effect, window manager, kernel module, device or shared service; run a nested isolated instance, send it commands, read a text state dump, inject real input, and confirm ground truth by a separate route
vocabulary: test session leaked into my real desktop nested instance sandbox isolated virtual compositor window manager wayland kwin x11 plugin effect extension kernel module device emulator vm container namespace dbus host desktop leak command channel state dump state line injected input fake input probe window ground truth separate route side effect isolation audit
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: heuristic -->
# Sandboxed Loop

The subject runs inside a platform: a compositor, a desktop shell, a kernel, a device, a shared service. Running it on the agent's own host risks the operator's live session, and the platform hides most of what happens. The loop runs a second, isolated instance of the real platform and builds a channel into the subject.

## Run the real platform, isolated

- **A nested or virtual instance**, started with its own configuration, its own session bus, its own data and cache directories, and a name so several can run at once. Prefer a virtual output: a nested window on the live desktop slows or stalls when it is covered.
- **Check the isolation instead of assuming it.** Helper processes inherit environment variables that point back at the live session, and a daemon started inside the nest can write to the operator's real configuration. List what the nest touches on the host (file modification times under the home directory, the live platform's log) and treat any change there as a failure.
- **Know the tell for harm to the host.** Every platform has one: a stall warning in the live session's journal, a reload of the live configuration. Watch for it during every run.
- **Bound the agent's own commands.** A filesystem search that walks network mounts, or a render loop in a windowed nest, can load the host as badly as the subject can (see environment/bounded-execution).

## Build a channel into the subject

- **A command channel.** When the platform gives the subject no IPC of its own, route commands through what it does read, such as a configuration key and a sequence number the subject watches. Add verbs as tests need them: open, move, zoom, place, reset.
- **A text state readout after every command.** The subject writes its full state as one parseable line, plus a line per object. Tests assert on numbers. A screenshot comparison is the second instrument, used where only the picture can show the defect.
- **Real input.** Inject pointer and key events through the platform's input protocol, not by calling the subject's handlers.

## Read ground truth by a separate route

The subject's own state line reports what it believes. Confirm the effect through something else: the platform's scripting interface listing real window geometry and stacking order, or a probe window that logs every press it receives with local coordinates. When the two disagree, the subject is wrong until shown otherwise.

## When the nest and the real platform differ

Record each known divergence where the next session will read it: a reload that serves a cached component, a reconfigure that skips newly enabled plugins, a late-loading plugin that reads as failed. A suite that depends on one platform build checks the build and skips, loudly, on any other.

## See Also

- evaluative(evaluative) — parent: author and judge, the shared core, choosing the loop
- environment/container-safety(softwaredev) — least privilege for container-based sandboxes
- environment/bounded-execution(softwaredev) — commands that never return or load the host
- environment/hostparity(softwaredev) — a pass in the nest is evidence about the nest
