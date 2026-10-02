//! Locator for Claude Code's data on disk. It will own the config directory, project and
//! session listing, the single project-path encoder and its inverse, and transcript lookup
//! by session id, replacing the copies spread across the workspace (ADR-504, ADR-505).
