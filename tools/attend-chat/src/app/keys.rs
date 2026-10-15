//! The chat's own key handlers: Enter (send, or run a slash command)
//! and Tab (complete). Each is a free function that takes the buffer,
//! cursor and cycle and returns the next state, so it can be tested
//! without a terminal; [`super::ChatPane`] applies the result. The
//! editing keys are the shared input's (`agent_tui::input`).

use agent_theme::ColorDepth;

use crate::chip::{known_of, resolve_identity, seen_identities, KnownIdentity, Seen};
use crate::groups::{channels, channels_in, live_peer_count, resolve_group_dir, BASE_CHANNEL_NAME};
use crate::legend::{
    apply_completion, find_trailing_mention, parse_addressed, Addressed, Sigil,
};
use crate::sessions::discover as discover_sessions;
use crate::signal::{
    broadcast_dir, compose_status_block, cwd_dir, parse_file, signals_base, write_broadcast, write_signal, Own, Signal,
};
use crate::tabs::{self, Tab};
use crate::slash;

/// Tab-completion cycle state.
///
/// First Tab on `@T<Tab>` inserts the first prefix-matching candidate
/// and stores the candidate list + sigil here. Each subsequent Tab
/// (with the buffer still ending in the previously-inserted token)
/// advances the index, wrapping at the end. Any other keystroke
/// implicitly resets the cycle: the Tab handler checks that the
/// buffer's tail still matches `candidates[index]`, and if it
/// doesn't, treats the press as a fresh completion start.
///
/// Same model applies to `#group<Tab>` cycling — the sigil
/// discriminates which pool to draw candidates from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TabCycle {
    pub sigil: Sigil,
    pub candidates: Vec<String>,
    pub index: usize,
}

/// What the screen does after `handle_enter` runs.
pub enum EnterAction {
    /// Empty input — no state change.
    None,
    /// Success path: set status, clear input + cursor.
    ClearWithStatus(String),
    /// Like [`EnterAction::ClearWithStatus`], but also append `echo` to the
    /// transcript: a local block never on the bus (`/peers` output), or
    /// what a send wrote, read back from where it went. The watcher reads
    /// those files too; the chat keeps one of each ([`Signal::same_file`]),
    /// so a send shows even when the watcher misses its event.
    ClearWithStatusAndEcho { status: String, echo: Vec<Signal> },
    /// Failure path: set status, leave input + cursor intact so the
    /// user can edit and retry.
    StatusOnly(String),
    /// A refusal because the agent named is quiet, though it may still
    /// read the message: like [`EnterAction::StatusOnly`], and the screen
    /// offers its send-now key for the draft as it stands.
    Quiet(String),
    /// `/clear` — Hide messages here on the foreground tab (ADR-405), clear
    /// input + cursor.
    HideHere,
    /// `/purge`, for one release the alias of Delete read messages for
    /// everyone… → All on this channel (ADR-405): the question opens at
    /// once and asks with what a dry run of `attend message delete` counts.
    DeleteRead(String),
    /// Success path that also moves the foreground tab — `/dissolve`
    /// closing the tab under the cursor advances focus (#393).
    ClearWithStatusAndFocus { status: String, focus: Tab },
}

/// Plain-Enter dispatch. Slash command first; otherwise parse the
/// `@`/`#` address sigil and write to the right inbox. `foreground`
/// is the active tab (#393): unaddressed sends target its channel,
/// and channel commands with no argument scope to it. `person` is the
/// membership id of the person at the chat, who a send to a channel makes
/// its member (ADR-404 D1); `None` for a chat that is no person, such as a
/// test's. `anyway` sends to an agent that is quiet but has not exited,
/// as the send-now key asks. Returns an `EnterAction` describing how the
/// screen should update its state.
pub fn handle_enter(input_value: &str, signals: &[Signal], foreground: &Tab, person: Option<&str>, anyway: bool) -> EnterAction {
    let msg = input_value.trim_end().to_string();
    if msg.is_empty() {
        return EnterAction::None;
    }
    // Slash-command interceptor — stops `/help`, `/whois`, etc.
    // from leaking onto the signal bus as plain messages. Runs
    // before the `@`/`#`/broadcast dispatch because slash syntax
    // is start-of-input only and unambiguous.
    if let Some((cmd, args)) = slash::parse(&msg) {
        return run_slash(slash::dispatch(cmd, args), foreground);
    }
    send_message(&msg, signals, foreground, person, anyway)
}

/// Run a dispatched slash command: the one path both the typed command
/// and the tab menu's items take.
pub fn run_slash(outcome: slash::SlashOutcome, foreground: &Tab) -> EnterAction {
    match outcome {
        slash::SlashOutcome::Ok(s) => EnterAction::ClearWithStatus(s),
        slash::SlashOutcome::Err(s) => EnterAction::StatusOnly(s),
        slash::SlashOutcome::HideHere => EnterAction::HideHere,
        slash::SlashOutcome::Join(name) => run_join(&name),
        slash::SlashOutcome::Leave(chan) => run_leave(chan, foreground),
        slash::SlashOutcome::Dissolve(chan) => run_dissolve(chan, foreground),
        slash::SlashOutcome::ListChannels => {
            channels_status_in(&signals_base(), attend_groups::member_alive)
        }
        slash::SlashOutcome::CreateChannel { name, description } => {
            create_channel_in(&signals_base(), &name, description.as_deref())
        }
        slash::SlashOutcome::DescribeChannel { name, description } => {
            describe_channel_in(&signals_base(), &name, &description)
        }
        slash::SlashOutcome::Purge(chan) => EnterAction::DeleteRead(tabs::command_scope(chan, foreground)),
        slash::SlashOutcome::Peers => run_peers(),
        slash::SlashOutcome::Whois(name) => run_whois(&name),
        slash::SlashOutcome::Invite { member, channel } => {
            run_invite(&member, channel, foreground)
        }
        slash::SlashOutcome::Kick { member, channel } => {
            run_kick(&member, channel, foreground)
        }
        slash::SlashOutcome::Config { key, value } => run_config(key, value),
    }
}

/// `/config`: list the chat's settings, or set one in attend's user file.
fn run_config(key: Option<String>, value: Option<String>) -> EnterAction {
    let dir = std::env::current_dir().unwrap_or_default();
    match (key, value) {
        (Some(k), Some(v)) => match crate::settings::set(&k, &v, &attend_config::user_path(), &dir) {
            Ok(s) => EnterAction::ClearWithStatus(s),
            Err(e) => EnterAction::StatusOnly(e),
        },
        _ => EnterAction::ClearWithStatus(crate::settings::listing(&dir)),
    }
}

/// Send `msg`: to the recipients its leading `@`/`#` run addresses, or
/// to the foreground tab's channel.
fn send_message(msg: &str, signals: &[Signal], foreground: &Tab, person: Option<&str>, anyway: bool) -> EnterAction {
    let msg = msg.to_string();
    let caps = ColorDepth::detect();
    let seeds = discover_sessions();
    // Local instance cache for this Enter-handler invocation.
    // Distinct from the per-render cache because key handlers are
    // not on the render path — they fire on demand and resolve
    // against the current registry state.
    let instance_cache = attend_instances::SnapshotCache::new();
    let alive = |id: &str| attend_presence::alive(id, &Default::default());
    let mut seen = seen_identities(signals, &seeds, caps, alive, |_| true, |_| false, &instance_cache);
    if anyway {
        wake_quiet(&mut seen);
    }
    let agents = known_of(&seen);
    let quiet = quiet_names(&seen);
    let gone = gone_names(&seen, &quiet);
    // The session behind a claude the chat knows: the id a direct message
    // to it is addressed to (ADR-401, `to`).
    let session_of = |k: &KnownIdentity| {
        seen.iter().find(|s| s.live && s.identity.is_claude && s.identity.nickname == k.nickname && s.identity.cwd == k.cwd).map(|s| s.member.clone())
    };
    // A message can address several recipients at once
    // (`@Tamsin @Cleo …`). Parse the whole leading run; an empty run
    // means "send to the foreground tab's channel" — `#open` from
    // merged (today's broadcast behavior), the tab's own channel
    // otherwise, with the destination flag (#392) showing the
    // resolution before Enter is pressed.
    let recipients = parse_addressed(&msg);
    // An agent's own DMs offer nothing to send: attend-chat has no send
    // into another agent's conversations. The view draws no compose box,
    // so this is a guard, not a path.
    if let Tab::Review(member) = foreground {
        let name = seen.iter().find(|s| s.member == *member).map_or_else(|| member.split('-').next().unwrap_or(member).to_string(), |s| s.identity.nickname.clone());
        return EnterAction::StatusOnly(format!("{name}'s own DMs are read only here; nothing sent"));
    }
    // In an agent's DM view an unaddressed message is a direct message to
    // that agent; an addressed one goes as it would from the operator view.
    if let (Tab::Dm(member), true) = (foreground, recipients.is_empty()) {
        return send_dm(&msg, member, &seen);
    }
    if recipients.is_empty() {
        let scope = tabs::send_scope(foreground);
        if scope == BASE_CHANNEL_NAME {
            match write_broadcast(&msg) {
                Ok(n) => sent(format!("sent: {n}"), read_back(&broadcast_dir(), &n).into_iter().collect()),
                Err(e) => EnterAction::StatusOnly(format!("send failed: {e}")),
            }
        } else {
            // Scoped send: same resolution + liveness gate as an
            // explicit `#g` prefix — only the channel came from the
            // tab instead of the message text. The body is unchanged
            // (wire format carries no channel).
            send_to_recipients(&msg, &[Addressed::Group(&scope)], &agents, (&quiet, &gone), &session_of, person, None)
        }
    } else if let (Tab::Channel(channel), true) = (foreground, recipients.iter().all(|r| matches!(r, Addressed::Agent(_)))) {
        // `@Name` while a channel is shown stays in that channel, addressed
        // to the agents it names; like any channel post it wakes the idle
        // members. Only the operator view makes it a direct message
        // (ADR-401, `to`).
        let to = match resolve_recipients(&recipients, &agents, (&quiet, &gone), &session_of) {
            Ok(dests) => dests.into_iter().map(|(_, to)| to).next().unwrap_or_default(),
            Err(refusal) => return refusal.action(),
        };
        // Only members read a named channel: a post addressed to an agent
        // outside it would reach no one, so it is refused and the draft
        // stays in the box.
        if channel != BASE_CHANNEL_NAME {
            let members = attend_groups::load_groups(&signals_base()).get(channel.as_str()).map(|e| e.members.clone()).unwrap_or_default();
            for r in &recipients {
                let Addressed::Agent(name) = r else { continue };
                let sid = resolve_recipients(&[Addressed::Agent(name)], &agents, (&quiet, &gone), &session_of).ok().and_then(|d| d.into_iter().next()).map(|(_, to)| to);
                if !sid.is_some_and(|sid| members.contains(&sid)) {
                    return EnterAction::StatusOnly(format!("@{name} is not in #{channel}; nothing sent"));
                }
            }
        }
        let names = recipient_labels(&recipients).join(" ");
        if channel == BASE_CHANNEL_NAME {
            match write_signal(&broadcast_dir(), &msg, &to) {
                Ok(n) => sent(format!("sent → #{BASE_CHANNEL_NAME} {names}"), read_back(&broadcast_dir(), &n).into_iter().collect()),
                Err(e) => EnterAction::StatusOnly(format!("send failed: {e}")),
            }
        } else {
            send_to_recipients(&msg, &[Addressed::Group(channel)], &agents, (&quiet, &gone), &session_of, person, Some(&to))
        }
    } else {
        send_to_recipients(&msg, &recipients, &agents, (&quiet, &gone), &session_of, person, None)
    }
}

/// A direct message to the claude whose session id is `member`, in its
/// project's tray, addressed to that session alone (ADR-401, `to`): what
/// Enter sends from its DM view. A quiet agent is refused, as an `@Name`
/// send to it is: one whose process has exited for good, one that may yet
/// read it until the send-now key sends it anyway.
fn send_dm(msg: &str, member: &str, seen: &[Seen]) -> EnterAction {
    let Some(s) = seen.iter().find(|s| s.member == member && s.identity.is_claude) else {
        return EnterAction::StatusOnly("that agent is not known to the chat any more; nothing sent".into());
    };
    let name = &s.identity.nickname;
    if !s.live {
        return if attend_presence::has_exited(member) { Refusal::exited(name) } else { Refusal::quiet(name) }.action();
    }
    let dir = cwd_dir(&s.identity.cwd);
    match write_signal(&dir, msg, member) {
        Ok(n) => sent(format!("sent → @{name}"), read_back(&dir, &n).into_iter().collect()),
        Err(e) => EnterAction::StatusOnly(format!("send failed: {e}")),
    }
}

/// The claudes the chat has seen that are quiet now, no heartbeat, by
/// name: those no live claude also answers to. Tab completes them and the
/// column lists them; a send to one is refused, and the destination flag
/// marks it.
pub(super) fn quiet_names(seen: &[Seen]) -> Vec<String> {
    let live = |n: &str| seen.iter().any(|s| s.live && s.identity.is_claude && s.identity.nickname.eq_ignore_ascii_case(n));
    seen.iter().filter(|s| s.identity.is_claude && !s.live && !live(&s.identity.nickname)).map(|s| s.identity.nickname.clone()).collect()
}

/// The quiet names in `quiet` whose every session has exited on this host
/// ([`attend_presence::has_exited`]): a send to one is refused for good,
/// whatever key sent it.
fn gone_names(seen: &[Seen], quiet: &[String]) -> Vec<String> {
    let exited = |n: &str| {
        seen.iter().filter(|s| s.identity.is_claude && !s.live && s.identity.nickname.eq_ignore_ascii_case(n)).all(|s| attend_presence::has_exited(&s.member))
    };
    quiet.iter().filter(|n| exited(n)).cloned().collect()
}

/// For the send-now key: a quiet agent that has not exited counts as live,
/// so the send reaches it as it would a live one. A name a live agent
/// answers to stays with the live one.
fn wake_quiet(seen: &mut [Seen]) {
    let quiet = quiet_names(seen);
    for s in seen.iter_mut() {
        if s.identity.is_claude && !s.live && quiet.iter().any(|q| q.eq_ignore_ascii_case(&s.identity.nickname)) && !attend_presence::has_exited(&s.member) {
            s.live = true;
        }
    }
}

/// Why a send was refused. `Quiet`: the agent named may still read it, and
/// the send-now key sends to it anyway. `Final`: anything else, in its own
/// words.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Refusal {
    Quiet(String),
    Final(String),
}

impl Refusal {
    fn quiet(name: &str) -> Self {
        Refusal::Quiet(name.to_string())
    }

    fn exited(name: &str) -> Self {
        Refusal::Final(format!("@{name} has exited and would not read this"))
    }

    fn action(self) -> EnterAction {
        match self {
            Refusal::Quiet(s) => EnterAction::Quiet(s),
            Refusal::Final(s) => EnterAction::StatusOnly(s),
        }
    }
}

/// The channels `_groups.yaml` still names whose directory was swept, by
/// name. Tab completes them and the column lists them; a send to one is
/// refused, and the destination flag marks it.
pub(super) fn swept_names(rows: &[crate::groups::ChannelRow]) -> Vec<String> {
    rows.iter().filter(|r| !r.on_disk).map(|r| r.known.group.name.clone()).collect()
}

/// Addressed-send core shared by the explicit `@`/`#` prefix path and
/// the foreground-tab scoped path. Resolves every recipient up front
/// and rejects the entire send if any address is bad, so one typo
/// doesn't half-deliver and the user can edit + retry the line.
fn send_to_recipients(
    msg: &str,
    recipients: &[Addressed<'_>],
    agents: &[KnownIdentity],
    quiet: (&[String], &[String]),
    session_of: &dyn Fn(&KnownIdentity) -> Option<String>,
    person: Option<&str>,
    addressed: Option<&str>,
) -> EnterAction {
    // A recipient that cannot read it refuses the send, in its own words.
    let dests = match resolve_recipients(recipients, agents, quiet, session_of) {
        Ok(dests) => dests,
        Err(refusal) => return refusal.action(),
    };
    // Posting in a channel makes the person its member, before the
    // message is written, so the membership precedes what an agent
    // answers (ADR-404 D1). A refused join does not hold the post back;
    // the status says it.
    let mut not_joined = Vec::new();
    for r in recipients {
        if let (Addressed::Group(name), Some(member)) = (r, person) {
            if let Err(e) = enter_channel(member, name) {
                not_joined.push((name, e));
            }
        }
    }
    // A channel post that names agents carries their ids in `to`.
    // Each copy written is read back: the channel's post, and each
    // project's direct message, which the chat takes as one send
    // ([`Signal::same_send`]).
    let written: Result<Vec<Option<Signal>>, _> = dests.iter().map(|(dir, to)| write_signal(dir, msg, addressed.unwrap_or(to)).map(|n| read_back(dir, &n))).collect();
    match written.map(|read| read.into_iter().flatten().collect::<Vec<_>>()) {
        Ok(read) => {
            let mut status = format!("sent → {}", recipient_labels(recipients).join(" "));
            // One channel needs no name in its note; several do.
            let named = recipients.iter().filter(|r| matches!(r, Addressed::Group(_))).count() > 1;
            for (name, e) in not_joined {
                status += &if named { format!(" (not joined #{name}: {e})") } else { format!(" (not joined: {e})") };
            }
            sent(status, read)
        }
        Err(e) => EnterAction::StatusOnly(format!("send failed: {e}")),
    }
}

/// Execute a validated `/join` against the live signals base as the
/// human member (ADR-170). Touches the username heartbeat first so
/// the join is immediately live — `live_peer_count` judges members by
/// heartbeat freshness, and waiting for the next 5s render tick would
/// leave a just-joined group looking dead to peer sends.
fn run_join(name: &str) -> EnterAction {
    let member = crate::signal::human_member_id();
    let _ = attend_presence::heartbeat::touch(&member);
    join_group_in(&signals_base(), &member, name)
}

/// How long a channel write from a key handler waits for another writer to
/// release `_groups.yaml` before it reports the channels busy.
const GROUPS_LOCK_WAIT: std::time::Duration = std::time::Duration::from_millis(500);

/// The group state as this screen writes it: each write waits at most
/// [`GROUPS_LOCK_WAIT`], so a stuck writer elsewhere cannot hang the key
/// handler, and a refused write reaches the status row as its error.
fn writing_groups(base: &std::path::Path, member: &str) -> attend_groups::Groups {
    attend_groups::Groups::new(base, member).waiting_at_most(GROUPS_LOCK_WAIT)
}

/// Make the person at the chat, `member`, a member of channel `name` when
/// they post in it (ADR-404 D1), as `/join` makes them, so an agent alone in
/// the channel with them can answer. Showing a channel joins nothing: the
/// tab bar is walked, cycled and advanced past channels nobody chose, and a
/// Leave must hold. The heartbeat is touched first, as `/join` does, so the
/// membership is live at once.
fn enter_channel(member: &str, name: &str) -> Result<bool, String> {
    let _ = attend_presence::heartbeat::touch(member);
    enter_channel_in(&signals_base(), member, name)
}

/// Filesystem-effect core of [`enter_channel`]. Only a channel that exists,
/// with its entry and its directory, is entered: a swept or unknown channel
/// is not brought back, as `/join` would. Returns whether it wrote a
/// membership, or the join's refusal.
fn enter_channel_in(base: &std::path::Path, member: &str, name: &str) -> Result<bool, String> {
    let groups = writing_groups(base, member);
    match groups.members(name) {
        Some(members) if !members.iter().any(|m| m == member) && groups.group_dir(name).is_dir() => {
            groups.join(name, false).map(|()| true)
        }
        _ => Ok(false),
    }
}

/// Execute a `/leave` against the live signals base. A missing
/// argument scopes to the foreground tab (#393); if that resolves to
/// the base channel, reject with the same message the explicit form
/// gets in dispatch — the tab default must not open a path around
/// the `#open` invariants.
fn run_leave(channel: Option<String>, foreground: &Tab) -> EnterAction {
    let name = tabs::command_scope(channel, foreground);
    if name == BASE_CHANNEL_NAME {
        return EnterAction::StatusOnly(
            "#open is the base channel — you can't leave it".into(),
        );
    }
    leave_group_in(&signals_base(), &crate::signal::human_member_id(), &name)
}

/// Filesystem-effect core of `/join`, parameterized on the base dir
/// so tests can drive it against a tempdir. Group-name validation
/// already happened in `slash::dispatch`; `Groups::join` re-validates
/// as defense in depth.
fn join_group_in(base: &std::path::Path, member: &str, name: &str) -> EnterAction {
    match writing_groups(base, member).join(name, false) {
        Ok(()) => EnterAction::ClearWithStatus(format!("joined #{name}")),
        Err(e) => EnterAction::StatusOnly(format!("/join failed: {e}")),
    }
}

/// Filesystem-effect core of `/leave`. Checks existence + membership
/// before writing so the user gets a precise status instead of a
/// silent no-op — `Groups::leave` itself succeeds on a group you were
/// never in.
fn leave_group_in(base: &std::path::Path, member: &str, name: &str) -> EnterAction {
    let groups = writing_groups(base, member);
    match groups.members(name) {
        None => EnterAction::StatusOnly(format!("#{name}: unknown group")),
        Some(members) if !members.iter().any(|m| m == member) => {
            EnterAction::StatusOnly(format!("#{name}: not a member"))
        }
        Some(_) => match groups.leave(name) {
            Ok(()) => EnterAction::ClearWithStatus(format!("left #{name}")),
            Err(e) => EnterAction::StatusOnly(format!("/leave failed: {e}")),
        },
    }
}

/// Execute a `/dissolve` against the live signals base. Scope
/// resolution mirrors `/leave`; on success the dissolve also closes
/// the tab — when the dissolved channel WAS the foreground, focus
/// advances to the tab now occupying its slot (floor `#open`, #393).
/// Other foregrounds keep their focus: dissolving a background tab
/// must not yank the operator out of the channel they're reading.
fn run_dissolve(channel: Option<String>, foreground: &Tab) -> EnterAction {
    let name = tabs::command_scope(channel, foreground);
    if name == BASE_CHANNEL_NAME {
        return EnterAction::StatusOnly(
            "#open is the base channel — you can't dissolve it".into(),
        );
    }
    let caps = ColorDepth::detect();
    let names_before = tabs::strip_names(&channels(caps));
    let me = crate::signal::human_member_id();
    match dissolve_group_in(&signals_base(), &name, &me, attend_groups::member_alive) {
        EnterAction::ClearWithStatus(status) => {
            let focus = if *foreground == Tab::Channel(name.clone()) {
                tabs::advance_after_dissolve(&name, &names_before)
            } else {
                foreground.clone()
            };
            EnterAction::ClearWithStatusAndFocus { status, focus }
        }
        other => other,
    }
}

/// Filesystem-effect core of `/dissolve`, parameterized on the base
/// dir and a liveness predicate so tests can drive both without a
/// real heartbeat sidecar.
///
/// The live-member guard is chat-side policy the CLI's
/// `attend channel dissolve` doesn't have: from the TUI this is a
/// channel-hygiene action, and yanking a group out from under peers
/// actively working in it shouldn't be one Enter away. A group whose
/// members are all heartbeat-stale — or an orphan `@dir` the yaml
/// has no entry for — dissolves freely; that's the clutter this
/// command exists to clear.
///
/// The operator running the command (`self_member`, ADR-170's username
/// member, kept heartbeat-fresh by the open chat) never counts as a live
/// member: the guard protects peers working in the channel, and without
/// the exclusion an operator could not dissolve any channel they had
/// joined. The empty-group send gate excludes the sender the same way.
fn dissolve_group_in<F: Fn(&str) -> bool>(
    base: &std::path::Path,
    name: &str,
    self_member: &str,
    is_live: F,
) -> EnterAction {
    // Member id is irrelevant for dissolve — it acts on the group,
    // not on this member's entry.
    let groups = writing_groups(base, "");
    let members = groups.members(name);
    let dir_exists = groups.group_dir(name).is_dir();
    if members.is_none() && !dir_exists {
        return EnterAction::StatusOnly(format!("#{name}: unknown group"));
    }
    let live = members
        .iter()
        .flatten()
        .filter(|m| m.as_str() != self_member && is_live(m))
        .count();
    if live > 0 {
        return EnterAction::StatusOnly(format!(
            "#{name}: {live} live member{} — not dissolving; /kick them first",
            if live == 1 { "" } else { "s" }
        ));
    }
    match groups.dissolve(name) {
        Ok(_) => EnterAction::ClearWithStatus(format!("dissolved #{name}")),
        Err(e) => EnterAction::StatusOnly(format!("/dissolve: {e}")),
    }
}

/// Filesystem-effect core of `/channels create` (#404), parameterized
/// on the base dir so tests can drive it against a tempdir. The
/// acting member id is irrelevant — create is a lifecycle verb, not a
/// membership change; `Groups::create` pins the channel so it
/// survives empty until joined or dissolved. Duplicate/reserved
/// errors pass through to the status row (#400).
fn create_channel_in(
    base: &std::path::Path,
    name: &str,
    description: Option<&str>,
) -> EnterAction {
    match writing_groups(base, "").create(name, description) {
        Ok(()) => EnterAction::ClearWithStatus(match description {
            Some(d) => format!("created #{name} — {d}"),
            None => format!("created #{name}"),
        }),
        Err(e) => EnterAction::StatusOnly(format!("/channels create: {e}")),
    }
}

/// Filesystem-effect core of `/channels describe` (#404). Empty text
/// clears the description — same contract as the CLI's `describe`.
/// `set_description` owns existence checking and its error strings.
fn describe_channel_in(base: &std::path::Path, name: &str, text: &str) -> EnterAction {
    match writing_groups(base, "").set_description(name, text) {
        Ok(()) if text.trim().is_empty() => {
            EnterAction::ClearWithStatus(format!("cleared #{name} description"))
        }
        Ok(()) => EnterAction::ClearWithStatus(format!("#{name} — {}", text.trim())),
        Err(e) => EnterAction::StatusOnly(format!("/channels describe: {e}")),
    }
}

/// `/channels` — one status line listing every channel with
/// live/total member counts, IRC `/list` shaped. Rides the same
/// discovery the channel bar uses (`channels_in`), so orphan dirs
/// show up here too — with `0/0 live`, which is the tell that
/// they're `/dissolve` fodder. Single-line on purpose: the status
/// row truncates on overflow rather than wrapping, and a channel
/// list long enough to truncate is a hygiene problem before it's a
/// rendering one.
fn channels_status_in<F: Fn(&str) -> bool>(
    base: &std::path::Path,
    is_live: F,
) -> EnterAction {
    let caps = ColorDepth::detect();
    let parts: Vec<String> = channels_in(base, caps)
        .iter()
        .map(|kg| {
            if kg.is_base {
                return format!("#{} (base)", kg.group.name);
            }
            let total = kg.membership.members.len();
            let live = kg.membership.members.iter().filter(|m| is_live(m)).count();
            let pin = if kg.membership.pinned { " pinned" } else { "" };
            // Description rides the same line (#404) — the listing is
            // the discovery surface, so "what is this channel for"
            // belongs next to "who's in it".
            let desc = kg
                .membership
                .description
                .as_deref()
                .map(|d| format!(" — {d}"))
                .unwrap_or_default();
            format!("#{} {live}/{total} live{pin}{desc}", kg.group.name)
        })
        .collect();
    EnterAction::ClearWithStatus(format!("channels: {}", parts.join(" · ")))
}

/// `/peers` — the live-session roster as a transcript status block
/// (ADR-173 Decision 5: operator discovery parity with agent-side
/// `attend peer list`). A block, not a status line: the roster carries a
/// root path per row and the status row truncates on overflow.
/// Roster enumeration is keyed by session id (issue #394), so one
/// session record renders exactly one row whatever its cwd history.
fn run_peers() -> EnterAction {
    let caps = ColorDepth::detect();
    let instance_cache = attend_instances::SnapshotCache::new();
    let roster = crate::peers::roster(caps, &instance_cache);
    if roster.is_empty() {
        return EnterAction::ClearWithStatus("no live peers".into());
    }
    let lines: Vec<String> = roster
        .iter()
        .map(|p| {
            // Short session-id tail for the block; `/whois` has the
            // full id for the row you actually care about.
            let sid_short: String = p.session_id.chars().take(8).collect();
            format!("@{}  {}  · session {}…", p.display, p.root, sid_short)
        })
        .collect();
    let n = roster.len();
    EnterAction::ClearWithStatusAndEcho {
        status: format!("{n} live peer{}", if n == 1 { "" } else { "s" }),
        echo: vec![compose_status_block("peers", &lines.join("\n"))],
    }
}

/// `/whois @name` — resolve a display name to its session id + origin
/// root. Exact-match only (see `peers::resolve_peer`); the send
/// path's fuzzy fallback stays out of identity queries.
fn run_whois(name: &str) -> EnterAction {
    let caps = ColorDepth::detect();
    let instance_cache = attend_instances::SnapshotCache::new();
    let roster = crate::peers::roster(caps, &instance_cache);
    match crate::peers::resolve_peer(name, &roster) {
        Ok(p) => EnterAction::ClearWithStatus(format!(
            "@{} — session {} · root {}",
            p.display, p.session_id, p.root
        )),
        Err(e) => EnterAction::StatusOnly(e),
    }
}

/// `/invite @name [#channel]` — invitation, never force-add. The
/// invitation is an ordinary directed signal to the peer's project
/// tray telling them how to join; entry happens only through their
/// own `attend channel join`, so the invitee's autonomy to decline (silence)
/// is preserved (ADR-173 / #393 consent asymmetry).
fn run_invite(member: &str, channel: Option<String>, foreground: &Tab) -> EnterAction {
    let g = tabs::command_scope(channel, foreground);
    // The base channel's rule fires before any roster read.
    if g == BASE_CHANNEL_NAME {
        return invite_peer(&g, &crate::peers::Peer { session_id: String::new(), root: String::new(), display: member.into() });
    }
    let caps = ColorDepth::detect();
    let instance_cache = attend_instances::SnapshotCache::new();
    let roster = crate::peers::roster(caps, &instance_cache);
    match crate::peers::resolve_peer(member, &roster) {
        Ok(peer) => invite_peer(&g, peer),
        Err(e) => EnterAction::StatusOnly(e),
    }
}

/// Invite `peer` to channel `g`: the one invitation `/invite` and a menu's
/// Add agent to chat both send, through attend-membership as every surface
/// invites. The peer joins itself, or stays silent. A peer invited already
/// is not told again.
pub fn invite_peer(g: &str, peer: &crate::peers::Peer) -> EnterAction {
    let bus = attend_membership::Bus::here().waiting_at_most(GROUPS_LOCK_WAIT);
    let target = attend_membership::Target { session_id: &peer.session_id, root: &peer.root };
    match attend_membership::invite(&bus, &crate::signal::actor(), g, target) {
        // The invitation shows as a direct message the person sent.
        Ok(attend_membership::Invited { told: Some(t) }) => {
            sent(format!("invited @{} to #{g}", peer.display), read_back(&t.dir, &t.filename).into_iter().collect())
        }
        Ok(attend_membership::Invited { told: None }) => {
            EnterAction::StatusOnly(format!("@{} is already invited to #{g}", peer.display))
        }
        Err(attend_membership::Refused::BaseChannel) => {
            EnterAction::StatusOnly("#open is the base channel — everyone is already there".into())
        }
        // Inviting into a channel that doesn't exist would hand the peer
        // a join command that mints a brand-new group — more likely a
        // typo than an intent. `/join` it first.
        Err(attend_membership::Refused::UnknownChannel(_)) => {
            EnterAction::StatusOnly(format!("#{g}: unknown channel — /join it first"))
        }
        Err(attend_membership::Refused::AlreadyMember(_)) => {
            EnterAction::StatusOnly(format!("@{} is already in #{g}", peer.display))
        }
        Err(e) => EnterAction::StatusOnly(format!("/invite failed: {e}")),
    }
}

/// `/kick @name [#channel]` — resolve the target, remove them, then
/// tell them. The directed notice is not a courtesy flourish: the
/// kicked session's routing changed without its participation, and
/// silent drift between its focus state and reality is exactly what
/// the notice prevents (#393). Best-effort like every signal write.
fn run_kick(member: &str, channel: Option<String>, foreground: &Tab) -> EnterAction {
    let g = tabs::command_scope(channel, foreground);
    if g == BASE_CHANNEL_NAME {
        return EnterAction::StatusOnly(
            "#open is the base channel — you can't kick from it".into(),
        );
    }
    let caps = ColorDepth::detect();
    let instance_cache = attend_instances::SnapshotCache::new();
    let roster = crate::peers::roster(caps, &instance_cache);
    let peer = match crate::peers::resolve_peer(member, &roster) {
        Ok(p) => p,
        Err(e) => return EnterAction::StatusOnly(e),
    };
    kick_member(&g, &peer.session_id, &peer.display, Some(&peer.root))
}

/// Remove the member `id` from channel `g`, named `label` on the status
/// line, live or not: the menus reach a quiet member through its
/// membership id, where `/kick @name` resolves only live peers. A claude
/// with a `root` is told in its inbox, which it reads when it next runs.
pub fn kick_member(g: &str, id: &str, label: &str, root: Option<&str>) -> EnterAction {
    if g == BASE_CHANNEL_NAME {
        return EnterAction::StatusOnly("#open is the base channel — you can't kick from it".into());
    }
    let action = kick_member_in(&signals_base(), g, id, label);
    if let (EnterAction::ClearWithStatus(_), Some(root)) = (&action, root) {
        let notice = format!(
            "you were removed from #{g} by the operator — rejoin with attend_join, or: attend channel join {g}"
        );
        let _ = write_signal(&cwd_dir(root), &notice, id);
    }
    action
}

/// Filesystem-effect core of `/kick`, parameterized on the base dir
/// so tests can drive it against a tempdir. `Groups::kick` owns the
/// state change and its precise error strings (unknown group,
/// non-member); the acting member id is irrelevant — kick targets
/// someone else's entry. `label` is the target's display name for
/// the status line (session ids don't belong in the status row).
fn kick_member_in(
    base: &std::path::Path,
    name: &str,
    member_id: &str,
    label: &str,
) -> EnterAction {
    match writing_groups(base, "").kick(name, member_id) {
        Ok(()) => EnterAction::ClearWithStatus(format!("kicked @{label} from #{name}")),
        Err(e) => EnterAction::StatusOnly(format!("/kick: {e}")),
    }
}

/// Resolved destination for the input box's lower-right flag (#392):
/// where will this buffer go when Enter is pressed? Reuses the send
/// path's own routing parse (`parse_addressed` + `recipient_labels`)
/// so the flag cannot disagree with what `handle_enter` does —
/// misrouted broadcast is the failure it exists to prevent. `None`
/// while a slash command is being composed (commands don't send).
/// `scope` is the channel an unaddressed message targets. An agent named
/// in `quiet` ([`quiet_names`]) and a channel named in `swept`
/// ([`swept_names`]) are marked: the send would be refused.
pub fn destination_label(input: &str, scope: &str, in_channel: bool, quiet: &[String], swept: &[String]) -> Option<String> {
    if slash::parse(input).is_some() || slash::find_slash_partial(input).is_some() {
        return None;
    }
    let recipients = parse_addressed(input);
    if recipients.is_empty() {
        return Some(format!("#{scope}"));
    }
    let labels: Vec<String> = recipients
        .iter()
        .zip(recipient_labels(&recipients))
        .map(|(r, label)| match r {
            Addressed::Agent(name) if quiet.iter().any(|q| q.eq_ignore_ascii_case(name)) => format!("{label} (quiet)"),
            Addressed::Group(name) if swept.iter().any(|s| s == name) => format!("{label} (swept)"),
            _ => label,
        })
        .collect();
    // `@Name` while a channel is shown is a post in that channel.
    if in_channel && recipients.iter().all(|r| matches!(r, Addressed::Agent(_))) {
        return Some(format!("#{scope} {}", labels.join(" ")));
    }
    Some(labels.join(" "))
}

/// The destination flag in the DM view of the agent `name`: the agent, for
/// an unaddressed message, marked when it is `quiet`; an addressed one as
/// the operator view flags it ([`destination_label`]).
pub fn dm_destination_label(input: &str, name: &str, quiet: bool, quiet_names: &[String], swept: &[String]) -> Option<String> {
    if parse_addressed(input).is_empty() && slash::parse(input).is_none() && slash::find_slash_partial(input).is_none() {
        return Some(if quiet { format!("@{name} (quiet)") } else { format!("@{name}") });
    }
    destination_label(input, BASE_CHANNEL_NAME, false, quiet_names, swept)
}

/// A send's success: `status`, and what it wrote, `read` back.
fn sent(status: String, read: Vec<Signal>) -> EnterAction {
    EnterAction::ClearWithStatusAndEcho { status, echo: read }
}

/// The signal just written to `dir` as `file`, read back as the watcher
/// would read it, against the chat's own trays.
fn read_back(dir: &std::path::Path, file: &str) -> Option<Signal> {
    parse_file(&dir.join(file), &Own::here())
}

/// Render the addressed recipients back as their `@name` / `#group`
/// labels for the "sent → …" status line.
fn recipient_labels(recipients: &[Addressed<'_>]) -> Vec<String> {
    recipients
        .iter()
        .map(|r| match r {
            Addressed::Agent(name) => format!("@{name}"),
            Addressed::Group(name) => format!("#{name}"),
        })
        .collect()
}

/// Resolve every addressed recipient to its destination inbox dir, or
/// return the first refusal, worded for the status line. All-or-nothing
/// on purpose: the
/// caller writes only when *every* recipient resolves, so a single bad
/// `@name`/`#group` rejects the whole multi-address send instead of
/// partially delivering. Adjacent or repeated addresses that resolve to
/// the same dir are deduped so a message is delivered once per inbox.
///
/// The empty-group rejection (ADR-129 follow-up) mirrors
/// `attend message send --channel` discipline: a `#groupname` send to a group
/// with zero live members would land in `@<name>/` and sit unread while
/// the chat reports "sent". The base channel (`#open`) bypasses the
/// check — it rides `_broadcast/`, scanned regardless of membership.
///
/// Tab completes quiet agents and swept channels, as the column lists
/// them, so a send to one says why it is refused: an agent named in
/// `quiet` has no heartbeat, which the send-now key overrides unless it is
/// also in `gone`, its process exited, and a channel whose directory was
/// swept has nobody live to read it until someone joins it again.
fn resolve_recipients(
    recipients: &[Addressed<'_>],
    agents: &[KnownIdentity],
    (quiet, gone): (&[String], &[String]),
    session_of: &dyn Fn(&KnownIdentity) -> Option<String>,
) -> Result<Vec<(std::path::PathBuf, String)>, Refusal> {
    let mut dests: Vec<(std::path::PathBuf, String)> = Vec::with_capacity(recipients.len());
    // Every agent addressed: each direct message carries the full set, as
    // RFC 5322 `To` does (ADR-401, `to`).
    let mut sessions: Vec<String> = Vec::new();
    // Self-exclusion for the liveness gate: since ADR-170 the human
    // is a heartbeating member, so without this a human alone in a
    // group would count themselves as the live listener — reopening
    // the silent-routing trap the gate exists to close.
    let self_member = crate::signal::human_member_id();
    for r in recipients {
        let (dir, to) = match r {
            Addressed::Agent(name) => {
                // An exact quiet name is refused before a fuzzy match could
                // carry it to a live agent with a name like it.
                let exact_live = agents.iter().filter(|k| k.is_claude && k.nickname.eq_ignore_ascii_case(name)).count();
                if let Some(q) = quiet.iter().find(|q| exact_live == 0 && q.eq_ignore_ascii_case(name)) {
                    return Err(if gone.contains(q) { Refusal::exited(q) } else { Refusal::quiet(q) });
                }
                // Two live sessions answering to one name: refused, as
                // `attend message send --to` refuses an ambiguous name (ADR-401).
                if exact_live > 1 {
                    return Err(Refusal::Final(format!("@{name} names {exact_live} live sessions; nothing sent")));
                }
                let k = resolve_identity(name, agents).ok_or_else(|| Refusal::Final(format!("@{name}: unknown nickname")))?;
                let sid = session_of(k).unwrap_or_default();
                if !sid.is_empty() && !sessions.contains(&sid) {
                    sessions.push(sid);
                }
                (cwd_dir(&k.cwd), None)
            }
            Addressed::Group(name) => match resolve_group_dir(name) {
                Some(dir) if live_peer_count(name, &self_member) > 0 => (dir, Some(format!("#{name}"))),
                Some(dir) if !dir.is_dir() => {
                    return Err(Refusal::Final(format!(
                        "#{name} was swept; /join #{name} and invite a live agent to bring it back, or send to #open"
                    )))
                }
                Some(_) => {
                    return Err(Refusal::Final(format!(
                        "#{name}: no live peers — message would not be read. \
                         Try #open to broadcast."
                    )))
                }
                None => return Err(Refusal::Final(format!("#{name}: unknown group"))),
            },
        };
        if !dests.iter().any(|(d, _)| *d == dir) {
            dests.push((dir, to.unwrap_or_default()));
        }
    }
    let direct = sessions.join(",");
    for (_, to) in dests.iter_mut().filter(|(_, to)| to.is_empty()) {
        to.clone_from(&direct);
    }
    Ok(dests)
}

/// Result of a Tab keypress. Always returns the next buffer +
/// cursor + cycle so the screen applies the result unconditionally,
/// with no "did anything change" branching at the call site.
pub struct TabResult {
    pub new_buf: String,
    pub new_cursor: usize,
    pub new_cycle: Option<TabCycle>,
}

/// Whether Tab in the compose box has a word to complete: Tab would change
/// the box's text, completing a command, a subcommand, an `@agent` or a
/// `#channel` over `agents` and `channels`, or moving the completion `cycle`
/// last made to its next candidate. A word nothing matches, or a cycle of
/// one, completes nothing, and Tab leaves the box instead ([`super::ix`]).
pub fn completable(buf: &str, cursor: usize, cycle: Option<&TabCycle>, agents: &[String], channels: &[String]) -> bool {
    handle_tab(buf, cursor, cycle.cloned(), agents, channels).new_buf != buf
}

/// Tab-completion + cycle advance. No-op when the caret is mid-buffer
/// (avoids surprising mid-sentence edits) or when no completion
/// applies. Slash completions short-circuit before `@`/`#` parsing.
/// An `@` completes over `agents` and a `#` over `channels`, in their
/// order: the names the left column lists (ADR-405), so Tab reaches a
/// quiet agent and a swept channel as a click does.
pub fn handle_tab(
    buf: &str,
    cursor: usize,
    current_cycle: Option<TabCycle>,
    agents: &[String],
    channels: &[String],
) -> TabResult {
    let unchanged = TabResult {
        new_buf: buf.to_string(),
        new_cursor: cursor,
        new_cycle: None,
    };

    if cursor != buf.chars().count() {
        return unchanged;
    }

    // Slash completion runs first — start-of-input-only grammar, so
    // when it matches the `@`/`#` logic below never applies. Silent
    // fall-through on no match so a stale `/` with no registry hit
    // doesn't trap the user.
    if let Some(partial) = slash::find_slash_partial(buf) {
        if let Some(hit) = slash::best_slash_completion(partial.partial) {
            let (next, new_cursor) = slash::apply_slash_completion(hit.name);
            return TabResult { new_buf: next, new_cursor, new_cycle: None };
        }
        // Slash and `@`/`#` share the cycle slot; resetting prevents
        // a stale agent cycle from carrying across a slash insertion.
        return unchanged;
    }

    // Subcommand completion (#405): past the name, at a grammar
    // subcommand choice, Tab completes the level's partial exactly
    // like the top level completes the name. Silent fall-through
    // otherwise — deeper levels (`@`/`#` tokens) keep the mention
    // completion below.
    if let Some((next, new_cursor)) = slash::complete_subcommand(buf) {
        return TabResult { new_buf: next, new_cursor, new_cycle: None };
    }

    // Cycling: if we have a stored cycle and the buffer still ends
    // with the candidate we just inserted (sigil + name + trailing
    // space, the exact form `apply_completion` produces), advance
    // to the next candidate.
    if let Some(c) = current_cycle {
        let sigil_char = match c.sigil {
            Sigil::Agent => '@',
            Sigil::Group => '#',
        };
        let expected_tail = format!("{}{} ", sigil_char, c.candidates[c.index]);
        if buf.ends_with(&expected_tail) && !c.candidates.is_empty() {
            let next_idx = (c.index + 1) % c.candidates.len();
            let next = &c.candidates[next_idx];
            // Splice the old tail off and append the new one. We
            // work in chars rather than bytes so multi-byte names
            // stay intact.
            let total_chars = buf.chars().count();
            let tail_chars = expected_tail.chars().count();
            let head: String = buf.chars().take(total_chars - tail_chars).collect();
            let new_buf = format!("{}{}{} ", head, sigil_char, next);
            let new_cursor = new_buf.chars().count();
            return TabResult {
                new_buf,
                new_cursor,
                new_cycle: Some(TabCycle { index: next_idx, ..c }),
            };
        }
    }

    // Fresh cycle: derive candidates from the current trailing mention.
    let Some(mention) = find_trailing_mention(buf) else {
        return unchanged;
    };
    let pool = match mention.sigil {
        Sigil::Agent => agents,
        Sigil::Group => channels,
    };
    let lc = mention.partial.to_ascii_lowercase();
    let candidates: Vec<String> = pool.iter().filter(|n| n.to_ascii_lowercase().starts_with(&lc)).cloned().collect();
    if candidates.is_empty() {
        return unchanged;
    }
    let first = candidates[0].clone();
    let (next_buf, new_cursor) = apply_completion(buf, &mention, &first);
    TabResult {
        new_buf: next_buf,
        new_cursor,
        new_cycle: Some(TabCycle {
            sigil: mention.sigil,
            candidates,
            index: 0,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tab has a word to complete only where it would change the box: the
    /// review of #972 found Tab dead on a cycle of one, an `@` or a `/` that
    /// matches nothing.
    #[test]
    fn tab_has_a_word_to_complete_only_where_it_changes_the_box() {
        let agents = ["alpha".to_string(), "alto".to_string()];
        let channels = ["open".to_string(), "deploy".to_string(), "archive".to_string()];
        let can = |buf: &str, cursor: usize, cycle: Option<&TabCycle>| completable(buf, cursor, cycle, &agents, &channels);
        assert!(can("/he", 3, None), "a command being typed");
        assert!(can("hi @al", 6, None), "an @agent in progress");
        assert!(can("#dep", 4, None), "a #channel in progress");
        let two = TabCycle { sigil: Sigil::Agent, candidates: vec!["alpha".into(), "alto".into()], index: 0 };
        assert!(can("hi @alpha ", 10, Some(&two)), "the completion Tab made: the next candidate");
        let one = TabCycle { sigil: Sigil::Group, candidates: vec!["archive".into()], index: 0 };
        assert!(can("#arch", 5, None), "#arch completes once");
        assert!(!can("#archive ", 9, Some(&one)), "a cycle of one: Tab would put back the same name");
        assert!(!can("hi @zzz", 7, None), "an @ nothing matches");
        assert!(!can("/zzz", 4, None), "a / nothing matches");
        assert!(!can("hi there", 8, None), "plain text");
        assert!(!can("", 0, None), "an empty box");
        assert!(!can("hi @alpha ", 10, None), "a finished mention");
        assert!(!can("hi @al", 3, None), "the caret mid-buffer");
        assert!(!can("mail a@b", 8, None), "an address, not a mention");
    }

    #[test]
    fn enter_empty_is_noop() {
        match handle_enter("   ", &[], &Tab::Merged, None, false) {
            EnterAction::None => {}
            _ => panic!("empty input should produce EnterAction::None"),
        }
    }

    #[test]
    fn foreground_scoped_commands_respect_base_channel_invariants() {
        // The tab default must not open a path around the `#open`
        // rules the explicit forms enforce in dispatch: a bare
        // /leave, /dissolve, /invite, or /kick whose foreground
        // resolves to the base channel gets the same friendly
        // rejection. Guards fire before any fs/roster IO.
        let open_tab = Tab::Channel("open".into());
        for tab in [&Tab::Merged, &open_tab] {
            match run_leave(None, tab) {
                EnterAction::StatusOnly(s) => assert!(s.contains("base channel"), "got: {s}"),
                _ => panic!("scoped /leave on base channel must reject"),
            }
            match run_dissolve(None, tab) {
                EnterAction::StatusOnly(s) => assert!(s.contains("base channel"), "got: {s}"),
                _ => panic!("scoped /dissolve on base channel must reject"),
            }
            match run_invite("Cleo", None, tab) {
                EnterAction::StatusOnly(s) => assert!(s.contains("base channel"), "got: {s}"),
                _ => panic!("scoped /invite on base channel must reject"),
            }
            match run_kick("Cleo", None, tab) {
                EnterAction::StatusOnly(s) => assert!(s.contains("base channel"), "got: {s}"),
                _ => panic!("scoped /kick on base channel must reject"),
            }
        }
    }

    fn tempdir_like() -> crate::test_dir::TestDir {
        crate::test_dir::unique("keys")
    }

    #[test]
    fn join_group_creates_membership_and_clears_input() {
        let base = tempdir_like();
        match join_group_in(&base, "aaron", "deploy") {
            EnterAction::ClearWithStatus(s) => assert!(s.contains("joined #deploy")),
            _ => panic!("successful join should clear input with status"),
        }
        assert!(base.join("@deploy").is_dir());
        let members = attend_groups::Groups::new(&base, "aaron")
            .members("deploy")
            .unwrap();
        assert_eq!(members, vec!["aaron"]);
    }

    #[test]
    fn a_join_while_another_writer_holds_the_channels_reports_busy() {
        let base = tempdir_like();
        let held = agent_settings::writer::Lock::acquire(&base.join("_groups.yaml")).unwrap();
        match join_group_in(&base, "aaron", "deploy") {
            EnterAction::StatusOnly(s) => assert!(s.contains(attend_groups::BUSY), "{s}"),
            _ => panic!("a join that waited out the lock should report it and keep the input"),
        }
        drop(held);
        assert!(attend_groups::Groups::new(&base, "aaron").members("deploy").is_none(), "nothing was written");
    }

    #[test]
    fn entering_joins_only_a_channel_that_exists() {
        let base = tempdir_like();
        assert_eq!(enter_channel_in(&base, "aaron", "ghost"), Ok(false), "an unknown channel is not made");
        assert!(!base.join("@ghost").exists());
        let agent = attend_groups::Groups::new(&base, "agent");
        agent.join("deploy", false).unwrap();
        agent.join("swept", false).unwrap();
        std::fs::remove_dir_all(base.join("@swept")).unwrap();
        assert_eq!(enter_channel_in(&base, "aaron", "swept"), Ok(false), "a swept channel is not brought back");
        assert!(!base.join("@swept").exists());
        assert_eq!(enter_channel_in(&base, "aaron", "deploy"), Ok(true));
        assert_eq!(enter_channel_in(&base, "aaron", "deploy"), Ok(false), "a member is not written twice");
        assert_eq!(agent.members("deploy").unwrap(), vec!["agent", "aaron"]);
    }

    #[test]
    fn leave_group_unknown_and_nonmember_are_precise() {
        let base = tempdir_like();
        // Unknown group: no yaml entry at all.
        match leave_group_in(&base, "aaron", "ghost") {
            EnterAction::StatusOnly(s) => assert!(s.contains("unknown group")),
            _ => panic!("unknown group should keep input with status"),
        }
        // Group exists, but we're not in it.
        attend_groups::Groups::new(&base, "someone-else")
            .join("deploy", false)
            .unwrap();
        match leave_group_in(&base, "aaron", "deploy") {
            EnterAction::StatusOnly(s) => assert!(s.contains("not a member")),
            _ => panic!("non-member leave should keep input with status"),
        }
    }

    #[test]
    fn join_then_leave_roundtrip_dissolves_unpinned() {
        let base = tempdir_like();
        join_group_in(&base, "aaron", "temp");
        match leave_group_in(&base, "aaron", "temp") {
            EnterAction::ClearWithStatus(s) => assert!(s.contains("left #temp")),
            _ => panic!("member leave should clear input with status"),
        }
        // Last member out of an unpinned group dissolves it.
        assert!(!base.join("@temp").exists());
    }

    #[test]
    fn dissolve_unknown_group_is_precise() {
        let base = tempdir_like();
        match dissolve_group_in(&base, "ghost", "aaron", |_| false) {
            EnterAction::StatusOnly(s) => assert!(s.contains("unknown group")),
            _ => panic!("unknown group should keep input with status"),
        }
    }

    #[test]
    fn dissolve_removes_orphan_dir_without_yaml_entry() {
        // The main quarry: a `@dir` on disk with no yaml entry —
        // invisible to member cleanup, rendered forever in the
        // channel bar until dissolved.
        let base = tempdir_like();
        std::fs::create_dir_all(base.join("@bg-test")).unwrap();
        match dissolve_group_in(&base, "bg-test", "aaron", |_| false) {
            EnterAction::ClearWithStatus(s) => assert!(s.contains("dissolved #bg-test")),
            _ => panic!("orphan dissolve should clear input with status"),
        }
        assert!(!base.join("@bg-test").exists());
    }

    #[test]
    fn dissolve_removes_group_with_only_stale_members() {
        let base = tempdir_like();
        attend_groups::Groups::new(&base, "dead-session")
            .join("temp", false)
            .unwrap();
        match dissolve_group_in(&base, "temp", "aaron", |_| false) {
            EnterAction::ClearWithStatus(s) => assert!(s.contains("dissolved #temp")),
            _ => panic!("stale-member dissolve should clear input with status"),
        }
        assert!(!base.join("@temp").exists());
        assert!(attend_groups::Groups::new(&base, "x").members("temp").is_none());
    }

    #[test]
    fn dissolve_refuses_group_with_live_members() {
        let base = tempdir_like();
        attend_groups::Groups::new(&base, "live-session")
            .join("deploy", false)
            .unwrap();
        match dissolve_group_in(&base, "deploy", "aaron", |_| true) {
            EnterAction::StatusOnly(s) => assert!(s.contains("1 live member")),
            _ => panic!("live-member dissolve should refuse with status"),
        }
        // Nothing was touched.
        assert!(base.join("@deploy").is_dir());
    }

    #[test]
    fn dissolve_does_not_count_the_operator_as_a_live_member() {
        // The operator joined the channel from the chat, whose heartbeat
        // keeps them live: their own membership must not hold the
        // channel open against them. A live peer still does.
        let base = tempdir_like();
        join_group_in(&base, "aaron", "kwin-canvas");
        match dissolve_group_in(&base, "kwin-canvas", "aaron", |_| true) {
            EnterAction::ClearWithStatus(s) => assert!(s.contains("dissolved #kwin-canvas"), "got: {s}"),
            _ => panic!("the operator alone in a channel should dissolve it"),
        }
        assert!(!base.join("@kwin-canvas").exists());
        assert!(attend_groups::Groups::new(&base, "x").members("kwin-canvas").is_none());

        join_group_in(&base, "aaron", "testing");
        join_group_in(&base, "live-session", "testing");
        match dissolve_group_in(&base, "testing", "aaron", |_| true) {
            EnterAction::StatusOnly(s) => assert!(s.contains("1 live member") && s.contains("/kick"), "got: {s}"),
            _ => panic!("a live peer still holds the channel"),
        }
    }

    #[test]
    fn kick_removes_member_and_cleans_empty_unpinned_group() {
        let base = tempdir_like();
        attend_groups::Groups::new(&base, "sess-a")
            .join("deploy", false)
            .unwrap();
        match kick_member_in(&base, "deploy", "sess-a", "Tamsin-alpha") {
            EnterAction::ClearWithStatus(s) => {
                assert!(s.contains("kicked @Tamsin-alpha from #deploy"), "got: {s}")
            }
            _ => panic!("successful kick should clear input with status"),
        }
        // Last member out of an unpinned group dissolves it — same
        // cleanup rule as leave.
        assert!(!base.join("@deploy").exists());
    }

    #[test]
    fn kick_unknown_group_and_nonmember_surface_kick_errors() {
        let base = tempdir_like();
        // Unknown group: Groups::kick's own error string passes through.
        match kick_member_in(&base, "ghost", "sess-a", "Cleo") {
            EnterAction::StatusOnly(s) => assert!(s.contains("unknown group"), "got: {s}"),
            _ => panic!("unknown group should keep input with status"),
        }
        // Group exists, target isn't in it.
        attend_groups::Groups::new(&base, "someone-else")
            .join("deploy", false)
            .unwrap();
        match kick_member_in(&base, "deploy", "sess-a", "Cleo") {
            EnterAction::StatusOnly(s) => assert!(s.contains("not a member"), "got: {s}"),
            _ => panic!("non-member kick should keep input with status"),
        }
        // The bystander's membership is untouched.
        let members = attend_groups::Groups::new(&base, "x").members("deploy").unwrap();
        assert_eq!(members, vec!["someone-else"]);
    }

    /// #406: the agent→human kick guard lives in the shared crate
    /// (`Groups::kick` refuses when the ACTING id is agent-shaped and
    /// the target is human). The TUI acts with an empty member id,
    /// which classifies as human — so the guard can never fire here
    /// and operator kicks of human members keep working. If the
    /// acting identity ever changes to an agent shape, this test
    /// fails before the regression ships; and if the guard did fire,
    /// `kick_member_in`'s Err arm surfaces its message through the
    /// #400 status path rather than swallowing it.
    #[test]
    fn tui_kick_acts_as_operator_and_can_remove_human_members() {
        let base = tempdir_like();
        assert!(
            !attend_groups::is_agent_member(""),
            "TUI acting id must classify as human (operator power)"
        );
        // A human member (username shape, ADR-170) is kickable from
        // the TUI — the one-way power the guard preserves.
        attend_groups::Groups::new(&base, "somebody")
            .join("ops", false)
            .unwrap();
        match kick_member_in(&base, "ops", "somebody", "somebody") {
            EnterAction::ClearWithStatus(s) => {
                assert!(s.contains("kicked @somebody from #ops"), "got: {s}")
            }
            _ => panic!("operator kick of a human member must succeed"),
        }
    }

    #[test]
    fn channels_status_lists_base_and_counts() {
        let base = tempdir_like();
        // One real group with a dead member, one orphan dir.
        attend_groups::Groups::new(&base, "dead-sess")
            .join("deploy", false)
            .unwrap();
        std::fs::create_dir_all(base.join("@bg-test")).unwrap();
        match channels_status_in(&base, |_| false) {
            EnterAction::ClearWithStatus(s) => {
                assert!(s.contains("#open (base)"), "got: {s}");
                assert!(s.contains("#deploy 0/1 live"), "got: {s}");
                assert!(s.contains("#bg-test 0/0 live"), "got: {s}");
            }
            _ => panic!("/channels should clear input with status"),
        }
    }

    #[test]
    fn channels_status_shows_descriptions() {
        let base = tempdir_like();
        create_channel_in(&base, "plans", Some("roadmap talk"));
        match channels_status_in(&base, |_| false) {
            EnterAction::ClearWithStatus(s) => {
                assert!(s.contains("#plans 0/0 live pinned — roadmap talk"), "got: {s}");
            }
            _ => panic!("/channels should clear input with status"),
        }
    }

    #[test]
    fn create_channel_pins_and_reports_duplicates() {
        let base = tempdir_like();
        match create_channel_in(&base, "plans", Some("roadmap talk")) {
            EnterAction::ClearWithStatus(s) => {
                assert!(s.contains("created #plans — roadmap talk"), "got: {s}")
            }
            _ => panic!("successful create should clear input with status"),
        }
        // Pinned, empty, dir present — the explicit-lifecycle contract.
        let entry = &attend_groups::load_groups(&base)["plans"];
        assert!(entry.pinned, "explicit create must survive empty");
        assert!(entry.members.is_empty());
        assert!(base.join("@plans").is_dir());
        // Duplicate: Groups::create's error passes through the #400
        // error path (input kept for retry).
        match create_channel_in(&base, "plans", None) {
            EnterAction::StatusOnly(s) => {
                assert!(s.contains("already exists"), "got: {s}")
            }
            _ => panic!("duplicate create should keep input with status"),
        }
    }

    #[test]
    fn describe_channel_sets_clears_and_rejects_unknown() {
        let base = tempdir_like();
        create_channel_in(&base, "plans", None);
        match describe_channel_in(&base, "plans", "roadmap talk") {
            EnterAction::ClearWithStatus(s) => {
                assert!(s.contains("#plans — roadmap talk"), "got: {s}")
            }
            _ => panic!("successful describe should clear input with status"),
        }
        assert_eq!(
            attend_groups::load_groups(&base)["plans"].description.as_deref(),
            Some("roadmap talk")
        );
        // Empty text clears — CLI parity.
        match describe_channel_in(&base, "plans", "") {
            EnterAction::ClearWithStatus(s) => {
                assert!(s.contains("cleared #plans description"), "got: {s}")
            }
            _ => panic!("clearing describe should clear input with status"),
        }
        assert_eq!(attend_groups::load_groups(&base)["plans"].description, None);
        // Unknown channel: set_description's error passes through.
        match describe_channel_in(&base, "ghost", "d") {
            EnterAction::StatusOnly(s) => {
                assert!(s.contains("unknown group"), "got: {s}")
            }
            _ => panic!("unknown channel should keep input with status"),
        }
    }

    #[test]
    fn destination_label_tracks_routing_live() {
        // Unaddressed → the scope channel.
        let destination_label = |i: &str, s: &str| destination_label(i, s, false, &[], &[]);
        assert_eq!(destination_label("", "open"), Some("#open".into()));
        assert_eq!(destination_label("hello", "open"), Some("#open".into()));
        assert_eq!(destination_label("hello", "deploy"), Some("#deploy".into()));
        // Leading address run wins, exactly as `handle_enter` routes.
        assert_eq!(destination_label("@Tamsin hi", "open"), Some("@Tamsin".into()));
        assert_eq!(destination_label("#infra hi", "open"), Some("#infra".into()));
        assert_eq!(
            destination_label("@Tamsin @Cleo sync", "open"),
            Some("@Tamsin @Cleo".into())
        );
        // A mid-message mention doesn't address — scope still shown.
        assert_eq!(destination_label("hi @Tamsin", "open"), Some("#open".into()));
        // `@Name` while a channel is shown posts in that channel, to them.
        assert_eq!(super::destination_label("@Tamsin hi", "deploy", true, &[], &[]), Some("#deploy @Tamsin".into()));
        assert_eq!(super::destination_label("@Tamsin hi", "open", true, &[], &[]), Some("#open @Tamsin".into()));
    }

    #[test]
    fn the_destination_flag_marks_a_quiet_agent() {
        let quiet = ["Lark".to_string()];
        assert_eq!(destination_label("@lark hi", "open", false, &quiet, &[]), Some("@lark (quiet)".into()));
        assert_eq!(destination_label("@Tamsin @Lark hi", "open", false, &quiet, &[]), Some("@Tamsin @Lark (quiet)".into()));
        assert_eq!(destination_label("hi", "open", false, &quiet, &[]), Some("#open".into()));
    }

    #[test]
    fn the_destination_flag_marks_a_swept_channel() {
        let swept = ["ghost".to_string()];
        assert_eq!(destination_label("#ghost hi", "open", false, &[], &swept), Some("#ghost (swept)".into()));
        assert_eq!(destination_label("#infra #ghost hi", "open", false, &[], &swept), Some("#infra #ghost (swept)".into()));
        assert_eq!(destination_label("#ghostly hi", "open", false, &[], &swept), Some("#ghostly".into()), "the name, not a prefix of it");
    }

    fn ident(name: &str, cwd: &str) -> KnownIdentity {
        let id = agent_identity::Identity::for_cwd(cwd, ColorDepth::Ansi16);
        KnownIdentity { nickname: name.into(), cwd: cwd.into(), is_claude: true, palette: id.palette, style: id.style }
    }

    #[test]
    fn a_send_to_a_quiet_agent_or_a_swept_channel_is_refused_in_its_own_words() {
        let live = [ident("Larks", "/w/larks")];
        let quiet = ["Lark".to_string()];
        let to = |r: Addressed<'_>| resolve_recipients(&[r], &live, (&quiet, &[]), &|_| None);
        assert_eq!(to(Addressed::Agent("Lark")), Err(Refusal::Quiet("Lark".into())), "not carried to Larks by the fuzzy match");
        let exited = resolve_recipients(&[Addressed::Agent("Lark")], &live, (&quiet, &quiet), &|_| None);
        assert_eq!(exited, Err(Refusal::Final("@Lark has exited and would not read this".into())), "an exited agent is refused for good");
        assert!(to(Addressed::Agent("Larks")).is_ok(), "a live agent is sent to");
        assert_eq!(to(Addressed::Agent("Nobody")), Err(Refusal::Final("@Nobody: unknown nickname".into())));
        let twins = [ident("Larks", "/w/larks"), ident("Larks", "/w/larks")];
        assert_eq!(
            resolve_recipients(&[Addressed::Agent("Larks")], &twins, (&[], &[]), &|_| None),
            Err(Refusal::Final("@Larks names 2 live sessions; nothing sent".into())),
            "two live sessions of one name are refused"
        );
        let seen = |name: &str, live: bool| Seen { identity: ident(name, &format!("/w/{name}")), live, member: name.into() };
        assert_eq!(quiet_names(&[seen("Lark", false), seen("Wells", true), seen("Wells", false)]), ["Lark"], "a name a live agent answers to is not quiet");
    }

    #[test]
    fn destination_label_hides_while_composing_slash() {
        let destination_label = |i: &str, s: &str| destination_label(i, s, false, &[], &[]);
        assert_eq!(destination_label("/", "open"), None);
        assert_eq!(destination_label("/jo", "open"), None);
        assert_eq!(destination_label("/join #deploy", "open"), None);
        // A mid-sentence slash is a literal slash — still a send.
        assert_eq!(destination_label("a / b", "open"), Some("#open".into()));
    }

    #[test]
    fn tab_mid_buffer_is_noop_and_resets_cycle() {
        let res = handle_tab("@foo bar", 3, None, &[], &[]);
        assert_eq!(res.new_buf, "@foo bar");
        assert_eq!(res.new_cursor, 3);
        assert!(res.new_cycle.is_none());
    }

    #[test]
    fn tab_completes_over_the_pools_it_is_given_in_their_order() {
        let agents = ["Laszlo-alpha".to_string(), "Quinn".into(), "Lark".into()];
        let channels = ["open".to_string(), "deploy".into(), "swept".into()];
        let r = handle_tab("hi @l", 5, None, &agents, &channels);
        assert_eq!(r.new_buf, "hi @Laszlo-alpha ");
        let r = handle_tab(&r.new_buf, r.new_buf.chars().count(), r.new_cycle, &agents, &channels);
        assert_eq!(r.new_buf, "hi @Lark ", "the next match, a quiet agent the column lists");
        let r = handle_tab("#sw", 3, None, &agents, &channels);
        assert_eq!(r.new_buf, "#swept ", "a channel only _groups.yaml names");
        let r = handle_tab("@zz", 3, None, &agents, &channels);
        assert_eq!(r.new_buf, "@zz", "nothing listed, nothing completed");
    }
}
