//! A way pulled on request: `ways_read` (ADR-701 §5).
//!
//! Injection decides whether a way fires; a pull is the agent's own choice, so
//! it needs no match, no judge and no scope, and the refire window (ADR-126)
//! does not hold it back. The body is rendered as injection renders it. The
//! disclosure is stamped as injection stamps it, so the next prompt scan sees
//! the way as shown and does not repeat it. The disable switches still apply:
//! a way the operator turned off is refused, with the toggle named.

use anyhow::{bail, Result};
use serde_json::json;

use super::{fireable_in_scope, render_way, stamp_disclosure, static_way_body, MacroRun};
use crate::session;

/// What a pull delivers and how it sat with the way's refire window.
#[derive(Debug)]
pub struct Pulled {
    pub body: String,
    /// The way was inside its re-disclosure suppression window, where
    /// injection would have held it back.
    pub out_of_band: bool,
    /// Epochs since the way's last disclosure; set only when `out_of_band`.
    pub epoch_distance: Option<u64>,
    /// Disclosure was stamped. False without a session, and for a way whose
    /// refire curve cannot be resolved (injection refuses such a way, so there
    /// is no repeat to prevent); `stamp_note` says which.
    pub stamped: bool,
    pub stamp_note: Option<String>,
}

/// Pull `id`. `session_id` is `None` when the caller cannot name its session:
/// the body is served and nothing is stamped.
pub fn pull(id: &str, session_id: Option<&str>) -> Result<Pulled> {
    if let Some(why) = disabled_reason(id) {
        bail!("way {id} is disabled: {why}");
    }
    let project_dir = crate::util::project_dir();
    let Some((way_file, is_project_local)) = session::resolve_way_file(id, &project_dir) else {
        bail!("no way named {id} in the project, user or core roots");
    };
    let domain = id.split('/').next().unwrap_or(id).to_string();

    let Some(sid) = session_id else {
        let (body, content) = body_of(&way_file)?;
        let run = MacroRun { session_id: "", project_dir: &project_dir, scope: "agent" };
        let out = render_way(&body, &content, &way_file, is_project_local, &run);
        log_pulled(id, &domain, "", &project_dir, "agent", None, false, None, "no session id");
        return Ok(Pulled {
            body: out,
            out_of_band: false,
            epoch_distance: None,
            stamped: false,
            stamp_note: Some("no session id: the pull was not stamped, so injection may deliver this way again".into()),
        });
    };

    let fireable = match fireable_in_scope(id, sid, false) {
        Ok(Some(f)) => f,
        Ok(None) => bail!("no way named {id} in the project, user or core roots"),
        Err(e) => {
            // A way with no resolvable `refire:` is never injected, so there is
            // nothing to stamp against. Serve it.
            let (body, content) = body_of(&way_file)?;
            let scope = session::detect_scope(sid);
            let run = MacroRun { session_id: sid, project_dir: &project_dir, scope: &scope };
            let out = render_way(&body, &content, &way_file, is_project_local, &run);
            log_pulled(id, &domain, sid, &project_dir, &scope, None, false, None, "unstamped");
            return Ok(Pulled { body: out, out_of_band: false, epoch_distance: None, stamped: false, stamp_note: Some(e.to_string()) });
        }
    };

    let tick = session::get_token_position(sid);
    let decision = session::way_fire_outcome(id, sid, &fireable.curve, tick);
    let out_of_band = decision.outcome == session::FireOutcome::Suppressed;
    let epoch_distance = out_of_band.then(|| session::get_epoch(sid).saturating_sub(session::get_way_epoch(id, sid)));
    let window = match decision.outcome {
        session::FireOutcome::FirstFire => "first_fire",
        session::FireOutcome::ReFire => "refire",
        session::FireOutcome::Suppressed => "suppressed",
    };

    let body = static_way_body(&fireable.content);
    let run = MacroRun { session_id: sid, project_dir: &fireable.project_dir, scope: &fireable.scope };
    let out = render_way(&body, &fireable.content, &fireable.way_file, fireable.is_project_local, &run);

    let lock = session::lock_engagement(id, sid);
    session::record_way_fire(id, sid, &fireable.curve, tick);
    drop(lock);
    stamp_disclosure(id, sid, tick);

    log_pulled(id, &fireable.domain, sid, &fireable.project_dir, &fireable.scope, Some(tick), out_of_band, epoch_distance, window);
    Ok(Pulled { body: out, out_of_band, epoch_distance, stamped: true, stamp_note: None })
}

fn body_of(way_file: &std::path::Path) -> Result<(String, String)> {
    let content = std::fs::read_to_string(way_file)?;
    Ok((static_way_body(&content), content))
}

/// The switch that turns `id` off, named for the operator, or `None` when the
/// way is on. A domain in the user's `disabled_domains` wins over a project
/// toggle; among project toggles the most specific key is the one named.
pub(crate) fn disabled_reason(id: &str) -> Option<String> {
    let domain = id.split('/').next().unwrap_or(id);
    if session::domain_disabled(domain) {
        return Some(format!("its domain `{domain}` is listed in `disabled_domains` in the user config"));
    }
    if !session::way_disabled(id) {
        return None;
    }
    let key = crate::config::global()
        .disabled_ways()
        .iter()
        .filter(|k| match k.strip_suffix("/*") {
            Some(prefix) => id == prefix || id.starts_with(&format!("{prefix}/")),
            None => k.as_str() == id,
        })
        .max_by_key(|k| k.len())?;
    Some(format!("the project toggle `{key}: false` in `.claude/ways.yaml`"))
}

#[allow(clippy::too_many_arguments)]
fn log_pulled(
    id: &str,
    domain: &str,
    session_id: &str,
    project_dir: &str,
    scope: &str,
    tick: Option<u64>,
    out_of_band: bool,
    epoch_distance: Option<u64>,
    window: &str,
) {
    let agent_id = session::current_agent();
    let tick = tick.map(|t| t.to_string());
    let mut fields = vec![
        ("event", "way_pulled"),
        ("way", id),
        ("domain", domain),
        ("window", window),
        ("scope", scope),
        ("project", project_dir),
        ("session", session_id),
        ("agent_id", agent_id.as_str()),
    ];
    if let Some(t) = tick.as_deref() {
        fields.push(("token_position", t));
    }
    let mut extra = vec![("out_of_band", json!(out_of_band))];
    if let Some(d) = epoch_distance {
        extra.push(("epoch_distance", json!(d)));
    }
    session::log_event_with(&fields, &extra);
}
