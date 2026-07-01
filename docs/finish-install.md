# Finishing an agent-ways install (semantic matching)

You're reading this because the installer got everything in place **except** the
semantic-matching engine — `way-embed` (the embedding binary) or its model isn't
working on this machine, so ways currently fall back to keyword/pattern matching
only. This is the guide to finish the job. It's written so **Claude Code can follow
it** — if a human pasted the installer's recovery prompt, that's you; do the steps
below and report back.

> This is a **discoverable** doc, not a way or a skill — it does nothing until
> someone opens it. It won't load into any session on its own.

## The situation

- The app source (with the Makefile and the `way-embed` source) is at
  **`$XDG_DATA_HOME/agent-ways`** (default `~/.local/share/agent-ways`).
- `way-embed` builds from a bundled `llama.cpp`, which needs a **C++ toolchain**
  (cmake + a C++ compiler). The likely reason semantic matching is off: no prebuilt
  binary exists for this platform *and* the toolchain isn't installed, so the
  source build couldn't run.

## Steps

Resolve the app dir once:

```bash
APP="${XDG_DATA_HOME:-$HOME/.local/share}/agent-ways"
```

**1. Check whether the toolchain is present.**

```bash
command -v cmake && command -v c++ 2>/dev/null || command -v g++ || command -v clang++
```

**2a. If cmake / a compiler is MISSING — install the toolchain.**

`make deps` auto-detects the package manager (pacman/apt/dnf/zypper/brew) and
installs cmake + a compiler + git. **It uses `sudo`.** If you're an agent: *ask the
human before running it* — installing system packages is their call.

```bash
cd "$APP" && make deps          # asks for sudo; confirm with the human first
```

**2b. If the toolchain is already present**, skip straight to build.

**3. Build + regenerate the corpus.**

```bash
cd "$APP" && make setup
```

This downloads/builds `way-embed`, fetches the model (~21MB), and regenerates the
corpus with embeddings.

**4. Verify.**

```bash
"$APP/bin/ways" status
```

Success looks like `Engine: embedding`, a `Model: … (OK)` line, and a non-zero
`Corpus:` count. If it still says `Engine: none`, read the `make setup` output for
the first error (usually a missing build tool) and resolve that.

**5. Restart Claude Code** so the freshly-built engine is picked up.

## If a prebuilt binary *should* have worked

If your platform is one of linux-x86_64 / linux-aarch64 / darwin-x86_64 /
darwin-arm64 and the download still failed, the binary may have downloaded but not
launched (a libc/toolchain mismatch). `make setup` falls back to a source build in
that case, so steps 2–4 still apply. You can also re-run the installer one-liner —
it's idempotent.
