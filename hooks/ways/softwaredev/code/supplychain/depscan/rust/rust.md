---
description: Rust dependency security, cargo audit, unsafe blocks, build script risks
vocabulary: cargo audit Cargo.lock Cargo.toml unsafe build.rs crate crates.io rustsec advisory
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: convention -->
# Rust Dependency Security

## Scanning

```bash
cargo audit
osv-scanner --lockfile=Cargo.lock
```

## Rust-Specific Risks

- **`build.rs` runs at compile time.** It's a build script with full system access — read it for unfamiliar crates.
- **`unsafe` blocks bypass the borrow checker.** Not inherently malicious but worth reviewing in unfamiliar code: `grep -rn 'unsafe' src/`
- **Proc macros are compile-time code execution.** Crates with `proc-macro = true` in `Cargo.toml` run arbitrary code during compilation.
- **Rust's supply chain is generally healthier than npm/PyPI** — smaller ecosystem, stronger cultural norms around safety — but not immune. Typosquatting and maintainer compromise still happen.
