//! `nus-hold`: a pty that outlives the app.
//!
//! One process per held shell. It spawns the shell in a pty it owns, keeps
//! the last 4 MB of output in a ring, and serves the pty over a loopback
//! socket to one client at a time (see `nus_pty::hold` for the frames).
//! It exits when the shell does, or when a client says `kill`. Nothing
//! else — no config, no UI; the app is the only thing that talks to it.
//!
//!   nus-hold --id <id> --dir <dir> --cols 80 --rows 24 --program pwsh
//!            [--cwd <dir>] [--env K=V]... [-- args...]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use nus_pty::hold::{parse_frames, recv, send, Info, Ring, RING};
use nus_pty::{Profile, Pty};

struct Args {
    vault_key_stdin: bool,
    id: String,
    dir: PathBuf,
    cols: u16,
    rows: u16,
    profile: Profile,
}

fn parse() -> Result<Args> {
    let mut vault_key_stdin = false;
    let mut it = std::env::args().skip(1);
    let (mut id, mut dir, mut cols, mut rows) = (None, None, 80u16, 24u16);
    let mut profile = Profile {
        name: "held".into(),
        program: String::new(),
        args: Vec::new(),
        cwd: None,
        env: Vec::new(),
    };
    while let Some(a) = it.next() {
        match a.as_str() {
            "--vault-key-stdin" => vault_key_stdin = true,
            "--id" => id = it.next(),
            "--dir" => dir = it.next().map(PathBuf::from),
            "--cols" => cols = it.next().and_then(|v| v.parse().ok()).unwrap_or(80),
            "--rows" => rows = it.next().and_then(|v| v.parse().ok()).unwrap_or(24),
            "--program" => profile.program = it.next().unwrap_or_default(),
            "--cwd" => profile.cwd = it.next(),
            "--env" => {
                if let Some((k, v)) = it.next().and_then(|kv| {
                    kv.split_once('=')
                        .map(|(k, v)| (k.to_string(), v.to_string()))
                }) {
                    profile.env.push((k, v));
                }
            }
            "--" => {
                profile.args.extend(it.by_ref());
                break;
            }
            other => return Err(anyhow!("unknown argument {other}")),
        }
    }
    if profile.program.is_empty() {
        return Err(anyhow!("--program is required"));
    }
    Ok(Args {
        vault_key_stdin,
        id: id.ok_or_else(|| anyhow!("--id is required"))?,
        dir: dir.ok_or_else(|| anyhow!("--dir is required"))?,
        cols,
        rows,
        profile,
    })
}

/// The bytes without `ESC [ 6 n`.
fn strip_dsr(out: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(out.len());
    let mut i = 0;
    while i < out.len() {
        if out[i..].starts_with(b"\x1b[6n") {
            i += 4;
        } else {
            v.push(out[i]);
            i += 1;
        }
    }
    v
}

fn token() -> Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| anyhow!("secure random source unavailable"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn main() {
    if std::env::args().nth(1).as_deref() == Some("--version") {
        println!(
            "nus-hold {} protocol {}",
            env!("CARGO_PKG_VERSION"),
            nus_compat::HOLD_PROTOCOL
        );
        return;
    }
    if let Err(e) = run() {
        eprintln!("nus-hold: {e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args = parse()?;
    if args.vault_key_stdin {
        nus_vault::receive_child_key(
            &nus_vault::profile_for(&args.dir.join("state.json")),
            std::io::stdin().lock(),
        )?;
    }
    nus_vault::available(&nus_vault::profile_for(&args.dir.join("state.json")))?;
    let listener = TcpListener::bind(("127.0.0.1", 0)).context("listen")?;
    let port = listener.local_addr()?.port();
    listener.set_nonblocking(true)?;

    // Output lands in the ring and, when someone is attached, on the wire.
    let ring = Arc::new(Mutex::new(Ring::new(RING)));
    let wake = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
    let w2 = wake.clone();
    let mut pty = Pty::spawn(&args.profile, args.cols, args.rows, move || {
        let (m, cv) = &*w2;
        *m.lock().unwrap() = true;
        cv.notify_one();
    })
    .context("spawn the shell")?;
    let pid = pty.pid().unwrap_or(0);
    let info = Info {
        protocol: nus_compat::HOLD_PROTOCOL,
        id: args.id.clone(),
        port,
        token: token()?,
        holder: std::process::id(),
        pid,
        program: args.profile.program.clone(),
        cwd: args.profile.cwd.clone(),
        started: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    };
    info.write(&args.dir)?;
    let file = Info::path(&args.dir, &args.id);

    let mut client: Option<TcpStream> = None;
    let mut inbuf: Vec<u8> = Vec::new();
    let mut scratch = vec![0u8; 64 * 1024];
    let mut exit: Option<u32> = None;
    let mut idle_since: Option<Instant> = None;
    loop {
        // Drain the pty: into the ring, and to the client if any.
        let out = pty.take_output();
        if !out.is_empty() {
            // ConPTY's conhost asks where the cursor is (DSR 6) and waits for
            // the answer before it draws anything. With a client attached its
            // VT core answers; alone, we do — and the ask stays out of the
            // ring, so a later attach does not answer it twice.
            let asked = out.windows(4).filter(|w| w == b"\x1b[6n").count();
            let kept = strip_dsr(&out);
            ring.lock().unwrap().push(&kept);
            match client.as_mut() {
                Some(c) => {
                    if send(c, b'o', &out).is_err() {
                        client = None;
                    }
                }
                None => {
                    for _ in 0..asked {
                        let _ = pty.write(b"\x1b[1;1R");
                    }
                }
            }
        }
        if exit.is_none() {
            exit = pty.exit_code();
            if let Some(code) = exit {
                if let Some(c) = client.as_mut() {
                    let _ = send(c, b'x', &code.to_le_bytes());
                }
                idle_since = Some(Instant::now());
            }
        }
        // The shell is gone: a moment for the last bytes to reach a client, then leave.
        if exit.is_some() && idle_since.is_some_and(|t| t.elapsed() > Duration::from_millis(300)) {
            break;
        }
        // A new client: check its token, greet it, hand it the ring.
        if let Ok((mut s, _)) = listener.accept() {
            s.set_nodelay(true).ok();
            s.set_read_timeout(Some(Duration::from_millis(500))).ok();
            s.set_write_timeout(Some(Duration::from_millis(500))).ok();
            let request = recv(&mut s);
            let ok = match &request {
                Ok(Some((b't', token))) => token == info.token.as_bytes(), // documented legacy v1
                Ok(Some((b'a', bytes))) => valid_hello(bytes, &info.token),
                _ => false,
            };
            if ok {
                let greeting = serde_json::to_vec(&info).unwrap_or_default();
                if send(&mut s, b'i', &greeting).is_ok() {
                    let bytes = ring.lock().unwrap().bytes().to_vec();
                    if send(&mut s, b'h', &bytes).is_ok() {
                        s.set_nonblocking(true).ok();
                        // One at a time: the newcomer replaces whoever was attached.
                        client = Some(s);
                        inbuf.clear();
                    }
                }
            } else if matches!(request, Ok(Some((b'p', _)))) {
                // Probes never replace the attached client or reveal credentials.
                let _ = s.write_all(b"i");
            } else {
                let _ = send(&mut s, b'e', b"HOLD_HANDSHAKE_REJECTED");
            }
        }
        // The client's frames: bytes to the shell, a resize, a kill. Read
        // whatever has arrived, then act on the frames that are complete.
        if let Some(c) = client.as_mut() {
            let mut gone = false;
            loop {
                match c.read(&mut scratch) {
                    Ok(0) => {
                        gone = true;
                        break;
                    }
                    Ok(n) => inbuf.extend_from_slice(&scratch[..n]),
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(_) => {
                        gone = true;
                        break;
                    }
                }
            }
            for (tag, p) in parse_frames(&mut inbuf) {
                match tag {
                    b'w' => {
                        let _ = pty.write(&p);
                    }
                    b'r' if p.len() >= 8 => {
                        let v = |i: usize| u16::from_le_bytes([p[i], p[i + 1]]);
                        let _ = pty.resize(v(0), v(2), (v(4), v(6)));
                    }
                    b'k' => pty.kill(),
                    _ => {}
                }
            }
            if gone {
                client = None;
                inbuf.clear();
            }
        }
        // Sleep until the pty has something or a little time passes.
        let (m, cv) = &*wake;
        let mut flag = m.lock().unwrap();
        if !*flag {
            let (f, _) = cv.wait_timeout(flag, Duration::from_millis(8)).unwrap();
            flag = f;
        }
        *flag = false;
    }
    let _ = std::fs::remove_file(file);
    Ok(())
}

fn valid_hello(bytes: &[u8], token: &str) -> bool {
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(bytes) else {
        return false;
    };
    v["token"].as_str() == Some(token)
        && v["protocol"].as_u64() == Some(nus_compat::HOLD_PROTOCOL as u64)
}

#[cfg(test)]
mod compatibility_tests {
    #[test]
    fn handshake_rejects_other_protocols_and_missing_fields() {
        assert!(super::valid_hello(br#"{"token":"t","protocol":1}"#, "t"));
        for bad in [
            br#"{"token":"t","protocol":2}"#.as_slice(),
            br#"{"token":"t"}"#,
            br#"{"token":"wrong","protocol":1}"#,
        ] {
            assert!(!super::valid_hello(bad, "t"));
        }
    }
}
