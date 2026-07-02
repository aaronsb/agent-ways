//! The `settings.json` three-way merge (ADR-142's shared-write seam).
//!
//! `settings.json` is owned by Claude Code; agent-ways co-owns only two slices:
//! the **hook entries** it ships and its **permission strings**. Everything else
//! (model, theme, plugins, env, credentials) is the user's and must survive
//! untouched. This is the `kubectl apply` problem — multiple writers, one
//! declarative object, field-level ownership — solved the same way: a **three-way
//! merge** keyed on a persisted *last-applied base*.
//!
//! - **base** — the exact slices we wrote last time ([`Owned`], stored in
//!   `$XDG_STATE/agent-ways/settings-applied.json`).
//! - **ours** — the desired slices (hooks from the app's settings.json; the
//!   static permission set).
//! - **theirs** — the live `settings.json` as it is now.
//!
//! Per owned slice: `result = (theirs − base − ours) ++ ours`. Dropping `base`
//! removes entries we previously added and no longer want; dropping `ours`
//! dedupes a re-apply; keeping the rest preserves the user's own hooks/perms.
//!
//! The merge is **self-auditing**: after writing, [`stripped_user_view`] of the
//! new file must equal that of the backup — i.e. the user's portion is provably
//! unchanged under JSON `Value` (semantic) equality. The whole file is
//! re-serialized via `to_string_pretty`, so the bytes aren't necessarily
//! identical, but `preserve_order` keeps key order stable and the *meaning* of
//! every unmanaged field is held invariant. If it isn't, the write is reverted
//! from the backup — turning a botched merge into a loud failure, not silent
//! corruption.

use anyhow::{bail, Context, Result};
use serde_json::{Map, Value};
use std::path::Path;

/// The permission strings agent-ways owns in `permissions.allow`.
pub const WAYS_PERMS: &[&str] = &[
    "Bash(ways:*)",
    "Bash(attend:*)",
    "Bash(attend-chat:*)",
    "Bash(way-embed:*)",
    "Edit(~/.claude/**)",
    "Write(~/.claude/**)",
];

/// The framework-default secret-path `permissions.deny` baseline (ADR-152).
/// Set-unioned into `permissions.deny` unless `config.secret_path_deny` is false.
/// A hard block on the agent's file tools reaching credential material — narrow
/// and high-confidence (paths with essentially no legitimate agent read target).
/// `.env.example` / `.env.sample` are intentionally NOT denied. Bash-path access
/// is out of scope (see the ADR).
pub const WAYS_DENY: &[&str] = &[
    "Read(~/.ssh/**)",
    "Edit(~/.ssh/**)",
    "Write(~/.ssh/**)",
    "Read(~/.aws/**)",
    "Read(~/.gnupg/**)",
    "Read(~/.config/gcloud/**)",
    "Read(~/.kube/config)",
    "Read(~/.netrc)",
    "Read(./.env)",
    "Read(./.env.local)",
];

/// The slices agent-ways last applied — the merge base.
#[derive(Debug, Clone, Default)]
pub struct Owned {
    /// Hook entries we wrote, per event (`SessionStart`, `PreToolUse`, …).
    pub hooks: Map<String, Value>,
    /// `permissions.allow` strings we added.
    pub perms: Vec<String>,
    /// `permissions.deny` strings we added (ADR-152).
    pub deny: Vec<String>,
}

impl Owned {
    fn from_value(v: &Value) -> Owned {
        let hooks = v.get("hooks").and_then(|h| h.as_object()).cloned().unwrap_or_default();
        let str_vec = |key: &str| {
            v.get(key)
                .and_then(|p| p.as_array())
                .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                .unwrap_or_default()
        };
        Owned { hooks, perms: str_vec("perms"), deny: str_vec("deny") }
    }

    fn to_value(&self) -> Value {
        serde_json::json!({
            "hooks": Value::Object(self.hooks.clone()),
            "perms": self.perms,
            "deny": self.deny,
        })
    }
}

/// Result of a merge: the new settings document and the new base to persist.
pub struct Merged {
    pub settings: Value,
    pub base: Owned,
}

/// Three-way merge of our owned slices into `live`, given the desired hooks and
/// the prior base. Pure: no I/O, fully testable.
pub fn merge(
    live: &Value,
    desired_hooks: &Value,
    base: &Owned,
    deny_secrets: bool,
) -> Result<Merged> {
    let mut out = live.as_object().cloned().unwrap_or_default();

    // --- hooks: per-event three-way ---
    let theirs_hooks = live.get("hooks").and_then(|h| h.as_object()).cloned().unwrap_or_default();
    let ours_hooks = desired_hooks.as_object().cloned().unwrap_or_default();

    let mut new_hooks: Map<String, Value> = Map::new();
    // Union of all event keys across theirs and ours, preserving a stable order:
    // their existing events first, then any new ones we introduce.
    let mut events: Vec<String> = theirs_hooks.keys().cloned().collect();
    for k in ours_hooks.keys() {
        if !events.contains(k) {
            events.push(k.clone());
        }
    }

    for event in &events {
        let theirs = theirs_hooks.get(event).and_then(|v| v.as_array()).cloned().unwrap_or_default();
        let base_entries =
            base.hooks.get(event).and_then(|v| v.as_array()).cloned().unwrap_or_default();
        let ours = ours_hooks.get(event).and_then(|v| v.as_array()).cloned().unwrap_or_default();
        // The form we actually write (command paths quoted, Windows-space safety).
        let ours_quoted: Vec<Value> = ours.iter().map(quote_entry_commands).collect();

        // User entries = theirs minus what we wrote last (base) minus what we're
        // about to write — matched in BOTH raw and quoted form. A settled install
        // stores our hooks quoted, so comparing only against raw `ours` fails to
        // recognize them: with an under-recording base (e.g. one whose hooks were
        // lost) we'd then duplicate every hook on re-apply and trip the self-audit.
        // NB: this byte-match assumes `quote_entry_commands` stays in lockstep with
        // the jq quoting the hook-install path applies (both quote the first token).
        // `entry_is_ours` is the base-independent backstop: it also drops a prior
        // entry of OURS whose command CHANGED (so it matches neither base nor the
        // new `ours`), which byte-matching alone would leave lingering.
        let mut merged: Vec<Value> = theirs
            .into_iter()
            .filter(|e| {
                !base_entries.contains(e)
                    && !ours.contains(e)
                    && !ours_quoted.contains(e)
                    && !entry_is_ours(e)
            })
            .collect();
        merged.extend(ours_quoted);

        if !merged.is_empty() {
            new_hooks.insert(event.clone(), Value::Array(merged));
        }
    }

    // The base records exactly the (quoted) hook entries we contributed.
    let mut base_hooks: Map<String, Value> = Map::new();
    for (event, ours) in &ours_hooks {
        if let Some(arr) = ours.as_array() {
            let quoted: Vec<Value> = arr.iter().map(quote_entry_commands).collect();
            base_hooks.insert(event.clone(), Value::Array(quoted));
        }
    }

    if new_hooks.is_empty() {
        out.remove("hooks");
    } else {
        out.insert("hooks".into(), Value::Object(new_hooks));
    }

    // --- permissions.allow: set-union with removal of deprecated-ours ---
    let mut perms_obj = out
        .get("permissions")
        .and_then(|p| p.as_object())
        .cloned()
        .unwrap_or_default();
    let theirs_allow = perms_obj.get("allow").and_then(|a| a.as_array()).cloned().unwrap_or_default();
    let ours_perms: Vec<String> = WAYS_PERMS.iter().map(|s| s.to_string()).collect();

    // Keep their entries except ones we previously added and no longer want.
    let deprecated: Vec<String> =
        base.perms.iter().filter(|p| !ours_perms.contains(p)).cloned().collect();
    let mut new_allow: Vec<Value> = theirs_allow
        .into_iter()
        .filter(|v| {
            v.as_str()
                .map(|s| !deprecated.iter().any(|d| d == s) && !ours_perms.iter().any(|o| o == s))
                .unwrap_or(true)
        })
        .collect();
    for p in &ours_perms {
        new_allow.push(Value::String(p.clone()));
    }
    perms_obj.insert("allow".into(), Value::Array(new_allow));

    // --- permissions.deny: same set-union + deprecated-ours cleanup (ADR-152) ---
    // `ours_deny` is empty when the operator opts out (`secret_path_deny: false`),
    // so the opt-out flows through the ordinary deprecated-cleanup: entries we
    // added on a prior reconcile are dropped here, restoring the user's own deny.
    let theirs_deny = perms_obj.get("deny").and_then(|a| a.as_array()).cloned().unwrap_or_default();
    let ours_deny: Vec<String> = if deny_secrets {
        WAYS_DENY.iter().map(|s| s.to_string()).collect()
    } else {
        Vec::new()
    };
    let deprecated_deny: Vec<String> =
        base.deny.iter().filter(|p| !ours_deny.contains(p)).cloned().collect();
    let mut new_deny: Vec<Value> = theirs_deny
        .into_iter()
        .filter(|v| {
            v.as_str()
                .map(|s| !deprecated_deny.iter().any(|d| d == s) && !ours_deny.iter().any(|o| o == s))
                .unwrap_or(true)
        })
        .collect();
    for d in &ours_deny {
        new_deny.push(Value::String(d.clone()));
    }
    if new_deny.is_empty() {
        perms_obj.remove("deny");
    } else {
        perms_obj.insert("deny".into(), Value::Array(new_deny));
    }

    out.insert("permissions".into(), Value::Object(perms_obj));

    Ok(Merged {
        settings: Value::Object(out),
        base: Owned { hooks: base_hooks, perms: ours_perms, deny: ours_deny },
    })
}

/// The user's portion of a settings doc: everything *except* the slices `base`
/// says we own. Two docs with equal user-views differ only in our fields — the
/// invariant the post-write check asserts.
pub fn stripped_user_view(settings: &Value, base: &Owned) -> Value {
    let mut obj = settings.as_object().cloned().unwrap_or_default();

    // Strip our hook entries per event.
    if let Some(hooks) = obj.get("hooks").and_then(|h| h.as_object()).cloned() {
        let mut user_hooks: Map<String, Value> = Map::new();
        for (event, entries) in &hooks {
            let ours = base.hooks.get(event).and_then(|v| v.as_array()).cloned().unwrap_or_default();
            // Strip our entries by the recorded base AND by structural signature, so
            // a stale/under-recording base can't leave an old-form entry of ours in
            // the user view — which would make before/after asymmetric and trip a
            // spurious revert. See `entry_is_ours`.
            let kept: Vec<Value> = entries
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(|e| !ours.contains(e) && !entry_is_ours(e))
                .collect();
            if !kept.is_empty() {
                user_hooks.insert(event.clone(), Value::Array(kept));
            }
        }
        if user_hooks.is_empty() {
            obj.remove("hooks");
        } else {
            obj.insert("hooks".into(), Value::Object(user_hooks));
        }
    }

    // Strip our perms from permissions.allow and our deny baseline from
    // permissions.deny — both are owned slices held invariant by the self-audit.
    if let Some(mut perms) = obj.get("permissions").and_then(|p| p.as_object()).cloned() {
        for (key, owned) in [("allow", &base.perms), ("deny", &base.deny)] {
            if let Some(entries) = perms.get(key).and_then(|a| a.as_array()).cloned() {
                let kept: Vec<Value> = entries
                    .into_iter()
                    .filter(|v| v.as_str().map(|s| !owned.iter().any(|p| p == s)).unwrap_or(true))
                    .collect();
                if kept.is_empty() {
                    perms.remove(key);
                } else {
                    perms.insert(key.into(), Value::Array(kept));
                }
            }
        }
        if perms.is_empty() {
            obj.remove("permissions");
        } else {
            obj.insert("permissions".into(), Value::Object(perms));
        }
    }

    Value::Object(obj)
}

/// Union of two owned records. The self-audit strips our contribution from the
/// pre-write `live`; using `prior ∪ new` ensures an under-recording base still
/// strips everything we own (old deprecated entries *and* current ones), so a
/// base whose hooks were lost can't trigger a spurious revert.
fn union_owned(a: &Owned, b: &Owned) -> Owned {
    let mut hooks = a.hooks.clone();
    for (event, entries) in &b.hooks {
        let mut combined =
            hooks.get(event).and_then(|v| v.as_array()).cloned().unwrap_or_default();
        if let Some(src) = entries.as_array() {
            for e in src {
                if !combined.contains(e) {
                    combined.push(e.clone());
                }
            }
        }
        hooks.insert(event.clone(), Value::Array(combined));
    }
    let union_vec = |a: &[String], b: &[String]| {
        let mut out = a.to_vec();
        for x in b {
            if !out.contains(x) {
                out.push(x.clone());
            }
        }
        out
    };
    Owned {
        hooks,
        perms: union_vec(&a.perms, &b.perms),
        deny: union_vec(&a.deny, &b.deny),
    }
}

/// Quote the first whitespace-bearing command-path token so a `${HOME}` that
/// expands to a spaced Windows path stays one token. Mirrors the jq transform.
fn quote_entry_commands(entry: &Value) -> Value {
    let mut e = entry.clone();
    if let Some(hooks) = e.get_mut("hooks").and_then(|h| h.as_array_mut()) {
        for h in hooks.iter_mut() {
            if let Some(cmd) = h.get("command").and_then(|c| c.as_str()) {
                let quoted = quote_first_token(cmd);
                if let Some(obj) = h.as_object_mut() {
                    obj.insert("command".into(), Value::String(quoted));
                }
            }
        }
    }
    e
}

fn quote_first_token(cmd: &str) -> String {
    if cmd.starts_with('"') {
        return cmd.to_string();
    }
    match cmd.find(' ') {
        None => format!("\"{cmd}\""),
        Some(i) => format!("\"{}\"{}", &cmd[..i], &cmd[i..]),
    }
}

/// True if a hook *command*'s executable is an agent-ways projection artifact —
/// a `.claude/hooks/…` script or a `.claude/bin/…` CLI (`ways`, `way-embed`,
/// `attend`). Only the **first (exe) token** is examined (quote-aware), so a
/// user hook that merely passes a projection path as an *argument* — e.g.
/// `tail -f ${HOME}/.claude/hooks/ways/x.log` — is NOT mistaken for ours. That
/// distinction is load-bearing: misclassifying a user hook would silently drop
/// it (the self-audit strips it symmetrically, so no revert catches it).
///
/// Path separators are normalized so a Windows `\`-form path matches. Matches
/// the literal `${HOME}`-prefixed form we ship and the expanded/quoted forms a
/// prior merge wrote.
///
/// Assumes the ADR-142 projection root (`~/.claude`) — the hooks we ship are
/// written with a literal `${HOME}/.claude/...` prefix, so a relocated config
/// dir (`CLAUDE_CONFIG_DIR`) is out of scope; if that ever changes, the
/// shipped-hooks guard test (`shipped_hooks_are_all_recognized_as_ours`) fails
/// loudly rather than the reconciler silently regressing.
fn command_is_ours(cmd: &str) -> bool {
    let exe = exe_token(cmd).replace('\\', "/");
    exe.contains(".claude/hooks/") || exe.contains(".claude/bin/")
}

/// The executable token of a hook command: the leading `"`-quoted span when the
/// command is quoted (the form our installer writes, e.g.
/// `"${HOME}/.claude/bin/ways" corpus`), else the run up to the first
/// whitespace. Mirrors the tokenization `quote_first_token` assumes.
fn exe_token(cmd: &str) -> &str {
    if let Some(rest) = cmd.strip_prefix('"') {
        match rest.find('"') {
            Some(i) => &rest[..i],
            None => rest,
        }
    } else {
        cmd.split_whitespace().next().unwrap_or(cmd)
    }
}

/// True if a hook *entry* is one agent-ways ships: it runs at least one command
/// and **every** command targets the projection ([`command_is_ours`]).
///
/// This makes hook ownership *self-identifying* — the merge and self-audit can
/// recognize our entries structurally, independent of the persisted base. That
/// matters because the base is only rewritten after a non-no-op apply (the
/// idempotent early-return skips it), so a long run of converged reconciles
/// leaves it stale (e.g. `hooks: {}`). The first update that *changes* a hook
/// then can't attribute the prior (old-command) entry to us via the base, and
/// the self-audit would false-positive "unmanaged field changed" and revert —
/// blocking exactly the updates that ship new hooks. Byte-matching against the
/// base still runs first; this is the base-independent backstop.
///
/// Trade-off: a user hook placed under `.claude/hooks/` (contrary to the ways
/// model, which extends via `ways/` fragments — not hand-edited settings.json)
/// would be treated as ours. Accepted and documented.
fn entry_is_ours(entry: &Value) -> bool {
    match entry.get("hooks").and_then(|h| h.as_array()) {
        Some(cmds) if !cmds.is_empty() => cmds.iter().all(|h| {
            h.get("command").and_then(|c| c.as_str()).map(command_is_ours).unwrap_or(false)
        }),
        _ => false,
    }
}

/// Apply the merge to the live files: back up, merge, atomic-write, persist the
/// base, and verify the user's portion is unchanged (reverting if not).
///
/// `source_settings` is the app's settings.json (for the desired hooks).
/// `dest_settings` is the live `~/.claude/settings.json`. `base_path` is the
/// persisted last-applied record. Returns a one-line summary.
pub fn apply_to_files(
    source_settings: &Path,
    dest_settings: &Path,
    base_path: &Path,
) -> Result<String> {
    let desired: Value = read_json_or_empty(source_settings)?;
    let desired_hooks = desired.get("hooks").cloned().unwrap_or(Value::Object(Map::new()));

    let live: Value = read_json_or_empty(dest_settings)?;
    // The merge base. With a last-applied record, use it (per-entry user-hook
    // preservation on subsequent applies). WITHOUT one — the first apply, e.g.
    // migrating an existing in-place install whose settings.json already holds
    // agent-ways hooks + perms from the pre-1.0 wholesale-replace sync — seed the
    // base from the live content so those pre-existing entries are treated as
    // OURS: replaced wholesale (matching pre-1.0 behavior) rather than mistaken
    // for user content. Without this seed the merge both duplicated our old hooks
    // in the result and the self-audit false-positived on us removing them.
    let base = if base_path.exists() {
        read_json_or_empty(base_path).map(|v| Owned::from_value(&v)).unwrap_or_default()
    } else {
        Owned {
            hooks: live.get("hooks").and_then(|h| h.as_object()).cloned().unwrap_or_default(),
            perms: WAYS_PERMS.iter().map(|s| s.to_string()).collect(),
            deny: WAYS_DENY.iter().map(|s| s.to_string()).collect(),
        }
    };

    let merged = merge(&live, &desired_hooks, &base, crate::config::global().secret_path_deny)?;

    // Idempotent: if nothing changed, don't churn the file or a backup.
    if merged.settings == live {
        return Ok("settings.json already up to date".into());
    }

    // Back up the live file before writing.
    let backup = dest_settings.with_extension("json.bak");
    if dest_settings.exists() {
        std::fs::copy(dest_settings, &backup)
            .with_context(|| format!("backing up {}", dest_settings.display()))?;
    }

    // Atomic write (temp + rename) so a crash never leaves a half-written file.
    write_json_atomic(dest_settings, &merged.settings)?;

    // Self-audit: the user's view must be identical before and after. Strip OUR
    // contribution from both sides — but from `live` use the union of the prior
    // base and the new base. A base that under-records what we own (e.g. one whose
    // hooks were lost) would otherwise leave our entries in the pre-write view,
    // which `after` correctly drops — a false "unmanaged field changed" revert.
    let after: Value = read_json_or_empty(dest_settings)?;
    let user_before = stripped_user_view(&live, &union_owned(&base, &merged.base));
    let user_after = stripped_user_view(&after, &merged.base);
    if user_before != user_after {
        // Revert and fail loud — we must never corrupt the user's settings.
        if backup.exists() {
            std::fs::copy(&backup, dest_settings).ok();
        }
        bail!(
            "settings merge changed unmanaged fields — reverted from {}. \
             This is a bug in the merge; the backup is your settings as they were.",
            backup.display()
        );
    }

    // Persist the new base only after a verified write.
    if let Some(parent) = base_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    write_json_atomic(base_path, &merged.base.to_value())?;

    Ok(format!(
        "merged settings.json (hooks + {} ways permissions); backup at {}",
        WAYS_PERMS.len(),
        backup.display()
    ))
}

fn read_json_or_empty(p: &Path) -> Result<Value> {
    match std::fs::read_to_string(p) {
        Ok(s) if s.trim().is_empty() => Ok(Value::Object(Map::new())),
        Ok(s) => serde_json::from_str(&s).with_context(|| format!("parsing {}", p.display())),
        Err(_) => Ok(Value::Object(Map::new())),
    }
}

pub(crate) fn write_json_atomic(p: &Path, v: &Value) -> Result<()> {
    let tmp = p.with_extension(format!("json.tmp.{}", std::process::id()));
    let body = serde_json::to_string_pretty(v)?;
    std::fs::write(&tmp, body).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, p).with_context(|| format!("renaming into {}", p.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ours_hooks() -> Value {
        json!({
            "SessionStart": [
                { "matcher": "startup", "hooks": [ { "type": "command", "command": "${HOME}/.claude/hooks/ways/check-setup.sh" } ] }
            ]
        })
    }

    #[test]
    fn fresh_merge_adds_hooks_and_perms() {
        let live = json!({});
        let m = merge(&live, &ours_hooks(), &Owned::default(), false).unwrap();
        assert!(m.settings["hooks"]["SessionStart"].is_array());
        let allow = m.settings["permissions"]["allow"].as_array().unwrap();
        assert_eq!(allow.len(), WAYS_PERMS.len());
        assert!(allow.iter().any(|v| v == "Bash(ways:*)"));
    }

    #[test]
    fn merge_is_idempotent() {
        let live = json!({});
        let m1 = merge(&live, &ours_hooks(), &Owned::default(), false).unwrap();
        // Second apply, with the first run's base and its output as the live file.
        let m2 = merge(&m1.settings, &ours_hooks(), &m1.base, false).unwrap();
        assert_eq!(m1.settings, m2.settings, "merge must be idempotent");
    }

    #[test]
    fn preserves_unrelated_user_keys() {
        let live = json!({ "model": "opus", "theme": "dark", "permissions": { "deny": ["Bash(rm:*)"] } });
        let m = merge(&live, &ours_hooks(), &Owned::default(), false).unwrap();
        assert_eq!(m.settings["model"], "opus");
        assert_eq!(m.settings["theme"], "dark");
        // permissions.deny (user's) survives alongside our allow additions.
        assert_eq!(m.settings["permissions"]["deny"][0], "Bash(rm:*)");
    }

    fn deny_list(settings: &Value) -> Vec<String> {
        settings["permissions"]["deny"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default()
    }

    #[test]
    fn deny_baseline_added_when_enabled() {
        let m = merge(&json!({}), &ours_hooks(), &Owned::default(), true).unwrap();
        let deny = deny_list(&m.settings);
        for want in WAYS_DENY {
            assert!(deny.iter().any(|d| d == want), "missing deny entry {want}");
        }
        assert_eq!(m.base.deny.len(), WAYS_DENY.len(), "base records owned deny");
    }

    #[test]
    fn deny_baseline_absent_when_opted_out() {
        // Opted out + no user deny → no `deny` key written at all.
        let m = merge(&json!({}), &ours_hooks(), &Owned::default(), false).unwrap();
        assert!(m.settings["permissions"].get("deny").is_none());
        assert!(m.base.deny.is_empty());
    }

    #[test]
    fn deny_preserves_user_deny_entries() {
        let live = json!({ "permissions": { "deny": ["Bash(rm:*)"] } });
        let m = merge(&live, &ours_hooks(), &Owned::default(), true).unwrap();
        let deny = deny_list(&m.settings);
        assert!(deny.iter().any(|d| d == "Bash(rm:*)"), "user deny must survive");
        assert!(deny.iter().any(|d| d == "Read(~/.ssh/**)"), "ours added alongside");
    }

    #[test]
    fn opt_out_removes_previously_owned_deny_keeps_user() {
        // A settled install: our baseline is present and recorded in the base.
        let enabled = merge(&json!({ "permissions": { "deny": ["Bash(rm:*)"] } }), &ours_hooks(), &Owned::default(), true).unwrap();
        // Now the operator opts out — our owned deny must vanish, the user's stays.
        let opted = merge(&enabled.settings, &ours_hooks(), &enabled.base, false).unwrap();
        let deny = deny_list(&opted.settings);
        assert_eq!(deny, vec!["Bash(rm:*)"], "only the user's own deny remains");
        assert!(opted.base.deny.is_empty());
    }

    #[test]
    fn deny_is_idempotent() {
        let m1 = merge(&json!({}), &ours_hooks(), &Owned::default(), true).unwrap();
        let m2 = merge(&m1.settings, &ours_hooks(), &m1.base, true).unwrap();
        assert_eq!(m1.settings, m2.settings, "re-applying the deny baseline is stable");
    }

    #[test]
    fn user_view_invariant_holds_with_deny() {
        // The self-audit: stripping our owned slices from before/after must match.
        let live = json!({ "model": "opus", "permissions": { "deny": ["Bash(rm:*)"] } });
        let m = merge(&live, &ours_hooks(), &Owned::default(), true).unwrap();
        assert_eq!(
            stripped_user_view(&live, &Owned::default()),
            stripped_user_view(&m.settings, &m.base),
            "the user's portion is provably unchanged under the deny baseline"
        );
    }

    #[test]
    fn preserves_user_authored_hooks() {
        // The key three-way property: a user's own hook entry is NOT clobbered.
        let user_hook = json!({ "matcher": "startup", "hooks": [ { "type": "command", "command": "/usr/local/bin/my-thing" } ] });
        let live = json!({ "hooks": { "SessionStart": [ user_hook.clone() ] } });
        let m = merge(&live, &ours_hooks(), &Owned::default(), false).unwrap();
        let entries = m.settings["hooks"]["SessionStart"].as_array().unwrap();
        assert!(entries.contains(&user_hook), "user hook must survive the merge");
        assert!(entries.len() >= 2, "both user and our hook present");
    }

    #[test]
    fn removes_our_deprecated_hooks_but_keeps_user() {
        // base says we previously wrote an old hook; ours no longer has it.
        let old = json!({ "matcher": "startup", "hooks": [ { "type": "command", "command": "${HOME}/.claude/hooks/ways/OLD.sh" } ] });
        let user_hook = json!({ "matcher": "stop", "hooks": [ { "type": "command", "command": "/usr/local/bin/mine" } ] });
        let live = json!({ "hooks": { "SessionStart": [ old.clone() ], "Stop": [ user_hook.clone() ] } });
        let mut base = Owned::default();
        base.hooks.insert("SessionStart".into(), json!([ old.clone() ]));

        let m = merge(&live, &ours_hooks(), &base, false).unwrap();
        let ss = m.settings["hooks"]["SessionStart"].as_array().unwrap();
        assert!(!ss.contains(&old), "our deprecated hook must be removed");
        // User's unrelated hook on another event is untouched.
        assert_eq!(m.settings["hooks"]["Stop"][0], user_hook);
    }

    #[test]
    fn user_view_invariant_holds_across_merge() {
        let live = json!({
            "model": "opus",
            "hooks": { "SessionStart": [ { "matcher": "startup", "hooks": [ { "type": "command", "command": "/u/mine" } ] } ] },
            "permissions": { "allow": ["Bash(git:*)"], "deny": ["Bash(rm:*)"] }
        });
        let m = merge(&live, &ours_hooks(), &Owned::default(), false).unwrap();
        // Stripping our slices from before and after must yield identical user views.
        let before = stripped_user_view(&live, &Owned::default());
        let after = stripped_user_view(&m.settings, &m.base);
        assert_eq!(before, after, "user portion must be preserved exactly");
    }

    #[test]
    fn quote_first_token_quotes_the_exe_path() {
        // Real inputs carry a LITERAL ${HOME} (no space until the shell expands
        // it), so quoting up to the first space wraps exactly the exe path —
        // keeping a later-expanded "C:\Users\John Doe\..." a single token.
        assert_eq!(
            quote_first_token("${HOME}/.claude/bin/ways corpus --quiet"),
            "\"${HOME}/.claude/bin/ways\" corpus --quiet"
        );
        // No args → the whole command is the exe path.
        assert_eq!(
            quote_first_token("${HOME}/.claude/hooks/ways/check-setup.sh"),
            "\"${HOME}/.claude/hooks/ways/check-setup.sh\""
        );
        // Already quoted → untouched (idempotent).
        assert_eq!(quote_first_token("\"already\" quoted"), "\"already\" quoted");
    }

    #[test]
    fn migrating_existing_install_seeds_base_and_passes_self_audit() {
        // Reproduces the north bug: an existing in-place install whose settings.json
        // already holds agent-ways hooks + the ways perms (from the pre-1.0
        // wholesale-replace sync) PLUS user content, with NO last-applied base
        // record (first migration). Without base-seeding the merge duplicated the
        // old hooks and the self-audit aborted the migration.
        use std::sync::atomic::{AtomicU32, Ordering};
        static SEQ: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir()
            .join(format!("ways-setmerge-{}-{}", std::process::id(), SEQ.fetch_add(1, Ordering::SeqCst)));
        std::fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("settings.json");
        let source = dir.join("source.json");
        let base = dir.join("base.json"); // deliberately does NOT exist

        std::fs::write(&dest, serde_json::to_string(&json!({
            "model": "opus",
            "theme": "dark",
            "hooks": { "SessionStart": [ { "matcher": "startup", "hooks": [
                { "type": "command", "command": "${HOME}/.claude/hooks/ways/OLD-check.sh" } ] } ] },
            "permissions": { "allow": ["Bash(ways:*)", "Bash(git:*)"], "deny": ["Bash(rm:*)"] }
        })).unwrap()).unwrap();
        std::fs::write(&source, serde_json::to_string(&json!({
            "hooks": { "SessionStart": [ { "matcher": "startup", "hooks": [
                { "type": "command", "command": "${HOME}/.claude/hooks/ways/check-setup.sh" } ] } ] }
        })).unwrap()).unwrap();

        // Must NOT abort — the self-audit must pass.
        apply_to_files(&source, &dest, &base).expect("migration settings merge must not abort");

        let after: Value = serde_json::from_str(&std::fs::read_to_string(&dest).unwrap()).unwrap();
        // User content preserved.
        assert_eq!(after["model"], "opus");
        assert_eq!(after["theme"], "dark");
        assert_eq!(after["permissions"]["deny"][0], "Bash(rm:*)");
        assert!(after["permissions"]["allow"].as_array().unwrap().iter().any(|v| v == "Bash(git:*)"));
        // Our old hook REPLACED by the new one — not duplicated, not lingering.
        let ss = after["hooks"]["SessionStart"].as_array().unwrap();
        assert_eq!(ss.len(), 1, "old agent-ways hook must be replaced, not duplicated: {ss:?}");
        let cmd = ss[0]["hooks"][0]["command"].as_str().unwrap();
        assert!(cmd.contains("check-setup.sh") && !cmd.contains("OLD-check.sh"), "got: {cmd}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reapply_is_idempotent_when_base_hooks_were_lost() {
        // A settled 1.0 install whose persisted base LOST its hooks record
        // (`hooks: {}`, perms still recorded) while the live settings.json already
        // holds our hooks in the QUOTED form a prior merge wrote. Re-applying must
        // not duplicate our hooks and must not trip the self-audit. Before the fix
        // the merge dedup'd `theirs` (quoted) only against raw (unquoted) `ours`, so
        // it duplicated every hook, and the empty base made the strip asymmetric —
        // reverting on every reconcile, forever (base never got corrected).
        use std::sync::atomic::{AtomicU32, Ordering};
        static SEQ: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "ways-setmerge-lostbase-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("settings.json");
        let source = dir.join("source.json");
        let base = dir.join("base.json");

        // Source ships one unquoted hook.
        std::fs::write(&source, serde_json::to_string(&json!({
            "hooks": { "SessionStart": [ { "matcher": "startup", "hooks": [
                { "type": "command", "command": "${HOME}/.claude/hooks/ways/check-setup.sh" } ] } ] }
        })).unwrap()).unwrap();

        // Live already holds our hook QUOTED (a prior successful merge wrote it),
        // plus a genuine user hook and a user perm.
        std::fs::write(&dest, serde_json::to_string(&json!({
            "model": "opus",
            "hooks": { "SessionStart": [
                { "matcher": "startup", "hooks": [ { "type": "command",
                    "command": "\"${HOME}/.claude/hooks/ways/check-setup.sh\"" } ] },
                { "matcher": "startup", "hooks": [ { "type": "command", "command": "/u/mine" } ] }
            ] },
            "permissions": { "allow": ["Bash(git:*)", "Bash(ways:*)", "Bash(attend:*)",
                "Bash(attend-chat:*)", "Bash(way-embed:*)", "Edit(~/.claude/**)", "Write(~/.claude/**)"] }
        })).unwrap()).unwrap();

        // Persisted base EXISTS but its hooks were lost; perms recorded.
        std::fs::write(&base, serde_json::to_string(&json!({
            "hooks": {},
            "perms": WAYS_PERMS
        })).unwrap()).unwrap();

        apply_to_files(&source, &dest, &base)
            .expect("re-apply with lost base hooks must not abort");

        let after: Value = serde_json::from_str(&std::fs::read_to_string(&dest).unwrap()).unwrap();
        let ss = after["hooks"]["SessionStart"].as_array().unwrap();
        assert_eq!(ss.len(), 2, "our hook must not be duplicated: {ss:?}");
        assert!(ss.iter().any(|e| e["hooks"][0]["command"] == "/u/mine"), "user hook preserved");
        assert_eq!(after["model"], "opus");
        assert!(after["permissions"]["allow"].as_array().unwrap().iter().any(|v| v == "Bash(git:*)"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn update_that_changes_our_hooks_with_a_lost_base_converges() {
        // The `north` reconcile failure: an `ways update` pulled CHANGED hook
        // definitions while the persisted base was stale (`hooks: {}`, from a long
        // run of no-op reconciles that never rewrote it). Live still held the OLD
        // ways-hook; source shipped a NEW one with a different command. Byte-matching
        // couldn't attribute the old entry to us (not in base, not equal to the new
        // `ours`), so the self-audit saw it as "user content that vanished" and
        // reverted with "settings merge changed unmanaged fields" — blocking exactly
        // the updates that ship new hooks. `entry_is_ours` (structural ownership)
        // fixes it: the old ways-hook is recognized as ours and replaced, not lingered.
        use std::sync::atomic::{AtomicU32, Ordering};
        static SEQ: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "ways-setmerge-changed-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("settings.json");
        let source = dir.join("source.json");
        let base = dir.join("base.json");

        // Source ships the NEW hook command.
        std::fs::write(&source, serde_json::to_string(&json!({
            "hooks": { "SessionStart": [ { "matcher": "startup", "hooks": [
                { "type": "command", "command": "${HOME}/.claude/hooks/ways/check-state.sh" } ] } ] }
        })).unwrap()).unwrap();

        // Live holds the OLD ways-hook (quoted, DIFFERENT command) + a genuine user
        // hook. This is the crux: old command != new command, so quoted/raw dedup
        // against `ours` cannot catch it.
        std::fs::write(&dest, serde_json::to_string(&json!({
            "model": "opus",
            "hooks": { "SessionStart": [
                { "matcher": "startup", "hooks": [ { "type": "command",
                    "command": "\"${HOME}/.claude/hooks/ways/check-setup.sh\"" } ] },
                { "matcher": "stop", "hooks": [ { "type": "command", "command": "/u/mine" } ] }
            ] },
            "permissions": { "allow": ["Bash(git:*)", "Bash(ways:*)", "Bash(attend:*)",
                "Bash(attend-chat:*)", "Bash(way-embed:*)", "Edit(~/.claude/**)", "Write(~/.claude/**)"] }
        })).unwrap()).unwrap();

        // Base is stale: hooks lost, perms recorded.
        std::fs::write(&base, serde_json::to_string(&json!({
            "hooks": {},
            "perms": WAYS_PERMS
        })).unwrap()).unwrap();

        apply_to_files(&source, &dest, &base)
            .expect("update that changes our hooks must not abort on a stale base");

        let after: Value = serde_json::from_str(&std::fs::read_to_string(&dest).unwrap()).unwrap();
        let ss = after["hooks"]["SessionStart"].as_array().unwrap();
        // Old ways-hook GONE, new one present, user hook preserved — no duplication.
        assert_eq!(ss.len(), 2, "old ours replaced (not lingered/duplicated): {ss:?}");
        let cmds: Vec<&str> = ss.iter().filter_map(|e| e["hooks"][0]["command"].as_str()).collect();
        assert!(cmds.iter().any(|c| c.contains("check-state.sh")), "new hook present: {cmds:?}");
        assert!(!cmds.iter().any(|c| c.contains("check-setup.sh")), "old hook removed: {cmds:?}");
        assert!(cmds.contains(&"/u/mine"), "user hook preserved: {cmds:?}");
        assert_eq!(after["model"], "opus", "user content preserved");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn command_is_ours_examines_the_exe_token_only() {
        // Ours: script + CLI, unquoted and quoted-with-args.
        assert!(command_is_ours("${HOME}/.claude/hooks/ways/check-setup.sh"));
        assert!(command_is_ours("\"${HOME}/.claude/hooks/ways/check-setup.sh\""));
        assert!(command_is_ours("\"${HOME}/.claude/bin/ways\" corpus --quiet"));
        // Sibling CLIs under .claude/bin/ (broadened past just `ways`).
        assert!(command_is_ours("${HOME}/.claude/bin/way-embed match"));
        assert!(command_is_ours("${HOME}/.claude/bin/attend"));
        // Windows backslash form of our path.
        assert!(command_is_ours("\"C:\\Users\\j\\.claude\\hooks\\ways\\c.sh\""));
        // NOT ours: a user command that merely references a projection path as an
        // ARGUMENT — the exe is `tail`, not ours. Must not be swept up (else it is
        // silently deleted with no revert).
        assert!(!command_is_ours("tail -f ${HOME}/.claude/hooks/ways/x.log"));
        assert!(!command_is_ours("cat ${HOME}/.claude/bin/ways"));
        assert!(!command_is_ours("/usr/local/bin/mine"));
    }

    #[test]
    fn entry_is_ours_requires_every_command_ours() {
        let ours = json!({ "hooks": [ { "type": "command",
            "command": "${HOME}/.claude/hooks/ways/a.sh" } ] });
        assert!(entry_is_ours(&ours));
        // A mixed entry (one ours + one user command) is NOT ours — .all() fails,
        // so the whole entry is preserved rather than dropped.
        let mixed = json!({ "hooks": [
            { "type": "command", "command": "${HOME}/.claude/hooks/ways/a.sh" },
            { "type": "command", "command": "/usr/local/bin/mine" } ] });
        assert!(!entry_is_ours(&mixed));
        // An empty/absent hooks array is not ours.
        assert!(!entry_is_ours(&json!({ "matcher": "x", "hooks": [] })));
        assert!(!entry_is_ours(&json!({ "matcher": "x" })));
    }

    #[test]
    fn merge_preserves_user_hook_referencing_our_path_as_argument() {
        // Regression for the review finding: a genuine user hook whose command
        // takes a projection path as an ARGUMENT must survive the merge untouched.
        let user_hook = json!({ "matcher": "startup", "hooks": [ { "type": "command",
            "command": "tail -f ${HOME}/.claude/hooks/ways/debug.log" } ] });
        let live = json!({ "hooks": { "SessionStart": [ user_hook.clone() ] } });
        let m = merge(&live, &ours_hooks(), &Owned::default(), false).unwrap();
        let ss = m.settings["hooks"]["SessionStart"].as_array().unwrap();
        assert!(ss.contains(&user_hook), "user hook (path-as-arg) must survive: {ss:?}");
        // And the self-audit view agrees it's user content on both sides.
        let before = stripped_user_view(&live, &Owned::default());
        let after = stripped_user_view(&m.settings, &m.base);
        assert_eq!(before, after, "user view preserved");
    }

    #[test]
    fn shipped_hooks_are_all_recognized_as_ours() {
        // Guard: every hook agent-ways ships in the repo settings.json must be
        // recognized by `entry_is_ours`. Otherwise structural ownership silently
        // regresses and a stale-base update false-reverts again. Catches a future
        // hook that shells something outside `.claude/hooks/` or `.claude/bin/`.
        let repo_settings =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../settings.json");
        let raw = match std::fs::read_to_string(&repo_settings) {
            Ok(s) => s,
            Err(_) => return, // not a source checkout — skip
        };
        let v: Value = serde_json::from_str(&raw).expect("repo settings.json parses");
        let Some(hooks) = v.get("hooks").and_then(|h| h.as_object()) else { return };
        for (event, entries) in hooks {
            for e in entries.as_array().into_iter().flatten() {
                assert!(entry_is_ours(e), "shipped hook in {event} not recognized as ours: {e}");
            }
        }
    }
}
