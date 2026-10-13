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
    compose_self_echo, compose_status_block, cwd_dir, encode_cwd, signals_base, write_broadcast,
    write_signal, Signal,
};
use crate::tabs::{self, Tab};
use crate::watcher::accept_path;
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
    /// Directed-send success: like [`EnterAction::ClearWithStatus`], but
    /// also append `echo` to the transcript. Directed (`@name`) sends
    /// land in the recipient's cwd inbox, which the sender does not
    /// watch, so — unlike broadcasts — they never round-trip back into
    /// the sender's own view. The screen appends `echo` so the sender
    /// sees their own message land.
    ClearWithStatusAndEcho { status: String, echo: Signal },
    /// Failure path: set status, leave input + cursor intact so the
    /// user can edit and retry.
    StatusOnly(String),
    /// `/clear` — empty the transcript buffer, clear input + cursor,
    /// show status.
    ClearTranscript,
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
/// test's. Returns an `EnterAction` describing how the screen should
/// update its state.
pub fn handle_enter(input_value: &str, signals: &[Signal], foreground: &Tab, person: Option<&str>) -> EnterAction {
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
    send_message(&msg, signals, foreground, person)
}

/// Run a dispatched slash command: the one path both the typed command
/// and the tab menu's items take.
pub fn run_slash(outcome: slash::SlashOutcome, foreground: &Tab) -> EnterAction {
    match outcome {
        slash::SlashOutcome::Ok(s) => EnterAction::ClearWithStatus(s),
        slash::SlashOutcome::Err(s) => EnterAction::StatusOnly(s),
        slash::SlashOutcome::ClearTranscript => EnterAction::ClearTranscript,
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
        slash::SlashOutcome::Purge(chan) => run_purge(chan, foreground),
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
fn send_message(msg: &str, signals: &[Signal], foreground: &Tab, person: Option<&str>) -> EnterAction {
    let msg = msg.to_string();
    let caps = ColorDepth::detect();
    let seeds = discover_sessions();
    // Local instance cache for this Enter-handler invocation.
    // Distinct from the per-render cache because key handlers are
    // not on the render path — they fire on demand and resolve
    // against the current registry state.
    let instance_cache = attend_instances::SnapshotCache::new();
    let alive = |id: &str| attend_presence::alive(id, &Default::default());
    let seen = seen_identities(signals, &seeds, caps, alive, |_| true, |_| false, &instance_cache);
    let agents = known_of(&seen);
    let quiet = quiet_names(&seen);
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
    if recipients.is_empty() {
        let scope = tabs::send_scope(foreground);
        if scope == BASE_CHANNEL_NAME {
            // Broadcast rides `_broadcast/`, which the sender's own
            // watcher surfaces — so it self-echoes through the normal
            // signal path and needs no synthetic echo here.
            match write_broadcast(&msg) {
                Ok(n) => EnterAction::ClearWithStatus(format!("sent: {n}")),
                Err(e) => EnterAction::StatusOnly(format!("send failed: {e}")),
            }
        } else {
            // Scoped send: same resolution + liveness gate as an
            // explicit `#g` prefix — only the channel came from the
            // tab instead of the message text. The body is unchanged
            // (wire format carries no channel).
            send_to_recipients(&msg, &[Addressed::Group(&scope)], &agents, &quiet, &session_of, person)
        }
    } else {
        send_to_recipients(&msg, &recipients, &agents, &quiet, &session_of, person)
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
    quiet: &[String],
    session_of: &dyn Fn(&KnownIdentity) -> Option<String>,
    person: Option<&str>,
) -> EnterAction {
    // A recipient that cannot read it refuses the send, in its own words.
    let dests = match resolve_recipients(recipients, agents, quiet, session_of) {
        Ok(dests) => dests,
        Err(refusal) => return EnterAction::StatusOnly(refusal),
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
    let sent = dests.iter().try_for_each(|(dir, to)| write_signal(dir, msg, to).map(|_| ())).map(|()| dests);
    match sent {
        Ok(dests) => {
            let mut status = format!("sent → {}", recipient_labels(recipients).join(" "));
            // One channel needs no name in its note; several do.
            let named = recipients.iter().filter(|r| matches!(r, Addressed::Group(_))).count() > 1;
            for (name, e) in not_joined {
                status += &if named { format!(" (not joined #{name}: {e})") } else { format!(" (not joined: {e})") };
            }
            // Echo only when the message reaches NO inbox the sender's
            // own watcher already surfaces — otherwise it round-trips
            // and a synthetic echo would double-render it. `#open`
            // lands in `_broadcast/` and `#group` in `@group/` (both
            // watched); an `@agent` in our *own* cwd lands in
            // `own_encoded` (watched). Only `@agent` sends to *other*
            // cwds are invisible without an echo.
            if dests.iter().any(|(d, _)| sender_watches(d)) {
                EnterAction::ClearWithStatus(status)
            } else {
                EnterAction::ClearWithStatusAndEcho {
                    status,
                    echo: compose_self_echo(msg),
                }
            }
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
/// `attend dissolve` doesn't have: from the TUI this is a
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
/// `attend peers`). A block, not a status line: the roster carries a
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
        echo: compose_status_block("peers", &lines.join("\n")),
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
/// own `attend join`, so the invitee's autonomy to decline (silence)
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
/// Add agent to chat both send. The peer joins itself, or stays silent.
pub fn invite_peer(g: &str, peer: &crate::peers::Peer) -> EnterAction {
    let g = g.to_string();
    if g == BASE_CHANNEL_NAME {
        return EnterAction::StatusOnly(
            "#open is the base channel — everyone is already there".into(),
        );
    }
    // Inviting into a channel that doesn't exist would hand the peer
    // a join command that mints a brand-new group — more likely a
    // typo than an intent. `/join` it first.
    let Some(dir) = resolve_group_dir(&g) else {
        return EnterAction::StatusOnly(format!("#{g}: unknown channel — /join it first"));
    };
    let dest = cwd_dir(&peer.root);
    let body = invite_body(&g, &dir);
    match write_signal(&dest, &body, &peer.session_id) {
        Ok(_) => {
            // The invitee's join answers this: it keeps the room's recent
            // history to read rather than holding it back (ADR-404).
            let _ = attend_groups::Groups::new(&signals_base(), "").invite(&g, &peer.session_id);
            let status = format!("invited @{} to #{g}", peer.display);
            // Same echo rule as directed sends: synthesize only when
            // the destination isn't a dir our own watcher surfaces.
            if sender_watches(&dest) {
                EnterAction::ClearWithStatus(status)
            } else {
                EnterAction::ClearWithStatusAndEcho {
                    status,
                    echo: compose_self_echo(&body),
                }
            }
        }
        Err(e) => EnterAction::StatusOnly(format!("/invite failed: {e}")),
    }
}

/// The invitation's text: how to join, and that the channel's recent history
/// is the invitee's to read only when the channel has any. A join that
/// answers an invitation keeps that history to read (ADR-404); in an empty
/// channel there is none, and saying so would send the invitee looking.
fn invite_body(g: &str, dir: &std::path::Path) -> String {
    let has_history = std::fs::read_dir(dir)
        .map(|entries| entries.flatten().any(|e| e.path().extension().is_some_and(|x| x == "signal")))
        .unwrap_or(false);
    let history = if has_history { " (its recent history is yours to read)" } else { "" };
    format!("you're invited to #{g} — join with: attend join {g}{history}")
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
            "you were removed from #{g} by the operator — rejoin with: attend join {g}"
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

/// Execute a `/purge` against the live signals base. The keep-tail is
/// the heartbeat grace window: anything younger may still be mid-scan
/// by a peer's sensor, so it survives; anything older has been
/// scannable for at least one full grace period by every live peer.
///
/// Tab scoping (#393) changes ONLY which channel is selected: a bare
/// `/purge` targets the foreground tab's channel (`#open` from
/// merged, preserving the old default). The ADR-172 consumer consult
/// and the age tail run identically for every resolution path.
fn run_purge(channel: Option<String>, foreground: &Tab) -> EnterAction {
    let resolved = tabs::command_scope(channel, foreground);
    // `purge_channel_in` speaks the on-disk dialect: `None` is the
    // base channel's `_broadcast/`.
    let target = if resolved == BASE_CHANNEL_NAME {
        None
    } else {
        Some(resolved.as_str())
    };
    let base = signals_base();
    // ADR-172 Decision 5 co-ship: purge consults live consumers' seen
    // sets so it cannot shred a message a live agent session hasn't
    // consumed. The age tail alone was the interim guard (ADR-170's
    // planned tightening — planned exactly because no consumption
    // record existed to consult until the drain shipped one).
    let consumers = crate::consumers::live_consumer_seen_sets(&base, target);
    purge_channel_in(&base, target, attend_presence::heartbeat::DEFAULT_GRACE, &consumers)
}

/// Filesystem-effect core of `/purge` — delete a channel's on-disk
/// signal history, keeping files younger than `keep`.
///
/// This is the deliberate human override of ADR-136's durability
/// default (messages are never age-reaped automatically; they wait
/// for their recipient or their author-project's deletion). A purge
/// is an explicit operator action, scoped to one channel: `None`
/// purges the base channel's `_broadcast/`; a named group purges its
/// `@dir` history without touching the group itself — membership and
/// the channel survive, unlike `/dissolve`.
fn purge_channel_in(
    base: &std::path::Path,
    channel: Option<&str>,
    keep: std::time::Duration,
    consumers: &[std::collections::HashSet<String>],
) -> EnterAction {
    let (label, dir) = match channel {
        None => ("open".to_string(), base.join(attend_groups::BROADCAST_DIR)),
        Some(g) => {
            let dir = base.join(format!("@{g}"));
            if !dir.is_dir() && !attend_groups::load_groups(base).contains_key(g) {
                return EnterAction::StatusOnly(format!("#{g}: unknown group"));
            }
            (g.to_string(), dir)
        }
    };
    let mut removed = 0usize;
    let mut kept = 0usize;
    let mut kept_unconsumed = 0usize;
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            let path = e.path();
            if path.extension().and_then(|s| s.to_str()) != Some("signal") {
                continue;
            }
            let old_enough = e
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| std::time::SystemTime::now().duration_since(t).ok())
                .is_some_and(|age| age > keep);
            // Seen-set consult (ADR-172 Decision 5): every live
            // consumer of this channel must have consumed the message
            // before purge may delete it. The key mirrors the sensor's
            // and the drain's: the signal filename.
            let consumed_by_all = e.file_name().to_str().is_some_and(|f| {
                let key = attend_state::seen_key(f);
                consumers.iter().all(|seen| seen.contains(&key))
            });
            if old_enough && consumed_by_all && std::fs::remove_file(&path).is_ok() {
                removed += 1;
            } else {
                if old_enough && !consumed_by_all {
                    kept_unconsumed += 1;
                }
                kept += 1;
            }
        }
    }
    let unconsumed_note = if kept_unconsumed > 0 {
        format!(", {kept_unconsumed} unconsumed by live sessions")
    } else {
        String::new()
    };
    EnterAction::ClearWithStatus(format!(
        "purged {removed} signal{} from #{label} ({kept} kept{unconsumed_note})",
        if removed == 1 { "" } else { "s" }
    ))
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
pub fn destination_label(input: &str, scope: &str, quiet: &[String], swept: &[String]) -> Option<String> {
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
    Some(labels.join(" "))
}

/// Does the sender's own chat watcher already surface signals written to
/// `dest`? Reuses [`accept_path`] — the single source of truth for what
/// the transcript shows — so a directed send that round-trips (a
/// `#group` / `#open` landing in a watched dir, or an `@agent` in our own
/// cwd) is not *also* echoed synthetically and rendered twice.
fn sender_watches(dest: &std::path::Path) -> bool {
    let base = signals_base();
    let own_cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    // `accept_path` classifies by the first path component (the inbox dir
    // name); hand it a probe file inside `dest` so it judges that dir.
    accept_path(&base, &encode_cwd(&own_cwd), &crate::signal::human_member_id(), &dest.join("probe.signal"))
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
/// `attend send --channel` discipline: a `#groupname` send to a group
/// with zero live members would land in `@<name>/` and sit unread while
/// the chat reports "sent". The base channel (`#open`) bypasses the
/// check — it rides `_broadcast/`, scanned regardless of membership.
///
/// Tab completes quiet agents and swept channels, as the column lists
/// them, so a send to one says why it is refused: an agent named in
/// `quiet` has no heartbeat, and a channel whose directory was swept has
/// nobody live to read it until someone joins it again.
fn resolve_recipients(
    recipients: &[Addressed<'_>],
    agents: &[KnownIdentity],
    quiet: &[String],
    session_of: &dyn Fn(&KnownIdentity) -> Option<String>,
) -> Result<Vec<(std::path::PathBuf, String)>, String> {
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
                    return Err(format!("@{q} is quiet (no heartbeat) and would not read this"));
                }
                // Two live sessions answering to one name: refused, as
                // `attend send --to` refuses an ambiguous name (ADR-401).
                if exact_live > 1 {
                    return Err(format!("@{name} names {exact_live} live sessions; nothing sent"));
                }
                let k = resolve_identity(name, agents).ok_or_else(|| format!("@{name}: unknown nickname"))?;
                let sid = session_of(k).unwrap_or_default();
                if !sid.is_empty() && !sessions.contains(&sid) {
                    sessions.push(sid);
                }
                (cwd_dir(&k.cwd), None)
            }
            Addressed::Group(name) => match resolve_group_dir(name) {
                Some(dir) if live_peer_count(name, &self_member) > 0 => (dir, Some(format!("#{name}"))),
                Some(dir) if !dir.is_dir() => {
                    return Err(format!(
                        "#{name} was swept; /join #{name} and invite a live agent to bring it back, or send to #open"
                    ))
                }
                Some(_) => {
                    return Err(format!(
                        "#{name}: no live peers — message would not be read. \
                         Try #open to broadcast."
                    ))
                }
                None => return Err(format!("#{name}: unknown group")),
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
        match handle_enter("   ", &[], &Tab::Merged, None) {
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

    /// The clean-slate test's finding 4: the invitation mentions history to
    /// read only when the channel holds a message.
    #[test]
    fn an_invitation_mentions_history_only_when_the_channel_has_some() {
        let dir = tempdir_like().join("@empty");
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(invite_body("empty", &dir), "you're invited to #empty — join with: attend join empty");
        std::fs::write(dir.join("notes.txt"), "not a message").unwrap();
        assert!(!invite_body("empty", &dir).contains("history"), "only a signal is history");
        std::fs::write(dir.join("peer-1-1.signal"), "claude:peer|x|/tmp/x|said\n").unwrap();
        assert_eq!(
            invite_body("empty", &dir),
            "you're invited to #empty — join with: attend join empty (its recent history is yours to read)"
        );
        assert!(!invite_body("gone", &dir.join("missing")).contains("history"), "a channel with no directory yet has none");
    }

    #[test]
    fn sender_watches_matches_the_watchers_accept_set() {
        // Regression guard for the double-render bug: a directed send is
        // echoed synthetically ONLY when it reaches no inbox the sender's
        // own watcher surfaces. `sender_watches` must agree with
        // `accept_path` about which destination dirs those are.
        use crate::signal::{broadcast_dir, cwd_dir, signals_base};

        // `#open` → _broadcast/ : watched (would round-trip, so NO echo).
        assert!(sender_watches(&broadcast_dir()));
        // `#group` → @group/ : watched.
        assert!(sender_watches(&signals_base().join("@deploy")));
        // `@agent` in our own cwd → own_encoded : watched.
        let own = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        assert!(sender_watches(&cwd_dir(&own)));
        // `@agent` in some other cwd : NOT watched (invisible → echo).
        assert!(!sender_watches(&signals_base().join("some-other-cwd-abc123")));
    }

    fn tempdir_like() -> std::path::PathBuf {
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
    fn purge_removes_aged_signals_and_keeps_recent() {
        let base = tempdir_like();
        let bdir = base.join("_broadcast");
        std::fs::create_dir_all(&bdir).unwrap();
        std::fs::write(bdir.join("old.signal"), "from|p|/x|old\n").unwrap();
        std::fs::write(bdir.join("notes.txt"), "not a signal").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        // Zero keep-tail: everything with positive age purges.
        match purge_channel_in(&base, None, std::time::Duration::ZERO, &[]) {
            EnterAction::ClearWithStatus(s) => {
                assert!(s.contains("purged 1 signal"), "got: {s}")
            }
            _ => panic!("purge should clear input with status"),
        }
        assert!(!bdir.join("old.signal").exists());
        // Non-signal files are untouched.
        assert!(bdir.join("notes.txt").exists());

        // Real grace window: a fresh signal survives.
        std::fs::write(bdir.join("fresh.signal"), "from|p|/x|hi\n").unwrap();
        match purge_channel_in(&base, None, attend_presence::heartbeat::DEFAULT_GRACE, &[]) {
            EnterAction::ClearWithStatus(s) => {
                assert!(s.contains("purged 0 signals from #open (1 kept)"), "got: {s}")
            }
            _ => panic!("purge should clear input with status"),
        }
        assert!(bdir.join("fresh.signal").exists());
    }

    /// ADR-172 Decision 5: an aged message a live consumer has NOT
    /// consumed survives the purge; once every consumer's seen-set
    /// contains it, it purges. `/purge` and the drain agree through
    /// the same `<dir>:<filename>` key.
    #[test]
    fn purge_keeps_aged_signal_a_live_consumer_has_not_consumed() {
        let base = tempdir_like();
        let bdir = base.join("_broadcast");
        std::fs::create_dir_all(&bdir).unwrap();
        std::fs::write(bdir.join("unread.signal"), "f|p|/x|msg\n").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));

        let key = attend_state::seen_key("unread.signal");
        let empty: std::collections::HashSet<String> = Default::default();
        let seen: std::collections::HashSet<String> = [key].into_iter().collect();

        // One live consumer that has consumed nothing → kept, reported.
        match purge_channel_in(&base, None, std::time::Duration::ZERO, &[empty]) {
            EnterAction::ClearWithStatus(s) => {
                assert!(s.contains("purged 0 signals"), "got: {s}");
                assert!(s.contains("1 unconsumed by live sessions"), "got: {s}");
            }
            _ => panic!("purge should clear input with status"),
        }
        assert!(bdir.join("unread.signal").exists());

        // The same consumer with the message in its seen-set → purged.
        match purge_channel_in(&base, None, std::time::Duration::ZERO, &[seen]) {
            EnterAction::ClearWithStatus(s) => {
                assert!(s.contains("purged 1 signal"), "got: {s}")
            }
            _ => panic!("purge should clear input with status"),
        }
        assert!(!bdir.join("unread.signal").exists());
    }

    #[test]
    fn purge_scopes_to_named_group_and_rejects_unknown() {
        let base = tempdir_like();
        std::fs::create_dir_all(base.join("@deploy")).unwrap();
        std::fs::create_dir_all(base.join("_broadcast")).unwrap();
        std::fs::write(base.join("@deploy").join("a.signal"), "f|p|/x|a\n").unwrap();
        std::fs::write(base.join("_broadcast").join("b.signal"), "f|p|/x|b\n").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        match purge_channel_in(&base, Some("deploy"), std::time::Duration::ZERO, &[]) {
            EnterAction::ClearWithStatus(s) => assert!(s.contains("#deploy"), "got: {s}"),
            _ => panic!("scoped purge should clear input with status"),
        }
        assert!(!base.join("@deploy").join("a.signal").exists());
        // Broadcast untouched by a scoped purge.
        assert!(base.join("_broadcast").join("b.signal").exists());

        match purge_channel_in(&base, Some("ghost"), std::time::Duration::ZERO, &[]) {
            EnterAction::StatusOnly(s) => assert!(s.contains("unknown group")),
            _ => panic!("unknown group should keep input with status"),
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
        let destination_label = |i: &str, s: &str| destination_label(i, s, &[], &[]);
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
    }

    #[test]
    fn the_destination_flag_marks_a_quiet_agent() {
        let quiet = ["Lark".to_string()];
        assert_eq!(destination_label("@lark hi", "open", &quiet, &[]), Some("@lark (quiet)".into()));
        assert_eq!(destination_label("@Tamsin @Lark hi", "open", &quiet, &[]), Some("@Tamsin @Lark (quiet)".into()));
        assert_eq!(destination_label("hi", "open", &quiet, &[]), Some("#open".into()));
    }

    #[test]
    fn the_destination_flag_marks_a_swept_channel() {
        let swept = ["ghost".to_string()];
        assert_eq!(destination_label("#ghost hi", "open", &[], &swept), Some("#ghost (swept)".into()));
        assert_eq!(destination_label("#infra #ghost hi", "open", &[], &swept), Some("#infra #ghost (swept)".into()));
        assert_eq!(destination_label("#ghostly hi", "open", &[], &swept), Some("#ghostly".into()), "the name, not a prefix of it");
    }

    fn ident(name: &str, cwd: &str) -> KnownIdentity {
        let id = agent_identity::Identity::for_cwd(cwd, ColorDepth::Ansi16);
        KnownIdentity { nickname: name.into(), cwd: cwd.into(), is_claude: true, palette: id.palette, style: id.style }
    }

    #[test]
    fn a_send_to_a_quiet_agent_or_a_swept_channel_is_refused_in_its_own_words() {
        let live = [ident("Larks", "/w/larks")];
        let quiet = ["Lark".to_string()];
        let to = |r: Addressed<'_>| resolve_recipients(&[r], &live, &quiet, &|_| None);
        assert_eq!(to(Addressed::Agent("Lark")), Err("@Lark is quiet (no heartbeat) and would not read this".into()), "not carried to Larks by the fuzzy match");
        assert!(to(Addressed::Agent("Larks")).is_ok(), "a live agent is sent to");
        assert_eq!(to(Addressed::Agent("Nobody")), Err("@Nobody: unknown nickname".into()));
        let twins = [ident("Larks", "/w/larks"), ident("Larks", "/w/larks")];
        assert_eq!(
            resolve_recipients(&[Addressed::Agent("Larks")], &twins, &[], &|_| None),
            Err("@Larks names 2 live sessions; nothing sent".into()),
            "two live sessions of one name are refused"
        );
        let seen = |name: &str, live: bool| Seen { identity: ident(name, &format!("/w/{name}")), live, member: name.into() };
        assert_eq!(quiet_names(&[seen("Lark", false), seen("Wells", true), seen("Wells", false)]), ["Lark"], "a name a live agent answers to is not quiet");
    }

    #[test]
    fn destination_label_hides_while_composing_slash() {
        let destination_label = |i: &str, s: &str| destination_label(i, s, &[], &[]);
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
