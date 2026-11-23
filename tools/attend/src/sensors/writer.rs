// SPDX-FileCopyrightText: 2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! The writer of what attend's sensors report: one system signal per line,
//! addressed to one session, in its project's tray.
//!
//! A line becomes a signal from `system:<sensor>` with `to` set to the
//! session, the lane the sensor runs on, and an expiry. On the event lane a
//! line lapses when its trigger's `expires` says, after the decay window
//! when the sensor gave no lapse, and never when the trigger keeps it until
//! the session reads it. A message-lane line lapses only when its trigger
//! says so. The sensor host and `attend sensor poll` and `fire` write
//! through it; the drain delivers what it writes like any message.
//!
//! References:
//! - ADR-401, The system sender: the envelope of a system signal.
//! - ADR-416 items 3 and 4: the store as transport, the system sender, the
//!   lane and the expiry.

use agent_identity::signal::{self, Lane, SystemSignal};
use sensor_spec::bounds::EXPIRES_DEFAULT;
use sensor_trait::Lapse;

/// What writes one session's system signals.
#[derive(Debug, Clone)]
pub struct Writer {
    session: String,
    /// The session's directory, whose tray the signals go to.
    cwd: String,
}

impl Writer {
    /// A writer for session `session` in the directory `cwd`. When the
    /// session's record names a directory that is `cwd` reached another
    /// way, such as through a symlink, the record's path is used, so the
    /// signals land in the tray the drain reads. Refused for an id no `to`
    /// tag can carry.
    pub fn new(session: &str, cwd: &str) -> Result<Writer, String> {
        if !signal::is_tag_value(session) {
            return Err(format!("{session:?} is not a session id a signal can be addressed to"));
        }
        let given = attend_presence::session::normalize_origin(cwd);
        let cwd = match attend_presence::session::origin_path(session) {
            Some(origin) if attend_presence::session::same_dir(&origin, &given) => origin,
            _ => given,
        };
        Ok(Writer { session: session.to_string(), cwd })
    }

    pub fn session(&self) -> &str {
        &self.session
    }

    pub fn cwd(&self) -> &str {
        &self.cwd
    }

    /// Write `text`, which sensor `sensor` reported on `lane`, lapsing as
    /// `lapse` says. Returns the signal's id.
    pub fn write(&self, sensor: &str, lane: sensor_spec::Lane, lapse: Option<Lapse>, text: &str) -> Result<String, String> {
        let line = self.line(sensor, lane, lapse, text, sensor_trait::epoch_secs())?;
        let dir = crate::util::signals_base().join(crate::util::encode_project(&self.cwd));
        let filename = agent_identity::signal_filename(sensor);
        signal::write_signal_file(&dir, &filename, &line).map_err(|e| format!("{}: {e}", dir.display()))?;
        Ok(filename.trim_end_matches(".signal").to_string())
    }

    /// The wire line for `text` at `now`, in Unix seconds.
    fn line(&self, sensor: &str, lane: sensor_spec::Lane, lapse: Option<Lapse>, text: &str, now: u64) -> Result<String, String> {
        let lane = match lane {
            sensor_spec::Lane::Event => Lane::Event,
            sensor_spec::Lane::Message => Lane::Message,
        };
        let sys = SystemSignal { sensor, to: &self.session, lane, expires: expires_at(lane, lapse, now) };
        signal::format_system(&signal::project_label(&self.cwd), &self.cwd, &sys, text).map_err(|e| e.to_string())
    }
}

/// When a line written at `now` on `lane` lapses, in Unix seconds, or none
/// when it stays until its session reads it.
pub fn expires_at(lane: Lane, lapse: Option<Lapse>, now: u64) -> Option<u64> {
    match (lapse, lane) {
        (Some(Lapse::After(secs)), _) => Some(now + secs),
        (Some(Lapse::UntilRead), _) => None,
        (None, Lane::Event) => Some(now + EXPIRES_DEFAULT),
        (None, Lane::Message) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SID: &str = "11111111-2222-3333-4444-555555555555";

    fn writer() -> Writer {
        Writer { session: SID.into(), cwd: "/work/proj".into() }
    }

    #[test]
    fn an_event_lapses_after_its_trigger_s_expiry_or_the_decay_window() {
        assert_eq!(expires_at(Lane::Event, Some(Lapse::After(120)), 1000), Some(1120));
        assert_eq!(expires_at(Lane::Event, None, 1000), Some(1000 + EXPIRES_DEFAULT));
        assert_eq!(expires_at(Lane::Event, Some(Lapse::UntilRead), 1000), None, "expires: session writes no tag");
        assert_eq!(expires_at(Lane::Message, None, 1000), None, "words on the message lane do not lapse unless asked");
        assert_eq!(expires_at(Lane::Message, Some(Lapse::After(5)), 1000), Some(1005));
    }

    /// The line reads back as a system signal from the sensor, addressed to
    /// the session, on its lane, with its expiry.
    #[test]
    fn a_line_carries_the_sender_the_addressee_the_lane_and_the_expiry() {
        let line = writer().line("git", sensor_spec::Lane::Event, Some(Lapse::After(60)), "branch switched to main", 1_790_000_000).unwrap();
        let s = signal::parse_signal(signal::wire_line(&line)).unwrap();
        assert_eq!(s.from, "system:git");
        assert_eq!((s.from_kind, s.from_id, s.to), (Some("system"), Some("git"), Some(SID)));
        assert_eq!((s.lane, s.expires), (Some(Lane::Event), Some(1_790_000_060)));
        assert_eq!((s.project, s.cwd, s.message), ("proj", "/work/proj", "branch switched to main"));
        let kept = writer().line("ways-update", sensor_spec::Lane::Event, Some(Lapse::UntilRead), "behind", 1).unwrap();
        assert!(!kept.contains("expires:"), "{kept}");
        let message = writer().line("chat", sensor_spec::Lane::Message, None, "hello", 1).unwrap();
        assert_eq!(signal::parse_signal(signal::wire_line(&message)).unwrap().lane, Some(Lane::Message));
    }

    #[test]
    fn a_session_id_no_tag_can_carry_is_refused() {
        for bad in ["", "a|b", "two words"] {
            assert!(Writer::new(bad, "/work/proj").is_err(), "{bad:?}");
        }
    }
}
