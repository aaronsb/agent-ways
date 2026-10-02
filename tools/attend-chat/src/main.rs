use std::io::{IsTerminal, Write};

use agent_theme::ColorDepth;
use agent_tui::screen::{run_screen, Screen};
use agent_tui::testkit;
use attend_chat::app::Chat;
use attend_chat::{signal, theme, watcher};

const HELP: &str = "attend-chat — interactive chat TUI for attend (ADR-120)

usage: attend-chat [--snap WxH [--keys \"KEYS\"]] [--depth DEPTH]

  Esc / Ctrl-C              exit
  Enter                     send to the foreground channel
  Tab                       cycle tabs (empty input) / complete @name #channel /command
  Alt+1..9                  jump to tab (1 = merged, 2 = #open, ...)
  Shift-Enter / Alt-Enter   insert newline
  Left / Right / Home / End move cursor
  Backspace / Delete        edit
  PgUp / PgDn               scroll the messages

  The colours follow the agent-ways theme (`ways settings theme`).

  --snap WxH     print one frame at W by H in agent-tui's frame format, headless
  --keys KEYS    keys for --snap, space separated (`text:hi enter tab esc alt-1` …);
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

/// `alt-<c>`, which the shared key script has no form for, then the
/// test kit's tokens.
fn keys(tokens: &[String]) -> Vec<agent_tui::ratatui::crossterm::event::KeyEvent> {
    use agent_tui::ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut out = Vec::new();
    for t in tokens {
        if let Some(c) = t.strip_prefix("alt-").filter(|c| c.chars().count() == 1) {
            out.push(KeyEvent::new(KeyCode::Char(c.chars().next().expect("one character")), KeyModifiers::ALT));
            continue;
        }
        match testkit::parse_keys([t.as_str()]) {
            Ok(k) => out.extend(k),
            Err(e) => usage(&format!("--keys: {e}")),
        }
    }
    out
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
        testkit::drive(&mut chat, &keys(&args.keys));
        let frame = testkit::frame(&testkit::render_screen(&mut chat, w, h));
        let _ = std::io::stdout().write_all(frame.as_bytes());
        std::process::exit(0);
    }

    // The screen's signal handler stays installed after it closes, so the
    // process ends here, with 128 plus the signal when one ended it.
    let code = match run_screen(&mut chat) {
        Ok(None) => 0,
        Ok(Some(sig)) => 128 + sig,
        Err(e) => {
            eprintln!("attend-chat: terminal: {e}");
            1
        }
    };
    std::process::exit(code);
}
