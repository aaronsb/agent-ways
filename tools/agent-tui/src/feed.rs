//! A feed: entries in arrival order, each a header band across the row, a
//! rule under it, and a body wrapped to the full row, with a blank row
//! after it. The newest entry sits at the bottom. While the feed is taller
//! than its area it shows the newest rows, and a scroll, counted in rows
//! from the bottom, moves back through the older ones; shorter, it starts
//! at the top.
//!
//! The header is a lozenge in the shape the screen draws its lozenges in
//! ([`Shape`]), sized to the row: its first segment is kept whole, the ones
//! after it are cut from the last back when the row is short, and the last
//! fills the rest of the row. The rule under it is drawn heavy, in the
//! selected style, under the entry a click selected.
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
use ratatui::widgets::{Paragraph, StatefulWidget, Widget};

use crate::app::theme::{self, Seg, Shape};
use crate::wrap::{str_width, truncate, wrap_lines};

/// The rows an entry takes above its body: the header and the rule.
const HEAD_ROWS: usize = 2;

/// One entry: its header band's segments and the body's lines, wrapped to
/// the row.
#[derive(Debug, Clone, Default)]
pub struct Entry {
    pub header: Vec<Seg>,
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

/// How far the feed is scrolled back, in rows from the bottom, the entry
/// selected, and the layout kept between frames. Drawing keeps the scroll
/// inside what the entries allow.
#[derive(Debug, Clone, Default)]
pub struct FeedState {
    pub scroll: usize,
    /// The entry a click selected: its label box is drawn in the feed's
    /// selected style.
    pub selected: Option<usize>,
    laid: Laid,
    /// The feed's row at the top of the area, and the area's height, in
    /// the last frame: where a click lands.
    top: usize,
    view: usize,
    /// How many times the entries were laid out, for tests of the cache.
    layouts: usize,
}

impl FeedState {
    /// How many times the entries have been wrapped and measured.
    pub fn layouts(&self) -> usize {
        self.layouts
    }

    /// Where entry `i` starts and ends, in feed rows, its blank row left out.
    fn span(&self, i: usize) -> Option<(usize, usize)> {
        let h = *self.laid.heights.get(i)?;
        let start: usize = self.laid.heights[..i].iter().sum();
        Some((start, start + h.saturating_sub(1)))
    }

    /// The entry drawn at `row` of the area in the last frame; `None` on a
    /// blank row between entries or below them.
    pub fn entry_at(&self, row: u16) -> Option<usize> {
        self.entry_row(row).map(|(i, _)| i)
    }

    /// The entry drawn at `row` of the area in the last frame, and which of
    /// its rows it is: 0 the header, 1 the rule, then the body. `None` on a
    /// blank row between entries or below them.
    pub fn entry_row(&self, row: u16) -> Option<(usize, usize)> {
        if row as usize >= self.view {
            return None;
        }
        let at = self.top + row as usize;
        let mut start = 0usize;
        for (i, h) in self.laid.heights.iter().enumerate() {
            if at < start + h {
                // The blank row after an entry is no entry's.
                return (at + 1 < start + h).then_some((i, at - start));
            }
            start += h;
        }
        None
    }

    /// Select entry `i` and scroll it into view: whole when it fits, else
    /// from its first row.
    pub fn reveal(&mut self, i: usize) {
        let Some((start, end)) = self.span(i) else { return };
        self.selected = Some(i);
        let (total, view) = (self.laid.total, self.view);
        let top = if start < self.top || end - start > view {
            start
        } else if end > self.top + view {
            end - view
        } else {
            self.top
        };
        self.scroll = total.saturating_sub(view + top);
    }
}

pub struct Feed<'a> {
    entries: &'a [Entry],
    /// The headers' caps and joins.
    shape: Shape,
    /// The rule under a header.
    rule: Style,
    /// The rule under the selected entry's header.
    selected: Style,
    /// Changes whenever the entries do; `None` lays them out every frame.
    generation: Option<u64>,
}

impl<'a> Feed<'a> {
    pub fn new(entries: &'a [Entry]) -> Feed<'a> {
        Feed { entries, shape: Shape::PLAIN, rule: Style::new(), selected: Style::new(), generation: None }
    }

    /// The shape the header bands are drawn in.
    pub fn shape(mut self, shape: Shape) -> Feed<'a> {
        self.shape = shape;
        self
    }

    /// The rule under each header.
    pub fn rule(mut self, s: Style) -> Feed<'a> {
        self.rule = s;
        self
    }

    /// The rule under the selected entry's header ([`FeedState::selected`]),
    /// drawn heavy.
    pub fn selected(mut self, s: Style) -> Feed<'a> {
        self.selected = s;
        self
    }

    /// The entries' generation: while it and the width stay the same, the
    /// layout from the last frame is reused. Bump it whenever the entries
    /// change.
    pub fn generation(mut self, g: u64) -> Feed<'a> {
        self.generation = Some(g);
        self
    }

    /// An entry's height: the header, the rule, the body, and the blank
    /// row after it.
    fn height(body_rows: usize) -> usize {
        HEAD_ROWS + body_rows + 1
    }

    fn lay_out(&self, width: u16) -> Laid {
        let bw = usize::from(width).max(1);
        let bodies: Vec<Vec<Line<'static>>> = self.entries.iter().map(|e| wrap_lines(&e.body, bw)).collect();
        let heights: Vec<usize> = bodies.iter().map(|b| Feed::height(b.len())).collect();
        let total = heights.iter().sum();
        Laid { generation: self.generation, width, bodies, heights, total }
    }

    /// Every row the entries take at `width`.
    pub fn total_height(&self, width: u16) -> usize {
        self.lay_out(width).total
    }

    /// Draw rows `rows` of entry `e` (entry-relative) at screen row `y` of
    /// `area`: its header, the rule under it in `rule` (heavy when
    /// `selected`), and its body.
    fn draw_rows(&self, e: &Entry, body: &[Line<'static>], (area, y): (Rect, u16), rows: std::ops::Range<usize>, (rule, selected): (Style, bool), buf: &mut Buffer) {
        let (from, to) = (rows.start, rows.end);
        let at = |row: usize| Rect::new(area.x, y + (row - from) as u16, area.width, 1);
        if from == 0 && to > 0 {
            Paragraph::new(band(&e.header, self.shape, usize::from(area.width))).render(at(0), buf);
        }
        if (from..to).contains(&1) {
            let glyph = if selected { "━" } else { "─" };
            Paragraph::new(Line::styled(glyph.repeat(usize::from(area.width)), rule)).render(at(1), buf);
        }
        // The body: its rows HEAD_ROWS.., only those on screen.
        let (a, b) = (from.max(HEAD_ROWS), to.min(body.len() + HEAD_ROWS));
        if a < b {
            let lines = body[a - HEAD_ROWS..b - HEAD_ROWS].to_vec();
            Paragraph::new(lines).render(Rect::new(area.x, y + (a - from) as u16, area.width, (b - a) as u16), buf);
        }
    }
}

/// A header band `width` columns wide: `segs` as one lozenge in `shape`.
/// The first segment is kept whole; when the row is short the others are
/// cut, the last first, down to a few columns each, and then left out from
/// the last; when it is long the last is padded to fill the row. Without
/// colour the lozenge draws no glyphs, so they take no room.
pub fn band(segs: &[Seg], shape: Shape, width: usize) -> Line<'static> {
    if segs.is_empty() {
        return Line::default();
    }
    let bare = theme::colourless();
    let glyphs = |n: usize| if bare { 0 } else { usize::from(!shape.cap.is_empty()) + n * usize::from(!shape.join.is_empty()) };
    let mut segs: Vec<Seg> = segs.to_vec();
    // Left out from the last while even the shortest cuts cannot fit.
    const LEAST: usize = 4;
    while segs.len() > 1 && glyphs(segs.len()) + str_width(&segs[0].text) + (segs.len() - 1) * LEAST > width {
        segs.pop();
    }
    let room = width.saturating_sub(glyphs(segs.len()));
    let mut over = segs.iter().map(|s| str_width(&s.text)).sum::<usize>().saturating_sub(room);
    for s in segs.iter_mut().skip(1).rev() {
        if over == 0 {
            break;
        }
        let w = str_width(&s.text);
        let keep = w.saturating_sub(over).max(LEAST.min(w));
        over -= w - keep;
        s.text = format!("{} ", truncate(s.text.trim_end(), keep.saturating_sub(1)));
    }
    if over > 0 {
        // One segment wider than the row: cut it too.
        segs[0].text = truncate(&segs[0].text, room);
    }
    let used: usize = segs.iter().map(|s| str_width(&s.text)).sum();
    if let Some(last) = segs.last_mut() {
        last.text.push_str(&" ".repeat(room.saturating_sub(used)));
    }
    Line::from(shape.lozenge(&segs))
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
        state.top = top;
        state.view = view;
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
            let picked = state.selected == Some(i);
            let rule = if picked { self.selected } else { self.rule };
            self.draw_rows(e, &laid.bodies[i], (area, dst), from..to, (rule, picked), buf);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::rows;

    fn entry(label: &str, body: &str) -> Entry {
        Entry { header: vec![Seg::styled(format!(" {label} "), Style::new())], body: vec![Line::from(body.to_string())] }
    }

    fn draw(entries: &[Entry], w: u16, h: u16, scroll: usize) -> (Vec<String>, usize) {
        let mut buf = Buffer::empty(Rect::new(0, 0, w, h));
        let mut st = FeedState { scroll, ..FeedState::default() };
        Feed::new(entries).render(buf.area, &mut buf, &mut st);
        (rows(&buf), st.scroll)
    }

    #[test]
    fn a_short_feed_starts_at_the_top_with_a_header_a_rule_and_the_body() {
        let (r, _) = draw(&[entry("ann", "hello")], 20, 6, 0);
        assert_eq!(r[0], " ann                ");
        assert_eq!(r[1], "────────────────────");
        assert_eq!(r[2], "hello               ");
        assert!(r[3].trim().is_empty());
    }

    #[test]
    fn a_tall_feed_shows_the_newest_rows_and_scrolls_back() {
        let es: Vec<Entry> = (0..4).map(|i| entry(&format!("e{i}"), "x")).collect();
        let (r, _) = draw(&es, 20, 4, 0);
        assert!(r[0].contains("e3"), "{r:?}");
        assert!(!r.iter().any(|l| l.contains("e0")));
        let (r, scroll) = draw(&es, 20, 4, 1000);
        assert_eq!(scroll, 12, "the scroll is kept inside the feed");
        assert!(r[0].contains("e0"), "{r:?}");
    }

    #[test]
    fn a_long_body_wraps_to_the_full_row_and_grows_the_entry() {
        let e = entry("ann", "one two three four five");
        let f = Feed::new(std::slice::from_ref(&e));
        assert_eq!(f.total_height(10), 6, "header, rule, three body rows, a blank row");
        let (r, _) = draw(&[e], 10, 6, 0);
        assert_eq!(r[2], "one two   ");
        assert_eq!(r[3], "three four");
        assert_eq!(r[4], "five      ");
    }

    fn long(lines: usize) -> Entry {
        let body = (0..lines).map(|i| Line::from(format!("line {i}"))).collect();
        Entry { header: vec![Seg::styled(" log ", Style::new())], body }
    }

    #[test]
    fn an_entry_taller_than_u16_reaches_its_tail_and_its_head() {
        let es = [long(70_000)];
        let (r, _) = draw(&es, 30, 6, 0);
        assert!(r.iter().any(|l| l.contains("line 69999")), "the newest row of the feed is the entry's last: {r:?}");
        let (r, scroll) = draw(&es, 30, 6, usize::MAX);
        assert_eq!(scroll, 70_003 - 6);
        assert!(r[0].contains("log") && r[2].contains("line 0"), "{r:?}");
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

    /// A click finds the entry drawn on its row and which row of it, and
    /// revealing an entry cut off at the top scrolls it whole into view;
    /// the selected entry's rule is heavy, in the selected style.
    #[test]
    fn a_row_names_its_entry_and_a_reveal_brings_it_whole_into_view() {
        let es: Vec<Entry> = (0..4).map(|i| entry(&format!("e{i}"), "x")).collect();
        let mut st = FeedState::default();
        let frame = |st: &mut FeedState| {
            let mut buf = Buffer::empty(Rect::new(0, 0, 20, 6));
            Feed::new(&es).selected(Style::new().fg(ratatui::style::Color::Red)).render(buf.area, &mut buf, st);
            buf
        };
        let buf = frame(&mut st);
        // 16 rows of feed in 6: e2 is cut at the top, e3 whole below it.
        let r = rows(&buf);
        assert!(r[2].contains("e3"), "{r:?}");
        assert_eq!(st.entry_row(2), Some((3, 0)), "e3's header");
        assert_eq!(st.entry_row(4), Some((3, 2)), "e3's body");
        assert_eq!(st.entry_at(5), None, "the blank row after an entry");
        assert_eq!(st.entry_at(0), Some(2));
        st.reveal(2);
        let buf = frame(&mut st);
        let r = rows(&buf);
        assert!(r[0].contains("e2") && r[1].starts_with('━'), "e2 whole from the top, its rule heavy: {r:?}");
        assert_eq!(buf[(0, 1)].fg, ratatui::style::Color::Red, "the selected entry's rule");
        st.reveal(3);
        let r = rows(&frame(&mut st));
        assert_eq!(st.scroll, 1, "e3 cut at the bottom: scrolled down to its last row");
        assert!(r[3].contains("e3") && r[4].starts_with('━') && r[5].starts_with('x'), "{r:?}");
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
        assert!(frame(&es, 2, &mut st)[0].contains("new"), "at the bottom the feed follows the newest");
    }

    fn text(l: &Line) -> String {
        l.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    /// The band fills its row; on a short row it cuts the segments after
    /// the first, the last first, and never the first; with too little
    /// room it leaves the last ones out. A wide character counts as two.
    #[test]
    fn a_band_fills_its_row_and_cuts_all_but_its_first_segment() {
        let segs = |name: &str| vec![Seg::styled(" 10-10 21:46 ", Style::new()), Seg::styled(format!(" {name} "), Style::new()), Seg::styled(" a-long-project-name ", Style::new())];
        let wide = text(&band(&segs("Chaucer"), Shape::PLAIN, 60));
        assert_eq!(str_width(&wide), 60, "{wide:?}");
        assert!(wide.starts_with(" 10-10 21:46  Chaucer  a-long-project-name "), "{wide:?}");
        let short = text(&band(&segs("Chaucer-the-very-long-name"), Shape::PLAIN, 36));
        assert_eq!(str_width(&short), 36, "{short:?}");
        assert!(short.starts_with(" 10-10 21:46 "), "the time whole: {short:?}");
        assert!(short.contains('…'), "the rest cut: {short:?}");
        let tiny = text(&band(&segs("Chaucer"), Shape::PLAIN, 18));
        assert!(tiny.starts_with(" 10-10 21:46 ") && str_width(&tiny) <= 18, "{tiny:?}");
        let cjk = text(&band(&[Seg::styled(" t ", Style::new()), Seg::styled(" 数据数据数据 ", Style::new())], Shape::PLAIN, 10));
        assert_eq!(str_width(&cjk), 10, "{cjk:?}");
        let round = band(&segs("Chaucer"), Shape::ROUND, 60);
        assert_eq!(str_width(&text(&round)), 60, "the caps and joins take their room");
    }
}
