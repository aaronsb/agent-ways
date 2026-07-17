//! `ways-core` — the shared engine for the ways toolchain.
//!
//! Extracted from the `ways` binary (ADR-151) so both the `ways` runtime and
//! the `ways-audit` compliance binary depend on one library rather than the
//! engine being trapped inside an executable. The library boundary is also what
//! makes this previously-untested engine unit-testable.
//!
//! Scope (ADR-151 §1): way discovery/scanning, frontmatter parsing, path and
//! projection resolution, configuration, the firing-event log reader, and the
//! compliance-claim sidecar model and manifest builder.
//!
//! Session lifecycle, command dispatch, and hooks stay in the `ways` binary;
//! the compliance operator surface lives in the `ways-audit` binary.

pub mod agents;
pub mod calibration;
pub mod config;
pub mod context_window;
pub mod finding;
pub mod firing;
pub mod frontmatter;
pub mod introspection;
pub mod paths;
pub mod provenance;
pub mod scanner;
pub mod transcript;
pub mod util;
