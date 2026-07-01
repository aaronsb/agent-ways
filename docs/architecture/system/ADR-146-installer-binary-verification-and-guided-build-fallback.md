---
status: Draft
date: 2026-06-30
deciders:
  - aaronsb
  - claude
related:
  - "[[ADR-142]]"
  - "[[ADR-144]]"
  - "[[ADR-125]]"
---

# ADR-146: installer binary verification and guided build fallback

## Context

The install (ADR-142/144) acquires platform binaries — `ways`, `attend`, and the
`way-embed` semantic matcher — by downloading a per-platform prebuilt release and,
failing that, building from source. Two gaps make a fresh install silently
under-deliver:

1. **Download success is not launch success.** The installer treats "the file
   arrived" as done. A binary can download and still fail to execute — wrong arch
   slipped through detection, a glibc/libstdc++ too old for the release, a
   truncated or corrupt asset. The user gets a broken engine with no signal.

2. **Source build is an unguided dead end.** When no prebuilt binary is available,
   the fallback compiles locally. `way-embed` needs a C++ toolchain (cmake +
   compiler, via llama.cpp). On a machine without it the build aborts with a raw
   error mid-install, and the model download — sequenced after the binary — never
   runs. Semantic matching (a hard dependency, ADR-125) is off, and 100+ ways
   silently degrade to keyword-only matching.

Local compilation is a **last resort** for regular users, not a first-class path.
When we can't hand them a working binary, the installer should set them up for
success — verify what it downloaded, and if it must fall back to building, do so
transparently and only with consent, never silently and never by installing system
packages behind their back.

The load-bearing constraint is the entry point: the published installer is
`curl -sL … | bash`, where **bash's stdin *is* the piped script**. A naive `read`
consumes script bytes or hits EOF, not a keypress — so any interactive prompt must
read from `/dev/tty`, and when there is no controlling terminal (CI, cron, a
container) the installer must not block.

## Decision

Add a **verify-then-guide** stage to the installer's acquisition path. Prebuilt
download stays primary; local compilation stays the last resort; between them sits
verification and a consent-gated, dependency-aware fallback.

**1. Verify every acquired binary launches.** After download, run each binary's
cheap liveness probe (`--version`). A binary that does not exit success is treated
as *not acquired* — the same as a missing download — and routed to the fallback.
Report per-binary which verified and which did not, rather than a single opaque
"installed."

**2. On a failed/absent binary, check — never install — build dependencies.**
Detect the toolchain the source build needs (cmake + a C++ compiler for
`way-embed`) with `command -v`. Checking is always safe and non-interactive;
installing system packages is neither and is never done implicitly.

**3. Branch on the dependency check:**

- **Deps satisfied** → the machine *can* build. Offer an explicit consent prompt —
  "press a key to build `<binary>` from source (~N min), or Ctrl-C to skip" — read
  from `/dev/tty`. On consent, build; on Ctrl-C or no consent, skip and report the
  degraded state plus the one command to finish later.
- **Deps missing but plausibly installable** (a package manager we recognize —
  pacman/apt/dnf/brew — is present) → do not build and do not install. Tell the
  user to run **`make deps`** (which *will* install the toolchain, with sudo as
  needed) and then re-run the installer. Print the exact command.
- **Deps missing and not resolvable** (no recognized package manager) → report the
  degraded state and the manual toolchain requirement; exit without blocking.

**4. Never block without a tty.** Every prompt reads from `/dev/tty`. When
`/dev/tty` is unavailable or not interactive, the installer skips the prompt,
prints the exact remediation commands (`make deps`, or the build command), and
exits with a status that reflects reality — success for a working install, a clear
non-fatal warning when semantic matching is down. Unattended installs never hang
and never surprise-compile.

**5. `make deps` is the toolchain installer; the installer only checks.** A new
`make deps` target detects the OS/package manager and installs the build
prerequisites (cmake + C++ compiler), using sudo where the platform requires it.
It is the single command the installer points users at, and the only place system
packages are installed. Keeping "install deps" out of the installer's own path
preserves the property that piping a script to bash never mutates system state
without an explicit, separately-invoked, user-run command.

The ordering also decouples the model download from the binary build: the model is
a hard dependency independent of *how* the binary was obtained, so its acquisition
must not be gated behind a binary build that may be skipped or deferred.

## Consequences

### Positive

- A broken or absent binary is caught at install time with a precise, per-binary
  signal instead of surfacing later as mysteriously bad matching.
- Regular users are never dropped into a raw compiler error: they get either a
  working prebuilt binary, a consented build, or one clear command (`make deps`)
  and a retry.
- The installer never installs system packages implicitly and never compiles
  without consent — `curl | bash` stays as low-surprise as its reputation demands.
- Unattended contexts (CI, cron, containers) get a deterministic, non-blocking
  outcome with actionable output.

### Negative

- More installer branching and platform-specific dependency detection to maintain
  (package-manager matrix in `make deps`).
- The interactive build path depends on `/dev/tty` semantics, which vary across
  terminals, SSH sessions, and container runtimes — a class of environment bugs to
  test for.
- A two-step "run `make deps`, then re-run the installer" flow is more friction
  than a one-shot install for the toolchain-less user — accepted deliberately as
  the price of never installing system packages behind their back.

### Neutral

- Prebuilt releases must exist for every supported platform for the happy path to
  stay toolchain-free (see the way-embed release job; this ADR assumes it).
- `make deps` becomes a documented, supported target users may run independently of
  installing agent-ways.

## Alternatives Considered

- **Auto-install deps and auto-build silently.** Rejected: a piped-to-bash
  installer that runs sudo package installs and compiles without consent violates
  the least-surprise contract and is a security smell.
- **Never build locally; require a prebuilt binary or fail.** Rejected as the sole
  policy: prebuilt coverage can lag (new platform, a release not yet cut), and a
  capable machine with the toolchain present should be allowed to build with
  consent rather than be told "unsupported."
- **Prompt with a plain `read` (stdin).** Rejected: broken under `curl | bash`
  where stdin is the script; `/dev/tty` with a non-interactive fallback is the only
  correct form.
- **Verify by file existence / checksum only.** Rejected as insufficient: a
  checksum proves the bytes match the release, not that the binary *runs* on this
  host (glibc/arch mismatches pass a checksum and still fail to launch). A launch
  probe is the actual property we need.
