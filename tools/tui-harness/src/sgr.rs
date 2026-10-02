//! ANSI text from `tmux capture-pane -ep` to a grid of styled cells.
//!
//! tmux has already applied every cursor movement, so the capture is a
//! final-state buffer: printable text, newlines, and SGR sequences. Only SGR
//! is interpreted; any other escape sequence is consumed and dropped.

use unicode_width::UnicodeWidthChar;

/// An RGB colour.
pub type Rgb = [u8; 3];

/// The 16 basic colours, tuned to roughly match a dark-theme terminal.
pub const BASIC: [Rgb; 16] = [
    [0, 0, 0],
    [170, 0, 0],
    [0, 170, 0],
    [170, 85, 0],
    [0, 0, 170],
    [170, 0, 170],
    [0, 170, 170],
    [170, 170, 170],
    [85, 85, 85],
    [255, 85, 85],
    [85, 255, 85],
    [255, 255, 85],
    [85, 85, 255],
    [255, 85, 255],
    [85, 255, 255],
    [255, 255, 255],
];

/// Foreground used when no colour is set.
pub const DEFAULT_FG: Rgb = [200, 200, 200];
/// Background used when no colour is set.
pub const DEFAULT_BG: Rgb = [15, 15, 15];

/// Entry `idx` of the xterm 256-colour palette: 16 basic, a 6x6x6 cube,
/// then 24 greys.
pub fn palette256(idx: u8) -> Rgb {
    const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
    match idx {
        0..=15 => BASIC[idx as usize],
        16..=231 => {
            let i = idx - 16;
            [
                LEVELS[(i / 36) as usize],
                LEVELS[(i / 6 % 6) as usize],
                LEVELS[(i % 6) as usize],
            ]
        }
        _ => {
            let v = 8 + 10 * (idx - 232);
            [v, v, v]
        }
    }
}

/// The SGR state of one cell. A colour of `None` means the terminal default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Style {
    pub fg: Option<Rgb>,
    pub bg: Option<Rgb>,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub reverse: bool,
}

impl Style {
    /// Apply one SGR parameter list, given as flat numbers (the `;` form).
    pub fn apply_sgr(&mut self, params: &[u16]) {
        if params.is_empty() {
            *self = Style::default();
            return;
        }
        let mut i = 0;
        while i < params.len() {
            let p = params[i];
            match p {
                0 => *self = Style::default(),
                1 => self.bold = true,
                2 => self.dim = true,
                3 => self.italic = true,
                4 => self.underline = true,
                7 => self.reverse = true,
                22 => {
                    self.bold = false;
                    self.dim = false;
                }
                23 => self.italic = false,
                24 => self.underline = false,
                27 => self.reverse = false,
                30..=37 => self.fg = Some(BASIC[(p - 30) as usize]),
                39 => self.fg = None,
                40..=47 => self.bg = Some(BASIC[(p - 40) as usize]),
                49 => self.bg = None,
                90..=97 => self.fg = Some(BASIC[(p - 90 + 8) as usize]),
                100..=107 => self.bg = Some(BASIC[(p - 100 + 8) as usize]),
                38 | 48 => {
                    if let Some((colour, used)) = extended_colour(&params[i + 1..]) {
                        if p == 38 {
                            self.fg = Some(colour);
                        } else {
                            self.bg = Some(colour);
                        }
                        i += used;
                    }
                }
                _ => {}
            }
            i += 1;
        }
    }
}

/// Decode the tail of a `38`/`48` sequence: `5;n` or `2;r;g;b`. Returns the
/// colour and how many parameters it consumed.
fn extended_colour(rest: &[u16]) -> Option<(Rgb, usize)> {
    match rest {
        [5, n, ..] => Some((palette256((*n & 0xFF) as u8), 2)),
        [2, r, g, b, ..] => Some(([clamp_u8(*r), clamp_u8(*g), clamp_u8(*b)], 4)),
        _ => None,
    }
}

fn clamp_u8(v: u16) -> u8 {
    v.min(255) as u8
}

/// One screen cell. `ch` is `None` for the right half of a wide character.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub ch: Option<char>,
    pub style: Style,
}

/// Rows of cells, as parsed. Rows may differ in length.
pub type Grid = Vec<Vec<Cell>>;

struct Builder {
    rows: Grid,
    style: Style,
}

impl vte::Perform for Builder {
    fn print(&mut self, c: char) {
        let width = c.width().unwrap_or(0);
        if width == 0 {
            // Combining marks and other zero-width characters would push the
            // rest of the row out of its column, so they are dropped.
            return;
        }
        let row = self.rows.last_mut().expect("builder always holds a row");
        row.push(Cell {
            ch: Some(c),
            style: self.style,
        });
        if width == 2 {
            row.push(Cell {
                ch: None,
                style: self.style,
            });
        }
    }

    fn execute(&mut self, byte: u8) {
        // Every other control character is dropped, as render.py does.
        if byte == b'\n' {
            self.rows.push(Vec::new());
        }
    }

    fn csi_dispatch(
        &mut self,
        params: &vte::Params,
        intermediates: &[u8],
        ignore: bool,
        action: char,
    ) {
        if action != 'm' || ignore || !intermediates.is_empty() {
            return;
        }
        // Flatten the `:` sub-parameter form (`38:2::r:g:b`) into the `;`
        // form, dropping the colour-space slot that the colon form carries.
        let mut flat: Vec<u16> = Vec::new();
        for group in params.iter() {
            match group {
                [38 | 48, 2, _cs, r, g, b] => flat.extend_from_slice(&[group[0], 2, *r, *g, *b]),
                _ => flat.extend_from_slice(group),
            }
        }
        self.style.apply_sgr(&flat);
    }
}

/// Parse a capture into rows of cells. A single trailing newline (tmux ends
/// every captured line with one) does not open an extra row. SGR state
/// carries across lines.
pub fn parse(text: &str) -> Grid {
    let mut builder = Builder {
        rows: vec![Vec::new()],
        style: Style::default(),
    };
    let mut parser = vte::Parser::new();
    parser.advance(&mut builder, text.as_bytes());
    if builder.rows.len() > 1
        && builder.rows.last().is_some_and(Vec::is_empty)
        && text.ends_with('\n')
    {
        builder.rows.pop();
    }
    builder.rows
}

/// Strip every escape sequence and return the plain text of a capture.
pub fn plain_text(grid: &Grid) -> String {
    grid.iter()
        .map(|row| row.iter().filter_map(|c| c.ch).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Test input: `^` stands for ESC, so fixture captures read as escape
/// sequences without being raw SGR literals in the source (ADR-504 §6 lints
/// those; here they are parser input, not output colour).
#[cfg(test)]
pub(crate) fn esc(s: &str) -> String {
    s.replace('^', "\x1b")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fixed capture covering basic, bright, 256 and truecolor fg and bg,
    /// the attributes, partial resets, a full reset, and a wide character.
    const CAPTURE: &str = "^[31mR^[1;92mG^[22m^[38;5;196mx^[48;5;244my^[0m.\n\
^[38;2;10;20;30;48;2;200;100;50mT^[39;49m ^[2;3;4;7mA^[27;24;23mB^[m\u{4e2d}z\n\
^[38:2::1:2:3mC^[?25l^[44mD\n";

    #[test]
    fn parses_fixed_capture() {
        let g = parse(&esc(CAPTURE));
        assert_eq!(g.len(), 3, "trailing newline must not add a row");

        let r0 = &g[0];
        assert_eq!(plain_text(&vec![r0.clone()]), "RGxy.");
        assert_eq!(r0[0].style.fg, Some(BASIC[1]));
        assert!(!r0[0].style.bold);
        assert_eq!(r0[1].style.fg, Some(BASIC[10]));
        assert!(r0[1].style.bold);
        // 22 clears bold, leaves the colour.
        assert!(!r0[2].style.bold);
        assert_eq!(r0[2].style.fg, Some([255, 0, 0]));
        assert_eq!(r0[3].style.bg, Some([128, 128, 128]));
        assert_eq!(r0[3].style.fg, Some([255, 0, 0]));
        assert_eq!(r0[4].style, Style::default());

        let r1 = &g[1];
        assert_eq!(r1[0].style.fg, Some([10, 20, 30]));
        assert_eq!(r1[0].style.bg, Some([200, 100, 50]));
        assert_eq!(r1[1].style.fg, None);
        assert_eq!(r1[1].style.bg, None);
        let a = r1[2].style;
        assert!(a.dim && a.italic && a.underline && a.reverse && !a.bold);
        let b = r1[3].style;
        assert!(b.dim && !b.italic && !b.underline && !b.reverse);
        // The wide character takes two cells, the second a filler.
        assert_eq!(r1[4].ch, Some('\u{4e2d}'));
        assert_eq!(r1[5].ch, None);
        assert_eq!(r1[5].style, Style::default());
        assert_eq!(r1[6].ch, Some('z'));
        assert_eq!(r1.len(), 7);

        let r2 = &g[2];
        assert_eq!(r2[0].style.fg, Some([1, 2, 3]));
        // A non-SGR CSI is dropped without touching style.
        assert_eq!(r2[1].ch, Some('D'));
        assert_eq!(r2[1].style.bg, Some(BASIC[4]));
        assert_eq!(r2[1].style.fg, Some([1, 2, 3]));
    }

    #[test]
    fn palette_cube_and_greys() {
        assert_eq!(palette256(16), [0, 0, 0]);
        assert_eq!(palette256(231), [255, 255, 255]);
        assert_eq!(palette256(232), [8, 8, 8]);
        assert_eq!(palette256(255), [238, 238, 238]);
        assert_eq!(palette256(9), BASIC[9]);
    }
}
