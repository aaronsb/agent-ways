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
        if text.is_empty() {
            return String::new();
        }
        let body = text.strip_suffix('\n');
        let lines: Vec<&str> = body.unwrap_or(text).split('\n').collect();
        let cols = width.min(u16::MAX as usize) as u16;
        let rows = lines.len().min(u16::MAX as usize) as u16;
        let (Some(ground), Some(roles)) = (self.ground(cols, rows).filter(|_| cols > 0), self.roles()) else { return text.to_string() };
        let fg = Color::Rgb(roles.body).at(self.depth());
        let mut out = String::with_capacity(text.len() * 2);
        let mut open = Vec::new();
        for (y, line) in lines.iter().enumerate() {
            if y > 0 {
                out.push('\n');
            }
            Band { ground: &ground, fg, row: y as u16, width, last: None, own: false, open: &mut open, out: &mut out }.line(line);
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
    /// The input's SGR parameters since its last reset, which the next line
    /// opens with again.
    open: &'a mut Vec<String>,
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
        let e = sgr_effect(params);
        let rest = match e.reset {
            Some(at) => {
                self.out.push_str(RESET);
                self.open.clear();
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
        self.open.push(rest.to_string());
        match e.bg {
            Chan::Default => {
                self.own = false;
                self.ground_at(col);
            }
            Chan::Own => {
                self.own = true;
                self.last = None;
            }
            Chan::Untouched => {}
        }
        if e.fg == Chan::Default {
            self.body();
        }
    }

    fn line(mut self, line: &str) {
        let mut col = 0usize;
        self.base(0);
        for params in std::mem::take(self.open) {
            self.style(&params, 0);
        }
        let mut chars = line.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\x1b' {
                let seq = escape(c, &mut chars);
                match seq.strip_prefix("\x1b[").and_then(|s| s.strip_suffix('m')).filter(|p| p.bytes().all(|b| b.is_ascii_digit() || b == b';' || b == b':')) {
                    Some(params) => self.style(params, col),
                    None => self.out.push_str(&seq),
                }
                continue;
            }
            // A carriage return would send the padding over the line.
            if c == '\r' {
                continue;
            }
            let (text, w) = if c == '\t' { (" ".repeat(8 - col % 8), 8 - col % 8) } else { (c.to_string(), c.width().unwrap_or(0)) };
            // A wide character that does not fit the row's end goes to the
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
            self.out.push_str(&text);
            col += w;
        }
        if self.own {
            self.own = false;
            self.base(col);
        }
        // Out to the end of the last row the line takes.
        let end = col.div_ceil(self.width).max(1) * self.width;
        self.pad(col, end);
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

/// What an SGR leaves on one channel, foreground or ground.
#[derive(Debug, PartialEq)]
enum Chan {
    /// The terminal's own colour (39, 49): the theme's goes back.
    Default,
    /// A colour of the span's own.
    Own,
    Untouched,
}

/// What an SGR's parameters do: where the parameters after its last reset
/// begin, and what those leave on each channel.
#[derive(Debug, PartialEq)]
struct Effect {
    reset: Option<usize>,
    fg: Chan,
    bg: Chan,
}

fn sgr_effect(params: &str) -> Effect {
    let mut e = Effect { reset: None, fg: Chan::Untouched, bg: Chan::Untouched };
    let ps: Vec<&str> = params.split(';').collect();
    let (mut i, mut at) = (0, 0);
    while i < ps.len() {
        let p = ps[i];
        at += p.len() + 1;
        i += 1;
        let head = p.split(':').next().unwrap_or("");
        match head.parse::<u16>().ok().or(head.is_empty().then_some(0)) {
            Some(0) => e = Effect { reset: Some(at.min(params.len())), fg: Chan::Untouched, bg: Chan::Untouched },
            Some(39) => e.fg = Chan::Default,
            Some(49) => e.bg = Chan::Default,
            Some(n @ (38 | 48)) => {
                // An extended colour: 5;n or 2;r;g;b, unless its colon form
                // carries them in this parameter.
                if !p.contains(':') {
                    let skip = if ps.get(i) == Some(&"5") { 2 } else { 4 };
                    for p in ps.iter().skip(i).take(skip) {
                        at += p.len() + 1;
                    }
                    i += skip;
                }
                if n == 38 {
                    e.fg = Chan::Own;
                } else {
                    e.bg = Chan::Own;
                }
            }
            Some(30..=37 | 90..=97) => e.fg = Chan::Own,
            Some(40..=47 | 100..=107) => e.bg = Chan::Own,
            _ => {}
        }
    }
    e
}
