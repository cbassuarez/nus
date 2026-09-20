//! Private sessions have their own process and disposable installation root.
//! An explicit CEF context with an empty cache_path provides memory-only storage.
//! A private window is also a smaller app: the CLI, shells, phone control,
//! remote control, external debugging and assistants are all refused, so the
//! session has fewer ways to reach the disk, the network or another process.
use std::sync::OnceLock;

static DOWNLOAD_DIR: OnceLock<std::path::PathBuf> = OnceLock::new();

/// What the private home screen says, and the whole of what incognito claims.
/// Keep it true: this screen, not the documentation, is what someone reads
/// before deciding what a private window is good for.
pub const NOTE: [&str; 5] = [
    "Pages and cookies are temporary.",
    "No saved history or session restore.",
    "Downloads, and where they came from, stay on disk.",
    "Websites and your network can still",
    "see your activity.",
];

/// The name every disposable private root carries, so the sweep can find
/// the ones their process never got to remove.
const PREFIX: &str = "nus-private-";

pub fn downloads_dir() -> Option<&'static std::path::Path> {
    DOWNLOAD_DIR.get().map(std::path::PathBuf::as_path)
}

pub fn enabled() -> bool {
    static PRIVATE: OnceLock<bool> = OnceLock::new();
    *PRIVATE.get_or_init(|| std::env::args().any(|a| a == "--incognito"))
}

pub fn prepare() -> anyhow::Result<Option<tempfile::TempDir>> {
    if std::env::args().any(|a| a.starts_with("--type=")) {
        return Ok(None);
    }
    // Every launch clears what a killed private session left behind, since
    // that session could not clean up after itself.
    sweep();
    if !enabled() {
        return Ok(None);
    }
    // Resolve the persistent destination before changing directories. Even
    // without HOME/USERPROFILE, the fallback must stay outside the private root.
    let _ = DOWNLOAD_DIR.set(crate::browser::default_downloads_dir());
    let root = tempfile::Builder::new().prefix(PREFIX).tempdir()?;
    // The owner, for the sweep: a root whose process is gone is abandoned.
    std::fs::write(root.path().join("owner"), std::process::id().to_string())?;
    std::env::set_current_dir(root.path())?;
    std::fs::create_dir(root.path().join("profile"))?;
    std::fs::write(root.path().join("profile/onboarded"), "skip")?;
    if let Ok(json) = std::env::var("NUS_PRIVATE_LOOK") {
        let mut prefs: crate::prefs::Prefs = serde_json::from_str(&json)?;
        constrain(&mut prefs);
        std::fs::write(
            root.path().join("profile/settings.json"),
            serde_json::to_vec(&prefs)?,
        )?;
        std::env::remove_var("NUS_PRIVATE_LOOK");
    }
    Ok(Some(root))
}

/// Only appearance crosses the boundary: no account data, rules, startup
/// pages, custom search URLs, automation or work folders.
pub fn launch() -> anyhow::Result<()> {
    let p = crate::prefs::Prefs::load();
    let look = crate::prefs::Prefs {
        schema: crate::prefs::SCHEMA,
        surface: p.surface,
        sidebar: p.sidebar,
        motion: p.motion,
        theme: p.theme,
        theme_mode: p.theme_mode,
        preset_name: p.preset_name,
        cursor: p.cursor,
        header: p.header,
        ..Default::default()
    };
    let mut command = std::process::Command::new(std::env::current_exe()?);
    command
        .arg("--incognito")
        .env("NUS_PRIVATE_LOOK", serde_json::to_string(&look)?)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    for key in [
        "NUS_SHOT",
        "NUS_SHOT2",
        "NUS_SHOT_DIR",
        "NUS_SHOT_OUT",
        "NUS_DUMP",
        "NUS_SHELL",
    ] {
        command.env_remove(key);
    }
    let mut child = command.spawn()?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// Remove private roots whose process is gone. A clean exit drops its own
/// root, but a crash, a kill or a power cut leaves one behind, holding the
/// session's cache and site storage on disk under a predictable name. The
/// owner file is the test: a live pid keeps its root, an absent one does not.
/// Unlinking is not erasure — the pages are gone from the filesystem, not
/// necessarily from the disk — so this bounds the residue, it does not shred it.
pub fn sweep() {
    sweep_in(&std::env::temp_dir());
}

fn sweep_in(temporary: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(temporary) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_ours = path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with(PREFIX));
        if !is_ours || !path.is_dir() {
            continue;
        }
        // No owner file means the root never finished being set up; anything
        // unreadable or still owned is left exactly where it is.
        let owner = std::fs::read_to_string(path.join("owner"));
        let abandoned = match &owner {
            Ok(text) => match text.trim().parse::<u32>() {
                Ok(pid) => !alive(pid),
                Err(_) => false,
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => true,
            Err(_) => false,
        };
        if abandoned {
            if let Err(e) = std::fs::remove_dir_all(&path) {
                tracing::debug!("Could not remove an abandoned private root: {e}");
            }
        }
    }
}

/// Is that process still running? A pid that has been reused answers yes and
/// its root survives another launch, which is the safe way to be wrong.
#[cfg(unix)]
fn alive(pid: u32) -> bool {
    // Signal 0 runs kill's existence and permission checks without sending
    // anything. Pid 0 would mean the caller's whole process group, which no
    // root can own, so it never reaches the call.
    if pid == 0 {
        return false;
    }
    let found = unsafe { libc::kill(pid as libc::pid_t, 0) } == 0;
    // EPERM means the process exists but belongs to someone else.
    found || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(not(unix))]
fn alive(_pid: u32) -> bool {
    // Without a cheap liveness check, keep every root and let the owning
    // process remove its own on exit.
    true
}

pub fn constrain(p: &mut crate::prefs::Prefs) {
    use crate::settings::*;
    p.behavior = Some(Behavior {
        lead: Lead::Browser,
        then: Then::Prompt,
        new_window: NewWindow::Prompt,
        splash: SplashMode::None,
        start_on_launch: false,
        startup_sound: false,
        remember: false,
        replay: ReplayKeep::Off,
        journal: false,
        phone: false,
        sync_at_quit: false,
        sync_every_min: 0,
        hatch_background: false,
        ports_remember: false,
        ..Default::default()
    });
    p.window_name = None;
    p.pinned_tabs=Some(vec![]);
    if let Some(side)=&mut p.sidebar {side.live_github=false;side.live_ports=false;}
}

pub fn allows(action: &crate::app::Action) -> bool {
    use crate::app::Action::*;
    matches!(
        action,
        Application(_)
            | NewPrivateWindow
            | NewWindow
            | NewBrowser(_)
            | OpenInPane(_)
            | SwitchTab(_)
            | Home
            | CloseTab
            | Reopen
            | ToggleSidebar
            | ToggleSplit
            | Downloads
            | CopyUrl
            | Pip
            | Focus
            | Compact
            | Tile
            | Untile
            | TileSwap
            | KeepPeek
            | OpenPalette(_)
            | Report(_)
            | Noop
    )
}

impl crate::app::App {
    pub(crate) fn private_rows(&self, input: &str) -> Vec<crate::app::PaletteRow> {
        use crate::app::{Action, PaletteRow};
        let q = input.trim();
        let row = |text: String, action| PaletteRow {
            num: "→".into(),
            text,
            action,
        };
        if q.is_empty() {
            return Vec::new();
        }
        match q.to_lowercase().as_str() {
            "report a bug" => {
                return vec![row(
                    "Report a bug · GitHub draft".into(),
                    Action::Report(crate::support::Kind::Bug),
                )]
            }
            "request a feature" => {
                return vec![row(
                    "Request a feature · GitHub draft".into(),
                    Action::Report(crate::support::Kind::Feature),
                )]
            }
            _ => {}
        }
        let url = crate::app::strict_url(q).unwrap_or_else(|| self.behavior.prompt.search_url(q));
        vec![row(
            format!("Browse privately · {q}"),
            Action::NewBrowser(url),
        )]
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn private_preferences_cannot_enable_persistence() {
        let mut p = crate::prefs::Prefs::default();
        super::constrain(&mut p);
        let b = p.behavior.unwrap();
        assert!(!b.remember && !b.phone && !b.sync_at_quit && !b.hatch_background);
        assert!(b.replay.days().is_none());
        assert_eq!(b.then, crate::settings::Then::Prompt);
    }

    /// The note is the promise. It may be reworded, but it may not quietly
    /// stop saying that the network sees everything or that downloads stay.
    #[test]
    fn the_private_note_keeps_its_disclosures() {
        let note = super::NOTE.join(" ").to_lowercase();
        assert!(note.contains("websites and your network can still see your activity"));
        assert!(note.contains("no saved history"));
        assert!(note.contains("downloads, and where they came from, stay on disk"));
    }

    /// A reaped child's pid is the one number we know belongs to no process.
    #[cfg(unix)]
    #[test]
    fn the_sweep_takes_abandoned_roots_and_leaves_live_ones() {
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let dead = child.id();
        child.wait().unwrap();

        let temporary = tempfile::tempdir().unwrap();
        let abandoned = temporary.path().join("nus-private-gone");
        let unfinished = temporary.path().join("nus-private-unfinished");
        let live = temporary.path().join("nus-private-live");
        let stranger = temporary.path().join("something-else");
        for directory in [&abandoned, &unfinished, &live, &stranger] {
            std::fs::create_dir(directory).unwrap();
        }
        std::fs::write(abandoned.join("owner"), dead.to_string()).unwrap();
        std::fs::write(live.join("owner"), std::process::id().to_string()).unwrap();
        std::fs::write(stranger.join("owner"), dead.to_string()).unwrap();

        super::sweep_in(temporary.path());

        assert!(!abandoned.exists(), "an abandoned root survived the sweep");
        assert!(!unfinished.exists(), "a root with no owner survived the sweep");
        assert!(live.exists(), "the sweep removed a live session's root");
        assert!(stranger.exists(), "the sweep reached outside its own prefix");
    }
}
