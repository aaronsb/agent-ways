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
    pub strike: bool,
    pub conceal: bool,
}

impl Style {
    /// Apply one SGR parameter list given in the `;` form only, one number
    /// per parameter. [`Style::apply_groups`] takes the `:` form as well.
    pub fn apply_sgr(&mut self, params: &[u16]) {
        let groups: Vec<&[u16]> = params.iter().map(std::slice::from_ref).collect();
        self.apply_groups(&groups);
    }

    /// Apply one SGR parameter list as tmux and vte deliver it: one group
    /// per `;`-separated parameter, each holding the parameter and its
    /// `:` sub-parameters.
    ///
    /// A group with sub-parameters is decoded whole: `4:n` sets underline
    /// for any non-zero style `n` (single, double, curly, dotted, dashed),
    /// `38`/`48` colours decode in place, `58` (underline colour) is read
    /// and dropped, and any other code with sub-parameters is ignored. In
    /// the `;` form an extended colour (`38`, `48`, `58` followed by `5;n`
    /// or `2;r;g;b`) consumes its parameters; a truncated one ends the
    /// sequence rather than having its leftovers read as attributes, and a
    /// palette index above 255 is ignored.
    pub fn apply_groups(&mut self, groups: &[&[u16]]) {
        if groups.is_empty() {
            *self = Style::default();
            return;
        }
        let mut i = 0;
        while i < groups.len() {
            let group = groups[i];
            i += 1;
            let Some((&p, subs)) = group.split_first() else {
                continue;
            };
            if !subs.is_empty() {
                match p {
                    4 => self.underline = subs[0] != 0,
                    38 | 48 | 58 => {
                        if let Some(colour) = colon_colour(subs) {
                            self.set_extended(p, colour);
                        }
                    }
                    _ => {}
                }
                continue;
            }
            match p {
                0 => *self = Style::default(),
                1 => self.bold = true,
                2 => self.dim = true,
                3 => self.italic = true,
                4 | 21 => self.underline = true,
                7 => self.reverse = true,
                8 => self.conceal = true,
                9 => self.strike = true,
                22 => {
                    self.bold = false;
                    self.dim = false;
                }
                23 => self.italic = false,
                24 => self.underline = false,
                27 => self.reverse = false,
                28 => self.conceal = false,
                29 => self.strike = false,
                30..=37 => self.fg = Some(BASIC[(p - 30) as usize]),
                39 => self.fg = None,
                40..=47 => self.bg = Some(BASIC[(p - 40) as usize]),
                49 => self.bg = None,
                90..=97 => self.fg = Some(BASIC[(p - 90 + 8) as usize]),
                100..=107 => self.bg = Some(BASIC[(p - 100 + 8) as usize]),
                38 | 48 | 58 => match semicolon_colour(&groups[i..]) {
                    Some((colour, used)) => {
                        if let Some(colour) = colour {
                            self.set_extended(p, colour);
                        }
                        i += used;
                    }
                    // Truncated or malformed: stop rather than misread.
                    None => return,
                },
                // 59 resets the underline colour, which is not drawn.
                _ => {}
            }
        }
    }

    /// Set the colour an extended-colour code names. `58` is the underline
    /// colour, which is parsed and not drawn.
    fn set_extended(&mut self, code: u16, colour: Rgb) {
        match code {
            38 => self.fg = Some(colour),
            48 => self.bg = Some(colour),
            _ => {}
        }
    }
}

/// Decode the sub-parameters of a colon-form colour: `5:n`, `2:r:g:b`, or
/// `2:cs:r:g:b` with a colour-space slot. `None` when malformed or when the
/// palette index is above 255.
fn colon_colour(subs: &[u16]) -> Option<Rgb> {
    match subs {
        [5, n] => palette_index(*n),
        [2, r, g, b] | [2, _, r, g, b] => Some(rgb(*r, *g, *b)),
        _ => None,
    }
}

/// Decode the `;`-form tail of a `38`/`48`/`58` code: `5;n` or `2;r;g;b`,
/// each a one-number group. Returns the colour (`None` for a palette index
/// above 255) and the number of groups consumed, or `None` when the tail is
/// truncated or malformed.
fn semicolon_colour(rest: &[&[u16]]) -> Option<(Option<Rgb>, usize)> {
    let mut nums = Vec::with_capacity(4);
    for group in rest.iter().take(4) {
        match group {
            [n] => nums.push(*n),
            _ => break,
        }
    }
    match nums.as_slice() {
        [5, n, ..] => Some((palette_index(*n), 2)),
        [2, r, g, b, ..] => Some((Some(rgb(*r, *g, *b)), 4)),
        _ => None,
    }
}

fn palette_index(n: u16) -> Option<Rgb> {
    u8::try_from(n).ok().map(palette256)
}

fn rgb(r: u16, g: u16, b: u16) -> Rgb {
    [clamp_u8(r), clamp_u8(g), clamp_u8(b)]
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
        let groups: Vec<&[u16]> = params.iter().collect();
        self.style.apply_groups(&groups);
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

    /// The style of the first cell of `capture` (ESC written as `^`).
    fn style_of(capture: &str) -> Style {
        parse(&esc(capture))[0][0].style
    }

    fn underlined() -> Style {
        Style {
            underline: true,
            ..Style::default()
        }
    }

    // The reviewer's probes (PR #724): sub-parameters and underline colour
    // must not leak into other attributes.

    #[test]
    fn curly_underline_is_underline_not_italic() {
        assert_eq!(style_of("^[4:3mC"), underlined());
        assert_eq!(style_of("^[4:2mC"), underlined());
        assert_eq!(style_of("^[4:1mC"), underlined());
    }

    #[test]
    fn underline_style_zero_turns_underline_off() {
        assert_eq!(style_of("^[4m^[4:0mC"), Style::default());
    }

    #[test]
    fn underline_colour_truecolor_keeps_the_underline() {
        // As tmux captures `\e[4:3m` + `\e[58:2::255:0:0m`.
        assert_eq!(style_of("^[4m^[58;2;255;0;0mU"), underlined());
    }

    #[test]
    fn underline_colour_indexed_is_ignored() {
        assert_eq!(style_of("^[58;5;7mU"), Style::default());
        assert_eq!(style_of("^[58;5;31mU"), Style::default());
        assert_eq!(style_of("^[4;58:5:31mU"), underlined());
    }

    #[test]
    fn underline_colour_colon_form_is_ignored() {
        assert_eq!(style_of("^[4;58:2::1:2:3mU"), underlined());
        assert_eq!(style_of("^[4;58:2:1:2:3mU"), underlined());
    }

    #[test]
    fn underline_colour_reset_is_ignored() {
        assert_eq!(style_of("^[4;59mU"), underlined());
    }

    #[test]
    fn truncated_extended_colour_stops_decoding() {
        assert_eq!(style_of("^[38;2;1;2mT"), Style::default());
        assert_eq!(style_of("^[48;5mT"), Style::default());
        assert_eq!(style_of("^[38mT"), Style::default());
    }

    #[test]
    fn palette_index_above_255_is_ignored() {
        assert_eq!(style_of("^[38;5;300mT"), Style::default());
        assert!(style_of("^[38;5;300;1mT").bold);
        assert_eq!(style_of("^[48:5:256mT"), Style::default());
    }

    #[test]
    fn colon_colour_forms_decode_in_place() {
        assert_eq!(style_of("^[38:5:196mX").fg, Some([255, 0, 0]));
        assert_eq!(style_of("^[38:2:1:2:3mX").fg, Some([1, 2, 3]));
        assert_eq!(style_of("^[48:2::4:5:6mX").bg, Some([4, 5, 6]));
        // Codes after a colon group still apply.
        let s = style_of("^[38:2::1:2:3;1mX");
        assert_eq!((s.fg, s.bold), (Some([1, 2, 3]), true));
    }

    #[test]
    fn unknown_colon_groups_are_ignored_whole() {
        assert_eq!(style_of("^[1:3mX"), Style::default());
    }

    #[test]
    fn strikethrough_and_conceal() {
        let s = style_of("^[9;8mX");
        assert!(s.strike && s.conceal);
        let s = style_of("^[9;8;29;28mX");
        assert!(!s.strike && !s.conceal);
    }
}
