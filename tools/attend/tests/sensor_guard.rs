// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! A process a sensor command started does not act as the session: its
//! ancestry passes through an attend sensor host, so every verb that sends,
//! joins, listens, drains or reads as the session refuses. The session's
//! own callers still resolve: the mod's calls, a Bash tool shell, the
//! Stop-hook drain and `attend run` itself.

#![cfg(unix)]

mod common;

use std::path::Path;
use std::time::Duration;

use common::Fixture;

const REFUSAL: &str = "this attend was started by a sensor command, so it does not act as the session";

/// A command sensor run by `attend run` whose command runs attend verbs
/// and records each one's stderr and exit status.
#[test]
fn refused_an_attend_a_sensor_command_runs_does_not_act_as_the_session() {
    let f = Fixture::new("guard-refused");
    let out = f.home.join("probe");
    std::fs::create_dir_all(&out).unwrap();
    let script = format!(
        "a='{bin}'; o='{out}'; \"$a\" peer whoami --machine > \"$o/whoami.out\" 2> \"$o/whoami.err\"; echo $? > \"$o/whoami.code\"; \
         \"$a\" message send from-a-sensor 2> \"$o/send.err\"; echo $? > \"$o/send.code\"; \
         \"$a\" channel join probes 2> \"$o/join.err\"; echo $? > \"$o/join.code\"; \
         \"$a\" peer list > /dev/null 2> \"$o/list.err\"; echo $? > \"$o/list.code\"; echo '{{}}'",
        bin = env!("CARGO_BIN_EXE_attend"),
        out = out.display()
    );
    f.sensor("probe", &format!("kind: command\ninterval: 1\nmin_interval: 1\nargv: [sh, -c, {}]\noutput: json\n", serde_json::to_string(&script).unwrap()));
    let run = f.run();
    run.wait_for("the sensor command to finish", Duration::from_secs(30), |_| out.join("list.code").exists());
    drop(run);
    let read = |n: &str| std::fs::read_to_string(out.join(n)).unwrap_or_default();
    assert_eq!(read("whoami.code").trim(), "3", "peer whoami: {}", read("whoami.err"));
    assert!(read("whoami.err").contains(REFUSAL), "{}", read("whoami.err"));
    assert!(!read("whoami.out").contains(&f.sid), "the session id reached the command");
    for verb in ["send", "join"] {
        assert_eq!(read(&format!("{verb}.code")).trim(), "3", "{verb}: {}", read(&format!("{verb}.err")));
        assert!(read(&format!("{verb}.err")).contains(REFUSAL));
    }
    assert_eq!(read("list.code").trim(), "0", "peer list needs no session: {}", read("list.err"));
    let open = f.signals().join(attend_groups::BROADCAST_DIR);
    assert!(std::fs::read_dir(&open).map(|d| d.count() == 0).unwrap_or(true), "the refused send wrote a signal");
}

fn whoami_via(f: &Fixture, shell: bool) -> std::process::Output {
    match shell {
        true => f.command_from(Path::new("sh")).args(["-c", "\"$0\" peer whoami --machine", env!("CARGO_BIN_EXE_attend")]).output().unwrap(),
        false => f.attend(&["peer", "whoami", "--machine"]),
    }
}

/// The mod's own `attend` call runs straight under Claude Code.
#[test]
fn allowed_the_mods_own_call_resolves_the_session() {
    let f = Fixture::new("guard-mod");
    let out = whoami_via(&f, false);
    assert!(out.status.success() && String::from_utf8_lossy(&out.stdout).contains(&f.sid), "{}", String::from_utf8_lossy(&out.stderr));
}

/// A user's shell inside Claude Code's Bash tool.
#[test]
fn allowed_a_bash_tool_shell_resolves_the_session() {
    let f = Fixture::new("guard-shell");
    let out = whoami_via(&f, true);
    assert!(out.status.success() && String::from_utf8_lossy(&out.stdout).contains(&f.sid), "{}", String::from_utf8_lossy(&out.stderr));
}

/// The Stop-hook drain, run by the hook's shell.
#[test]
fn allowed_the_stop_hook_drain_runs() {
    let f = Fixture::new("guard-drain");
    let out = f.command_from(Path::new("sh")).args(["-c", "\"$0\" message inbox --drain --format hook", env!("CARGO_BIN_EXE_attend")]).output().unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success() && !err.contains(REFUSAL), "{err}");
}

/// `attend run` resolves its own session: its startup line names it.
#[test]
fn allowed_attend_run_resolves_its_own_session() {
    let f = Fixture::new("guard-run");
    let run = f.run();
    run.wait_for("the startup line", Duration::from_secs(20), |r| r.output().contains("you are"));
    assert!(!std::fs::read_to_string(f.home.join("run.err")).unwrap_or_default().contains(REFUSAL));
}

/// Two ways a command might hide the host: a session record forged under a
/// scratch config dir and named by a shell sixteen levels below the host,
/// and a shell that calls itself `claude` (`exec -a claude`). Neither
/// resolves the forged session.
#[test]
fn refused_a_forged_record_deep_or_behind_a_fake_claude() {
    let f = Fixture::new("guard-forged");
    let out = f.home.join("forged");
    let cfg = f.home.join("scratch-config");
    std::fs::create_dir_all(cfg.join("sessions")).unwrap();
    std::fs::create_dir_all(&out).unwrap();
    let bin = env!("CARGO_BIN_EXE_attend");
    // Write a record naming this shell as session sess-forged, then ask who
    // the session is, saving the answer under the probe's name.
    let inner = format!(
        "printf '{{\"pid\":%s,\"sessionId\":\"sess-forged\",\"cwd\":\"{origin}\"}}' $$ > '{cfg}/sessions/'$$.json\n\
         CLAUDE_CONFIG_DIR='{cfg}' '{bin}' peer whoami --machine > '{out}/'$1.out 2> '{out}/'$1.err; echo $? > '{out}/'$1.code\n",
        origin = f.origin,
        cfg = cfg.display(),
        out = out.display(),
    );
    std::fs::write(f.home.join("inner.sh"), inner).unwrap();
    // Sixteen nested shells; the trailing `:` keeps each from exec'ing the
    // next in place.
    let nest = format!("if [ \"$1\" -gt 0 ]; then sh \"$0\" $(($1 - 1)); :; else sh '{}' depth; :; fi\n", f.home.join("inner.sh").display());
    std::fs::write(f.home.join("nest.sh"), nest).unwrap();
    let script = format!(
        "sh '{nest}' 16; bash -c \"exec -a claude bash '{inner}' fake-claude\"; echo '{{}}'",
        nest = f.home.join("nest.sh").display(),
        inner = f.home.join("inner.sh").display()
    );
    f.sensor("forger", &format!("kind: command\ninterval: 1\nmin_interval: 1\nargv: [sh, -c, {}]\noutput: json\n", serde_json::to_string(&script).unwrap()));
    let run = f.run();
    run.wait_for("both probes", Duration::from_secs(30), |_| out.join("fake-claude.code").exists());
    drop(run);
    let read = |n: &str| std::fs::read_to_string(out.join(n)).unwrap_or_default();
    for probe in ["depth", "fake-claude"] {
        assert!(!read(&format!("{probe}.out")).contains("sess-forged"), "{probe}: the forged session resolved");
        assert_eq!(read(&format!("{probe}.code")).trim(), "3", "{probe}: {}", read(&format!("{probe}.err")));
        assert!(read(&format!("{probe}.err")).contains(REFUSAL), "{probe}: {}", read(&format!("{probe}.err")));
    }
}
