//! The holder end to end: spawn a shell through nus-hold, detach, and come
//! back to find the ring and the live process. Needs a built `nus-hold`
//! (cargo build -p nus-hold); skips quietly otherwise.

use std::time::{Duration, Instant};

use nus_pty::hold::{holder_exe, Info};
use nus_pty::{Profile, Pty};

fn shell() -> Profile {
    if cfg!(windows) {
        Profile {
            name: "test".into(),
            program: "cmd".into(),
            args: vec!["/Q".into(), "/K".into(), "echo held-hello".into()],
            cwd: None,
            env: Vec::new(),
        }
    } else {
        Profile {
            name: "test".into(),
            program: "sh".into(),
            args: vec!["-c".into(), "echo held-hello; cat".into()],
            cwd: None,
            env: Vec::new(),
        }
    }
}

/// Read until `needle` shows up, answering as a terminal would on the way:
/// ConPTY's conhost asks where the cursor is (DSR 6) and draws nothing
/// until it hears back. The app's VT core answers that; here we do.
fn wait_for(pty: &mut Pty, needle: &str, secs: u64) -> String {
    let mut got = String::new();
    let mut answered = 0;
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        got.push_str(&String::from_utf8_lossy(&pty.take_output()));
        if got.contains(needle) {
            break;
        }
        let asks = got.matches("[6n").count();
        while answered < asks {
            pty.write(b"[1;1R").expect("answer DSR");
            answered += 1;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    got
}

#[test]
fn spawn_detach_attach_kill() {
    let Some(exe) = holder_exe() else {
        eprintln!("no nus-hold binary; skipping");
        return;
    };
    eprintln!("holder: {}", exe.display());
    let dir = std::env::temp_dir().join(format!("nus-hold-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    // Headless CI may have no Secret Service/login credential store. It must
    // exercise the fail-closed path rather than silently writing a plaintext token.
    if nus_vault::available(&dir).is_err() {
        assert!(Pty::spawn_held(&shell(), 80, 24, &dir, || {}).is_err());
        assert!(Info::all(&dir).is_empty());
        eprintln!("Credential store unavailable: fail-closed holder path verified; live resume requires a native keychain session");
        let _ = std::fs::remove_dir_all(&dir);
        return;
    }
    // Spawn through the holder; the shell's greeting arrives over the socket.
    let mut pty = Pty::spawn_held(&shell(), 80, 24, &dir, || {}).expect("spawn held");
    let id = pty.held_id().expect("held").to_string();
    let got = wait_for(&mut pty, "held-hello", 15);
    assert!(got.contains("held-hello"), "no greeting: {got:?}");
    let info = Info::read(&dir, &id).expect("info file");
    assert_eq!(info.id, id);
    let ciphertext = std::fs::read(Info::path(&dir, &id)).unwrap();
    assert!(ciphertext.starts_with(b"NUSENC01"));
    assert!(!ciphertext
        .windows(info.token.len())
        .any(|w| w == info.token.as_bytes()));
    assert!(info.pid > 0);
    // A health probe must answer promptly and leave the attached client live.
    let probe_started = Instant::now();
    assert!(info.alive(&dir), "live holder did not answer ping");
    assert!(probe_started.elapsed() < Duration::from_millis(400));
    assert!(Info::path(&dir, &id).exists(), "ping removed a live holder");

    // A future client must not take over the existing attached terminal.
    let mut stranger = std::net::TcpStream::connect(("127.0.0.1", info.port)).unwrap();
    stranger
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let bad = serde_json::json!({"token":info.token,"protocol":999});
    nus_pty::hold::send(&mut stranger, b'a', &serde_json::to_vec(&bad).unwrap()).unwrap();
    assert!(matches!(
        nus_pty::hold::recv(&mut stranger).unwrap(),
        Some((b'e', _))
    ));
    pty.write(b"compatibility-still-attached\n").unwrap();
    assert!(wait_for(&mut pty, "compatibility-still-attached", 5)
        .contains("compatibility-still-attached"));
    let mut incompatible = info.clone();
    incompatible.protocol = 999;
    assert!(Pty::attach(incompatible, 80, 24, || {}).is_err());
    assert!(Info::path(&dir, &id).exists());

    // Detach: the holder and the shell stay.
    pty.detach();
    std::thread::sleep(Duration::from_millis(300));
    assert!(info.alive(&dir), "holder gone after detach");

    // Attach: the ring replays the greeting, then the shell is live.
    let mut again = Pty::attach(info.clone(), 80, 24, || {}).expect("attach");
    let replay = wait_for(&mut again, "held-hello", 5);
    assert!(
        replay.contains("held-hello"),
        "no ring on attach: {replay:?}"
    );
    again.write(b"echo held-again\r\n").unwrap();
    let live = wait_for(&mut again, "held-again", 10);
    assert!(
        live.contains("held-again"),
        "not live after attach: {live:?}"
    );

    // Kill: the child goes, the holder reports the exit and leaves.
    again.kill();
    let deadline = Instant::now() + Duration::from_secs(10);
    while again.exit_code().is_none() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(again.exit_code().is_some(), "no exit after kill");
    std::thread::sleep(Duration::from_millis(600));
    assert!(!Info::path(&dir, &id).exists(), "info file left behind");
    nus_vault::remove_test_key(&dir).expect("remove fixture credential");
    let _ = std::fs::remove_dir_all(&dir);
}
