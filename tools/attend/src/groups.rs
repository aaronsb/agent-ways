//! Named group management for attend (ADR-118).
//!
//! Groups are named signal namespaces, shown to the operator as channels.
//! Storage: `@group-name/` directories under the signals base, with membership
//! tracked in `_groups.yaml`.
//!
//! Every agent is always in its implicit project group (from cwd).
//! Named groups are explicit and opt-in via `attend join <name>`.
//!
//! The state manager and `_groups.yaml` I/O live in the shared
//! `attend-groups` crate (ADR-170) — attend-chat's `/join` write path
//! uses the same implementation, so the wire format has a single owner.

pub use attend_groups::Groups;
