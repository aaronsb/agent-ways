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

/// The slices agent-ways last applied — the merge base.
#[derive(Debug, Clone, Default)]
pub struct Owned {
    /// Hook entries we wrote, per event (`SessionStart`, `PreToolUse`, …).
    pub hooks: Map<String, Value>,
    /// Permission strings we added.
    pub perms: Vec<String>,
}

impl Owned {
    fn from_value(v: &Value) -> Owned {
        let hooks = v.get("hooks").and_then(|h| h.as_object()).cloned().unwrap_or_default();
        let perms = v
            .get("perms")
            .and_then(|p| p.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
            .unwrap_or_default();
        Owned { hooks, perms }
    }

    fn to_value(&self) -> Value {
        serde_json::json!({
            "hooks": Value::Object(self.hooks.clone()),
            "perms": self.perms,
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
pub fn merge(live: &Value, desired_hooks: &Value, base: &Owned) -> Result<Merged> {
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
        let mut merged: Vec<Value> = theirs
            .into_iter()
            .filter(|e| {
                !base_entries.contains(e) && !ours.contains(e) && !ours_quoted.contains(e)
            })
            .collect();
        merged.extend(ours_quoted.iter().cloned());

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
    out.insert("permissions".into(), Value::Object(perms_obj));

    Ok(Merged {
        settings: Value::Object(out),
        base: Owned { hooks: base_hooks, perms: ours_perms },
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
            let kept: Vec<Value> = entries
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(|e| !ours.contains(e))
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

    // Strip our perms from permissions.allow.
    if let Some(mut perms) = obj.get("permissions").and_then(|p| p.as_object()).cloned() {
        if let Some(allow) = perms.get("allow").and_then(|a| a.as_array()).cloned() {
            let kept: Vec<Value> = allow
                .into_iter()
                .filter(|v| v.as_str().map(|s| !base.perms.iter().any(|p| p == s)).unwrap_or(true))
                .collect();
            if kept.is_empty() {
                perms.remove("allow");
            } else {
                perms.insert("allow".into(), Value::Array(kept));
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
    let mut perms = a.perms.clone();
    for p in &b.perms {
        if !perms.contains(p) {
            perms.push(p.clone());
        }
    }
    Owned { hooks, perms }
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
        }
    };

    let merged = merge(&live, &desired_hooks, &base)?;

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

fn write_json_atomic(p: &Path, v: &Value) -> Result<()> {
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
        let m = merge(&live, &ours_hooks(), &Owned::default()).unwrap();
        assert!(m.settings["hooks"]["SessionStart"].is_array());
        let allow = m.settings["permissions"]["allow"].as_array().unwrap();
        assert_eq!(allow.len(), WAYS_PERMS.len());
        assert!(allow.iter().any(|v| v == "Bash(ways:*)"));
    }

    #[test]
    fn merge_is_idempotent() {
        let live = json!({});
        let m1 = merge(&live, &ours_hooks(), &Owned::default()).unwrap();
        // Second apply, with the first run's base and its output as the live file.
        let m2 = merge(&m1.settings, &ours_hooks(), &m1.base).unwrap();
        assert_eq!(m1.settings, m2.settings, "merge must be idempotent");
    }

    #[test]
    fn preserves_unrelated_user_keys() {
        let live = json!({ "model": "opus", "theme": "dark", "permissions": { "deny": ["Bash(rm:*)"] } });
        let m = merge(&live, &ours_hooks(), &Owned::default()).unwrap();
        assert_eq!(m.settings["model"], "opus");
        assert_eq!(m.settings["theme"], "dark");
        // permissions.deny (user's) survives alongside our allow additions.
        assert_eq!(m.settings["permissions"]["deny"][0], "Bash(rm:*)");
    }

    #[test]
    fn preserves_user_authored_hooks() {
        // The key three-way property: a user's own hook entry is NOT clobbered.
        let user_hook = json!({ "matcher": "startup", "hooks": [ { "type": "command", "command": "/usr/local/bin/my-thing" } ] });
        let live = json!({ "hooks": { "SessionStart": [ user_hook.clone() ] } });
        let m = merge(&live, &ours_hooks(), &Owned::default()).unwrap();
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

        let m = merge(&live, &ours_hooks(), &base).unwrap();
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
        let m = merge(&live, &ours_hooks(), &Owned::default()).unwrap();
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
}
