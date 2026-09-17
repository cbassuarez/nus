//! Optional tools, fetched on request: language servers, tree-sitter
//! grammars, assistant CLIs. The list is assets/bundles.json (a
//! profile/bundles.json beside it adds or overrides entries); each entry
//! names a download per platform and where it unpacks under profile/,
//! or a command that installs itself. Nothing arrives unless the user
//! presses GET on the welcome page's OPTIONAL TOOLS — the first boot's
//! question — and a bundle is a folder you can delete.

use crate::app::App;
use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver};

#[derive(Clone, Debug, serde::Deserialize)]
pub struct Platform {
    pub url: String,
    #[serde(default)]
    pub unpack: String,
    /// For a single-file download: the name to give it.
    #[serde(default)]
    pub file: String,
}

#[derive(Clone, Debug, serde::Deserialize)]
pub struct Bundle {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub about: String,
    #[serde(default)]
    pub size_mb: u32,
    /// Where it lands, under profile/ ("bin", "grammars/rust", "" for a command).
    #[serde(default)]
    pub into: String,
    #[serde(default)]
    pub platforms: HashMap<String, Platform>,
    /// A command that installs the tool itself (no download here).
    #[serde(default)]
    pub command: Vec<String>,
    /// Not published yet.
    #[serde(default)]
    pub soon: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum State {
    Absent,
    Soon,
    NoPlatform,
    Fetching,
    Installed,
    Failed(String),
}

pub struct Jobs {
    pub rx: Receiver<(String, Result<(), String>)>,
    pub tx: std::sync::mpsc::Sender<(String, Result<(), String>)>,
    pub running: Vec<String>,
    pub failed: HashMap<String, String>,
}

impl Jobs {
    pub fn new() -> Jobs {
        let (tx, rx) = channel();
        Jobs { rx, tx, running: Vec::new(), failed: HashMap::new() }
    }
}

impl Default for Jobs {
    fn default() -> Self {
        Self::new()
    }
}

pub fn platform() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => "windows-x64",
        ("windows", "aarch64") => "windows-arm64",
        ("macos", "aarch64") => "macos-arm64",
        ("macos", "x86_64") => "macos-x64",
        ("linux", "x86_64") => "linux-x64",
        ("linux", "aarch64") => "linux-arm64",
        _ => "other",
    }
}

fn profile_dir() -> std::path::PathBuf {
    std::env::current_dir().unwrap_or_default().join("profile")
}

/// The bundle list: the built-in one, then profile/bundles.json's entries
/// (by id) on top.
pub fn list() -> Vec<Bundle> {
    let mut v: Vec<Bundle> = serde_json::from_str(include_str!("../assets/bundles.json")).unwrap_or_default();
    if let Ok(text) = std::fs::read_to_string(profile_dir().join("bundles.json")) {
        if let Ok(mine) = serde_json::from_str::<Vec<Bundle>>(&text) {
            for b in mine {
                v.retain(|x| x.id != b.id);
                v.push(b);
            }
        }
    }
    v
}

/// Where a bundle's files live; None for a self-installing command.
pub fn dir_of(b: &Bundle) -> Option<std::path::PathBuf> {
    if b.into.is_empty() {
        None
    } else {
        Some(profile_dir().join(&b.into))
    }
}

fn no_window(c: &mut std::process::Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
}

fn run(mut c: std::process::Command) -> Result<(), String> {
    no_window(&mut c);
    let out = c.output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(crate::surface::first_line(String::from_utf8_lossy(&out.stderr).trim()))
    }
}

/// Fetch and unpack on a worker; the result comes back on the channel.
fn fetch(b: Bundle, tx: std::sync::mpsc::Sender<(String, Result<(), String>)>) {
    let id = b.id.clone();
    std::thread::spawn(move || {
        let r = (|| -> Result<(), String> {
            if !b.command.is_empty() {
                let mut c = std::process::Command::new(&b.command[0]);
                c.args(&b.command[1..]);
                return run(c);
            }
            let p = b.platforms.get(platform()).ok_or("not for this platform")?;
            let dir = dir_of(&b).ok_or("nowhere to put it")?;
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let tmp = profile_dir().join(format!("{}.download", b.id));
            let mut curl = std::process::Command::new("curl");
            curl.args(["-sSL", "--fail", "-o"]).arg(&tmp).arg(&p.url);
            run(curl)?;
            match p.unpack.as_str() {
                "zip" => {
                    let c = if cfg!(windows) {
                        let mut c = std::process::Command::new("powershell");
                        c.args(["-NoProfile", "-Command", &format!("Expand-Archive -Force -LiteralPath '{}' -DestinationPath '{}'", tmp.display(), dir.display())]);
                        c
                    } else {
                        let mut c = std::process::Command::new("unzip");
                        c.arg("-o").arg(&tmp).arg("-d").arg(&dir);
                        c
                    };
                    let r = run(c);
                    let _ = std::fs::remove_file(&tmp);
                    r?;
                }
                "gz" => {
                    let name = if p.file.is_empty() { b.id.clone() } else { p.file.clone() };
                    let out = dir.join(&name);
                    let mut c = std::process::Command::new("sh");
                    c.arg("-c").arg(format!("gzip -dc '{}' > '{}' && chmod +x '{}'", tmp.display(), out.display(), out.display()));
                    let r = run(c);
                    let _ = std::fs::remove_file(&tmp);
                    r?;
                }
                _ => {
                    let name = if p.file.is_empty() { b.id.clone() } else { p.file.clone() };
                    std::fs::rename(&tmp, dir.join(name)).map_err(|e| e.to_string())?;
                }
            }
            Ok(())
        })();
        let _ = tx.send((id, r));
    });
}

impl App {
    /// A bundle's state: on disk, fetching, failed, not for here, soon.
    pub(crate) fn bundle_state(&self, b: &Bundle) -> State {
        if self.jobs.running.contains(&b.id) {
            return State::Fetching;
        }
        if let Some(e) = self.jobs.failed.get(&b.id) {
            return State::Failed(e.clone());
        }
        if b.soon {
            return State::Soon;
        }
        if !b.command.is_empty() {
            // A self-installing tool: ask its own presence check.
            if b.id == "copilot-cli" {
                let base = std::env::var("LOCALAPPDATA").map(std::path::PathBuf::from).unwrap_or_default();
                return if base.join("GitHub CLI").join("copilot").exists() { State::Installed } else { State::Absent };
            }
            return State::Absent;
        }
        if !b.platforms.contains_key(platform()) {
            return State::NoPlatform;
        }
        match dir_of(b) {
            Some(d) if d.exists() && std::fs::read_dir(&d).map(|mut r| r.next().is_some()).unwrap_or(false) => State::Installed,
            _ => State::Absent,
        }
    }

    /// GET: fetch a bundle; REMOVE: delete its folder.
    pub(crate) fn bundle_toggle(&mut self, id: &str) {
        let Some(b) = list().into_iter().find(|b| b.id == id) else { return };
        match self.bundle_state(&b) {
            State::Installed => {
                if let Some(d) = dir_of(&b) {
                    let _ = std::fs::remove_dir_all(d);
                }
            }
            State::Absent | State::Failed(_) => {
                self.jobs.failed.remove(id);
                self.jobs.running.push(id.to_string());
                fetch(b, self.jobs.tx.clone());
            }
            _ => {}
        }
        self.play_event("control.press");
        self.dirty = true;
    }

    /// Once a loop: finished fetches.
    pub(crate) fn tend_bundles(&mut self) {
        while let Ok((id, r)) = self.jobs.rx.try_recv() {
            self.jobs.running.retain(|x| x != &id);
            match r {
                Ok(()) => {
                    self.play_event("page.ready");
                }
                Err(e) => {
                    self.jobs.failed.insert(id, e);
                }
            }
            self.dirty = true;
        }
    }
}
