//! Keyboard handlers extracted from the iocraft component closure.
//!
//! Each handler is a free function that takes the relevant string +
//! cursor + cycle inputs and returns the next state. The component
//! closure does the `State::set` calls; pulling the logic out keeps
//! the closure short and lets the handlers be unit-tested without
//! standing up an iocraft `App` instance.

use agent_identity::TermCaps;

use crate::chip::{known_identities, resolve_nickname, KnownIdentity};
use crate::groups::{channels, channels_in, live_peer_count, resolve_group_dir, BASE_CHANNEL_NAME};
use crate::legend::{
    all_completions, all_group_completions, apply_completion, find_trailing_mention,
    parse_addressed, Addressed, Sigil,
};
use crate::sessions::discover as discover_sessions;
use crate::signal::{
    compose_self_echo, compose_status_block, cwd_dir, encode_cwd, signals_base, write_broadcast,
    write_signal, Signal,
};
use crate::tabs::{self, Tab};
use crate::watcher::accept_path;
use crate::slash;
use crate::text_layout::split_at_char;

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

/// What the closure should do after `handle_enter` runs.
pub enum EnterAction {
    /// Empty input — no state change.
    None,
    /// Success path: set status, clear input + cursor.
    ClearWithStatus(String),
    /// Directed-send success: like [`EnterAction::ClearWithStatus`], but
    /// also append `echo` to the transcript. Directed (`@name`) sends
    /// land in the recipient's cwd inbox, which the sender does not
    /// watch, so — unlike broadcasts — they never round-trip back into
    /// the sender's own view. The closure appends `echo` so the sender
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
/// and channel commands with no argument scope to it. Returns an
/// `EnterAction` describing how the closure should update state.
pub fn handle_enter(input_value: &str, signals: &[Signal], foreground: &Tab) -> EnterAction {
    let msg = input_value.trim_end().to_string();
    if msg.is_empty() {
        return EnterAction::None;
    }
    // Slash-command interceptor — stops `/help`, `/whois`, etc.
    // from leaking onto the signal bus as plain messages. Runs
    // before the `@`/`#`/broadcast dispatch because slash syntax
    // is start-of-input only and unambiguous.
    if let Some((cmd, args)) = slash::parse(&msg) {
        return match slash::dispatch(cmd, args) {
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
        };
    }
    let caps = TermCaps::detect();
    let seeds = discover_sessions();
    // Local instance cache for this Enter-handler invocation.
    // Distinct from the per-render cache because key handlers are
    // not on the render path — they fire on demand and resolve
    // against the current registry state.
    let instance_cache = attend_instances::SnapshotCache::new();
    let agents = known_identities(signals, &seeds, caps, &instance_cache);
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
            send_to_recipients(&msg, &[Addressed::Group(&scope)], &agents)
        }
    } else {
        send_to_recipients(&msg, &recipients, &agents)
    }
}

/// Addressed-send core shared by the explicit `@`/`#` prefix path and
/// the foreground-tab scoped path. Resolves every recipient up front
/// and rejects the entire send if any address is bad, so one typo
/// doesn't half-deliver and the user can edit + retry the line.
fn send_to_recipients(
    msg: &str,
    recipients: &[Addressed<'_>],
    agents: &[KnownIdentity],
) -> EnterAction {
    let sent = resolve_recipients(recipients, agents).and_then(|dests| {
        for dir in &dests {
            write_signal(dir, msg)?;
        }
        Ok(dests)
    });
    match sent {
        Ok(dests) => {
            let status = format!("sent → {}", recipient_labels(recipients).join(" "));
            // Echo only when the message reaches NO inbox the sender's
            // own watcher already surfaces — otherwise it round-trips
            // and a synthetic echo would double-render it. `#open`
            // lands in `_broadcast/` and `#group` in `@group/` (both
            // watched); an `@agent` in our *own* cwd lands in
            // `own_encoded` (watched). Only `@agent` sends to *other*
            // cwds are invisible without an echo.
            if dests.iter().any(|d| sender_watches(d)) {
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
    let _ = attend_heartbeat::touch(&member);
    join_group_in(&signals_base(), &member, name)
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
    match attend_groups::Groups::new(base, member).join(name, false) {
        Ok(()) => EnterAction::ClearWithStatus(format!("joined #{name}")),
        Err(e) => EnterAction::StatusOnly(format!("/join failed: {e}")),
    }
}

/// Filesystem-effect core of `/leave`. Checks existence + membership
/// before writing so the user gets a precise status instead of a
/// silent no-op — `Groups::leave` itself succeeds on a group you were
/// never in.
fn leave_group_in(base: &std::path::Path, member: &str, name: &str) -> EnterAction {
    let groups = attend_groups::Groups::new(base, member);
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
    let caps = TermCaps::detect();
    let names_before = tabs::strip_names(&channels(caps));
    match dissolve_group_in(&signals_base(), &name, attend_groups::member_alive) {
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
/// `attend focus dissolve` doesn't have: from the TUI this is a
/// channel-hygiene action, and yanking a group out from under peers
/// actively working in it shouldn't be one Enter away. A group whose
/// members are all heartbeat-stale — or an orphan `@dir` the yaml
/// has no entry for — dissolves freely; that's the clutter this
/// command exists to clear.
fn dissolve_group_in<F: Fn(&str) -> bool>(
    base: &std::path::Path,
    name: &str,
    is_live: F,
) -> EnterAction {
    // Member id is irrelevant for dissolve — it acts on the group,
    // not on this member's entry.
    let groups = attend_groups::Groups::new(base, "");
    let members = groups.members(name);
    let dir_exists = groups.group_dir(name).is_dir();
    if members.is_none() && !dir_exists {
        return EnterAction::StatusOnly(format!("#{name}: unknown group"));
    }
    let live = members
        .iter()
        .flatten()
        .filter(|m| is_live(m))
        .count();
    if live > 0 {
        return EnterAction::StatusOnly(format!(
            "#{name}: {live} live member{} — not dissolving",
            if live == 1 { "" } else { "s" }
        ));
    }
    groups.dissolve(name);
    EnterAction::ClearWithStatus(format!("dissolved #{name}"))
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
    match attend_groups::Groups::new(base, "").create(name, description) {
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
    match attend_groups::Groups::new(base, "").set_description(name, text) {
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
    let caps = TermCaps::detect();
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
    let caps = TermCaps::detect();
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
    let caps = TermCaps::detect();
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
    if g == BASE_CHANNEL_NAME {
        return EnterAction::StatusOnly(
            "#open is the base channel — everyone is already there".into(),
        );
    }
    // Inviting into a channel that doesn't exist would hand the peer
    // a join command that mints a brand-new group — more likely a
    // typo than an intent. `/join` it first.
    if resolve_group_dir(&g).is_none() {
        return EnterAction::StatusOnly(format!("#{g}: unknown channel — /join it first"));
    }
    let caps = TermCaps::detect();
    let instance_cache = attend_instances::SnapshotCache::new();
    let roster = crate::peers::roster(caps, &instance_cache);
    let peer = match crate::peers::resolve_peer(member, &roster) {
        Ok(p) => p,
        Err(e) => return EnterAction::StatusOnly(e),
    };
    let dest = cwd_dir(&peer.root);
    let body = format!("you're invited to #{g} — join with: attend join {g}");
    match write_signal(&dest, &body) {
        Ok(_) => {
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
    let caps = TermCaps::detect();
    let instance_cache = attend_instances::SnapshotCache::new();
    let roster = crate::peers::roster(caps, &instance_cache);
    let peer = match crate::peers::resolve_peer(member, &roster) {
        Ok(p) => p,
        Err(e) => return EnterAction::StatusOnly(e),
    };
    let action = kick_member_in(&signals_base(), &g, &peer.session_id, &peer.display);
    if matches!(action, EnterAction::ClearWithStatus(_)) {
        let notice = format!(
            "you were removed from #{g} by the operator — rejoin with: attend join {g}"
        );
        let _ = write_signal(&cwd_dir(&peer.root), &notice);
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
    match attend_groups::Groups::new(base, "").kick(name, member_id) {
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
    purge_channel_in(&base, target, attend_heartbeat::DEFAULT_GRACE, &consumers)
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
        None => ("open".to_string(), base.join("_broadcast")),
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
            // and the drain's collision-proof `<dir>:<filename>` form.
            let consumed_by_all = e.file_name().to_str().is_some_and(|f| {
                let key = format!("{}:{}", dir.display(), f);
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
/// `scope` is the channel an unaddressed message targets.
pub fn destination_label(input: &str, scope: &str) -> Option<String> {
    if slash::parse(input).is_some() || slash::find_slash_partial(input).is_some() {
        return None;
    }
    let recipients = parse_addressed(input);
    if recipients.is_empty() {
        Some(format!("#{scope}"))
    } else {
        Some(recipient_labels(&recipients).join(" "))
    }
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
    let own_encoded = encode_cwd(&own_cwd);
    // `accept_path` classifies by the first path component (the inbox dir
    // name); hand it a probe file inside `dest` so it judges that dir.
    accept_path(&base, &own_encoded, &dest.join("probe.signal"))
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
/// return the first addressing error. All-or-nothing on purpose: the
/// caller writes only when *every* recipient resolves, so a single bad
/// `@name`/`#group` rejects the whole multi-address send instead of
/// partially delivering. Adjacent or repeated addresses that resolve to
/// the same dir are deduped so a message is delivered once per inbox.
///
/// The empty-group rejection (ADR-129 follow-up) mirrors
/// `attend send --focus` discipline: a `#groupname` send to a group
/// with zero live members would land in `@<name>/` and sit unread while
/// the chat reports "sent". The base channel (`#open`) bypasses the
/// check — it rides `_broadcast/`, scanned regardless of membership.
fn resolve_recipients(
    recipients: &[Addressed<'_>],
    agents: &[KnownIdentity],
) -> std::io::Result<Vec<std::path::PathBuf>> {
    let mut dests = Vec::with_capacity(recipients.len());
    let mut seen = std::collections::HashSet::new();
    // Self-exclusion for the liveness gate: since ADR-170 the human
    // is a heartbeating member, so without this a human alone in a
    // group would count themselves as the live listener — reopening
    // the silent-routing trap the gate exists to close.
    let self_member = crate::signal::human_member_id();
    for r in recipients {
        let dir = match r {
            Addressed::Agent(name) => resolve_nickname(name, agents)
                .map(|cwd| cwd_dir(&cwd))
                .ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!("@{name}: unknown nickname"),
                    )
                })?,
            Addressed::Group(name) => match resolve_group_dir(name) {
                Some(dir) if live_peer_count(name, &self_member) > 0 => dir,
                Some(_) => {
                    return Err(std::io::Error::other(format!(
                        "#{name}: no live peers — message would not be read. \
                         Try #open to broadcast."
                    )))
                }
                None => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!("#{name}: unknown group"),
                    ))
                }
            },
        };
        if seen.insert(dir.clone()) {
            dests.push(dir);
        }
    }
    Ok(dests)
}

/// Result of a Tab keypress. Always returns the next buffer +
/// cursor + cycle so the closure applies the result unconditionally,
/// with no "did anything change" branching at the call site.
pub struct TabResult {
    pub new_buf: String,
    pub new_cursor: usize,
    pub new_cycle: Option<TabCycle>,
}

/// Tab-completion + cycle advance. No-op when the caret is mid-buffer
/// (avoids surprising mid-sentence edits) or when no completion
/// applies. Slash completions short-circuit before `@`/`#` parsing,
/// matching the original closure's order.
pub fn handle_tab(
    buf: &str,
    cursor: usize,
    current_cycle: Option<TabCycle>,
    signals: &[Signal],
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
    let caps = TermCaps::detect();
    let candidates: Vec<String> = match mention.sigil {
        Sigil::Agent => {
            let seeds = discover_sessions();
            let instance_cache = attend_instances::SnapshotCache::new();
            let agents = known_identities(signals, &seeds, caps, &instance_cache);
            all_completions(mention.partial, &agents)
        }
        Sigil::Group => {
            let groups = channels(caps);
            all_group_completions(mention.partial, &groups)
        }
    };
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

/// Insert a newline at the cursor (Shift-Enter / Alt-Enter).
pub fn handle_newline_insert(buf: &str, cursor: usize) -> (String, usize) {
    let (before, after) = split_at_char(buf, cursor);
    (format!("{}\n{}", before, after), cursor + 1)
}

/// Insert a single char at the cursor.
pub fn handle_char_insert(buf: &str, cursor: usize, c: char) -> (String, usize) {
    let (before, after) = split_at_char(buf, cursor);
    (format!("{}{}{}", before, c, after), cursor + 1)
}

/// Drop the char to the left of the cursor.
pub fn handle_backspace(buf: &str, cursor: usize) -> (String, usize) {
    if cursor == 0 {
        return (buf.to_string(), 0);
    }
    let (before, after) = split_at_char(buf, cursor);
    let trimmed: String = before.chars().take(cursor.saturating_sub(1)).collect();
    (format!("{}{}", trimmed, after), cursor - 1)
}

/// Drop the char at the cursor.
pub fn handle_delete(buf: &str, cursor: usize) -> (String, usize) {
    let total = buf.chars().count();
    if cursor >= total {
        return (buf.to_string(), cursor);
    }
    let (before, after) = split_at_char(buf, cursor);
    let trimmed: String = after.chars().skip(1).collect();
    (format!("{}{}", before, trimmed), cursor)
}

/// Jump to the start of the current logical line.
pub fn handle_home(buf: &str, cursor: usize) -> usize {
    let (before, _) = split_at_char(buf, cursor);
    before
        .rfind('\n')
        .map(|i| before[..=i].chars().count())
        .unwrap_or(0)
}

/// Jump to the end of the current logical line.
pub fn handle_end(buf: &str, cursor: usize) -> usize {
    let (_, after) = split_at_char(buf, cursor);
    let add = match after.find('\n') {
        Some(byte) => after[..byte].chars().count(),
        None => after.chars().count(),
    };
    cursor + add
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enter_empty_is_noop() {
        match handle_enter("   ", &[], &Tab::Merged) {
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

    #[test]
    fn backspace_at_zero_is_noop() {
        let (b, c) = handle_backspace("hello", 0);
        assert_eq!(b, "hello");
        assert_eq!(c, 0);
    }

    #[test]
    fn backspace_drops_left_char() {
        let (b, c) = handle_backspace("hello", 5);
        assert_eq!(b, "hell");
        assert_eq!(c, 4);
    }

    #[test]
    fn delete_at_end_is_noop() {
        let (b, c) = handle_delete("hi", 2);
        assert_eq!(b, "hi");
        assert_eq!(c, 2);
    }

    #[test]
    fn delete_drops_right_char() {
        let (b, c) = handle_delete("hi", 0);
        assert_eq!(b, "i");
        assert_eq!(c, 0);
    }

    #[test]
    fn char_insert_splits_at_cursor() {
        let (b, c) = handle_char_insert("abc", 1, 'X');
        assert_eq!(b, "aXbc");
        assert_eq!(c, 2);
    }

    #[test]
    fn newline_insert_splits_at_cursor() {
        let (b, c) = handle_newline_insert("abc", 1);
        assert_eq!(b, "a\nbc");
        assert_eq!(c, 2);
    }

    #[test]
    fn home_jumps_to_line_start() {
        // "ab\ncd" with cursor on `d` (chars index 4) → jump to char 3
        // (first char of second line).
        assert_eq!(handle_home("ab\ncd", 4), 3);
        // Single-line buffer → start of buffer.
        assert_eq!(handle_home("hello", 3), 0);
    }

    #[test]
    fn end_jumps_to_line_end() {
        // "ab\ncd" cursor on `c` (index 3) → end of line is index 5.
        assert_eq!(handle_end("ab\ncd", 3), 5);
        // No trailing newline → end of buffer.
        assert_eq!(handle_end("abc", 1), 3);
    }

    fn tempdir_like() -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!(
            "attend-chat-keys-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
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
        match dissolve_group_in(&base, "ghost", |_| false) {
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
        match dissolve_group_in(&base, "bg-test", |_| false) {
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
        match dissolve_group_in(&base, "temp", |_| false) {
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
        match dissolve_group_in(&base, "deploy", |_| true) {
            EnterAction::StatusOnly(s) => assert!(s.contains("1 live member")),
            _ => panic!("live-member dissolve should refuse with status"),
        }
        // Nothing was touched.
        assert!(base.join("@deploy").is_dir());
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
        match purge_channel_in(&base, None, attend_heartbeat::DEFAULT_GRACE, &[]) {
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

        let key = format!("{}:unread.signal", bdir.display());
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
    fn destination_label_hides_while_composing_slash() {
        assert_eq!(destination_label("/", "open"), None);
        assert_eq!(destination_label("/jo", "open"), None);
        assert_eq!(destination_label("/join #deploy", "open"), None);
        // A mid-sentence slash is a literal slash — still a send.
        assert_eq!(destination_label("a / b", "open"), Some("#open".into()));
    }

    #[test]
    fn tab_mid_buffer_is_noop_and_resets_cycle() {
        let res = handle_tab("@foo bar", 3, None, &[]);
        assert_eq!(res.new_buf, "@foo bar");
        assert_eq!(res.new_cursor, 3);
        assert!(res.new_cycle.is_none());
    }
}
