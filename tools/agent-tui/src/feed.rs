//! A feed: entries in arrival order, each a boxed label of fixed width
//! beside a body wrapped to the rest of the row, with a blank row after
//! it. The newest entry sits at the bottom. While the feed is taller than
//! its area it shows the newest rows, and a scroll, counted in rows from
//! the bottom, moves back through the older ones; shorter, it starts at
//! the top.
//!
//! Drawing costs what is on screen, not what the feed holds: the wrapped
//! bodies and the heights are kept in [`FeedState`] until the entries'
//! generation or the width changes, and only the visible rows of an entry
//! are drawn. Heights are counted in `usize`, so an entry of any length can
//! be scrolled to its end.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Block, BorderType, Padding, Paragraph, StatefulWidget, Widget};

use crate::wrap::{str_width, truncate, wrap_lines};

/// One entry: the label's lines, drawn inside a rounded box, and the body's
/// lines, wrapped. The body starts on the label's first text row.
#[derive(Debug, Clone, Default)]
pub struct Entry {
    pub label: Vec<Line<'static>>,
    pub body: Vec<Line<'static>>,
}

/// The entries laid out at one width.
#[derive(Debug, Clone, Default)]
struct Laid {
    generation: Option<u64>,
    width: u16,
    bodies: Vec<Vec<Line<'static>>>,
    heights: Vec<usize>,
    total: usize,
}

/// How far the feed is scrolled back, in rows from the bottom, and the
/// layout kept between frames. Drawing keeps the scroll inside what the
/// entries allow.
#[derive(Debug, Clone, Default)]
pub struct FeedState {
    pub scroll: usize,
    laid: Laid,
    /// How many times the entries were laid out, for tests of the cache.
    layouts: usize,
}

impl FeedState {
    /// How many times the entries have been wrapped and measured.
    pub fn layouts(&self) -> usize {
        self.layouts
    }
}

pub struct Feed<'a> {
    entries: &'a [Entry],
    /// The label box's width, borders included.
    label_width: u16,
    border: Style,
    /// Changes whenever the entries do; `None` lays them out every frame.
    generation: Option<u64>,
}

impl<'a> Feed<'a> {
    pub fn new(entries: &'a [Entry]) -> Feed<'a> {
        Feed { entries, label_width: 20, border: Style::new(), generation: None }
    }

    pub fn label_width(mut self, w: u16) -> Feed<'a> {
        self.label_width = w.max(4);
        self
    }

    /// The label box's border.
    pub fn border(mut self, s: Style) -> Feed<'a> {
        self.border = s;
        self
    }

    /// The entries' generation: while it and the width stay the same, the
    /// layout from the last frame is reused. Bump it whenever the entries
    /// change.
    pub fn generation(mut self, g: u64) -> Feed<'a> {
        self.generation = Some(g);
        self
    }

    /// The body's columns at a feed `width`: the row less the box and a
    /// column of padding either side.
    fn body_width(&self, width: u16) -> usize {
        (width.saturating_sub(self.label_width) as usize).saturating_sub(2).max(1)
    }

    /// An entry's height, the blank row after it included.
    fn height(e: &Entry, body_rows: usize) -> usize {
        (e.label.len() + 2).max(body_rows + 1) + 1
    }

    fn lay_out(&self, width: u16) -> Laid {
        let bw = self.body_width(width);
        let bodies: Vec<Vec<Line<'static>>> = self.entries.iter().map(|e| wrap_lines(&e.body, bw)).collect();
        let heights: Vec<usize> = self.entries.iter().zip(&bodies).map(|(e, b)| Feed::height(e, b.len())).collect();
        let total = heights.iter().sum();
        Laid { generation: self.generation, width, bodies, heights, total }
    }

    /// Every row the entries take at `width`.
    pub fn total_height(&self, width: u16) -> usize {
        self.lay_out(width).total
    }

    /// Draw rows `rows` of entry `e` (entry-relative) at screen row `y`.
    fn draw_rows(&self, e: &Entry, body: &[Line<'static>], area: Rect, y: u16, rows: std::ops::Range<usize>, buf: &mut Buffer) {
        let (from, to) = (rows.start, rows.end);
        let lw = self.label_width.min(area.width);
        let label_h = e.label.len() + 2;
        // The label box: small, so drawn whole off screen and copied in.
        if from < label_h {
            let interior = lw.saturating_sub(4) as usize;
            let label: Vec<Line<'static>> = e
                .label
                .iter()
                .map(|l| {
                    let text: String = l.spans.iter().map(|s| s.content.as_ref()).collect();
                    if str_width(&text) <= interior {
                        l.clone()
                    } else {
                        Line::styled(truncate(&text, interior), l.spans.first().map_or(Style::new(), |s| s.style))
                    }
                })
                .collect();
            let boxed = Rect::new(0, 0, lw, label_h.min(u16::MAX as usize) as u16);
            let mut scratch = Buffer::empty(boxed);
            let block = Block::bordered().border_type(BorderType::Rounded).border_style(self.border).padding(Padding::horizontal(1));
            Paragraph::new(label).block(block).render(boxed, &mut scratch);
            for row in from..to.min(label_h) {
                let dst = y + (row - from) as u16;
                for x in 0..lw {
                    buf[(area.x + x, dst)] = scratch[(x, row as u16)].clone();
                }
            }
        }
        // The body: its rows 1..=len, only those on screen.
        if area.width > lw + 2 {
            let (a, b) = (from.max(1), to.min(body.len() + 1));
            if a < b {
                let lines = body[a - 1..b - 1].to_vec();
                let at = Rect::new(area.x + lw + 1, y + (a - from) as u16, area.width - lw - 2, (b - a) as u16);
                Paragraph::new(lines).render(at, buf);
            }
        }
    }
}

impl StatefulWidget for Feed<'_> {
    type State = FeedState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut FeedState) {
        if area.is_empty() {
            return;
        }
        let fresh = self.generation.is_none() || state.laid.generation != self.generation || state.laid.width != area.width;
        if fresh {
            let laid = self.lay_out(area.width);
            // Paged back while entries arrive: keep the rows in view where
            // they are, rather than let the new ones push them up.
            if state.scroll > 0 && state.laid.width == area.width && laid.total > state.laid.total {
                state.scroll += laid.total - state.laid.total;
            }
            state.laid = laid;
            state.layouts += 1;
        }
        let laid = &state.laid;
        let view = area.height as usize;
        state.scroll = state.scroll.min(laid.total.saturating_sub(view));
        // The feed's row shown at the top of the area.
        let top = laid.total.saturating_sub(view + state.scroll);
        let mut y = 0usize;
        for (i, e) in self.entries.iter().enumerate() {
            let Some(&h) = laid.heights.get(i) else { break };
            let (start, end) = (y, y + h);
            y = end;
            if end <= top {
                continue;
            }
            if start >= top + view {
                break;
            }
            let (from, to) = (start.max(top) - start, end.min(top + view) - start);
            let dst = area.y + (start + from - top) as u16;
            self.draw_rows(e, &laid.bodies[i], area, dst, from..to, buf);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::rows;

    fn entry(label: &str, body: &str) -> Entry {
        Entry { label: vec![Line::from(label.to_string())], body: vec![Line::from(body.to_string())] }
    }

    fn draw(entries: &[Entry], w: u16, h: u16, scroll: usize) -> (Vec<String>, usize) {
        let mut buf = Buffer::empty(Rect::new(0, 0, w, h));
        let mut st = FeedState { scroll, ..FeedState::default() };
        Feed::new(entries).label_width(8).render(buf.area, &mut buf, &mut st);
        (rows(&buf), st.scroll)
    }

    #[test]
    fn a_short_feed_starts_at_the_top_with_the_body_beside_the_label() {
        let (r, _) = draw(&[entry("ann", "hello")], 20, 6, 0);
        assert_eq!(r[0], "╭──────╮            ");
        assert_eq!(r[1], "│ ann  │ hello      ");
        assert_eq!(r[2], "╰──────╯            ");
        assert!(r[3].trim().is_empty());
    }

    #[test]
    fn a_tall_feed_shows_the_newest_rows_and_scrolls_back() {
        let es: Vec<Entry> = (0..4).map(|i| entry(&format!("e{i}"), "x")).collect();
        let (r, _) = draw(&es, 20, 4, 0);
        assert!(r[1].contains("e3"), "{r:?}");
        assert!(!r.iter().any(|l| l.contains("e0")));
        let (r, scroll) = draw(&es, 20, 4, 1000);
        assert_eq!(scroll, 12, "the scroll is kept inside the feed");
        assert!(r[1].contains("e0"), "{r:?}");
    }

    #[test]
    fn a_long_body_wraps_and_grows_the_entry() {
        let e = entry("ann", "one two three four");
        let f = Feed::new(std::slice::from_ref(&e)).label_width(8);
        assert_eq!(f.total_height(18), 5, "three body rows after a padding row, then a blank row");
        let (r, _) = draw(&[e], 18, 6, 0);
        assert_eq!(r[1], "│ ann  │ one two  ");
        assert_eq!(r[2], "╰──────╯ three    ");
        assert_eq!(r[3], "         four     ");
    }

    fn long(lines: usize) -> Entry {
        let body = (0..lines).map(|i| Line::from(format!("line {i}"))).collect();
        Entry { label: vec![Line::from("log")], body }
    }

    #[test]
    fn an_entry_taller_than_u16_reaches_its_tail_and_its_head() {
        let es = [long(70_000)];
        let (r, _) = draw(&es, 30, 6, 0);
        assert!(r.iter().any(|l| l.contains("line 69999")), "the newest row of the feed is the entry's last: {r:?}");
        let (r, scroll) = draw(&es, 30, 6, usize::MAX);
        assert_eq!(scroll, 70_002 - 6);
        assert!(r[1].contains("│ log") && r[1].contains("line 0"), "{r:?}");
    }

    #[test]
    fn the_layout_is_kept_until_the_generation_or_the_width_changes() {
        let es = [long(20_000), entry("ann", "hello")];
        let mut st = FeedState::default();
        for _ in 0..50 {
            let mut buf = Buffer::empty(Rect::new(0, 0, 40, 10));
            Feed::new(&es).generation(7).render(buf.area, &mut buf, &mut st);
        }
        assert_eq!(st.layouts(), 1, "fifty frames of an unchanged feed wrap it once");
        let mut buf = Buffer::empty(Rect::new(0, 0, 41, 10));
        Feed::new(&es).generation(7).render(buf.area, &mut buf, &mut st);
        assert_eq!(st.layouts(), 2, "a new width lays it out again");
        Feed::new(&es).generation(8).render(buf.area, &mut buf, &mut st);
        assert_eq!(st.layouts(), 3, "a new generation lays it out again");
    }

    #[test]
    fn paged_back_the_view_stays_put_as_entries_arrive() {
        let mut es: Vec<Entry> = (0..6).map(|i| entry(&format!("e{i}"), "x")).collect();
        let mut st = FeedState { scroll: 8, ..FeedState::default() };
        let frame = |es: &[Entry], g: u64, st: &mut FeedState| {
            let mut buf = Buffer::empty(Rect::new(0, 0, 20, 4));
            Feed::new(es).generation(g).render(buf.area, &mut buf, st);
            rows(&buf)
        };
        let before = frame(&es, 1, &mut st);
        es.push(entry("new", "y"));
        assert_eq!(frame(&es, 2, &mut st), before, "the rows in view do not move");
        let mut st = FeedState::default();
        frame(&es[..6], 1, &mut st);
        assert!(frame(&es, 2, &mut st)[1].contains("new"), "at the bottom the feed follows the newest");
    }
}
