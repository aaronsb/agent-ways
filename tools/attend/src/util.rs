//! Shared helpers reused across the `cmd::*` subcommand modules.
//!
//! These were free functions in the top of `main.rs` before the
//! dispatcher split (issue #51). Addressing-layer concerns — the
//! signals base path, project-name encoding, own-session resolution,
//! and the `Groups` builder that joins the two — collect here so every
//! command module can import them from a single place instead of
//! reaching into `main`.

use crate::groups;

pub(crate) fn signals_base() -> std::path::PathBuf {
    attend_presence::cache::signals_dir()
}

/// Where keepwarm's per-session arm file and ledger live (ADR-182).
/// Attend-owned state: the CLI verbs are the only sanctioned readers.
#[cfg(feature = "sensor-keepwarm")]
pub(crate) fn keepwarm_dir() -> std::path::PathBuf {
    attend_presence::cache::dir().join("keepwarm")
}

/// Claude Code's per-project data dir. A project is "live" iff its
/// encoded-cwd subdir exists here; message-tray lifetime is bound to it
/// (ADR-136) rather than to a wall-clock age.
pub(crate) fn projects_base() -> std::path::PathBuf {
    claude_sessions::ClaudeDir::user().projects_dir()
}

/// The name of a project's signal tray, `claude_sessions::attend_key`: the
/// project slug, so the project-liveness lookup that reaps trays reads the
/// project directory from it (ADR-136 Decision 3), and a hash of the full
/// path, so two projects with one slug never share a tray.
pub(crate) fn encode_project(path: &str) -> String {
    claude_sessions::attend_key(path)
}

/// Delegate to the canonical identity derivation (issue #378). No
/// longer gated on the sensor-peers feature — a minimal attend build
/// used to degrade own-identity to `pid-<pid>`, which polluted
/// `_groups.yaml` member ids.
pub(crate) fn own_session_id() -> Option<String> {
    attend_presence::session::find_own_session_id(std::process::id())
}

/// Whether session `ident`'s prompt cache has lapsed now, on keepwarm's
/// clock (ADR-406 item 1). An unresolved identity has no transcript to
/// read, which counts as lapsed.
pub(crate) fn cache_lapsed(ident: &Session) -> bool {
    !ident.resolved() || sensor_keepwarm::session_cache_lapsed(&ident.session_id, &ident.origin_path, sensor_trait::epoch_secs())
}

/// The ids session `ident` reads as: its own, then the ids it had before
/// each `/clear` (`enrollment::previous_ids`). A message sent to an earlier
/// id is still its to read, and a reply to one it sent under an earlier id
/// still wakes it (ADR-401, `to`). Before a verb that enrolls has moved the
/// enrollment to the new id, the earlier id is found as
/// [`adopt_previous_enrollment`] finds it, and nothing is moved.
pub(crate) fn reading_ids(ident: &Session) -> Vec<String> {
    use attend_presence::enrollment;
    let mut ids = vec![ident.session_id.clone()];
    ids.extend(enrollment::previous_ids(&ident.session_id));
    if ids.len() == 1 && enrollment::any_indexed() {
        if let Some(old) = ident.claude_key().and_then(|key| enrollment::previous_id(&key, &ident.session_id)) {
            ids.extend(enrollment::previous_ids(&old));
            ids.push(old);
        }
    }
    ids
}

/// The identity of the session this process runs in, as one value.
pub(crate) type Session = attend_presence::session::SessionIdentity;

/// This process's session, resolved now by process ancestry (ADR-171).
/// Not the memoized `attend_presence::session::identity()`: a library
/// caller outlives one verb, and `/clear` can give its session a new id
/// between two calls. A verb resolves this once at its start and hands the
/// same value to every helper, so one call never mixes two ids.
pub(crate) fn current_session() -> Session {
    attend_presence::session::identity_for_pid(std::process::id())
}

/// The channel membership view for session `me`.
pub(crate) fn groups_for(me: &Session) -> groups::Groups {
    groups::Groups::new(&signals_base(), &me.session_id)
}

/// The cwd this session is *about* — the session record's origin path
/// (issue #378), falling back to the process cwd only when no Claude
/// session owns this process. Every subcommand that means "my
/// project" (send identity, tray scan, status, registration) resolves
/// through here, so a stray shell `cd` can no longer put the session
/// on the bus as a different persona. A verb that already holds its
/// [`Session`] reads `origin_path` from it instead.
pub(crate) fn own_origin_cwd() -> String {
    current_session().origin_path
}

/// Move everything attend keeps under session id `old` to `new`, for a
/// session whose id changed under the same Claude Code process (`/clear`):
/// the seen-set and cold-start mark, the enrollment record, the registry
/// slot with its instance name, channel memberships, the last-inbound
/// record and the banner stamp. The old heartbeat is cleared. Idempotent:
/// a move already done, or done in part, completes.
pub(crate) fn move_session(old: &str, new: &str, origin: &str) {
    let old_store = attend_state::StateStore::new(Some(old.to_string()));
    if let Some(snapshot) = old_store.load() {
        attend_state::StateStore::new(Some(new.to_string())).checkpoint(&snapshot);
        old_store.clear();
    }
    attend_presence::enrollment::carry(old, new).ok();
    attend_instances::Registry::new().rename(origin, old, new).ok();
    groups::Groups::new(&signals_base(), new).rename_member(old, new);
    let state_dir = attend_presence::cache::state_dir();
    for ext in ["last-inbound", "banner"] {
        std::fs::rename(state_dir.join(format!("{old}.{ext}")), state_dir.join(format!("{new}.{ext}"))).ok();
    }
    attend_presence::heartbeat::clear(old).ok();
}

/// When session `ident` is not enrolled but its Claude Code process enrolled
/// under an earlier id (`/clear` gives the process a new one), move that
/// enrollment here with its seen-set and channels, as `attend run` would.
/// Returns whether `ident` is enrolled afterwards. The process key costs a
/// `ps` off Linux, so it is computed only when some enrollment was ever
/// recorded under a process key.
pub(crate) fn adopt_previous_enrollment(ident: &Session) -> bool {
    use attend_presence::enrollment;
    if enrollment::is_enrolled(&ident.session_id) {
        return true;
    }
    let previous = enrollment::any_indexed()
        .then(|| ident.claude_key())
        .flatten()
        .and_then(|key| enrollment::previous_id(&key, &ident.session_id));
    let Some(old) = previous else { return false };
    move_session(&old, &ident.session_id, &ident.origin_path);
    // The record moved with any pending opt-out; this applies it.
    enrollment::is_enrolled(&ident.session_id)
}

/// Enroll session `ident` by `source`: `attend listen on`, a channel join,
/// or `attend run`. A session that was not listening before has its cold
/// start reopened (`StateStore::reopen_cold_start`), and its enrollment
/// record notes when it started listening. Whichever way it listens again,
/// the next conduit delivers what was addressed to its project since its
/// Claude Code process started and what `#open` and its channels carried
/// since it started listening, and holds back the older backlog
/// (`attend_state::cold_start`).
/// An enrollment this Claude Code process made under its id before
/// `/clear` counts as listening, and moves here first. Returns whether the
/// session was listening before. Only a resolved session enrolls: the
/// drain never runs for any other. `host` records what hosts delivery (the
/// attend mod); `None` keeps the host already recorded.
pub(crate) fn enroll_as(
    ident: &Session,
    source: attend_presence::enrollment::Source,
    host: Option<attend_presence::enrollment::Host>,
) -> std::io::Result<bool> {
    let was = adopt_previous_enrollment(ident);
    attend_presence::enrollment::enroll(&ident.session_id, source, host)?;
    if !was {
        attend_state::StateStore::new(Some(ident.session_id.clone())).reopen_cold_start();
    }
    Ok(was)
}

/// Record that this session enrolled by joining a channel (#720).
pub(crate) fn enroll_by_join(ident: &Session) {
    if ident.resolved() {
        enroll_as(ident, attend_presence::enrollment::Source::Join, None).ok();
    }
}

/// Before this session joins channel `name`: mark the channel's old
/// messages seen, as the cold start does for every room (ADR-136
/// Decision 2, ADR-172's cold-start rule), so joining a channel late does
/// not deliver its history. Messages younger than the fresh window stay
/// live. A channel the session is already in is left alone: what it has
/// not consumed there is still owed to it. Returns how many were marked;
/// only a resolved session keeps a seen-set, so any other marks nothing.
pub(crate) fn baseline_joined_room(ident: &Session, groups: &groups::Groups, name: &str) -> usize {
    if !ident.resolved() || groups.my_groups().iter().any(|(n, _)| n == name) {
        return 0;
    }
    let backlog = attend_state::cold_start::room_backlog(&groups.group_dir(name));
    let n = backlog.len();
    attend_state::StateStore::new(Some(ident.session_id.clone())).mark_seen(backlog);
    n
}

/// Before this session joins channel `name` in answer to an invitation:
/// the room's history is a briefing, so it stays deliverable, the newest
/// [`attend_state::cold_start::ADDRESSED_MAX`] of it, as addressed mail
/// is. A session that has not had its cold start yet has it now, over the
/// rooms it already receives, so the first scan after the join is warm and
/// does not baseline the briefing away. Returns the line the join prints.
pub(crate) fn brief_invited_room(ident: &Session, groups: &groups::Groups, name: &str) -> String {
    if !ident.resolved() || groups.my_groups().iter().any(|(n, _)| n == name) {
        return String::new();
    }
    let store = attend_state::StateStore::new(Some(ident.session_id.clone()));
    let snapshot = store.load();
    let mut note = None;
    if !snapshot.as_ref().is_some_and(|s| s.baselined) {
        let seen = snapshot.map(|s| s.seen_signals).unwrap_or_default();
        let ids = reading_ids(ident);
        let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
        let scan = crate::cmd::inbox::scan_pending(&groups.receive_dirs(&ident.origin_path), &seen, &ids, true);
        let owed: std::collections::HashSet<String> = scan.delivered.iter().map(|d| attend_state::seen_key(&format!("{}.signal", d.id))).collect();
        store.baseline(scan.mark.into_iter().filter(|k| !owed.contains(k)));
        note = scan.note;
    }
    let dir = groups.group_dir(name);
    let held = attend_state::cold_start::room_beyond_newest(&dir, attend_state::cold_start::ADDRESSED_MAX);
    let total = std::fs::read_dir(&dir).map(|d| d.flatten().filter(|e| e.file_name().to_string_lossy().ends_with(".signal")).count()).unwrap_or(0);
    let held_n = held.len();
    store.mark_seen(held);
    let mut line = format!(" (invited: {} earlier message(s) to read", total - held_n);
    if held_n > 0 {
        line += &format!(", {held_n} older not shown");
    }
    line += ")";
    if let Some(n) = note {
        line += &format!("; {n}");
    }
    line
}

/// What a join prints after its line for the backlog it did not hand over.
pub(crate) fn backlog_note(n: usize) -> String {
    attend_state::cold_start::held_back_note(n).map(|note| format!(" ({note})")).unwrap_or_default()
}

/// The line an enrollment prints so session `me` learns the name peers see
/// it by: `you are <name>`, on its own line, in the Bash output the model
/// reads. `None` for an unresolved session, which peers do not see by name.
pub(crate) fn you_are_line(me: &Session) -> Option<String> {
    you_are(me).map(|name| format!("you are {name}"))
}

/// The name peers see session `me` by, or `None` for an unresolved session.
pub(crate) fn you_are(me: &Session) -> Option<String> {
    me.resolved().then(|| display_name(me))
}

/// The rendered display name: the origin path's nickname, plus the
/// instance suffix when several sessions share that origin. The name peers
/// see this session by; enrollment, the startup banner and the first drain
/// print it, and `attend status` names each run's owner by it.
pub(crate) fn display_name(ident: &Session) -> String {
    let nickname = agent_identity::Identity::for_cwd(&ident.origin_path, agent_theme::ColorDepth::detect()).nickname;
    let instance = attend_instances::Registry::new()
        .lookup(&ident.origin_path, &ident.session_id);
    join_display(nickname, instance.as_deref())
}

pub(crate) fn join_display(nickname: &str, instance: Option<&str>) -> String {
    match instance {
        Some(suffix) => format!("{nickname}-{suffix}"),
        None => nickname.to_string(),
    }
}

/// After this session's channels changed: a session in no channel any more
/// withdraws its join enrollment. One that also ran `attend run` stays
/// enrolled through that.
pub(crate) fn settle_join_enrollment(ident: &Session, groups: &groups::Groups) {
    if ident.resolved() && groups.my_groups().is_empty() {
        attend_presence::enrollment::withdraw(&ident.session_id, attend_presence::enrollment::Source::Join).ok();
    }
}

pub(crate) fn count_signals(dir: &std::path::Path) -> usize {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("signal"))
                .count()
        })
        .unwrap_or(0)
}

pub(crate) fn get_groups() -> groups::Groups {
    let session_id = own_session_id().unwrap_or_else(|| format!("pid-{}", std::process::id()));
    groups::Groups::new(&signals_base(), &session_id)
}
