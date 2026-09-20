//! Held shells: a pty that outlives the app.
//!
//! `nus-hold` (crates/hold) is one process per held shell. It spawns the
//! shell in a pty it owns, keeps a ring of the last 4 MB of output, and
//! serves the pty over a loopback socket — one client at a time. The app
//! is that client. Detach is closing the socket; attach is opening it and
//! taking the ring, which the VT core replays before going live. The holder
//! outlives the app on purpose: on Windows `ClosePseudoConsole` terminates
//! the client, so whoever owns the pseudoconsole decides whether `claude`
//! survives a restart. The holder exits when its child does, or on `kill`.
//!
//! Frames on the socket: `[tag u8][len u32 LE][payload]`.
//!   client → holder   'w' bytes to the pty · 'r' cols,rows,px_w,px_h (u16 ×4)
//!                     · 'k' kill the child · 'p' ping
//!   holder → client   'i' info JSON, on connect · 'h' the ring, on connect
//!                     · 'o' output · 'x' exit code (u32)
//!
//! `<dir>/<id>.json` names the holder: port, token, pid, program, cwd,
//! started. A stale file (no process behind it) is just removed.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

use anyhow::{anyhow, Context, Result};

pub const RING: usize = 4 * 1024 * 1024;

/// What `<dir>/<id>.json` says about a holder.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Info {
    pub id: String,
    pub port: u16,
    pub token: String,
    /// The holder's own pid, and the shell's.
    pub holder: u32,
    pub pid: u32,
    pub program: String,
    pub cwd: Option<String>,
    pub started: u64,
}

impl Info {
    pub fn path(dir: &Path, id: &str) -> PathBuf {
        dir.join(format!("{id}.json"))
    }

    pub fn write(&self, dir: &Path) -> Result<()> {
        std::fs::create_dir_all(dir)?;
        std::fs::write(
            Self::path(dir, &self.id),
            serde_json::to_string_pretty(self)?,
        )?;
        Ok(())
    }

    pub fn read(dir: &Path, id: &str) -> Option<Info> {
        let text = std::fs::read_to_string(Self::path(dir, id)).ok()?;
        serde_json::from_str(&text).ok()
    }

    /// Every holder the directory names, alive or not (see [`alive`]).
    pub fn all(dir: &Path) -> Vec<Info> {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut v: Vec<Info> = rd
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
            .filter_map(|e| std::fs::read_to_string(e.path()).ok())
            .filter_map(|t| serde_json::from_str(&t).ok())
            .collect();
        v.sort_by_key(|i| i.started);
        v
    }

    /// Is the holder still there? A ping answers; a stale file is removed.
    pub fn alive(&self, dir: &Path) -> bool {
        match TcpStream::connect(("127.0.0.1", self.port)) {
            Ok(mut s) => {
                let _ = s.set_read_timeout(Some(std::time::Duration::from_millis(400)));
                // The greeting is 'i'; a holder that answers is alive. We do
                // not attach, so it sends nothing more once we hang up.
                let mut tag = [0u8; 1];
                let ok = s.read_exact(&mut tag).is_ok() && tag[0] == b'i';
                if !ok {
                    let _ = std::fs::remove_file(Self::path(dir, &self.id));
                }
                ok
            }
            Err(_) => {
                let _ = std::fs::remove_file(Self::path(dir, &self.id));
                false
            }
        }
    }
}

/// Write one frame.
pub fn send(w: &mut impl Write, tag: u8, payload: &[u8]) -> std::io::Result<()> {
    w.write_all(&[tag])?;
    w.write_all(&(payload.len() as u32).to_le_bytes())?;
    w.write_all(payload)?;
    w.flush()
}

/// Read one frame; `None` at a clean EOF.
pub fn recv(r: &mut impl Read) -> std::io::Result<Option<(u8, Vec<u8>)>> {
    let mut head = [0u8; 5];
    match r.read_exact(&mut head) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_le_bytes([head[1], head[2], head[3], head[4]]) as usize;
    if len > 64 * 1024 * 1024 {
        return Err(std::io::Error::other("frame too large"));
    }
    let mut payload = vec![0u8; len];
    r.read_exact(&mut payload)?;
    Ok(Some((head[0], payload)))
}

/// Complete frames at the front of `buf`, leaving any partial one in place —
/// for a non-blocking reader that gets bytes in whatever pieces arrive.
pub fn parse_frames(buf: &mut Vec<u8>) -> Vec<(u8, Vec<u8>)> {
    let mut out = Vec::new();
    let mut at = 0;
    while buf.len() >= at + 5 {
        let len = u32::from_le_bytes([buf[at + 1], buf[at + 2], buf[at + 3], buf[at + 4]]) as usize;
        if buf.len() < at + 5 + len {
            break;
        }
        out.push((buf[at], buf[at + 5..at + 5 + len].to_vec()));
        at += 5 + len;
    }
    buf.drain(..at);
    out
}

/// The last `cap` bytes of everything pushed: what a client gets on attach.
pub struct Ring {
    buf: Vec<u8>,
    cap: usize,
}

impl Ring {
    pub fn new(cap: usize) -> Ring {
        Ring {
            buf: Vec::with_capacity(cap.min(1 << 16)),
            cap,
        }
    }
    pub fn push(&mut self, bytes: &[u8]) {
        if bytes.len() >= self.cap {
            self.buf.clear();
            self.buf.extend_from_slice(&bytes[bytes.len() - self.cap..]);
        } else {
            let cut = (self.buf.len() + bytes.len()).saturating_sub(self.cap);
            self.buf.drain(..cut);
            self.buf.extend_from_slice(bytes);
        }
    }
    pub fn bytes(&self) -> &[u8] {
        &self.buf
    }
}

/// The app's end of a holder: the pty's bytes both ways over the socket.
pub struct Client {
    stream: TcpStream,
    pub info: Info,
    pub output: mpsc::Receiver<Vec<u8>>,
    exited: Arc<Mutex<Option<u32>>>,
}

impl Client {
    /// Connect and attach: the greeting, then the ring, then live output —
    /// all on the same channel, in order.
    pub fn attach(info: Info, on_output: impl Fn() + Send + 'static) -> Result<Client> {
        let mut stream =
            TcpStream::connect(("127.0.0.1", info.port)).context("connect to the holder")?;
        stream.set_nodelay(true).ok();
        // The token proves we read the file; the holder checks it before serving.
        send(&mut stream, b't', info.token.as_bytes())?;
        let Some((b'i', greeting)) = recv(&mut stream)? else {
            return Err(anyhow!("no greeting from the holder"));
        };
        let info: Info = serde_json::from_slice(&greeting).context("holder greeting")?;
        let exited = Arc::new(Mutex::new(None));
        let (tx, rx) = mpsc::sync_channel(32);
        let mut reader = stream.try_clone().context("clone the socket")?;
        let flag = exited.clone();
        thread::Builder::new()
            .name("hold-reader".into())
            .spawn(move || {
                loop {
                    match recv(&mut reader) {
                        Ok(Some((b'h', bytes))) | Ok(Some((b'o', bytes))) => {
                            for chunk in bytes.chunks(64 * 1024) {
                                if tx.send(chunk.to_vec()).is_err() {
                                    return;
                                }
                                on_output();
                            }
                        }
                        Ok(Some((b'x', code))) => {
                            let code = code
                                .get(..4)
                                .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                                .unwrap_or(0);
                            *flag.lock().unwrap() = Some(code);
                            on_output();
                            break;
                        }
                        Ok(Some(_)) => {}
                        Ok(None) | Err(_) => {
                            // The holder went away without an exit: treat as gone.
                            let mut f = flag.lock().unwrap();
                            if f.is_none() {
                                *f = Some(u32::MAX);
                            }
                            on_output();
                            break;
                        }
                    }
                }
            })
            .context("spawn hold reader")?;
        Ok(Client {
            stream,
            info,
            output: rx,
            exited,
        })
    }

    pub fn write(&mut self, bytes: &[u8]) -> Result<()> {
        send(&mut self.stream, b'w', bytes).context("hold write")
    }

    pub fn resize(&mut self, cols: u16, rows: u16, px: (u16, u16)) -> Result<()> {
        let mut p = Vec::with_capacity(8);
        for v in [cols, rows, px.0, px.1] {
            p.extend_from_slice(&v.to_le_bytes());
        }
        send(&mut self.stream, b'r', &p).context("hold resize")
    }

    pub fn kill(&mut self) {
        let _ = send(&mut self.stream, b'k', &[]);
    }

    pub fn exit_code(&self) -> Option<u32> {
        *self.exited.lock().unwrap()
    }
}

/// Where the holder binary is: `NUS_HOLD`, beside this exe, or the
/// workspace's target dir while developing.
pub fn holder_exe() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("NUS_HOLD") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    let name = if cfg!(windows) {
        "nus-hold.exe"
    } else {
        "nus-hold"
    };
    let mut cands = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            cands.push(dir.join(name));
            // target/debug/deps/<test> -> target/debug/
            if let Some(up) = dir.parent() {
                cands.push(up.join(name));
            }
            // spikes/<x>/target/<profile>/ → <repo>/target/<profile>/
            for up in [
                dir.join("../../../../target/debug"),
                dir.join("../../../../target/release"),
            ] {
                cands.push(up.join(name));
            }
        }
    }
    cands
        .into_iter()
        .find(|p| p.is_file())
        .map(|p| p.canonicalize().unwrap_or(p))
}

/// Start a holder for `profile` and attach to it.
pub fn spawn_held(
    profile: &crate::Profile,
    cols: u16,
    rows: u16,
    dir: &Path,
    on_output: impl Fn() + Send + 'static,
) -> Result<Client> {
    let exe = holder_exe().ok_or_else(|| anyhow!("no nus-hold binary"))?;
    std::fs::create_dir_all(dir)?;
    let id = format!(
        "{:x}-{:x}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    );
    let mut cmd = std::process::Command::new(&exe);
    cmd.arg("--id")
        .arg(&id)
        .arg("--dir")
        .arg(dir)
        .arg("--cols")
        .arg(cols.to_string())
        .arg("--rows")
        .arg(rows.to_string())
        .arg("--program")
        .arg(&profile.program);
    if let Some(cwd) = &profile.cwd {
        cmd.arg("--cwd").arg(cwd);
    }
    for (k, v) in &profile.env {
        cmd.arg("--env").arg(format!("{k}={v}"));
    }
    cmd.arg("--").args(&profile.args);
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Its own process group and no console: the app's death takes nothing with it.
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
    }
    let _child = cmd
        .spawn()
        .with_context(|| format!("start {}", exe.display()))?;
    // The holder writes its file once it listens; give it a moment.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let info = loop {
        if let Some(i) = Info::read(dir, &id) {
            break i;
        }
        if std::time::Instant::now() > deadline {
            return Err(anyhow!("the holder did not start"));
        }
        thread::sleep(std::time::Duration::from_millis(20));
    };
    Client::attach(info, on_output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_roundtrip() {
        let mut buf = Vec::new();
        send(&mut buf, b'o', b"hello").unwrap();
        send(&mut buf, b'x', &7u32.to_le_bytes()).unwrap();
        let mut r = std::io::Cursor::new(buf);
        assert_eq!(recv(&mut r).unwrap(), Some((b'o', b"hello".to_vec())));
        assert_eq!(
            recv(&mut r).unwrap(),
            Some((b'x', 7u32.to_le_bytes().to_vec()))
        );
        assert_eq!(recv(&mut r).unwrap(), None);
    }

    #[test]
    fn frames_in_pieces() {
        let mut whole = Vec::new();
        send(&mut whole, b'w', b"abc").unwrap();
        send(&mut whole, b'k', b"").unwrap();
        let mut buf = Vec::new();
        buf.extend_from_slice(&whole[..3]);
        assert!(parse_frames(&mut buf).is_empty());
        buf.extend_from_slice(&whole[3..9]);
        assert_eq!(parse_frames(&mut buf), vec![(b'w', b"abc".to_vec())]);
        buf.extend_from_slice(&whole[9..]);
        assert_eq!(parse_frames(&mut buf), vec![(b'k', Vec::new())]);
        assert!(buf.is_empty());
    }

    #[test]
    fn ring_keeps_the_tail() {
        let mut ring = Ring::new(8);
        ring.push(b"abcdef");
        ring.push(b"ghij");
        assert_eq!(ring.bytes(), b"cdefghij");
        ring.push(&vec![b'x'; 1024 * 1024]);
        assert_eq!(ring.bytes(), b"xxxxxxxx");
        assert!(
            ring.buf.capacity() <= 16,
            "oversized writes must not inflate the retained allocation"
        );
        let mut empty = Ring::new(0);
        empty.push(b"ignored");
        assert!(empty.bytes().is_empty());
    }
}
