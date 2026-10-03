//! The timeline screen shape (ADR-504 §3): a run of frames, a position in
//! it, playback, and for a live source a follow of the newest frame; a
//! scrubber that draws the position; and the bottom bar of a screen that
//! is not the settings shell.
//!
//! [`Playback`] holds no frames, only how many there are, so an
//! application keeps its frames in its own types.

use std::time::Duration;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use crate::theme::{self, Ground, Seg, Shape};

/// The playback speeds: the time each frame shows, and its label.
pub const SPEEDS: &[(u64, &str)] = &[(2000, "2.0s"), (1000, "1.0s"), (500, "0.5s"), (250, "0.25s"), (100, "0.1s")];

/// The speed a replay starts at.
const DEFAULT_SPEED: usize = 1;

/// Where a timeline is and how it moves.
///
/// A replay starts at the first frame, paused. A live timeline starts at
/// the newest and follows it as frames arrive, until a move to an earlier
/// frame stops the follow; the end, or the toggle, resumes it.
#[derive(Debug, Clone, PartialEq)]
pub struct Playback {
    len: usize,
    pos: usize,
    playing: bool,
    speed: usize,
    live: bool,
    following: bool,
}

impl Playback {
    /// A replay of `len` frames, at the first.
    pub fn replay(len: usize) -> Playback {
        Playback { len, pos: 0, playing: false, speed: DEFAULT_SPEED, live: false, following: false }
    }

    /// A live timeline of `len` frames, at the newest and following it.
    pub fn live(len: usize) -> Playback {
        Playback { len, pos: len.saturating_sub(1), playing: false, speed: DEFAULT_SPEED, live: true, following: true }
    }

    /// The speed whose frame time is the first at or below `ms`; the
    /// fastest when `ms` is faster than every speed.
    pub fn with_speed_ms(mut self, ms: u64) -> Playback {
        self.speed = SPEEDS.iter().position(|(s, _)| *s <= ms).unwrap_or(SPEEDS.len() - 1);
        self
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    pub fn playing(&self) -> bool {
        self.playing
    }

    pub fn is_live(&self) -> bool {
        self.live
    }

    pub fn following(&self) -> bool {
        self.following
    }

    pub fn speed_label(&self) -> &'static str {
        SPEEDS[self.speed].1
    }

    /// How long a frame shows while playing.
    pub fn frame_time(&self) -> Duration {
        Duration::from_millis(SPEEDS[self.speed].0)
    }

    fn last(&self) -> usize {
        self.len.saturating_sub(1)
    }

    /// Move to frame `to`, clamped. A move by hand stops playback and the
    /// live follow.
    pub fn go(&mut self, to: usize) {
        self.playing = false;
        self.following = false;
        self.pos = to.min(self.last());
    }

    /// One frame on, or back.
    pub fn step(&mut self, forward: bool) {
        let to = if forward { self.pos + 1 } else { self.pos.saturating_sub(1) };
        self.go(to);
    }

    pub fn home(&mut self) {
        self.go(0);
    }

    /// The newest frame; live, it follows again.
    pub fn end(&mut self) {
        self.go(usize::MAX);
        self.following = self.live;
    }

    /// Play or pause a replay; follow or stop following live.
    pub fn toggle(&mut self) {
        if self.live {
            self.following = !self.following;
            if self.following {
                self.pos = self.last();
            }
        } else {
            self.playing = !self.playing;
        }
    }

    pub fn faster(&mut self) {
        self.speed = (self.speed + 1).min(SPEEDS.len() - 1);
    }

    pub fn slower(&mut self) {
        self.speed = self.speed.saturating_sub(1);
    }

    pub fn pause(&mut self) {
        self.playing = false;
    }

    /// A frame time has passed: a playing replay moves on one frame, and
    /// stops at the last. True when the position moved.
    pub fn tick(&mut self) -> bool {
        if !self.playing || self.live {
            return false;
        }
        if self.pos < self.last() {
            self.pos += 1;
            true
        } else {
            self.playing = false;
            false
        }
    }

    /// The frames were read again and there are `len` now. A live
    /// timeline that follows, or sat at the newest, goes to the newest;
    /// otherwise the position stays, clamped.
    pub fn resize(&mut self, len: usize) {
        let at_last = self.pos >= self.last();
        self.len = len;
        self.pos = if self.live && (self.following || at_last) { self.last() } else { self.pos.min(self.last()) };
    }
}

/// The position in a timeline as a track: played in the accent, the rest
/// in the rule colour, `marks` (frame indexes, such as where a window
/// starts) as ticks, `notes` (frames that carry something to look at) as
/// warnings, the position as a dot, and `pos/len` at the right.
pub struct Scrubber<'a> {
    pub len: usize,
    pub pos: usize,
    pub marks: &'a [usize],
    pub notes: &'a [usize],
}

impl Scrubber<'_> {
    /// The line the scrubber draws in `width` columns.
    pub fn line(&self, width: u16) -> Line<'static> {
        let label = format!(" {}/{}", if self.len == 0 { 0 } else { self.pos + 1 }, self.len);
        let track = (width as usize).saturating_sub(label.chars().count());
        let mut spans = Vec::new();
        if track > 0 {
            // The cell a frame falls in: the first frame at the left end,
            // the last at the right.
            let cell = |i: usize| if self.len <= 1 { 0 } else { i * (track - 1) / (self.len - 1) };
            let head = cell(self.pos.min(self.len.saturating_sub(1)));
            let marks: Vec<usize> = self.marks.iter().filter(|m| **m < self.len).map(|m| cell(*m)).collect();
            let notes: Vec<usize> = self.notes.iter().filter(|m| **m < self.len).map(|m| cell(*m)).collect();
            for x in 0..track {
                let (glyph, style) = if x == head {
                    ("●", theme::accent().add_modifier(Modifier::BOLD))
                } else if notes.contains(&x) {
                    ("⊝", theme::warn())
                } else if marks.contains(&x) {
                    ("┼", theme::muted())
                } else if x < head {
                    ("━", theme::accent())
                } else {
                    ("─", theme::rule())
                };
                spans.push(Span::styled(glyph, style));
            }
        }
        spans.push(Span::styled(label, Style::new().add_modifier(Modifier::BOLD)));
        Line::from(spans)
    }
}

impl Widget for Scrubber<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Paragraph::new(self.line(area.width)).render(area, buf);
    }
}

/// A screen's bottom bar: the mode as a lozenge, then each key and what it
/// does, then `right` at the right end when there is room.
pub fn key_bar(shape: Shape, mode: &str, ground: Ground, keys: &[(&str, &str)], right: Vec<Span<'static>>, width: u16) -> Line<'static> {
    let mut spans = shape.lozenge(&[Seg::on(format!(" {mode} "), ground).bold()]);
    for (k, what) in keys {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(k.to_string(), theme::accent().add_modifier(Modifier::BOLD)));
        spans.push(Span::styled(format!(" {what}"), theme::hint()));
    }
    let used: usize = spans.iter().map(Span::width).sum();
    let tail: usize = right.iter().map(Span::width).sum();
    if !right.is_empty() && used + tail + 2 <= width as usize {
        spans.push(Span::raw(" ".repeat(width as usize - used - tail)));
        spans.extend(right);
    }
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{set, Palette};
    use agent_theme::ColorDepth;

    #[test]
    fn a_replay_plays_to_the_end_and_stops() {
        let mut p = Playback::replay(3);
        assert_eq!((p.pos(), p.playing()), (0, false));
        assert!(!p.tick(), "paused, a tick does nothing");
        p.toggle();
        assert!(p.tick() && p.tick());
        assert_eq!(p.pos(), 2);
        assert!(!p.tick(), "the last frame ends playback");
        assert!(!p.playing());
    }

    #[test]
    fn a_move_by_hand_stops_play_and_the_follow() {
        let mut p = Playback::replay(5);
        p.toggle();
        p.step(true);
        assert_eq!((p.pos(), p.playing()), (1, false));
        p.go(99);
        assert_eq!(p.pos(), 4, "clamped to the last frame");
        p.home();
        assert_eq!(p.pos(), 0);

        let mut l = Playback::live(5);
        assert_eq!((l.pos(), l.following()), (4, true));
        l.step(false);
        assert_eq!((l.pos(), l.following()), (3, false));
        l.end();
        assert_eq!((l.pos(), l.following()), (4, true), "the end resumes the follow");
        l.toggle();
        assert!(!l.following());
        l.go(1);
        l.toggle();
        assert_eq!((l.pos(), l.following()), (4, true), "following again jumps to the newest");
    }

    #[test]
    fn new_frames_move_a_following_timeline_and_leave_one_looking_back() {
        let mut l = Playback::live(3);
        l.resize(5);
        assert_eq!(l.pos(), 4);
        l.go(1);
        l.resize(8);
        assert_eq!(l.pos(), 1, "looking back stays put");
        let mut r = Playback::replay(5);
        r.go(4);
        r.resize(8);
        assert_eq!(r.pos(), 4, "a replay never follows");
        r.resize(2);
        assert_eq!(r.pos(), 1, "clamped when frames go");
    }

    #[test]
    fn speeds_step_within_their_range() {
        let mut p = Playback::replay(1).with_speed_ms(500);
        assert_eq!(p.speed_label(), "0.5s");
        for _ in 0..9 {
            p.faster();
        }
        assert_eq!(p.frame_time(), Duration::from_millis(100));
        for _ in 0..9 {
            p.slower();
        }
        assert_eq!(p.speed_label(), "2.0s");
        assert_eq!(Playback::replay(1).with_speed_ms(50).speed_label(), "0.1s", "faster than every speed is the fastest");
        assert_eq!(Playback::replay(1).with_speed_ms(5000).speed_label(), "2.0s", "slower than every speed is the slowest");
    }

    fn glyphs(l: &Line) -> String {
        l.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn the_scrubber_puts_the_first_frame_left_the_last_right_and_marks_between() {
        set(Palette::terminal(ColorDepth::TrueColor));
        let at = |pos| glyphs(&Scrubber { len: 5, pos, marks: &[2], notes: &[] }.line(14));
        assert_eq!(at(0), "●───┼───── 1/5");
        assert_eq!(at(4), "━━━━┼━━━━● 5/5");
        assert_eq!(at(2), "━━━━●───── 3/5", "the position hides a mark under it");
        assert_eq!(glyphs(&Scrubber { len: 0, pos: 0, marks: &[], notes: &[] }.line(8)), "●─── 0/0");
        assert_eq!(glyphs(&Scrubber { len: 3, pos: 1, marks: &[], notes: &[] }.line(3)), " 2/3", "no room, no track");
        // A note shows over a mark in the same cell, and under the position.
        let noted = |pos| glyphs(&Scrubber { len: 5, pos, marks: &[2], notes: &[2, 3] }.line(14));
        assert_eq!(noted(0), "●───⊝─⊝─── 1/5");
        assert_eq!(noted(3), "━━━━⊝━●─── 4/5");
    }

    #[test]
    fn the_key_bar_puts_the_right_part_at_the_right_edge_when_it_fits() {
        set(Palette::terminal(ColorDepth::TrueColor));
        let l = key_bar(Shape::PLAIN, "pick", Ground::Accent, &[("q", "quit")], vec![Span::raw("3/9")], 24);
        assert_eq!(glyphs(&l), " pick   q quit       3/9");
        let tight = key_bar(Shape::PLAIN, "pick", Ground::Accent, &[("q", "quit")], vec![Span::raw("3/9")], 14);
        assert_eq!(glyphs(&tight), " pick   q quit", "dropped when it does not fit");
    }
}
