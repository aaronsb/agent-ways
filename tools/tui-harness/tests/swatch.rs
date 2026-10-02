//! The swatch is the tool's regression target. `fixtures/swatch.sh` prints a
//! row per colour mode, attribute and glyph class; `fixtures/swatch.ansi` is
//! its capture through tmux (100x14). These tests render that capture, so
//! they need no tmux.

use std::path::{Path, PathBuf};

use image::RgbImage;
use tui_harness::render::fc_has_family;
use tui_harness::{parse, Grid, Renderer, DEFAULT_FONT, DEFAULT_SIZE, FALLBACK_FONT};

const COLS: u32 = 100;
const ROWS: u32 = 14;
/// Every row starts with a label padded to this many columns.
const LABEL: usize = 10;
const CELL_W: u32 = 8;
const CELL_H: u32 = 16;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn swatch_grid() -> Grid {
    let bytes = std::fs::read(fixtures().join("swatch.ansi")).expect("reading swatch.ansi");
    parse(&String::from_utf8_lossy(&bytes))
}

/// The row whose label is `label`.
fn row_of(grid: &Grid, label: &str) -> u32 {
    grid.iter()
        .position(|row| {
            let text: String = row.iter().filter_map(|c| c.ch).take(LABEL).collect();
            text.trim_end() == label
        })
        .unwrap_or_else(|| panic!("no swatch row labelled {label:?}")) as u32
}

struct Swatch {
    grid: Grid,
    img: RgbImage,
}

impl Swatch {
    fn new() -> Swatch {
        let grid = swatch_grid();
        let img = Renderer::without_fonts(CELL_W, CELL_H).render(&grid, Some(COLS), Some(ROWS));
        Swatch { grid, img }
    }

    /// The pixel at the centre of the cell `offset` columns past the label.
    fn centre(&self, label: &str, offset: u32) -> [u8; 3] {
        let row = row_of(&self.grid, label);
        let x = (LABEL as u32 + offset) * CELL_W + CELL_W / 2;
        self.img.get_pixel(x, row * CELL_H + CELL_H / 2).0
    }

    /// The pixel on the cell's last row, where an underline is drawn.
    fn underline(&self, label: &str, offset: u32) -> [u8; 3] {
        let row = row_of(&self.grid, label);
        let x = (LABEL as u32 + offset) * CELL_W + CELL_W / 2;
        self.img.get_pixel(x, row * CELL_H + CELL_H - 1).0
    }
}

// One test per mode, so a failure names the mode that drifted.

#[test]
fn basic16_red() {
    assert_eq!(Swatch::new().centre("basic16", 2), [170, 0, 0]);
}

#[test]
fn bright16_red() {
    assert_eq!(Swatch::new().centre("basic16", 17 + 2), [255, 85, 85]);
}

#[test]
fn cube256_first_corner_16() {
    assert_eq!(Swatch::new().centre("cube256", 0), [0, 0, 0]);
}

#[test]
fn cube256_red_196() {
    assert_eq!(Swatch::new().centre("cube256", 36), [255, 0, 0]);
}

#[test]
fn cube256_last_corner_231() {
    assert_eq!(Swatch::new().centre("cube256", 43), [255, 255, 255]);
}

#[test]
fn grey_ramp_first_232() {
    assert_eq!(Swatch::new().centre("grey", 0), [8, 8, 8]);
}

#[test]
fn grey_ramp_last_255() {
    assert_eq!(Swatch::new().centre("grey", 23), [238, 238, 238]);
}

#[test]
fn truecolor_first_stop() {
    assert_eq!(Swatch::new().centre("truecolor", 0), [0, 255, 128]);
}

#[test]
fn truecolor_middle_stop() {
    assert_eq!(Swatch::new().centre("truecolor", 32), [128, 127, 128]);
}

#[test]
fn reverse_swaps_fg_and_bg() {
    // Red on blue, reversed: the cell is filled red.
    assert_eq!(Swatch::new().centre("rev/dim", 0), [170, 0, 0]);
}

#[test]
fn reverse_of_default_colours() {
    // Default fg (200) becomes the background.
    assert_eq!(Swatch::new().centre("rev/dim", 5), [200, 200, 200]);
}

#[test]
fn dim_halves_the_foreground() {
    // Dim yellow (170, 85, 0) underlined: the underline is (85, 42, 0).
    assert_eq!(Swatch::new().underline("rev/dim", 8), [85, 42, 0]);
}

#[test]
fn swatch_has_every_row() {
    let grid = swatch_grid();
    for label in [
        "attrs",
        "basic16",
        "cube256",
        "grey",
        "truecolor",
        "fg/bg",
        "rev/dim",
        "braille",
        "box",
        "icons",
        "wide",
    ] {
        row_of(&grid, label);
    }
}

/// How far a fresh render may drift from the golden image. A pixel counts as
/// different when any channel differs by more than `CHANNEL_TOLERANCE`; the
/// test fails when more than `MAX_DIFFERENT` of all pixels are different.
/// The render.py comparison put hinting and anti-aliasing drift at about 1%
/// of pixels beyond 32 levels, so the bounds sit there: a font version bump
/// passes, a misplaced row, a lost colour or a missing glyph class does not.
const CHANNEL_TOLERANCE: u8 = 32;
const MAX_DIFFERENT: f64 = 0.01;

/// The CJK family the golden image was recorded with.
const CJK_FAMILY: &str = "Noto Sans CJK SC";

/// Compare a render with the fonts against `fixtures/swatch.golden.png`.
/// Runs only where fontconfig has the fonts the golden was recorded with.
/// Set `TUI_HARNESS_BLESS=1` to write `swatch.golden.new.png` beside it;
/// the run then fails, so a person reviews the image and moves it into
/// place by hand.
#[test]
fn swatch_matches_golden_png() {
    let missing: Vec<&str> = [DEFAULT_FONT, FALLBACK_FONT, CJK_FAMILY]
        .into_iter()
        .filter(|f| !fc_has_family(f))
        .collect();
    if !missing.is_empty() {
        eprintln!(
            "SKIPPED swatch_matches_golden_png: fontconfig lacks {}",
            missing.join(", ")
        );
        return;
    }
    let renderer = Renderer::new(DEFAULT_FONT, DEFAULT_SIZE);
    let fresh = renderer.render(&swatch_grid(), Some(COLS), Some(ROWS));
    let golden_path = fixtures().join("swatch.golden.png");

    if std::env::var_os("TUI_HARNESS_BLESS").is_some_and(|v| v == "1") {
        let new_path = fixtures().join("swatch.golden.new.png");
        fresh.save(&new_path).expect("writing the blessed image");
        panic!(
            "blessed: wrote {}. Review it, then move it over {} by hand.",
            new_path.display(),
            golden_path.display()
        );
    }

    let golden = image::open(&golden_path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", golden_path.display()))
        .to_rgb8();
    assert_eq!(
        fresh.dimensions(),
        golden.dimensions(),
        "image size changed"
    );
    let different = fresh
        .pixels()
        .zip(golden.pixels())
        .filter(|(a, b)| {
            a.0.iter()
                .zip(b.0.iter())
                .any(|(x, y)| x.abs_diff(*y) > CHANNEL_TOLERANCE)
        })
        .count();
    let share = different as f64 / (fresh.width() * fresh.height()) as f64;
    if share > MAX_DIFFERENT {
        let actual = std::env::temp_dir().join("tui-harness-swatch.actual.png");
        let _ = fresh.save(&actual);
        panic!(
            "{:.2}% of pixels differ by more than {CHANNEL_TOLERANCE} (limit {:.2}%); \
             this render is at {}",
            share * 100.0,
            MAX_DIFFERENT * 100.0,
            display(&actual)
        );
    }
}

fn display(p: &Path) -> String {
    p.display().to_string()
}
