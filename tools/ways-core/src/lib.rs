//! `ways-core` — the shared engine for the ways toolchain.
//!
//! Extracted from the `ways` binary (ADR-151) so both the `ways` runtime and
//! the `ways-audit` compliance binary depend on one library rather than the
//! engine being trapped inside an executable. The library boundary is also what
//! makes this previously-untested engine unit-testable.
//!
//! Scope (ADR-151 §1): way discovery/scanning, frontmatter parsing, path and
//! projection resolution, and configuration. The compliance claim model and
//! firing-log reader migrate here alongside `ways-audit` (ADR-151 §2–3).
//!
//! Session lifecycle, command dispatch, and hooks stay in the `ways` binary.

pub mod agents;
pub mod config;
pub mod frontmatter;
pub mod paths;
pub mod scanner;
pub mod util;
