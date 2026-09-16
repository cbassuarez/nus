//! nus windows are OS windows: one process each for now (CEF wants one
//! profile per process, so a second window gets its own cache under
//! profile/win-<pid>). Each registers itself in profile/windows/<pid>.json
//! with its name and loopback port; the sidebar header lists them and
//! fronts one by asking its port to "raise".

use std::io::Write;
use std::net::TcpStream;
use std::path::PathBuf;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Entry {
    pub pid: u32,
    pub name: String,
    pub port: u16,
    pub tabs: usize,
}

fn dir() -> PathBuf {
    std::env::current_dir().unwrap_or_default().join("profile").join("windows")
}

fn file() -> PathBuf {
    dir().join(format!("{}.json", std::process::id()))
}

/// Write (or rewrite) this window's entry.
pub fn register(name: &str, port: u16, tabs: usize) {
    let _ = std::fs::create_dir_all(dir());
    let e = Entry { pid: std::process::id(), name: name.to_string(), port, tabs };
    let _ = std::fs::write(file(), serde_json::to_string(&e).unwrap_or_default());
}

pub fn unregister() {
    let _ = std::fs::remove_file(file());
}

/// Every registered window, this one first. Entries whose port no longer
/// answers are stale (a crash) and are dropped.
pub fn list() -> Vec<Entry> {
    let me = std::process::id();
    let mut v: Vec<Entry> = std::fs::read_dir(dir())
        .map(|rd| {
            rd.flatten()
                .filter_map(|d| std::fs::read_to_string(d.path()).ok())
                .filter_map(|s| serde_json::from_str::<Entry>(&s).ok())
                .collect()
        })
        .unwrap_or_default();
    v.retain(|e| e.pid == me || TcpStream::connect(("127.0.0.1", e.port)).is_ok() || { let _ = std::fs::remove_file(dir().join(format!("{}.json", e.pid))); false });
    v.sort_by_key(|e| (e.pid != me, e.pid));
    v
}

/// Bring another window to the front.
pub fn front(e: &Entry) {
    if let Ok(mut s) = TcpStream::connect(("127.0.0.1", e.port)) {
        let _ = writeln!(s, "raise");
    }
}

/// Launch a new window: a second process with its own CEF cache.
pub fn spawn() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::process::Command::new(exe).arg("--window").current_dir(std::env::current_dir().unwrap_or_default()).spawn();
    }
}

/// Is this process a secondary window?
pub fn is_secondary() -> bool {
    std::env::args().any(|a| a == "--window")
}
