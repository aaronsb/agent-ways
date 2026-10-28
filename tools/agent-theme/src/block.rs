//! A report printed as a band of the theme: under `THEME_BACKGROUND="fill"`
//! every line of it sits on the theme's ground, out to the terminal's width.

use unicode_width::UnicodeWidthChar;

use crate::color::Color;
use crate::paint::{painter, Ground, Painter, RESET};

impl Painter {
    /// `text` with the theme's ground behind every line, padded with ground
    /// cells to `width` columns, and body as the colour of text that sets
    /// none. A line longer than `width` is padded out to the end of the row
    /// it wraps onto. A vertical gradient runs down the block's lines and a
    /// horizontal one across `width`; at 256 colours the ground is flat bg.
    /// A span that sets its own ground keeps it until it resets. Without a
    /// fill theme to paint (a terminal-background theme, 16 colours, no
    /// colour, plain output) `text` comes back as it was.
    pub fn block(&self, text: &str, width: usize) -> String {
        let body = text.strip_suffix('\n');
        let lines: Vec<&str> = body.unwrap_or(text).split('\n').collect();
        let cols = width.min(u16::MAX as usize) as u16;
        let rows = lines.len().min(u16::MAX as usize) as u16;
        let (Some(ground), Some(roles)) = (self.ground(cols, rows).filter(|_| cols > 0), self.roles()) else { return text.to_string() };
        let fg = Color::Rgb(roles.body).at(self.depth());
        let mut out = String::with_capacity(text.len() * 2);
        for (y, line) in lines.iter().enumerate() {
            if y > 0 {
                out.push('\n');
            }
            Band { ground: &ground, fg, row: y as u16, width, last: None, out: &mut out }.line(line);
        }
        if body.is_some() {
            out.push('\n');
        }
        out
    }
}

/// `text` as a band of the process painter's theme ([`Painter::block`]).
pub fn block(text: &str, width: usize) -> String {
    painter().block(text, width)
}

/// One line of a block being painted.
struct Band<'a> {
    ground: &'a Ground,
    fg: Option<Color>,
    row: u16,
    /// The terminal's columns; a longer line wraps onto further rows.
    width: usize,
    /// The ground last written.
    last: Option<Color>,
    out: &'a mut String,
}

impl Band<'_> {
    fn sgr(&mut self, params: &str) {
        self.out.push_str("\x1b[");
        self.out.push_str(params);
        self.out.push('m');
    }

    /// The ground under `col` of the line, which wraps every `width` cells.
    fn colour(&self, col: usize) -> Color {
        self.ground.at((col % self.width).min(u16::MAX as usize) as u16, self.row)
    }

    fn ground_at(&mut self, col: usize) {
        let c = self.colour(col);
        self.sgr(&c.sgr_params(true));
        self.last = Some(c);
    }

    /// The ground at `col`, written only when it differs from the last.
    fn ground_moves(&mut self, col: usize) {
        if self.last != Some(self.colour(col)) {
            self.ground_at(col);
        }
    }

    /// The line's base: body text on the ground at `col`.
    fn base(&mut self, col: usize) {
        if let Some(c) = self.fg {
            self.sgr(&c.sgr_params(false));
        }
        self.ground_at(col);
    }

    fn line(mut self, line: &str) {
        let mut col = 0usize;
        // Whether a span has set its own ground.
        let mut own = false;
        self.base(0);
        let mut chars = line.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\x1b' {
                let seq = escape(c, &mut chars);
                self.out.push_str(&seq);
                if let Some(params) = seq.strip_prefix("\x1b[").and_then(|s| s.strip_suffix('m')) {
                    match sgr_ground(params) {
                        SgrGround::Reset => {
                            own = false;
                            self.base(col);
                        }
                        SgrGround::Default => {
                            own = false;
                            self.ground_at(col);
                        }
                        SgrGround::Own => {
                            own = true;
                            self.last = None;
                        }
                        SgrGround::Untouched => {}
                    }
                }
                continue;
            }
            let (text, w) = if c == '\t' { (" ".repeat(8 - col % 8), 8 - col % 8) } else { (c.to_string(), c.width().unwrap_or(0)) };
            // A horizontal gradient changes the ground cell by cell.
            if !own && w > 0 {
                self.ground_moves(col);
            }
            self.out.push_str(&text);
            col += w;
        }
        if own {
            self.base(col);
        }
        // Out to the end of the last row the line takes.
        let end = col.div_ceil(self.width).max(1) * self.width;
        while col < end {
            self.ground_moves(col);
            self.out.push(' ');
            col += 1;
        }
        self.out.push_str(RESET);
    }
}

/// The whole escape sequence that `first` (ESC) opens: a CSI to its final
/// byte, an OSC to BEL or ST, or ESC and one more character.
fn escape(first: char, rest: &mut std::iter::Peekable<std::str::Chars>) -> String {
    let mut seq = String::from(first);
    match rest.next() {
        Some('[') => {
            seq.push('[');
            for c in rest.by_ref() {
                seq.push(c);
                if ('\x40'..='\x7e').contains(&c) {
                    break;
                }
            }
        }
        Some(']') => {
            seq.push(']');
            while let Some(c) = rest.next() {
                seq.push(c);
                if c == '\x07' {
                    break;
                }
                if c == '\x1b' && rest.peek() == Some(&'\\') {
                    seq.push(rest.next().unwrap_or('\\'));
                    break;
                }
            }
        }
        Some(c) => seq.push(c),
        None => {}
    }
    seq
}

/// What an SGR does to the ground.
#[derive(Debug, PartialEq)]
enum SgrGround {
    /// Everything off: the base goes back on.
    Reset,
    /// The terminal's own ground (49): the theme's goes back under it.
    Default,
    /// A span's own ground.
    Own,
    Untouched,
}

fn sgr_ground(params: &str) -> SgrGround {
    if params.is_empty() {
        return SgrGround::Reset;
    }
    let mut out = SgrGround::Untouched;
    let mut it = params.split(';');
    while let Some(p) = it.next() {
        match p {
            "0" => out = SgrGround::Reset,
            "49" => out = SgrGround::Default,
            "38" | "48" => {
                // An extended colour: 5;n or 2;r;g;b.
                let skip = if it.next() == Some("5") { 1 } else { 3 };
                for _ in 0..skip {
                    it.next();
                }
                if p == "48" {
                    out = SgrGround::Own;
                }
            }
            _ => {
                if let Ok(n) = p.parse::<u16>() {
                    if (40..=47).contains(&n) || (100..=107).contains(&n) {
                        out = SgrGround::Own;
                    }
                }
            }
        }
    }
    out
}
