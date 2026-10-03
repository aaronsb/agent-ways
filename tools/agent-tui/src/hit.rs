//! Hit-testing what a frame drew, so each screen maps a click to its own
//! action and the mouse reads the same on every screen (#739): a click
//! on a row selects it, a click on the selected row acts on it as Enter
//! does, and the wheel moves the selection as the arrows do.

use ratatui::layout::{Position, Rect};

/// The row of a bordered list or table under `at`. `area` is the widget's
/// rect, its border included; `header` the rows its header takes (1 for
/// a table's, 0 for a list); `offset` the first row shown, from the
/// widget's state. Rows are one line each. `None` off the rows.
pub fn row_at(area: Rect, at: Position, header: u16, offset: usize) -> Option<usize> {
    let inner = Rect { x: area.x + 1, y: area.y + 1 + header, width: area.width.saturating_sub(2), height: area.height.saturating_sub(2 + header) };
    inner.contains(at).then(|| offset + (at.y - inner.y) as usize)
}

/// The row under `at` of a bordered table whose rows take `heights` lines
/// each, as [`row_at`] finds one of a table of one-line rows. `None` off
/// the rows and below the last.
pub fn row_in(area: Rect, at: Position, header: u16, offset: usize, heights: &[u16]) -> Option<usize> {
    let first = row_at(area, at, header, 0)?;
    let mut top = 0usize;
    for (i, h) in heights.iter().enumerate().skip(offset) {
        top += *h as usize;
        if first < top {
            return Some(i);
        }
    }
    None
}

/// A click on row `i` of `len` rows whose selection is `sel`: the row
/// selected, and whether it was the selected one already, which a click
/// acts on. A row past the end selects nothing.
pub fn pick(sel: &mut usize, i: usize, len: usize) -> bool {
    if i >= len {
        return false;
    }
    let again = *sel == i;
    *sel = i;
    again
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_is_found_inside_the_border_and_below_the_header() {
        let area = Rect::new(2, 3, 20, 6);
        assert_eq!(row_at(area, Position::new(5, 3), 1, 0), None, "the border");
        assert_eq!(row_at(area, Position::new(5, 4), 1, 0), None, "the header");
        assert_eq!(row_at(area, Position::new(5, 5), 1, 0), Some(0));
        assert_eq!(row_at(area, Position::new(5, 7), 1, 4), Some(6), "scrolled by the offset");
        assert_eq!(row_at(area, Position::new(5, 8), 1, 0), None, "the bottom border");
        assert_eq!(row_at(area, Position::new(2, 5), 1, 0), None, "the left border");
        assert_eq!(row_at(area, Position::new(5, 4), 0, 0), Some(0), "a list has no header");
    }

    #[test]
    fn a_tall_row_takes_every_line_it_draws() {
        let area = Rect::new(0, 0, 20, 10);
        let heights = [2, 1, 2];
        let row = |y| row_in(area, Position::new(3, y), 1, 0, &heights);
        assert_eq!([row(2), row(3), row(4), row(5), row(6), row(7)], [Some(0), Some(0), Some(1), Some(2), Some(2), None]);
        assert_eq!(row_in(area, Position::new(3, 2), 1, 1, &heights), Some(1), "scrolled past the first");
    }

    #[test]
    fn a_click_selects_and_a_second_acts() {
        let mut sel = 0;
        assert!(!pick(&mut sel, 2, 3));
        assert_eq!(sel, 2);
        assert!(pick(&mut sel, 2, 3));
        assert!(!pick(&mut sel, 5, 3), "past the end");
        assert_eq!(sel, 2);
    }
}
