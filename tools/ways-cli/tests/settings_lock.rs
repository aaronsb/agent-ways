//! The settings writer's lock keeps writers apart (ADR-503 §6), on every
//! platform CI runs. It lives here, in the `ways` package, because the
//! Windows CI job runs this package's tests: Windows is where the lock file's
//! removal on release once let a second writer in (#713, W1).

use agent_settings::writer::{edit_file, lock_path};
use serde_yaml::Value;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ways-settings-lock-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Add one to `count` in the file, holding the lock for `hold` while doing so.
fn increment(path: &Path, hold: Duration) {
    edit_file(path, None, |d| {
        let n = d.get(&["count".to_string()]).and_then(Value::as_i64).unwrap_or(0);
        std::thread::sleep(hold);
        d.set(&["count".to_string()], &Value::Number((n + 1).into()))
    })
    .unwrap();
}

fn count(path: &Path) -> i64 {
    let v: Value = serde_yaml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    v.get("count").and_then(Value::as_i64).unwrap()
}

#[test]
fn a_waiter_and_a_later_opener_never_lose_an_update() {
    // A holds the lock; B opens the lock file and waits on it; A releases
    // (and removes the lock file if it can); C opens only after A is done.
    // If A's removal could succeed under B, B and C would hold the lock at
    // once, read the same count and one increment would be lost.
    let d = dir("abc");
    let path = d.join("config.yaml");
    std::fs::write(&path, "# shared\ncount: 0\n").unwrap();
    let (a_holding, wait_a_holding) = mpsc::channel();
    let (a_done, wait_a_done) = mpsc::channel();
    let pa = path.clone();
    let a = std::thread::spawn(move || {
        edit_file(&pa, None, |doc| {
            a_holding.send(()).unwrap();
            let n = doc.get(&["count".to_string()]).and_then(Value::as_i64).unwrap_or(0);
            std::thread::sleep(Duration::from_millis(300));
            doc.set(&["count".to_string()], &Value::Number((n + 1).into()))
        })
        .unwrap();
        a_done.send(()).unwrap();
    });
    wait_a_holding.recv().unwrap();
    let pb = path.clone();
    let b = std::thread::spawn(move || increment(&pb, Duration::from_millis(300)));
    // Give B time to open the lock file and block on it.
    std::thread::sleep(Duration::from_millis(100));
    wait_a_done.recv().unwrap();
    let pc = path.clone();
    let c = std::thread::spawn(move || increment(&pc, Duration::from_millis(50)));
    for t in [a, b, c] {
        t.join().unwrap();
    }
    assert_eq!(count(&path), 3, "an update was lost");
    assert!(std::fs::read_to_string(&path).unwrap().starts_with("# shared\n"));
    std::fs::remove_dir_all(&d).ok();
}

#[test]
fn many_writers_never_lose_an_update() {
    let d = dir("many");
    let path = d.join("config.yaml");
    std::fs::write(&path, "count: 0\n").unwrap();
    let writers: Vec<_> = (0..8)
        .map(|_| {
            let p = path.clone();
            std::thread::spawn(move || {
                for _ in 0..20 {
                    increment(&p, Duration::from_millis(1));
                }
            })
        })
        .collect();
    for w in writers {
        w.join().unwrap();
    }
    assert_eq!(count(&path), 160, "an update was lost");
    assert!(!lock_path(&path).exists(), "the lock file is removed once nobody holds it");
    std::fs::remove_dir_all(&d).ok();
}
