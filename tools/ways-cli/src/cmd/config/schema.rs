//! Curated settings.json schema for the linter (ADR-147).
//!
//! This is a **deliberately partial** table of Claude Code `settings.json` keys.
//! Its job is to power three checks — schema-valid, scope-legal, duplicate-scalar
//! — not to be an exhaustive schema. Claude Code version-gates ~90 keys and adds
//! more over time, so an **unknown key is not an error**: [`lookup`] returns
//! `None` and the linter emits a *warning*, never a hard failure. Rejecting
//! valid-but-new config is the worse failure mode.
//!
//! Keys are matched at the **top level** of a fragment's `settings:` object.
//! Every high-value managed-only key is top-level; the two nested `sandbox.*`
//! managed sub-locks are intentionally out of scope for v1 (see the `sandbox`
//! entry). Object-valued keys (`env`, `permissions`, `hooks`, `statusLine`,
//! `sandbox`) are opaque — their contents are not schema-checked here.
//!
//! Source of truth: <https://code.claude.com/docs/en/settings>,
//! <https://code.claude.com/docs/en/permissions#managed-only-settings>, and
//! <https://code.claude.com/docs/en/server-managed-settings>. project-pulse
//! tracks drift in those docs so this table gets a refill signal.

/// How a key relates to scope — the axis the scope-legal check turns on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeClass {
    /// Settable at any scope.
    Normal,
    /// Only valid at managed scope. Authored at user/project scope it has no
    /// effect — Claude Code ignores it. Scope-legal treats this as an ERROR.
    ManagedOnly,
    /// Settable at user scope, but a managed endpoint's value hard-*replaces*
    /// it (dead on arrival there). Scope-legal treats a user/project author of
    /// one of these as a WARNING. (ADR-147 "Managed-scope interop".)
    ManagedOverridable,
}

/// The JSON type a key's value is expected to take. `Any` is used where the
/// documented type is uncertain, so the schema-valid check never false-positives
/// on a type the table isn't sure about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueType {
    Bool,
    Number,
    String,
    Array,
    Object,
    Any,
}

impl ValueType {
    /// Whether `v` satisfies this expected type.
    pub fn matches(&self, v: &serde_json::Value) -> bool {
        match self {
            ValueType::Any => true,
            ValueType::Bool => v.is_boolean(),
            ValueType::Number => v.is_number(),
            ValueType::String => v.is_string(),
            ValueType::Array => v.is_array(),
            ValueType::Object => v.is_object(),
        }
    }

    /// Human name, for diagnostics.
    pub fn name(&self) -> &'static str {
        match self {
            ValueType::Any => "any",
            ValueType::Bool => "boolean",
            ValueType::Number => "number",
            ValueType::String => "string",
            ValueType::Array => "array",
            ValueType::Object => "object",
        }
    }
}

/// The schema entry for one key.
#[derive(Debug, Clone, Copy)]
pub struct KeySpec {
    pub class: ScopeClass,
    pub ty: ValueType,
}

/// Look up a top-level `settings.json` key. `None` means "not in the curated
/// table" — an unrecognized (possibly newer or version-gated) key, which the
/// linter surfaces as a warning rather than an error.
pub fn lookup(key: &str) -> Option<KeySpec> {
    use ScopeClass::*;
    use ValueType::*;
    let (class, ty) = match key {
        // ── Managed-overridable (dead on arrival if an org sets them) ──
        "model" | "fallbackModel" => (ManagedOverridable, String),
        "availableModels" => (ManagedOverridable, Array),

        // ── Managed-only: policy locks & enforcement (permissions.md) ──
        "allowManagedHooksOnly"
        | "allowManagedPermissionRulesOnly"
        | "allowManagedMcpServersOnly"
        | "strictPluginOnlyCustomization"
        | "strictKnownMarketplaces"
        | "channelsEnabled"
        | "allowAllClaudeAiMcps"
        | "wslInheritsWindowsSettings"
        | "forceRemoteSettingsRefresh" => (ManagedOnly, Bool),
        // Uncertain value shapes — scope-class is what matters, type stays Any.
        "disableSideloadFlags" | "blockedMarketplaces" | "allowedChannelPlugins"
        | "allowedMcpServers" | "deniedMcpServers" => (ManagedOnly, Any),
        "pluginTrustMessage" | "forceLoginOrgUUID" | "requiredMinimumVersion"
        | "requiredMaximumVersion" => (ManagedOnly, String),

        // ── Normal: common user/project-settable keys ──
        "apiKeyHelper" | "awsAuthRefresh" | "awsCredentialExport"
        | "otelHeadersHelper" | "outputStyle" | "forceLoginMethod" => (Normal, String),
        "cleanupPeriodDays" => (Normal, Number),
        "includeCoAuthoredBy" | "enableAllProjectMcpServers" | "autoUpdates" => (Normal, Bool),
        "enabledMcpjsonServers" | "disabledMcpjsonServers" => (Normal, Array),
        // Opaque objects — contents intentionally not schema-checked in v1.
        // `sandbox` includes managed sub-locks (sandbox.*.allowManaged*Only) that
        // a future slice may scope-check; for now the whole object is Normal.
        "env" | "permissions" | "hooks" | "statusLine" | "sandbox" => (Normal, Object),

        _ => return None,
    };
    Some(KeySpec { class, ty })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn managed_only_keys_classify() {
        assert_eq!(lookup("allowManagedHooksOnly").unwrap().class, ScopeClass::ManagedOnly);
        assert_eq!(lookup("strictPluginOnlyCustomization").unwrap().class, ScopeClass::ManagedOnly);
        assert_eq!(lookup("allowedMcpServers").unwrap().class, ScopeClass::ManagedOnly);
        assert_eq!(lookup("forceLoginOrgUUID").unwrap().class, ScopeClass::ManagedOnly);
    }

    #[test]
    fn managed_overridable_keys_classify() {
        assert_eq!(lookup("model").unwrap().class, ScopeClass::ManagedOverridable);
        assert_eq!(lookup("fallbackModel").unwrap().class, ScopeClass::ManagedOverridable);
        assert_eq!(lookup("availableModels").unwrap().class, ScopeClass::ManagedOverridable);
    }

    #[test]
    fn normal_keys_classify_with_types() {
        let p = lookup("permissions").unwrap();
        assert_eq!(p.class, ScopeClass::Normal);
        assert_eq!(p.ty, ValueType::Object);
        assert_eq!(lookup("cleanupPeriodDays").unwrap().ty, ValueType::Number);
        assert_eq!(lookup("includeCoAuthoredBy").unwrap().ty, ValueType::Bool);
    }

    #[test]
    fn unknown_key_is_none() {
        assert!(lookup("totallyMadeUpKey").is_none());
        assert!(lookup("").is_none());
    }

    #[test]
    fn value_type_matches() {
        assert!(ValueType::Bool.matches(&json!(true)));
        assert!(!ValueType::Bool.matches(&json!("yes")));
        assert!(ValueType::String.matches(&json!("opus")));
        assert!(ValueType::Number.matches(&json!(30)));
        assert!(ValueType::Object.matches(&json!({"a": 1})));
        assert!(ValueType::Array.matches(&json!([1, 2])));
        assert!(ValueType::Any.matches(&json!(null)));
        assert!(ValueType::Any.matches(&json!("whatever")));
    }
}
