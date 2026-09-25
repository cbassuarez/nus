//! Closing a tab over a held shell: the app says kill and drops its end in
//! the same breath. Everything the shell started must go with it — the
//! foreground job and one left in the background. Needs a built `nus-hold`
//! and a credential store; skips quietly otherwise.
#![cfg(unix)]

use std::time::{Duration, Instant};

use nus_pty::hold::holder_exe;
use nus_pty::{Profile, Pty};

fn alive(pid: u32) -> bool {
    nus_pty::ports::process_identity(pid).is_some()
}

#[cfg(unix)]
#[test]
fn kill_then_drop_takes_the_whole_tree() {
    if holder_exe().is_none() {
        eprintln!("no nus-hold binary; skipping");
        return;
    }
    let dir = std::env::temp_dir().join(format!("nus-hold-close-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    if nus_vault::available(&dir).is_err() {
        eprintln!("no credential store; skipping");
        return;
    }
    let profile = Profile {
        name: "test".into(),
        program: "sh".into(),
        args: vec!["-c".into(), "sleep 4242 & sleep 4243; cat".into()],
        cwd: None,
        env: Vec::new(),
    };
    let mut pty = Pty::spawn_held(&profile, 80, 24, &dir, || {}).expect("spawn held");
    let shell = pty.pid().expect("shell pid");
    // Wait for both sleeps to be under the shell.
    let deadline = Instant::now() + Duration::from_secs(10);
    let kids = loop {
        let kids = nus_pty::ports::descendants(shell);
        if kids.len() >= 2 || Instant::now() > deadline {
            break kids;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(kids.len() >= 2, "sleeps never started: {kids:?}");
    // What closing a tab does: the word, then the pane is dropped.
    pty.kill_reaped();
    drop(pty);
    let deadline = Instant::now() + Duration::from_secs(5);
    while (alive(shell) || kids.iter().any(|&k| alive(k))) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    let left: Vec<u32> = kids
        .iter()
        .copied()
        .chain([shell])
        .filter(|&p| alive(p))
        .collect();
    for &p in &left {
        nus_pty::ports::kill_one(p, true);
    }
    nus_vault::remove_test_key(&dir).ok();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(left.is_empty(), "left running after close: {left:?}");
}
