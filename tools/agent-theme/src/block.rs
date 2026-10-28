//! A report printed as a band of the theme: under `THEME_BACKGROUND="fill"`
//! every line of it sits on the theme's ground, out to the terminal's width.

use unicode_segmentation::UnicodeSegmentation;

use crate::color::Color;
use crate::paint::{painter, Ground, Painter, RESET};
use crate::text::cell_width;

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
        if text.is_empty() {
            return String::new();
        }
        let body = text.strip_suffix('\n');
        let lines: Vec<&str> = body.unwrap_or(text).split('\n').collect();
        let cols = width.min(u16::MAX as usize) as u16;
        let rows = lines.len().min(u16::MAX as usize) as u16;
        let (Some(ground), Some(roles)) = (self.ground(cols, rows), self.roles()) else { return text.to_string() };
        let fg = Color::Rgb(roles.body).at(self.depth());
        let mut out = String::with_capacity(text.len() * 2);
        let mut pen = Pen::default();
        for (y, line) in lines.iter().enumerate() {
            if y > 0 {
                out.push('\n');
            }
            Band { ground: &ground, fg, row: y as u16, width, last: None, own: false, pen: &mut pen, out: &mut out }.line(line);
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
    /// Whether a span has set its own ground.
    own: bool,
    /// The style the input has open, which the next line opens with again.
    pen: &'a mut Pen,
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

    fn body(&mut self) {
        if let Some(c) = self.fg {
            self.sgr(&c.sgr_params(false));
        }
    }

    /// The line's base: body text on the ground at `col`.
    fn base(&mut self, col: usize) {
        self.body();
        self.ground_at(col);
    }

    /// Blank cells from `col` to `end`, on the theme's ground unless a span
    /// holds its own.
    fn pad(&mut self, mut col: usize, end: usize) {
        while col < end {
            if !self.own {
                self.ground_moves(col);
            }
            self.out.push(' ');
            col += 1;
        }
    }

    /// The input's SGR `params` at `col`. A reset brings the base back before
    /// the parameters after it; 49 brings the theme's ground back and 39 the
    /// body colour.
    fn style(&mut self, params: &str, col: usize) {
        let e = self.pen.apply(params);
        let rest = match e.reset {
            Some(at) => {
                self.out.push_str(RESET);
                self.own = false;
                self.base(col);
                &params[at..]
            }
            None => params,
        };
        if rest.is_empty() {
            return;
        }
        self.sgr(rest);
        if e.bg {
            self.own(col);
        }
        if e.fg && self.pen.fg.is_none() {
            self.body();
        }
    }

    /// Whether the pen holds a ground of its own; the theme's comes back at
    /// `col` when it does not.
    fn own(&mut self, col: usize) {
        self.own = self.pen.bg.is_some();
        if self.own {
            self.last = None;
        } else {
            self.ground_at(col);
        }
    }

    /// The pen the line before left open, as one SGR.
    fn replay(&mut self) {
        let params = self.pen.params();
        if !params.is_empty() {
            self.sgr(&params);
            if self.pen.bg.is_some() {
                self.own(0);
            }
        }
    }

    fn line(mut self, line: &str) {
        self.base(0);
        self.replay();
        let (mut col, mut rest) = (0, line);
        while let Some(at) = rest.find('\x1b') {
            col = self.text(&rest[..at], col);
            let (n, whole) = escape(&rest[at..]);
            let seq = &rest[at..at + n];
            match seq.strip_prefix("\x1b[").and_then(|s| s.strip_suffix('m')).filter(|p| p.bytes().all(|b| b.is_ascii_digit() || b == b';' || b == b':')) {
                Some(params) => self.style(params, col),
                None if whole => self.out.push_str(seq),
                None => {}
            }
            rest = &rest[at + n..];
        }
        col = self.text(rest, col);
        // The padding is ground alone; the pen stays open for the next line.
        if self.pen.inked() {
            self.out.push_str(RESET);
        }
        if self.pen.inked() || self.own {
            self.own = false;
            self.base(col);
        }
        // Out to the end of the last row the line takes.
        let end = col.div_ceil(self.width).max(1) * self.width;
        self.pad(col, end);
        self.out.push_str(RESET);
    }

    /// `text`, which holds no escape, from `col`, a grapheme at a time;
    /// the column after it.
    fn text(&mut self, text: &str, mut col: usize) -> usize {
        for g in text.graphemes(true) {
            // A carriage return would send the padding over the line.
            if g == "\r" {
                continue;
            }
            let w = match g {
                "\t" => 8 - col % 8,
                g if g.starts_with(char::is_control) => 0,
                g => cell_width(g),
            };
            // A wide grapheme that does not fit the row's end goes to the
            // next row, as the terminal puts it.
            if w <= self.width && col % self.width + w > self.width {
                let end = col.div_ceil(self.width) * self.width;
                self.pad(col, end);
                col = end;
            }
            // A horizontal gradient changes the ground cell by cell.
            if !self.own && w > 0 {
                self.ground_moves(col);
            }
            match g {
                "\t" => self.out.extend(std::iter::repeat_n(' ', w)),
                g => self.out.push_str(g),
            }
            col += w;
        }
        col
    }
}

/// The escape sequence at the start of `s`, which opens with ESC: its length
/// in bytes, and whether it is whole. A CSI runs to its final byte; an OSC,
/// DCS, SOS, PM or APC to BEL or ST; any other is ESC, its intermediates
/// (0x20 to 0x2F) and a final byte. A sequence the line's end or a byte it
/// cannot hold cuts off is not whole, and ends before that byte.
fn escape(s: &str) -> (usize, bool) {
    let b = s.as_bytes();
    let run = |from: usize, keep: fn(u8) -> bool| from + b[from..].iter().take_while(|c| keep(**c)).count();
    match b.get(1) {
        Some(b'[') => {
            let i = run(2, |c| (0x20..=0x3f).contains(&c));
            match b.get(i) {
                Some(0x40..=0x7e) => (i + 1, true),
                _ => (i, false),
            }
        }
        Some(b']' | b'P' | b'X' | b'^' | b'_') => {
            let i = run(2, |c| c != 0x07 && c != 0x1b);
            match (b.get(i), b.get(i + 1)) {
                (Some(0x07), _) => (i + 1, true),
                (Some(0x1b), Some(b'\\')) => (i + 2, true),
                _ => (i, false),
            }
        }
        _ => {
            let i = run(1, |c| (0x20..=0x2f).contains(&c));
            match b.get(i) {
                Some(0x30..=0x7e) => (i + 1, true),
                _ => (i, false),
            }
        }
    }
}

/// The style the input has open: what each line opens with again, as one
/// SGR. Each part holds the parameters that set it.
#[derive(Debug, Default, PartialEq)]
struct Pen {
    fg: Option<String>,
    bg: Option<String>,
    /// The underline's colour, 58.
    ul: Option<String>,
    /// Bold, dim, italic, underline, blink, reverse, hidden, strike.
    attrs: [Option<String>; 8],
}

/// What an SGR's parameters did: where those after its last reset begin,
/// and whether those touched the foreground or the ground.
#[derive(Debug, PartialEq)]
struct Effect {
    reset: Option<usize>,
    fg: bool,
    bg: bool,
}

impl Pen {
    /// Whether anything but a ground of its own is open.
    fn inked(&self) -> bool {
        self.fg.is_some() || self.ul.is_some() || self.attrs.iter().any(Option::is_some)
    }

    /// The parameters that open this style again, empty for none.
    fn params(&self) -> String {
        let parts = self.attrs.iter().chain([&self.fg, &self.bg, &self.ul]);
        parts.flatten().map(String::as_str).collect::<Vec<_>>().join(";")
    }

    /// The pen after `params`.
    fn apply(&mut self, params: &str) -> Effect {
        let mut e = Effect { reset: None, fg: false, bg: false };
        let ps: Vec<&str> = params.split(';').collect();
        let (mut i, mut at) = (0, 0);
        while i < ps.len() {
            let (start, p) = (i, ps[i]);
            at += p.len() + 1;
            i += 1;
            let mut sub = p.split(':');
            let head = sub.next().unwrap_or("");
            let Some(n) = head.parse::<u16>().ok().or(head.is_empty().then_some(0)) else { continue };
            if matches!(n, 38 | 48 | 58) && !p.contains(':') {
                // An extended colour: 5;n or 2;r;g;b, unless its colon form
                // carries them in this parameter.
                let skip = if ps.get(i) == Some(&"5") { 2 } else { 4 };
                for p in ps.iter().skip(i).take(skip) {
                    at += p.len() + 1;
                }
                i = (i + skip).min(ps.len());
            }
            let set = Some(ps[start..i].join(";"));
            match n {
                0 => {
                    *self = Pen::default();
                    e = Effect { reset: Some(at.min(params.len())), fg: false, bg: false };
                }
                1..=9 | 21 => {
                    // 4:0 is underline off.
                    let off = n == 4 && sub.next() == Some("0");
                    self.attrs[attr(n)] = if off { None } else { set };
                }
                22 => self.attrs[..2].fill(None),
                23..=25 | 27..=29 => self.attrs[attr(n - 20)] = None,
                30..=38 | 90..=97 => (e.fg, self.fg) = (true, set),
                39 => (e.fg, self.fg) = (true, None),
                40..=48 | 100..=107 => (e.bg, self.bg) = (true, set),
                49 => (e.bg, self.bg) = (true, None),
                58 => self.ul = set,
                59 => self.ul = None,
                _ => {}
            }
        }
        e
    }
}

/// The slot of attribute `n` in [`Pen::attrs`]: 6 is the fast blink and 21
/// the double underline.
fn attr(n: u16) -> usize {
    match n {
        1..=5 => n as usize - 1,
        6 => 4,
        7..=9 => n as usize - 2,
        _ => 3,
    }
}
