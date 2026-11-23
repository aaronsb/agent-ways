// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! The signal wire format: one line, `from|project|cwd|message`, or
//! `from|project|cwd|re:<signal-id>|message` for a threaded reply. Tagged
//! fields sit between `cwd` and the message, in a fixed order:
//! `from_kind:`, `from_id:`, `on_behalf_of:`, `to:`, `lane:`, `expires:`,
//! then `re:` last. An unthreaded message whose text itself starts with a
//! known tag is written with an empty thread field, `re:|`, which ends the
//! tags, so the text after it is read as written, whatever it looks like.
//!
//! A signal attend itself writes has `from_kind:system`, `from` set to
//! `system:<sensor>` and `from_id` set to the sensor's name. It carries its
//! lane in `lane:` (`message` or `event`) and may carry an expiry in
//! `expires:`, in Unix seconds. A reader takes `lane:` and `expires:` as
//! tags only after a `from_kind:system` tag, so a line from any other
//! sender reads as it always has.
//!
//! Lives beside [`crate::signal_filename`], which mints the ids a reply
//! references, so every reader (the attend CLI, the attend-chat TUI, the
//! peers sensor) applies one rule for what counts as threaded, and both
//! writers (`attend message send`, attend-chat) produce the same line, the same
//! sender identity and the same atomic file.
//!
//! References:
//! - ADR-120: the line and the `re:` thread field.
//! - ADR-401: the tagged envelope fields and their fence.
//! - ADR-416 §4: the system sender, and the `lane:` and `expires:` tags.

use std::io::{self, Read};
use std::path::Path;

/// A parsed signal line. Legacy signals have no `reply_to`; threaded replies
/// carry the original signal's id there. Borrows from the input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSignal<'a> {
    /// The sender, with any whitespace an older writer let through as `_`
    /// (a `USER` such as `John Smith` under MSYS), so it reads as one word.
    pub from: std::borrow::Cow<'a, str>,
    pub project: &'a str,
    pub cwd: &'a str,
    pub reply_to: Option<&'a str>,
    /// `human`, `agent` or `system`, as the sender's process resolved it.
    pub from_kind: Option<&'a str>,
    /// The canonical sender id: a session id, or a person's member id.
    pub from_id: Option<&'a str>,
    /// The principal whose request the message carries, as the sender claims.
    pub on_behalf_of: Option<&'a str>,
    /// Who the message is for: `*`, `#<channel>`, or a comma-separated set
    /// of session and member ids. `None` on a record written before the tag.
    pub to: Option<&'a str>,
    /// The lane a system signal rides. `None` on any other signal, and on a
    /// system signal written without the tag; [`ParsedSignal::on_lane`]
    /// gives the lane either way.
    pub lane: Option<Lane>,
    /// When a system signal lapses, in Unix seconds. `None` when it has no
    /// expiry and stays until its session reads it.
    pub expires: Option<u64>,
    pub message: &'a str,
}

/// The sender's kind, from the `from_kind` tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Human,
    Agent,
    /// attend itself, writing what one of its sensors reported.
    System,
}

impl Kind {
    /// The value the `from_kind` tag carries.
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Human => "human",
            Kind::Agent => "agent",
            Kind::System => SYSTEM_KIND,
        }
    }
}

/// The lane a signal rides. Words a person or an agent composed ride the
/// message lane. Reports from the environment, or from attend itself, ride
/// the event lane.
///
/// References:
/// - ADR-136: the message lane is split from the sensor bus.
/// - ADR-416 §4: the lane rule has no exception, and a system signal carries
///   its lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lane {
    Message,
    Event,
}

impl Lane {
    /// The value the `lane:` tag carries.
    pub fn as_str(self) -> &'static str {
        match self {
            Lane::Message => "message",
            Lane::Event => "event",
        }
    }

    /// The lane a `lane:` value names, or `None` for any other value.
    pub fn parse(value: &str) -> Option<Lane> {
        match value {
            "message" => Some(Lane::Message),
            "event" => Some(Lane::Event),
            _ => None,
        }
    }
}

/// The prefix of the `from` field on a signal attend itself writes.
pub const SYSTEM_PREFIX: &str = "system:";

/// The `from_kind` value of a signal attend itself writes.
pub const SYSTEM_KIND: &str = "system";

impl ParsedSignal<'_> {
    /// The canonical sender id: the `from_id` tag, or for a record without
    /// it, derived from `from`: a `claude:` sender's session id, an
    /// `external:` sender's user part as a member id (ADR-170).
    pub fn sender_id(&self) -> String {
        if let Some(id) = self.from_id {
            return id.to_string();
        }
        derived_sender_id(&self.from)
    }

    /// The sender's kind: the `from_kind` tag, or from `from`'s prefix.
    /// `None` for a prefix no attend writes. A `from_kind:system` tag counts
    /// only when `from` starts `system:` too, since one writer sets both; on
    /// any other line the kind comes from `from`'s prefix.
    pub fn sender_kind(&self) -> Option<Kind> {
        match self.from_kind {
            Some("human") => Some(Kind::Human),
            Some("agent") => Some(Kind::Agent),
            Some(SYSTEM_KIND) if self.from.starts_with(SYSTEM_PREFIX) => Some(Kind::System),
            _ if self.from.starts_with("claude:") => Some(Kind::Agent),
            _ if self.from.starts_with("external:") => Some(Kind::Human),
            _ if self.from.starts_with(SYSTEM_PREFIX) => Some(Kind::System),
            _ => None,
        }
    }

    /// The lane this signal rides: its `lane:` tag, else the event lane for
    /// a system signal and the message lane for anything a person or an
    /// agent sent.
    pub fn on_lane(&self) -> Lane {
        match self.lane {
            Some(lane) => lane,
            None if self.sender_kind() == Some(Kind::System) => Lane::Event,
            None => Lane::Message,
        }
    }

    /// The name of the sensor that wrote this signal, when attend itself
    /// wrote it.
    pub fn system_sensor(&self) -> Option<String> {
        (self.sender_kind() == Some(Kind::System)).then(|| self.sender_id())
    }

    /// Whether `to` names `id`.
    pub fn addressed_to(&self, id: &str) -> bool {
        self.to.is_some_and(|to| to.split(',').any(|t| t == id))
    }

    /// Whether the session or person reading as `ids` (its id, then the ids
    /// it had before a `/clear`) reads this signal from its tray: when `to`
    /// names one of them, or when the signal carries no `to`, which an older
    /// writer left and which is for every session at the project.
    pub fn tray_reads(&self, ids: &[&str]) -> bool {
        self.to.is_none() || ids.iter().any(|id| self.addressed_to(id))
    }

    /// Whether this signal starts a turn in the idle session reading as
    /// `ids`: it rides the message lane, and it was posted in a named
    /// channel the session is a member of (`in_channel`), `to` names it, it
    /// sat in its tray (`in_tray`) with no `to` (sent to its project by an
    /// older writer), or it replies to a message the session sent. Anything
    /// else waits for the session's next turn: an `#open` post that names
    /// someone else, and every signal on the event lane, even one addressed
    /// to the session. The caller has already set aside the session's own
    /// signals.
    ///
    /// Every reader that decides whether to wake a session asks this, so
    /// the lane rule holds whichever conduit carries the signal.
    ///
    /// References:
    /// - ADR-406 item 2: what wakes an idle session.
    /// - ADR-416 §4: an event-lane signal never starts a turn.
    pub fn wakes(&self, in_tray: bool, in_channel: bool, ids: &[&str]) -> bool {
        self.on_lane() == Lane::Message
            && (in_channel
                || ids.iter().any(|id| self.addressed_to(id))
                || (in_tray && self.to.is_none())
                || self.reply_to.is_some_and(|re| ids.iter().any(|id| sent_by(re, id))))
    }

    /// Whether this signal has lapsed at `now`, in Unix seconds by the
    /// reader's clock: `now` is past its `expires`. A signal with no
    /// `expires` never lapses; the writer leaves the tag off for one kept
    /// until its session reads it (`expires: session`), and writes the
    /// event lane's default window as a value. A reader drops a lapsed
    /// signal unread.
    ///
    /// The writer stamps `expires` from its own clock, and the reader
    /// compares it with its own, so a skew between the two moves the lapse
    /// by as much. Both are this host's clock when the sensor host writes
    /// to its own session's store.
    ///
    /// References:
    /// - ADR-401, The system sender: `expires` is the second after which
    ///   the drain drops the signal unread, and its absence keeps it.
    pub fn lapsed(&self, now: u64) -> bool {
        self.expires.is_some_and(|at| now > at)
    }
}

/// Whether signal id `re` names a message `sender` sent: the id's stem is
/// the sender's sanitized id ([`crate::signal_filename`]).
pub fn sent_by(re: &str, sender: &str) -> bool {
    let stem = crate::sanitize_id_component(sender);
    !stem.is_empty() && re.strip_prefix(&stem).is_some_and(|rest| rest.starts_with('-'))
}

/// The sender id a record without `from_id` carries in `from`.
pub fn derived_sender_id(from: &str) -> String {
    if let Some(sid) = from.strip_prefix("claude:") {
        return sid.to_string();
    }
    if let Some(rest) = from.strip_prefix("external:") {
        return crate::sanitize_id_component(rest.split('@').next().unwrap_or(rest));
    }
    if let Some(sensor) = from.strip_prefix(SYSTEM_PREFIX) {
        return sensor.to_string();
    }
    from.to_string()
}

/// How a reader names the sender of a system signal from `sensor`: as
/// attend, with the sensor beside it, never as a person or an agent.
pub fn system_label(sensor: &str) -> String {
    format!("attend ({sensor})")
}

/// What attend itself writes to one session: a report from `sensor`,
/// addressed to the session id `to`, on `lane`, lapsing at `expires` (Unix
/// seconds), or kept until the session reads it when `expires` is `None`.
///
/// References:
/// - ADR-401, The system sender: `from`, `from_id` and `to` of a system
///   signal, and no `expires` tag for a trigger that keeps until read.
/// - ADR-416 §4: the sensor host writes these, addressed to its session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SystemSignal<'a> {
    pub sensor: &'a str,
    pub to: &'a str,
    pub lane: Lane,
    pub expires: Option<u64>,
}

/// The tagged fields a writer puts on a signal (ADR-401). A field left
/// `None`, or whose value falls outside [`is_tag_value`], is not written.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Envelope<'a> {
    pub from_kind: Option<&'a str>,
    pub from_id: Option<&'a str>,
    pub on_behalf_of: Option<&'a str>,
    pub to: Option<&'a str>,
}

/// The value class of `from_id`, `on_behalf_of` and `to`:
/// `[A-Za-z0-9_@.,:/#*-]+`. It excludes `|`, whitespace and control
/// characters, so no value can break the tagged-field parse.
pub fn is_tag_value(v: &str) -> bool {
    !v.is_empty() && v.chars().all(|c| c.is_ascii_alphanumeric() || "_@.,:/#*-".contains(c))
}

/// The tags a reader consumes from any sender. A writer emits them in this
/// order, with [`SYSTEM_TAGS`] between `to` and `re`.
const TAGS: [&str; 5] = ["from_kind", "from_id", "on_behalf_of", "to", "re"];

/// The tags a reader consumes only after `from_kind:system`. A writer puts
/// them after `to`, so a reader built before them still reads the sender
/// and the addressee, and shows these at the start of the text.
///
/// References:
/// - ADR-401, On-disk form: the seven tags, their order, and the note that
///   a reader takes these two only after `from_kind:system`.
/// - ADR-416 §4: `lane:` and `expires:` join the fenced set.
const SYSTEM_TAGS: [&str; 2] = ["lane", "expires"];

/// Signal ids are filename stems in the form `<sender-id>-<timestamp>`,
/// which is always `[A-Za-z0-9_-]+`. This char class is the discriminator
/// fence that keeps legacy prose starting with "re:" from being misparsed as
/// threaded: `attend message send "re: the thing we discussed|still open"` stays a
/// legacy message because `the thing we discussed` has a space.
pub fn is_valid_signal_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Parse a single-line signal. The discriminator is a `re:<id>|` prefix on
/// the field that follows `cwd`, where `<id>` passes [`is_valid_signal_id`].
/// An empty one, `re:|`, marks an unthreaded message whose text starts
/// `re:` ([`format_signal`] writes it), and is dropped. Any other malformed
/// `re:` prefix degrades to the legacy reading, so prose written before the
/// marker existed round-trips intact. Pipes in the message are kept.
///
/// References:
/// - ADR-401, On-disk form: the tag fence this parse applies.
/// - ADR-416 §4: the system sender's tags.
pub fn parse_signal(content: &str) -> Option<ParsedSignal<'_>> {
    let parts: Vec<&str> = content.splitn(4, '|').collect();
    if parts.len() < 4 {
        return None;
    }
    // The header is attend's, never the sender's prose: a control character
    // in it is a forged line, and it reads as nothing rather than reach a
    // surface that shows the sender. Whitespace in `from` is only an older
    // writer's, so it is mended on read rather than dropped.
    if parts[..3].iter().any(|f| f.chars().any(char::is_control)) {
        return None;
    }
    let from = if parts[0].contains(char::is_whitespace) {
        std::borrow::Cow::Owned(parts[0].chars().map(|c| if c.is_whitespace() { '_' } else { c }).collect())
    } else {
        std::borrow::Cow::Borrowed(parts[0])
    };
    let mut sig = ParsedSignal {
        from,
        project: parts[1],
        cwd: parts[2],
        reply_to: None,
        from_kind: None,
        from_id: None,
        on_behalf_of: None,
        to: None,
        lane: None,
        expires: None,
        message: parts[3],
    };
    // The tags: `key:value|` segments while the key is a known one not yet
    // seen and the value is in its class. `lane:` and `expires:` are known
    // only on a line from `system:` once a `from_kind:system` tag has been
    // read. `re:` is last; it
    // ends the run, and an empty one, `re:|`, marks an unthreaded message.
    while let Some((seg, rest)) = sig.message.split_once('|') {
        let Some((key, value)) = seg.split_once(':') else { break };
        let word = || Some(value).filter(|v| is_tag_value(v));
        let system = sig.from_kind == Some(SYSTEM_KIND) && sig.from.starts_with(SYSTEM_PREFIX);
        let taken = match key {
            "re" if value.is_empty() || is_valid_signal_id(value) => {
                sig.reply_to = (!value.is_empty()).then_some(value);
                sig.message = rest;
                break;
            }
            "from_kind" => take(&mut sig.from_kind, word()),
            "from_id" => take(&mut sig.from_id, word()),
            "on_behalf_of" => take(&mut sig.on_behalf_of, word()),
            "to" => take(&mut sig.to, word()),
            "lane" if system => take(&mut sig.lane, Lane::parse(value)),
            "expires" if system => take(&mut sig.expires, parse_expiry(value)),
            _ => false,
        };
        if !taken {
            break;
        }
        sig.message = rest;
    }
    Some(sig)
}

/// Put `value` in `slot` when the slot is empty and the value was read.
/// Returns whether it did; a repeated tag, or a value outside its class,
/// ends the tags.
fn take<T>(slot: &mut Option<T>, value: Option<T>) -> bool {
    if slot.is_some() || value.is_none() {
        return false;
    }
    *slot = value;
    true
}

/// The value class of `expires`: a count of Unix seconds, ASCII digits only.
fn parse_expiry(value: &str) -> Option<u64> {
    (!value.is_empty() && value.bytes().all(|b| b.is_ascii_digit())).then(|| value.parse().ok()).flatten()
}

/// Whether `message` begins with a tag a reader would take after the tags
/// already written, so a writer must end the tags before it with `re:|`.
/// `system` is whether those tags include `from_kind:system`, after which
/// `lane:` and `expires:` are tags too.
fn starts_with_tag(message: &str, system: bool) -> bool {
    let extra: &[&str] = if system { &SYSTEM_TAGS } else { &[] };
    TAGS.iter().chain(extra).any(|t| message.strip_prefix(t).is_some_and(|r| r.starts_with(':')))
}

/// A signal file's record without the one newline [`format_signal`] ends it
/// with. Only that newline goes: the message is the rest of the line as the
/// sender wrote it, trailing spaces and inner newlines included. Every reader
/// of a `.signal` file passes its content through here before
/// [`parse_signal`], so a body reads back the same in every surface.
pub fn wire_line(content: &str) -> &str {
    content.strip_suffix('\n').unwrap_or(content)
}

/// One wire line, newline-terminated: `from|project|cwd|message`, or
/// `from|project|cwd|re:<id>|message` when `reply_to` is given. An
/// unthreaded message that starts `re:` gets the empty thread field,
/// `re:|`, so no reader can take its text for a thread id.
pub fn format_signal(from: &str, project: &str, cwd: &str, reply_to: Option<&str>, message: &str) -> String {
    format_tagged(from, project, cwd, &Envelope::default(), reply_to, message)
}

/// [`format_signal`] with the ADR-401 tags of `env`, in the fixed order,
/// before the thread field. A tag whose value falls outside its class is
/// left out. An unthreaded message that starts with a known tag gets the
/// empty thread field, `re:|`, so no reader takes its text for a tag.
pub fn format_tagged(from: &str, project: &str, cwd: &str, env: &Envelope, reply_to: Option<&str>, message: &str) -> String {
    let tags: Vec<(&str, String)> = [("from_kind", env.from_kind), ("from_id", env.from_id), ("on_behalf_of", env.on_behalf_of), ("to", env.to)]
        .into_iter()
        .filter_map(|(key, value)| value.filter(|v| is_tag_value(v)).map(|v| (key, v.to_string())))
        .collect();
    format_line(from, project, cwd, &tags, reply_to, message)
}

/// One wire line for a signal attend itself writes: `from` is
/// `system:<sensor>`, followed by the tags `from_kind:system`,
/// `from_id:<sensor>`, `to:<session>`, `lane:` and, when the signal lapses,
/// `expires:`. A sensor name or session id outside [`is_tag_value`] is
/// refused: a line without `to` would reach and wake every session at the
/// project, and one without `from_id` would name a sender its `from` does
/// not.
///
/// References:
/// - ADR-401, The system sender: the line this writes.
/// - ADR-416 §4: what the sensor host writes for each report.
pub fn format_system(project: &str, cwd: &str, sys: &SystemSignal, message: &str) -> Result<String, BadSystemSignal> {
    if !is_tag_value(sys.sensor) {
        return Err(BadSystemSignal::Sensor(sys.sensor.to_string()));
    }
    if !is_tag_value(sys.to) {
        return Err(BadSystemSignal::Addressee(sys.to.to_string()));
    }
    let from = format!("{SYSTEM_PREFIX}{}", sys.sensor);
    let mut tags = vec![
        ("from_kind", SYSTEM_KIND.to_string()),
        ("from_id", sys.sensor.to_string()),
        ("to", sys.to.to_string()),
        ("lane", sys.lane.as_str().to_string()),
    ];
    if let Some(at) = sys.expires {
        tags.push(("expires", at.to_string()));
    }
    Ok(format_line(&from, project, cwd, &tags, None, message))
}

/// Why [`format_system`] refused to write a line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BadSystemSignal {
    /// The sensor name falls outside [`is_tag_value`].
    Sensor(String),
    /// The addressee falls outside [`is_tag_value`].
    Addressee(String),
}

impl std::fmt::Display for BadSystemSignal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BadSystemSignal::Sensor(s) => write!(f, "a system signal needs a sensor name usable as `from_id`, and {s:?} is not one"),
            BadSystemSignal::Addressee(to) => write!(f, "a system signal needs one session id in `to`, and {to:?} is not one"),
        }
    }
}

impl std::error::Error for BadSystemSignal {}

/// The line both writers build: the header, the `key:value|` tags in the
/// order given, the thread field, then the message and one newline.
fn format_line(from: &str, project: &str, cwd: &str, tags: &[(&str, String)], reply_to: Option<&str>, message: &str) -> String {
    // A header field never carries the field separator or a control
    // character, and `from` no whitespace, whatever the environment held.
    let from = header_field(from, true);
    let project = header_field(project, false);
    let cwd = header_field(cwd, false);
    let mut line = format!("{from}|{project}|{cwd}|");
    for (key, value) in tags {
        line += &format!("{key}:{value}|");
    }
    let system = from.starts_with(SYSTEM_PREFIX) && tags.iter().any(|(key, value)| *key == "from_kind" && value == SYSTEM_KIND);
    match reply_to {
        Some(id) => line += &format!("re:{id}|"),
        None if starts_with_tag(message, system) => line += "re:|",
        None => {}
    }
    line += message;
    line.push('\n');
    line
}

/// `s` with each `|` and control character as `_`, and with `no_space`
/// each whitespace character too, so the field cannot end early or break
/// the line.
fn header_field(s: &str, no_space: bool) -> String {
    s.chars().map(|c| if c == '|' || c.is_control() || (no_space && c.is_whitespace()) { '_' } else { c }).collect()
}

/// `s` as a word that is safe in a sender id: ASCII letters, digits, `.`,
/// `_` and `-`, with each run of any other characters as one `_`. A user
/// or terminal name from the environment goes through this, so a newline
/// or markup in `USER` never reaches a peer.
pub fn safe_word(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
            out.push(c);
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    out
}

/// The `project` field for a sender at `cwd`: its last non-empty path
/// segment, or `?` when there is none.
pub fn project_label(cwd: &str) -> String {
    cwd.rsplit('/').find(|seg| !seg.is_empty()).unwrap_or("?").to_string()
}

/// The longest message body `attend message send` and `attend message reply` take, in
/// bytes: 64 KiB. Linux caps one argv element at 128 KiB (`MAX_ARG_STRLEN`),
/// and both the CLI and the attend mod pass the body as one argument, so a
/// body past that fails at exec before attend runs. Half of it leaves room
/// for the signal's header, and a drain delivers at most about 16k tokens
/// per message into the receiving session (ADR-412).
pub const MAX_MESSAGE_BYTES: usize = 64 * 1024;

/// The largest signal file a reader loads, in bytes: 256 KiB. That is above
/// anything an attend wrote, the 128 KiB argv ceiling included, so the
/// limit only turns away a file no attend could have written. A reader skips
/// such a file rather than load it unbounded.
pub const MAX_SIGNAL_FILE_BYTES: u64 = 256 * 1024;

/// A message body over [`MAX_MESSAGE_BYTES`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TooLong {
    pub bytes: usize,
    pub max: usize,
}

impl std::fmt::Display for TooLong {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "the message is {} bytes, over attend's limit of {} bytes ({} KiB); shorten it, or write it to a file and send the path",
            self.bytes,
            self.max,
            self.max / 1024
        )
    }
}

impl std::error::Error for TooLong {}

impl From<TooLong> for io::Error {
    fn from(e: TooLong) -> Self {
        io::Error::new(io::ErrorKind::InvalidInput, e)
    }
}

/// Refuse a body over [`MAX_MESSAGE_BYTES`]. Every writer calls this
/// before it formats a signal: `attend message send` and `reply`, and attend-chat.
pub fn check_message_len(message: &str) -> Result<(), TooLong> {
    if message.len() > MAX_MESSAGE_BYTES {
        return Err(TooLong { bytes: message.len(), max: MAX_MESSAGE_BYTES });
    }
    Ok(())
}

/// Read the signal file at `path` as text, loading at most
/// [`MAX_SIGNAL_FILE_BYTES`] plus one byte. A larger file is an error of
/// kind [`io::ErrorKind::FileTooLarge`], which readers skip with a note.
pub fn read_signal_file(path: &Path) -> io::Result<String> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?.take(MAX_SIGNAL_FILE_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_SIGNAL_FILE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::FileTooLarge,
            format!("signal file is over the {} KiB read limit", MAX_SIGNAL_FILE_BYTES / 1024),
        ));
    }
    String::from_utf8(bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Write `content` into `dest` as `filename`, creating `dest` first. The
/// line goes to `<filename>.tmp` and is renamed into place, so a reader
/// never sees a half-written signal. A line over
/// [`MAX_SIGNAL_FILE_BYTES`] is refused, so no writer leaves a file that
/// every reader would skip.
pub fn write_signal_file(dest: &Path, filename: &str, content: &str) -> io::Result<()> {
    if content.len() as u64 > MAX_SIGNAL_FILE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::FileTooLarge,
            format!("signal is over the {} KiB file limit", MAX_SIGNAL_FILE_BYTES / 1024),
        ));
    }
    std::fs::create_dir_all(dest)?;
    let tmp = dest.join(format!("{filename}.tmp"));
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, dest.join(filename)).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// The login name: `USER`, else `LOGNAME`, else `unknown`.
pub fn user_name() -> String {
    let raw = std::env::var("USER").or_else(|_| std::env::var("LOGNAME")).unwrap_or_default();
    let name = safe_word(&raw);
    if name.trim_matches('_').is_empty() {
        "unknown".to_string()
    } else {
        name
    }
}

/// The terminal a person is typing in, from the environment: a specific
/// emulator or multiplexer first, then `TERM_PROGRAM` (lowercased), an SSH
/// session, and `TERMINAL`'s basename. Empty when nothing names one.
pub fn detect_terminal() -> String {
    for (var, name) in [
        ("KITTY_PID", "kitty"),
        ("ALACRITTY_SOCKET", "alacritty"),
        ("WEZTERM_PANE", "wezterm"),
        ("TMUX", "tmux"),
        ("STY", "screen"),
    ] {
        if std::env::var_os(var).is_some() {
            return name.to_string();
        }
    }
    if let Ok(tp) = std::env::var("TERM_PROGRAM") {
        return safe_word(&tp.to_lowercase());
    }
    if std::env::var_os("SSH_CONNECTION").is_some() {
        return "ssh".to_string();
    }
    if let Ok(t) = std::env::var("TERMINAL") {
        return safe_word(t.rsplit('/').next().unwrap_or(&t));
    }
    String::new()
}

/// Who is sending, as `(sender_id, kind)`: `(session_id, "claude")` inside
/// a Claude session, else `(user@terminal, "external")`, or the bare user
/// when no terminal is known. The wire `from` is `kind:sender_id`.
pub fn identify_sender(session_id: Option<String>) -> (String, &'static str) {
    if let Some(sid) = session_id {
        return (sid, "claude");
    }
    let user = user_name();
    let terminal = detect_terminal();
    if terminal.is_empty() {
        (user, "external")
    } else {
        (format!("{user}@{terminal}"), "external")
    }
}

/// The short handle a delivery line shows for message `id`, written `~` and
/// five base-36 characters: FNV-1a (32-bit) over the id's UTF-16 code units,
/// the last five digits. The attend mod's `shortId` (plugins/attend/hooks/
/// lib.ts) computes the same handle, so a line the mod formats and a line
/// the drain formats name a message alike (ADR-406 item 5), and the
/// Monitor line of the peers sensor carries it too (ADR-412).
pub fn short_id(id: &str) -> String {
    let mut h: u32 = 0x811c_9dc5;
    for unit in id.encode_utf16() {
        h = (h ^ u32::from(unit)).wrapping_mul(0x0100_0193);
    }
    let mut digits = Vec::new();
    loop {
        digits.push(char::from_digit(h % 36, 36).expect("a base-36 digit"));
        h /= 36;
        if h == 0 {
            break;
        }
    }
    while digits.len() < 5 {
        digits.push('0');
    }
    digits.iter().take(5).rev().collect()
}

/// Whether `s` has a short handle's shape: five base-36 characters. A
/// signal id always carries a `-`, so no full id has this shape.
pub fn is_short_id(s: &str) -> bool {
    s.len() == 5 && s.bytes().all(|b| b.is_ascii_digit() || b.is_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_and_parse_round_trip() {
        let line = format_signal("claude:a", "p", "/x/p", Some("id-1"), "a | b");
        assert_eq!(line, "claude:a|p|/x/p|re:id-1|a | b\n");
        let s = parse_signal(line.trim()).unwrap();
        assert_eq!((s.reply_to, s.message), (Some("id-1"), "a | b"));
        assert_eq!(format_signal("claude:a", "p", "/x/p", None, "hi"), "claude:a|p|/x/p|hi\n");
    }

    /// ADR-401: the tagged fields round-trip in the writer's fixed order,
    /// and a reader takes them in any order.
    #[test]
    fn tagged_fields_round_trip() {
        let env = Envelope { from_kind: Some("agent"), from_id: Some("sid-1"), on_behalf_of: Some("aaron"), to: Some("sid-2,sid-3") };
        let line = format_tagged("claude:sid-1", "p", "/x/p", &env, Some("id-1"), "hi | there");
        assert_eq!(line, "claude:sid-1|p|/x/p|from_kind:agent|from_id:sid-1|on_behalf_of:aaron|to:sid-2,sid-3|re:id-1|hi | there\n");
        let s = parse_signal(wire_line(&line)).unwrap();
        assert_eq!((s.from_kind, s.from_id, s.on_behalf_of, s.to), (Some("agent"), Some("sid-1"), Some("aaron"), Some("sid-2,sid-3")));
        assert_eq!((s.reply_to, s.message), (Some("id-1"), "hi | there"));
        let any_order = parse_signal("claude:a|p|/x|to:*|from_kind:agent|body").unwrap();
        assert_eq!((any_order.to, any_order.from_kind, any_order.message), (Some("*"), Some("agent"), "body"));
    }

    const SID: &str = "11111111-2222-3333-4444-555555555555";

    /// What a line reads as: `from_kind`, `from_id`, `to`, `re` and the
    /// message.
    type Fields<'a> = (Option<&'a str>, Option<&'a str>, Option<&'a str>, Option<&'a str>, &'a str);

    /// A system signal on the event lane with an expiry, as this build
    /// writes it, byte for byte.
    const SYSTEM_EVENT_LINE: &str = "system:git|proj|/work/proj|from_kind:system|from_id:git|to:11111111-2222-3333-4444-555555555555|lane:event|expires:1790003600|branch switched to main";

    /// A system signal round-trips on each lane, with and without an
    /// expiry, through the writer and the parser.
    #[test]
    fn a_system_signal_round_trips_on_each_lane() {
        let event = SystemSignal { sensor: "git", to: SID, lane: Lane::Event, expires: Some(1_790_003_600) };
        assert_eq!(format_system("proj", "/work/proj", &event, "branch switched to main").unwrap(), format!("{SYSTEM_EVENT_LINE}\n"));
        for (lane, expires) in [(Lane::Event, Some(1_790_003_600)), (Lane::Event, None), (Lane::Message, Some(42)), (Lane::Message, None)] {
            let sys = SystemSignal { sensor: "ways-update", to: SID, lane, expires };
            let line = format_system("proj", "/work/proj", &sys, "agent-ways has a newer release").unwrap();
            let s = parse_signal(wire_line(&line)).unwrap();
            assert_eq!(s.from, "system:ways-update", "{line}");
            assert_eq!((s.from_kind, s.from_id, s.to), (Some("system"), Some("ways-update"), Some(SID)), "{line}");
            assert_eq!((s.lane, s.expires), (Some(lane), expires), "{line}");
            assert_eq!((s.reply_to, s.message), (None, "agent-ways has a newer release"), "{line}");
            assert_eq!(s.sender_kind(), Some(Kind::System), "{line}");
            assert_eq!(s.on_lane(), lane, "{line}");
            assert_eq!(s.system_sensor().as_deref(), Some("ways-update"), "{line}");
            assert!(s.tray_reads(&[SID]) && !s.tray_reads(&["another-session"]), "{line}");
        }
    }

    /// A line is a system signal only when its `from` starts `system:` as
    /// well as carrying `from_kind:system`. An agent's line that claims the
    /// system kind reads as the agent's, and its `lane:` and `expires:` are
    /// text.
    #[test]
    fn a_system_kind_needs_a_system_from() {
        let line = format!("claude:a|p|/x|from_kind:system|from_id:git|to:{SID}|lane:event|text");
        let s = parse_signal(&line).unwrap();
        assert_eq!(s.sender_kind(), Some(Kind::Agent));
        assert_eq!((s.from_kind, s.from_id, s.to), (Some("system"), Some("git"), Some(SID)));
        assert_eq!((s.lane, s.expires, s.message), (None, None, "lane:event|text"));
        assert_eq!((s.on_lane(), s.system_sensor()), (Lane::Message, None));
        let odd = parse_signal("odd|p|/x|from_kind:system|expires:5|text").unwrap();
        assert_eq!((odd.sender_kind(), odd.expires, odd.message), (None, None, "expires:5|text"));
    }

    /// The writer refuses a system signal it cannot address to one session,
    /// or whose sensor name cannot be its `from_id`, and writes nothing.
    #[test]
    fn a_system_signal_with_a_bad_addressee_or_sensor_is_refused() {
        for to in ["", "a session", "a|b", "line\nbreak"] {
            let sys = SystemSignal { sensor: "git", to, lane: Lane::Event, expires: None };
            assert_eq!(format_system("p", "/x", &sys, "m"), Err(BadSystemSignal::Addressee(to.to_string())), "{to:?}");
        }
        for sensor in ["", "my sensor", "a|b"] {
            let sys = SystemSignal { sensor, to: SID, lane: Lane::Event, expires: None };
            assert_eq!(format_system("p", "/x", &sys, "m"), Err(BadSystemSignal::Sensor(sensor.to_string())), "{sensor:?}");
        }
        let e = BadSystemSignal::Addressee("a|b".into());
        assert_eq!(e.to_string(), "a system signal needs one session id in `to`, and \"a|b\" is not one");
    }

    /// A system signal whose text starts with a tag reads back as written.
    #[test]
    fn a_system_message_starting_with_a_tag_round_trips() {
        let sys = SystemSignal { sensor: "git", to: SID, lane: Lane::Message, expires: None };
        for text in ["expires:9|soon", "lane:event|x", "to:x|y", "re:abc-1|z", "plain"] {
            let line = format_system("p", "/x", &sys, text).unwrap();
            let s = parse_signal(wire_line(&line)).unwrap();
            assert_eq!((s.lane, s.expires, s.reply_to, s.message), (Some(Lane::Message), None, None, text), "{line}");
        }
    }

    /// Lines from before this build, literal, each with every field it
    /// reads as. `lane:` and `expires:` after a sender that is not a system
    /// sender are message text, as they were.
    #[test]
    fn lines_from_before_the_system_sender_parse_unchanged() {
        let cases: [(&str, Fields); 7] = [
            ("claude:sid-9|p|/x|hello", (None, None, None, None, "hello")),
            ("claude:a|p|/x|re:sender-123-0|reply body", (None, None, None, Some("sender-123-0"), "reply body")),
            ("claude:a|p|/x|from_kind:agent|from_id:a|to:b|lane:event|hello", (Some("agent"), Some("a"), Some("b"), None, "lane:event|hello")),
            ("external:aaron@kitty|p|/x|from_kind:human|from_id:aaron|to:*|expires:5|hi", (Some("human"), Some("aaron"), Some("*"), None, "expires:5|hi")),
            ("claude:a|p|/x|to:*|lane:message|expires:1|body", (None, None, Some("*"), None, "lane:message|expires:1|body")),
            ("claude:a|p|/x|from_kind:agent|re:|lane:event|x", (Some("agent"), None, None, None, "lane:event|x")),
            ("odd|p|/x|lane:event|hello", (None, None, None, None, "lane:event|hello")),
        ];
        for (line, (kind, id, to, re, message)) in cases {
            let s = parse_signal(line).unwrap();
            assert_eq!((s.from_kind, s.from_id, s.to, s.reply_to, s.message), (kind, id, to, re, message), "{line}");
            assert_eq!((s.lane, s.expires), (None, None), "{line}");
            assert_eq!(s.on_lane(), Lane::Message, "{line}");
        }
    }

    /// A tag value outside its class begins the message: a lane other than
    /// `message` or `event`, an expiry that is not a count of seconds.
    #[test]
    fn the_system_tags_keep_the_fence() {
        let head = format!("system:git|p|/x|from_kind:system|from_id:git|to:{SID}|");
        for rest in ["lane:urgent|body", "lane:|body", "expires:soon|body", "expires:-1|body", "expires:|body", "expires:99999999999999999999999|body"] {
            let line = format!("{head}{rest}");
            let s = parse_signal(&line).unwrap();
            assert_eq!((s.lane, s.expires, s.message), (None, None, rest), "{line}");
        }
        // A repeated system tag ends the tag run.
        let repeated = format!("{head}lane:event|lane:message|body");
        let s = parse_signal(&repeated).unwrap();
        assert_eq!((s.lane, s.message), (Some(Lane::Event), "lane:message|body"));
        // A system signal without a lane tag rides the event lane.
        let untagged = format!("{head}body");
        let s = parse_signal(&untagged).unwrap();
        assert_eq!((s.lane, s.on_lane()), (None, Lane::Event));
    }

    /// A message an agent sends whose text starts `lane:` is written as
    /// before, with no thread marker, and reads back as written.
    #[test]
    fn an_agent_message_starting_lane_is_written_as_before() {
        let env = Envelope { from_kind: Some("agent"), from_id: Some("a"), on_behalf_of: None, to: Some("*") };
        let line = format_tagged("claude:a", "p", "/x", &env, None, "lane:event|x");
        assert_eq!(line, "claude:a|p|/x|from_kind:agent|from_id:a|to:*|lane:event|x\n");
        assert_eq!(parse_signal(wire_line(&line)).unwrap().message, "lane:event|x");
    }

    /// The parser as attend released it before the system sender, kept
    /// verbatim so the tests can say what an older reader makes of a line
    /// this build writes.
    fn released_reader(content: &str) -> Option<Fields<'_>> {
        let parts: Vec<&str> = content.splitn(4, '|').collect();
        if parts.len() < 4 || parts[..3].iter().any(|f| f.chars().any(char::is_control)) {
            return None;
        }
        let (mut from_kind, mut from_id, mut on_behalf_of, mut to, mut reply_to, mut message) = (None, None, None, None, None, parts[3]);
        while let Some((seg, rest)) = message.split_once('|') {
            let Some((key, value)) = seg.split_once(':') else { break };
            let slot = match key {
                "re" if value.is_empty() || is_valid_signal_id(value) => {
                    reply_to = (!value.is_empty()).then_some(value);
                    message = rest;
                    break;
                }
                "from_kind" => &mut from_kind,
                "from_id" => &mut from_id,
                "on_behalf_of" => &mut on_behalf_of,
                "to" => &mut to,
                _ => break,
            };
            if slot.is_some() || !is_tag_value(value) {
                break;
            }
            *slot = Some(value);
            message = rest;
        }
        let _ = on_behalf_of;
        Some((from_kind, from_id, to, reply_to, message))
    }

    /// An older reader still parses a system line: it reads the sender,
    /// its kind as written and the addressee, so only the session the line
    /// is for reads it, and it shows the lane and expiry tags at the start
    /// of the text.
    #[test]
    fn an_older_reader_parses_a_system_line() {
        let (kind, id, to, re, message) = released_reader(SYSTEM_EVENT_LINE).unwrap();
        assert_eq!((kind, id, to, re), (Some("system"), Some("git"), Some(SID), None));
        assert_eq!(message, "lane:event|expires:1790003600|branch switched to main");
        // The released reader reads an older line as this build does.
        let old = "claude:a|p|/x|from_kind:agent|from_id:a|to:b|re:a-1-0|hi | there";
        let new = parse_signal(old).unwrap();
        assert_eq!(released_reader(old).unwrap(), (new.from_kind, new.from_id, new.to, new.reply_to, new.message));
    }

    /// A legacy record carries no tags; the sender id and kind derive from
    /// `from`.
    #[test]
    fn a_legacy_record_derives_the_sender() {
        let s = parse_signal("claude:sid-9|p|/x|hello").unwrap();
        assert_eq!((s.to, s.from_id), (None, None));
        assert_eq!((s.sender_id(), s.sender_kind()), ("sid-9".to_string(), Some(Kind::Agent)));
        let h = parse_signal("external:aaron@kitty|p|/x|hello").unwrap();
        assert_eq!((h.sender_id(), h.sender_kind()), ("aaron".to_string(), Some(Kind::Human)));
        assert_eq!(parse_signal("odd|p|/x|hello").unwrap().sender_kind(), None);
        let sys = parse_signal("system:git|p|/x|hello").unwrap();
        assert_eq!((sys.sender_id(), sys.sender_kind(), sys.on_lane()), ("git".to_string(), Some(Kind::System), Lane::Event));
        assert_eq!(system_label("git"), "attend (git)");
        let tagged = parse_signal("external:aaron@kitty|p|/x|from_kind:human|from_id:aaron2|hi").unwrap();
        assert_eq!(tagged.sender_id(), "aaron2", "the tag wins over the derivation");
    }

    /// A value outside its class, or an unknown key, begins the message.
    #[test]
    fn the_tag_fence_keeps_prose() {
        for line in ["claude:a|p|/x|to:has space|body", "claude:a|p|/x|note:x|body", "claude:a|p|/x|to:|body", "claude:a|p|/x|to:a;b|body"] {
            let s = parse_signal(line).unwrap();
            assert_eq!(s.to, None, "{line}");
            assert_eq!(s.message, line.splitn(4, '|').nth(3).unwrap(), "{line}");
        }
        // A repeated key ends the tag run.
        let s = parse_signal("claude:a|p|/x|to:a|to:b|body").unwrap();
        assert_eq!((s.to, s.message), (Some("a"), "to:b|body"));
    }

    /// An unthreaded message whose text starts with a known tag reads back as
    /// written: the writer marks it with the empty thread field.
    #[test]
    fn a_message_starting_with_a_tag_round_trips() {
        let env = Envelope { to: Some("*"), ..Envelope::default() };
        for text in ["to:x|looks tagged", "from_id:y|z", "re:abc-1|q", "plain"] {
            let line = format_tagged("claude:a", "p", "/x", &env, None, text);
            let s = parse_signal(wire_line(&line)).unwrap();
            assert_eq!((s.to, s.reply_to, s.message), (Some("*"), None, text), "{line}");
        }
    }

    /// A tag value the writer cannot fence is left out, never written.
    #[test]
    fn a_tag_value_outside_its_class_is_not_written() {
        let env = Envelope { to: Some("a|b"), from_id: Some("ok"), ..Envelope::default() };
        assert_eq!(format_tagged("claude:a", "p", "/x", &env, None, "m"), "claude:a|p|/x|from_id:ok|m\n");
    }

    /// Who reads a tray signal: one whose `to` holds the reader, or one with
    /// no `to` at all (ADR-401, `to`).
    #[test]
    fn a_tray_reader_reads_its_own_and_the_untagged() {
        let mine = parse_signal("claude:x|p|/x|to:me,other|m").unwrap();
        let theirs = parse_signal("claude:x|p|/x|to:other|m").unwrap();
        let legacy = parse_signal("claude:x|p|/x|m").unwrap();
        assert!(mine.addressed_to("me") && mine.tray_reads(&["me"]));
        assert!(!theirs.addressed_to("me") && !theirs.tray_reads(&["me"]));
        assert!(!legacy.addressed_to("me") && legacy.tray_reads(&["me"]));
        // A session after `/clear` still reads what was sent to its old id.
        assert!(theirs.tray_reads(&["me", "other"]));
    }

    /// The wake rule: a post in one of the session's named channels,
    /// addressed to the session, sent to its project (a tray signal without
    /// `to`), or a reply to a message it sent.
    #[test]
    fn the_wake_rule() {
        let me = "11111111-2222-3333-4444-555555555555";
        let (dm_line, reply_line) = (format!("claude:x|p|/x|to:{me}|m"), format!("claude:x|p|/x|to:#deploy|re:{me}-17-0|m"));
        let dm = parse_signal(&dm_line).unwrap();
        let open = parse_signal("claude:x|p|/x|to:*|m").unwrap();
        let channel = parse_signal("claude:x|p|/x|to:#deploy|m").unwrap();
        let legacy = parse_signal("claude:x|p|/x|m").unwrap();
        let reply = parse_signal(&reply_line).unwrap();
        let to_other = parse_signal("claude:x|p|/x|to:other-sess|m").unwrap();
        assert!(dm.wakes(true, false, &[me]) && dm.wakes(false, false, &[me]));
        assert!(legacy.wakes(true, false, &[me]), "sent to its project");
        assert!(!legacy.wakes(false, false, &[me]), "an untagged #open post");
        assert!(!open.wakes(false, false, &[me]));
        assert!(!to_other.wakes(false, false, &[me]), "an #open post naming someone else");
        assert!(channel.wakes(false, true, &[me]), "a post in its channel");
        assert!(to_other.wakes(false, true, &[me]), "a channel post naming someone else");
        assert!(reply.wakes(false, false, &[me]), "a reply to a message it sent");
        // After `/clear`, the old id still counts: a DM to it, a reply to it.
        assert!(dm.wakes(false, false, &["new-id", me]) && reply.wakes(false, false, &["new-id", me]));
        assert!(!reply.wakes(false, false, &["new-id"]));
    }

    /// A system signal on the event lane never wakes, wherever it sits and
    /// whoever it names; one on the message lane wakes as an addressed
    /// message does. A system line without `lane:` rides the event lane.
    #[test]
    fn an_event_lane_signal_never_wakes() {
        let me = "11111111-2222-3333-4444-555555555555";
        let line = |lane: &str| format!("system:git|p|/x|from_kind:system|from_id:git|to:{me}|{lane}branch switched");
        let (event, message, untagged) = (line("lane:event|"), line("lane:message|"), line(""));
        let (event, message, untagged) = (parse_signal(&event).unwrap(), parse_signal(&message).unwrap(), parse_signal(&untagged).unwrap());
        for (in_tray, in_channel) in [(true, false), (false, false), (false, true)] {
            assert!(!event.wakes(in_tray, in_channel, &[me]), "event lane, tray {in_tray}, channel {in_channel}");
            assert!(!untagged.wakes(in_tray, in_channel, &[me]), "no lane tag reads as the event lane");
        }
        assert!(message.wakes(true, false, &[me]), "the message lane wakes the session it names");
        assert!(!message.wakes(true, false, &["someone-else"]), "and only that one");
    }

    /// A signal lapses once the reader's clock passes its `expires`, not at
    /// that second. One with no `expires` never lapses: that is how the
    /// writer records `expires: session`. A non-system line's `expires:`
    /// text is its message, so it never lapses.
    #[test]
    fn a_signal_lapses_after_its_expiry() {
        let at = |tags: &str| format!("system:git|p|/x|from_kind:system|from_id:git|to:s|lane:event|{tags}m");
        let (dated, kept) = (at("expires:1000|"), at(""));
        let (dated, kept) = (parse_signal(&dated).unwrap(), parse_signal(&kept).unwrap());
        assert!(!dated.lapsed(999) && !dated.lapsed(1000), "not before or at the second");
        assert!(dated.lapsed(1001), "after it");
        assert!(!kept.lapsed(u64::MAX), "no expiry, no lapse");
        let peer = parse_signal("claude:x|p|/x|to:*|expires:5|text").unwrap();
        assert!(!peer.lapsed(u64::MAX), "a peer's text is not a tag");
    }

    #[test]
    fn project_label_is_the_last_non_empty_segment() {
        assert_eq!(project_label("/home/u/proj"), "proj");
        assert_eq!(project_label("/home/u/proj/"), "proj");
        assert_eq!(project_label(""), "?");
        assert_eq!(project_label("/"), "?");
    }

    #[test]
    fn a_session_id_makes_a_claude_sender() {
        assert_eq!(identify_sender(Some("sess".into())), ("sess".to_string(), "claude"));
    }

    #[test]
    fn external_sender_env_precedence() {
        // Env vars are process-global, so the whole precedence lattice
        // runs inside one test, resetting between cases.
        let keys = [
            "USER", "LOGNAME", "KITTY_PID", "ALACRITTY_SOCKET", "WEZTERM_PANE", "TMUX", "STY",
            "TERM_PROGRAM", "SSH_CONNECTION", "TERMINAL",
        ];
        let original: Vec<(&str, Option<std::ffi::OsString>)> =
            keys.iter().map(|k| (*k, std::env::var_os(k))).collect();
        let clear_all = || keys.iter().for_each(|k| std::env::remove_var(k));
        let id = || identify_sender(None);

        clear_all();
        std::env::set_var("USER", "tester");
        std::env::set_var("KITTY_PID", "123");
        std::env::set_var("TERM_PROGRAM", "Apple_Terminal");
        assert_eq!(id(), ("tester@kitty".to_string(), "external"), "a specific emulator wins");

        clear_all();
        std::env::set_var("USER", "tester");
        std::env::set_var("TERM_PROGRAM", "iTerm.app");
        assert_eq!(id().0, "tester@iterm.app", "TERM_PROGRAM is lowercased");

        clear_all();
        std::env::set_var("USER", "tester");
        std::env::set_var("TMUX", "/tmp/tmux-0/default,123,0");
        std::env::set_var("TERM_PROGRAM", "Apple_Terminal");
        assert_eq!(id().0, "tester@tmux", "a multiplexer beats the host emulator");

        clear_all();
        std::env::set_var("USER", "tester");
        std::env::set_var("SSH_CONNECTION", "1.2.3.4 22 5.6.7.8 22");
        assert_eq!(id().0, "tester@ssh");

        clear_all();
        std::env::set_var("USER", "tester");
        std::env::set_var("TERMINAL", "/usr/bin/foot");
        assert_eq!(id().0, "tester@foot", "TERMINAL's basename is the last resort");

        clear_all();
        std::env::set_var("USER", "tester");
        assert_eq!(id().0, "tester");

        clear_all();
        std::env::set_var("LOGNAME", "backup_name");
        assert_eq!(id().0, "backup_name");

        clear_all();
        for (k, v) in original {
            if let Some(v) = v {
                std::env::set_var(k, v);
            }
        }
    }

    #[test]
    fn written_signal_lands_whole_under_its_name() {
        let dir = std::env::temp_dir().join(format!("agent-identity-write-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        write_signal_file(&dir.join("tray"), "x-1-0.signal", "a|b|c|d\n").unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("tray").join("x-1-0.signal")).unwrap(), "a|b|c|d\n");
        assert!(!dir.join("tray").join("x-1-0.signal.tmp").exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn threaded_reply_carries_its_id() {
        let s = parse_signal("claude:a|p|/x|re:sender-123-0|reply body").unwrap();
        assert_eq!(s.reply_to, Some("sender-123-0"));
        assert_eq!(s.message, "reply body");
    }

    #[test]
    fn prose_starting_re_stays_legacy() {
        let s = parse_signal("claude:a|p|/x|re: prose|x").unwrap();
        assert_eq!(s.reply_to, None);
        assert_eq!(s.message, "re: prose|x");
    }

    #[test]
    fn legacy_body_keeps_its_pipes() {
        let s = parse_signal("claude:a|p|/x|a | b").unwrap();
        assert_eq!((s.reply_to, s.message), (None, "a | b"));
        assert!(parse_signal("claude:a|p|/x").is_none());
        assert!(parse_signal("").is_none());
    }

    #[test]
    fn a_malformed_re_prefix_keeps_the_raw_tail() {
        for line in ["claude:a|p|/x|re:alone", "claude:a|p|/x|re:has spaces|body"] {
            let s = parse_signal(line).unwrap();
            assert_eq!(s.reply_to, None, "{line}");
            assert_eq!(s.message, line.splitn(4, '|').nth(3).unwrap(), "{line}");
        }
    }

    /// An unthreaded message that starts `re:` reads back as written, even
    /// when its start looks like a thread id: the writer marks it with the
    /// empty thread field.
    #[test]
    fn an_unthreaded_message_starting_re_round_trips() {
        for text in ["re:abc-1|looks threaded", "re: the plan", "re:", "re:|already marked"] {
            let line = format_signal("claude:a", "p", "/x", None, text);
            let s = parse_signal(wire_line(&line)).unwrap();
            assert_eq!((s.reply_to, s.message), (None, text), "{line}");
        }
        let line = format_signal("claude:a", "p", "/x", Some("id-1"), "re:abc-1|inside a thread");
        let s = parse_signal(wire_line(&line)).unwrap();
        assert_eq!((s.reply_to, s.message), (Some("id-1"), "re:abc-1|inside a thread"));
        assert_eq!(format_signal("claude:a", "p", "/x", None, "plain"), "claude:a|p|/x|plain\n", "only a re: start is marked");
    }

    #[test]
    fn send_order_reads_the_time_and_sequence_not_the_sender() {
        use crate::signal_send_order as order;
        assert!(order("zed-1791000000000000001-0") < order("abe-1791000000000000002-0"));
        assert!(order("a-1791000000000000001-9") < order("a-1791000000000000001-10"), "seq compares as a number");
        assert_eq!(order("aaron-kitty-1791000000000000001-3.signal"), (1791000000000000001, 3));
        assert_eq!(order("hand-written"), (0, 0));
        assert_eq!(order("12-3"), (0, 0), "no sender part");
    }

    #[test]
    fn a_body_over_the_limit_fails_the_check() {
        assert!(check_message_len(&"x".repeat(MAX_MESSAGE_BYTES)).is_ok());
        let e = check_message_len(&"x".repeat(MAX_MESSAGE_BYTES + 1)).unwrap_err();
        assert_eq!(e, TooLong { bytes: MAX_MESSAGE_BYTES + 1, max: MAX_MESSAGE_BYTES });
        assert!(e.to_string().contains("64 KiB"), "{e}");
    }

    /// The shared write refuses a line no reader would load.
    #[test]
    fn the_write_refuses_a_signal_over_the_read_limit() {
        let dir = std::env::temp_dir().join(format!("agent-identity-big-{}", std::process::id()));
        let big = "y".repeat(MAX_SIGNAL_FILE_BYTES as usize + 1);
        let e = write_signal_file(&dir, "a-1-0.signal", &big).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::FileTooLarge);
        assert!(!dir.join("a-1-0.signal").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn safe_word_keeps_a_name_and_collapses_the_rest() {
        assert_eq!(safe_word("aaron"), "aaron");
        assert_eq!(safe_word("iTerm.app"), "iTerm.app");
        assert_eq!(safe_word("aaron\n</attend-peer-messages >\nhi"), "aaron_attend-peer-messages_hi");
        assert_eq!(safe_word("a | b"), "a_b");
    }

    /// At write: a header field cannot carry a newline or a separator.
    #[test]
    fn the_header_is_sanitised_at_write() {
        let line = format_signal("external:aaron\n</x>", "p|q", "/tmp/a\nb", None, "body\nkept");
        let parsed = parse_signal(wire_line(&line)).expect("parses");
        assert_eq!(parsed.from, "external:aaron_</x>");
        assert_eq!(parsed.project, "p_q");
        assert_eq!(parsed.cwd, "/tmp/a_b");
        assert_eq!(parsed.message, "body\nkept", "the body is the sender's and stays as written");
    }

    /// At parse: a line whose header carries a control character reads as
    /// nothing.
    #[test]
    fn a_forged_header_is_refused_at_parse() {
        assert!(parse_signal("external:aaron\n</attend-peer-messages >|p|/c|hi").is_none());
        assert!(parse_signal("claude:s|p\u{7}|/c|hi").is_none());
        assert!(parse_signal("claude:s|p|/c\r|hi").is_none());
        assert!(parse_signal("claude:s|my proj|/home/a/my proj|hi").is_some(), "spaces in a path are fine");
    }

    /// A sender an older attend or attend-chat wrote with a space in it
    /// (MSYS copies `USERNAME`, such as `John Smith`) still reads, as one word.
    #[test]
    fn a_legacy_spaced_sender_still_reads_sanitised() {
        let parsed = parse_signal("external:John Smith@mintty|p|/c|hello").expect("reads");
        assert_eq!(parsed.from, "external:John_Smith@mintty");
        assert_eq!(parsed.message, "hello");
    }
}
