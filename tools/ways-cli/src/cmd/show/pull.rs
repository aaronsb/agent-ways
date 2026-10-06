//! A way pulled on request: `ways_read` (ADR-701 §5).
//!
//! Serving and stamping are two steps, because only the second needs to know
//! which agent asked. [`read`] returns the body as injection renders it and
//! changes nothing: the MCP server is shared by a session's subagents and
//! cannot tell them from main. [`stamp`] runs from the PostToolUse hook on
//! `ways_read` (`ways hook pull`), whose payload names the calling agent, and
//! records the disclosure for that agent as injection does, so the next scan
//! does not repeat the way. Every pull the hook sees, stamped or refused, also
//! writes a decision record that joins its turn's scan (ADR-701 §2).
//!
//! A pull needs no match, no judge and no scope, and the refire window
//! (ADR-126) does not hold it back. The disable switches still apply, and an
//! id is validated and its file confined to the ways roots before anything is
//! read or written.

use anyhow::Result;
use serde_json::json;
use std::path::{Path, PathBuf};

use super::{fireable_in_scope, render_way, stamp_disclosure, static_way_body, MacroRun};
use crate::session;

/// What a read serves.
#[derive(Debug)]
pub struct Read {
    pub body: String,
    /// The way's `scope:` field, `agent` when it names none.
    pub scope: String,
    /// Set when the way's scope does not match the session's.
    pub note: Option<String>,
}

/// A pull refused before it touched anything. `reason` is a short fixed phrase
/// the event log can carry; `message` is for the caller and may quote the id.
#[derive(Debug)]
pub(crate) struct Refused {
    pub reason: &'static str,
    message: String,
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Refused {}

fn refuse(reason: &'static str, message: String) -> anyhow::Error {
    Refused { reason, message }.into()
}

/// The reason a pull error carries, `error` for anything unplanned.
pub(crate) fn reason_of(e: &anyhow::Error) -> &'static str {
    e.downcast_ref::<Refused>().map_or("error", |r| r.reason)
}

/// Longest id written to the event log. The id comes from the model.
const LOGGED_ID_CHARS: usize = 64;

/// Log a pull the hook declined to stamp, with the reason.
pub fn log_refused(id: &str, session_id: &str, reason: &str) {
    let shown: String = id.chars().take(LOGGED_ID_CHARS).collect();
    let project_dir = crate::util::project_dir();
    let scope = session::detect_scope(session_id);
    log_pulled(&Pulled {
        id: &shown,
        domain: "",
        session_id,
        project_dir: &project_dir,
        scope: &scope,
        tick: None,
        window: "none",
        out_of_band: false,
        epoch_distance: None,
        stamped: false,
        reason: Some(reason),
    });
}

/// An id is a way path: `/`-separated parts of `[A-Za-z0-9._-]`, none empty,
/// `.` or `..`. That rules out absolute paths, `\`, and any climb out of a root.
pub(crate) fn check_id(id: &str) -> Result<()> {
    let plain = |part: &str| {
        !part.is_empty() && part != "." && part != ".." && part.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    };
    if id.split('/').all(plain) {
        Ok(())
    } else {
        Err(refuse("invalid id", format!("`{id}` is not a way id: expected `/`-separated parts of letters, digits, `.`, `_` and `-`")))
    }
}

/// The way's file and whether it is project-local, for an id that passed
/// [`check_id`], is on, and resolves to a file inside a ways root.
fn resolve(id: &str, project_dir: &str) -> Result<(PathBuf, bool)> {
    check_id(id)?;
    if let Some(why) = disabled_reason(id) {
        return Err(refuse("disabled", format!("way {id} is disabled: {why}")));
    }
    let Some((file, local)) = session::resolve_way_file(id, project_dir) else {
        return Err(refuse("not found", format!("no way named {id} in the project, user or core roots")));
    };
    if !inside_a_root(&file, project_dir) {
        return Err(refuse("outside roots", format!("way {id} resolves outside the ways roots")));
    }
    Ok((file, local))
}

/// Whether `file`, links resolved, sits under one of the ways roots, links resolved.
fn inside_a_root(file: &Path, project_dir: &str) -> bool {
    let Ok(file) = std::fs::canonicalize(file) else { return false };
    ways_core::paths::ways_roots(Some(Path::new(project_dir)))
        .iter()
        .filter_map(|r| std::fs::canonicalize(r).ok())
        .any(|root| file.starts_with(root))
}

/// Serve `id`'s body as injection renders it, changing no state. `session_id`,
/// when known, is the session whose scope a mismatch is reported against.
pub fn read(id: &str, session_id: Option<&str>) -> Result<Read> {
    let project_dir = crate::util::project_dir();
    let (way_file, is_project_local) = resolve(id, &project_dir)?;
    let content = std::fs::read_to_string(&way_file)?;
    let session_scope = session_id.map_or_else(|| "agent".to_string(), session::detect_scope);
    let field = crate::frontmatter::field_in(&content, "scope").unwrap_or_default();
    let note = (!session::scope_matches(&field, &session_scope)).then(|| {
        format!("this way is scoped to `{field}` and this session is `{session_scope}`, so injection would not deliver it here")
    });
    let run = MacroRun { session_id: session_id.unwrap_or(""), project_dir: &project_dir, scope: &session_scope };
    let body = render_way(&static_way_body(&content), &content, &way_file, is_project_local, &run);
    Ok(Read { body, scope: if field.is_empty() { "agent".into() } else { field }, note })
}

/// Record a pull of `id` as a disclosure to the agent this process serves
/// (`CLAUDE_AGENT_ID`, as every hook sets it) in `session_id`, and log it.
pub fn stamp(id: &str, session_id: &str) -> Result<()> {
    let project_dir = crate::util::project_dir();
    resolve(id, &project_dir)?;
    let domain = id.split('/').next().unwrap_or(id).to_string();
    let scope = session::detect_scope(session_id);

    let Ok(Some(fireable)) = fireable_in_scope(id, session_id, false) else {
        // A way with no resolvable `refire:` is never injected, so there is no
        // repeat to prevent. The pull is still logged.
        log_pulled(&Pulled { id, domain: &domain, session_id, project_dir: &project_dir, scope: &scope, tick: None, window: "none", out_of_band: false, epoch_distance: None, stamped: false, reason: Some("no refire curve") });
        return Ok(());
    };

    let tick = session::get_token_position(session_id);
    let decision = session::way_fire_outcome(id, session_id, &fireable.curve, tick);
    let out_of_band = decision.outcome == session::FireOutcome::Suppressed;
    let epoch_distance = out_of_band.then(|| session::get_epoch(session_id).saturating_sub(session::get_way_epoch(id, session_id)));
    let window = match decision.outcome {
        session::FireOutcome::FirstFire => "first_fire",
        session::FireOutcome::ReFire => "refire",
        session::FireOutcome::Suppressed => "suppressed",
    };

    let lock = session::lock_engagement(id, session_id);
    session::record_way_fire(id, session_id, &fireable.curve, tick);
    drop(lock);
    stamp_disclosure(id, session_id, tick);

    log_pulled(&Pulled {
        id,
        domain: &fireable.domain,
        session_id,
        project_dir: &fireable.project_dir,
        scope: &fireable.scope,
        tick: Some(tick),
        window,
        out_of_band,
        epoch_distance,
        stamped: true,
        reason: None,
    });
    Ok(())
}

/// The switch that turns `id` off, named for the operator, or `None` when the
/// way is on. A domain in the user's `disabled_domains` wins over a project toggle.
fn disabled_reason(id: &str) -> Option<String> {
    let domain = id.split('/').next().unwrap_or(id);
    if session::domain_disabled(domain) {
        return Some(format!("its domain `{domain}` is listed in `disabled_domains` in the user config"));
    }
    let key = crate::config::global().disabling_toggle(id)?;
    Some(format!("the project toggle `{key}: false` in `.claude/ways.yaml`"))
}

struct Pulled<'a> {
    id: &'a str,
    domain: &'a str,
    session_id: &'a str,
    project_dir: &'a str,
    scope: &'a str,
    tick: Option<u64>,
    /// `first_fire`, `refire` or `suppressed` as the firing engine classified the
    /// pull, `none` when it was not consulted.
    window: &'a str,
    out_of_band: bool,
    epoch_distance: Option<u64>,
    stamped: bool,
    /// Why nothing was stamped, when nothing was.
    reason: Option<&'a str>,
}

/// Log a pull twice: the `way_pulled` event, the debugging trail, and a
/// `kind: pull` decision record that joins the record of the turn the pull
/// happened in (ADR-701 §2).
fn log_pulled(p: &Pulled) {
    let agent_id = session::current_agent();
    session::log_decision(&pull_record(p, &agent_id, &agent_fmt::when::now_utc_iso()));
    let tick = p.tick.map(|t| t.to_string());
    let mut fields = vec![
        ("event", "way_pulled"),
        ("way", p.id),
        ("domain", p.domain),
        ("window", p.window),
        ("scope", p.scope),
        ("project", p.project_dir),
        ("session", p.session_id),
        ("agent_id", agent_id.as_str()),
    ];
    if let Some(t) = tick.as_deref() {
        fields.push(("token_position", t));
    }
    if let Some(r) = p.reason {
        fields.push(("reason", r));
    }
    let mut extra = vec![("out_of_band", json!(p.out_of_band)), ("stamped", json!(p.stamped))];
    if let Some(d) = p.epoch_distance {
        extra.push(("epoch_distance", json!(d)));
    }
    session::log_event_with(&fields, &extra);
}

/// The pull's decision record, a follow-up to its turn's scan record rather
/// than an update to it. `scan_id` is the one the calling agent's last-scan
/// marker names, null when it has none. The epoch is recorded as the pull saw
/// it: the command and file lanes bump it on every tool call, so it orders the
/// pull against the agent's other stamps but does not name its turn.
fn pull_record(p: &Pulled, agent: &str, ts: &str) -> serde_json::Value {
    let mut r = json!({
        "ts": ts,
        "kind": "pull",
        "session": p.session_id,
        "agent": agent,
        "epoch": session::get_epoch(p.session_id),
        "token_position": p.tick.unwrap_or_else(|| session::get_token_position(p.session_id)),
        "way": p.id,
        "window": p.window,
        "out_of_band": p.out_of_band,
        "stamped": p.stamped,
        "scan_id": session::read_last_scan(p.session_id),
    });
    if let Some(reason) = p.reason {
        r["reason"] = reason.into();
    }
    r
}

#[cfg(test)]
mod tests {
    use super::check_id;

    #[test]
    fn plain_way_paths_pass() {
        for id in ["a", "softwaredev/code/quality", "d/with-dash_and.dot", "d/v1.2"] {
            assert!(check_id(id).is_ok(), "{id}");
        }
    }

    #[test]
    fn anything_that_could_leave_a_root_or_name_nothing_fails() {
        for id in ["", "/", "/etc/passwd", "..", "../x", "a/../b", "a/./b", "./a", "a//b", "a/", "a\\b", "C:\\x", "a b", "a/\u{0}"] {
            assert!(check_id(id).is_err(), "{id:?}");
        }
    }
}
