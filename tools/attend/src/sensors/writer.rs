// SPDX-FileCopyrightText: 2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! The writer of what attend's sensors report: one system signal per line,
//! addressed to one session, in its project's tray.
//!
//! A line becomes a signal from `system:<sensor>` with `to` set to the
//! session, the lane the sensor runs on, and an expiry. A line whose
//! trigger sets `expires` lapses when it says, or never for `session`. A
//! line with no `expires`, a compiled sensor's line among them, takes its
//! lane's default: the decay window on the event lane, and no expiry on the
//! message lane. The sensor host and `attend sensor poll` and `fire` write
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

/// The magnitude an event-lane batch needs to be written; one under it is
/// logged alone. A message-lane batch has no cut.
pub const EVENT_CUT: f64 = 3.0;

/// Whether a batch that accumulated `magnitude` on `lane` falls under the
/// cut, so attend logs it and writes no signal.
///
/// References:
/// - ADR-416 item 4: the host applies the event lane's salience before it
///   writes.
pub fn below_cut(lane: sensor_spec::Lane, magnitude: f64) -> bool {
    lane == sensor_spec::Lane::Event && magnitude < EVENT_CUT
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

    /// The lane and expiry of the line the file sensor `yaml` makes of
    /// `result`, with the lapse its trigger gave it, as the writer writes it
    /// at `now`.
    fn written(yaml: &str, result: serde_json::Value, now: u64) -> (Option<Lane>, Option<u64>) {
        struct Never;
        impl crate::sensors::hosted::Source for Never {
            fn fetch(&mut self) -> Result<crate::sensors::hosted::Fetched, String> {
                Err("unused".into())
            }
        }
        let spec = sensor_spec::parse("probe", &serde_yaml::from_str(yaml).unwrap()).spec.unwrap();
        let mut s = crate::sensors::hosted::FileSensor::declared(&spec, Box::new(Never), "/work/proj", None, false);
        let (_, text) = s.evaluate(&result).pop().expect("the trigger fires");
        let line = writer().line("probe", spec.lane(), sensor_trait::Sensor::lapse(&s, &text), &text, now).unwrap();
        let s = signal::parse_signal(signal::wire_line(&line)).unwrap();
        (s.lane, s.expires)
    }

    /// A trigger with no `expires` takes its lane's default: a message-lane
    /// line does not lapse, and an event-lane line lapses with the decay
    /// window. One that sets it lapses when it says.
    #[test]
    fn a_trigger_without_expires_takes_its_lane_s_default() {
        let trigger = |extra: &str| format!("triggers: [{{name: up, path: \"$\", on: enters, where: {{up: true}}, memory: session, author: who{extra}}}]\n");
        let command = "kind: command\nargv: [x]\noutput: json\n";
        let result = serde_json::json!({"up": true, "who": "ann"});
        assert_eq!(written(&format!("{command}lane: message\n{}", trigger("")), result.clone(), 1000), (Some(Lane::Message), None));
        assert_eq!(written(&format!("{command}{}", trigger("")), result.clone(), 1000), (Some(Lane::Event), Some(1000 + EXPIRES_DEFAULT)));
        assert_eq!(written(&format!("{command}lane: message\n{}", trigger(", expires: 60")), result, 1000), (Some(Lane::Message), Some(1060)));
    }

    #[test]
    fn only_an_event_batch_under_the_cut_is_held() {
        assert!(below_cut(sensor_spec::Lane::Event, 2.9));
        assert!(!below_cut(sensor_spec::Lane::Event, 3.0));
        assert!(!below_cut(sensor_spec::Lane::Message, 0.5), "a message-lane batch has no cut");
    }

    #[test]
    fn a_session_id_no_tag_can_carry_is_refused() {
        for bad in ["", "a|b", "two words"] {
            assert!(Writer::new(bad, "/work/proj").is_err(), "{bad:?}");
        }
    }
}
