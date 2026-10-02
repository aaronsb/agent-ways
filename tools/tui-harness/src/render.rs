//! A cell grid to a PNG.
//!
//! Fonts come from the system through `fc-match`, never bundled. Each glyph
//! is drawn with the primary family when that family maps the character, and
//! with the fallback family (a Nerd Font, for Braille and icons) otherwise.
//! A glyph neither has (Chinese, Japanese, Korean) comes from a CJK font
//! found through fontconfig, when one is installed; else it is the fallback
//! font's missing-glyph box.
//! Metrics follow render.py (PIL on FreeType) so the two produce images of
//! the same geometry from the same capture.
//!
//! A wide character in the last column would overhang the image; tmux never
//! puts one there, and the image grows by a cell rather than crop it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, OnceLock};

use ab_glyph::{point, Font, FontVec, PxScale};
use anyhow::{Context, Result};
use image::{Rgb as Pixel, RgbImage};

use crate::sgr::{Grid, Rgb, DEFAULT_BG, DEFAULT_FG};

/// The default primary family.
pub const DEFAULT_FONT: &str = "JetBrains Mono";
/// The default size in pixels per em (render.py's "pt").
pub const DEFAULT_SIZE: u32 = 14;
/// The family used for glyphs the primary font lacks.
pub const FALLBACK_FONT: &str = "CaskaydiaMono Nerd Font Mono";
/// Families tried, in order, for glyphs missing from both fonts above. After
/// these, any font fontconfig lists for `:lang=zh`.
pub const CJK_FONTS: [&str; 2] = ["Noto Sans Mono CJK SC", "Noto Sans CJK SC"];
/// The character a CJK candidate must map to be used.
const CJK_PROBE: char = '\u{4e2d}';
/// The largest grid, in cells, a render covers in either direction: tmux's
/// own window size limit. Larger requests are clamped.
pub const MAX_CELLS: u32 = 10_000;
/// The largest font size, in pixels per em, a renderer accepts.
pub const MAX_SIZE: u32 = 512;

/// A loaded face plus the metrics needed to place its glyphs.
struct Face {
    font: FontVec,
    scale: PxScale,
    /// Ascent in whole pixels, rounded up as FreeType does.
    ascent: i32,
}

impl Face {
    fn load(path: &Path, index: u32, ppem: f32) -> Option<Face> {
        let data = std::fs::read(path).ok()?;
        let font = FontVec::try_from_vec_and_index(data, index).ok()?;
        let upem = font.units_per_em()?;
        let scale = PxScale::from(ppem * font.height_unscaled() / upem);
        let ascent = (font.ascent_unscaled() * ppem / upem).ceil() as i32;
        Some(Face {
            font,
            scale,
            ascent,
        })
    }

    fn has(&self, ch: char) -> bool {
        self.font.glyph_id(ch).0 != 0
    }
}

/// Ask fontconfig for the file behind `family` in the given style.
pub fn fc_match(family: &str, bold: bool, italic: bool) -> Option<(PathBuf, u32)> {
    let style = match (bold, italic) {
        (false, false) => "",
        (true, false) => ":style=Bold",
        (false, true) => ":style=Italic",
        (true, true) => ":style=Bold Italic",
    };
    let out = Command::new("fc-match")
        .args(["-f", "%{file}\n%{index}", &format!("{family}{style}")])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8(out.stdout).ok()?;
    let mut lines = text.lines();
    let file = lines.next().filter(|f| !f.is_empty())?;
    let index = lines
        .next()
        .and_then(|i| i.trim().parse().ok())
        .unwrap_or(0);
    Some((PathBuf::from(file), index))
}

/// Whether fontconfig resolves `family` to that family itself rather than
/// to a substitute. `fc-match` always answers with something, so a caller
/// that needs the real font (a golden-image test) checks this first.
pub fn fc_has_family(family: &str) -> bool {
    Command::new("fc-match")
        .args(["-f", "%{family}", family])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .split(',')
                .any(|f| f.trim().eq_ignore_ascii_case(family))
        })
        .unwrap_or(false)
}

/// Every font fontconfig lists as covering `lang`, as `(file, index)`,
/// sorted so the choice is stable.
fn fc_list_lang(lang: &str) -> Vec<(PathBuf, u32)> {
    let Ok(out) = Command::new("fc-list")
        .args(["-f", "%{file}\t%{index}\n", &format!(":lang={lang}")])
        .output()
    else {
        return Vec::new();
    };
    let mut found: Vec<(PathBuf, u32)> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            let (file, index) = l.split_once('\t')?;
            Some((PathBuf::from(file), index.trim().parse().unwrap_or(0)))
        })
        .collect();
    found.sort();
    found.dedup();
    found
}

/// Renders grids at one font and size. Build it once and reuse it: font
/// lookup and loading happen in [`Renderer::new`]. It is `Send` and `Sync`,
/// so tests can share one across threads.
pub struct Renderer {
    /// Indexed by `bold as usize | (italic as usize) << 1`.
    primary: [Option<Arc<Face>>; 4],
    fallback: [Option<Arc<Face>>; 4],
    /// Whether fontconfig has the requested families themselves, rather
    /// than substitutes for them.
    primary_found: bool,
    fallback_found: bool,
    requested: String,
    /// Loaded on first use: CJK fonts are large and most screens need none.
    /// Holds the face and a label naming where it came from.
    cjk: OnceLock<Option<(String, Face)>>,
    /// Pixels per em; 0 when the renderer draws no glyphs.
    ppem: f32,
    cell_w: u32,
    cell_h: u32,
    /// Offset from the cell top to the strikethrough line.
    strike_y: u32,
}

impl Renderer {
    /// Find `family` and the fallback family through `fc-match` and load
    /// all four styles of each. With no usable font at all the renderer
    /// still draws colours and attributes, just no glyphs.
    pub fn new(family: &str, size: u32) -> Renderer {
        let size = size.clamp(1, MAX_SIZE);
        let ppem = size as f32;
        let mut cache: HashMap<(PathBuf, u32), Option<Arc<Face>>> = HashMap::new();
        let mut load = |fam: &str, style: usize| {
            let (path, index) = fc_match(fam, style & 1 != 0, style & 2 != 0)?;
            cache
                .entry((path.clone(), index))
                .or_insert_with(|| Face::load(&path, index, ppem).map(Arc::new))
                .clone()
        };
        let primary: [Option<Arc<Face>>; 4] = std::array::from_fn(|s| load(family, s));
        let fallback: [Option<Arc<Face>>; 4] = std::array::from_fn(|s| load(FALLBACK_FONT, s));

        let metrics_face = primary[0].as_ref().or(fallback[0].as_ref());
        let (cell_w, cell_h) = match metrics_face {
            Some(face) => measure_cell(face, ppem),
            None => fontless_cell(size),
        };
        // Through the middle of the x-height: about 0.25 em above the
        // baseline.
        let strike_y = match metrics_face {
            Some(face) => (face.ascent - (ppem * 0.25).round() as i32).max(0) as u32,
            None => cell_h / 2,
        };
        Renderer {
            primary,
            fallback,
            primary_found: fc_has_family(family),
            fallback_found: fc_has_family(FALLBACK_FONT),
            requested: family.to_string(),
            cjk: OnceLock::new(),
            ppem,
            cell_w,
            cell_h,
            strike_y: strike_y.min(cell_h.saturating_sub(1)),
        }
    }

    /// A renderer with no fonts and a fixed cell size: colours, reverse,
    /// dim and underline only. Deterministic on any machine, for tests.
    pub fn without_fonts(cell_w: u32, cell_h: u32) -> Renderer {
        let cell_w = cell_w.clamp(1, MAX_SIZE);
        let cell_h = cell_h.clamp(1, MAX_SIZE);
        Renderer {
            primary: Default::default(),
            fallback: Default::default(),
            primary_found: false,
            fallback_found: false,
            requested: String::new(),
            cjk: OnceLock::new(),
            ppem: 0.0,
            cell_w,
            cell_h,
            strike_y: cell_h / 2,
        }
    }

    /// Whether the requested primary family is installed and loaded.
    /// `fc-match` always answers, so a missing or misspelt family loads a
    /// substitute; this is false then, and [`Renderer::warnings`] says so.
    pub fn has_primary_font(&self) -> bool {
        self.primary_found && self.primary[0].is_some()
    }

    /// Whether the Nerd Font fallback family is installed and loaded.
    pub fn has_fallback_font(&self) -> bool {
        self.fallback_found && self.fallback[0].is_some()
    }

    /// Font problems worth telling a person about: a requested family that
    /// fontconfig replaced with a substitute, or a missing fallback.
    pub fn warnings(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.ppem <= 0.0 {
            return out;
        }
        if !self.has_primary_font() {
            out.push(format!(
                "font '{}' is not installed; fontconfig substituted another font, so cell size and glyphs will differ",
                self.requested
            ));
        }
        if !self.has_fallback_font() {
            out.push(format!(
                "fallback font '{FALLBACK_FONT}' is not installed; Braille and icon glyphs may draw as boxes"
            ));
        }
        out
    }

    /// Whether a CJK font was found (this loads it on first call).
    pub fn has_cjk_font(&self) -> bool {
        self.cjk_face().is_some()
    }

    /// Where the CJK face came from: the family name for one of
    /// [`CJK_FONTS`], or `fc-list:<file>#<index>`. Loads it on first call.
    pub fn cjk_font(&self) -> Option<&str> {
        self.cjk_entry().map(|(label, _)| label.as_str())
    }

    fn cjk_face(&self) -> Option<&Face> {
        self.cjk_entry().map(|(_, face)| face)
    }

    /// The CJK face and its label, found and loaded on first call. A named
    /// family is used only when fontconfig has that family itself.
    fn cjk_entry(&self) -> Option<&(String, Face)> {
        self.cjk
            .get_or_init(|| {
                if self.ppem <= 0.0 {
                    return None;
                }
                let named = CJK_FONTS
                    .iter()
                    .filter(|f| fc_has_family(f))
                    .filter_map(|f| {
                        let (path, index) = fc_match(f, false, false)?;
                        Some((f.to_string(), path, index))
                    });
                let listed = fc_list_lang("zh").into_iter().map(|(path, index)| {
                    (format!("fc-list:{}#{index}", path.display()), path, index)
                });
                named.chain(listed).find_map(|(label, path, index)| {
                    let face = Face::load(&path, index, self.ppem).filter(|f| f.has(CJK_PROBE))?;
                    Some((label, face))
                })
            })
            .as_ref()
    }

    /// The face to draw `c` with: the primary font, then the fallback, then
    /// the CJK font, then the fallback's missing-glyph box.
    fn face_for(&self, c: char, key: usize) -> Option<&Face> {
        let primary = self.primary[key].as_deref();
        let fallback = self.fallback[key].as_deref();
        if let Some(p) = primary.filter(|p| p.has(c)) {
            return Some(p);
        }
        if let Some(f) = fallback.filter(|f| f.has(c)) {
            return Some(f);
        }
        if primary.is_some() || fallback.is_some() {
            if let Some(k) = self.cjk_face().filter(|k| k.has(c)) {
                return Some(k);
            }
        }
        fallback.or(primary)
    }

    /// Cell size in pixels, `(width, height)`.
    pub fn cell_size(&self) -> (u32, u32) {
        (self.cell_w, self.cell_h)
    }

    /// Render `grid`. `cols` and `rows`, when given, set the image size in
    /// cells (padding with the default background, never cropping), up to
    /// [`MAX_CELLS`] each way; cells beyond that are not drawn.
    pub fn render(&self, grid: &Grid, cols: Option<u32>, rows: Option<u32>) -> RgbImage {
        let widest = grid.iter().map(Vec::len).max().unwrap_or(0) as u32;
        let cols = cols.unwrap_or(widest).max(widest).clamp(1, MAX_CELLS);
        let rows_n = rows.unwrap_or(0).max(grid.len() as u32).clamp(1, MAX_CELLS);
        let (cw, ch) = (self.cell_w, self.cell_h);
        let mut img = RgbImage::from_pixel(cols * cw, rows_n * ch, Pixel(DEFAULT_BG));

        for (ry, row) in grid.iter().take(rows_n as usize).enumerate() {
            let y = ry as u32 * ch;
            for (cx, cell) in row.iter().take(cols as usize).enumerate() {
                let x = cx as u32 * cw;
                let st = cell.style;
                let mut fg = st.fg.unwrap_or(DEFAULT_FG);
                let mut bg = st.bg.unwrap_or(DEFAULT_BG);
                if st.reverse {
                    std::mem::swap(&mut fg, &mut bg);
                }
                if st.dim {
                    fg = fg.map(|c| c / 2);
                }
                if bg != DEFAULT_BG {
                    fill(&mut img, x, y, cw, ch, bg);
                }
                if let Some(c) = cell.ch.filter(|c| *c != ' ' && !st.conceal) {
                    let key = st.bold as usize | (st.italic as usize) << 1;
                    if let Some(face) = self.face_for(c, key) {
                        draw_glyph(&mut img, face, c, x, y, fg);
                    }
                }
                if st.underline {
                    fill(&mut img, x, y + ch - 1, cw, 1, fg);
                }
                if st.strike {
                    fill(&mut img, x, y + self.strike_y, cw, 1, fg);
                }
            }
        }
        img
    }

    /// Render and write a PNG.
    pub fn render_to(
        &self,
        grid: &Grid,
        cols: Option<u32>,
        rows: Option<u32>,
        out: &Path,
    ) -> Result<()> {
        if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        self.render(grid, cols, rows)
            .save_with_format(out, image::ImageFormat::Png)
            .with_context(|| format!("writing {}", out.display()))
    }
}

/// Cell width from the "M" glyph (its rounded advance, widened to its ink
/// if the ink overhangs) and height from the rounded ascent plus descent.
fn measure_cell(face: &Face, ppem: f32) -> (u32, u32) {
    let font = &face.font;
    let upem = font.units_per_em().unwrap_or(1000.0);
    let descent = (-font.descent_unscaled() * ppem / upem).ceil() as i32;
    let id = font.glyph_id('M');
    let advance = (font.h_advance_unscaled(id) * ppem / upem).round() as i32;
    let ink = font
        .outline_glyph(id.with_scale_and_position(face.scale, point(0.0, 0.0)))
        .map(|g| g.px_bounds().max.x.ceil() as i32)
        .unwrap_or(0);
    let w = advance.max(ink).max(1) as u32;
    let h = (face.ascent + descent).max(1) as u32;
    (w, h)
}

fn fontless_cell(size: u32) -> (u32, u32) {
    (
        (size * 3).div_ceil(5).max(1),
        (size * 10).div_ceil(7).max(1),
    )
}

fn fill(img: &mut RgbImage, x: u32, y: u32, w: u32, h: u32, colour: Rgb) {
    let (iw, ih) = img.dimensions();
    for py in y..(y + h).min(ih) {
        for px in x..(x + w).min(iw) {
            img.put_pixel(px, py, Pixel(colour));
        }
    }
}

/// Draw `c` with its pen at `(x, y + ascent)`, blending `fg` over what is
/// already there by glyph coverage.
fn draw_glyph(img: &mut RgbImage, face: &Face, c: char, x: u32, y: u32, fg: Rgb) {
    let baseline = y as f32 + face.ascent as f32;
    let glyph = face
        .font
        .glyph_id(c)
        .with_scale_and_position(face.scale, point(x as f32, baseline));
    let Some(outline) = face.font.outline_glyph(glyph) else {
        return;
    };
    let bounds = outline.px_bounds();
    let (iw, ih) = img.dimensions();
    outline.draw(|gx, gy, coverage| {
        let px = bounds.min.x as i64 + gx as i64;
        let py = bounds.min.y as i64 + gy as i64;
        if px < 0 || py < 0 || px >= iw as i64 || py >= ih as i64 {
            return;
        }
        let a = coverage.clamp(0.0, 1.0);
        let dst = img.get_pixel_mut(px as u32, py as u32);
        for (d, f) in dst.0.iter_mut().zip(fg) {
            *d = (*d as f32 * (1.0 - a) + f as f32 * a).round() as u8;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sgr::{esc, parse, BASIC};

    fn centre(r: &Renderer, col: u32, row: u32) -> Rgb {
        let (w, h) = r.cell_size();
        r.render(&parse(&esc(GRID)), None, None)
            .get_pixel(col * w + w / 2, row * h + h / 2)
            .0
    }

    /// Row 0: a blue-background space, a default space, a reversed red
    /// space. Row 1: a truecolor background under a dim, underlined space.
    const GRID: &str = "^[44m ^[0m ^[31;7m ^[0m\n^[48;2;1;2;3;2;4m ^[0m\n";

    #[test]
    fn cell_centre_colours() {
        let r = Renderer::without_fonts(8, 16);
        let img = r.render(&parse(&esc(GRID)), None, None);
        assert_eq!(img.dimensions(), (3 * 8, 2 * 16));
        assert_eq!(centre(&r, 0, 0), BASIC[4]);
        assert_eq!(centre(&r, 1, 0), DEFAULT_BG);
        // Reverse paints the foreground colour as the background.
        assert_eq!(centre(&r, 2, 0), BASIC[1]);
        assert_eq!(centre(&r, 0, 1), [1, 2, 3]);
        // The underline sits on the cell's last pixel row in the dimmed fg.
        let under = img.get_pixel(4, 2 * 16 - 1).0;
        assert_eq!(under, DEFAULT_FG.map(|c| c / 2));
    }

    #[test]
    fn padding_to_requested_geometry() {
        let r = Renderer::without_fonts(8, 16);
        let img = r.render(&parse("ab\n"), Some(10), Some(4));
        assert_eq!(img.dimensions(), (80, 64));
    }

    #[test]
    fn a_missing_family_is_not_reported_as_the_primary_font() {
        let r = Renderer::new("NoSuchFontXYZ", DEFAULT_SIZE);
        assert!(!r.has_primary_font());
    }

    #[test]
    fn cjk_glyph_comes_from_the_cjk_font_when_one_exists() {
        let r = Renderer::new(DEFAULT_FONT, DEFAULT_SIZE);
        let Some(cjk) = r.cjk_face() else {
            eprintln!("skipping CJK check: no CJK font installed");
            return;
        };
        let face = r.face_for('\u{4e2d}', 0).expect("a face");
        assert!(std::ptr::eq(face, cjk));
        // Latin text never reaches the CJK font.
        let m = r.face_for('M', 0).expect("a face");
        assert!(!std::ptr::eq(m, cjk));
    }

    #[test]
    fn full_block_glyph_takes_fg_when_a_font_exists() {
        let r = Renderer::new(DEFAULT_FONT, DEFAULT_SIZE);
        if !r.has_primary_font() {
            eprintln!("skipping glyph check: {DEFAULT_FONT} is not installed");
            return;
        }
        let (w, h) = r.cell_size();
        let img = r.render(&parse(&esc("^[32;45m\u{2588}^[0m")), None, None);
        assert_eq!(img.get_pixel(w / 2, h / 2).0, BASIC[2]);
        // Concealed, the same cell shows only its background.
        let img = r.render(&parse(&esc("^[8;32;45m\u{2588}^[0m")), None, None);
        assert_eq!(img.get_pixel(w / 2, h / 2).0, BASIC[5]);
    }

    #[test]
    fn strikethrough_draws_a_line_in_the_fg() {
        let r = Renderer::without_fonts(8, 16);
        let img = r.render(&parse(&esc("^[9;33m ^[0m")), None, None);
        assert_eq!(img.get_pixel(4, 8).0, BASIC[3]);
        assert_eq!(img.get_pixel(4, 2).0, DEFAULT_BG);
    }

    #[test]
    fn geometry_is_clamped() {
        let r = Renderer::without_fonts(1, 1);
        let img = r.render(&parse("x"), Some(u32::MAX), Some(3));
        assert_eq!(img.dimensions(), (MAX_CELLS, 3));
    }

    #[test]
    fn renderer_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Renderer>();
    }
}
