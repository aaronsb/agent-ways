---
id: 05.001.H
domain: platform
mode: how-to
related: []
aliases: []
---

# Fix a way-embed SIGKILL on macOS

## Symptom

SessionStart prints:

```
⚠  Embedding engine is NOT functional — semantic way matching is OFF.
```

and `ways corpus` reports `EN embedding generation failed (signal: 9 (SIGKILL))`. Ways still fire on `pattern:`, `commands:` and `files:` triggers, but semantic matching is off, so ways over- and under-fire.

Any invocation of the embedder, even `--version`, exits `137` with no output:

```console
$ ~/.cache/agent-ways/user/way-embed --version
$ echo $?
137            # 128 + 9 = killed by SIGKILL
```

`ways status` names the `way-embed` binary in use. The examples below use the default engine directory, `$XDG_CACHE_HOME/agent-ways/user` (usually `~/.cache/agent-ways/user`).

## Confirm the cause

The crash report gives the reason:

```console
$ ls -t ~/Library/Logs/DiagnosticReports/way-embed-*.ips | head -1 | xargs grep -o '"signal":"[^"]*"'
"signal":"SIGKILL (Code Signature Invalid)"
```

The binary is not broken and the process was not killed for memory. macOS kills it at `exec`, before dyld loads any library, so linked libraries are a red herring. The binary is validly ad-hoc signed and `codesign --verify` passes. The mismatch is in the kernel: the Code Signing Monitor holds a cached cdhash for the file's inode, and after an update copies a new `way-embed` over the old path, that cache no longer matches the file.

To confirm, copy the binary to a fresh inode; the copy launches:

```console
$ cp ~/.cache/agent-ways/user/way-embed /tmp/we && /tmp/we --version
way-embed 0.1.0
```

## Fix

Re-sign the binary in place. That rewrites the file and invalidates the stale cache. No rebuild is needed.

```console
$ "${XDG_DATA_HOME:-$HOME/.local/share}/agent-ways/scripts/fix-way-embed-signature.sh"
```

The script re-signs the engine copy and every `way-embed` under the app's `bin/`, checks that the engine copy launches, and rebuilds the corpus. Start a new Claude Code session afterwards.

By hand:

```console
$ codesign --force --sign - ~/.cache/agent-ways/user/way-embed
$ ways corpus
```

## A different signal: SIGILL on Linux

A `way-embed` that dies with `SIGILL` on an older x86-64 CPU is a different problem: a prebuilt compiled for instructions the CPU lacks. Linux prebuilts are now built without native CPU tuning, so `ways update` fixes it. Until the engine runs, the corpus build keeps the previous corpus and `ways status` names the failure.
