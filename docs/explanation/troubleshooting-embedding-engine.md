# Troubleshooting: embedding engine "NOT functional" (way-embed SIGKILL)

## Symptom

SessionStart prints:

```
⚠  Embedding engine is NOT functional — semantic way matching is OFF.
WARNING: EN embedding generation failed (signal: 9 (SIGKILL))
```

Ways still fire on explicit `pattern:` / `commands:` / `files:` triggers, but
meaning-based (semantic) matching is off, so ways over- and under-fire.

Running the embedder directly confirms it: **any** invocation of `way-embed` —
even `--version` — exits `137` with no output.

```console
$ bin/way-embed --version
$ echo $?
137            # 128 + 9 = killed by SIGKILL
```

## Root cause

This is **not** a broken binary and **not** an out-of-memory kill. The
authoritative reason is in the crash report:

```console
$ ls -t ~/Library/Logs/DiagnosticReports/way-embed-*.ips | head -1 | xargs grep -o '"signal":"[^"]*"'
"signal":"SIGKILL (Code Signature Invalid)"
```

macOS kills the process at `exec`, *before* dyld loads a single library
(`usedImages` in the report lists only `dyld`), so linked libraries such as
Homebrew's OpenSSL are a red herring.

The binary is validly ad-hoc (linker-)signed — `codesign --verify` passes
"valid on disk". The mismatch is in the kernel: macOS 26's Code Signing Monitor
(`codeSigningMonitor: 2` in the report) holds a **cached cdhash for the file's
inode**, and after an in-place replace — a version upgrade that `cp`s a new
`way-embed` over the existing path — that cache no longer matches the file
content. Userspace `codesign` recomputes from disk and passes; the kernel
compares against the stale cache and kills the launch.

A quick way to confirm it is a cache issue and not the file: copy the binary to
a fresh inode and it launches fine.

```console
$ cp bin/way-embed /tmp/we && /tmp/we --version
way-embed 0.1.0
```

## Fix

Re-sign the binary in place. That rewrites the signature (and the file), which
invalidates the stale cache so the next `exec` re-evaluates cleanly. **No
rebuild is needed** — `make setup` would work but is unnecessary and slow, and
requires the C++ toolchain.

```console
$ scripts/fix-way-embed-signature.sh
```

The script re-signs every `way-embed` binary in `bin/`, verifies the invoked
one now launches, and regenerates the corpus so semantic matching is live. Then
start a new Claude Code session to pick up the repaired engine.

Manual equivalent:

```console
$ codesign --force --sign - bin/way-embed
$ codesign --force --sign - bin/way-embed-darwin-arm64
$ bin/ways corpus
```

## Why the build doesn't hit this

`make setup` builds `way-embed` and `cp`s it into place while it is *not* mapped
by a running process, so the kernel caches the fresh cdhash on first launch. The
stale-cache case only arises when a new binary replaces one whose old cdhash the
kernel already cached for that path — i.e. an update over a previously-run
install.
