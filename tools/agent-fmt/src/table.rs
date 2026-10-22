//! Shared ANSI table formatter for terminal output.
//!
//! Usage:
//!   let mut t = Table::new(&["Way", "Score", "Description"]);
//!   t.align(1, Align::Right);
//!   t.add(vec!["softwaredev/code/testing", "8.96", "unit testing..."]);
//!   t.print();
//!
//! Terminal width is auto-detected via ioctl (Unix) or $COLUMNS. A table
//! too wide narrows its text columns before its right-aligned (number)
//! columns, and spare width goes to the first column.

use crate::width::{truncate_visible, visible_len};

/// Column alignment.
#[derive(Clone, Copy)]
pub enum Align {
    Left,
    Right,
}

pub struct Table {
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
    aligns: Vec<Align>,
    max_widths: Vec<usize>,
    cap_widths: Vec<Option<usize>>,
    indent: usize,
    auto_fit: bool,
}

/// Detect terminal width via ioctl. Tries stderr first (most reliable when
/// stdout is piped), then stdout, then stdin. Falls back to $COLUMNS, then 80.
pub fn terminal_width() -> usize {
    #[cfg(unix)]
    {
        use std::mem::MaybeUninit;
        // Try all three fds — stderr is most likely to be the real terminal
        for fd in [libc::STDERR_FILENO, libc::STDOUT_FILENO, libc::STDIN_FILENO] {
            unsafe {
                let mut ws = MaybeUninit::<libc::winsize>::zeroed();
                if libc::ioctl(fd, libc::TIOCGWINSZ, ws.as_mut_ptr()) == 0 {
                    let ws = ws.assume_init();
                    if ws.ws_col > 0 {
                        return ws.ws_col as usize;
                    }
                }
            }
        }
    }

    // Fallback: $COLUMNS env var (set by some shells)
    if let Ok(cols) = std::env::var("COLUMNS") {
        if let Ok(w) = cols.parse::<usize>() {
            if w > 0 {
                return w;
            }
        }
    }

    80
}

impl Table {
    pub fn new(headers: &[&str]) -> Self {
        let n = headers.len();
        let headers: Vec<String> = headers.iter().map(|s| s.to_string()).collect();
        let max_widths: Vec<usize> = headers.iter().map(|h| h.len()).collect();
        Table {
            headers,
            rows: Vec::new(),
            aligns: vec![Align::Left; n],
            max_widths,
            cap_widths: vec![None; n],
            indent: 2,
            auto_fit: true,
        }
    }

    /// Set alignment for a column.
    pub fn align(&mut self, col: usize, a: Align) {
        if col < self.aligns.len() {
            self.aligns[col] = a;
        }
    }

    /// Set left indent (spaces before each row).
    pub fn indent(&mut self, n: usize) {
        self.indent = n;
    }

    /// Set a maximum width cap for a column (truncates with ellipsis).
    pub fn max_width(&mut self, col: usize, width: usize) {
        if col < self.cap_widths.len() {
            self.cap_widths[col] = Some(width);
        }
    }

    /// Disable auto-fitting to terminal width.
    #[allow(dead_code)]
    pub fn no_auto_fit(&mut self) {
        self.auto_fit = false;
    }

    /// Add a row. Accepts anything that can be stringified.
    pub fn add(&mut self, cells: Vec<&str>) {
        let row: Vec<String> = cells.iter().map(|s| s.to_string()).collect();
        // Update max widths (measuring visible length, not ANSI codes)
        for (i, cell) in row.iter().enumerate() {
            if i < self.max_widths.len() {
                let visible = visible_len(cell);
                if visible > self.max_widths[i] {
                    self.max_widths[i] = visible;
                }
            }
        }
        self.rows.push(row);
    }

    /// Add a row from owned strings.
    pub fn add_owned(&mut self, cells: Vec<String>) {
        for (i, cell) in cells.iter().enumerate() {
            if i < self.max_widths.len() {
                let visible = visible_len(cell);
                if visible > self.max_widths[i] {
                    self.max_widths[i] = visible;
                }
            }
        }
        self.rows.push(cells);
    }

    /// Print the table to stdout.
    pub fn print(&self) {
        let pad = " ".repeat(self.indent);
        let ncols = self.headers.len();
        let widths = self.widths(self.auto_fit.then(terminal_width));

        // Header
        let mut header = String::new();
        for (i, h) in self.headers.iter().enumerate() {
            if i > 0 {
                header.push(' ');
            }
            let w = widths.get(i).copied().unwrap_or(10);
            header.push_str(&pad_cell(h, w, self.aligns.get(i).copied().unwrap_or(Align::Left)));
        }
        println!("{pad}{}", agent_theme::paint(agent_theme::Style::new().bold(), header));

        // Separator
        let total_width: usize = widths.iter().sum::<usize>() + ncols.saturating_sub(1);
        println!("{pad}{}", agent_theme::paint(agent_theme::Role::Muted, "─".repeat(total_width)));

        // Rows
        for row in &self.rows {
            let mut line = String::new();
            for (i, cell) in row.iter().enumerate() {
                if i > 0 {
                    line.push(' ');
                }
                let w = widths.get(i).copied().unwrap_or(10);
                let align = self.aligns.get(i).copied().unwrap_or(Align::Left);
                let truncated = truncate_visible(cell, w);
                line.push_str(&pad_cell(&truncated, w, align));
            }
            println!("{pad}{line}");
        }
    }

    /// Column widths: each column's widest cell within its cap, then, with a
    /// terminal width, fitted to it. A table too wide narrows its widest
    /// left-aligned column a cell at a time, capped or not, down to 4; a
    /// right-aligned column (a number) narrows only once every left-aligned
    /// one is at 4. Spare width goes to the first column.
    fn widths(&self, term_w: Option<usize>) -> Vec<usize> {
        let mut widths: Vec<usize> = self
            .max_widths
            .iter()
            .enumerate()
            .map(|(i, w)| self.cap_widths.get(i).copied().flatten().map_or(*w, |cap| (*w).min(cap)))
            .collect();
        let Some(term_w) = term_w else { return widths };
        if widths.is_empty() {
            return widths;
        }
        let available = term_w.saturating_sub(self.indent + widths.len() - 1);
        let mut total: usize = widths.iter().sum();
        let right = |i: usize| matches!(self.aligns.get(i), Some(Align::Right));
        while total > available {
            let pick = |want_right: bool| {
                (0..widths.len()).filter(|&i| right(i) == want_right && widths[i] > 4).max_by_key(|&i| widths[i])
            };
            let Some(i) = pick(false).or_else(|| pick(true)) else { break };
            widths[i] -= 1;
            total -= 1;
        }
        if total < available {
            widths[0] += available - total;
        }
        widths
    }

    /// Return the number of rows.
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Returns true if the table has no rows.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// Pad a cell to a width with the given alignment.
fn pad_cell(s: &str, width: usize, align: Align) -> String {
    let vlen = visible_len(s);
    if vlen >= width {
        return s.to_string();
    }
    let padding = width - vlen;
    match align {
        Align::Left => format!("{s}{}", " ".repeat(padding)),
        Align::Right => format!("{}{s}", " ".repeat(padding)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Jaccard shape: two capped name columns and a right-aligned value.
    fn pair_table(name: &str) -> Table {
        let mut t = Table::new(&["Way A", "Way B", "Jaccard"]);
        t.max_width(0, 36);
        t.max_width(1, 36);
        t.align(2, Align::Right);
        t.indent(0);
        t.add_owned(vec![name.into(), name.into(), "0.123".into()]);
        t
    }

    #[test]
    fn capped_text_columns_narrow_before_a_number() {
        let t = pair_table(&"x".repeat(60));
        let w = t.widths(Some(60));
        assert_eq!(w[2], 7, "the value keeps its width: {w:?}");
        assert_eq!(w.iter().sum::<usize>() + 2, 60, "{w:?}");
        assert!(w[0].abs_diff(w[1]) <= 1, "the names share the narrowing: {w:?}");
    }

    #[test]
    fn a_number_narrows_only_after_the_text_is_at_four() {
        let t = pair_table(&"x".repeat(60));
        // 16 columns less two separators leaves 14: the names stop at 4, the value takes 6.
        assert_eq!(t.widths(Some(16)), vec![4, 4, 6]);
        assert_eq!(t.widths(Some(14)), vec![4, 4, 4]);
    }

    #[test]
    fn spare_width_goes_to_the_first_column() {
        let t = pair_table("short");
        assert_eq!(t.widths(Some(40)), vec![26, 5, 7]);
        assert_eq!(t.widths(None), vec![5, 5, 7]);
    }
}
