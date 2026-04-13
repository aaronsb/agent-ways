mod config;
mod groups;
mod scenes;
mod state;
mod emit;
mod sensors;

use sensors::Focus;
use std::collections::BinaryHeap;
use std::path::Path;
use std::time::{Duration, Instant};

// --- Disclosure governor ---

struct DisclosureGovernor {
    base_cooldown: Duration,
    last_disclosure: Option<Instant>,
    max_disclosures_per_window: u32,
    window_disclosures: u32,
    window_start: Instant,
    rate_window: Duration,
    total_events: u32,
    total_events_start: Instant,
}

impl DisclosureGovernor {
    fn new(base_cooldown: Duration, max_per_window: u32, rate_window: Duration) -> Self {
        let now = Instant::now();
        Self {
            base_cooldown,
            last_disclosure: None,
            max_disclosures_per_window: max_per_window,
            window_disclosures: 0,
            window_start: now,
            rate_window,
            total_events: 0,
            total_events_start: now,
        }
    }

    fn record_event(&mut self) {
        self.total_events += 1;
    }

    fn aggregate_rate(&self) -> f64 {
        let elapsed = self.total_events_start.elapsed().as_secs_f64();
        if elapsed < 0.001 { return 0.0; }
        self.total_events as f64 / elapsed
    }

    fn cooldown(&self) -> Duration {
        let rate = self.aggregate_rate();
        let multiplier = 1.0 + rate.sqrt() * 3.0;
        self.base_cooldown.mul_f64(multiplier)
    }

    fn can_disclose(&mut self) -> bool {
        if self.window_start.elapsed() >= self.rate_window {
            self.window_disclosures = 0;
            self.window_start = Instant::now();
        }

        if self.window_disclosures >= self.max_disclosures_per_window {
            return false;
        }

        if let Some(last) = self.last_disclosure {
            if last.elapsed() < self.cooldown() {
                return false;
            }
        }

        true
    }

    fn record_disclosure(&mut self) {
        self.last_disclosure = Some(Instant::now());
        self.window_disclosures += 1;
    }
}

// --- Priority queue entry ---

struct ScheduledSensor {
    fire_at: Instant,
    index: usize,
}

impl Eq for ScheduledSensor {}
impl PartialEq for ScheduledSensor {
    fn eq(&self, other: &Self) -> bool { self.fire_at == other.fire_at }
}
impl Ord for ScheduledSensor {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other.fire_at.cmp(&self.fire_at)
    }
}
impl PartialOrd for ScheduledSensor {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

// --- Subcommands ---

fn cmd_run_with_catchup(catchup: bool) {
    emit::log("starting attend");

    let focus = Focus::default_focus();
    emit::log(&format!("focus: {} ({})", focus.description, focus.working_dir));

    // Load config: user scope → project scope overlay
    let cfg = config::Config::load(&focus.working_dir);

    // Initialize rooms for signal routing (ADR-118)
    let session_id = own_session_id().unwrap_or_else(|| format!("pid-{}", std::process::id()));
    let group_mgr = groups::Groups::new(&signals_base(), &session_id);

    // Self-documenting startup
    let my_groups = group_mgr.my_groups();
    let focus_desc = if my_groups.is_empty() {
        "project only".to_string()
    } else {
        let names: Vec<&str> = my_groups.iter().map(|(n, _)| n.as_str()).collect();
        format!("project + {}", names.join(", "))
    };

    // Register sensors from config + feature flags

    let (mut slots, enabled_names) = sensors::register_sensors(&cfg, &focus, catchup, &group_mgr);

    // Apply engagement config (ADR-119 action potential) to every slot.
    // All sensors share the same engagement parameters; per-sensor overrides
    // can be added later if the defaults turn out to be too coarse.
    for slot in &mut slots {
        slot.engagement = sensor_trait::EngagementState::with_params(
            cfg.engagement.burst_window,
            cfg.engagement.burst_threshold,
            cfg.engagement.step_multiplier,
            cfg.engagement.absolute_refractory,
            cfg.engagement.decay_per_minute,
        );
    }

    // State persistence
    let session_id = own_session_id();
    let state_store = state::StateStore::new(session_id);

    // Try to restore state from previous run
    if let Some(snapshot) = state_store.restore() {
        // Distribute state to matching sensors
        for slot in &mut slots {
            let sensor_state: Vec<(String, String)> = match slot.name() {
                "peers" => snapshot.seen_signals.iter()
                    .map(|s| ("seen_signal".to_string(), s.clone()))
                    .chain(std::iter::once(("reply_hint_shown".to_string(),
                        snapshot.reply_hint_shown.to_string())))
                    .collect(),
                "context" => snapshot.disclosed_thresholds.iter()
                    .map(|t| ("disclosed_threshold".to_string(), t.to_string()))
                    .collect(),
                _ => Vec::new(),
            };
            if !sensor_state.is_empty() {
                slot.import_state(&sensor_state);
            }
        }
    }

    let sensor_list = enabled_names.join(", ");
    let banner_fingerprint = format!(
        "v{}:{}:{}:{}",
        env!("CARGO_PKG_VERSION"), env!("ATTEND_COMMIT"), sensor_list, focus_desc
    );

    // Suppress repeated startup banners — only emit full banner when config changes
    let stamp_path = signals_base().join("_last_banner");
    let prev_fingerprint = std::fs::read_to_string(&stamp_path).unwrap_or_default();
    if banner_fingerprint == prev_fingerprint.trim() {
        println!("[attend] restarted (unchanged)");
    } else {
        println!("[attend] v{} ({}) — sensors: {} | focus: {} | send: attend send <msg> (broadcast)",
            env!("CARGO_PKG_VERSION"), env!("ATTEND_COMMIT"), sensor_list, focus_desc);
        std::fs::write(&stamp_path, &banner_fingerprint).ok();
    }

    let mut governor = DisclosureGovernor::new(
        cfg.governor.base_cooldown,
        cfg.governor.max_per_window,
        cfg.governor.rate_window,
    );

    let mut queue: BinaryHeap<ScheduledSensor> = BinaryHeap::new();
    for (i, slot) in slots.iter().enumerate() {
        queue.push(ScheduledSensor { fire_at: slot.next_fire, index: i });
    }

    // Checkpoint timer — save state every 30s
    let mut last_checkpoint = Instant::now();
    let checkpoint_interval = Duration::from_secs(30);

    // Auto-cleanup timer — prune stale signal files and empty project dirs.
    // Default 30-day retention + 10-minute sweep interval (see CleanupConfig).
    // Fire a first sweep on startup so long-running instances don't wait
    // a full interval before the first prune.
    let mut last_cleanup: Option<Instant> = None;
    let cleanup_enabled = cfg.cleanup.enabled;
    let cleanup_interval = cfg.cleanup.interval;
    let cleanup_retention = cfg.cleanup.retention;

    emit::log(&format!("tick loop running — {} sensors registered", slots.len()));
    for slot in &slots {
        emit::log(&format!(
            "  {} (base={:.0}s, min={:.0}s, threshold={:.1})",
            slot.name(),
            slot.sensor.base_interval().as_secs_f64(),
            slot.sensor.min_interval().as_secs_f64(),
            slot.sensor.emission_threshold(),
        ));
    }

    // Self-reload: track own binary mtime
    let self_exe = std::env::current_exe().ok();
    let initial_mtime = self_exe.as_ref()
        .and_then(|p| std::fs::metadata(p).ok())
        .and_then(|m| m.modified().ok());
    let mut last_reload_check = Instant::now();
    let reload_check_interval = Duration::from_secs(10);

    loop {
        // Check for binary change
        if last_reload_check.elapsed() >= reload_check_interval {
            if let (Some(ref exe), Some(ref orig_mtime)) = (&self_exe, &initial_mtime) {
                if let Ok(meta) = std::fs::metadata(exe) {
                    if let Ok(current_mtime) = meta.modified() {
                        if current_mtime != *orig_mtime {
                            // Binary changed — checkpoint and exec self
                            emit::log("binary changed — checkpointing and reloading");
                            let snapshot = collect_snapshot(&slots);
                            state_store.checkpoint(&snapshot);

                            // Flush stdout before exec to avoid losing buffered output
                            use std::io::Write;
                            std::io::stdout().flush().ok();

                            // exec self via std::os::unix
                            use std::os::unix::process::CommandExt;
                            let args: Vec<String> = std::env::args().collect();
                            let err = std::process::Command::new(&args[0])
                                .args(&args[1..])
                                .exec();
                            // exec() only returns on failure
                            emit::log(&format!("self-reload failed: {}", err));
                        }
                    }
                }
            }
            last_reload_check = Instant::now();
        }

        let next = match queue.peek() {
            Some(s) => s.fire_at,
            None => break,
        };

        let now = Instant::now();
        if next > now {
            std::thread::sleep(next - now);
        }

        let mut ready_indices = Vec::new();

        while let Some(scheduled) = queue.peek() {
            if scheduled.fire_at > Instant::now() {
                break;
            }
            let scheduled = queue.pop().unwrap();
            let i = scheduled.index;

            let changed = slots[i].poll(&focus);

            if changed {
                governor.record_event();
            }

            // Only log when something changed — quiet polls are silent
            if changed {
                let refractory = slots[i].effective_threshold()
                    .map(|t| format!("threshold={:.1}", t))
                    .unwrap_or_else(|| "ABSOLUTE REFRACTORY".to_string());
                emit::log(&format!(
                    "{}: change detected (interval={:.1}s, accum={:.1}, events={}, {})",
                    slots[i].name(),
                    slots[i].interval.current.as_secs_f64(),
                    slots[i].accumulator.magnitude,
                    slots[i].accumulator.event_count,
                    refractory,
                ));
            }

            if slots[i].ready_to_disclose() {
                ready_indices.push(i);
            } else if slots[i].accumulator.magnitude > 0.0 && changed {
                // Accumulated but blocked by refractory — log it so we can
                // see when action potential is holding the line.
                if slots[i].effective_threshold().is_none() {
                    emit::log(&format!(
                        "{}: held in absolute refractory (magnitude={:.1})",
                        slots[i].name(), slots[i].accumulator.magnitude,
                    ));
                }
            }

            slots[i].schedule_next();
            queue.push(ScheduledSensor { fire_at: slots[i].next_fire, index: i });
        }

        // Batch disclosure
        if !ready_indices.is_empty() && governor.can_disclose() {
            let mut batch = Vec::new();

            for &i in &ready_indices {
                let slot = &slots[i];
                let priority = if slot.accumulator.magnitude >= 5.0 {
                    "high"
                } else if slot.accumulator.magnitude >= 3.0 {
                    "medium"
                } else {
                    "low"
                };

                batch.push((
                    slot.name().to_string(),
                    priority.to_string(),
                    slot.accumulator.drain_events(),
                ));
            }

            emit::log(&format!(
                "disclosing batch of {} sensors (cooldown was {:.1}s)",
                batch.len(), governor.cooldown().as_secs_f64(),
            ));
            let emitted = emit::emit_batch(&batch);
            if emitted {
                governor.record_disclosure();
                // Record engagement only for sensors whose events actually
                // fired (not the quiet ones that got suppressed). Action
                // potential refractory is per-sensor.
                for &i in &ready_indices {
                    let slot = &slots[i];
                    let was_actionable = slot.accumulator.magnitude >= 3.0;
                    if was_actionable {
                        slots[i].engagement.record_disclosure();
                    }
                }
            }

            for &i in &ready_indices {
                slots[i].accumulator.reset();
            }
        } else if !ready_indices.is_empty() {
            emit::log(&format!(
                "{} sensors ready but governor holding ({}/{} in window)",
                ready_indices.len(),
                governor.window_disclosures,
                governor.max_disclosures_per_window,
            ));
        }

        // Periodic checkpoint
        if last_checkpoint.elapsed() >= checkpoint_interval {
            let snapshot = collect_snapshot(&slots);
            state_store.checkpoint(&snapshot);
            last_checkpoint = Instant::now();
        }

        // Periodic cleanup sweep — remove stale signal files and empty
        // project subdirs from the signals base. Scoped strictly to
        // attend's own data (~/.cache/attend/signals/); never touches
        // ways data or anything else.
        if cleanup_enabled {
            let due = match last_cleanup {
                None => true,
                Some(t) => t.elapsed() >= cleanup_interval,
            };
            if due {
                let base = signals_base();
                let stats = run_cleanup(&base, cleanup_retention, false, false);
                if stats.removed > 0 || stats.dirs_removed > 0 {
                    emit::log(&format!(
                        "cleanup: removed {} signal(s) ({} bytes), {} empty project dir(s)",
                        stats.removed, stats.bytes, stats.dirs_removed,
                    ));
                }
                last_cleanup = Some(Instant::now());
            }
        }
    }
}

fn collect_snapshot(slots: &[sensors::SensorSlot]) -> state::StateSnapshot {
    let mut snapshot = state::StateSnapshot::default();
    for slot in slots {
        for (key, value) in slot.export_state() {
            match key.as_str() {
                "seen_signal" => { snapshot.seen_signals.insert(value); }
                "disclosed_threshold" => {
                    if let Ok(t) = value.parse::<u8>() {
                        snapshot.disclosed_thresholds.push(t);
                    }
                }
                "reply_hint_shown" => {
                    snapshot.reply_hint_shown = value == "true";
                }
                "context_pct" => {
                    snapshot.context_pct = value.parse().ok();
                }
                _ => {}
            }
        }
    }
    snapshot
}

fn cmd_inbox_read(msg_id: &str) {
    let base = signals_base();
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let own_encoded = encode_project(&cwd);
    let r = get_groups();
    let mut scan_dirs = vec![
        base.join(&own_encoded),
        base.join("_broadcast"),
    ];
    for name in r.joined_group_names() {
        scan_dirs.push(r.group_dir(&name));
    }

    // Search for the signal file by ID
    let target = format!("{msg_id}.signal");
    for dir in &scan_dirs {
        let path = dir.join(&target);
        if !path.is_file() {
            continue;
        }
        // File exists: from here on, any failure is a corrupt-file
        // condition, not a benign "already consumed" miss. Distinguish
        // them so operators can tell partial-write / disk-full bugs
        // from ordinary races.
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("(signal {msg_id} exists but could not be read: {e})");
                return;
            }
        };
        let sig = match parse_signal(content.trim()) {
            Some(s) => s,
            None => {
                eprintln!("(signal {msg_id} exists but its wire format is corrupt)");
                return;
            }
        };
        let from = sig.from;
        let project = sig.project;
        let source_cwd = sig.cwd;
        let (kind, identity) = from.split_once(':').unwrap_or(("?", from));
        let sender = match kind {
            "claude" => format!("claude/{source_cwd}"),
            "external" => identity.to_string(),
            _ => format!("{project} ({from})"),
        };
        println!("From: {sender}");
        println!("ID:   {msg_id}");
        if let Some(re_id) = sig.reply_to {
            println!("Re:   {re_id}");
        }
        println!();
        println!("{}", sig.message);
        return;
    }
    // Benign miss — message may already be consumed or expired.
    // Exit 0 so callers don't treat a normal race as an error.
    println!("(no message by that id — already consumed or expired)");
}

fn cmd_inbox() {
    let base = signals_base();
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let own_session_id = own_session_id().unwrap_or_default();

    // Scan same dirs as the peer sensor: own project + broadcast + focus group
    let own_encoded = encode_project(&cwd);
    let r = get_groups();
    let mut scan_dirs = vec![
        base.join(&own_encoded),
        base.join("_broadcast"),
    ];
    // Add focus group dirs
    for name in r.joined_group_names() {
        scan_dirs.push(r.group_dir(&name));
    }

    // Collect all messages with mtime for chronological ordering
    struct InboxEntry {
        mtime: std::time::SystemTime,
        scope: String,
        sender: String,
        message: String,
        source: String,
        id: String,
        re: String,
    }
    let mut entries: Vec<InboxEntry> = Vec::new();

    for dir in &scan_dirs {
        let dir_entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => continue,
        };

        let dir_name = dir.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("?")
            .to_string();
        let scope = if dir_name == "_broadcast" { "broadcast" }
            else if dir_name == own_encoded { "project" }
            else { "focus" };

        for entry in dir_entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("signal") {
                continue;
            }

            let mtime = std::fs::metadata(&path)
                .ok()
                .and_then(|m| m.modified().ok())
                .unwrap_or(std::time::UNIX_EPOCH);

            let content = match std::fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let content = content.trim().to_string();
            let sig = match parse_signal(&content) {
                Some(s) => s,
                None => continue,
            };

            // Skip own messages
            if let Some((_, identity)) = sig.from.split_once(':') {
                if identity == own_session_id { continue; }
            }

            let (kind, identity) = sig.from.split_once(':').unwrap_or(("unknown", sig.from));
            let sender = match kind {
                "claude" => format!("claude/{}", sig.cwd),
                "external" => identity.to_string(),
                _ => format!("{} ({})", sig.project, sig.from),
            };

            let id = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();

            entries.push(InboxEntry {
                mtime,
                scope: scope.to_string(),
                sender,
                message: sig.message.to_string(),
                source: sig.cwd.to_string(),
                id,
                re: sig.reply_to.map(|s| s.to_string()).unwrap_or_default(),
            });
        }
    }

    // Sort chronologically — oldest first (ledger order)
    entries.sort_by_key(|e| e.mtime);

    if entries.is_empty() {
        println!("no messages");
    } else {
        // Render a stable 6-column layout regardless of whether any
        // message is threaded. The `Re` column stays empty for legacy
        // entries — visual stability beats saving a column, and the
        // inbox reshuffling mid-conversation as threads come and go
        // was surprising in review.
        let mut t = agent_fmt::Table::new(&["Scope", "From", "ID", "Re", "Message", "Source"]);
        t.max_width(0, 10);
        t.max_width(1, 24);
        t.max_width(2, 20);
        t.max_width(3, 20);
        for entry in &entries {
            t.add(vec![&entry.scope, &entry.sender, &entry.id, &entry.re, &entry.message, &entry.source]);
        }
        t.print();
        println!("  {} message(s)", entries.len());
    }
}

/// Parsed signal record (ADR-120 wire format).
///
/// Legacy signals have no `reply_to`; threaded replies carry the original
/// signal's ID in that field. Borrows from the input to keep the parse
/// allocation-free at the hot path.
struct ParsedSignal<'a> {
    from: &'a str,
    project: &'a str,
    cwd: &'a str,
    reply_to: Option<&'a str>,
    message: &'a str,
}

/// Signal IDs are filename stems in the form `<sender-id>-<timestamp>`,
/// which is always `[A-Za-z0-9_-]+`. Using this char class as the
/// discriminator fence keeps legacy prose that happens to start with
/// "re:" from being misparsed as threaded — e.g. `attend send "re: the
/// thing we discussed|still open"` stays a 4-field legacy message
/// because `the thing we discussed` has a space.
fn is_valid_signal_id(id: &str) -> bool {
    !id.is_empty()
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Parse a single-line signal. Accepts both the legacy 4-field format and
/// the 5-field threaded format; the discriminator is a `re:<id>|` prefix
/// on the field that follows `cwd`, where `<id>` matches
/// `is_valid_signal_id`. A malformed or ambiguous `re:` prefix degrades
/// to legacy interpretation so real prose round-trips cleanly.
fn parse_signal(content: &str) -> Option<ParsedSignal<'_>> {
    let parts: Vec<&str> = content.splitn(4, '|').collect();
    if parts.len() < 4 {
        return None;
    }
    let tail = parts[3];
    let (reply_to, message) = match tail.strip_prefix("re:").and_then(|rest| rest.split_once('|')) {
        Some((id, msg)) if is_valid_signal_id(id) => (Some(id), msg),
        // Either not threaded, or the `re:` prefix is followed by text
        // that doesn't look like a signal id — fall back to legacy so
        // prose like "re: the thing we discussed" stays intact.
        _ => (None, tail),
    };
    Some(ParsedSignal {
        from: parts[0],
        project: parts[1],
        cwd: parts[2],
        reply_to,
        message,
    })
}

fn cmd_peers() {
    let r = get_groups();

    #[cfg(feature = "sensor-peers")]
    let peers = {
        let sensor = sensors::PeerSensor::new();
        sensor.list_peers()
    };
    #[cfg(not(feature = "sensor-peers"))]
    let peers: Vec<(String, String, String, f64)> = Vec::new();

    let my_focus = r.my_groups();

    let mut t = agent_fmt::Table::new(&["Focus", "Agent", "Status", "Context"]);
    t.max_width(1, 24);

    // Show self in project group
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let self_project = cwd.rsplit('/').next().unwrap_or("?");
    t.add(vec!["(project)", self_project, "working", ""]);

    // Show named groups we're focused on
    for (name, pinned) in &my_focus {
        let pin_marker = if *pinned { " (pinned)" } else { "" };
        let label = format!("{name}{pin_marker}");
        t.add(vec![&label, "(you)", "", ""]);
    }

    // Show peers
    if !peers.is_empty() {
        t.add(vec!["", "", "", ""]);
        for (peer_cwd, project, status, ctx) in &peers {
            let focus_label = if *peer_cwd == cwd {
                "(project)".to_string()
            } else {
                String::new()
            };
            t.add(vec![&focus_label, project, status, &format!("{ctx:.0}%")]);
        }
    }

    t.print();

    let focus_count = my_focus.len();
    let peer_count = peers.len();
    println!(
        "  {} agent(s), {} focus group(s)",
        peer_count + 1,
        focus_count + 1
    );
}

fn cmd_send(args: &[String]) {
    // Parse flags: --broadcast, --to <project-path>, --focus <name>, --re <signal-id>
    let mut broadcast = false;
    let mut target_dir: Option<String> = None;
    let mut target_focus: Option<String> = None;
    let mut reply_to: Option<String> = None;
    let mut message_parts: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--broadcast" => broadcast = true,
            "--to" => {
                i += 1;
                if i < args.len() {
                    target_dir = Some(args[i].clone());
                } else {
                    eprintln!("attend send: --to requires a project path");
                    std::process::exit(1);
                }
            }
            "--focus" => {
                i += 1;
                if i < args.len() {
                    target_focus = Some(args[i].clone());
                } else {
                    eprintln!("attend send: --focus requires a focus group name");
                    std::process::exit(1);
                }
            }
            "--re" => {
                i += 1;
                if i < args.len() {
                    reply_to = Some(args[i].clone());
                } else {
                    eprintln!("attend send: --re requires a signal id");
                    std::process::exit(1);
                }
            }
            _ => message_parts.push(&args[i]),
        }
        i += 1;
    }

    // A signal id must match the same character class the parser uses to
    // disambiguate threaded records from legacy messages that happen to
    // start with "re:". Signal filename stems are `<sender-id>-<ts>`, so
    // `[A-Za-z0-9_-]+` comfortably covers the real shape and rejects
    // anything that would break the wire format (pipes, whitespace,
    // control chars) or trip the ambiguity fence in parse_signal.
    if let Some(ref id) = reply_to {
        if !is_valid_signal_id(id) {
            eprintln!("attend send: --re signal id must be non-empty and match [A-Za-z0-9_-]+");
            std::process::exit(1);
        }
    }

    let message = message_parts.join(" ");
    if message.is_empty() {
        eprintln!("usage: attend send <message>");
        eprintln!("  (reaches every peer and Aaron — no routing flags needed)");
        eprintln!("  tip: wrap message in double quotes to avoid shell expansion");
        std::process::exit(1);
    }

    // Fence: detect probable shell glob expansion.
    // If any message part is an existing file path, the shell likely
    // expanded a metachar (e.g. zsh expanded "hello?" into filenames).
    let suspect_expansion = message_parts.iter().any(|part| {
        std::path::Path::new(part).exists() && !part.contains(' ')
    });
    if suspect_expansion {
        eprintln!("[attend] warning: message contains existing file paths — shell may have expanded metacharacters");
        eprintln!("[attend] did you mean: attend send \"{}\"", message);
        eprintln!("[attend] sending anyway, but wrap in quotes next time");
    }

    let base = signals_base();
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

    // Validate --to path against active peers
    if let Some(ref path) = target_dir {
        let resolved = std::fs::canonicalize(path)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| path.clone());

        #[cfg(feature = "sensor-peers")]
        let peers = {
            let sensor = sensors::PeerSensor::new();
            sensor.list_peers()
        };
        #[cfg(not(feature = "sensor-peers"))]
        let peers: Vec<(String, String, String, String)> = Vec::new();
        let peer_paths: Vec<&str> = peers.iter().map(|(cwd, _, _, _)| cwd.as_str()).collect();

        if !peer_paths.contains(&resolved.as_str()) {
            eprintln!("error: no active peer at {}", resolved);
            if peers.is_empty() {
                eprintln!("\nno active peer sessions found");
            } else {
                eprintln!("\nactive peers:");
                for (peer_cwd, project, _, _) in &peers {
                    eprintln!("  {} ({})", peer_cwd, project);
                }
                // Fuzzy suggest: find closest match by path suffix
                if let Some(suggestion) = find_closest_peer(&resolved, &peer_paths) {
                    eprintln!("\ndid you mean: {}?", suggestion);
                }
            }
            std::process::exit(1);
        }
    }

    // Determine target directories.
    // Default is broadcast — simplest possible routing: every send reaches
    // every peer. Escape hatches remain for humans and scripts:
    //   --to <path>: specific project only
    //   --focus <name>: specific focus group only
    //   --broadcast: explicit (same as default)
    let r = get_groups();
    let dest_dirs: Vec<std::path::PathBuf> = if let Some(ref focus_name) = target_focus {
        vec![r.group_dir(focus_name)]
    } else if let Some(ref path) = target_dir {
        let resolved = std::fs::canonicalize(path)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| path.clone());
        vec![base.join(encode_project(&resolved))]
    } else {
        // Default (and --broadcast): reach everyone via the broadcast dir.
        let _ = broadcast; // flag now redundant, kept for compat
        vec![base.join("_broadcast")]
    };

    let (sender_id, source_kind) = identify_sender();
    let project = cwd.rsplit('/').next().unwrap_or("?");
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let from = format!("{}:{}", source_kind, sender_id);
    let filename = format!("{}-{}.signal", sender_id.replace('/', "-"), ts);
    // Wire format: `from|project|cwd|message` (legacy) or
    // `from|project|cwd|re:signal-id|message` (threaded reply). The `re:`
    // field is only emitted when --re was given; unthreaded sends stay
    // byte-identical to the pre-ADR-120 format.
    let content = match &reply_to {
        Some(id) => format!("{}|{}|{}|re:{}|{}\n", from, project, cwd, id, message),
        None => format!("{}|{}|{}|{}\n", from, project, cwd, message),
    };

    let scope = if target_focus.is_some() { "focus" }
        else if target_dir.is_some() { "directed" }
        else { "broadcast" };

    for dest_dir in &dest_dirs {
        std::fs::create_dir_all(dest_dir).ok();
        let path = dest_dir.join(&filename);
        let tmp_path = dest_dir.join(format!("{}.tmp", filename));

        match std::fs::write(&tmp_path, &content) {
            Ok(_) => {
                if let Err(e) = std::fs::rename(&tmp_path, &path) {
                    eprintln!("[attend] error renaming signal: {}", e);
                    std::fs::remove_file(&tmp_path).ok();
                }
            }
            Err(e) => eprintln!("[attend] error writing signal to {}: {}", dest_dir.display(), e),
        }
    }

    eprintln!("[attend] signal written ({}, {} dirs): {}", scope, dest_dirs.len(), filename);
}

// ─────────────────────────────────────────────────────────────────
// attend tune — survey session history and derive engagement config
// ─────────────────────────────────────────────────────────────────

fn cmd_tune(apply: bool) {
    let home = std::env::var("HOME").unwrap_or_default();
    let projects_root = std::path::PathBuf::from(&home).join(".claude").join("projects");

    // Gather the 10 most-recently-modified project directories.
    let mut proj_dirs: Vec<(std::path::PathBuf, std::time::SystemTime)> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&projects_root) {
        for entry in entries.flatten() {
            if let Ok(ft) = entry.file_type() {
                if ft.is_dir() {
                    let mt = entry.metadata()
                        .and_then(|m| m.modified())
                        .unwrap_or(std::time::UNIX_EPOCH);
                    proj_dirs.push((entry.path(), mt));
                }
            }
        }
    }
    proj_dirs.sort_by(|a, b| b.1.cmp(&a.1));
    proj_dirs.truncate(10);

    // For each project, take the 5 most-recent .jsonl files.
    let mut sessions: Vec<std::path::PathBuf> = Vec::new();
    for (proj, _) in &proj_dirs {
        let mut in_proj: Vec<(std::path::PathBuf, std::time::SystemTime)> = Vec::new();
        if let Ok(entries) = std::fs::read_dir(proj) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                    continue;
                }
                let mt = entry.metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::UNIX_EPOCH);
                in_proj.push((path, mt));
            }
        }
        in_proj.sort_by(|a, b| b.1.cmp(&a.1));
        in_proj.truncate(5);
        sessions.extend(in_proj.into_iter().map(|(p, _)| p));
    }

    eprintln!("[tune] surveying {} sessions across {} projects",
        sessions.len(), proj_dirs.len());

    let mut a2u_gaps: Vec<f64> = Vec::new();
    let mut u2u_gaps: Vec<f64> = Vec::new();

    for session in &sessions {
        let content = match std::fs::read_to_string(session) {
            Ok(c) => c,
            Err(_) => continue,
        };

        // Parse to (timestamp_secs, kind) where kind is 0=user, 1=assistant.
        //
        // Claude Code's JSONL format has the top-level `"type"` field AFTER
        // a nested `"message"` object, and that nested object contains its
        // own `"type":"message"` marker. A naive first-match extractor for
        // `"type"` picks the wrong one. We match the top-level discriminators
        // directly instead.
        let mut events: Vec<(f64, u8)> = Vec::new();
        for line in content.lines() {
            let is_assistant = line.contains("\"type\":\"assistant\"");
            let is_user = line.contains("\"type\":\"user\"");
            if !is_assistant && !is_user { continue; }

            let kind: u8 = if is_assistant {
                1
            } else {
                // user — skip tool_result entries (mechanical, not a real turn)
                if line.contains("\"type\":\"tool_result\"") { continue; }
                0
            };

            let Some(ts_str) = extract_json_str(line, "timestamp") else { continue; };
            let Some(ts) = parse_iso8601(&ts_str) else { continue; };
            events.push((ts, kind));
        }

        // Walk events computing gaps
        let mut last_assistant: Option<f64> = None;
        let mut last_user: Option<f64> = None;
        for (ts, kind) in &events {
            if *kind == 0 {
                // user
                if let Some(la) = last_assistant {
                    let gap = ts - la;
                    if gap > 0.0 && gap < 7200.0 {
                        a2u_gaps.push(gap);
                    }
                    last_assistant = None;
                }
                if let Some(lu) = last_user {
                    let gap = ts - lu;
                    if gap > 1.0 && gap < 7200.0 {
                        u2u_gaps.push(gap);
                    }
                }
                last_user = Some(*ts);
            } else {
                last_assistant = Some(*ts);
            }
        }
    }

    if u2u_gaps.is_empty() {
        eprintln!("[tune] no session data found — keeping defaults");
        return;
    }

    let pct = |data: &[f64], p: f64| -> f64 {
        let mut sorted: Vec<f64> = data.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let idx = (((sorted.len() as f64 - 1.0) * p).round() as usize).min(sorted.len() - 1);
        sorted[idx]
    };

    let a2u_median = pct(&a2u_gaps, 0.5);
    let a2u_p75 = pct(&a2u_gaps, 0.75);
    let a2u_p90 = pct(&a2u_gaps, 0.90);
    let u2u_median = pct(&u2u_gaps, 0.5);
    let u2u_p75 = pct(&u2u_gaps, 0.75);
    let u2u_p90 = pct(&u2u_gaps, 0.90);

    // Derive engagement config from percentiles.
    //
    // burst_window: p90 of the full turn cycle × burst_threshold. This is
    //   the window in which "3 turn cycles" would typically complete.
    //   Clamped to at least 5 minutes so very fast sessions still have a
    //   reasonable floor.
    //
    // absolute_refractory: median assistant→user gap (one "think time"
    //   pause). This is how long the other side typically takes to respond,
    //   so blocking disclosures for that long forces a natural beat.
    //
    // decay_per_minute: chosen so peak multiplier (2.25 at burst 3) decays
    //   back to rest (1.0) over ~2× burst_window minutes. That keeps the
    //   refractory in effect for roughly twice as long as the conversation
    //   that triggered it.
    //
    // peer_activity_window: same as burst_window.

    let burst_threshold = 3.0_f64;
    let step_multiplier = 1.25_f64;
    let peak_multiplier = 1.0 + (1.0 * step_multiplier); // peak at exactly burst_threshold

    let burst_window_s = (u2u_p90 * burst_threshold).clamp(300.0, 3600.0) as u64;
    let abs_refractory_s = a2u_median.clamp(15.0, 300.0) as u64;
    let burst_window_min = burst_window_s as f64 / 60.0;
    let decay_per_minute = (peak_multiplier - 1.0) / (2.0 * burst_window_min);

    println!();
    println!("=== attend tune — session survey ===");
    println!("  projects surveyed:  {}", proj_dirs.len());
    println!("  sessions parsed:    {}", sessions.len());
    println!("  turn samples:       {}", u2u_gaps.len());
    println!();
    println!("  assistant → user (think time):");
    println!("    median={:.0}s  p75={:.0}s  p90={:.0}s", a2u_median, a2u_p75, a2u_p90);
    println!("  user → user (full cycle):");
    println!("    median={:.0}s  p75={:.0}s  p90={:.0}s", u2u_median, u2u_p75, u2u_p90);
    println!();
    println!("=== derived engagement config ===");
    println!("engagement:");
    println!("  burst_window: {}          # {:.0}s p90 × {} burst threshold",
        burst_window_s, u2u_p90, burst_threshold as usize);
    println!("  burst_threshold: {}", burst_threshold as usize);
    println!("  step_multiplier: {}", step_multiplier);
    println!("  absolute_refractory: {}     # median think time", abs_refractory_s);
    println!("  decay_per_minute: {:.4}     # peak decays over 2× burst_window",
        decay_per_minute);
    println!("  peer_activity_window: {}    # matches burst_window", burst_window_s);
    println!();

    if apply {
        match apply_engagement_tune(burst_window_s, abs_refractory_s, decay_per_minute) {
            Ok(path) => println!("[tune] wrote updated engagement section to {}", path.display()),
            Err(e) => eprintln!("[tune] error writing config: {}", e),
        }
    } else {
        println!("(pass --apply to write these values to your attend config)");
    }
}

fn apply_engagement_tune(
    burst_window_s: u64,
    abs_refractory_s: u64,
    decay_per_minute: f64,
) -> std::io::Result<std::path::PathBuf> {
    let home = std::env::var("HOME").unwrap_or_default();
    let config_dir = std::env::var("XDG_CONFIG_HOME")
        .unwrap_or_else(|_| format!("{}/.config", home));
    let path = std::path::PathBuf::from(config_dir).join("attend").join("config.yaml");

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let existing = std::fs::read_to_string(&path).unwrap_or_else(|_| config::Config::default_yaml());

    let new_section = format!(
        "engagement:\n  burst_window: {}\n  burst_threshold: 3\n  step_multiplier: 1.25\n  absolute_refractory: {}\n  decay_per_minute: {:.4}\n  peer_activity_window: {}\n",
        burst_window_s, abs_refractory_s, decay_per_minute, burst_window_s,
    );

    let updated = replace_engagement_section(&existing, &new_section);
    std::fs::write(&path, updated)?;
    Ok(path)
}

/// Replace (or insert) the `engagement:` section in a YAML config string.
fn replace_engagement_section(existing: &str, new_section: &str) -> String {
    let mut result = String::new();
    let mut skipping = false;
    let mut found = false;

    for line in existing.lines() {
        let is_top_level = !line.is_empty()
            && !line.starts_with(' ')
            && !line.starts_with('\t');

        if is_top_level && line.starts_with("engagement:") {
            skipping = true;
            found = true;
            result.push_str(new_section);
            continue;
        }

        if skipping {
            // Stay in skip mode until we hit another top-level, non-comment line.
            if is_top_level && !line.starts_with('#') {
                skipping = false;
                // fall through to emit this line
            } else {
                continue;
            }
        }

        result.push_str(line);
        result.push('\n');
    }

    if !found {
        if !result.ends_with('\n') {
            result.push('\n');
        }
        result.push('\n');
        result.push_str(new_section);
    }

    result
}

/// Extract a "key":"value" string from a single JSON line (naive, fast).
fn extract_json_str(line: &str, key: &str) -> Option<String> {
    let needle = format!("\"{}\":\"", key);
    let start = line.find(&needle)? + needle.len();
    let rest = &line[start..];
    let mut end = None;
    let bytes = rest.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i += 2;
            continue;
        }
        if bytes[i] == b'"' {
            end = Some(i);
            break;
        }
        i += 1;
    }
    Some(rest[..end?].to_string())
}

/// Parse an ISO 8601 timestamp (YYYY-MM-DDTHH:MM:SS[.fff][Z|±HH:MM])
/// into seconds since the Unix epoch. Assumes UTC if a Z suffix or no
/// offset is present.
fn parse_iso8601(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.len() < 19 { return None; }
    let year: i32 = s.get(0..4)?.parse().ok()?;
    let month: u32 = s.get(5..7)?.parse().ok()?;
    let day: u32 = s.get(8..10)?.parse().ok()?;
    let hour: u32 = s.get(11..13)?.parse().ok()?;
    let minute: u32 = s.get(14..16)?.parse().ok()?;
    let second: u32 = s.get(17..19)?.parse().ok()?;

    let mut fraction: f64 = 0.0;
    if s.len() > 20 && s.as_bytes()[19] == b'.' {
        let rest = &s[20..];
        let end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
        let frac_str = &rest[..end];
        if !frac_str.is_empty() {
            if let Ok(v) = frac_str.parse::<f64>() {
                fraction = v / 10f64.powi(frac_str.len() as i32);
            }
        }
    }

    let days = days_from_civil(year, month, day);
    let seconds = days * 86400
        + (hour as i64) * 3600
        + (minute as i64) * 60
        + (second as i64);
    Some(seconds as f64 + fraction)
}

/// Days since 1970-01-01 (UTC) for a given civil date.
/// Howard Hinnant's algorithm — exact, no dependencies.
fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y / 400 } else { (y - 399) / 400 };
    let yoe = (y - era * 400) as u32;
    let m_adj = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * m_adj + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    (era as i64) * 146097 + (doe as i64) - 719468
}

fn cmd_status() {
    // Check if attend run is already active
    let output = std::process::Command::new("ps")
        .args(["--no-headers", "-eo", "pid,args"])
        .output()
        .ok();

    let mut instances: Vec<(String, String)> = Vec::new(); // (pid, info)
    if let Some(out) = output {
        let stdout = String::from_utf8_lossy(&out.stdout);
        let own_pid = std::process::id();
        for line in stdout.lines() {
            let line = line.trim();
            // Only match actual attend binary, not shell wrappers that contain "attend run"
            if !line.contains("attend run") || line.contains(&own_pid.to_string()) {
                continue;
            }
            // Skip zsh/bash wrapper lines (contain shell-snapshots or eval)
            if line.contains("shell-snapshots") || line.contains("eval '") {
                continue;
            }
            // Extract PID and show clean output
            let parts: Vec<&str> = line.splitn(2, char::is_whitespace).collect();
            if parts.len() == 2 {
                instances.push((parts[0].trim().to_string(), parts[1].trim().to_string()));
            }
        }
    }

    // Gather all data before building a single unified table
    let base = signals_base();
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let own_dir = base.join(encode_project(&cwd));
    let broadcast_dir = base.join("_broadcast");
    let own_count = count_signals(&own_dir);
    let broadcast_count = count_signals(&broadcast_dir);

    let r = get_groups();
    let my_focus = r.my_groups();

    // Single table: Section | Detail | Info
    let mut t = agent_fmt::Table::new(&["", "Detail", "Info"]);
    t.align(0, agent_fmt::Align::Left);

    // ── Instances section
    if instances.is_empty() {
        t.add(vec!["instances", "(none)", ""]);
    } else {
        for (i, (pid, cmd)) in instances.iter().enumerate() {
            let label = if i == 0 { "instances" } else { "" };
            t.add(vec![label, &format!("PID {pid}"), cmd]);
        }
    }

    // ── Separator
    t.add(vec!["", "", ""]);

    // ── Signals section
    t.add(vec!["signals", "project", &format!("{own_count} pending")]);
    t.add(vec!["", "broadcast", &format!("{broadcast_count} pending")]);

    // ── Separator
    t.add(vec!["", "", ""]);

    // ── Focus section
    if my_focus.is_empty() {
        t.add(vec!["focus", "project only", ""]);
    } else {
        for (i, (name, pinned)) in my_focus.iter().enumerate() {
            let label = if i == 0 { "focus" } else { "" };
            let pin = if *pinned { " (pinned)" } else { "" };
            let info = format!("{name}{pin}");
            t.add(vec![label, &info, ""]);
        }
    }

    t.print();
}

fn display_config(cfg: &config::Config) {
    // Governor section
    let mut t = agent_fmt::Table::new(&["", "Setting", "Value"]);
    t.align(0, agent_fmt::Align::Left);

    t.add(vec![
        "governor",
        "base_cooldown",
        &format!("{}s", cfg.governor.base_cooldown.as_secs()),
    ]);
    t.add(vec![
        "",
        "max_per_window",
        &cfg.governor.max_per_window.to_string(),
    ]);
    t.add(vec![
        "",
        "rate_window",
        &format!("{}s", cfg.governor.rate_window.as_secs()),
    ]);

    t.add(vec!["", "", ""]);

    // Engagement section (ADR-119 action potential)
    t.add(vec![
        "engagement",
        "burst_window",
        &format!("{}s", cfg.engagement.burst_window.as_secs()),
    ]);
    t.add(vec![
        "",
        "burst_threshold",
        &cfg.engagement.burst_threshold.to_string(),
    ]);
    t.add(vec![
        "",
        "step_multiplier",
        &format!("{:.2}", cfg.engagement.step_multiplier),
    ]);
    t.add(vec![
        "",
        "absolute_refractory",
        &format!("{}s", cfg.engagement.absolute_refractory.as_secs()),
    ]);
    t.add(vec![
        "",
        "decay_per_minute",
        &format!("{:.4}", cfg.engagement.decay_per_minute),
    ]);
    t.add(vec![
        "",
        "peer_activity_window",
        &format!("{}s", cfg.engagement.peer_activity_window.as_secs()),
    ]);

    t.add(vec!["", "", ""]);

    // Sensors — sorted by name
    let mut names: Vec<&String> = cfg.sensors.keys().collect();
    names.sort();

    for (i, name) in names.iter().enumerate() {
        let sc = &cfg.sensors[*name];
        let sensor_type = if sc.script.is_some() { "script" } else { "crate" };
        let enabled = if sc.enabled { "" } else { " (disabled)" };
        let label = format!("{name}{enabled}");

        let section = if i == 0 { "sensors" } else { "" };
        t.add(vec![
            section,
            &label,
            &format!(
                "[{sensor_type}] interval={}s min={}s threshold={} decay={}",
                sc.interval.as_secs(),
                sc.min_interval.as_secs(),
                sc.threshold,
                sc.decay_threshold,
            ),
        ]);

        if let Some(ref script) = sc.script {
            t.add(vec!["", "", &format!("script: {script}")]);
        }
        if !sc.requires.is_empty() {
            t.add(vec!["", "", &format!("requires: [{}]", sc.requires.join(", "))]);
        }
    }

    t.print();
}

fn count_signals(dir: &std::path::Path) -> usize {
    std::fs::read_dir(dir)
        .map(|entries| entries
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("signal"))
            .count())
        .unwrap_or(0)
}

// --- Helpers ---

/// Determine who is sending this signal.
/// Returns (identity_string, source_kind) where source_kind is "claude" or "external".
fn identify_sender() -> (String, &'static str) {
    // First, try to find a Claude session ID (we're inside a Claude session)
    if let Some(sid) = own_session_id() {
        return (sid, "claude");
    }

    // Not inside Claude — build identity from environment
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "unknown".to_string());

    // Detect terminal: check common terminal-specific env vars
    let terminal = detect_terminal();

    let identity = if !terminal.is_empty() {
        format!("{}@{}", user, terminal)
    } else {
        user
    };

    (identity, "external")
}

/// Best-effort terminal detection from environment variables.
fn detect_terminal() -> String {
    // Specific terminal emulators set their own env vars
    if std::env::var("KITTY_PID").is_ok() {
        return "kitty".to_string();
    }
    if std::env::var("ALACRITTY_SOCKET").is_ok() {
        return "alacritty".to_string();
    }
    if std::env::var("WEZTERM_PANE").is_ok() {
        return "wezterm".to_string();
    }
    if std::env::var("TMUX").is_ok() {
        return "tmux".to_string();
    }
    if std::env::var("STY").is_ok() {
        return "screen".to_string();
    }
    // TERM_PROGRAM is set by some terminals (macOS Terminal, iTerm2, VS Code)
    if let Ok(tp) = std::env::var("TERM_PROGRAM") {
        return tp.to_lowercase();
    }
    // SSH session
    if std::env::var("SSH_CONNECTION").is_ok() {
        return "ssh".to_string();
    }
    // Fallback: try TERMINAL or just use the shell
    if let Ok(t) = std::env::var("TERMINAL") {
        return t.rsplit('/').next().unwrap_or(&t).to_string();
    }
    String::new()
}

/// Find the closest matching peer path by comparing path suffixes.
fn find_closest_peer<'a>(target: &str, peers: &[&'a str]) -> Option<&'a str> {
    // Try matching the last N segments of the target against peer paths
    let target_parts: Vec<&str> = target.rsplit('/').collect();
    let mut best: Option<(&str, usize)> = None;

    for peer in peers {
        let peer_parts: Vec<&str> = peer.rsplit('/').collect();
        let common = target_parts.iter().zip(peer_parts.iter())
            .take_while(|(a, b)| a == b)
            .count();
        if common > 0 && (best.is_none() || common > best.unwrap().1) {
            best = Some((peer, common));
        }
    }

    best.map(|(p, _)| p)
}

fn signals_base() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    std::path::PathBuf::from(home)
        .join(".cache")
        .join("attend")
        .join("signals")
}

/// Encode a project path the same way Claude Code does: '/', '_', '.' → '-'
fn encode_project(path: &str) -> String {
    path.chars()
        .map(|c| match c {
            '/' | '_' | '.' => '-',
            _ => c,
        })
        .collect()
}

/// Delegate to the shared implementation in the peer sensor module.
fn own_session_id() -> Option<String> {
    #[cfg(feature = "sensor-peers")]
    { sensors::find_own_session_id(std::process::id()) }
    #[cfg(not(feature = "sensor-peers"))]
    { None }
}

// --- Entry point ---

fn cmd_scene(args: &[String]) {
    let name = match args.first() {
        Some(n) => n,
        None => {
            eprintln!("usage: attend scene <name>");
            eprintln!("  try: attend scenes (to list available)");
            std::process::exit(1);
        }
    };

    let r = get_groups();
    match scenes::activate(name, &r) {
        Ok(result) => println!("[attend] scene '{name}': {result}"),
        Err(e) => {
            eprintln!("[attend] scene: {e}");
            std::process::exit(1);
        }
    }
}

fn cmd_scenes() {
    let all = scenes::load_scenes();
    let mut names: Vec<&String> = all.keys().collect();
    names.sort();

    let mut t = agent_fmt::Table::new(&["Scene", "Focus groups"]);
    for name in &names {
        let scene = &all[*name];
        let groups_str = if scene.rooms.is_empty() {
            "(none — project only)".to_string()
        } else {
            scene.rooms.join(", ")
        };
        t.add(vec![name.as_str(), &groups_str]);
    }
    t.print();
}

fn get_groups() -> groups::Groups {
    let session_id = own_session_id().unwrap_or_else(|| format!("pid-{}", std::process::id()));
    groups::Groups::new(&signals_base(), &session_id)
}

fn cmd_focus_new(args: &[String]) {
    let r = get_groups();

    match args.first().map(|s| s.as_str()) {
        Some("on") => {
            let name = match args.get(1) {
                Some(n) => n,
                None => {
                    eprintln!("usage: attend focus on <name> [--pin]");
                    std::process::exit(1);
                }
            };
            let pin = args.iter().any(|a| a == "--pin");
            match r.join(name, pin) {
                Ok(()) => {
                    let suffix = if pin { " (pinned)" } else { "" };
                    println!("[attend] focus: attending to {name}{suffix}");
                }
                Err(e) => {
                    eprintln!("[attend] focus: {e}");
                    std::process::exit(1);
                }
            }
        }
        Some("off") => {
            let name = match args.get(1) {
                Some(n) => n,
                None => {
                    eprintln!("usage: attend focus off <name>");
                    std::process::exit(1);
                }
            };
            r.leave(name).ok();
            println!("[attend] focus: released {name}");
        }
        Some("clear") => {
            for (name, _) in r.my_groups() {
                r.leave(&name).ok();
            }
            println!("[attend] focus: cleared (project only)");
        }
        Some("pin") => {
            let name = match args.get(1) {
                Some(n) => n,
                None => {
                    eprintln!("usage: attend focus pin <name>");
                    std::process::exit(1);
                }
            };
            r.pin(name);
            println!("[attend] focus: pinned {name}");
        }
        Some("unpin") => {
            let name = match args.get(1) {
                Some(n) => n,
                None => {
                    eprintln!("usage: attend focus unpin <name>");
                    std::process::exit(1);
                }
            };
            r.unpin(name);
            println!("[attend] focus: unpinned {name}");
        }
        Some("dissolve") => {
            let name = match args.get(1) {
                Some(n) => n,
                None => {
                    eprintln!("usage: attend focus dissolve <name>");
                    std::process::exit(1);
                }
            };
            let members = r.dissolve(name);
            if members.is_empty() {
                println!("[attend] focus: dissolved {name} (was empty)");
            } else {
                println!("[attend] focus: dissolved {name} ({} members released)", members.len());
            }
        }
        Some("all") => {
            r.cleanup_stale();
            let all = r.all_groups();
            if all.is_empty() {
                println!("no active focus groups");
                return;
            }
            let mut t = agent_fmt::Table::new(&["Focus", "Members", "Pinned"]);
            t.align(1, agent_fmt::Align::Right);
            for (name, count, pinned) in &all {
                t.add(vec![
                    name.as_str(),
                    &count.to_string(),
                    if *pinned { "yes" } else { "no" },
                ]);
            }
            t.print();
        }
        Some("list") | None => {
            let my = r.my_groups();
            if my.is_empty() {
                println!("focus: project only");
            } else {
                let mut t = agent_fmt::Table::new(&["Focus", "Pinned"]);
                for (name, pinned) in &my {
                    t.add(vec![name.as_str(), if *pinned { "yes" } else { "no" }]);
                }
                t.print();
            }
        }
        Some(unknown) => {
            eprintln!("attend focus: unknown subcommand '{unknown}' — try on, off, list, all, clear, pin, unpin, dissolve");
            std::process::exit(1);
        }
    }
}

// --- Permissions audit (ADR-116) ---

fn cmd_permissions_audit() {
    use agent_fmt::permissions;

    let focus = Focus::default_focus();
    let cfg = config::Config::load(&focus.working_dir);

    // Load settings.json grants
    let home = std::env::var("HOME").unwrap_or_default();
    let settings_path = std::path::PathBuf::from(&home).join(".claude/settings.json");
    let grants = permissions::load_settings_permissions(&settings_path);

    if grants.is_empty() {
        eprintln!("Warning: no permissions found in {}", settings_path.display());
    }

    // Collect (sensor_name, requires) pairs from config
    let mut requirements: Vec<(String, Vec<String>)> = Vec::new();
    let mut names: Vec<&String> = cfg.sensors.keys().collect();
    names.sort();
    for name in names {
        let sensor = &cfg.sensors[name];
        if !sensor.requires.is_empty() {
            let prefix = if sensor.script.is_some() { "+" } else { "" };
            requirements.push((
                format!("{prefix}{name}"),
                sensor.requires.clone(),
            ));
        }
    }

    let results = permissions::audit(&requirements, &grants);
    permissions::display_audit("Attend Permissions Audit", "Sensor", &results, false);
}

/// Parse a duration like "30s", "5m", "1h". Bare digits are treated as seconds.
fn parse_duration_arg(s: &str) -> Option<Duration> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let (num, unit) = match s.chars().last()? {
        c if c.is_ascii_digit() => (s, "s"),
        _ => s.split_at(s.len() - 1),
    };
    let n: u64 = num.parse().ok()?;
    let mult: u64 = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3600,
        "d" => 86400,
        _ => return None,
    };
    Some(Duration::from_secs(n * mult))
}

/// Statistics from a cleanup sweep.
#[derive(Default, Debug)]
struct CleanupStats {
    examined: u64,
    removed: u64,
    bytes: u64,
    dirs_removed: u64,
}

/// Core cleanup routine, shared by `attend cleanup` and the in-loop auto-sweep.
///
/// Two passes over the signals base:
///   1. Remove stale `*.signal` files older than `older_than` (or all if `nuke_all`).
///   2. Remove now-empty encoded-cwd project subdirs — the shells left behind
///      after projects go dormant. Never removes `_broadcast`, `@groups`, or
///      any dir containing non-signal files (e.g., `_groups.yaml`).
///
/// On `dry_run`, emits a line per candidate to stdout instead of deleting.
fn run_cleanup(base: &Path, older_than: Duration, dry_run: bool, nuke_all: bool) -> CleanupStats {
    let mut stats = CleanupStats::default();
    if !base.is_dir() {
        return stats;
    }

    let now = std::time::SystemTime::now();
    let entries = match std::fs::read_dir(base) {
        Ok(e) => e,
        Err(_) => return stats,
    };

    // Pass 1: prune stale signal files.
    for sub in entries.flatten() {
        let subpath = sub.path();
        if !subpath.is_dir() {
            continue;
        }
        let files = match std::fs::read_dir(&subpath) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for f in files.flatten() {
            let path = f.path();
            let name = match path.file_name().and_then(|s| s.to_str()) {
                Some(n) => n,
                None => continue,
            };
            if !name.ends_with(".signal") {
                continue;
            }
            stats.examined += 1;

            let meta = match f.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            let age = meta
                .modified()
                .ok()
                .and_then(|mt| now.duration_since(mt).ok())
                .unwrap_or(Duration::ZERO);

            if !nuke_all && age < older_than {
                continue;
            }

            let size = meta.len();
            if dry_run {
                println!("would remove {} ({}s old, {} bytes)", path.display(), age.as_secs(), size);
            } else if std::fs::remove_file(&path).is_ok() {
                stats.removed += 1;
                stats.bytes += size;
            }
        }
    }

    // Pass 2: remove empty encoded-cwd project subdirs left as shells.
    // A project subdir is a non-reserved name (not _broadcast, not @group,
    // not _anything) that now contains nothing. Focus-group dirs self-clean
    // on leave/dissolve already; we don't touch those here.
    if let Ok(entries) = std::fs::read_dir(base) {
        for sub in entries.flatten() {
            let subpath = sub.path();
            if !subpath.is_dir() {
                continue;
            }
            let name = match subpath.file_name().and_then(|s| s.to_str()) {
                Some(n) => n,
                None => continue,
            };
            // Reserved names we never touch.
            if name.starts_with('_') || name.starts_with('@') {
                continue;
            }
            // Dir is a candidate only if fully empty now.
            let empty = std::fs::read_dir(&subpath)
                .map(|mut it| it.next().is_none())
                .unwrap_or(false);
            if !empty {
                continue;
            }
            if dry_run {
                println!("would remove empty project dir {}", subpath.display());
            } else if std::fs::remove_dir(&subpath).is_ok() {
                stats.dirs_removed += 1;
            }
        }
    }

    stats
}

fn cmd_cleanup(args: &[String]) {
    // Default to the config's retention so the manual command's semantics
    // match the auto-sweep by default. Overrides with --older-than.
    let focus = Focus::default_focus();
    let cfg = config::Config::load(&focus.working_dir);
    let mut older_than = cfg.cleanup.retention;
    let mut dry_run = false;
    let mut nuke_all = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--dry-run" | "-n" => dry_run = true,
            "--all" => nuke_all = true,
            "--older-than" => {
                if let Some(v) = args.get(i + 1) {
                    match parse_duration_arg(v) {
                        Some(d) => older_than = d,
                        None => {
                            eprintln!("attend cleanup: invalid duration '{}' — try 5m, 1h, 30s", v);
                            std::process::exit(2);
                        }
                    }
                    i += 1;
                } else {
                    eprintln!("attend cleanup: --older-than requires a value");
                    std::process::exit(2);
                }
            }
            "--help" | "-h" => {
                println!("attend cleanup — remove stale signal files from ~/.cache/attend/signals/\n");
                println!("usage: attend cleanup [--older-than <dur>] [--dry-run] [--all]\n");
                println!("  --older-than <dur>  age cutoff (default: cleanup.retention from config)");
                println!("                       duration format: 30s, 5m, 1h, 2d");
                println!("  --dry-run, -n       list what would be removed without deleting");
                println!("  --all               remove every signal file regardless of age");
                println!();
                println!("Auto-cleanup also runs inside `attend run` every cleanup.interval seconds.");
                return;
            }
            other => {
                eprintln!("attend cleanup: unknown flag '{other}' — try --help");
                std::process::exit(2);
            }
        }
        i += 1;
    }

    let base = signals_base();
    if !base.is_dir() {
        println!("no signals base at {} — nothing to clean", base.display());
        return;
    }

    let stats = run_cleanup(&base, older_than, dry_run, nuke_all);

    if dry_run {
        println!("\ndry run: examined {} signal file(s)", stats.examined);
    } else {
        println!(
            "cleaned up {} signal file(s), freed {} bytes (examined {}); removed {} empty project dir(s)",
            stats.removed, stats.bytes, stats.examined, stats.dirs_removed,
        );
    }
}

// --- Entry point ---

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    match args.first().map(|s| s.as_str()) {
        Some("run") => {
            let catchup = args.iter().any(|a| a == "--catchup");
            cmd_run_with_catchup(catchup);
        }
        Some("peers") => cmd_peers(),
        Some("inbox") => {
            if let Some(msg_id) = args.get(1) {
                cmd_inbox_read(msg_id);
            } else {
                cmd_inbox();
            }
        }
        Some("status") => cmd_status(),
        Some("send") => {
            cmd_send(&args[1..]);
        }
        Some("focus") => {
            cmd_focus_new(&args[1..]);
        }
        Some("scene") => {
            cmd_scene(&args[1..]);
        }
        Some("scenes") => {
            cmd_scenes();
        }
        Some("tune") => {
            let apply = args.iter().any(|a| a == "--apply");
            cmd_tune(apply);
        }
        Some("permissions") => {
            match args.get(1).map(|s| s.as_str()) {
                Some("audit") | None => cmd_permissions_audit(),
                Some(sub) => {
                    eprintln!("attend permissions: unknown subcommand '{}' — try audit", sub);
                    std::process::exit(1);
                }
            }
        }
        Some("cleanup") => {
            cmd_cleanup(&args[1..]);
        }
        Some("config") => {
            match args.get(1).map(|s| s.as_str()) {
                Some("init") => {
                    let path = config::Config::init_user_config();
                    println!("wrote default config to {}", path.display());
                }
                Some("show") | None => {
                    let focus = Focus::default_focus();
                    let cfg = config::Config::load(&focus.working_dir);
                    display_config(&cfg);
                }
                Some("path") => {
                    let home = std::env::var("XDG_CONFIG_HOME")
                        .unwrap_or_else(|_| format!("{}/.config", std::env::var("HOME").unwrap_or_default()));
                    println!("user:    {}/attend/config.yaml", home);
                    let cwd = std::env::current_dir()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_default();
                    println!("project: {}/.claude/attend.yaml", cwd);
                }
                Some(sub) => {
                    eprintln!("attend config: unknown subcommand '{}' — try init, show, path", sub);
                    std::process::exit(1);
                }
            }
        }
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
            agent_fmt::print_commands("commands", &[
                ("run",         "Start the sensor loop (use with Monitor for async delivery)"),
                ("peers",       "List active Claude Code sessions and focus groups"),
                ("inbox",       "Read pending messages from peers"),
                ("send",        "Send a signal to peer sessions"),
                ("focus",       "Manage attention groups (on, off, list, all, clear, pin, dissolve)"),
                ("scene",       "Activate a named scene (reconfigure focus)"),
                ("scenes",      "List available scenes"),
                ("config",      "Manage configuration (init/show/path)"),
                ("tune",        "Survey session history and derive engagement config (--apply to write)"),
                ("permissions", "Audit sensor permissions against settings.json"),
                ("cleanup",     "Remove stale signal files from the signals base (default: 5m)"),
                ("status",      "Show running instances, signals, and focus state"),
                ("help",        "Show this help"),
            ]);
            println!();
            println!("  send defaults to broadcast (reaches every peer and Aaron).");
            agent_fmt::print_commands("send flags (rarely needed)", &[
                ("--focus <name>", "Scope send to a named group only"),
                ("--to <path>",    "Scope send to a specific project only"),
            ]);
        }
        Some(unknown) => {
            eprintln!("attend: unknown command '{}' — try 'attend help'", unknown);
            std::process::exit(1);
        }
    }
}
