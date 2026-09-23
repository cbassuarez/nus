//! Sync in the app: the key lives in profile/sync/key, the carriers come
//! from the settings, and an exchange runs on a worker every few minutes,
//! on demand (the palette, `nus sync now`), and once at quit. After a
//! pull the prefs, rules and folders reload so what arrived shows.

use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::time::Instant;

use crate::app::App;

pub struct SyncState {
    pub rx: Option<Receiver<nus_sync::Report>>,
    pub last: Option<(Instant, nus_sync::Report)>,
    pub running_since: Option<Instant>,
    pub last_auto: Option<Instant>,
}

impl Default for SyncState {
    fn default() -> Self {
        SyncState { rx: None, last: None, running_since: None, last_auto: None }
    }
}

fn profile_dir() -> PathBuf {
    std::env::current_dir().unwrap_or_default().join("profile")
}

fn key_path() -> PathBuf {
    profile_dir().join("sync").join("key")
}

/// The key on this device, if one has been made or joined.
pub fn key() -> Option<[u8; nus_sync::KEY_LEN]> {
    let word = crate::protected_state::read_text(&key_path()).ok()?;
    nus_sync::decode_key(word.trim())
}

/// Make a key (or keep the one there) and return the word to copy.
pub fn make_key() -> String {
    if let Some(k) = key() {
        return nus_sync::encode_key(&k);
    }
    let k = nus_sync::new_key();
    let word = nus_sync::encode_key(&k);
    write_key(&word);
    word
}

pub fn write_key(word: &str) {
    let p = key_path();
    let _ = std::fs::create_dir_all(p.parent().unwrap());
    let _ = crate::protected_state::write(&p, word.trim().as_bytes());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600));
    }
}

pub fn forget_key() {
    let _ = std::fs::remove_file(key_path());
}

impl App {
    /// The carriers the settings name, if any.
    fn sync_carriers(&self) -> (Option<PathBuf>, Option<String>) {
        let b = &self.behavior;
        let folder = (!b.sync_folder.trim().is_empty()).then(|| PathBuf::from(b.sync_folder.trim()));
        let git = (!b.sync_git.trim().is_empty()).then(|| b.sync_git.trim().to_string());
        (folder, git)
    }

    pub(crate) fn sync_ready(&self) -> bool {
        let (f, g) = self.sync_carriers();
        key().is_some() && (f.is_some() || g.is_some())
    }

    /// Run one exchange on a worker; the report comes back through tick.
    pub(crate) fn sync_now(&mut self) {
        self.library.flush(true);
        if self.sync.rx.is_some() {
            return;
        }
        let Some(k) = key() else {
            self.notice("sync · no key yet · nus sync key");
            return;
        };
        let (folder, git) = self.sync_carriers();
        if folder.is_none() && git.is_none() {
            self.notice("sync · no carrier · a folder or a git remote in settings");
            return;
        }
        let profile = profile_dir();
        let device = crate::me::device();
        let session = self.behavior.sync_session;
        let (tx, rx) = channel();
        self.sync.rx = Some(rx);
        self.sync.running_since = Some(crate::clock::now());
        std::thread::Builder::new()
            .name("sync".into())
            .spawn(move || {
                let mut carriers: Vec<Box<dyn nus_sync::Carrier>> = Vec::new();
                if let Some(f) = folder {
                    carriers.push(Box::new(nus_sync::Folder { root: f }));
                }
                if let Some(g) = git {
                    // A forge's repo speaks with its token, as a header.
                    let auth = crate::forge::load().filter(|f| f.clone_url == g).and_then(|_| crate::forge::auth_header());
                    carriers.push(Box::new(nus_sync::Git::with_auth(&g, &profile.join("sync").join("git"), auth)));
                }
                let refs: Vec<&dyn nus_sync::Carrier> = carriers.iter().map(|c| c.as_ref()).collect();
                let rep = nus_sync::exchange(&profile, &device, &k, session, &refs);
                let _ = tx.send(rep);
            })
            .ok();
        self.dirty = true;
    }

    /// Once a loop: the timer, and a finished exchange.
    pub(crate) fn sync_tick(&mut self) {
        // On a clock, when configured.
        let every = self.behavior.sync_every_min as u64 * 60;
        if every > 0 && self.sync_ready() && self.sync.rx.is_none() {
            let due = self.sync.last_auto.map(|t| crate::clock::since(t).as_secs() >= every).unwrap_or(crate::clock::since(self.started).as_secs() >= 30);
            if due {
                self.sync.last_auto = Some(crate::clock::now());
                self.sync_now();
            }
        }
        let Some(rx) = self.sync.rx.as_ref() else { return };
        let Ok(rep) = rx.try_recv() else { return };
        self.sync.rx = None;
        self.sync.running_since = None;
        let pulled = !rep.pulled.is_empty();
        let summary = if !rep.errors.is_empty() {
            format!("sync · {}", rep.errors[0])
        } else if pulled || !rep.pushed.is_empty() {
            format!("sync · {} in · {} out{}", rep.pulled.len(), rep.pushed.len(), if rep.kept.is_empty() { String::new() } else { format!(" · {} kept as .lost", rep.kept.len()) })
        } else {
            "sync · up to date".into()
        };
        if pulled {
            // What arrived shows: prefs, rules, folders.
            self.apply_prefs(crate::prefs::Prefs::load());
            self.rules.reload();
            self.load_folders();
            self.library.reload();
            self.layout();
        }
        if pulled || !rep.errors.is_empty() {
            self.notice(&summary);
        }
        self.sync.last = Some((crate::clock::now(), rep));
        self.dirty = true;
    }

    /// At quit: one last exchange, waited on briefly.
    pub(crate) fn sync_at_quit(&mut self) {
        if !self.behavior.sync_at_quit || !self.sync_ready() {
            return;
        }
        self.sync_now();
        if let Some(rx) = self.sync.rx.take() {
            let _ = rx.recv_timeout(std::time::Duration::from_secs(8));
        }
    }

    /// A line for the settings page.
    pub(crate) fn sync_status(&self) -> String {
        let key = key().is_some();
        let (f, g) = self.sync_carriers();
        let mut s = String::new();
        s.push_str(if key { "key on this device" } else { "no key" });
        s.push_str(" · ");
        // A forge's repo by its name, not its url.
        let g = g.map(|g| match crate::forge::load() {
            Some(f) if f.clone_url == g => f.word(),
            _ => format!("git {g}"),
        });
        s.push_str(&match (f, g) {
            (Some(f), Some(g)) => format!("folder {} + {}", f.display(), g),
            (Some(f), None) => format!("folder {}", f.display()),
            (None, Some(g)) => g,
            (None, None) => "no carrier".into(),
        });
        if self.sync.rx.is_some() {
            s.push_str(" · syncing…");
        } else if let Some((at, rep)) = &self.sync.last {
            let ago = crate::clock::since(at).as_secs();
            let when = if ago < 60 { format!("{ago}s ago") } else if ago < 3600 { format!("{}m ago", ago / 60) } else { format!("{}h ago", ago / 3600) };
            if rep.errors.is_empty() {
                s.push_str(&format!(" · last {when}: {} in, {} out", rep.pulled.len(), rep.pushed.len()));
            } else {
                s.push_str(&format!(" · last {when}: {}", rep.errors[0]));
            }
        }
        s
    }
}
