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

Two facts about the entry point shape the answer:

- **The published installer is `curl -sL … | bash`, where bash's stdin *is* the
  piped script.** A naive `read` consumes script bytes or hits EOF, not a keypress.
  Interactive prompting through a pipe is fragile (`/dev/tty` may or may not exist,
  behaves differently across SSH/containers), so an in-band "press a key to build"
  is the wrong primitive for the common path.
- **The bootstrap already clones the full source** (with submodules) to
  `$XDG_DATA_HOME/agent-ways` to enable in-place updates. So the machinery to build
  from source — Makefile, `tools/way-embed`, llama.cpp submodule — is *already on
  disk* after acquisition. Recovery needs no separate `git clone`; it is a `cd` into
  the staged app dir plus `make`.

Together these point away from prompting and toward **halting with precise, local
instructions**: when a downloaded binary won't run, stop and tell the user exactly
how to finish against the source we already staged.

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

**3. On a verification failure, halt fast and hand off — do not prompt-and-build
in-band.** Stop at the *first* binary that won't launch and print a
dependency-aware **recovery card** rather than attempting an interactive build
through a pipe. Guiding a novice through a raw compile is not, by itself, good UX,
so the card leads with the higher-leverage route and keeps the manual one as a
backstop:

- **Ask the agent (preferred).** Claude Code is, by definition, installed — this is
  being set up *for* it. The card tells the user to open Claude Code in the staged
  app dir (`$XDG_DATA_HOME/agent-ways`) and ask it to finish setup. A shipped
  agent-context file (see 4) primes the agent with the exact, safe steps. This
  turns "go compile a C++ project" into "ask the assistant that's already here."
- **Do it yourself.** The precise commands against the *already-staged* source,
  shaped by the dep check: deps present → `cd "$XDG_DATA_HOME/agent-ways" && make
  setup`; deps missing but a recognized package manager (pacman/apt/dnf/brew) is
  present → `… && make deps && make setup`; no recognized manager → install cmake +
  a C++ compiler, then `make setup`. No `git clone` step — the source is already on
  disk from the bootstrap.

Then exit with a status reflecting reality: the install is functional for
keyword/pattern matching but degraded (semantic matching is a hard dependency,
ADR-125), so exit non-fatal-warning, never a hang.

**4. Ship the agent build-context as a *discoverable* doc — never a way or skill.**
A small install-completion context file (e.g. an `AGENTS.md` / finish-setup doc at
`$XDG_DATA_HOME/agent-ways`) gives Claude Code what it needs to complete the build
*safely*: what `way-embed` is, how to check deps, that `make deps` may need sudo
(ask the human first), `make setup`, and how to verify success (`ways status` shows
the engine up). It is a **plain document, found on demand** — the recovery card
names its path, and the user opens Claude Code in the app dir and asks it to finish.

It is explicitly **not** a way (`hooks/ways/…`) or a skill (`skills/…`): those load
into session context — a way via matching, a skill at startup — for *every* user in
*every* project, forever, to serve a recovery path almost no one hits. That is
exactly the always-on context tax the framework's progressive-disclosure model
exists to avoid. Install-completion is disclosed at the moment of need (the failed
install, the pointer in the card), not injected proactively.

**5. `make deps` is the toolchain installer; the installer only checks.** A new
`make deps` target detects the OS/package manager and installs the build
prerequisites (cmake + C++ compiler), using sudo where the platform requires it.
It is the single command both the recovery card and the agent point at, and the
only place system packages are installed. Keeping "install deps" out of the
installer's own path preserves the property that piping a script to bash never
mutates system state without an explicit, separately-invoked, user-run command.

**6. An inline interactive build is an optional convenience, never the contract.**
When `install.sh` is run *directly* (not piped) and a genuine interactive terminal
is present, it MAY additionally offer "build now? [y/N]" reading from the tty. This
is a nicety for the power user who cloned and ran the script by hand; it never
replaces the recovery card, which is what the piped `curl | bash` path relies on.

The ordering also decouples the model download from the binary build: the model is
a hard dependency independent of *how* the binary was obtained, so its acquisition
must not be gated behind a binary build that may be skipped or deferred.

## Consequences

### Positive

- A broken or absent binary is caught at install time with a precise, per-binary
  signal instead of surfacing later as mysteriously bad matching.
- Regular users are never dropped into a raw compiler error: they get a working
  prebuilt binary, or a recovery card that hands off to the Claude Code they're
  already installing this for (primed by a shipped context doc) — with exact
  one-line commands against the already-staged source as the backstop.
- The recovery leverages what's uniquely true here — an AI agent is guaranteed
  present — without taxing every session: the build context is a discoverable doc,
  not an always-loaded way or skill.
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
- **Prompt-and-build in-band (via `/dev/tty`).** Considered as the primary fallback
  and rejected: `/dev/tty` prompting through `curl | bash` is fragile across SSH,
  containers, and CI, and building a C++ project mid-install is poor UX for a
  novice. Since the full source is already staged, halting with a recovery card
  (agent handoff + exact commands) is more robust; an inline tty prompt survives
  only as an optional convenience for the directly-run script (Decision 6).
- **Carry the install-completion context as a way or skill.** Rejected: a way
  matches into context and a skill loads at startup — both would inject this into
  *every* session for *every* user to serve a rare recovery path, the exact always-on
  context tax progressive disclosure exists to prevent. It must be a discoverable
  doc, surfaced only at the point of need.
- **Verify by file existence / checksum only.** Rejected as insufficient: a
  checksum proves the bytes match the release, not that the binary *runs* on this
  host (glibc/arch mismatches pass a checksum and still fail to launch). A launch
  probe is the actual property we need.
