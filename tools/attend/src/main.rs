mod cmd;
mod config;
mod config_lint;
mod emit;
mod groups;
mod identity_view;
mod scenes;
mod sensors;
mod state;
mod util;

use sensors::Focus;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    match args.first().map(|s| s.as_str()) {
        Some("run") => {
            let catchup = args.iter().any(|a| a == "--catchup");
            cmd::run::cmd_run_with_catchup(catchup);
        }
        Some("peers") => cmd::peers::cmd_peers(),
        Some("inbox") => {
            if let Some(msg_id) = args.get(1) {
                cmd::inbox::cmd_inbox_read(msg_id);
            } else {
                cmd::inbox::cmd_inbox();
            }
        }
        Some("status") => cmd::status::cmd_status(),
        Some("send") => {
            cmd::send::cmd_send(&args[1..]);
        }
        Some("chat") => {
            // `attend chat` is a thin shim that execs the standalone
            // `attend-chat` binary. Keeping the iocraft dependency
            // (and its async runtime) out of the attend crate means
            // `attend status`, `attend send`, and the sensor loop stay
            // cheap to cold-start from hooks. See ADR-120.
            use std::os::unix::process::CommandExt;
            let err = std::process::Command::new("attend-chat")
                .args(&args[1..])
                .exec();
            eprintln!("attend chat: failed to launch attend-chat: {}", err);
            eprintln!("  (is `attend-chat` on PATH? run `make install` from the repo root)");
            std::process::exit(1);
        }
        Some("reply") => {
            cmd::send::cmd_reply(&args[1..]);
        }
        Some("focus") => {
            cmd::focus::cmd_focus_new(&args[1..]);
        }
        Some("scene") => {
            cmd::scene::cmd_scene(&args[1..]);
        }
        Some("scenes") => {
            cmd::scene::cmd_scenes();
        }
        Some("tune") => {
            let apply = args.iter().any(|a| a == "--apply");
            cmd::tune::cmd_tune(apply);
        }
        Some("permissions") => match args.get(1).map(|s| s.as_str()) {
            Some("audit") | None => cmd::permissions::cmd_permissions_audit(),
            Some(sub) => {
                eprintln!("attend permissions: unknown subcommand '{}' — try audit", sub);
                std::process::exit(1);
            }
        },
        Some("cleanup") => {
            cmd::cleanup::cmd_cleanup(&args[1..]);
        }
        Some("config") => match args.get(1).map(|s| s.as_str()) {
            Some("init") => {
                let path = config::Config::init_user_config();
                println!("wrote default config to {}", path.display());
            }
            Some("show") | None => {
                let focus = Focus::default_focus();
                let cfg = config::Config::load(&focus.working_dir);
                cmd::config_cmd::display_config(&cfg);
            }
            Some("path") => {
                let home = std::env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| {
                    format!("{}/.config", std::env::var("HOME").unwrap_or_default())
                });
                println!("user:    {}/attend/config.yaml", home);
                let cwd = std::env::current_dir()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();
                println!("project: {}/.claude/attend.yaml", cwd);
            }
            Some("lint") => {
                let fix = args.iter().any(|a| a == "--fix");
                let check = args.iter().any(|a| a == "--check");
                let code = config_lint::run(fix, check);
                if code != 0 {
                    std::process::exit(code);
                }
            }
            Some(sub) => {
                eprintln!("attend config: unknown subcommand '{}' — try init, show, path, lint", sub);
                std::process::exit(1);
            }
        },
        Some("--version") | Some("-V") => {
            println!("attend {} ({})", env!("CARGO_PKG_VERSION"), env!("ATTEND_COMMIT"));
        }
        Some("help") | Some("--help") | Some("-h") | None => {
            let version = format!("v{} ({})", env!("CARGO_PKG_VERSION"), env!("ATTEND_COMMIT"));
            agent_fmt::Banner::new("ATTEND")
                .subtitle("active awareness for Claude Code sessions")
                .version(&version)
                .gradient(&agent_fmt::GRADIENT_TEAL)
                .print();
            println!("usage: attend <command>\n");
            agent_fmt::print_commands(
                "commands",
                &[
                    ("run", "Start the sensor loop (use with Monitor for async delivery)"),
                    ("peers", "List active Claude Code sessions and focus groups"),
                    ("inbox", "Read pending messages from peers"),
                    ("send", "Send a signal to peer sessions"),
                    ("reply", "Reply to the most recent peer message (auto-threaded)"),
                    ("chat", "Launch the interactive chat TUI (ADR-120)"),
                    ("focus", "Manage attention groups (on, off, list, all, clear, pin, dissolve)"),
                    ("scene", "Activate a named scene (reconfigure focus)"),
                    ("scenes", "List available scenes"),
                    ("config", "Manage configuration (init/show/path/lint)"),
                    ("tune", "Survey session history and derive engagement config (--apply to write)"),
                    ("permissions", "Audit sensor permissions against settings.json"),
                    ("cleanup", "Remove stale signal files from the signals base (default: 5m)"),
                    ("status", "Show running instances, signals, and focus state"),
                    ("help", "Show this help"),
                ],
            );
            println!();
            println!("  send defaults to broadcast (reaches every peer and Aaron).");
            agent_fmt::print_commands(
                "send flags (rarely needed)",
                &[
                    ("--focus <name>", "Scope send to a named group only"),
                    ("--to <path>", "Scope send to a specific project only"),
                ],
            );
        }
        Some(unknown) => {
            eprintln!("attend: unknown command '{}' — try 'attend help'", unknown);
            std::process::exit(1);
        }
    }
}
