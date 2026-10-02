//! Permission matching engine for ADR-116.
//!
//! Parses permission strings (Tool, Tool(scope), *) and checks whether
//! a grant satisfies a requirement via containment hierarchy.
//!
//! Rules:
//! - `*` covers everything
//! - `Bash(*)` covers any `Bash(cmd:*)`
//! - `Read` (unscoped) covers `Read(/any/path)`
//! - `Bash(git:*)` covers `Bash(git:status)` but NOT `Bash(*)`

use std::fmt;
use std::path::Path;

/// A parsed permission.
#[derive(Debug, Clone, PartialEq)]
pub enum Permission {
    /// Wildcard — covers everything.
    Wildcard,
    /// Unscoped tool access (e.g., `Read`, `Edit`).
    Tool(String),
    /// Scoped tool access (e.g., `Bash(git:*)`, `Read(/home/**)`).
    Scoped(String, String),
}

impl Permission {
    /// Parse a permission string into a Permission.
    ///
    /// Examples: `"*"`, `"Read"`, `"Bash(git:*)"`, `"Write(/home/**)"`.
    pub fn parse(s: &str) -> Option<Permission> {
        let s = s.trim();
        if s == "*" {
            return Some(Permission::Wildcard);
        }
        if let Some(paren_start) = s.find('(') {
            if !s.ends_with(')') {
                return None;
            }
            let tool = s[..paren_start].to_string();
            let scope = s[paren_start + 1..s.len() - 1].to_string();
            if tool.is_empty() || scope.is_empty() {
                return None;
            }
            Some(Permission::Scoped(tool, scope))
        } else if !s.is_empty() && s.chars().next().unwrap().is_ascii_uppercase() {
            Some(Permission::Tool(s.to_string()))
        } else {
            None
        }
    }
}

impl fmt::Display for Permission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Permission::Wildcard => write!(f, "*"),
            Permission::Tool(t) => write!(f, "{t}"),
            Permission::Scoped(t, s) => write!(f, "{t}({s})"),
        }
    }
}

/// Check if a grant satisfies a requirement.
///
/// A grant satisfies a requirement if the grant is at least as broad:
/// - `*` satisfies any requirement
/// - `Bash(*)` satisfies `Bash(git:*)` but not vice versa
/// - `Read` (unscoped) satisfies `Read(/path)` but not vice versa
/// - `Bash(git:*)` satisfies `Bash(git:status)` (prefix match on command scope)
pub fn grant_satisfies(grant: &Permission, requirement: &Permission) -> bool {
    match (grant, requirement) {
        // Wildcard grant covers everything
        (Permission::Wildcard, _) => true,
        // Nothing else covers a wildcard requirement
        (_, Permission::Wildcard) => false,

        // Unscoped grant covers unscoped or any scoped requirement of the same tool
        (Permission::Tool(gt), Permission::Tool(rt)) => gt == rt,
        (Permission::Tool(gt), Permission::Scoped(rt, _)) => gt == rt,

        // Scoped grant does not cover unscoped requirement
        (Permission::Scoped(_, _), Permission::Tool(_)) => false,

        // Scoped grant covers scoped requirement if tools match and scope contains
        (Permission::Scoped(gt, gs), Permission::Scoped(rt, rs)) => {
            gt == rt && scope_contains(gs, rs)
        }
    }
}

/// Check if a grant scope contains a requirement scope.
///
/// For Bash commands: `*` contains anything, `git:*` contains `git:status`.
/// For paths: glob-style containment — `/home/**` contains `/home/user/file`.
fn scope_contains(grant_scope: &str, req_scope: &str) -> bool {
    // Wildcard scope covers everything
    if grant_scope == "*" {
        return true;
    }
    // Exact match
    if grant_scope == req_scope {
        return true;
    }
    // Command prefix: grant "git:*" matches requirement "git:status"
    if let Some(prefix) = grant_scope.strip_suffix(":*") {
        if let Some(req_prefix) = req_scope.strip_suffix(":*") {
            // git:* covers git:* (already caught by exact match above, but be safe)
            return req_prefix == prefix || req_prefix.starts_with(&format!("{prefix}:"));
        }
        // grant "git:*" covers requirement "git:status"
        return req_scope.starts_with(&format!("{prefix}:"));
    }
    // Path glob: grant "/home/**" matches requirement "/home/user/file"
    if grant_scope.contains("**") {
        let prefix = grant_scope.trim_end_matches("**");
        return req_scope.starts_with(prefix);
    }
    // Tilde expansion: "~/.claude/**" matches "/home/user/.claude/foo"
    if grant_scope.starts_with("~/") {
        let expanded = format!("{}{}", claude_sessions::home_dir().display(), &grant_scope[1..]);
        return scope_contains(&expanded, req_scope);
    }
    false
}

/// Load permissions from settings.json.
///
/// Reads `~/.claude/settings.json` and parses `permissions.allow` array.
pub fn load_settings_permissions(settings_path: &Path) -> Vec<Permission> {
    let content = match std::fs::read_to_string(settings_path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let doc: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    let allow = match doc.get("permissions").and_then(|p| p.get("allow")).and_then(|a| a.as_array()) {
        Some(arr) => arr,
        None => return Vec::new(),
    };

    allow
        .iter()
        .filter_map(|v| v.as_str())
        .filter_map(Permission::parse)
        .collect()
}

/// Audit result for a single requirement.
#[derive(Debug)]
pub struct AuditResult {
    /// Source of the requirement (way ID or sensor name).
    pub source: String,
    /// The permission string required.
    pub requirement: String,
    /// Whether it's satisfied by a grant in settings.json.
    pub granted: bool,
}

/// Audit a set of (source, requirements) against granted permissions.
pub fn audit(
    requirements: &[(String, Vec<String>)],
    grants: &[Permission],
) -> Vec<AuditResult> {
    let mut results = Vec::new();
    for (source, reqs) in requirements {
        for req_str in reqs {
            let granted = if let Some(req) = Permission::parse(req_str) {
                grants.iter().any(|g| grant_satisfies(g, &req))
            } else {
                false // malformed requirement
            };
            results.push(AuditResult {
                source: source.clone(),
                requirement: req_str.clone(),
                granted,
            });
        }
    }
    results
}

/// Display audit results as an agent-fmt Table.
///
/// `title` is the heading (e.g., "Permissions Audit" or "Attend Permissions Audit").
/// `source_header` is the first column label (e.g., "Way" or "Sensor").
/// `has_tpm` triggers a deprecation notice for trusted-project-macros.
pub fn display_audit(
    title: &str,
    source_header: &str,
    results: &[AuditResult],
    has_tpm: bool,
) {
    use crate::{Align, Table};
    use agent_theme::{paint, Role, Style};

    println!();
    println!("  {} (ADR-116)", paint(Style::new().bold(), title));
    println!();

    if results.is_empty() {
        println!("  No {source_header}s declare requires: fields.");
        println!();
        return;
    }

    let mut table = Table::new(&[source_header, "Requires", "Status"]);
    table.align(0, Align::Left);
    table.align(1, Align::Left);
    table.align(2, Align::Left);

    let mut missing_count = 0u32;
    let mut missing_perms: Vec<String> = Vec::new();

    for r in results {
        let status = if r.granted {
            paint(Role::Ok, "granted")
        } else {
            missing_count += 1;
            if !missing_perms.contains(&r.requirement) {
                missing_perms.push(r.requirement.clone());
            }
            paint(Role::Err, "MISSING")
        };
        table.add(vec![&r.source, &r.requirement, &status]);
    }

    table.print();
    println!();

    if missing_count > 0 {
        println!("  {} Add to settings.json:", paint(Role::Warn, format!("{missing_count} missing permission(s).")));
        for p in &missing_perms {
            println!("    \"{p}\"");
        }
    } else {
        println!("  {}", paint(Role::Ok, "All permissions granted."));
    }

    if has_tpm {
        println!();
        println!("  {} ~/.claude/trusted-project-macros found.", paint(Role::Warn, "Deprecation:"));
        println!("  This file is deprecated — use requires: fields in way frontmatter instead.");
        println!("  See ADR-116 for migration guidance.");
    }

    println!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_wildcard() {
        assert_eq!(Permission::parse("*"), Some(Permission::Wildcard));
    }

    #[test]
    fn test_parse_tool() {
        assert_eq!(Permission::parse("Read"), Some(Permission::Tool("Read".to_string())));
    }

    #[test]
    fn test_parse_scoped() {
        assert_eq!(
            Permission::parse("Bash(git:*)"),
            Some(Permission::Scoped("Bash".to_string(), "git:*".to_string()))
        );
    }

    #[test]
    fn test_wildcard_covers_all() {
        let grant = Permission::Wildcard;
        let req = Permission::parse("Bash(git:*)").unwrap();
        assert!(grant_satisfies(&grant, &req));
    }

    #[test]
    fn test_unscoped_covers_scoped() {
        let grant = Permission::Tool("Read".to_string());
        let req = Permission::Scoped("Read".to_string(), "/home/user".to_string());
        assert!(grant_satisfies(&grant, &req));
    }

    #[test]
    fn test_scoped_does_not_cover_unscoped() {
        let grant = Permission::Scoped("Read".to_string(), "/home/**".to_string());
        let req = Permission::Tool("Read".to_string());
        assert!(!grant_satisfies(&grant, &req));
    }

    #[test]
    fn test_bash_wildcard_scope() {
        let grant = Permission::Scoped("Bash".to_string(), "*".to_string());
        let req = Permission::Scoped("Bash".to_string(), "git:*".to_string());
        assert!(grant_satisfies(&grant, &req));
    }

    #[test]
    fn test_bash_prefix_scope() {
        let grant = Permission::Scoped("Bash".to_string(), "git:*".to_string());
        let req = Permission::Scoped("Bash".to_string(), "git:status".to_string());
        assert!(grant_satisfies(&grant, &req));
    }

    #[test]
    fn test_bash_no_cross_tool() {
        let grant = Permission::Scoped("Bash".to_string(), "git:*".to_string());
        let req = Permission::Scoped("Bash".to_string(), "gh:*".to_string());
        assert!(!grant_satisfies(&grant, &req));
    }

    #[test]
    fn test_path_glob_containment() {
        let grant = Permission::Scoped("Write".to_string(), "/home/aaron/.claude/**".to_string());
        let req = Permission::Scoped("Write".to_string(), "/home/aaron/.claude/foo".to_string());
        assert!(grant_satisfies(&grant, &req));
    }

    #[test]
    fn test_audit_finds_missing() {
        let grants = vec![
            Permission::parse("Bash(git:*)").unwrap(),
            Permission::parse("Read").unwrap(),
        ];
        let reqs = vec![
            ("github".to_string(), vec!["Bash(git:*)".to_string(), "Bash(gh:*)".to_string()]),
        ];
        let results = audit(&reqs, &grants);
        assert_eq!(results.len(), 2);
        assert!(results[0].granted);  // git:* — granted
        assert!(!results[1].granted); // gh:* — missing
    }
}
