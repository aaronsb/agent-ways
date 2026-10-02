//! Settings registry machinery shared by ways and attend. It will own schema types,
//! the registry, layered loading with per-section fallback, lint, fragment emit and the
//! file writer, while each component keeps its own schema in its own crate (ADR-503).
