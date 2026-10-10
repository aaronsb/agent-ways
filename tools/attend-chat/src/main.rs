use std::io::{IsTerminal, Write};

use agent_theme::ColorDepth;
use agent_tui::screen::Screen;
use agent_tui::testkit;
use attend_chat::app::Chat;
use attend_chat::{signal, theme, watcher};

const HELP: &str = "attend-chat — interactive chat TUI for attend (ADR-120)

usage: attend-chat [--snap WxH [--keys \"KEYS\"]] [--depth DEPTH]

  Esc / Ctrl-C              exit; with a draft in the compose box, asks first. Where
                            the terminal does not report Ctrl+digits, Ctrl+3 is Esc,
                            so Esc asks first there too: y quits
  Enter                     send to the foreground channel
  Ctrl+1..9                 show tab N (1 = merged, 2 = #open, ...); again on the
                            shown tab, its menu. Needs a terminal that speaks the
                            kitty keyboard protocol; F1 says whether yours does
  Alt+1..9                  show tab N where Ctrl+digits do not arrive (Konsole and
                            GNOME Terminal keep Alt+digits for their own tabs)
  F2 / Ctrl+T               the tab bar (footer: TABS; the tab under the cursor is
                            reversed behind a ▸): Left Right move, Enter the tab's
                            menu, Esc back to the compose box
  Ctrl+N                    ask for a new channel's name in the compose box, from
                            the compose box or the tab bar; a draft comes back after
  Tab                       next tab (empty input) / complete @name #channel /command
  tab menus                 ≡ (left of merged, no number): Theme, Keybinding set,
                            Mouse at start, Settings. merged: Clear view.
                            #open: Clear view, Clear history. A channel: Add
                            agent (not built yet), Invite agent, Remove agent,
                            Describe, Clear history, Leave, Delete channel.
                            The + slot (or Ctrl+N) asks for a new channel's name
                            with no menu. Clear history and Delete
                            ask first: y goes ahead. Channel tabs are ordered
                            by their newest message, #open first
  Shift-Enter / Alt-Enter   insert newline
  Left / Right / Home / End move cursor
  Backspace / Delete        edit
  PgUp / PgDn               scroll the messages
  Alt+m                     mouse on or off; off at the start, so the terminal
                            selects text and middle-click pastes
  F1                        the keys
  mouse (on)                click a tab to show it, a message to select it, the
                            compose box to place the cursor, a chip to complete it;
                            the wheel scrolls the messages; Shift-drag selects
                            text in most terminals

  The colours follow the agent-ways theme (`ways settings theme`). Which keys
  reach the tabs, and whether the mouse starts on, are attend.chat.* settings:
  /config lists and sets them, as does `ways settings`.

  --snap WxH     print one frame at W by H in agent-tui's frame format, headless
  --keys KEYS    keys for --snap, space separated (`text:hi enter tab esc alt-1 ctrl-3 f2` …),
                 and the mouse: `click:COL,ROW`, `wheel:up@COL,ROW` (from 0, top left);
                 a dry run: Enter sends nothing to the bus and runs no slash command
  --depth DEPTH  colour depth: truecolor, 256, 16 or none; the terminal's by default
";

/// What the command line asked for.
struct Args {
    snap: Option<(u16, u16)>,
    keys: Vec<String>,
    depth: Option<ColorDepth>,
}

fn usage(msg: &str) -> ! {
    eprintln!("attend-chat: {msg}");
    std::process::exit(2);
}

fn parse_args(args: &[String]) -> Args {
    let mut out = Args { snap: None, keys: Vec::new(), depth: None };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--snap" => {
                let v = it.next().unwrap_or_else(|| usage("--snap needs WxH"));
                let (w, h) = v.split_once('x').unwrap_or_else(|| usage("--snap needs WxH"));
                match (w.parse(), h.parse()) {
                    (Ok(w), Ok(h)) if w > 0 && h > 0 => out.snap = Some((w, h)),
                    _ => usage("--snap needs WxH"),
                }
            }
            "--keys" => out.keys = it.next().unwrap_or_else(|| usage("--keys needs keys")).split_whitespace().map(str::to_string).collect(),
            "--depth" => {
                out.depth = Some(match it.next().map(String::as_str) {
                    Some("truecolor") => ColorDepth::TrueColor,
                    Some("256") => ColorDepth::Ansi256,
                    Some("16") => ColorDepth::Ansi16,
                    Some("none") => ColorDepth::NoColor,
                    _ => usage("--depth: one of truecolor, 256, 16, none"),
                })
            }
            other => usage(&format!("unknown argument `{other}`; see --help")),
        }
    }
    if !out.keys.is_empty() && out.snap.is_none() {
        usage("--keys goes with --snap");
    }
    out
}

/// The test kit's script: keys, `alt-<c>`, and the mouse (`click:C,R`,
/// `wheel:up@C,R`).
fn events(tokens: &[String]) -> Vec<agent_tui::ratatui::crossterm::event::Event> {
    testkit::parse_events(tokens.iter().map(String::as_str)).unwrap_or_else(|e| usage(&format!("--keys: {e}")))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("attend-chat {} ({})", env!("CARGO_PKG_VERSION"), env!("ATTEND_CHAT_COMMIT"));
        return;
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print!("{HELP}");
        return;
    }
    let args = parse_args(&args);
    // The screen needs a terminal on both ends. Started anywhere else (a
    // Monitor, a pipe) it says so and exits, before it takes a terminal or
    // installs a signal handler.
    if args.snap.is_none() && !(std::io::stdin().is_terminal() && std::io::stdout().is_terminal()) {
        usage("needs a terminal; `attend inbox` prints messages, `attend send` sends one");
    }

    let (tx, rx) = std::sync::mpsc::channel::<signal::Signal>();
    let base = signal::signals_base();
    let own_cwd = std::env::current_dir().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
    if let Err(e) = watcher::spawn_watcher(base.clone(), signal::encode_cwd(&own_cwd), tx) {
        eprintln!("attend-chat: failed to start signal watcher: {e}");
        eprintln!("  signals base: {}", base.display());
        eprintln!("  (no point opening the TUI — nothing would stream in.)");
        std::process::exit(1);
    }

    let choice = theme::choice();
    let depth = args.depth.unwrap_or_else(ColorDepth::detect);
    let (palette, warning) = theme::palette(choice.active.as_deref(), depth);
    let mut chat = Chat::new(Some(rx), palette, choice.shape);
    if let Some(w) = warning {
        chat.say(format!("theme: {w}"), true);
    }

    if let Some((w, h)) = args.snap {
        // Headless: the backlog the watcher read at start, the keys, one
        // frame. No heartbeat: a snapshot is no presence. A dry run: Enter
        // sends nothing and runs no slash command.
        let mut chat = chat.heartbeat(false).dry_run(true);
        chat.tick();
        // A frame before each key, as the terminal draws one before it
        // reads the next: what a key does can depend on what was drawn,
        // such as a page's height.
        testkit::play(&mut chat, &events(&args.keys), Some((w, h)));
        let frame = testkit::frame(&testkit::render_screen(&mut chat, w, h));
        let _ = std::io::stdout().write_all(frame.as_bytes());
        std::process::exit(0);
    }

    // The shell's signal handler stays installed after it closes, so the
    // process ends here, with 128 plus the signal when one ended it.
    let code = match agent_tui::run(chat.into_app()) {
        Ok(session) => session.signal.map_or(0, |sig| 128 + sig),
        Err(e) => {
            eprintln!("attend-chat: terminal: {e}");
            1
        }
    };
    std::process::exit(code);
}
