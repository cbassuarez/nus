//! Preferences on disk: profile/settings.json. Saved after every change
//! from the settings tab; loaded at launch. (v1: ~/.config/nus, and
//! init.luau can set the same fields.)

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
static REVISION: AtomicU64 = AtomicU64::new(0);

use crate::anim::{LoadBar, Motion};
use crate::app::App;
use crate::settings::Behavior;
use crate::surface::{SidebarRules, Surface};

/// The shape of settings.json this nus writes. A file from an older nus
/// has a lower number (0 before there was one) and is brought up in
/// `migrate`; one from a newer nus is read for what this build knows
/// (store.rs) and the rest is kept through the save.
pub const SCHEMA: u32 = 2;

/// Keys the last load had to leave out, for a word to the user once.
static SALVAGED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

#[derive(Serialize, Deserialize, Default)]
pub struct Prefs {
    #[serde(default)]
    pub schema: u32,
    pub surface: Option<Surface>,
    pub sidebar: Option<SidebarRules>,
    pub motion: Option<Motion>,
    pub load_bar: Option<LoadBar>,
    pub behavior: Option<Behavior>,
    pub sidebar_pinned: Option<bool>,
    pub pinned_tabs: Option<Vec<crate::pins::Pin>>,
    pub sound: Option<crate::sound::SoundPrefs>,
    pub window_rect: Option<(i32, i32, u32, u32)>,
    pub theme: Option<crate::theme_edit::ThemeEdit>,
    pub cursor: Option<crate::settings::CursorPrefs>,
    pub header: Option<crate::settings::HeaderPrefs>,
    pub window_name: Option<String>,
    pub tab_colours: Option<String>,
    pub theme_mode: Option<String>,
    pub preset_name: Option<String>,
}

fn path() -> std::path::PathBuf {
    std::env::current_dir().unwrap_or_default().join("profile").join("settings.json")
}

impl Prefs {
    /// The file, whole or salvaged (store.rs), brought up to this schema.
    pub fn load() -> Prefs {
        let read = crate::store::read_json::<Prefs>(&path());
        if !read.dropped.is_empty() {
            *SALVAGED.lock().unwrap() = read.dropped;
        }
        let mut p = read.value;
        migrate(&mut p);
        if crate::private::enabled() { crate::private::constrain(&mut p); }
        p
    }
}

/// Older files, brought up. Each step is idempotent; the number only
/// says which steps have run.
fn migrate(p: &mut Prefs) {
    if p.schema < 1 {
        // Two start pages the page no longer offers, from before it had
        // these four (also applied late in apply_prefs, for the window).
        if let Some(b) = p.behavior.as_mut() {
            match b.then {
                crate::settings::Then::Shell => {
                    b.then = crate::settings::Then::Prompt;
                    b.lead = crate::settings::Lead::Terminal;
                }
                crate::settings::Then::Restore => {
                    b.then = crate::settings::Then::Prompt;
                    b.remember = true;
                    if b.atlas == crate::settings::AtlasMode::Planet {
                        b.atlas = crate::settings::AtlasMode::AtLaunch;
                    }
                }
                _ => {}
            }
        }
    }
    // 2: white paper; a surface base of the old paper swatch means "none".
    if p.schema < 2 {
        if let Some(s) = p.surface.as_mut() {
            if s.base.is_some_and(|b| (b[0] - 0.957).abs() < 0.01 && (b[1] - 0.945).abs() < 0.01 && (b[2] - 0.918).abs() < 0.01) {
                s.base = None;
                s.tint = 0.0;
            }
        }
    }
    p.schema = SCHEMA;
}

/// The settings Chromium takes on its command line, stored where the
/// browser process reads them before it starts (browser.rs): smooth
/// scrolling and the scrollbars. Called from main, before CEF.
pub fn apply_start_switches() {
    use std::sync::atomic::Ordering::Relaxed;
    let p = Prefs::load();
    let b = p.behavior.unwrap_or_default();
    crate::browser::SMOOTH_SCROLL.store(b.page_smooth_scroll, Relaxed);
    crate::browser::SCROLLBARS.store(b.scrollbars as u8, Relaxed);
}

impl App {
    pub(crate) fn import_settings(&mut self, source: &std::path::Path) -> std::io::Result<()> {
        let bytes = std::fs::read(source)?;
        let mut prefs: Prefs = serde_json::from_slice(&bytes).map_err(std::io::Error::other)?;
        if prefs.schema > SCHEMA {
            return Err(std::io::Error::other("These settings require a newer nus version; nothing was imported"));
        }
        migrate(&mut prefs);
        // Commit before changing live state. The ordinary save merges only
        // changed fields, so applying first would reset its comparison baseline.
        crate::store::write_json(&path(), &prefs)?;
        self.apply_prefs(prefs);
        self.rebuild_theme();
        self.pointer_request = Some(self.cursor.pointer);
        self.hatch_settings_changed();
        self.save_prefs();
        self.layout();
        self.dirty = true;
        Ok(())
    }

    pub(crate) fn apply_prefs(&mut self, p: Prefs) {
        crate::downloads::init();
        if !crate::private::enabled() { if let Some(pins)=p.pinned_tabs {self.apply_pins(pins);} }
        crate::browser::BLOCKING.store(p.behavior.as_ref().map(|b| b.block_content).unwrap_or(true), std::sync::atomic::Ordering::Relaxed);
        crate::browser::SMOOTH_SCROLL.store(p.behavior.as_ref().map(|b| b.page_smooth_scroll).unwrap_or(true), std::sync::atomic::Ordering::Relaxed);
        if let Some(b) = p.behavior.as_ref() {
            self.apply_behavior_statics(b);
        }
        if let Some(s) = p.surface {
            self.surface = s;
        }
        if let Some(s) = p.sidebar {
            self.sidebar_rules = s;
            self.sync_live_folders();
        }
        if let Some(m) = p.motion {
            self.motion = m;
        }
        if let Some(l) = p.load_bar {
            self.load_bar = l;
        }
        if let Some(b) = p.behavior {
            self.behavior = b;
            // Two start pages the page no longer offers, from before it
            // had these four. A shell at launch is the prompt with a
            // terminal lead; a restored session is ON QUIT, KEEP — so
            // keep what each was for, and let the picker tell the truth.
            match self.behavior.then {
                crate::settings::Then::Shell => {
                    self.behavior.then = crate::settings::Then::Prompt;
                    self.behavior.lead = crate::settings::Lead::Terminal;
                }
                crate::settings::Then::Restore => {
                    self.behavior.then = crate::settings::Then::Prompt;
                    self.behavior.remember = true;
                    // The session waits in the atlas now, so show it.
                    if self.behavior.atlas == crate::settings::AtlasMode::Planet {
                        self.behavior.atlas = crate::settings::AtlasMode::AtLaunch;
                    }
                }
                _ => {}
            }
        }
        crate::downloads::set_rename(self.behavior.download_rename);
        // NUS_SHELL=<profile name> picks the shell new tabs run (a test hook).
        if let Ok(name) = std::env::var("NUS_SHELL") {
            if let Some(i) = self.profiles.iter().position(|p| p.name.eq_ignore_ascii_case(&name)) {
                self.behavior.default_profile = i;
            }
        }
        if let Some(p) = p.sidebar_pinned {
            self.sidebar = p;
        }
        if let Some(s) = p.sound {
            self.sound.prefs = s;
        }
        self.window_rect = p.window_rect;
        if let Some(t) = p.theme {
            self.theme_edit = t;
        }
        if let Some(c) = p.cursor {
            self.cursor = c;
        }
        if let Some(h) = p.header {
            self.header = h;
        }
        if let Some(t) = p.tab_colours {
            self.tab_colours = t;
        }
        if self.ordinal == 0 {
            self.window_named = p.window_name;
        }
        if let Some(name) = p.preset_name { self.preset_name = name; }
        if !self.behavior.follow_os_theme && std::env::var_os("NUS_MODE").is_none() {
            if let Some(mode) = p.theme_mode {
                self.set_mode(if mode == "paper" { nus_render::Mode::Paper } else { nus_render::Mode::Ink });
            }
        }
        self.apply_fonts();
        *self.prefs_baseline.borrow_mut() = self.prefs_snapshot();
        self.prefs_revision.set(REVISION.load(Ordering::Relaxed));
        // A file this nus could not read whole: say what was set aside,
        // once, and where the original is.
        let dropped = std::mem::take(&mut *SALVAGED.lock().unwrap());
        if !dropped.is_empty() {
            let shown: Vec<&str> = dropped.iter().map(|s| s.as_str()).take(4).collect();
            self.notice(&format!("settings · {} kept as {} · the original is settings.unread", if dropped.len() > 4 { format!("{} and {} more", shown.join(", "), dropped.len() - 4) } else { shown.join(", ") }, "the default"));
        }
    }

    /// The behaviour the browser glue reads without an `App` in hand:
    /// downloads, the privacy signal, the default zoom. Once at load and
    /// again whenever one of them changes.
    pub(crate) fn apply_behavior_statics(&self, b: &Behavior) {
        crate::app_icon::select(b.app_icon);
        use std::sync::atomic::Ordering::Relaxed;
        crate::browser::set_downloads_dir(&b.download_dir);
        crate::browser::DOWNLOAD_ASK.store(b.download_ask, Relaxed);
        crate::browser::PRIVACY_SIGNAL.store(b.privacy_signal, Relaxed);
        crate::sites::DEFAULT_ZOOM.store(b.page_zoom.clamp(25, 500) as u32, Relaxed);
    }

    fn prefs_snapshot(&self) -> serde_json::Value {
        let p = Prefs {
            schema: SCHEMA,
            surface: Some(self.surface.clone()),
            sidebar: Some(self.sidebar_rules.clone()),
            motion: Some(self.motion.clone()),
            load_bar: Some(self.load_bar.clone()),
            behavior: Some(self.behavior.clone()),
            sidebar_pinned: Some(self.sidebar),
            pinned_tabs: Some(self.pins.items.clone()),
            sound: Some(self.sound.prefs.clone()),
            window_rect: self.window_rect,
            theme: Some(self.theme_edit.clone()),
            cursor: Some(self.cursor.clone()),
            header: Some(self.header.clone()),
            tab_colours: Some(self.tab_colours.clone()),
            theme_mode: Some(if self.theme.mode == nus_render::Mode::Ink { "ink" } else { "paper" }.into()),
            preset_name: Some(self.preset_name.clone()),
            window_name: if self.ordinal == 0 { self.window_named.clone() } else { None },
        };
        serde_json::to_value(p).unwrap_or_default()
    }

    pub(crate) fn save_prefs(&self) {
        if crate::private::enabled() { return; }
        let current = self.prefs_snapshot();
        let mut latest = std::fs::read(path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_else(|| current.clone());
        merge_changes(&self.prefs_baseline.borrow(), &current, &mut latest);
        // Secondary windows do not own the main window's name or geometry.
        if self.ordinal != 0 {
            if let Ok(p) = std::fs::read(path()).and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).map_err(std::io::Error::other)) {
                for key in ["window_name", "window_rect"] { latest[key] = p[key].clone(); }
            }
        }
        let destination = path();
        let result = (|| -> std::io::Result<()> {
            std::fs::create_dir_all(destination.parent().unwrap())?;
            let temporary = destination.with_extension("json.tmp");
            std::fs::write(&temporary, serde_json::to_vec_pretty(&latest)?)?;
            std::fs::rename(temporary, destination)
        })();
        if let Err(error) = result { tracing::error!("Could not save settings: {error}"); return; }
        *self.prefs_baseline.borrow_mut() = current;
        self.prefs_revision.set(REVISION.fetch_add(1, Ordering::Relaxed) + 1);
    }

    /// All windows see a setting change before their next input frame.
    pub(crate) fn refresh_shared_prefs(&mut self) {
        if self.prefs_revision.get() == REVISION.load(Ordering::Relaxed) { return; }
        let rect = self.window_rect;
        let name = self.window_named.clone();
        let replay = self.behavior.replay;
        let mut prefs = Prefs::load();
        prefs.window_rect = rect;
        prefs.window_name = name;
        self.apply_prefs(prefs);
        if replay != self.behavior.replay {
            self.recorder = self.behavior.replay.days().and_then(crate::replay::Recorder::new);
        }
        self.rebuild_theme();
        self.pointer_request = Some(self.cursor.pointer);
        self.hatch_settings_changed();
        self.layout();
        self.dirty = true;
    }
}

/// A stale window may save its geometry after another window changes a
/// setting. Only apply the fields this window actually changed.
fn merge_changes(before: &serde_json::Value, after: &serde_json::Value, latest: &mut serde_json::Value) {
    if before == after { return; }
    if let (Some(before), Some(after), Some(latest)) = (before.as_object(), after.as_object(), latest.as_object_mut()) {
        for (key, value) in after {
            merge_changes(before.get(key).unwrap_or(&serde_json::Value::Null), value, latest.entry(key).or_insert_with(|| value.clone()));
        }
    } else { *latest = after.clone(); }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn a_stale_window_does_not_undo_settings() {
        let before = json!({"behavior":{"phone":false,"links":"Stack"},"window_rect":[0,0,800,600]});
        let after = json!({"behavior":{"phone":false,"links":"Stack"},"window_rect":[0,0,900,700]});
        let mut latest = json!({"behavior":{"phone":true,"links":"Split"},"window_rect":[0,0,800,600]});
        merge_changes(&before,&after,&mut latest);
        assert_eq!(latest["behavior"],json!({"phone":true,"links":"Split"}));
        assert_eq!(latest["window_rect"],json!([0,0,900,700]));
    }
}
