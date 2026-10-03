//! What `nus sync …` asks of the app beyond now/key/join/status/folder/git:
//! the key on paper, the devices, a new key, the conflicts sync kept, older
//! versions from the git carrier, and a forge sign-in started from a terminal.
//! The app answers because the key and the forge token live in its vault.
//!
//! Anything that touches a carrier (a git pull, re-sealing) runs as a job on
//! a worker; the CLI polls `sync job` for the result, one job at a time.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};

use serde_json::{json, Value};

use crate::app::App;

pub struct Job {
    pub name: &'static str,
    pub rx: Receiver<Result<Value, String>>,
    /// The app applies a finished job itself once it is this old and no
    /// terminal has come for the answer.
    pub started: std::time::Instant,
}

fn profile_dir() -> PathBuf {
    std::env::current_dir().unwrap_or_default().join("profile")
}

/// The carriers, as the worker builds them: each with the root its sealed
/// files sit under (rotation removes device folders there).
struct Spec {
    folder: Option<PathBuf>,
    git: Option<(String, Option<String>)>,
}

impl Spec {
    fn build(&self, profile: &Path) -> Vec<(PathBuf, Box<dyn nus_sync::Carrier>)> {
        let mut v: Vec<(PathBuf, Box<dyn nus_sync::Carrier>)> = Vec::new();
        if let Some(f) = &self.folder {
            v.push((f.clone(), Box::new(nus_sync::Folder { root: f.clone() })));
        }
        if let Some((remote, auth)) = &self.git {
            let work = profile.join("sync").join("git");
            v.push((work.clone(), Box::new(nus_sync::Git::with_auth(remote, &work, auth.clone()))));
        }
        v
    }
}

/// A file sync replaced, and the copy of ours it kept: `<rel>.<device>[.n].lost`.
fn lost_of(rel_lost: &str) -> Option<(String, String)> {
    let stem = rel_lost.strip_suffix(".lost")?;
    let stem = match stem.rsplit_once('.') {
        Some((head, n)) if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => head,
        _ => stem,
    };
    let (file, device) = stem.rsplit_once('.')?;
    (!file.is_empty() && !device.is_empty()).then(|| (file.to_string(), device.to_string()))
}

fn read(profile: &Path, rel: &str) -> Result<Vec<u8>, String> {
    crate::protected_state::read(&profile.join(rel)).map_err(|e| format!("{rel}: {e}"))
}

fn write(profile: &Path, rel: &str, bytes: &[u8]) -> Result<(), String> {
    let path = profile.join(rel);
    if crate::protected_state::is_private_path(&path) {
        crate::protected_state::write(&path, bytes)
    } else {
        std::fs::write(&path, bytes)
    }
    .map_err(|e| format!("{rel}: {e}"))
}

/// Every conflict sync kept, oldest first.
fn conflicts(profile: &Path) -> Vec<Value> {
    let mut found = Vec::new();
    let mut stack = vec![profile.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            let Ok(rel) = p.strip_prefix(profile) else { continue };
            let rel = rel.to_string_lossy().replace('\\', "/");
            if p.is_dir() {
                // Sync's own clone and the browser's data are not the profile's files.
                if !matches!(rel.as_str(), "sync" | "browser" | "cef" | "replay" | "generations") {
                    stack.push(p);
                }
            } else if let Some((file, device)) = lost_of(&rel) {
                let when = e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
                found.push((when, json!({ "lost": rel, "file": file, "device": device, "at": when })));
            }
        }
    }
    found.sort_by_key(|(at, _)| *at);
    found.into_iter().map(|(_, v)| v).collect()
}

/// A lost copy's path, checked to be one sync made inside the profile.
fn lost_path(profile: &Path, lost: &str) -> Result<(PathBuf, String), String> {
    if lost.contains("..") || Path::new(lost).is_absolute() {
        return Err("that is not a conflict in this profile".into());
    }
    let (file, _) = lost_of(lost).ok_or("that is not a conflict sync kept")?;
    let p = profile.join(lost);
    if !p.is_file() {
        return Err(format!("{lost} is not there any more"));
    }
    Ok((p, file))
}

/// When, as seconds since the epoch: `3h`, `2d`, `45m` ago, or a date
/// `2026-10-01`, or a moment `2026-10-01T14:30`.
pub fn parse_when(text: &str, now: u64) -> Option<u64> {
    let t = text.trim();
    if let Some(n) = t.strip_suffix('m').and_then(|n| n.parse::<u64>().ok()) {
        return Some(now.saturating_sub(n * 60));
    }
    if let Some(n) = t.strip_suffix('h').and_then(|n| n.parse::<u64>().ok()) {
        return Some(now.saturating_sub(n * 3600));
    }
    if let Some(n) = t.strip_suffix('d').and_then(|n| n.parse::<u64>().ok()) {
        return Some(now.saturating_sub(n * 86400));
    }
    let (date, time) = t.split_once('T').unwrap_or((t, "23:59"));
    let mut d = date.split('-').map(|p| p.parse::<i64>().ok());
    let (y, mo, da) = (d.next()??, d.next()??, d.next()??);
    let mut hm = time.split(':').map(|p| p.parse::<i64>().ok());
    let (h, mi) = (hm.next()??, hm.next().flatten().unwrap_or(0));
    // Days from the civil date (Howard Hinnant's algorithm), in UTC.
    let yy = if mo <= 2 { y - 1 } else { y };
    let era = yy.div_euclid(400);
    let yoe = yy - era * 400;
    let doy = (153 * (mo + if mo > 2 { -3 } else { 9 }) + 2) / 5 + da - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    let s = days * 86400 + h * 3600 + mi * 60;
    (s >= 0).then_some(s as u64)
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl App {
    fn spec(&self) -> Spec {
        let b = &self.behavior;
        let folder = (!b.sync_folder.trim().is_empty()).then(|| PathBuf::from(b.sync_folder.trim()));
        let git = (!b.sync_git.trim().is_empty()).then(|| {
            let g = b.sync_git.trim().to_string();
            let auth = crate::forge::load().filter(|f| f.clone_url == g).and_then(|_| crate::forge::auth_header());
            (g, auth)
        });
        Spec { folder, git }
    }

    /// A finished job, applied here; a running one is left alone.
    pub(crate) fn reap_sync_job(&mut self) {
        use std::sync::mpsc::TryRecvError;
        let Some((name, got)) = self.sync.job.as_ref().map(|j| (j.name, j.rx.try_recv())) else { return };
        match got {
            Ok(result) => {
                self.sync.job = None;
                self.finish_job(Some(name), &result);
            }
            Err(TryRecvError::Disconnected) => self.sync.job = None,
            Err(TryRecvError::Empty) => {}
        }
    }

    /// What a finished job changes in the running app.
    fn finish_job(&mut self, name: Option<&'static str>, result: &Result<Value, String>) {
        let Ok(v) = result else { return };
        match name {
            Some("restore") if v.get("restored").is_some() => {
                self.apply_prefs(crate::prefs::Prefs::load());
                self.rules.reload();
                self.load_folders();
                self.layout();
            }
            Some("rotate" | "pair-receive") => self.sync_now(),
            _ => {}
        }
    }

    fn start_job(&mut self, name: &'static str, work: impl FnOnce() -> Result<Value, String> + Send + 'static) -> Result<Value, String> {
        // A job whose answer nobody came for (the terminal closed) is
        // finished here, so it never holds the slot.
        use std::sync::mpsc::TryRecvError;
        match self.sync.job.as_ref().map(|j| (j.name, j.rx.try_recv())) {
            Some((busy, Err(TryRecvError::Empty))) => return Err(format!("sync is busy with {busy}")),
            Some((done, Ok(result))) => {
                self.sync.job = None;
                self.finish_job(Some(done), &result);
            }
            Some((_, Err(TryRecvError::Disconnected))) => self.sync.job = None,
            None => {}
        }
        if self.sync.rx.is_some() {
            return Err("a sync is running; try again in a moment".into());
        }
        let (tx, rx) = channel();
        std::thread::Builder::new()
            .name(format!("sync {name}"))
            .spawn(move || {
                let _ = tx.send(work());
            })
            .map_err(|e| e.to_string())?;
        self.sync.job = Some(Job { name, rx, started: std::time::Instant::now() });
        Ok(json!({ "job": name }))
    }

    /// The `sync` verbs this module answers; None for the ones it doesn't.
    pub(crate) fn sync_remote(&mut self, what: &str, args: &Value) -> Option<Result<Value, String>> {
        let s = |k: &str| args.get(k).and_then(Value::as_str).map(str::to_string);
        let profile = profile_dir();
        Some(match what {
            // Pairing runs in the CLI; it needs the key (made now on a first
            // device) and what to call this one.
            "device" => Ok(json!({ "device": crate::me::device(), "keyed": crate::syncui::key().is_some() })),
            // The key as 24 words, to write down. Only the words: the CLI
            // has no use for the key itself.
            "paper" => match crate::syncui::key() {
                Some(k) => Ok(json!({ "words": nus_sync::paper_key(&k) })),
                None => Err("this device has no sync key yet: nus sync key".into()),
            },
            // Pairing runs here, on a worker; the key never leaves the app
            // except sealed to the other device. The CLI shows the code and
            // the address, and polls `job`.
            "pair-offer" => {
                let word = crate::syncui::make_key();
                let device = crate::me::device();
                let announce = args.get("discover").and_then(Value::as_bool).unwrap_or(false);
                // Only on this machine's LAN address, not every interface.
                let ip = nus_sync::pair::local_address().ok_or("no network address to offer on: is this device on a network?");
                let ip = match ip {
                    Ok(ip) => ip,
                    Err(e) => return Some(Err(e.into())),
                };
                // Scripted checks of a debug build pair over loopback, so a
                // test never opens a port on whatever network it runs on.
                let ip = if crate::browser_runtime::test_harness() && std::env::var_os("NUS_TEST_PAIR_LOOPBACK").is_some() { std::net::IpAddr::from([127, 0, 0, 1]) } else { ip };
                let listener = match std::net::TcpListener::bind((ip, 0)) {
                    Ok(l) => l,
                    Err(e) => return Some(Err(format!("could not listen on {ip}: {e}"))),
                };
                let addr = listener.local_addr().map(|a| a.to_string()).unwrap_or_default();
                let code = nus_sync::pair::new_code();
                let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                self.sync.pair_cancel = Some(cancel.clone());
                let c = code.clone();
                let started = self.start_job("pair-offer", move || {
                    nus_sync::pair::offer(&listener, &c, &word, &device, std::time::Duration::from_secs(180), announce, &cancel).map(|other| json!({ "device": other }))
                });
                started.map(|_| json!({ "job": "pair-offer", "code": code, "address": addr, "expires": 180, "discover": announce }))
            }
            "pair-receive" => {
                let Some(code) = s("code") else { return Some(Err("pair-receive needs a code".into())) };
                if crate::syncui::key().is_some() && args.get("replace").and_then(Value::as_bool) != Some(true) {
                    return Some(Err("this device already has a sync key; receiving replaces it (the CLI asks first)".into()));
                }
                let direct = match s("to").filter(|t| !t.is_empty()) {
                    Some(t) => match t.parse::<std::net::SocketAddr>() {
                        Ok(a) => Some(a),
                        Err(_) => return Some(Err(format!("{t}: an address is like 192.168.1.20:51807"))),
                    },
                    None => None,
                };
                let discover = args.get("discover").and_then(Value::as_bool).unwrap_or(false);
                let device = crate::me::device();
                let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                self.sync.pair_cancel = Some(cancel.clone());
                self.start_job("pair-receive", move || {
                    let (word, from) = nus_sync::pair::receive(&code, &device, direct, std::time::Duration::from_secs(120), discover, &cancel)?;
                    if nus_sync::decode_key(&word).is_none() {
                        return Err("the other device sent something that is not a key".into());
                    }
                    crate::syncui::write_key(&word);
                    Ok(json!({ "device": from }))
                })
            }
            "pair-cancel" => {
                if let Some(c) = self.sync.pair_cancel.take() {
                    c.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                Ok(json!({ "cancelled": true }))
            }
            "job" => match self.sync.job.as_ref().map(|j| j.rx.try_recv()) {
                None => Err("no sync job".into()),
                Some(Err(std::sync::mpsc::TryRecvError::Empty)) => Ok(json!({ "running": self.sync.job.as_ref().map(|j| j.name) })),
                Some(Err(_)) => {
                    self.sync.job = None;
                    Err("the sync job stopped without an answer".into())
                }
                Some(Ok(result)) => {
                    let name = self.sync.job.take().map(|j| j.name);
                    self.finish_job(name, &result);
                    result.map(|v| json!({ "done": name, "result": v }))
                }
            },
            "devices" => {
                let Some(key) = crate::syncui::key() else { return Some(Err("this device has no sync key yet: nus sync key".into())) };
                let spec = self.spec();
                let me = crate::me::device();
                self.start_job("devices", move || {
                    let carriers = spec.build(&profile);
                    if carriers.is_empty() {
                        return Err("no carrier: a folder or a git remote first (nus sync folder · nus sync git · nus sync forge github)".into());
                    }
                    for (_, c) in &carriers {
                        c.before().map_err(|e| format!("{}: {e}", c.name()))?;
                    }
                    let refs: Vec<&dyn nus_sync::Carrier> = carriers.iter().map(|(_, c)| c.as_ref()).collect();
                    let list: Vec<Value> = nus_sync::devices(&key, &refs)
                        .into_iter()
                        .map(|m| json!({ "device": m.device, "at": m.at, "files": m.files.len(), "nus": m.nus, "this": m.device == me }))
                        .collect();
                    Ok(json!({ "devices": list }))
                })
            }
            "rotate" => {
                let Some(old) = crate::syncui::key() else { return Some(Err("this device has no sync key yet: nus sync key".into())) };
                let spec = self.spec();
                let me = crate::me::device();
                let session = self.behavior.sync_session;
                self.library.flush(true);
                self.start_job("rotate", move || {
                    let carriers = spec.build(&profile);
                    if carriers.is_empty() {
                        return Err("no carrier to re-seal".into());
                    }
                    let new = nus_sync::new_key();
                    let roots: Vec<(PathBuf, &dyn nus_sync::Carrier)> = carriers.iter().map(|(r, c)| (r.clone(), c.as_ref())).collect();
                    let refs: Vec<&dyn nus_sync::Carrier> = carriers.iter().map(|(_, c)| c.as_ref()).collect();
                    let before = nus_sync::devices(&old, &refs).len();
                    let (rep, rotated) = nus_sync::rotate(&profile, &me, &old, &new, session, &roots);
                    if !rotated {
                        return Err(format!("{} · nothing changed; the old key still stands", rep.errors.first().map(String::as_str).unwrap_or("the first round failed")));
                    }
                    // The new key is this device's from here, written now rather
                    // than when the CLI collects the answer: a closed terminal
                    // must not leave the device holding a key that reads nothing.
                    let word = nus_sync::encode_key(&new);
                    crate::syncui::write_key(&word);
                    Ok(json!({ "key": word, "words": nus_sync::paper_key(&new), "removed": before.saturating_sub(1), "pushed": rep.pushed.len(), "unfinished": rep.errors }))
                })
            }
            "conflicts" => Ok(json!({ "conflicts": conflicts(&profile) })),
            "conflict" => {
                let Some(lost) = s("lost") else { return Some(Err("conflict needs lost".into())) };
                (|| {
                    let (p, file) = lost_path(&profile, &lost)?;
                    let mine = crate::protected_state::read(&p).map_err(|e| e.to_string())?;
                    let theirs = read(&profile, &file).unwrap_or_default();
                    Ok(json!({ "file": file, "mine": String::from_utf8_lossy(&mine), "theirs": String::from_utf8_lossy(&theirs) }))
                })()
            }
            "resolve" => {
                let (Some(lost), Some(keep)) = (s("lost"), s("keep")) else { return Some(Err("resolve needs lost and keep".into())) };
                let done = (|| {
                    let (p, file) = lost_path(&profile, &lost)?;
                    match keep.as_str() {
                        // Ours again: the kept copy back in place, newer than
                        // theirs, so the next sync carries it out.
                        "mine" => {
                            let mine = crate::protected_state::read(&p).map_err(|e| e.to_string())?;
                            write(&profile, &file, &mine)?;
                        }
                        "theirs" => {}
                        other => return Err(format!("keep mine or theirs, not {other}")),
                    }
                    std::fs::remove_file(&p).map_err(|e| e.to_string())?;
                    Ok(json!({ "file": file, "kept": keep }))
                })();
                if done.is_ok() && keep == "mine" {
                    self.apply_prefs(crate::prefs::Prefs::load());
                    self.rules.reload();
                    self.load_folders();
                    self.layout();
                }
                done
            }
            "restore" => {
                let Some(file) = s("file") else { return Some(Err("restore needs a file".into())) };
                let Some(key) = crate::syncui::key() else { return Some(Err("this device has no sync key yet".into())) };
                let at = match s("at") {
                    Some(t) => match parse_when(&t, now()) {
                        Some(v) => Some(v),
                        None => return Some(Err(format!("{t}: a time is like 3h, 2d, 2026-10-01 or 2026-10-01T14:30"))),
                    },
                    None => None,
                };
                let spec = self.spec();
                let me = crate::me::device();
                if file.contains("..") || Path::new(&file).is_absolute() {
                    return Some(Err("a profile file, like rules.luau or themes/dusk.theme".into()));
                }
                self.start_job("restore", move || {
                    let Some((remote, auth)) = spec.git else {
                        return Err("restoring needs the git carrier, whose history keeps older versions".into());
                    };
                    let work = profile.join("sync").join("git");
                    let git = nus_sync::Git::with_auth(&remote, &work, auth);
                    nus_sync::Carrier::before(&git).map_err(|e| e.to_string())?;
                    let versions = nus_sync::history(&work, &key, &file).map_err(|e| e.to_string())?;
                    let Some(at) = at else {
                        let list: Vec<Value> = versions.iter().take(30).map(|v| json!({ "at": v.at, "device": v.device })).collect();
                        return Ok(json!({ "file": file, "versions": list }));
                    };
                    let v = versions.iter().find(|v| v.at <= at).ok_or("nothing that old on the carrier")?;
                    let bytes = nus_sync::version_at(&work, &key, &file, v).map_err(|e| e.to_string())?;
                    // What is there now is kept, as sync keeps what it replaces.
                    let dest = profile.join(&file);
                    if dest.is_file() {
                        let mut lost = profile.join(format!("{file}.{me}.lost"));
                        let mut n = 2;
                        while lost.exists() {
                            lost = profile.join(format!("{file}.{me}.{n}.lost"));
                            n += 1;
                        }
                        std::fs::copy(&dest, &lost).map_err(|e| e.to_string())?;
                    } else if let Some(parent) = dest.parent() {
                        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                    }
                    write(&profile, &file, &bytes)?;
                    Ok(json!({ "file": file, "restored": v.at, "device": v.device }))
                })
            }
            "forge" => {
                // A token is never passed in: from the GitHub CLI's own
                // sign-in (read here, not by the nus command), or the device
                // flow in a browser.
                let via_gh = args.get("gh").and_then(Value::as_bool) == Some(true);
                let flow = if via_gh {
                    let out = nus_compat::command("gh").args(["auth", "token", "--hostname", "github.com"]).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null()).output();
                    match out {
                        Ok(o) if o.status.success() => {
                            let t = String::from_utf8_lossy(&o.stdout).trim().to_string();
                            if t.is_empty() {
                                return Some(Err("the GitHub CLI is not signed in: gh auth login".into()));
                            }
                            crate::forge::start_token(crate::forge::Kind::GitHub, crate::forge::Kind::GitHub.default_host(), &t)
                        }
                        Ok(_) => return Some(Err("the GitHub CLI is not signed in: gh auth login".into())),
                        Err(_) => return Some(Err("the GitHub CLI (gh) is not installed".into())),
                    }
                } else {
                    match crate::forge::client_id() {
                        Some(id) => crate::forge::start_device(&id),
                        None => return Some(Err("no-client".into())),
                    }
                };
                if let Some(old) = self.sync.forge.replace(flow) {
                    old.cancel();
                }
                Ok(json!({ "started": true }))
            }
            "forge-status" => {
                let Some(phase) = self.sync.forge.as_ref().map(|f| f.phase()) else { return Some(Err("no sign-in under way".into())) };
                use crate::forge::Phase;
                Ok(match phase {
                    Phase::Starting => json!({ "phase": "starting" }),
                    Phase::Code { user_code, uri } => json!({ "phase": "code", "code": user_code, "uri": uri }),
                    Phase::Verifying => json!({ "phase": "verifying" }),
                    Phase::Making => json!({ "phase": "making" }),
                    Phase::Failed(e) => {
                        self.sync.forge = None;
                        json!({ "phase": "failed", "error": e })
                    }
                    Phase::Done(f) => {
                        // As the profile card does: the forge's repo is the git carrier.
                        self.behavior.sync_git = f.clone_url.clone();
                        self.save_prefs();
                        self.sync.forge = None;
                        json!({ "phase": "done", "repo": f.word() })
                    }
                })
            }
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lost_copy_names_its_file_and_device() {
        assert_eq!(lost_of("settings.json.desk.lost"), Some(("settings.json".into(), "desk".into())));
        assert_eq!(lost_of("themes/dusk.theme.laptop.3.lost"), Some(("themes/dusk.theme".into(), "laptop".into())));
        assert_eq!(lost_of("settings.json"), None);
        assert_eq!(lost_of(".lost"), None);
    }

    #[test]
    fn times_read_as_ago_dates_and_moments() {
        let now = 1_790_000_000;
        assert_eq!(parse_when("3h", now), Some(now - 3 * 3600));
        assert_eq!(parse_when("2d", now), Some(now - 2 * 86400));
        assert_eq!(parse_when("1970-01-02", now), Some(86400 + 23 * 3600 + 59 * 60));
        assert_eq!(parse_when("2026-10-01T14:30", now), Some(1_790_865_000));
        assert_eq!(parse_when("yesterday", now), None);
    }
}
