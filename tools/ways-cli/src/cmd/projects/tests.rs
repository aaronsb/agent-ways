//! `ways projects` against a fixture `~/.claude`, never the real one.

use super::*;
use std::path::{Path, PathBuf};

/// A fixture home: `<base>/home/.claude/projects/...`, `<base>/home/.claude.json`.
struct Fixture {
    base: PathBuf,
    env: Env,
}

/// 2026-10-01T00:00:00Z.
const NOW: u64 = 1_790_812_800;

impl Fixture {
    fn new(tag: &str) -> Self {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let base = std::env::temp_dir().join(format!(
            "ways-projects-{tag}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&base);
        let home = base.join("home");
        std::fs::create_dir_all(home.join(".claude/projects")).unwrap();
        let env = Env {
            claude: ClaudeDir::at(home.join(".claude")),
            claude_json: home.join(".claude.json"),
            home: home.to_string_lossy().into_owned(),
            now: NOW,
        };
        Fixture { base, env }
    }

    fn home(&self) -> PathBuf {
        PathBuf::from(&self.env.home)
    }

    fn projects(&self) -> PathBuf {
        self.env.projects()
    }

    fn write(&self, rel: &str, content: &str) -> PathBuf {
        let p = self.home().join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, content).unwrap();
        p
    }

    /// A transcript whose records name `cwd`, with its mtime set to `mtime`.
    fn transcript(&self, project: &str, sid: &str, cwd: &str, mtime: u64) -> PathBuf {
        let name = claude_sessions::project_slug(project);
        let body = format!(
            "{{\"type\":\"mode\",\"mode\":\"default\"}}\n{{\"type\":\"user\",\"cwd\":\"{cwd}\",\"message\":\"hello from {cwd}\"}}\n"
        );
        let p = self.write(&format!(".claude/projects/{name}/{sid}.jsonl"), &body);
        set_mtime(&p, mtime);
        p
    }

    fn run(&self, command: ProjectsCommand) -> (bool, String) {
        self.run_answering(command, false)
    }

    fn run_answering(&self, command: ProjectsCommand, yes: bool) -> (bool, String) {
        let mut out = Vec::new();
        let mut asked = false;
        let mut confirm = |_: &str| {
            asked = true;
            yes
        };
        let ok = dispatch(&self.env, Some(command), &mut out, &mut confirm).unwrap();
        (ok, String::from_utf8(out).unwrap())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

fn set_mtime(p: &Path, secs: u64) {
    let t = std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs);
    std::fs::File::options().write(true).open(p).unwrap().set_modified(t).unwrap();
}

fn relocate_args(old: &str, new: &str) -> RelocateArgs {
    RelocateArgs {
        old_path: old.to_string(),
        new_path: new.to_string(),
        execute: false,
        dry_run: false,
        keep_transcript_cwd: false,
        merge: false,
        force: false,
    }
}

/// Three projects: one active with memory (`/srv/app_one`), one older with
/// a legacy sessions index (`/srv/legacy`), one empty (`/srv/gone`).
fn populated(tag: &str) -> Fixture {
    let f = Fixture::new(tag);
    f.transcript("/srv/app_one", "s1", "/srv/app_one", NOW - 3_600);
    f.transcript("/srv/app_one", "s2", "/srv/app_one", NOW - 2 * 86_400);
    f.write(".claude/projects/-srv-app-one/memory/MEMORY.md", "# m\n");
    f.write(".claude/projects/-srv-app-one/memory/.gitkeep", "");
    f.transcript("/srv/legacy", "L1", "/srv/legacy", NOW - 40 * 86_400);
    f.write(
        ".claude/projects/-srv-legacy/sessions-index.json",
        r#"{"version":1,"originalPath":"/srv/legacy","entries":[
            {"sessionId":"L1","summary":"Port the parser","firstPrompt":"port the parser to rust","messageCount":12,"created":"2026-08-01T10:00:00Z","modified":"2026-08-20T10:00:00Z","gitBranch":"feat/parser","isSidechain":false},
            {"sessionId":"L0","summary":"Initial setup","firstPrompt":"set up the repo","messageCount":3,"created":"2026-07-01T10:00:00Z","modified":"2026-07-02T10:00:00Z","gitBranch":"main","isSidechain":true}
        ]}"#,
    );
    std::fs::create_dir_all(f.projects().join("-srv-gone/subagents")).unwrap();
    f
}

#[test]
fn list_orders_by_activity_and_marks_memory() {
    let f = populated("list");
    let (ok, out) = f.run(ProjectsCommand::List(ListArgs::default()));
    assert!(ok);
    assert!(out.contains("Claude Code Projects  (3 shown)"), "{out}");
    let rows: Vec<&str> = out.lines().filter(|l| l.contains("/srv/") || l.contains("-srv-")).collect();
    assert_eq!(rows.len(), 3, "{out}");
    // The path comes from the transcript cwd, `_` included.
    assert!(rows[0].contains("/srv/app_one") && rows[0].contains("2t") && rows[0].contains("today"));
    assert!(rows[0].trim_end().ends_with('●'));
    assert!(rows[1].contains("/srv/legacy") && rows[1].contains(" 2 ") && rows[1].contains("1mo"));
    assert!(rows[2].contains("–") && rows[2].trim_end().ends_with('·'));
}

#[test]
fn list_filters() {
    let f = populated("filters");
    let active = ListArgs { active: true, ..Default::default() };
    assert!(f.run(ProjectsCommand::List(active)).1.contains("(2 shown)"));
    let memory = ListArgs { memory: true, ..Default::default() };
    assert!(f.run(ProjectsCommand::List(memory)).1.contains("(1 shown)"));
    let stale = ListArgs { stale: true, ..Default::default() };
    let out = f.run(ProjectsCommand::List(stale)).1;
    assert!(out.contains("(1 shown)") && out.contains("-srv-gone"), "{out}");
    let urls = ListArgs { urls: true, ..Default::default() };
    let out = f.run(ProjectsCommand::List(urls)).1;
    assert!(out.contains("file:///srv/app_one"), "{out}");
}

#[test]
fn list_on_an_empty_projects_dir() {
    let f = Fixture::new("empty");
    assert_eq!(f.run(ProjectsCommand::List(ListArgs::default())).1, "No matching projects found.\n");
}

#[test]
fn search_scores_paths_and_summaries() {
    let f = populated("search");
    let (_, out) = f.run(ProjectsCommand::Search { query: "parser".into(), deep: false });
    assert!(out.contains("Search: parser  (1 matches)"), "{out}");
    assert!(out.contains("file:///srv/legacy") && out.contains("› Port the parser"), "{out}");
    let (_, out) = f.run(ProjectsCommand::Search { query: "app_one".into(), deep: false });
    assert!(out.contains("file:///srv/app_one") && out.contains("2t · today"), "{out}");
    let (_, out) = f.run(ProjectsCommand::Search { query: "hello".into(), deep: false });
    assert!(out.contains("try --deep"), "{out}");
    let (_, out) = f.run(ProjectsCommand::Search { query: "HELLO from".into(), deep: true });
    assert!(out.contains("Deep search") && out.contains("file:///srv/app_one"), "{out}");
}

#[test]
fn show_prints_index_sessions() {
    let f = populated("show");
    let (_, out) = f.run(ProjectsCommand::Show { project: "legacy".into() });
    assert!(out.contains("Dir: -srv-legacy"), "{out}");
    assert!(out.contains("First session: 2026-07-01"), "{out}");
    assert!(out.contains("Last active:  2026-08-20 (1mo)"), "{out}");
    assert!(out.contains("Last branch:  feat/parser"), "{out}");
    assert!(out.contains("Sessions:     2"), "{out}");
    assert!(out.contains("Memory:       no"), "{out}");
    assert!(out.contains("2026-07-02    3 msgs  main                 ⑂"), "{out}");
    let (_, out) = f.run(ProjectsCommand::Show { project: "app".into() });
    assert!(out.contains("Memory:       yes (1 files)"), "{out}");
    let (_, out) = f.run(ProjectsCommand::Show { project: "nothing".into() });
    assert_eq!(out, "No project matching 'nothing'\n");
}

#[test]
fn stats_totals() {
    let f = populated("stats");
    let (_, out) = f.run(ProjectsCommand::Stats);
    assert!(out.contains("Projects:          3"), "{out}");
    assert!(out.contains("with sessions:   1"), "{out}");
    assert!(out.contains("with transcripts:2"), "{out}");
    assert!(out.contains("with memory:     1"), "{out}");
    assert!(out.contains("empty:           1"), "{out}");
    assert!(out.contains("Total sessions:    2"), "{out}");
    assert!(out.contains("Total transcripts: 3"), "{out}");
    assert!(out.contains("       2  /srv/legacy"), "{out}");
}

#[test]
fn cleanup_dry_run_and_decline_remove_nothing() {
    let f = populated("cleanup-dry");
    let (_, out) = f.run(ProjectsCommand::Cleanup { dry_run: true });
    assert!(out.contains("Empty projects (1 found)") && out.contains("dry run"), "{out}");
    assert!(f.projects().join("-srv-gone").exists());
    let (_, out) = f.run_answering(ProjectsCommand::Cleanup { dry_run: false }, false);
    assert!(out.contains("Cancelled."), "{out}");
    assert!(f.projects().join("-srv-gone").exists());
}

#[test]
fn cleanup_removes_only_dirs_with_no_files() {
    let f = populated("cleanup");
    // Empty by the listing's measure, but holding a file: kept.
    f.write(".claude/projects/-srv-busy/subagents/agent-1.meta.json", "{}");
    let (_, out) = f.run_answering(ProjectsCommand::Cleanup { dry_run: false }, true);
    assert!(out.contains("Removed 1 directories."), "{out}");
    assert!(out.contains("skipped") && out.contains("(has 1 files)"), "{out}");
    assert!(!f.projects().join("-srv-gone").exists());
    assert!(f.projects().join("-srv-busy").exists());
    assert!(f.projects().join("-srv-app-one").exists());
}

#[test]
fn hygiene_finds_orphans_and_empty_dirs() {
    let f = populated("hygiene");
    f.transcript("/srv/legacy", "orphan", "/srv/legacy", NOW);
    std::fs::create_dir_all(f.projects().join("-srv-app-one/s1")).unwrap();
    let (_, out) = f.run(ProjectsCommand::Hygiene { dry_run: true });
    assert!(out.contains("Orphaned transcripts (not in sessions-index): 1"), "{out}");
    assert!(out.contains("Empty session dirs: 2"), "{out}");
    assert!(out.contains("pass without --dry-run"), "{out}");
    assert!(f.projects().join("-srv-legacy/orphan.jsonl").exists());

    let (_, out) = f.run_answering(ProjectsCommand::Hygiene { dry_run: false }, true);
    assert!(out.contains("Removed 1 files"), "{out}");
    assert!(!f.projects().join("-srv-legacy/orphan.jsonl").exists());
    // L1 is in the index and stays.
    assert!(f.projects().join("-srv-legacy/L1.jsonl").exists());
    assert!(!f.projects().join("-srv-app-one/s1").exists());
}

#[test]
fn relocate_previews_by_default() {
    let f = populated("reloc-preview");
    let before = std::fs::read(f.projects().join("-srv-app-one/s2.jsonl")).unwrap();
    let (ok, out) = f.run(ProjectsCommand::Relocate(relocate_args("/srv/app_one", "/srv/app-two")));
    assert!(ok, "{out}");
    assert!(out.contains("rename → -srv-app-two"), "{out}");
    assert!(out.contains("transcripts 2 cwd field(s) across 2 file(s)"), "{out}");
    assert!(out.contains("Preview only"), "{out}");
    // s1 was written an hour ago: not live. Nothing moved.
    assert!(!out.contains("warning"), "{out}");
    assert!(f.projects().join("-srv-app-one").exists());
    assert_eq!(std::fs::read(f.projects().join("-srv-app-one/s2.jsonl")).unwrap(), before);
}

#[test]
fn relocate_execute_moves_history_and_config() {
    let f = populated("reloc-exec");
    let target = f.base.join("work/app-two");
    let target_s = target.to_string_lossy().into_owned();
    std::fs::write(
        &f.env.claude_json,
        "{\n  \"a\": 1,\n  \"projects\": {\n    \"/x\": {},\n    \"/srv/app_one\": {\"k\": true},\n    \"/z\": {}\n  }\n}",
    )
    .unwrap();
    f.write(
        ".claude/history.jsonl",
        "{\"display\":\"hi\",\"project\":\"/srv/app_one\"}\n{\"display\":\"x\",\"project\":\"/other\"}\n",
    );

    let mut args = relocate_args("/srv/app_one", &target_s);
    args.execute = true;
    let (ok, out) = f.run(ProjectsCommand::Relocate(args));
    assert!(ok, "{out}");
    let new_name = claude_sessions::project_slug(&target_s);
    let new_dir = f.projects().join(&new_name);
    assert!(target.is_dir(), "the working directory is created");
    assert!(!f.projects().join("-srv-app-one").exists(), "{out}");
    let s1 = std::fs::read_to_string(new_dir.join("s1.jsonl")).unwrap();
    assert!(s1.contains(&format!("\"cwd\":\"{target_s}\"")), "{s1}");
    // Message text is left as the record of what happened.
    assert!(s1.contains("hello from /srv/app_one"), "{s1}");
    assert!(new_dir.join("memory/MEMORY.md").exists());

    let cfg: Value = serde_json::from_str(&std::fs::read_to_string(&f.env.claude_json).unwrap()).unwrap();
    let keys: Vec<&String> = cfg["projects"].as_object().unwrap().keys().collect();
    assert_eq!(keys, vec!["/x", &target_s, "/z"], "the renamed key keeps its place");
    let hist = std::fs::read_to_string(f.env.claude.history_file()).unwrap();
    assert!(hist.contains(&format!("\"project\":\"{target_s}\"")) && hist.contains("\"/other\""));
    assert!(out.contains("Done. Resume with:"), "{out}");
    assert!(out.contains("To reverse: ways projects relocate"), "{out}");
    let backups = std::fs::read_dir(f.home())
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with(".claude.json.bak-relocate-"))
        .count();
    assert_eq!(backups, 1);
}

#[test]
fn relocate_refuses_a_live_session_and_an_existing_target() {
    let f = populated("reloc-refuse");
    f.transcript("/srv/app_one", "live", "/srv/app_one", super::epoch_now());
    let target = f.base.join("work/t");
    let mut args = relocate_args("/srv/app_one", &target.to_string_lossy());
    args.execute = true;
    let (ok, out) = f.run(ProjectsCommand::Relocate(args));
    assert!(!ok && out.contains("Refusing to rewrite transcripts"), "{out}");
    assert!(f.projects().join("-srv-app-one/live.jsonl").exists());

    let mut args = relocate_args("/srv/legacy", "/srv/app_one");
    args.execute = true;
    let (ok, out) = f.run(ProjectsCommand::Relocate(args));
    assert!(!ok && out.contains("Refusing to overwrite an existing project directory"), "{out}");
    assert!(f.projects().join("-srv-legacy/L1.jsonl").exists());
}

#[test]
fn relocate_merge_unions_the_index() {
    let f = populated("reloc-merge");
    // Executed relocations only ever target paths inside the fixture.
    let new = f.base.join("work/new").to_string_lossy().into_owned();
    std::fs::create_dir_all(&new).unwrap();
    let new_name = claude_sessions::project_slug(&new);
    f.transcript(&new, "N1", &new, NOW - 86_400);
    f.write(
        &format!(".claude/projects/{new_name}/sessions-index.json"),
        &format!(r#"{{"entries":[{{"sessionId":"N1","projectPath":"{new}"}},{{"sessionId":"L1","summary":"stale copy"}}]}}"#),
    );
    let mut args = relocate_args("/srv/legacy", &new);
    args.execute = true;
    args.merge = true;
    let (ok, out) = f.run(ProjectsCommand::Relocate(args));
    assert!(ok, "{out}");
    assert!(out.contains("merged sessions-index.json (3 entries)"), "{out}");
    let new_dir = f.projects().join(&new_name);
    let idx: Value =
        serde_json::from_str(&std::fs::read_to_string(new_dir.join("sessions-index.json")).unwrap()).unwrap();
    let ids: Vec<&str> = idx["entries"].as_array().unwrap().iter().map(|e| str_field(e, "sessionId")).collect();
    assert_eq!(ids, vec!["N1", "L1", "L0"]);
    // The source wins for a shared id.
    assert_eq!(str_field(&idx["entries"][1], "summary"), "Port the parser");
    assert_eq!(str_field(&idx["entries"][1], "projectPath"), "");
    assert!(new_dir.join("L1.jsonl").exists());
    assert!(!f.projects().join("-srv-legacy").exists());
}

#[test]
fn relocate_reports_missing_history() {
    let f = populated("reloc-missing");
    let (ok, out) = f.run(ProjectsCommand::Relocate(relocate_args("/srv/app", "/srv/x")));
    assert!(!ok);
    assert!(out.contains("No session history for /srv/app"), "{out}");
    assert!(out.contains("Similar project directories:") && out.contains("-srv-app-one"), "{out}");
}

#[test]
fn heuristic_path_drops_the_user() {
    assert_eq!(heuristic_path("-home-aaron-Projects-foo"), "~/Projects/foo");
    assert_eq!(heuristic_path("-home-aaron"), "~");
    assert_eq!(heuristic_path("-srv-x"), "-srv-x");
}

#[test]
fn fmt_bytes_and_age_match_claude_projects() {
    assert_eq!(fmt_bytes(999), "999B");
    assert_eq!(fmt_bytes(1_500), "2K");
    assert_eq!(fmt_bytes(2_500_000), "2.5M");
    assert_eq!(fmt_bytes(3_000_000_000), "3.0G");
    assert_eq!(age(NOW, Some(NOW)), "today");
    assert_eq!(age(NOW, Some(NOW - 86_400)), "1d");
    assert_eq!(age(NOW, Some(NOW - 14 * 86_400)), "2w");
    assert_eq!(age(NOW, Some(NOW - 90 * 86_400)), "3mo");
    assert_eq!(age(NOW, Some(NOW - 800 * 86_400)), "2y");
    assert_eq!(age(NOW, None), "");
}
