//! The scope-class overlay for settings keys (ADR-147).
//!
//! The vendored JSON Schema ([`super::schema_doc`]) owns the *shape* — the valid
//! key set, types, and descriptions. What a generic JSON Schema cannot express is
//! **scope-class**: which keys only work at managed scope, and which are settable
//! at user scope but hard-replaced by a managed endpoint. That semantic — drawn
//! from ADR-147's "Managed-scope interop" research — is this small hand-curated
//! overlay.
//!
//! It lists *only* the keys with a non-normal scope relationship. A key absent
//! here has no scope restriction (the common case), so [`scope_class`] returns
//! `None`. The overlay also carries a few keys the community schema still lags
//! (e.g. `fallbackModel`, `disableSideloadFlags`), so it doubles as a
//! known-key supplement for the linter.
//!
//! Source: <https://code.claude.com/docs/en/permissions#managed-only-settings>
//! and <https://code.claude.com/docs/en/server-managed-settings>. project-pulse
//! tracks drift.

/// How a key relates to scope. Absence from the overlay means "normal" — no
/// scope restriction — so there is no `Normal` variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeClass {
    /// Only valid at managed scope. Authored at user/project scope it has no
    /// effect — Claude Code ignores it. Scope-legal treats this as an ERROR.
    ManagedOnly,
    /// Settable at user scope, but a managed endpoint's value hard-*replaces*
    /// it (dead on arrival there). Scope-legal treats a user/project author of
    /// one of these as a WARNING. (ADR-147 "Managed-scope interop".)
    ManagedOverridable,
}

/// Valid Claude Code settings keys that the vendored schema *lags* (SchemaStore
/// hasn't added them yet) and that carry no scope restriction. Without this, the
/// linter would false-warn "unrecognized" on current, valid settings. Managed-
/// scoped laggards need no entry here — [`scope_class`] already covers them.
const KNOWN_LAGGED: &[&str] = &[
    // Present in the CLI as a boolean; SchemaStore exposes `autoUpdatesChannel`
    // but not the older `autoUpdates` toggle.
    "autoUpdates",
];

/// Whether the overlay recognizes `key` — either it has a scope class or it is a
/// known key the vendored schema lags. Lets the linter avoid false "unrecognized"
/// warnings on keys the schema doesn't (yet) list.
pub fn overlay_knows(key: &str) -> bool {
    scope_class(key).is_some() || KNOWN_LAGGED.contains(&key)
}

/// The scope-class of a key, or `None` if it has no scope restriction.
pub fn scope_class(key: &str) -> Option<ScopeClass> {
    use ScopeClass::*;
    let class = match key {
        // Managed-overridable (dead on arrival if an org sets them).
        "model" | "fallbackModel" | "availableModels" => ManagedOverridable,

        // Managed-only: policy locks & enforcement.
        "allowManagedHooksOnly"
        | "allowManagedPermissionRulesOnly"
        | "allowManagedMcpServersOnly"
        | "strictPluginOnlyCustomization"
        | "strictKnownMarketplaces"
        | "channelsEnabled"
        | "allowAllClaudeAiMcps"
        | "wslInheritsWindowsSettings"
        | "forceRemoteSettingsRefresh"
        | "disableSideloadFlags"
        | "blockedMarketplaces"
        | "allowedChannelPlugins"
        | "pluginTrustMessage"
        | "forceLoginOrgUUID"
        | "requiredMinimumVersion"
        | "requiredMaximumVersion" => ManagedOnly,

        // Everything else — including the user-settable MCP allow/deny lists,
        // which ADR-147 interop names as concatenating keys valid at user scope
        // — has no scope restriction.
        _ => return None,
    };
    Some(class)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn managed_only_keys_classify() {
        assert_eq!(scope_class("allowManagedHooksOnly"), Some(ScopeClass::ManagedOnly));
        assert_eq!(scope_class("strictPluginOnlyCustomization"), Some(ScopeClass::ManagedOnly));
        assert_eq!(scope_class("disableSideloadFlags"), Some(ScopeClass::ManagedOnly));
        assert_eq!(scope_class("forceLoginOrgUUID"), Some(ScopeClass::ManagedOnly));
    }

    #[test]
    fn managed_overridable_keys_classify() {
        assert_eq!(scope_class("model"), Some(ScopeClass::ManagedOverridable));
        assert_eq!(scope_class("fallbackModel"), Some(ScopeClass::ManagedOverridable));
        assert_eq!(scope_class("availableModels"), Some(ScopeClass::ManagedOverridable));
    }

    #[test]
    fn mcp_allow_deny_lists_have_no_scope_restriction() {
        // ADR-147 interop: deniedMcpServers concatenates from user scope, so it
        // must not be flagged — no scope class.
        assert_eq!(scope_class("deniedMcpServers"), None);
        assert_eq!(scope_class("allowedMcpServers"), None);
    }

    #[test]
    fn normal_and_unknown_keys_have_no_scope_class() {
        assert_eq!(scope_class("permissions"), None);
        assert_eq!(scope_class("cleanupPeriodDays"), None);
        assert_eq!(scope_class("totallyMadeUpKey"), None);
        assert_eq!(scope_class(""), None);
    }
}
