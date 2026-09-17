//! Preferences on disk: profile/settings.json. Saved after every change
//! from the settings tab; loaded at launch. (v1: ~/.config/nus, and
//! init.luau can set the same fields.)

use serde::{Deserialize, Serialize};

use crate::anim::{LoadBar, Motion};
use crate::app::App;
use crate::settings::Behavior;
use crate::surface::{SidebarRules, Surface};

#[derive(Serialize, Deserialize, Default)]
pub struct Prefs {
    pub surface: Option<Surface>,
    pub sidebar: Option<SidebarRules>,
    pub motion: Option<Motion>,
    pub load_bar: Option<LoadBar>,
    pub behavior: Option<Behavior>,
    pub sidebar_pinned: Option<bool>,
    pub sound: Option<crate::sound::SoundPrefs>,
    pub window_rect: Option<(i32, i32, u32, u32)>,
    pub theme: Option<crate::theme_edit::ThemeEdit>,
    pub cursor: Option<crate::settings::CursorPrefs>,
    pub header: Option<crate::settings::HeaderPrefs>,
    pub window_name: Option<String>,
}

fn path() -> std::path::PathBuf {
    std::env::current_dir().unwrap_or_default().join("profile").join("settings.json")
}

impl Prefs {
    pub fn load() -> Prefs {
        std::fs::read_to_string(path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }
}

impl App {
    pub(crate) fn apply_prefs(&mut self, p: Prefs) {
        crate::browser::BLOCKING.store(p.behavior.as_ref().map(|b| b.block_content).unwrap_or(true), std::sync::atomic::Ordering::Relaxed);
        if let Some(s) = p.surface {
            self.surface = s;
        }
        if let Some(s) = p.sidebar {
            self.sidebar_rules = s;
        }
        if let Some(m) = p.motion {
            self.motion = m;
        }
        if let Some(l) = p.load_bar {
            self.load_bar = l;
        }
        if let Some(b) = p.behavior {
            self.behavior = b;
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
        if self.ordinal == 0 {
            self.window_named = p.window_name;
        }
    }

    pub(crate) fn save_prefs(&self) {
        let p = Prefs {
            surface: Some(self.surface.clone()),
            sidebar: Some(self.sidebar_rules.clone()),
            motion: Some(self.motion.clone()),
            load_bar: Some(self.load_bar.clone()),
            behavior: Some(self.behavior.clone()),
            sidebar_pinned: Some(self.sidebar),
            sound: Some(self.sound.prefs.clone()),
            window_rect: self.window_rect,
            theme: Some(self.theme_edit.clone()),
            cursor: Some(self.cursor.clone()),
            header: Some(self.header.clone()),
            window_name: if self.ordinal == 0 { self.window_named.clone() } else { None },
        };
        let _ = std::fs::create_dir_all(path().parent().unwrap());
        let _ = std::fs::write(path(), serde_json::to_string_pretty(&p).unwrap_or_default());
    }
}
