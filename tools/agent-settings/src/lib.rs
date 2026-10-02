//! Settings registry machinery shared by ways and attend (ADR-503). It owns the
//! schema types, the registry, layered loading with per-section fallback, lint,
//! fragment emit and the file writer. Each component keeps its own schema in
//! its own crate; this crate depends on none of them.

pub mod load;
pub mod registry;
pub mod schema;
pub mod writer;
pub mod yaml_edit;

pub use load::{Finding, Layer, Resolved};
pub use registry::{Bound, Registry};
pub use schema::{DefaultValue, FileSpec, Kind, KeySpec, LayerScope, Schema, Scope, SectionSpec};

/// Exit codes of `ways settings` property and object mode (ADR-503 §9).
pub mod exit {
    /// Done.
    pub const OK: i32 = 0;
    /// Usage error or unknown key.
    pub const USAGE: i32 = 2;
    /// Value rejected by the schema; lint found something.
    pub const REJECTED: i32 = 3;
    /// Written, but a higher layer overrides it.
    pub const OVERRIDDEN: i32 = 4;
    /// Write failed: lock, permission, disk.
    pub const WRITE_FAILED: i32 = 5;
}
