//! The channel membership one member makes for another (ADR-404 D1): an
//! add, which enrolls the member, and an invitation, which asks it to join.
//!
//! Every surface that adds or invites calls this crate: `attend channel add`,
//! attend-chat's `/invite` and its menus. Each call writes the record in
//! `_groups.yaml` or beside the room (attend-groups), the briefing the
//! member is owed (attend-state), an add's enrollment (attend-presence), and
//! the direct message that tells the member, to its project tray, under the
//! actor's identity.

use std::path::{Path, PathBuf};
use std::time::Duration;

use attend_groups::Groups;
use attend_state::cold_start::{split_room, ADDRESSED_MAX};

/// Where the membership and the seen-sets live.
#[derive(Debug, Clone)]
pub struct Bus {
    /// The signals base: the rooms, the trays and `_groups.yaml`.
    pub signals: PathBuf,
    /// The per-session state files, which hold each seen-set.
    pub state: PathBuf,
    /// The enrollment records ([`attend_presence::enrollment::records_dir`]).
    pub enrolled: PathBuf,
    /// How long a write waits for another writer's lock on `_groups.yaml`;
    /// `None` waits as long as it is held ([`Groups::waiting_at_most`]).
    pub lock_wait: Option<Duration>,
}

impl Bus {
    /// This host's bus, under attend's cache.
    pub fn here() -> Self {
        Bus {
            signals: attend_presence::cache::signals_dir(),
            state: attend_presence::cache::state_dir(),
            enrolled: attend_presence::enrollment::records_dir(),
            lock_wait: None,
        }
    }

    /// The same bus, its writes waiting at most `limit` for the lock.
    pub fn waiting_at_most(mut self, limit: Duration) -> Self {
        self.lock_wait = Some(limit);
        self
    }

    fn groups(&self, member: &str) -> Groups {
        let g = Groups::new(&self.signals, member);
        match self.lock_wait {
            Some(limit) => g.waiting_at_most(limit),
            None => g,
        }
    }
}

/// The member who adds or invites, as the told member reads it and as the
/// message is sent under.
#[derive(Debug, Clone)]
pub struct Actor {
    /// The member id recorded as inviter or adder, and the message's
    /// `from_id`: a session id for an agent, a username for a person.
    pub member_id: String,
    /// The message's `from_kind`: `agent` or `human`.
    pub kind: &'static str,
    /// The name the told member reads: `aaron`, `Chaucer-alpha`.
    pub name: String,
    /// The sender id the signal's file name is made from.
    pub sender_id: String,
    /// The wire `from` field, `<kind>:<sender id>`.
    pub from: String,
    /// The wire project label.
    pub project: String,
    /// The wire cwd.
    pub cwd: String,
}

/// The agent session brought in: its id, and the project root whose tray
/// it reads.
#[derive(Debug, Clone, Copy)]
pub struct Target<'a> {
    pub session_id: &'a str,
    pub root: &'a str,
}

/// The message that told the member: its directory and file name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Told {
    pub dir: PathBuf,
    pub filename: String,
}

/// Why an add or an invitation was refused. Nothing was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    /// `#open`: every session is in it already.
    BaseChannel,
    /// No channel by that name.
    UnknownChannel(String),
    /// The member is in the channel already.
    AlreadyMember(String),
    /// A name, an id or a write that failed.
    Failed(String),
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refused::BaseChannel => f.write_str("#open is the base channel: everyone is already there"),
            Refused::UnknownChannel(name) => write!(f, "#{name}: unknown channel"),
            Refused::AlreadyMember(name) => write!(f, "already in #{name}"),
            Refused::Failed(e) => f.write_str(e),
        }
    }
}

impl std::error::Error for Refused {}

/// What an invitation did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invited {
    /// The message that told the invitee; `None` when it was invited
    /// already, so its record was rewritten and nothing was sent again.
    pub told: Option<Told>,
}

/// What an add did. The member is in the channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Added {
    /// The message that told the member, or why it was not written.
    pub told: Result<Told, String>,
    /// How many of the channel's messages it has to read.
    pub to_read: usize,
    /// How many older ones it was not handed.
    pub held: usize,
}

/// Channel `channel`, as a name an add or an invitation can take: the
/// leading `#` dropped, `#open` and unknown channels refused, and a member
/// already in it refused. A `member` that is not a member id is refused
/// before its id names any file.
fn checked<'a>(bus: &Bus, channel: &'a str, member: &str) -> Result<&'a str, Refused> {
    let name = channel.trim_start_matches('#');
    if name == attend_groups::BASE_CHANNEL {
        return Err(Refused::BaseChannel);
    }
    attend_groups::validate_group_name(name).map_err(Refused::Failed)?;
    attend_groups::validate_member_id(member).map_err(Refused::Failed)?;
    let state = attend_groups::load_groups(&bus.signals);
    let Some(entry) = state.get(name).cloned().or_else(|| {
        bus.signals.join(format!("@{name}")).is_dir().then(Default::default)
    }) else {
        return Err(Refused::UnknownChannel(name.to_string()));
    };
    if entry.members.iter().any(|m| m == member) {
        return Err(Refused::AlreadyMember(name.to_string()));
    }
    Ok(name)
}

/// Whether the room at `dir` holds a message.
fn has_history(dir: &Path) -> bool {
    std::fs::read_dir(dir)
        .map(|entries| entries.flatten().any(|e| e.path().extension().is_some_and(|x| x == "signal")))
        .unwrap_or(false)
}

/// The invitation's text: how to join, and that the channel's recent history
/// is the invitee's to read when the channel has any. A join that answers an
/// invitation keeps that history to read (ADR-404); in an empty channel
/// there is none, and saying so would send the invitee looking.
pub fn invite_body(channel: &str, dir: &Path) -> String {
    let history = if has_history(dir) { " (its recent history is yours to read)" } else { "" };
    format!("you're invited to #{channel} — join with attend_join, or: attend channel join {channel}{history}")
}

/// The add's text, in the order the member meets it: who added it where,
/// the history it may read when the channel has any, then how to leave. The
/// member did not ask to be added, and leaving its last channel stops the
/// listening the add started. The CLI is named, since a mod host may offer
/// no tool for leaving.
pub fn add_body(actor: &str, channel: &str, dir: &Path) -> String {
    let history = if has_history(dir) { " Its recent history is yours to read." } else { "" };
    format!("{actor} added you to #{channel}.{history} Leave with attend channel leave {channel}.")
}

/// Write `body` to `target`'s project tray as `actor`, addressed to the
/// target's session.
fn tell(bus: &Bus, actor: &Actor, target: Target, body: &str) -> Result<Told, String> {
    let dir = bus.signals.join(claude_sessions::attend_key(target.root));
    let envelope = agent_identity::Envelope {
        from_kind: Some(actor.kind),
        from_id: Some(&actor.member_id),
        on_behalf_of: None,
        to: Some(target.session_id),
    };
    let content = agent_identity::signal::format_tagged(&actor.from, &actor.project, &actor.cwd, &envelope, None, body);
    let filename = agent_identity::signal_filename(&actor.sender_id);
    agent_identity::signal::write_signal_file(&dir, &filename, &content).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(Told { dir, filename })
}

/// Invite `target` to `channel` (ADR-404 D1): record the invitation with
/// `actor` as inviter, then tell the invitee how to join. An invitation
/// whose message cannot be written is withdrawn, so the invitee is told of
/// every invitation recorded, and of no other. The invitee joins itself,
/// or not; its join answers the invitation with the room's recent history.
/// An invitee invited already has its record rewritten and is not told
/// again.
pub fn invite(bus: &Bus, actor: &Actor, channel: &str, target: Target) -> Result<Invited, Refused> {
    let name = checked(bus, channel, target.session_id)?;
    let groups = bus.groups(&actor.member_id);
    let again = groups.is_invited(name, target.session_id);
    groups.invite(name, target.session_id).map_err(Refused::Failed)?;
    if again {
        return Ok(Invited { told: None });
    }
    match tell(bus, actor, target, &invite_body(name, &groups.group_dir(name))) {
        Ok(told) => Ok(Invited { told: Some(told) }),
        Err(e) => {
            groups.uninvite(name, target.session_id);
            Err(Refused::Failed(e))
        }
    }
}

/// Add `target` to `channel` (ADR-404 D1): enroll it, with no answer from
/// it, then tell it. The session is enrolled for delivery as well as made a
/// member: the drain delivers to it, and the membership sweep keeps it while
/// its Claude Code process runs, listening or not. It is handed the newest
/// [`ADDRESSED_MAX`] of the room's history to read, as a join answering an
/// invitation is: the older ones are marked seen before the membership is
/// written, so no conduit scans the room with them unseen. A session that
/// was not listening has its cold start reopened, as `attend listen on`
/// does, and is owed the rest as a briefing its cold start delivers
/// ([`Groups::brief`]). Any invitation it had there is spent.
pub fn add(bus: &Bus, actor: &Actor, channel: &str, target: Target) -> Result<Added, Refused> {
    let name = checked(bus, channel, target.session_id)?;
    let sid = target.session_id;
    let groups = bus.groups(&actor.member_id);
    let dir = groups.group_dir(name);
    let split = split_room(&dir, ADDRESSED_MAX);
    let store = attend_state::StateStore::new_in(bus.state.clone(), Some(sid.to_string())).with_signals_base(bus.signals.clone());
    // A session that listened and then stopped is still baselined, and its
    // seen-set ends where it stopped. Enrolled again, it starts cold, as a
    // session that turns listening back on does, so it is not handed every
    // message the rooms carried since.
    let was_enrolled = attend_presence::enrollment::is_enrolled_in(&bus.enrolled, sid);
    let baselined = store.load().is_some_and(|s| s.baselined);
    if baselined && !was_enrolled {
        store.reopen_cold_start();
    }
    let warm = baselined && was_enrolled;
    let held = split.held.len();
    store.mark_seen(split.held);
    let claude = attend_presence::session::claude_key_of(sid);
    // Under the channels lock, once the add is sure to be written: the
    // briefing, then the enrollment, so the sweep never finds the member
    // without them and a refused add leaves neither.
    let admit = || {
        let briefed = !warm && !groups.is_briefed(name, sid);
        if !warm {
            groups.brief(name, sid)?;
        }
        attend_presence::enrollment::enroll_added_in(&bus.enrolled, sid, claude).map_err(|e| {
            if briefed {
                groups.unbrief(name, sid);
            }
            format!("enrolling {sid}: {e}")
        })
    };
    groups.add(name, sid, admit).map_err(|e| match e {
        attend_groups::AddError::UnknownChannel(c) => Refused::UnknownChannel(c),
        attend_groups::AddError::AlreadyMember(c) => Refused::AlreadyMember(c),
        attend_groups::AddError::Failed(e) => Refused::Failed(e),
    })?;
    let told = tell(bus, actor, target, &add_body(&actor.name, name, &dir));
    Ok(Added { told, to_read: split.kept, held })
}

/// Remove `member` from `channel` as `by` (the operator's `/kick`,
/// ADR-173): unilateral, since it only reduces the member's reach. A member
/// left in no channel stops listening by the channel ([`release`]).
pub fn remove(bus: &Bus, by: &str, channel: &str, member: &str) -> Result<(), String> {
    bus.groups(by).kick(channel.trim_start_matches('#'), member)?;
    release(bus, member);
    Ok(())
}

/// Dissolve `channel` as `by`, releasing every member, and return the
/// members it had. Each member left in no channel stops listening by the
/// channel ([`release`]).
pub fn dissolve(bus: &Bus, by: &str, channel: &str) -> Result<Vec<String>, String> {
    let members = bus.groups(by).dissolve(channel.trim_start_matches('#'))?;
    for member in &members {
        release(bus, member);
    }
    Ok(members)
}

/// After `member` lost a channel: once it is in no channel, the enrollment
/// a join or an add gave it is withdrawn, as leaving its last channel
/// withdraws it. One that also turned listening on or ran `attend run`
/// stays enrolled through that. The check and the withdrawal run under the
/// channels lock, so an add landing at the same time keeps its enrollment.
pub fn release(bus: &Bus, member: &str) {
    use attend_presence::enrollment::{withdraw_in, Source};
    bus.groups("").when_in_no_channel(member, || {
        for source in [Source::Join, Source::Add] {
            withdraw_in(&bus.enrolled, member, source).ok();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A bus under the build's target directory, removed when dropped. The
    /// test binary runs from `<target>/<profile>/deps/`.
    struct Scratch(PathBuf);

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn scratch(tag: &str) -> (Scratch, Bus) {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let exe = std::env::current_exe().unwrap();
        let profile = exe.parent().and_then(Path::parent).unwrap();
        let root = profile.join("test-scratch").join(format!(
            "attend-membership-{tag}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = Bus { signals: root.join("signals"), state: root.join("state"), enrolled: root.join("enrolled"), lock_wait: None };
        (Scratch(root), bus)
    }

    fn aaron() -> Actor {
        Actor {
            member_id: "aaron".into(),
            kind: "human",
            name: "aaron".into(),
            sender_id: "aaron@term".into(),
            from: "external:aaron@term".into(),
            project: "chat".into(),
            cwd: "/home/aaron".into(),
        }
    }

    const SID: &str = "2f2632d7-3f59-4c24-9aae-000000000001";
    const ROOT: &str = "/home/aaron/proj";

    fn target() -> Target<'static> {
        Target { session_id: SID, root: ROOT }
    }

    fn tray(bus: &Bus) -> PathBuf {
        bus.signals.join(claude_sessions::attend_key(ROOT))
    }

    /// The one message in the target's tray.
    fn told(bus: &Bus) -> String {
        let files: Vec<_> = std::fs::read_dir(tray(bus)).unwrap().flatten().collect();
        assert_eq!(files.len(), 1, "one message in the tray");
        std::fs::read_to_string(files[0].path()).unwrap()
    }

    /// The target enrolled by `attend listen on`.
    fn listening(bus: &Bus) {
        std::fs::create_dir_all(&bus.enrolled).unwrap();
        std::fs::write(bus.enrolled.join(SID), "listen\n").unwrap();
    }

    /// `n` messages in `#<channel>`, the first oldest.
    fn history(bus: &Bus, channel: &str, n: usize) {
        let dir = bus.signals.join(format!("@{channel}"));
        std::fs::create_dir_all(&dir).unwrap();
        let now = std::time::SystemTime::now();
        for i in 0..n {
            let f = std::fs::File::create(dir.join(format!("peer-{i:03}.signal"))).unwrap();
            f.set_modified(now - Duration::from_secs(3600 + (n - i) as u64 * 60)).unwrap();
        }
    }

    #[test]
    fn an_invitation_tells_the_invitee_and_records_the_inviter() {
        let (_s, bus) = scratch("invite");
        bus.groups("aaron").create("topic", None).unwrap();
        let invited = invite(&bus, &aaron(), "#topic", target()).unwrap();
        let t = invited.told.expect("told");
        assert_eq!(t.dir, tray(&bus));
        let line = told(&bus);
        assert!(line.contains(&format!("from_kind:human|from_id:aaron|to:{SID}|")), "{line}");
        assert!(line.contains("you're invited to #topic"), "{line}");
        let invites = bus.groups("x").invites("topic");
        assert_eq!((invites[0].member.as_str(), invites[0].by.as_deref()), (SID, Some("aaron")));
    }

    #[test]
    fn inviting_again_rewrites_the_record_and_sends_nothing() {
        let (_s, bus) = scratch("invite-again");
        bus.groups("aaron").create("topic", None).unwrap();
        invite(&bus, &aaron(), "topic", target()).unwrap();
        let again = invite(&bus, &Actor { member_id: "bob".into(), ..aaron() }, "topic", target()).unwrap();
        assert_eq!(again.told, None);
        told(&bus);
        assert_eq!(bus.groups("x").invites("topic")[0].by.as_deref(), Some("bob"));
    }

    #[test]
    fn add_and_invite_refuse_open_unknown_and_members() {
        let (_s, bus) = scratch("refuse");
        for channel in ["open", "#open"] {
            assert_eq!(invite(&bus, &aaron(), channel, target()), Err(Refused::BaseChannel));
            assert_eq!(add(&bus, &aaron(), channel, target()), Err(Refused::BaseChannel));
        }
        assert_eq!(invite(&bus, &aaron(), "ghost", target()), Err(Refused::UnknownChannel("ghost".into())));
        assert_eq!(add(&bus, &aaron(), "ghost", target()), Err(Refused::UnknownChannel("ghost".into())));
        assert!(!tray(&bus).exists(), "a refusal tells no one");
        assert!(!bus.enrolled.join(SID).exists(), "and enrolls no one");
        assert!(!bus.signals.join("@ghost").exists());

        Groups::new(&bus.signals, SID).join("topic", false).unwrap();
        assert_eq!(invite(&bus, &aaron(), "topic", target()), Err(Refused::AlreadyMember("topic".into())));
        assert_eq!(add(&bus, &aaron(), "topic", target()), Err(Refused::AlreadyMember("topic".into())));
    }

    #[test]
    fn an_add_enrolls_spends_the_invitation_and_tells_the_member() {
        let (_s, bus) = scratch("add");
        bus.groups("aaron").create("topic", None).unwrap();
        history(&bus, "topic", 3);
        bus.groups("aaron").invite("topic", SID).unwrap();
        let added = add(&bus, &aaron(), "topic", target()).unwrap();
        assert_eq!((added.to_read, added.held), (3, 0));
        assert_eq!(added.told.as_ref().map(|t| t.dir.clone()), Ok(tray(&bus)));
        assert_eq!(bus.groups("x").members("topic").unwrap(), vec![SID]);
        assert!(bus.groups("x").invites("topic").is_empty(), "the invitation is spent");
        let record = std::fs::read_to_string(bus.enrolled.join(SID)).expect("an enrollment record");
        assert!(record.lines().any(|l| l == "add"), "enrolled by the add: {record}");
        let line = told(&bus);
        assert!(line.contains(&format!("from_kind:human|from_id:aaron|to:{SID}|")), "{line}");
        assert!(
            line.ends_with("aaron added you to #topic. Its recent history is yours to read. Leave with attend channel leave topic.\n"),
            "{line}"
        );
    }

    #[test]
    fn an_add_to_an_empty_channel_says_nothing_of_history() {
        let (_s, bus) = scratch("add-empty");
        bus.groups("aaron").create("topic", None).unwrap();
        add(&bus, &aaron(), "topic", target()).unwrap();
        assert!(told(&bus).ends_with("|aaron added you to #topic. Leave with attend channel leave topic.\n"));
    }

    /// The briefing is the invitation's: the newest 50 of the room stay to
    /// read, the older are marked seen. A member whose cold start has not
    /// run is owed them as a briefing; a warm one, listening and baselined,
    /// reads them as unseen.
    #[test]
    fn an_add_hands_over_the_newest_history_warm_or_cold() {
        for warm in [true, false] {
            let (_s, bus) = scratch(if warm { "add-warm" } else { "add-cold" });
            bus.groups("aaron").create("topic", None).unwrap();
            history(&bus, "topic", ADDRESSED_MAX + 4);
            let store = attend_state::StateStore::new_in(bus.state.clone(), Some(SID.into())).with_signals_base(bus.signals.clone());
            if warm {
                store.baseline(Vec::<String>::new());
                listening(&bus);
            }
            let added = add(&bus, &aaron(), "topic", target()).unwrap();
            assert_eq!((added.to_read, added.held), (ADDRESSED_MAX, 4));
            let seen = store.load().unwrap().seen_signals;
            let oldest: Vec<String> = (0..4).map(|i| format!("peer-{i:03}.signal")).collect();
            assert!(oldest.iter().all(|k| seen.contains(k)), "warm={warm}: the oldest are held: {seen:?}");
            assert!(!seen.contains("peer-004.signal"), "warm={warm}: the newest stay to read");
            let briefed = attend_groups::briefings(&bus.signals, SID);
            assert_eq!(briefed.len(), usize::from(!warm), "warm={warm}: only a cold member is owed a briefing");
            assert_eq!(store.load().unwrap().baselined, warm, "the add runs no cold start");
        }
    }

    /// Two adds of one session race: the first is under way when the
    /// second lands, enrollment and membership both, and the first is then
    /// refused. The refusal leaves the second add's enrollment standing.
    #[test]
    fn a_refused_add_leaves_another_adds_enrollment() {
        let (_s, bus) = scratch("race");
        bus.groups("aaron").create("topic", None).unwrap();
        let state_path = bus.signals.join("_groups.yaml");
        let held = agent_settings::writer::Lock::acquire(&state_path).unwrap();
        let first = {
            let bus = bus.clone();
            std::thread::spawn(move || add(&bus, &aaron(), "topic", target()))
        };
        // The first add waits for the lock; let it get as far as it goes.
        let start = std::time::Instant::now();
        while !bus.enrolled.join(SID).exists() && start.elapsed() < Duration::from_millis(500) {
            std::thread::sleep(Duration::from_millis(10));
        }
        attend_presence::enrollment::enroll_added_in(&bus.enrolled, SID, None).unwrap();
        let mut state = attend_groups::load_groups(&bus.signals);
        state.get_mut("topic").unwrap().members.push(SID.into());
        std::fs::write(&state_path, attend_groups::serialize_groups_yaml(&state)).unwrap();
        drop(held);
        assert_eq!(first.join().unwrap(), Err(Refused::AlreadyMember("topic".into())));
        assert!(attend_presence::enrollment::is_enrolled_in(&bus.enrolled, SID), "the second add's enrollment stands");
    }

    /// An add that fails after it began leaves no briefing behind.
    #[test]
    fn a_failed_add_leaves_no_briefing() {
        let (_s, bus) = scratch("no-brief");
        bus.groups("aaron").create("topic", None).unwrap();
        // A file where the enrollment records go: enrolling fails.
        std::fs::write(&bus.enrolled, "").unwrap();
        assert!(matches!(add(&bus, &aaron(), "topic", target()), Err(Refused::Failed(_))));
        assert!(attend_groups::briefings(&bus.signals, SID).is_empty(), "no briefing is left");
        assert!(bus.groups("x").members("topic").unwrap().is_empty());
    }

    /// A session id that is not a member id is refused before anything is
    /// written: the seen-set, the briefing, the enrollment or the message.
    #[test]
    fn a_session_id_that_is_no_member_id_is_refused_before_any_write() {
        let (s, bus) = scratch("bad-id");
        bus.groups("aaron").create("topic", None).unwrap();
        history(&bus, "topic", ADDRESSED_MAX + 2);
        let target = Target { session_id: "../escape", root: ROOT };
        assert!(matches!(add(&bus, &aaron(), "topic", target), Err(Refused::Failed(_))));
        assert!(matches!(invite(&bus, &aaron(), "topic", target), Err(Refused::Failed(_))));
        assert!(!s.0.join("escape.state").exists(), "no seen-set beside the state dir");
        assert!(!tray(&bus).exists(), "no one is told");
        assert!(!bus.enrolled.exists(), "no one is enrolled");
    }

    /// An invitation is recorded and sent, or neither: one whose record
    /// cannot be written is not sent, and one whose message cannot be
    /// written is withdrawn.
    #[test]
    fn an_invitation_is_recorded_and_sent_or_neither() {
        let (_s, bus) = scratch("invite-unrecorded");
        bus.groups("aaron").create("topic", None).unwrap();
        std::fs::create_dir_all(bus.signals.join("@topic").join(format!("{SID}.invite"))).unwrap();
        assert!(matches!(invite(&bus, &aaron(), "topic", target()), Err(Refused::Failed(_))));
        assert!(!tray(&bus).exists(), "no one is told of an invitation that is not recorded");

        let (_s, bus) = scratch("invite-unsent");
        bus.groups("aaron").create("topic", None).unwrap();
        std::fs::write(tray(&bus), "").unwrap();
        assert!(matches!(invite(&bus, &aaron(), "topic", target()), Err(Refused::Failed(_))));
        assert!(bus.groups("x").invites("topic").is_empty(), "an invitation not sent is not recorded");
    }
}
