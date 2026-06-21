---
description: adversarial security review of pull requests and patches from external or unknown contributors, hunting subtle malicious changes
vocabulary: contributor external untrusted pr contribution review malicious backdoor exfiltration trojan homoglyph bidi diff scrutiny insider driveby fork patch
pattern: external.?(pr|contribut|patch)|untrusted.?(pr|contribut)|from.?(people|strangers|outside)|unknown.?contribut|drive.?by|third.?part(y|ies).?(pr|patch|contribut)
scope: agent, subagent
refire: 0.2
---
<!-- epistemic: constraint -->
# Untrusted Contributions Way

A pull request or patch from someone who is not a known, trusted maintainer is **untrusted input**, not just code to review for correctness. A `code-reviewer` pass checks "does this work?" — this way checks "is this trying to do something it isn't telling me about?" Run it *in addition to* the normal review, never instead.

Trust is per-contributor and earns in: a first-time or unknown author gets the full pass; a long-trusted maintainer gets judgment. When unsure, treat as untrusted.

## The Core Question

**Does the diff only do what the PR says — and nothing more?** Read every changed line with that lens. A clean correctness review and green CI prove *intended* behavior works; they say nothing about *hidden* behavior.

## Adversarial Checklist

| Check | What you're hunting | How |
|-------|--------------------|-----|
| **Scope of touch** | Edits to files unrelated to the stated purpose | `git diff --stat`; flag any CI/workflow, `package.json`/lockfile, build script, install hook, or security-sensitive file (auth, crypto, path/input validation, network binding) the PR description doesn't justify |
| **Privilege direction** | Changes that *expand* access/exposure vs. *restrict* it | A diff that only adds filters/guards is low-risk; one that loosens a check, widens a bind, or returns *more* data deserves real scrutiny |
| **Exfiltration primitives** | New ways for data/control to leave | grep added lines for `fetch`/`http`/`net`/`fs` writes/`child_process`/`exec`/`eval`/`new Function`/dynamic `require`/`import()` |
| **Hidden characters** | Trojan Source: bidi overrides, zero-width, homoglyphs | scan added lines for `[\x{200B}-\x{200F}\x{202A}-\x{202E}\x{2060}-\x{2064}\x{FEFF}\x{2066}-\x{2069}]` and non-ASCII in identifiers; "looks like X, runs as Y" |
| **Obfuscation** | Code hiding its real effect | base64/hex blobs, string-built identifiers, deeply indirected calls, minified chunks in a source PR |
| **Weakened guards/tests** | Sabotage disguised as a stub update | did any *existing* assertion get removed or loosened? do new mocks mask the very behavior under test? |
| **Dependencies** | Malicious or typosquatted packages, install scripts | new entries in the manifest/lockfile; postinstall scripts; apply the project's version-age / supply-chain hold |

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "It's a tiny diff" | Small is where subtle hides — one flipped comparison, one widened glob. Read all of it. |
| "Tests pass" | Tests prove the happy path runs, not that nothing extra runs. An attacker writes passing tests too. |
| "The code-reviewer already approved it" | That pass optimizes for correctness/design, not intent. This is a separate lens. |
| "They seem helpful / the PR is well-written" | Social proof is the cheapest part to fake. Polished prose is not evidence of benign code. |
| "It only touches test files" | Test/CI files run in your pipeline with your secrets. They're a prime injection point, not a safe zone. |

## Reporting

State the verdict as its own line — "no malicious patterns found" or "flagging X for explanation" — kept distinct from the correctness review, so the human sees the security judgment explicitly rather than buried in style nits. If something looks off, ask the contributor to explain it before merge; don't silently fix and absorb it.

## See Also

- code/supplychain/sourceaudit(softwaredev) — auditing third-party/dependency source
- delivery/github(softwaredev) — the PR review and merge workflow this hardens
- code/security(softwaredev) — secure-coding review defaults
