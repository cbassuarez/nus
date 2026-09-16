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
    }

    pub(crate) fn save_prefs(&self) {
        let p = Prefs {
            surface: Some(self.surface.clone()),
            sidebar: Some(self.sidebar_rules.clone()),
            motion: Some(self.motion.clone()),
            load_bar: Some(self.load_bar.clone()),
            behavior: Some(self.behavior.clone()),
            sidebar_pinned: Some(self.sidebar),
        };
        let _ = std::fs::create_dir_all(path().parent().unwrap());
        let _ = std::fs::write(path(), serde_json::to_string_pretty(&p).unwrap_or_default());
    }
}
