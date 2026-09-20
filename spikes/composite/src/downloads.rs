//! Browser downloads: one process-wide queue, persistent history, and local UI.
use crate::app::{App, Pane};
use cef::{ImplBeforeDownloadCallback, ImplDownloadItemCallback};
use nus_render::{Rect, Scene, Style};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Rename {
    #[default]
    Off,
    All,
    Selective,
}
static RENAME: AtomicU8 = AtomicU8::new(0);
pub static REVISION: AtomicU64 = AtomicU64::new(0);
pub fn set_rename(mode: Rename) {
    RENAME.store(mode as u8, Ordering::Relaxed);
}
pub fn rename_mode() -> Rename {
    match RENAME.load(Ordering::Relaxed) {
        1 => Rename::All,
        2 => Rename::Selective,
        _ => Rename::Off,
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Download {
    pub key: u64,
    pub id: u32,
    pub name: String,
    pub original: String,
    pub path: String,
    pub url: String,
    pub source_url: String,
    pub container: String,
    pub title: String,
    pub received: i64,
    pub total: i64,
    pub speed: i64,
    pub done: bool,
    pub cancelled: bool,
    pub interrupted: bool,
    pub paused: bool,
    pub started: u64,
    #[serde(skip)]
    pub live: bool,
}
impl Download {
    pub fn active(&self) -> bool {
        self.live && !self.done && !self.cancelled && !self.interrupted
    }
    pub fn status(&self) -> String {
        if self.done {
            return format!("Complete · {}", bytes(self.received.max(self.total)));
        }
        if self.cancelled {
            return "Cancelled".into();
        }
        if self.interrupted {
            return "Interrupted · ready to retry".into();
        }
        if self.paused {
            return format!("Paused · {} received", bytes(self.received));
        }
        let amount = if self.total > 0 {
            format!(
                "{} of {} · {}%",
                bytes(self.received),
                bytes(self.total),
                (self.received.saturating_mul(100) / self.total).clamp(0, 100)
            )
        } else {
            format!("{} received", bytes(self.received))
        };
        if self.speed > 0 {
            format!("{amount} · {}/s", bytes(self.speed))
        } else {
            format!("{amount} · waiting for data")
        }
    }
}
pub fn bytes(n: i64) -> String {
    let n = n.max(0) as f64;
    if n >= 1_073_741_824.0 {
        format!("{:.1} GB", n / 1_073_741_824.0)
    } else if n >= 1_048_576.0 {
        format!("{:.1} MB", n / 1_048_576.0)
    } else if n >= 1024.0 {
        format!("{:.0} KB", n / 1024.0)
    } else {
        format!("{n:.0} B")
    }
}
fn history_path() -> PathBuf {
    PathBuf::from("profile/downloads.json")
}
pub fn init() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let mut rows: Vec<Download> = std::fs::read(history_path())
            .ok()
            .and_then(|v| serde_json::from_slice(&v).ok())
            .unwrap_or_default();
        for d in &mut rows {
            if !d.done && !d.cancelled {
                d.interrupted = true;
                d.paused = false;
            }
            d.live = false;
        }
        *crate::browser::DOWNLOADS.lock().unwrap() = rows;
    });
}
pub fn save(rows: &[Download]) {
    if let Ok(data) = serde_json::to_vec_pretty(rows) {
        let path = history_path();
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, data).is_ok() {
            let _ = std::fs::rename(tmp, path);
        }
    }
}
pub fn list() -> Vec<Download> {
    crate::browser::DOWNLOADS
        .lock()
        .unwrap()
        .iter()
        .rev()
        .cloned()
        .collect()
}
pub fn changed() {
    REVISION.fetch_add(1, Ordering::Relaxed);
}
thread_local! {
    static CALLBACKS:std::cell::RefCell<std::collections::HashMap<u64,cef::DownloadItemCallback>>=std::cell::RefCell::new(Default::default());
    /// Downloads to cancel as soon as Chromium hands us their callback: a
    /// save dialog that was dismissed, after the download had to start.
    static DROP_WHEN_SEEN: std::cell::RefCell<std::collections::HashSet<u64>> = std::cell::RefCell::new(Default::default());
}
pub fn track(key: u64, cb: Option<&mut cef::DownloadItemCallback>, active: bool) {
    let drop = DROP_WHEN_SEEN.with(|d| d.borrow_mut().remove(&key));
    CALLBACKS.with(|c| {
        let mut c = c.borrow_mut();
        if active {
            if let Some(cb) = cb {
                if drop {
                    cb.cancel();
                    return;
                }
                c.insert(key, cb.clone());
            }
        } else {
            c.remove(&key);
        }
    });
}
/// Cancel this download the moment it can be.
pub fn drop_when_seen(key: u64) {
    DROP_WHEN_SEEN.with(|d| {
        d.borrow_mut().insert(key);
    });
    let cb = CALLBACKS.with(|c| c.borrow().get(&key).cloned());
    if let Some(cb) = cb {
        cb.cancel();
    }
}

impl App {
    /// BROWSER · DOWNLOADS · CHOOSE FOLDER: the system's folder dialog;
    /// `tend_download_dialogs` takes the answer.
    pub(crate) fn pick_download_dir(&mut self) {
        match crate::pick::folder(&self.window, "Where downloads go") {
            Ok(p) => self.download_ui.dir_pick = Some(p),
            Err(e) => self.notice(&format!("downloads · {e}")),
        }
    }

    /// Once a tick: the folder dialog, the save dialogs, and what a
    /// finished download asked for.
    pub(crate) fn tend_downloads(&mut self) {
        // The folder chooser.
        if let Some(mut pick) = self.download_ui.dir_pick.take() {
            match pick.poll() {
                None => self.download_ui.dir_pick = Some(pick),
                Some(Ok(Some(dir))) => {
                    self.behavior.download_dir = dir.display().to_string();
                    let b = self.behavior.clone();
                    self.apply_behavior_statics(&b);
                    self.save_prefs();
                    self.notice(&format!("downloads · {}", dir.display()));
                    self.dirty = true;
                }
                Some(Ok(None)) => {}
                Some(Err(e)) => self.notice(&format!("downloads · {e}")),
            }
        }
        // ASK WHERE TO SAVE: one dialog at a time, in the order they came.
        if self.download_ui.save_ask.is_none() {
            let next = crate::browser::SAVE_ASKS.with(|q| {
                let mut q = q.borrow_mut();
                if q.is_empty() { None } else { Some(q.remove(0)) }
            });
            if let Some(ask) = next {
                let dir = ask.suggested.parent().map(|p| p.to_path_buf()).unwrap_or_else(crate::browser::downloads_dir);
                let name = ask.suggested.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "download".into());
                match crate::pick::save_as(&self.window, "Save as", dir, name) {
                    Ok(p) => self.download_ui.save_ask = Some((ask, p)),
                    Err(e) => {
                        // No dialog to be had: the folder it was going to.
                        self.notice(&format!("downloads · {e} · saved to the folder"));
                        ask.callback.cont(Some(&ask.suggested.to_string_lossy().as_ref().into()), 0);
                    }
                }
            }
        }
        if let Some((ask, mut pick)) = self.download_ui.save_ask.take() {
            match pick.poll() {
                None => self.download_ui.save_ask = Some((ask, pick)),
                Some(Ok(Some(path))) => {
                    {
                        let mut rows = crate::browser::DOWNLOADS.lock().unwrap();
                        if let Some(d) = rows.iter_mut().find(|d| d.key == ask.key) {
                            d.path = path.display().to_string();
                            d.name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or(d.name.clone());
                        }
                        save(&rows);
                    }
                    changed();
                    ask.callback.cont(Some(&path.to_string_lossy().as_ref().into()), 0);
                }
                Some(Ok(None)) | Some(Err(_)) => {
                    // Dismissed: Chromium wants a path to start on before it
                    // can be cancelled, so it starts and is dropped at once.
                    drop_when_seen(ask.key);
                    ask.callback.cont(Some(&ask.suggested.to_string_lossy().as_ref().into()), 0);
                    self.notice("download · cancelled");
                }
            }
        }
        // WHEN A DOWNLOAD FINISHES.
        let just_done: Vec<(u64, String)> = list().into_iter().filter(|d| d.live && d.done && !self.download_ui.finished.contains(&d.key)).map(|d| (d.key, d.name.clone())).collect();
        for (key, name) in just_done {
            self.download_ui.finished.insert(key);
            match self.behavior.download_done {
                crate::settings::DownloadDone::Notice => self.toast_with(Some(nus_render::text::icons::DOWNLOAD), "DOWNLOADED", name, Some(crate::toast::Act::RevealDownload(key))),
                crate::settings::DownloadDone::Reveal => self.download_action(Hit::Reveal(key)),
                crate::settings::DownloadDone::Open => self.download_action(Hit::Open(key)),
                crate::settings::DownloadDone::Quiet => {}
            }
        }
    }
}

/// Keep filenames portable and confined to Downloads, even with hostile headers.
pub fn safe_name(raw: &str) -> String {
    let raw = raw.rsplit(['/', '\\']).next().unwrap_or("");
    let mut name = String::new();
    for c in raw.chars() {
        if c.is_control() || matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|') {
            name.push(' ');
        } else {
            name.push(c);
        }
    }
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut name = name.trim_matches([' ', '.']).to_string();
    while name.len() > 220 {
        name.pop();
    }
    if name.is_empty() {
        name = "download".into();
    }
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    if matches!(
        stem.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    ) {
        name.insert(0, '_');
    }
    name
}
fn extension(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    for ext in [".tar.gz", ".tar.bz2", ".tar.xz", ".tar.zst"] {
        if lower.ends_with(ext) {
            return name[name.len() - ext.len()..].into();
        }
    }
    Path::new(name)
        .extension()
        .map(|s| format!(".{}", s.to_string_lossy()))
        .unwrap_or_default()
}
pub fn filename(original: &str, title: &str, mode: Rename) -> String {
    let original = safe_name(original);
    if mode == Rename::Off {
        return original;
    }
    let ext = extension(&original);
    if mode == Rename::Selective {
        let readable = matches!(
            ext.to_ascii_lowercase().as_str(),
            ".pdf"
                | ".epub"
                | ".doc"
                | ".docx"
                | ".odt"
                | ".ppt"
                | ".pptx"
                | ".png"
                | ".jpg"
                | ".jpeg"
                | ".webp"
                | ".gif"
                | ".avif"
                | ".mp3"
                | ".m4a"
                | ".mp4"
                | ".mov"
                | ".webm"
        );
        // Versioned names, checksums and signed companion names retain their identity.
        let technical = original
            .split(|c: char| !c.is_ascii_alphanumeric())
            .any(|s| s.len() >= 12 && s.chars().all(|c| c.is_ascii_hexdigit()))
            || original.to_lowercase().contains("signed")
            || original
                .split('.')
                .filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()))
                .count()
                >= 2;
        if !readable || technical {
            return original;
        }
    }
    let title = title.trim();
    if title.is_empty()
        || title.contains("://")
        || matches!(
            title.to_lowercase().as_str(),
            "download" | "downloads" | "untitled" | "home" | "new tab"
        )
    {
        return original;
    }
    // A page title is text, not a path. Slashes become separators, not directories.
    let title = safe_name(&title.replace(['/', '\\'], " - "));
    let stem = if !ext.is_empty() && title.to_lowercase().ends_with(&ext.to_lowercase()) {
        title[..title.len() - ext.len()].to_string()
    } else {
        title
    };
    let mut stem = stem;
    while stem.len() + ext.len() > 220 {
        stem.pop();
    }
    format!("{}{ext}", stem.trim_end())
}
pub fn available_path(dir: &Path, name: &str, rows: &[Download]) -> PathBuf {
    let ext = extension(name);
    let stem = &name[..name.len() - ext.len()];
    let mut path = dir.join(name);
    let mut n = 1;
    while path.exists()
        || rows
            .iter()
            .any(|d| d.active() && Path::new(&d.path) == path)
    {
        n += 1;
        path = dir.join(format!("{stem} ({n}){ext}"));
    }
    path
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Close,
    Page,
    Folder,
    Clear,
    Search,
    ClearSearch,
    Open(u64),
    Retry(u64),
    Pause(u64),
    Resume(u64),
    Cancel(u64),
    Reveal(u64),
    Source(u64),
}
impl Hit {
    pub fn label(self) -> &'static str {
        match self {
            Self::Close => "Close downloads",
            Self::Page => "Open Downloads page",
            Self::Folder => "Open Downloads folder",
            Self::Clear => "Clear finished download history; keep files",
            Self::Search => "Search downloads",
            Self::ClearSearch => "Clear download search",
            Self::Open(_) => "Open downloaded file",
            Self::Retry(_) => "Retry download",
            Self::Pause(_) => "Pause download",
            Self::Resume(_) => "Resume download",
            Self::Cancel(_) => "Cancel download",
            Self::Reveal(_) => "Show downloaded file in folder",
            Self::Source(_) => "Open download source",
        }
    }
    fn key(self) -> Option<u64> {
        match self {
            Self::Open(k)
            | Self::Retry(k)
            | Self::Pause(k)
            | Self::Resume(k)
            | Self::Cancel(k)
            | Self::Reveal(k)
            | Self::Source(k) => Some(k),
            _ => None,
        }
    }
}
pub struct DownloadsPane {
    pub rect: Rect,
    pub scroll: f32,
}
impl Default for DownloadsPane {
    fn default() -> Self {
        Self {
            rect: Rect::new(0.0, 0.0, 1.0, 1.0),
            scroll: 0.0,
        }
    }
}
#[derive(Default)]
pub struct Ui {
    pub hits: Vec<(Rect, Hit)>,
    pub anchor: Option<Rect>,
    pub rect: Option<Rect>,
    pub scroll: f32,
    pub reach: f32,
    pub revision: u64,
    pub hover_since: Option<std::time::Instant>,
    pub focus: Option<Hit>,
    pub query: String,
    pub cursor: usize,
    pub select_all: bool,
    /// BROWSER · DOWNLOADS · LOCATION: the folder dialog while it is up.
    pub dir_pick: Option<crate::pick::Picker>,
    /// ASK WHERE TO SAVE: the download waiting, and its save dialog.
    pub save_ask: Option<(crate::browser::SaveAsk, crate::pick::Picker)>,
    /// Finished downloads already acted on (WHEN A DOWNLOAD FINISHES).
    pub finished: std::collections::HashSet<u64>,
}
pub(crate) fn matches(d: &Download, query: &str) -> bool {
    let text = format!(
        "{} {} {} {}",
        d.name,
        d.original,
        crate::sites::host_of(if d.source_url.is_empty() {
            &d.url
        } else {
            &d.source_url
        }),
        d.status()
    )
    .to_lowercase();
    query
        .split_whitespace()
        .all(|word| text.contains(&word.to_lowercase()))
}
impl Ui {
    fn insert(&mut self, text: &str) {
        if self.select_all {
            self.query.clear();
            self.cursor = 0;
            self.select_all = false;
        }
        let text: String = text.chars().filter(|c| !c.is_control()).collect();
        self.query.insert_str(self.cursor, &text);
        self.cursor += text.len();
    }
    fn backspace(&mut self) {
        if self.select_all {
            self.query.clear();
            self.cursor = 0;
            self.select_all = false;
        } else if self.cursor > 0 {
            let previous = self.query[..self.cursor]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.query.drain(previous..self.cursor);
            self.cursor = previous;
        }
    }
}
impl App {
    pub(crate) fn open_downloads(&mut self) {
        self.close_menus();
        self.download_ui.focus = None;
        if let Some(i) = self
            .tabs
            .iter()
            .position(|t| matches!(t.left, Pane::Downloads(_)))
        {
            self.activate(i);
            return;
        }
        let tab = self.make_tab(Pane::Downloads(DownloadsPane::default()), None);
        self.tabs.push(tab);
        self.activate(self.tabs.len() - 1);
        self.layout();
        self.save_session();
    }
    fn download_open_url(&mut self, url: &str, container: &str) {
        let container = if container.is_empty() {
            crate::containers::PERSONAL
        } else {
            container
        };
        if let Some(w) = self.new_web_pane_in(url, container) {
            self.close_menus();
            let tab = self.make_tab(Pane::Web(w), None);
            self.tabs.push(tab);
            self.activate(self.tabs.len() - 1);
            self.layout();
            self.save_session();
        }
    }
    pub(crate) fn download_label(&self, hit: Hit) -> String {
        let name = hit
            .key()
            .and_then(|key| list().into_iter().find(|d| d.key == key))
            .map(|d| d.name);
        name.map(|name| format!("{}: {name}", hit.label()))
            .unwrap_or_else(|| hit.label().into())
    }
    pub(crate) fn download_query_changed(&mut self) {
        self.download_ui.scroll = 0.0;
        if let Some(t) = self.tabs.get_mut(self.active) {
            for p in std::iter::once(&mut t.left).chain(t.right.iter_mut()) {
                if let Pane::Downloads(p) = p {
                    p.scroll = 0.0;
                }
            }
        }
        self.dirty = true;
    }
    pub(crate) fn download_action(&mut self, hit: Hit) {
        match hit {
            Hit::Close => self.close_menus(),
            Hit::Page => self.open_downloads(),
            Hit::Search => {
                self.download_ui.focus = Some(Hit::Search);
                self.download_ui.cursor = self.download_ui.query.len();
            }
            Hit::ClearSearch => {
                self.download_ui.query.clear();
                self.download_ui.cursor = 0;
                self.download_ui.select_all = false;
                self.download_ui.focus = Some(Hit::Search);
                self.download_query_changed();
            }
            Hit::Folder => reveal(&crate::browser::downloads_dir(), false),
            Hit::Clear => {
                let mut rows = crate::browser::DOWNLOADS.lock().unwrap();
                rows.retain(|d| d.active());
                save(&rows);
                changed();
            }
            Hit::Pause(key) | Hit::Resume(key) | Hit::Cancel(key) => {
                // Clone before calling CEF: it may synchronously update the list.
                let cb = CALLBACKS.with(|c| c.borrow().get(&key).cloned());
                if let Some(cb) = cb {
                    match hit {
                        Hit::Pause(_) => cb.pause(),
                        Hit::Resume(_) => cb.resume(),
                        _ => cb.cancel(),
                    }
                }
                self.download_ui.focus = match hit {
                    Hit::Pause(k) => Some(Hit::Resume(k)),
                    Hit::Resume(k) => Some(Hit::Pause(k)),
                    _ => None,
                };
            }
            Hit::Open(key) | Hit::Reveal(key) | Hit::Source(key) | Hit::Retry(key) => {
                if let Some(d) = list().into_iter().find(|d| d.key == key) {
                    match hit {
                        Hit::Open(_) if d.done => open_file(Path::new(&d.path)),
                        Hit::Reveal(_) if d.done => reveal(Path::new(&d.path), true),
                        Hit::Source(_) => self.download_open_url(
                            if d.source_url.is_empty() {
                                &d.url
                            } else {
                                &d.source_url
                            },
                            &d.container,
                        ),
                        Hit::Retry(_) if !d.active() => {
                            // Preserve the originating cookie jar, including when the
                            // Downloads page is in a different container.
                            let container = if d.container.is_empty() {
                                crate::containers::PERSONAL
                            } else {
                                &d.container
                            };
                            let web = self
                                .tabs
                                .iter()
                                .flat_map(|t| std::iter::once(&t.left).chain(t.right.iter()))
                                .find_map(|p| if let Pane::Web(w) = p { (w.container == container).then_some(w) } else { None });
                            if let Some(w) = web {
                                w.tab.download(&d.url);
                            } else {
                                self.download_open_url(&d.url, container);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        self.dirty = true;
    }
    pub(crate) fn download_click(&mut self, x: f32, y: f32) -> bool {
        if let Some((_, hit)) = self
            .download_ui
            .hits
            .iter()
            .rev()
            .find(|(r, _)| r.contains(x, y))
            .copied()
        {
            self.download_ui.focus = Some(hit);
            self.download_ui.select_all = false;
            self.download_action(hit);
            return true;
        }
        self.download_ui.focus = None;
        if self.dl_menu {
            if !self.download_ui.rect.is_some_and(|r| r.contains(x, y)) {
                self.close_menus();
            }
            return true;
        }
        false
    }
    pub(crate) fn download_key(&mut self, ev: &crate::app::KeyIn) -> bool {
        use winit::keyboard::{Key, NamedKey};
        let command = if cfg!(target_os = "macos") {
            self.mods.super_key()
        } else {
            self.mods.control_key()
        };
        let shift = self.mods.shift_key();
        match &ev.logical_key {
            Key::Character(c) if command && c.eq_ignore_ascii_case("f") => {
                self.download_action(Hit::Search);
                self.download_ui.select_all = true;
            }
            Key::Named(NamedKey::Escape) => {
                if self.dl_menu {
                    self.close_menus();
                } else if !self.download_ui.query.is_empty() {
                    self.download_action(Hit::ClearSearch);
                } else {
                    self.download_ui.focus = None;
                }
            }
            Key::Named(NamedKey::Tab) => {
                let hits = &self.download_ui.hits;
                let n = hits.len();
                if n > 0 {
                    let i = self
                        .download_ui
                        .focus
                        .and_then(|f| hits.iter().position(|(_, h)| *h == f))
                        .map(|i| if shift { (i + n - 1) % n } else { (i + 1) % n })
                        .unwrap_or(if shift { n - 1 } else { 0 });
                    self.download_ui.focus = Some(hits[i].1);
                    self.download_ui.select_all = false;
                }
            }
            _ if self.download_ui.focus == Some(Hit::Search) && !self.dl_menu => {
                match &ev.logical_key {
                    Key::Character(c) if command && c.eq_ignore_ascii_case("a") => {
                        self.download_ui.select_all = true
                    }
                    Key::Character(c) if command && c.eq_ignore_ascii_case("v") => {
                        if let Ok(mut cb) = arboard::Clipboard::new() {
                            if let Ok(text) = cb.get_text() {
                                self.download_ui.insert(&text);
                                self.download_query_changed();
                            }
                        }
                    }
                    Key::Character(c)
                        if command
                            && (c.eq_ignore_ascii_case("c") || c.eq_ignore_ascii_case("x")) =>
                    {
                        if self.download_ui.select_all {
                            if let Ok(mut cb) = arboard::Clipboard::new() {
                                let _ = cb.set_text(self.download_ui.query.clone());
                            }
                            if c.eq_ignore_ascii_case("x") {
                                self.download_ui.backspace();
                                self.download_query_changed();
                            }
                        }
                    }
                    Key::Named(NamedKey::Backspace) => {
                        self.download_ui.backspace();
                        self.download_query_changed();
                    }
                    Key::Named(NamedKey::Delete) => {
                        if self.download_ui.select_all {
                            self.download_ui.backspace();
                        } else {
                            let pos = self.download_ui.cursor;
                            if pos < self.download_ui.query.len() {
                                self.download_ui.query.remove(pos);
                            }
                        }
                        self.download_query_changed();
                    }
                    Key::Named(NamedKey::Home) => {
                        self.download_ui.cursor = 0;
                        self.download_ui.select_all = false;
                    }
                    Key::Named(NamedKey::End) => {
                        self.download_ui.cursor = self.download_ui.query.len();
                        self.download_ui.select_all = false;
                    }
                    Key::Named(NamedKey::ArrowLeft) => {
                        self.download_ui.cursor = self.download_ui.query[..self.download_ui.cursor]
                            .char_indices()
                            .next_back()
                            .map(|(i, _)| i)
                            .unwrap_or(0);
                        self.download_ui.select_all = false;
                    }
                    Key::Named(NamedKey::ArrowRight) => {
                        let i = self.download_ui.cursor;
                        self.download_ui.cursor += self.download_ui.query[i..]
                            .chars()
                            .next()
                            .map(char::len_utf8)
                            .unwrap_or(0);
                        self.download_ui.select_all = false;
                    }
                    Key::Named(NamedKey::Space) => {
                        self.download_ui.insert(" ");
                        self.download_query_changed();
                    }
                    Key::Character(c) if !command && !self.mods.control_key() => {
                        self.download_ui.insert(c);
                        self.download_query_changed();
                    }
                    Key::Named(NamedKey::Enter) => self.download_ui.focus = None,
                    _ => return self.dl_menu,
                }
            }
            Key::Named(NamedKey::Enter) | Key::Named(NamedKey::Space) => {
                if let Some(hit) = self.download_ui.focus {
                    self.download_action(hit);
                }
            }
            Key::Named(
                key @ (NamedKey::ArrowUp
                | NamedKey::ArrowDown
                | NamedKey::PageUp
                | NamedKey::PageDown
                | NamedKey::Home
                | NamedKey::End),
            ) => {
                let delta = self.px(if matches!(key, NamedKey::PageUp | NamedKey::PageDown) {
                    300.0
                } else {
                    60.0
                }) * if matches!(key, NamedKey::ArrowUp | NamedKey::PageUp) {
                    -1.0
                } else {
                    1.0
                };
                let reach = self.download_ui.reach;
                let scroll = if self.dl_menu {
                    Some(&mut self.download_ui.scroll)
                } else {
                    self.tabs.get_mut(self.active).and_then(|t| {
                        if let Pane::Downloads(p) = t.focused() {
                            Some(&mut p.scroll)
                        } else {
                            None
                        }
                    })
                };
                if let Some(scroll) = scroll {
                    *scroll = match key {
                        NamedKey::Home => 0.0,
                        NamedKey::End => reach,
                        _ => (*scroll + delta).clamp(0.0, reach),
                    };
                }
                self.download_ui.focus = None;
            }
            _ => return self.dl_menu,
        }
        self.dirty = true;
        true
    }
    pub(crate) fn download_button(&mut self, scene: &mut Scene, r: Rect, label: &str, hit: Hit) {
        let hot = r.contains(self.mouse.0, self.mouse.1);
        let focused = self.download_ui.focus == Some(hit);
        if hot || focused {
            scene.rect(r, self.theme.tint);
        }
        if focused {
            scene.outline(r, self.px(1.0), self.surface.signal);
        }
        let color = if hit == Hit::Page {
            self.surface.signal
        } else {
            self.theme.ink
        };
        if label.is_empty() {
            use nus_render::text::icons;
            let icon = match hit {
                Hit::Close | Hit::Cancel(_) | Hit::ClearSearch => icons::CLOSE,
                Hit::Pause(_) => icons::PAUSE,
                Hit::Resume(_) => icons::PLAY,
                Hit::Open(_) => icons::OPEN_EXTERNAL,
                Hit::Reveal(_) | Hit::Folder => icons::FOLDER,
                Hit::Retry(_) => icons::RELOAD,
                _ => icons::LINK,
            };
            let size = self.px(16.0);
            self.fonts.draw_icon(
                scene,
                icon,
                size,
                r.x + (r.w - size) * 0.5,
                r.y + (r.h - size) * 0.5,
                color,
            );
        } else {
            let st = Style {
                color,
                ..self.label()
            };
            let text = self.fit(st, label, r.w - self.px(12.0));
            let w = self.fonts.measure(st, &text);
            self.fonts.draw(
                scene,
                st,
                r.x + (r.w - w) * 0.5,
                r.y + r.h * 0.5 + self.px(4.0),
                &text,
            );
        }
        if hot {
            self.tip = Some(crate::app::Tip {
                anchor: r,
                text: self.download_label(hit),
                since: crate::clock::now() - std::time::Duration::from_millis(800),
            });
        }
        self.download_ui.hits.push((r, hit));
    }
    pub(crate) fn draw_downloads(
        &mut self,
        scene: &mut Scene,
        r: Rect,
        scroll: f32,
        modal: bool,
    ) -> f32 {
        use nus_render::text::icons;
        let all = list();
        let rows: Vec<_> = all
            .iter()
            .filter(|d| modal || matches(d, &self.download_ui.query))
            .collect();
        let scale = self.scale;
        let px = |n: f32| n * scale;
        let pad = px(if r.w < px(500.0) { 20.0 } else { 32.0 });
        let width = (r.w - pad * 2.0).min(px(980.0)).max(1.0);
        let x = r.x + (r.w - width) * 0.5;
        let ink = self.theme.ink;
        let muted = Style {
            color: self.theme.dim,
            ..self.label()
        };
        let rule = self.theme.tint;
        scene.layer(Some(r));
        scene.rect(r, self.paper());
        let top = r.y + px(if modal { 26.0 } else { 36.0 });
        let title = Style {
            px: px(if width < px(280.0) {
                20.0
            } else if modal {
                24.0
            } else {
                28.0
            }),
            ..self.label()
        };
        self.fonts
            .draw(scene, title, x, top + px(20.0), "Downloads");
        let title_w = self.fonts.measure(title, "Downloads");
        self.fonts.draw(
            scene,
            Style {
                color: self.surface.signal,
                ..title
            },
            x + title_w,
            top + px(20.0),
            ".",
        );
        let active = all.iter().filter(|d| d.active() && !d.paused).count();
        let paused = all.iter().filter(|d| d.active() && d.paused).count();
        let summary = if paused > 0 {
            format!("{active} active · {paused} paused · Saved to Downloads")
        } else {
            format!("{active} active · Saved to Downloads")
        };
        let summary = self.fit(muted, &summary, width);
        self.fonts.draw(scene, muted, x, top + px(43.0), &summary);
        if modal {
            self.download_button(
                scene,
                Rect::new(x + width - px(28.0), top - px(5.0), px(28.0), px(28.0)),
                "",
                Hit::Close,
            );
        } else if width > px(440.0) {
            self.download_button(
                scene,
                Rect::new(x + width - px(122.0), top - px(4.0), px(122.0), px(32.0)),
                "Open folder ↗",
                Hit::Folder,
            );
        }
        let search_y = top + px(65.0);
        if !modal {
            let field = Rect::new(x, search_y, width, px(36.0));
            let focused = self.download_ui.focus == Some(Hit::Search);
            self.fonts.draw_icon(
                scene,
                icons::SEARCH,
                px(16.0),
                field.x,
                field.y + px(8.0),
                self.theme.dim,
            );
            let area = width - px(62.0);
            let query = self.download_ui.query.clone();
            let text = if query.is_empty() {
                "Find a file, site, or type"
            } else {
                &query
            };
            let st = if query.is_empty() {
                muted
            } else {
                self.label()
            };
            let visible = self.fit(st, text, area);
            if focused && self.download_ui.select_all && !query.is_empty() {
                scene.rect(
                    Rect::new(
                        x + px(26.0),
                        field.y + px(3.0),
                        self.fonts.measure(st, &visible),
                        px(24.0),
                    ),
                    self.theme.tint,
                );
            }
            self.fonts
                .draw(scene, st, x + px(26.0), field.y + px(22.0), &visible);
            if focused && !self.download_ui.select_all {
                let before = &query[..self.download_ui.cursor.min(query.len())];
                let caret = self.fonts.measure(self.label(), before).min(area);
                scene.rect(
                    Rect::new(x + px(26.0) + caret, field.y + px(6.0), px(1.0), px(18.0)),
                    self.surface.signal,
                );
            }
            scene.hline(
                x,
                field.bottom(),
                width,
                px(1.0),
                if focused { self.surface.signal } else { rule },
            );
            self.download_ui.hits.push((field, Hit::Search));
            if !query.is_empty() {
                self.download_button(
                    scene,
                    Rect::new(field.right() - px(30.0), field.y, px(30.0), px(30.0)),
                    "",
                    Hit::ClearSearch,
                );
            }
        }
        let body_y = if modal {
            top + px(64.0)
        } else {
            search_y + px(54.0)
        };
        let footer_y = (r.bottom() - px(58.0)).max(body_y);
        let body = Rect::new(x, body_y, width, (footer_y - body_y - px(12.0)).max(0.0));
        let narrow = width < px(440.0);
        let row_h = px(if narrow { 140.0 } else { 112.0 });
        let reach = (rows.len() as f32 * row_h - body.h).max(0.0);
        let scroll = scroll.clamp(0.0, reach);
        scene.layer(Some(body));
        if rows.is_empty() {
            let title = if all.is_empty() {
                "No downloads yet"
            } else {
                "No matching downloads"
            };
            self.fonts
                .draw(scene, self.label_strong(), body.x, body.y + px(34.0), title);
            let note = self.fit(
                muted,
                if all.is_empty() {
                    "Files you save will appear here."
                } else {
                    "Try another filename, site, or file type."
                },
                body.w,
            );
            self.fonts
                .draw(scene, muted, body.x, body.y + px(60.0), &note);
        }
        for (i, d) in rows.iter().enumerate() {
            let y = (body.y + i as f32 * row_h - scroll).round();
            if y + row_h < body.y || y > body.bottom() {
                continue;
            }
            scene.hline(body.x, y, body.w, px(1.0), rule);
            let icon = if d.done {
                icons::CHECK
            } else if d.cancelled || d.interrupted {
                icons::WARNING
            } else {
                icons::DOWNLOAD
            };
            self.fonts.draw_icon(
                scene,
                icon,
                px(16.0),
                x,
                y + px(19.0),
                if d.active() {
                    self.surface.signal
                } else {
                    self.theme.dim
                },
            );
            let tx = x + px(30.0);
            let controls = px(102.0);
            let tw = (width - px(30.0) - if narrow { 0.0 } else { controls + px(12.0) }).max(1.0);
            let name = self.fit(self.label_strong(), &d.name, tw);
            self.fonts
                .draw(scene, self.label_strong(), tx, y + px(30.0), &name);
            let source = crate::sites::host_of(if d.source_url.is_empty() {
                &d.url
            } else {
                &d.source_url
            });
            let source = if d.name != d.original && !d.original.is_empty() {
                format!("{source} · originally {}", d.original)
            } else {
                source
            };
            let source = self.fit(muted, &source, tw);
            self.fonts.draw(scene, muted, tx, y + px(49.0), &source);
            let status_style = Style {
                color: if d.interrupted {
                    self.surface.signal
                } else {
                    self.theme.dim
                },
                ..muted
            };
            let status = self.fit(status_style, &d.status(), tw);
            self.fonts
                .draw(scene, status_style, tx, y + px(70.0), &status);
            if d.active() {
                let bar = Rect::new(tx, y + px(84.0), tw, px(3.0));
                scene.rect(bar, rule);
                let fraction = if d.total > 0 {
                    (d.received as f32 / d.total as f32).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                scene.rect(
                    Rect::new(bar.x, bar.y, bar.w * fraction, bar.h),
                    if d.paused {
                        self.theme.dim
                    } else {
                        self.surface.signal
                    },
                );
            }
            let mut actions = Vec::new();
            if d.active() {
                actions.push(if d.paused {
                    Hit::Resume(d.key)
                } else {
                    Hit::Pause(d.key)
                });
                actions.push(Hit::Cancel(d.key));
            } else if d.done {
                actions.push(Hit::Open(d.key));
                actions.push(Hit::Reveal(d.key));
            } else if !d.url.is_empty() {
                actions.push(Hit::Retry(d.key));
            }
            if !d.url.is_empty() {
                actions.push(Hit::Source(d.key));
            }
            let bx = x + width - px(34.0) * actions.len() as f32;
            let by = y + px(if narrow { 96.0 } else { 33.0 });
            for (j, hit) in actions.into_iter().enumerate() {
                let b = Rect::new(bx + px(j as f32 * 34.0), by, px(32.0), px(32.0));
                if b.y >= body.y && b.bottom() <= body.bottom() {
                    self.download_button(scene, b, "", hit);
                }
            }
            if Rect::new(tx, y + px(10.0), tw, px(65.0)).contains(self.mouse.0, self.mouse.1) {
                self.tip = Some(crate::app::Tip {
                    anchor: Rect::new(tx, y + px(10.0), tw, px(65.0)),
                    text: format!("{}\n{}\n{}", d.name, d.status(), d.path),
                    since: crate::clock::now() - std::time::Duration::from_millis(800),
                });
            }
        }
        scene.layer(Some(r));
        if reach > 0.0 && body.h > 0.0 {
            let h = (body.h * body.h / (body.h + reach))
                .max(px(24.0))
                .min(body.h);
            let y = body.y + (body.h - h) * scroll / reach;
            scene.rect(
                Rect::new(x + width + px(9.0), y, px(3.0), h),
                self.theme.dim,
            );
        }
        scene.hline(x, footer_y, width, px(1.0), rule);
        let footer_h = px(32.0);
        let right_w = px(if modal { 144.0 } else { 134.0 }).min(width * 0.6);
        let right = Rect::new(x + width - right_w, footer_y + px(10.0), right_w, footer_h);
        if modal {
            self.download_button(
                scene,
                right,
                if width < px(300.0) {
                    "View all ↗"
                } else {
                    "All downloads ↗"
                },
                Hit::Page,
            );
        } else if all.iter().any(|d| !d.active()) {
            self.download_button(scene, right, "Clear finished", Hit::Clear);
        }
        if width > px(520.0) {
            let note = if modal {
                "Your files stay here when tabs close"
            } else {
                "Clear history keeps your files"
            };
            self.fonts.draw(scene, muted, x, footer_y + px(31.0), note);
        } else {
            self.download_button(
                scene,
                Rect::new(
                    x,
                    footer_y + px(10.0),
                    px(102.0).min(width - right_w),
                    footer_h,
                ),
                if width < px(300.0) {
                    "Folder"
                } else {
                    "Open folder"
                },
                Hit::Folder,
            );
        }
        scene.layer(None);
        reach
    }
    pub(crate) fn draw_download_overlay(&mut self, scene: &mut Scene) {
        if self.dl_menu {
            self.tip = None;
            let full = Rect::new(
                0.0,
                0.0,
                self.target.size.0 as f32,
                self.target.size.1 as f32,
            );
            let w = self.px(640.0).min(full.w - self.px(24.0));
            let h = self
                .px(160.0
                    + list().len().clamp(1, 3) as f32
                        * if w < self.px(480.0) { 140.0 } else { 112.0 })
                .min(full.h - self.px(48.0));
            let r = Rect::new(
                ((full.w - w) * 0.5).round(),
                ((full.h - h) * 0.5).round(),
                w,
                h,
            );
            scene.layer(None);
            scene.rect(full, crate::app::fade(self.theme.ink, 0.22));
            scene.rect(
                Rect::new(r.x + self.px(6.0), r.y + self.px(6.0), r.w, r.h),
                crate::app::fade(self.theme.ink, 0.16),
            );
            self.download_ui.hits.clear();
            self.download_ui.rect = Some(r);
            self.download_ui.reach = self.draw_downloads(scene, r, self.download_ui.scroll, true);
            let (scroll, reach) = (self.download_ui.scroll, self.download_ui.reach);
            let moving = self.gliding(crate::scrolling::Glider::DownloadsMenu);
            self.draw_thumb(scene, r, scroll, reach + r.h, moving);
            scene.outline(r, self.px(1.0), self.theme.ink);
            return;
        }
        self.download_ui.rect = None;
        let hot = self
            .download_ui
            .anchor
            .filter(|r| r.contains(self.mouse.0, self.mouse.1));
        if let Some(anchor) = hot {
            let since = *self
                .download_ui
                .hover_since
                .get_or_insert_with(std::time::Instant::now);
            if crate::clock::since(since).as_millis() < 250 {
                self.dirty = true;
                return;
            }
            let rows = list();
            let active: Vec<_> = rows.iter().filter(|d| d.active()).collect();
            let shown: Vec<_> = if active.is_empty() {
                rows.iter().take(2).collect()
            } else {
                active.into_iter().take(3).collect()
            };
            let w = self
                .px(340.0)
                .min(self.target.size.0 as f32 - self.px(16.0));
            let h = self.px(44.0 + shown.len() as f32 * 44.0);
            let x = (anchor.x - w * 0.5).clamp(
                self.px(8.0),
                (self.target.size.0 as f32 - w - self.px(8.0)).max(self.px(8.0)),
            );
            let y = (anchor.y - h - self.px(8.0)).max(self.px(8.0));
            let r = Rect::new(x, y, w, h);
            scene.layer(None);
            scene.rect(r, self.paper());
            scene.outline(r, self.px(1.0), self.theme.ink);
            let st = self.label();
            let title = if rows.is_empty() {
                "Downloads · no files yet".into()
            } else {
                format!(
                    "Downloads · {} in progress",
                    rows.iter().filter(|d| d.active()).count()
                )
            };
            self.fonts.draw(
                scene,
                self.label_strong(),
                x + self.px(12.0),
                y + self.px(23.0),
                &title,
            );
            for (i, d) in shown.iter().enumerate() {
                let y = y + self.px(46.0 + i as f32 * 44.0);
                let name = self.fit(st, &d.name, w - self.px(24.0));
                self.fonts.draw(scene, st, x + self.px(12.0), y, &name);
                let note = self.fit(st, &d.status(), w - self.px(24.0));
                self.fonts.draw(
                    scene,
                    Style {
                        color: self.theme.dim,
                        ..st
                    },
                    x + self.px(12.0),
                    y + self.px(17.0),
                    &note,
                );
            }
        } else {
            self.download_ui.hover_since = None;
        }
    }
}
fn open_file(path: &Path) {
    if !path.is_file() {
        return;
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(path).spawn();
    }
    #[cfg(windows)]
    {
        // ShellExecute avoids cmd.exe parsing of downloaded filenames.
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                std::ptr::null(),
                path.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                1,
            );
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(path).spawn();
    }
}
fn reveal(path: &Path, file: bool) {
    #[cfg(target_os = "macos")]
    {
        let mut c = std::process::Command::new("open");
        if file {
            c.arg("-R");
        }
        let _ = c.arg(path).spawn();
    }
    #[cfg(windows)]
    {
        let mut c = std::process::Command::new("explorer");
        if file {
            c.arg(format!("/select,{}", path.display()));
        } else {
            c.arg(path);
        }
        let _ = c.spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let p = if file {
            path.parent().unwrap_or(path)
        } else {
            path
        };
        let _ = std::process::Command::new("xdg-open").arg(p).spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn search_handles_names_sites_types_and_state() {
        let d = Download {
            name: "Annual résumé.pdf".into(),
            original: "document.pdf".into(),
            url: "https://cdn.example/file".into(),
            source_url: "https://research.example/reports".into(),
            done: true,
            received: 4096,
            ..Default::default()
        };
        for q in ["résumé", "PDF research", "complete", "document"] {
            assert!(matches(&d, q));
        }
        assert!(!matches(&d, "report.doc"));
    }
    #[test]
    fn search_editing_preserves_unicode_boundaries() {
        let mut ui = Ui::default();
        ui.insert("café🙂");
        ui.backspace();
        assert_eq!(ui.query, "café");
        ui.backspace();
        assert_eq!(ui.query, "caf");
        ui.cursor = 0;
        ui.insert("文");
        assert_eq!(ui.query, "文caf");
        ui.select_all = true;
        ui.insert("report\n.pdf");
        assert_eq!(ui.query, "report.pdf");
        assert_eq!(ui.cursor, 10);
    }
    #[test]
    fn old_download_history_defaults_the_source() {
        let d: Download = serde_json::from_str(r#"{"name":"report.pdf","done":true}"#).unwrap();
        assert!(d.source_url.is_empty());
        assert!(d.done);
    }
    #[test]
    fn renaming_is_opt_in_and_preserves_extensions() {
        assert_eq!(
            filename("Report Final.pdf", "New title", Rename::Off),
            "Report Final.pdf"
        );
        assert_eq!(
            filename("archive.tar.gz", "Project / Release", Rename::All),
            "Project - Release.tar.gz"
        );
        assert_eq!(
            filename("d.pdf", "Readable report.pdf", Rename::All),
            "Readable report.pdf"
        );
        assert_eq!(filename("d.pdf", "Downloads", Rename::All), "d.pdf");
    }
    #[test]
    fn selective_preserves_technical_identity() {
        for n in [
            "app.dmg",
            "source.tar.gz",
            "package.json",
            "app-1.2.3.pdf",
            "0123456789abcdef.pdf",
            "signed-copy.pdf",
            "id.csv",
        ] {
            assert_eq!(filename(n, "Readable title", Rename::Selective), n);
        }
        assert_eq!(
            filename("document.pdf", "Annual report", Rename::Selective),
            "Annual report.pdf"
        );
    }
    #[test]
    fn names_cannot_escape_the_download_folder() {
        for n in [
            "../../bad.exe",
            "C:\\folder\\file.pdf",
            "...",
            "foo:bar?.txt",
            "CON.txt",
        ] {
            let v = filename(n, "title", Rename::Off);
            assert!(!v.contains('/') && !v.contains('\\') && !v.contains(':'));
            assert!(!v.starts_with('.'));
        }
        assert_eq!(safe_name("CON.txt"), "_CON.txt");
    }
    #[test]
    fn pending_downloads_reserve_their_filename() {
        let dir = Path::new("/tmp/nus-collision-test");
        let d = Download {
            path: dir.join("report.pdf").to_string_lossy().into(),
            live: true,
            ..Default::default()
        };
        assert_eq!(
            available_path(dir, "report.pdf", &[d]),
            dir.join("report (2).pdf")
        );
    }
}
