//! A feed: entries in arrival order, each a boxed label of fixed width
//! beside a body wrapped to the rest of the row, with a blank row after
//! it. The newest entry sits at the bottom. While the feed is taller than
//! its area it shows the newest rows, and a scroll, counted in rows from
//! the bottom, moves back through the older ones; shorter, it starts at
//! the top.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Block, BorderType, Paragraph, StatefulWidget, Widget};

use crate::wrap::{truncate, wrap_lines};

/// One entry: the label's lines, drawn inside a rounded box, and the body's
/// lines, wrapped. The body starts on the label's first text row.
#[derive(Debug, Clone, Default)]
pub struct Entry {
    pub label: Vec<Line<'static>>,
    pub body: Vec<Line<'static>>,
}

/// How far the feed is scrolled back, in rows from the bottom. Drawing
/// keeps it inside what the entries allow.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FeedState {
    pub scroll: usize,
}

pub struct Feed<'a> {
    entries: &'a [Entry],
    /// The label box's width, borders included.
    label_width: u16,
    border: Style,
}

impl<'a> Feed<'a> {
    pub fn new(entries: &'a [Entry]) -> Feed<'a> {
        Feed { entries, label_width: 20, border: Style::new() }
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

    /// The body's columns at a feed `width`: the row less the box and a
    /// column of padding either side.
    fn body_width(&self, width: u16) -> usize {
        (width.saturating_sub(self.label_width) as usize).saturating_sub(2).max(1)
    }

    /// An entry's wrapped body and its height, the blank row after it
    /// included.
    fn layout(&self, e: &Entry, width: u16) -> (Vec<Line<'static>>, u16) {
        let body = wrap_lines(&e.body, self.body_width(width));
        let label = e.label.len() as u16 + 2;
        let h = label.max(body.len() as u16 + 1);
        (body, h + 1)
    }

    /// Every row the entries take at `width`.
    pub fn height(&self, width: u16) -> usize {
        self.entries.iter().map(|e| self.layout(e, width).1 as usize).sum()
    }

    /// Draw one entry into a buffer of its own height.
    fn draw_entry(&self, e: &Entry, body: Vec<Line<'static>>, width: u16, h: u16) -> Buffer {
        let mut buf = Buffer::empty(Rect::new(0, 0, width, h));
        let lw = self.label_width.min(width);
        let boxed = Rect::new(0, 0, lw, (e.label.len() as u16 + 2).min(h));
        let interior = lw.saturating_sub(4) as usize;
        let label: Vec<Line<'static>> = e
            .label
            .iter()
            .map(|l| {
                // Each label row is cut to the box; a styled row keeps its spans when it fits.
                let text: String = l.spans.iter().map(|s| s.content.as_ref()).collect();
                if crate::wrap::str_width(&text) <= interior {
                    l.clone()
                } else {
                    let st = l.spans.first().map_or(Style::new(), |s| s.style);
                    Line::styled(truncate(&text, interior), st)
                }
            })
            .collect();
        let block = Block::bordered().border_type(BorderType::Rounded).border_style(self.border).padding(ratatui::widgets::Padding::horizontal(1));
        Paragraph::new(label).block(block).render(boxed, &mut buf);
        if width > lw + 2 {
            let at = Rect::new(lw + 1, 1, width - lw - 2, h.saturating_sub(1));
            Paragraph::new(body).render(at, &mut buf);
        }
        buf
    }
}

impl StatefulWidget for Feed<'_> {
    type State = FeedState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut FeedState) {
        if area.is_empty() {
            return;
        }
        let laid: Vec<(Vec<Line<'static>>, u16)> = self.entries.iter().map(|e| self.layout(e, area.width)).collect();
        let total: usize = laid.iter().map(|(_, h)| *h as usize).sum();
        let view = area.height as usize;
        state.scroll = state.scroll.min(total.saturating_sub(view));
        // The feed's row shown at the top of the area.
        let top = total.saturating_sub(view + state.scroll);
        let mut y = 0usize;
        for (e, (body, h)) in self.entries.iter().zip(laid) {
            let (start, end) = (y, y + h as usize);
            y = end;
            if end <= top || start >= top + view {
                continue;
            }
            let entry = self.draw_entry(e, body, area.width, h);
            for row in start.max(top)..end.min(top + view) {
                let (src, dst) = ((row - start) as u16, area.y + (row - top) as u16);
                for x in 0..area.width {
                    buf[(area.x + x, dst)] = entry[(x, src)].clone();
                }
            }
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
        let mut st = FeedState { scroll };
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
        assert!(r[1].contains("e3") || r[0].contains("e3") || r[2].contains("e3"), "{r:?}");
        assert!(!r.iter().any(|l| l.contains("e0")));
        let (r, scroll) = draw(&es, 20, 4, 1000);
        assert_eq!(scroll, 12, "the scroll is kept inside the feed");
        assert!(r[1].contains("e0"), "{r:?}");
    }

    #[test]
    fn a_long_body_wraps_and_grows_the_entry() {
        let e = entry("ann", "one two three four");
        let f = Feed::new(std::slice::from_ref(&e)).label_width(8);
        assert_eq!(f.height(18), 5, "three body rows after a padding row, then a blank row");
        let (r, _) = draw(&[e], 18, 6, 0);
        assert_eq!(r[1], "│ ann  │ one two  ");
        assert_eq!(r[2], "╰──────╯ three    ");
        assert_eq!(r[3], "         four     ");
    }
}
