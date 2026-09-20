//! Closing a tab closes what it started. Killing the shell alone is not
//! enough: the pty closing sends SIGHUP to the *foreground* group, so a
//! command still in front dies along with it, but anything put in the
//! background survives — reparented to init, still holding its port.
//! That is the stray these tests are about, so they background the
//! sleeper on purpose. `Pty::kill`, and the drop that stands in for it,
//! have to take the whole tree.

use std::time::{Duration, Instant};

use nus_pty::{Profile, Pty};

fn shell() -> Profile {
    let (program, args) = if cfg!(windows) {
        ("cmd", vec!["/Q".into(), "/K".into()])
    } else {
        ("sh", vec!["-i".into()])
    };
    Profile { name: "test".into(), program: program.into(), args, cwd: None, env: Vec::new() }
}

/// A long sleeper that a dying terminal does not reach: backgrounded,
/// and deaf to SIGHUP. This is the shape of the strays — a dev server
/// that daemonises, anything started with nohup, anything that puts
/// itself in its own session.
fn background_sleep() -> &'static str {
    if cfg!(windows) {
        "start /b timeout /t 120 /nobreak\r\n"
    } else {
        "nohup sleep 120 >/dev/null 2>&1 &\n"
    }
}

/// A pid the system still knows about.
fn alive(pid: u32) -> bool {
    nus_pty::ports::process_tree().contains_key(&pid)
}

/// Wait for the shell to have started something, and say what.
fn wait_for_child(pid: u32, secs: u64) -> Option<u32> {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        let tree = nus_pty::ports::process_tree();
        let kids = nus_pty::ports::descendants_in(&tree, pid);
        if let Some(&k) = kids.first() {
            return Some(k);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    None
}

#[test]
fn killing_a_shell_takes_what_it_started() {
    let mut pty = Pty::spawn(&shell(), 80, 24, || {}).expect("spawn");
    let shell_pid = pty.pid().expect("pid");
    pty.write(background_sleep().as_bytes()).expect("write");

    let Some(child) = wait_for_child(shell_pid, 10) else {
        eprintln!("the shell never started anything; skipping");
        return;
    };
    assert!(alive(child), "the sleeper should be running");

    pty.kill();
    // The signals have gone out; the table takes a moment to catch up.
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline && (alive(child) || alive(shell_pid)) {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(!alive(child), "the sleeper outlived the shell: pid {child}");
    assert!(!alive(shell_pid), "the shell outlived its kill: pid {shell_pid}");
}

#[test]
fn dropping_a_shell_reaps_it_too() {
    let child = {
        let mut pty = Pty::spawn(&shell(), 80, 24, || {}).expect("spawn");
        let shell_pid = pty.pid().expect("pid");
        pty.write(background_sleep().as_bytes()).expect("write");
        match wait_for_child(shell_pid, 10) {
            Some(c) => c,
            None => {
                eprintln!("the shell never started anything; skipping");
                return;
            }
        }
        // `pty` goes out of scope here.
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline && alive(child) {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(!alive(child), "the sleeper outlived the drop: pid {child}");
}
