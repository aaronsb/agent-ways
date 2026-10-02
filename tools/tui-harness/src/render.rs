//! A cell grid to a PNG.
//!
//! Fonts come from the system through `fc-match`, never bundled. Each glyph
//! is drawn with the primary family when that family maps the character, and
//! with the fallback family (a Nerd Font, for Braille and icons) otherwise.
//! Metrics follow render.py (PIL on FreeType) so the two produce images of
//! the same geometry from the same capture.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;

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

/// Renders grids at one font and size. Build it once and reuse it: font
/// lookup and loading happen in [`Renderer::new`].
pub struct Renderer {
    /// Indexed by `bold as usize | (italic as usize) << 1`.
    primary: [Option<Rc<Face>>; 4],
    fallback: [Option<Rc<Face>>; 4],
    cell_w: u32,
    cell_h: u32,
}

impl Renderer {
    /// Find `family` and the fallback family through `fc-match` and load
    /// all four styles of each. With no usable font at all the renderer
    /// still draws colours and attributes, just no glyphs.
    pub fn new(family: &str, size: u32) -> Renderer {
        let ppem = size as f32;
        let mut cache: HashMap<(PathBuf, u32), Option<Rc<Face>>> = HashMap::new();
        let mut load = |fam: &str, style: usize| {
            let (path, index) = fc_match(fam, style & 1 != 0, style & 2 != 0)?;
            cache
                .entry((path.clone(), index))
                .or_insert_with(|| Face::load(&path, index, ppem).map(Rc::new))
                .clone()
        };
        let primary: [Option<Rc<Face>>; 4] = std::array::from_fn(|s| load(family, s));
        let fallback: [Option<Rc<Face>>; 4] = std::array::from_fn(|s| load(FALLBACK_FONT, s));

        let (cell_w, cell_h) = match primary[0].as_ref().or(fallback[0].as_ref()) {
            Some(face) => measure_cell(face, ppem),
            None => fontless_cell(size),
        };
        Renderer {
            primary,
            fallback,
            cell_w,
            cell_h,
        }
    }

    /// A renderer with no fonts and a fixed cell size: colours, reverse,
    /// dim and underline only. Deterministic on any machine, for tests.
    pub fn without_fonts(cell_w: u32, cell_h: u32) -> Renderer {
        Renderer {
            primary: Default::default(),
            fallback: Default::default(),
            cell_w: cell_w.max(1),
            cell_h: cell_h.max(1),
        }
    }

    /// Whether the primary family resolved to a loadable font.
    pub fn has_primary_font(&self) -> bool {
        self.primary[0].is_some()
    }

    /// Cell size in pixels, `(width, height)`.
    pub fn cell_size(&self) -> (u32, u32) {
        (self.cell_w, self.cell_h)
    }

    /// Render `grid`. `cols` and `rows`, when given, set the image size in
    /// cells (padding with the default background, never cropping).
    pub fn render(&self, grid: &Grid, cols: Option<u32>, rows: Option<u32>) -> RgbImage {
        let widest = grid.iter().map(Vec::len).max().unwrap_or(0) as u32;
        let cols = cols.unwrap_or(widest).max(widest).max(1);
        let rows_n = rows.unwrap_or(0).max(grid.len() as u32).max(1);
        let (cw, ch) = (self.cell_w, self.cell_h);
        let mut img = RgbImage::from_pixel(cols * cw, rows_n * ch, Pixel(DEFAULT_BG));

        for (ry, row) in grid.iter().enumerate() {
            let y = ry as u32 * ch;
            for (cx, cell) in row.iter().enumerate() {
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
                if let Some(c) = cell.ch.filter(|c| *c != ' ') {
                    let key = st.bold as usize | (st.italic as usize) << 1;
                    let face = match (&self.primary[key], &self.fallback[key]) {
                        (Some(p), _) if p.has(c) => Some(p),
                        (p, f) => f.as_ref().or(p.as_ref()),
                    };
                    if let Some(face) = face {
                        draw_glyph(&mut img, face, c, x, y, fg);
                    }
                }
                if st.underline {
                    fill(&mut img, x, y + ch - 1, cw, 1, fg);
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
    fn full_block_glyph_takes_fg_when_a_font_exists() {
        let r = Renderer::new(DEFAULT_FONT, DEFAULT_SIZE);
        if !r.has_primary_font() {
            eprintln!("skipping glyph check: fc-match found no font");
            return;
        }
        let (w, h) = r.cell_size();
        let img = r.render(&parse(&esc("^[32;45m\u{2588}^[0m")), None, None);
        assert_eq!(img.get_pixel(w / 2, h / 2).0, BASIC[2]);
    }
}
