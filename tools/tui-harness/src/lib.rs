//! Drive a TUI in a detached terminal and screenshot what it renders.
//!
//! tmux owns the PTY, the input and the screen buffer. This crate launches a
//! command in a detached tmux session of a fixed geometry, sends it keys,
//! captures the pane as text, and renders the pane to a PNG from
//! `tmux capture-pane -ep` output. No X server, no GPU.
//!
//! ```no_run
//! use tui_harness::{Harness, LaunchOptions};
//! # fn main() -> anyhow::Result<()> {
//! let h = Harness::new(std::env::temp_dir().join("my-test"));
//! let s = h.launch("demo", &LaunchOptions::default(), &["htop".into()])?;
//! s.send(&["q"])?;
//! let text = s.text(false)?;
//! let png = s.shot(None)?;
//! s.down()?;
//! # Ok(()) }
//! ```

pub mod render;
pub mod session;
pub mod sgr;

pub use render::{Renderer, DEFAULT_FONT, DEFAULT_SIZE, FALLBACK_FONT};
pub use session::{
    default_state_dir, tmux_available, Harness, LaunchOptions, Session, DEFAULT_COLS, DEFAULT_ROWS,
};
pub use sgr::{parse, Cell, Grid, Style};
