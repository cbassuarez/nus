//! You, on this machine. A name, a face (the initial in the signal, an
//! emoji, or profile/avatar.png), the day it began, and this device's
//! name for sync. `profile/me.json` — a file in a folder, and that is the
//! whole account: no server behind it, nothing counted, nothing sent.
//!
//! The card rises from the avatar in the footer. The first time it walks
//! you through — hello, name, face, device, and how the profile lives —
//! and says, in so many words, that this stays here. After that it is the
//! profile at a glance: the face, the name, a day-count badge, the rows,
//! and MORE for the full page under settings.
//!
//! How it lives, three ways: here, a folder on this machine and nothing
//! leaves; carried by a folder your OS already syncs; or carried by a
//! private repo on a forge — GitHub signed into from the card or with a
//! token, Forgejo, Gitea or GitLab with a token — that nus makes for you.
//! The last two share a key you copy; only ciphertext goes anywhere.

use std::path::PathBuf;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use nus_render::text::{icons, Style};
use nus_render::theme::metric as m;
use nus_render::{Rect, Scene, Theme};
use serde::{Deserialize, Serialize};
use winit::event::{ElementState, MouseButton};
use winit::keyboard::{Key as WKey, NamedKey};

use crate::anim::{base, Anim};
use crate::app::{fade, hover_key, App, Caps, Hover, Tip};

// ── The file ─────────────────────────────────────────────────────────────

/// What stands in for you in the footer.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Face {
    /// The initial of the name, in the signal.
    #[default]
    Initial,
    /// An emoji (or any short string).
    Emoji(String),
    /// profile/avatar.png.
    Picture,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Me {
    pub name: String,
    #[serde(default)]
    pub face: Face,
    /// YYYY-MM-DD, the day the profile began.
    pub created: String,
}

fn profile_dir() -> PathBuf {
    std::env::current_dir().unwrap_or_default().join("profile")
}

fn me_path() -> PathBuf {
    profile_dir().join("me.json")
}

fn device_path() -> PathBuf {
    profile_dir().join("sync").join("device")
}

impl Me {
    pub fn load() -> Option<Me> {
        let s = std::fs::read_to_string(me_path()).ok()?;
        serde_json::from_str(&s).ok()
    }

    pub fn save(&self) {
        let _ = std::fs::create_dir_all(profile_dir());
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(me_path(), s);
        }
    }

    pub fn forget() {
        let _ = std::fs::remove_file(me_path());
    }

    /// Days since `created`, today counting as day 1.
    pub fn days(&self) -> u64 {
        let then = parse_date(&self.created).unwrap_or_else(today_days);
        (today_days() - then).max(0) as u64 + 1
    }

    /// The badge's word: "day 1", then "12 days".
    pub fn day_word(&self) -> String {
        match self.days() {
            1 => "day 1".into(),
            n => format!("{n} days"),
        }
    }

    pub fn initial(&self) -> String {
        self.name.trim().chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "?".into())
    }
}

/// This device's name for sync: profile/sync/device, else the hostname.
/// It is the one thing about you that must not sync.
pub fn device() -> String {
    std::fs::read_to_string(device_path()).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).unwrap_or_else(nus_sync::device_name)
}

pub fn set_device(name: &str) {
    let p = device_path();
    let _ = std::fs::create_dir_all(p.parent().unwrap());
    let _ = std::fs::write(p, name.trim());
}

/// The OS's name for you, as a first guess.
pub fn os_user() -> String {
    std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_else(|_| "you".into())
}

// Civil dates without a crate: days since 1970-01-01 (Howard Hinnant).
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn today_days() -> i64 {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64;
    secs.div_euclid(86_400)
}

pub fn today() -> String {
    let (y, m, d) = civil_from_days(today_days());
    format!("{y:04}-{m:02}-{d:02}")
}

fn parse_date(s: &str) -> Option<i64> {
    let mut it = s.trim().split('-');
    let y: i64 = it.next()?.parse().ok()?;
    let m: u32 = it.next()?.parse().ok()?;
    let d: u32 = it.next()?.parse().ok()?;
    (1..=12).contains(&m).then(|| days_from_civil(y, m, d.clamp(1, 31)))
}

/// When this profile really began, for someone who was here before the
/// card: the oldest of the profile's own files, else today.
pub fn first_seen() -> String {
    let mut oldest: Option<i64> = None;
    for f in ["onboarded", "settings.json", "rules.luau", "session.json"] {
        if let Ok(md) = std::fs::metadata(profile_dir().join(f)) {
            let at = md.created().or_else(|_| md.modified()).ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| (d.as_secs() as i64).div_euclid(86_400));
            if let Some(a) = at {
                oldest = Some(oldest.map_or(a, |o: i64| o.min(a)));
            }
        }
    }
    let (y, m, d) = civil_from_days(oldest.unwrap_or_else(today_days));
    format!("{y:04}-{m:02}-{d:02}")
}

// ── The card ─────────────────────────────────────────────────────────────

/// The steps of the first walk, and of an edit (which returns to the view).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Step {
    Hello,
    Name,
    Face,
    Device,
    /// How the profile lives: here, a folder you sync, a private repo.
    Sync,
    /// The folder your OS carries.
    Folder,
    /// Which forge, and how to sign in.
    Forge,
    /// A token, pasted.
    ForgeToken,
    /// The worker's phase: a code to enter, verifying, making, done.
    ForgeWait,
    /// The key: made here, or joined with the word from another device.
    Key,
    Done,
}

/// The three ways the profile can live.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Way {
    #[default]
    Here,
    Folder,
    Forge,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CardHit {
    Next,
    Back,
    NotNow,
    Close,
    More,
    Face(u8),
    SetupSync,
    Edit(Step),
    Folder,
    Badge,
    /// One of the three ways.
    Way(u8),
    /// One of the forges.
    ForgeKind(u8),
    /// GitHub, from the card (the device flow).
    ForgeSignIn,
    /// Any forge, with a token.
    ForgeToken,
    /// The page the device code goes on, in a tab.
    OpenCodePage,
    CopyCode,
    /// The key: 0 make one here, 1 join with the word.
    KeyMode(u8),
    CopyKey,
    /// Undo the forge: the token and the repo's name forgotten.
    ForgeForget,
}

pub struct MeCard {
    pub open: bool,
    /// None: the view. Some: a step of the walk, or an edit of one field.
    pub step: Option<Step>,
    /// True while a single field is being edited from the view.
    pub editing: bool,
    pub input: String,
    /// The face picked during the walk (the input holds the emoji).
    pub face: Face,
    pub rise: Anim,
    pub rect: Rect,
    pub hits: Vec<(Rect, CardHit)>,
    /// The way picked, the forge picked, the key's mode (0 make, 1 join).
    pub way: Way,
    pub forge: crate::forge::Kind,
    pub key_mode: u8,
    /// The forge worker, while it runs; the word the key was made as.
    pub flow: Option<crate::forge::Flow>,
    pub made_key: String,
    /// The host typed for a forge that is not GitHub.
    pub host: String,
}

impl Default for MeCard {
    fn default() -> Self {
        MeCard {
            open: false,
            step: None,
            editing: false,
            input: String::new(),
            face: Face::Initial,
            rise: Anim::at(0.0),
            rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            hits: Vec::new(),
            way: Way::Here,
            forge: crate::forge::Kind::GitHub,
            key_mode: 0,
            flow: None,
            made_key: String::new(),
            host: String::new(),
        }
    }
}

impl App {
    /// The footer's avatar: the card, at the view or at the start of the walk.
    pub(crate) fn open_me_card(&mut self) {
        let c = &mut self.me_card;
        c.open = true;
        c.editing = false;
        c.input.clear();
        c.step = if self.me.is_none() { Some(Step::Hello) } else { None };
        c.rise.replay(0.0, 1.0, self.motion.dur(base::PALETTE) * 1.6);
        self.palette = None;
        self.dirty = true;
    }

    /// One field from the view (or the settings page).
    pub(crate) fn open_me_card_at(&mut self, step: Step) {
        self.open_me_card();
        let seed = match step {
            Step::Name => self.me.as_ref().map(|m| m.name.clone()).unwrap_or_else(os_user),
            Step::Device => device(),
            Step::Face => match self.me.as_ref().map(|m| &m.face) {
                Some(Face::Emoji(e)) => e.clone(),
                _ => String::new(),
            },
            Step::Folder => self.behavior.sync_folder.clone(),
            _ => String::new(),
        };
        let c = &mut self.me_card;
        c.step = Some(step);
        // The sync steps are a walk of their own, never a one-field edit.
        c.editing = self.me.is_some() && !matches!(step, Step::Sync | Step::Folder | Step::Forge | Step::ForgeToken | Step::ForgeWait | Step::Key);
        c.input = seed;
        c.face = self.me.as_ref().map(|m| m.face.clone()).unwrap_or_default();
        c.way = if !self.behavior.sync_git.is_empty() { Way::Forge } else if !self.behavior.sync_folder.is_empty() { Way::Folder } else { Way::Here };
        c.forge = crate::forge::load().map(|f| f.kind).unwrap_or_default();
        c.host = c.forge.default_host().to_string();
        c.key_mode = 0;
        c.made_key.clear();
        if let Some(f) = c.flow.take() {
            f.cancel();
        }
    }

    pub(crate) fn close_me_card(&mut self) {
        self.me_card.open = false;
        self.me_card.hits.clear();
        if let Some(f) = self.me_card.flow.take() {
            f.cancel();
        }
        self.dirty = true;
    }

    /// The name the rest of the app uses.
    pub(crate) fn me_name(&self) -> String {
        self.me.as_ref().map(|m| m.name.clone()).filter(|n| !n.trim().is_empty()).unwrap_or_else(os_user)
    }

    /// For the shot driver.
    pub(crate) fn me_next_pub(&mut self) {
        self.me_next();
    }
    pub(crate) fn me_back_pub(&mut self) {
        self.me_back();
    }

    /// Enter, or NEXT: commit this step and go on.
    fn me_next(&mut self) {
        let Some(step) = self.me_card.step else { return };
        let editing = self.me_card.editing;
        let input = self.me_card.input.trim().to_string();
        match step {
            Step::Hello => {
                self.me_card.step = Some(Step::Name);
                self.me_card.input = os_user();
            }
            Step::Name => {
                if input.is_empty() {
                    return;
                }
                if editing {
                    if let Some(me) = self.me.as_mut() {
                        me.name = input;
                        me.save();
                    }
                    self.me_card.step = None;
                    self.me_card.editing = false;
                } else {
                    self.me_card.face = Face::Initial;
                    self.pending_name = input;
                    self.me_card.step = Some(Step::Face);
                    self.me_card.input.clear();
                }
            }
            Step::Face => {
                let face = match &self.me_card.face {
                    Face::Emoji(_) if input.is_empty() => Face::Initial,
                    Face::Emoji(_) => Face::Emoji(input.clone()),
                    f => f.clone(),
                };
                if editing {
                    if let Some(me) = self.me.as_mut() {
                        me.face = face;
                        me.save();
                    }
                    self.me_card.step = None;
                    self.me_card.editing = false;
                } else {
                    self.me_card.face = face;
                    self.me_card.step = Some(Step::Device);
                    self.me_card.input = device();
                }
            }
            Step::Device => {
                if !input.is_empty() {
                    set_device(&input);
                }
                if editing {
                    self.me_card.step = None;
                    self.me_card.editing = false;
                } else {
                    self.me_card.step = Some(Step::Sync);
                    self.me_card.input.clear();
                }
            }
            Step::Sync => match self.me_card.way {
                Way::Here => self.me_sync_done(),
                Way::Folder => {
                    self.me_card.step = Some(Step::Folder);
                    self.me_card.input = self.behavior.sync_folder.clone();
                }
                Way::Forge => {
                    self.me_card.step = Some(Step::Forge);
                    self.me_card.input.clear();
                }
            },
            Step::Folder => {
                if input.is_empty() {
                    return;
                }
                self.behavior.sync_folder = input;
                self.save_prefs();
                self.me_card.step = Some(Step::Key);
                self.me_card.input.clear();
            }
            Step::Forge => {
                // NEXT here means a token; GitHub's sign-in has its own button.
                self.me_card.host = if input.is_empty() { self.me_card.forge.default_host().to_string() } else { input };
                self.me_card.step = Some(Step::ForgeToken);
                self.me_card.input.clear();
            }
            Step::ForgeToken => {
                if input.is_empty() {
                    return;
                }
                let kind = self.me_card.forge;
                let host = self.me_card.host.clone();
                self.me_card.flow = Some(crate::forge::start_token(kind, &host, &input));
                self.me_card.step = Some(Step::ForgeWait);
                self.me_card.input.clear();
            }
            Step::ForgeWait => {
                // Only on to the key once the worker is done.
                if let Some(crate::forge::Phase::Done(f)) = self.me_card.flow.as_ref().map(|f| f.phase()) {
                    self.behavior.sync_git = f.clone_url.clone();
                    self.save_prefs();
                    self.me_card.flow = None;
                    self.me_card.step = Some(Step::Key);
                    self.me_card.input.clear();
                }
            }
            Step::Key => {
                if self.me_card.key_mode == 1 {
                    if nus_sync::decode_key(&input).is_none() {
                        self.notice("that is not a nus key · nus5-…");
                        return;
                    }
                    crate::syncui::write_key(&input);
                } else if crate::syncui::key().is_none() {
                    self.me_card.made_key = crate::syncui::make_key();
                    // Shown first; NEXT again goes on.
                    return;
                }
                self.me_sync_done();
                self.sync_now();
            }
            Step::Done => {
                self.me_card.step = None;
            }
        }
        self.user_name = self.me_name();
        self.dirty = true;
    }

    /// The sync walk ends: the profile is saved if it wasn't, and the card
    /// says how it lives now.
    fn me_sync_done(&mut self) {
        if self.me.is_none() {
            self.me_finish();
        } else {
            self.me_card.step = Some(Step::Done);
            self.me_card.input.clear();
        }
        self.me_card.editing = false;
    }

    /// The walk's end: the file, day 1 (or the day it really began).
    fn me_finish(&mut self) {
        let me = Me { name: std::mem::take(&mut self.pending_name), face: self.me_card.face.clone(), created: first_seen() };
        me.save();
        self.me = Some(me);
        self.user_name = self.me_name();
        self.me_card.step = Some(Step::Done);
        self.me_card.input.clear();
        self.play_event("onboarding.tick");
    }

    fn me_back(&mut self) {
        let c = &mut self.me_card;
        if c.editing {
            c.step = None;
            c.editing = false;
            return;
        }
        if let Some(f) = c.flow.take() {
            f.cancel();
        }
        c.step = match c.step {
            Some(Step::Name) => Some(Step::Hello),
            Some(Step::Face) => Some(Step::Name),
            Some(Step::Device) => Some(Step::Face),
            Some(Step::Sync) => Some(Step::Device),
            Some(Step::Folder) | Some(Step::Forge) => Some(Step::Sync),
            Some(Step::ForgeToken) | Some(Step::ForgeWait) => Some(Step::Forge),
            Some(Step::Key) => Some(if c.way == Way::Forge { Step::Forge } else { Step::Folder }),
            s => s,
        };
        c.input = match c.step {
            Some(Step::Name) => std::mem::take(&mut self.pending_name),
            Some(Step::Device) => device(),
            _ => String::new(),
        };
    }

    fn me_hit(&mut self, hit: CardHit) {
        match hit {
            CardHit::Next => self.me_next(),
            CardHit::Back => self.me_back(),
            CardHit::NotNow => {
                if self.me_card.step == Some(Step::Sync) {
                    self.me_finish();
                } else {
                    self.close_me_card();
                }
            }
            CardHit::Close => self.close_me_card(),
            CardHit::More => {
                self.close_me_card();
                self.open_settings_at(crate::settings::SEC_PROFILE, None);
            }
            CardHit::Face(k) => {
                self.me_card.face = match k {
                    0 => Face::Initial,
                    1 => Face::Emoji(self.me_card.input.clone()),
                    _ => Face::Picture,
                };
                if k != 1 {
                    self.me_card.input.clear();
                }
            }
            CardHit::SetupSync => {
                if self.me.is_none() {
                    self.me_finish();
                }
                self.open_me_card_at(Step::Sync);
            }
            CardHit::Way(k) => {
                self.me_card.way = match k { 1 => Way::Folder, 2 => Way::Forge, _ => Way::Here };
            }
            CardHit::ForgeKind(k) => {
                let kind = crate::forge::Kind::ALL[(k as usize).min(3)];
                self.me_card.forge = kind;
                self.me_card.host = kind.default_host().to_string();
                self.me_card.input = if kind == crate::forge::Kind::GitHub { String::new() } else { kind.default_host().to_string() };
            }
            CardHit::ForgeSignIn => {
                if let Some(id) = crate::forge::client_id() {
                    self.me_card.flow = Some(crate::forge::start_device(&id));
                    self.me_card.step = Some(Step::ForgeWait);
                    self.me_card.input.clear();
                }
            }
            CardHit::ForgeToken => {
                let input = self.me_card.input.trim().to_string();
                self.me_card.host = if input.is_empty() { self.me_card.forge.default_host().to_string() } else { input };
                self.me_card.step = Some(Step::ForgeToken);
                self.me_card.input.clear();
            }
            CardHit::OpenCodePage => {
                if let Some(crate::forge::Phase::Code { uri, .. }) = self.me_card.flow.as_ref().map(|f| f.phase()) {
                    self.open_url(&uri, true);
                }
            }
            CardHit::CopyCode => {
                if let Some(crate::forge::Phase::Code { user_code, .. }) = self.me_card.flow.as_ref().map(|f| f.phase()) {
                    if let Ok(mut cb) = arboard::Clipboard::new() {
                        let _ = cb.set_text(user_code.clone());
                    }
                    self.toast(format!("COPIED · {user_code}"), None);
                }
            }
            CardHit::KeyMode(k) => {
                self.me_card.key_mode = k;
                self.me_card.input.clear();
                if k == 0 {
                    self.me_card.made_key = crate::syncui::make_key();
                }
            }
            CardHit::CopyKey => {
                let word = if self.me_card.made_key.is_empty() { crate::syncui::make_key() } else { self.me_card.made_key.clone() };
                if let Ok(mut cb) = arboard::Clipboard::new() {
                    let _ = cb.set_text(word);
                }
                self.toast("COPIED · THE KEY · PASTE IT ON THE OTHER DEVICE", None);
            }
            CardHit::ForgeForget => {
                crate::forge::forget();
                self.behavior.sync_git.clear();
                self.save_prefs();
                self.me_card.step = Some(Step::Sync);
                self.me_card.way = Way::Here;
            }
            CardHit::Edit(step) => self.open_me_card_at(step),
            CardHit::Folder => {
                let dir = profile_dir();
                let cmd = if cfg!(target_os = "windows") {
                    format!("start \"\" \"{}\"", dir.display())
                } else if cfg!(target_os = "macos") {
                    format!("open \"{}\"", dir.display())
                } else {
                    format!("xdg-open \"{}\"", dir.display())
                };
                self.run_in_shell(&cmd);
            }
            CardHit::Badge => {}
        }
        self.dirty = true;
    }

    /// Keys while the card is up. Returns true when consumed.
    pub(crate) fn me_key(&mut self, ev: &winit::event::KeyEvent) -> bool {
        if !self.me_card.open {
            return false;
        }
        if ev.state != ElementState::Pressed {
            return true;
        }
        let typing = matches!(self.me_card.step, Some(Step::Name) | Some(Step::Device) | Some(Step::Folder) | Some(Step::ForgeToken))
            || (self.me_card.step == Some(Step::Face) && matches!(self.me_card.face, Face::Emoji(_)))
            || (self.me_card.step == Some(Step::Forge) && self.me_card.forge != crate::forge::Kind::GitHub)
            || (self.me_card.step == Some(Step::Key) && self.me_card.key_mode == 1);
        // Paste: a path, a token, a key — one line of it.
        if typing && self.mods.control_key() && matches!(&ev.logical_key, WKey::Character(c) if c.eq_ignore_ascii_case("v")) {
            if let Some(text) = arboard::Clipboard::new().ok().and_then(|mut cb| cb.get_text().ok()) {
                let line = text.lines().next().unwrap_or("").trim().to_string();
                let room = 200usize.saturating_sub(self.me_card.input.chars().count());
                self.me_card.input.extend(line.chars().take(room));
            }
            self.dirty = true;
            return true;
        }
        match &ev.logical_key {
            WKey::Named(NamedKey::Escape) => {
                if self.me_card.editing {
                    self.me_back();
                } else {
                    self.close_me_card();
                }
            }
            WKey::Named(NamedKey::Enter) => {
                if self.me_card.step.is_some() {
                    self.me_next();
                } else {
                    self.close_me_card();
                }
            }
            WKey::Named(NamedKey::Backspace) if typing => {
                self.me_card.input.pop();
            }
            WKey::Named(NamedKey::Space) if typing => self.me_card.input.push(' '),
            WKey::Character(c) if typing && !self.mods.control_key() && !self.mods.super_key() && c.chars().all(|ch| !ch.is_control()) => {
                let room = if matches!(self.me_card.step, Some(Step::Name) | Some(Step::Device) | Some(Step::Face)) { 40 } else { 200 };
                if self.me_card.input.chars().count() < room {
                    self.me_card.input.push_str(c);
                }
            }
            _ => {}
        }
        self.dirty = true;
        true
    }

    /// Mouse while the card is up: a hit, or outside closes.
    pub(crate) fn me_mouse(&mut self, button: MouseButton, state: ElementState, x: f32, y: f32) -> bool {
        if !self.me_card.open {
            return false;
        }
        if state != ElementState::Pressed || button != MouseButton::Left {
            return true;
        }
        if let Some((_, h)) = self.me_card.hits.iter().find(|(r, _)| r.contains(x, y)).copied() {
            self.me_hit(h);
        } else if !self.me_card.rect.contains(x, y) {
            self.close_me_card();
        }
        self.dirty = true;
        true
    }

    /// A rest under the pointer, for a tooltip with words of its own.
    fn me_tip(&mut self, key: u64, hit: Rect, words: String) {
        let (mx, my) = self.mouse;
        let hot = hit.contains(mx, my);
        let h = self.hovers.entry(key).or_insert_with(|| Hover { alpha: Anim::at(0.0), pulse: Anim::at(1.0), hot: false, since: Instant::now() });
        if hot != h.hot {
            h.hot = hot;
            if hot {
                h.since = Instant::now();
            }
        }
        if hot {
            let since = h.since;
            self.tip = Some(Tip { anchor: hit, text: words, since });
            if since.elapsed().as_millis() < 700 {
                self.dirty = true;
            }
        }
    }

    /// The face at `r`: the picture, the emoji, or the initial in the signal.
    pub(crate) fn draw_face(&mut self, scene: &mut Scene, r: Rect, face: &Face, name: &str) {
        let initial = name.trim().chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "?".into());
        match (face, self.avatar.clone()) {
            (Face::Picture, Some(b)) => {
                scene.texture(r, b, None);
                scene.layer(None);
            }
            (Face::Emoji(e), _) if !e.trim().is_empty() => {
                let st = Style { font: self.f.ui, px: (r.h * 0.68).round(), color: self.theme.ink, tracking: 0.0 };
                let ew = self.fonts.measure(st, e);
                self.fonts.draw(scene, st, r.x + ((r.w - ew) / 2.0).max(0.0), r.y + r.h * 0.5 + st.px * 0.36, e);
            }
            _ => {
                scene.rect(r, self.surface.signal);
                let st = Style { font: self.f.strong, px: (r.h * 0.5).round(), color: [1.0, 1.0, 1.0, 1.0], tracking: 0.0 };
                let iw = self.fonts.measure(st, &initial);
                self.fonts.draw(scene, st, r.x + (r.w - iw) / 2.0, r.y + r.h * 0.5 + st.px * 0.36, &initial);
            }
        }
    }

    /// A chip button: ink outline, hard shadow when primary, the word.
    fn me_button(&mut self, scene: &mut Scene, x: f32, base: f32, word: &str, primary: bool, hit: CardHit) -> f32 {
        let strong = self.label_strong();
        let t = self.theme.clone();
        let w = self.fonts.measure(strong, word) + self.px(24.0);
        let b = Rect::new(x, base - self.px(m::LABEL_PX) - self.px(8.0), w, self.px(m::LABEL_PX) + self.px(16.0));
        if primary {
            scene.rect(Rect::new(b.x + self.px(3.0), b.y + self.px(3.0), b.w, b.h), t.ink);
            scene.rect(b, self.surface.signal);
            scene.outline(b, self.px(m::STRUCTURE), t.ink);
            self.fonts.draw(scene, Style { color: [1.0, 1.0, 1.0, 1.0], ..strong }, b.x + self.px(12.0), base, word);
        } else {
            scene.rect(b, t.paper);
            scene.outline(b, self.px(m::HAIRLINE), t.ink);
            self.fonts.draw(scene, strong, b.x + self.px(12.0), base, word);
        }
        self.me_card.hits.push((b, hit));
        w
    }

    /// A typed line: the text, a block caret, a rule under it.
    fn me_input(&mut self, scene: &mut Scene, x: f32, y: f32, w: f32, hint: &str) -> f32 {
        let t = self.theme.clone();
        let big = Style { font: self.f.ui, px: self.px(18.0), color: t.ink, tracking: 0.0 };
        // A token shows as dots but its last four.
        let text = if self.me_card.step == Some(Step::ForgeToken) {
            let n = self.me_card.input.chars().count();
            let tail: String = self.me_card.input.chars().skip(n.saturating_sub(4)).collect();
            if n > 4 { format!("{}{}", "•".repeat((n - 4).min(24)), tail) } else { "•".repeat(n) }
        } else {
            self.me_card.input.clone()
        };
        let base = y + self.px(22.0);
        if text.is_empty() {
            self.fonts.draw(scene, Style { color: t.dim, ..big }, x, base, hint);
            scene.rect(Rect::new(x, base - self.px(16.0), self.px(10.0), self.px(20.0)), fade(t.ink, 0.35));
        } else {
            let tw = self.fonts.draw(scene, big, x, base, &self.fit(big, &text, w - self.px(14.0)));
            scene.rect(Rect::new(x + tw + self.px(2.0), base - self.px(16.0), self.px(10.0), self.px(20.0)), t.ink);
        }
        scene.hline(x, base + self.px(8.0), w, self.px(m::STRUCTURE), t.ink);
        base + self.px(8.0) + self.px(m::STRUCTURE)
    }

    /// Draw the card: above the footer's avatar when the sidebar shows,
    /// centred otherwise.
    pub(crate) fn draw_me_card(&mut self, scene: &mut Scene) {
        if !self.me_card.open && !self.me_card.rise.active() {
            return;
        }
        let rise = self.me_card.rise.value();
        if self.me_card.rise.active() {
            self.dirty = true;
        }
        let (w, h) = (self.target.size.0 as f32, self.target.size.1 as f32);
        let t: Theme = self.theme.clone();
        let ink = t.ink;
        let label = self.label();
        let strong = self.label_strong();
        let ui = self.ui();
        let ui_strong = self.ui_strong();
        let dim = Style { color: t.dim, ..label };
        let pad = self.px(20.0);
        let step = self.me_card.step;
        let wide = matches!(step, Some(Step::Sync) | Some(Step::Folder) | Some(Step::Forge) | Some(Step::ForgeToken) | Some(Step::ForgeWait) | Some(Step::Key));
        let cw = self.px(if wide { 440.0 } else { 380.0 }).min(w - self.px(32.0));
        self.me_card.hits.clear();

        // Height by what's on the card.
        let editing = self.me_card.editing;
        let has_me = self.me.is_some();
        let head_h = self.px(76.0);
        let row_h = self.px(40.0);
        let foot_h = self.px(52.0);
        let body_h = match step {
            None => row_h * 5.0 + self.px(12.0),
            Some(Step::Hello) => self.px(150.0),
            Some(Step::Name) | Some(Step::Device) => self.px(126.0),
            Some(Step::Face) => self.px(176.0),
            Some(Step::Sync) => self.px(196.0),
            Some(Step::Folder) => self.px(150.0),
            Some(Step::Forge) => self.px(212.0),
            Some(Step::ForgeToken) => self.px(150.0),
            Some(Step::ForgeWait) => self.px(170.0),
            Some(Step::Key) => self.px(196.0),
            Some(Step::Done) => self.px(126.0),
        };
        let ch = head_h + body_h + foot_h;

        // Where: over the avatar, or the middle.
        let sb = self.sidebar_rect();
        let anchored = self.sidebar_visible() && sb.w > self.px(60.0);
        let (cx, cy) = if anchored {
            let x = (sb.x + self.px(8.0)).min(w - cw - self.px(8.0)).max(self.px(8.0));
            let y = (sb.bottom() - self.px(m::FOOT_H) - self.px(10.0) - ch).max(self.px(8.0));
            (x, y)
        } else {
            (((w - cw) / 2.0).round(), ((h - ch) * 0.4).round())
        };
        let cy = cy + (1.0 - rise) * self.px(12.0);
        let r = Rect::new(cx.round(), cy.round(), cw, ch);
        self.me_card.rect = r;

        scene.layer(None);
        let a = rise;
        scene.rect(Rect::new(r.x + self.px(8.0), r.y + self.px(8.0), r.w, r.h), fade(ink, a));
        scene.rect(r, fade(t.paper, a));
        scene.outline(r, self.px(m::FLOATING), fade(ink, a));
        if a < 0.999 {
            scene.layer(Some(Rect::new(r.x, r.y, r.w, r.h * a.max(0.01))));
        }

        // Head: the face, the name, the device; the badge.
        let fsz = self.px(40.0);
        let fr = Rect::new(r.x + pad, r.y + ((head_h - fsz) / 2.0).round(), fsz, fsz);
        let (name, face) = match (&self.me, step) {
            (Some(me), _) if !(step.is_some() && !editing) => (me.name.clone(), me.face.clone()),
            _ => {
                let n = if self.pending_name.is_empty() { self.me_card.input.clone() } else { self.pending_name.clone() };
                let f = match &self.me_card.face {
                    Face::Emoji(_) if step == Some(Step::Face) => Face::Emoji(self.me_card.input.clone()),
                    f => f.clone(),
                };
                (if n.trim().is_empty() { os_user() } else { n }, f)
            }
        };
        self.draw_face(scene, fr, &face, &name);
        let nx = fr.right() + self.px(14.0);
        let base1 = r.y + head_h / 2.0 - self.px(2.0);
        let big = Style { font: self.f.strong, px: self.px(16.0), color: ink, tracking: 0.0 };
        let shown_name = if has_me || step.is_some_and(|s| s != Step::Hello) { name.clone() } else { "you".to_string() };
        let name_w = self.fonts.draw(scene, big, nx, base1, &self.fit(big, &shown_name, r.right() - pad - nx - self.px(90.0)));
        let _ = name_w;
        let under = if has_me { format!("on {} · local", device()) } else { "a profile that lives here".to_string() };
        self.fonts.draw(scene, dim, nx, base1 + self.px(18.0), &self.fit(dim, &under.caps(), r.right() - pad - nx - self.px(90.0)));
        if let Some(me) = &self.me {
            // The badge: a calendar and the count; the words in the tooltip.
            let days = me.days();
            let word = days.to_string();
            let isz = self.px(13.0);
            let bw = isz + self.px(6.0) + self.fonts.measure(strong, &word) + self.px(20.0);
            let bh = self.px(24.0);
            let br = Rect::new(r.right() - pad - bw, r.y + ((head_h - bh) / 2.0).round(), bw, bh);
            scene.rect(Rect::new(br.x + self.px(2.0), br.y + self.px(2.0), br.w, br.h), ink);
            scene.rect(br, t.paper);
            scene.outline(br, self.px(m::HAIRLINE), ink);
            self.fonts.draw_icon(scene, icons::CALENDAR, isz, br.x + self.px(10.0), br.y + ((bh - isz) / 2.0).round(), self.surface.signal);
            self.fonts.draw(scene, strong, br.x + self.px(10.0) + isz + self.px(6.0), br.y + bh / 2.0 + self.px(4.0), &word);
            let tip = format!("{} with nus · since {}", me.day_word(), me.created);
            self.me_tip(hover_key("me-badge", 0), br, tip);
            self.me_card.hits.push((br, CardHit::Badge));
        }
        scene.hline(r.x, r.y + head_h - self.px(m::STRUCTURE), r.w, self.px(m::STRUCTURE), ink);

        // Body.
        let bx = r.x + pad;
        let bw = r.w - 2.0 * pad;
        let mut y = r.y + head_h + self.px(14.0);
        let foot_base = r.bottom() - foot_h / 2.0 + self.px(4.0);
        match step {
            None => {
                let me = self.me.clone().unwrap_or(Me { name: os_user(), face: Face::Initial, created: today() });
                let face_word = match &me.face {
                    Face::Initial => "the initial".to_string(),
                    Face::Emoji(e) => e.clone(),
                    Face::Picture => "profile/avatar.png".to_string(),
                };
                let sync = if self.sync_ready() {
                    match crate::forge::load() {
                        Some(f) if f.clone_url == self.behavior.sync_git => f.word(),
                        _ if !self.behavior.sync_folder.is_empty() => "on · a folder".to_string(),
                        _ => "on · git".to_string(),
                    }
                } else {
                    "off · nothing leaves".to_string()
                };
                let rows: Vec<(&str, String, (&'static str, &'static str), CardHit)> = vec![
                    ("NAME", me.name.clone(), icons::TEXT_AA, CardHit::Edit(Step::Name)),
                    ("FACE", face_word, icons::SMILEY, CardHit::Edit(Step::Face)),
                    ("DEVICE", device(), icons::DESKTOP, CardHit::Edit(Step::Device)),
                    ("SYNC", sync, icons::BROADCAST, CardHit::SetupSync),
                    ("PRIVATE", "no account · no server · no telemetry".into(), icons::EYE_SLASH, CardHit::Folder),
                ];
                let (mx, my) = self.mouse;
                for (k, (what, value, icon, hit)) in rows.into_iter().enumerate() {
                    let rr = Rect::new(r.x, y, r.w, row_h);
                    if rr.contains(mx, my) && k < 4 {
                        scene.rect(rr, t.tint);
                    }
                    let base = y + row_h / 2.0 + self.px(4.0);
                    let isz = self.px(14.0);
                    self.fonts.draw_icon(scene, icon, isz, bx, base - isz + self.px(2.0), if k == 4 { t.dim } else { ink });
                    self.fonts.draw(scene, label, bx + isz + self.px(10.0), base, what);
                    let vx = bx + isz + self.px(10.0) + self.px(64.0);
                    let vs = if k == 4 { dim } else { ui };
                    let shown = if matches!(icon.0, "smiley") && matches!(me.face, Face::Emoji(_)) { value.clone() } else { value.to_lowercase() };
                    let vw = r.right() - pad - vx - self.px(20.0);
                    let vf = self.fit(vs, &shown, vw);
                    self.fonts.draw(scene, vs, vx, base, &vf);
                    if k < 4 {
                        let px = self.px(12.0);
                        self.fonts.draw_icon(scene, if k == 3 { icons::CARET_RIGHT } else { icons::PENCIL }, px, r.right() - pad - px, base - px + self.px(2.0), t.dim);
                    }
                    if k < 4 {
                        scene.hline(bx, y + row_h - self.px(m::HAIRLINE), bw, self.px(m::HAIRLINE), t.tint);
                    }
                    self.me_card.hits.push((rr, hit));
                    y += row_h;
                }
                let mut x = bx;
                x += self.me_button(scene, x, foot_base, "MORE", true, CardHit::More) + self.px(10.0);
                let _ = x;
                let cw2 = self.fonts.measure(strong, "CLOSE") + self.px(24.0);
                self.me_button(scene, r.right() - pad - cw2, foot_base, "CLOSE", false, CardHit::Close);
            }
            Some(Step::Hello) => {
                let isz = self.px(22.0);
                self.fonts.draw_icon(scene, icons::SHIELD, isz, bx, y, self.surface.signal);
                let head = Style { font: self.f.strong, px: self.px(15.0), color: ink, tracking: 0.0 };
                self.fonts.draw(scene, head, bx + isz + self.px(12.0), y + self.px(16.0), "This is your profile.");
                y += self.px(34.0);
                let lines = [
                    "It is a folder on this machine — settings, rules,",
                    "memory, sites, the lot. There is no account behind it,",
                    "no server, and nothing is counted or sent. Sync, if you",
                    "want it, is a key you copy; only ciphertext ever leaves.",
                ];
                for l in lines {
                    self.fonts.draw(scene, ui, bx, y + self.px(12.0), l);
                    y += self.px(19.0);
                }
                let mut x = bx;
                x += self.me_button(scene, x, foot_base, "BEGIN", true, CardHit::Next) + self.px(10.0);
                self.me_button(scene, x, foot_base, "NOT NOW", false, CardHit::NotNow);
            }
            Some(Step::Name) => {
                self.fonts.draw(scene, strong, bx, y + self.px(8.0), if editing { "YOUR NAME" } else { "WHAT NUS CALLS YOU" });
                y += self.px(22.0);
                y = self.me_input(scene, bx, y, bw, "a name");
                self.fonts.draw(scene, dim, bx, y + self.px(18.0), "THE INITIAL IS YOUR FACE UNTIL YOU PICK ONE · ENTER GOES ON");
                let mut x = bx;
                x += self.me_button(scene, x, foot_base, if editing { "SAVE" } else { "NEXT" }, true, CardHit::Next) + self.px(10.0);
                self.me_button(scene, x, foot_base, "BACK", false, CardHit::Back);
            }
            Some(Step::Face) => {
                self.fonts.draw(scene, strong, bx, y + self.px(8.0), "YOUR FACE");
                y += self.px(22.0);
                // Three tiles: the initial, an emoji, the picture.
                let tw = self.px(96.0);
                let th = self.px(64.0);
                let gap = self.px(12.0);
                let picked = self.me_card.face.clone();
                let n = if self.pending_name.is_empty() { name.clone() } else { self.pending_name.clone() };
                let tiles: [(u8, &str); 3] = [(0, "INITIAL"), (1, "EMOJI"), (2, "PICTURE")];
                for (k, word) in tiles {
                    let tx = bx + k as f32 * (tw + gap);
                    let tr = Rect::new(tx, y, tw, th);
                    let on = matches!((&picked, k), (Face::Initial, 0) | (Face::Emoji(_), 1) | (Face::Picture, 2));
                    scene.rect(Rect::new(tr.x + self.px(3.0), tr.y + self.px(3.0), tr.w, tr.h), if on { ink } else { fade(ink, 0.25) });
                    scene.rect(tr, t.paper);
                    scene.outline(tr, self.px(if on { m::FLOATING } else { m::HAIRLINE }), ink);
                    let fsz = self.px(26.0);
                    let fr = Rect::new(tr.x + self.px(10.0), tr.y + ((th - fsz) / 2.0).round(), fsz, fsz);
                    match k {
                        0 => self.draw_face(scene, fr, &Face::Initial, &n),
                        1 => {
                            let e = self.me_card.input.clone();
                            if e.trim().is_empty() {
                                self.fonts.draw_icon(scene, icons::SMILEY, fsz, fr.x, fr.y, t.dim);
                            } else {
                                self.draw_face(scene, fr, &Face::Emoji(e), &n);
                            }
                        }
                        _ => {
                            if self.avatar.is_some() {
                                self.draw_face(scene, fr, &Face::Picture, &n);
                            } else {
                                self.fonts.draw_icon(scene, icons::IMAGE, fsz, fr.x, fr.y, t.dim);
                            }
                        }
                    }
                    self.fonts.draw(scene, Style { color: if on { ink } else { t.dim }, ..label }, fr.right() + self.px(8.0), tr.y + th / 2.0 + self.px(4.0), word);
                    self.me_card.hits.push((tr, CardHit::Face(k)));
                }
                y += th + self.px(14.0);
                match &picked {
                    Face::Emoji(_) => {
                        y = self.me_input(scene, bx, y, bw, "type an emoji");
                        let _ = y;
                    }
                    Face::Picture => {
                        let words = if self.avatar.is_some() { "PROFILE/AVATAR.PNG · FOUND" } else { "DROP A PNG AT PROFILE/AVATAR.PNG" };
                        self.fonts.draw(scene, dim, bx, y + self.px(14.0), words);
                        let fw = self.fonts.measure(strong, "OPEN THE FOLDER") + self.px(24.0);
                        self.me_button(scene, r.right() - pad - fw, y + self.px(18.0), "OPEN THE FOLDER", false, CardHit::Folder);
                    }
                    Face::Initial => {
                        self.fonts.draw(scene, dim, bx, y + self.px(14.0), "THE FIRST LETTER OF YOUR NAME, IN THE SPACE'S SIGNAL");
                    }
                }
                let mut x = bx;
                x += self.me_button(scene, x, foot_base, if editing { "SAVE" } else { "NEXT" }, true, CardHit::Next) + self.px(10.0);
                self.me_button(scene, x, foot_base, "BACK", false, CardHit::Back);
            }
            Some(Step::Device) => {
                self.fonts.draw(scene, strong, bx, y + self.px(8.0), "THIS DEVICE");
                y += self.px(22.0);
                y = self.me_input(scene, bx, y, bw, "a name for this machine");
                self.fonts.draw(scene, dim, bx, y + self.px(18.0), "SYNC NAMES WHAT THIS MACHINE WROTE BY IT · IT NEVER SYNCS ITSELF");
                let mut x = bx;
                x += self.me_button(scene, x, foot_base, if editing { "SAVE" } else { "NEXT" }, true, CardHit::Next) + self.px(10.0);
                self.me_button(scene, x, foot_base, "BACK", false, CardHit::Back);
            }
            Some(Step::Sync) => {
                self.fonts.draw(scene, strong, bx, y + self.px(8.0), "WHERE IT LIVES");
                y += self.px(22.0);
                // Three tiles: here, a folder you sync, a private repo.
                let tw = ((bw - 2.0 * self.px(10.0)) / 3.0).floor();
                let th = self.px(74.0);
                let picked = self.me_card.way;
                let tiles: [(u8, &str, &str, (&'static str, &'static str)); 3] = [
                    (0, "HERE", "nothing leaves", icons::EYE_SLASH),
                    (1, "A FOLDER", "one you sync", icons::FOLDER_SIMPLE),
                    (2, "A REPO", "on a forge", icons::GITHUB),
                ];
                for (k, word, sub, icon) in tiles {
                    let tx = bx + k as f32 * (tw + self.px(10.0));
                    let tr = Rect::new(tx, y, tw, th);
                    let on = matches!((picked, k), (Way::Here, 0) | (Way::Folder, 1) | (Way::Forge, 2));
                    scene.rect(Rect::new(tr.x + self.px(3.0), tr.y + self.px(3.0), tr.w, tr.h), if on { ink } else { fade(ink, 0.25) });
                    scene.rect(tr, t.paper);
                    scene.outline(tr, self.px(if on { m::FLOATING } else { m::HAIRLINE }), ink);
                    let isz = self.px(16.0);
                    self.fonts.draw_icon(scene, icon, isz, tr.x + self.px(10.0), tr.y + self.px(10.0), if on { self.surface.signal } else { t.dim });
                    self.fonts.draw(scene, Style { color: if on { ink } else { t.dim }, ..strong }, tr.x + self.px(10.0), tr.y + self.px(44.0), word);
                    let small = Style { px: self.px(9.5), ..dim };
                    let sw = self.fit(small, sub, tw - self.px(16.0));
                    self.fonts.draw(scene, small, tr.x + self.px(10.0), tr.y + self.px(60.0), &sw);
                    self.me_card.hits.push((tr, CardHit::Way(k)));
                }
                y += th + self.px(14.0);
                let words = match picked {
                    Way::Here => "A FOLDER ON THIS MACHINE · NO ACCOUNT, NO SERVER, NOTHING SENT",
                    Way::Folder => "ICLOUD, ONEDRIVE, DROPBOX, SYNCTHING, A STICK · SEALED WITH A KEY YOU COPY",
                    Way::Forge => "GITHUB · FORGEJO · GITEA · GITLAB · NUS MAKES NUS-PROFILE, PRIVATE · SEALED",
                };
                for l in crate::reader::wrap(&self.fonts, dim, words, bw) {
                    self.fonts.draw(scene, dim, bx, y + self.px(12.0), &l);
                    y += self.px(15.0);
                }
                let mut x = bx;
                x += self.me_button(scene, x, foot_base, if picked == Way::Here { "KEEP IT HERE" } else { "NEXT" }, true, CardHit::Next) + self.px(10.0);
                if !editing && !has_me {
                    x += self.me_button(scene, x, foot_base, "NOT NOW", false, CardHit::NotNow) + self.px(10.0);
                }
                self.me_button(scene, x, foot_base, "BACK", false, CardHit::Back);
            }
            Some(Step::Folder) => {
                self.fonts.draw(scene, strong, bx, y + self.px(8.0), "THE FOLDER THAT TRAVELS");
                y += self.px(22.0);
                y = self.me_input(scene, bx, y, bw, "a path your OS already syncs");
                for l in crate::reader::wrap(&self.fonts, dim, "SEALED FILES PER DEVICE LAND THERE · CTRL+V PASTES · NEXT: THE KEY", bw) {
                    self.fonts.draw(scene, dim, bx, y + self.px(18.0), &l);
                    y += self.px(15.0);
                }
                let mut x = bx;
                x += self.me_button(scene, x, foot_base, "NEXT", true, CardHit::Next) + self.px(10.0);
                self.me_button(scene, x, foot_base, "BACK", false, CardHit::Back);
            }
            Some(Step::Forge) => {
                self.fonts.draw(scene, strong, bx, y + self.px(8.0), "WHICH FORGE");
                y += self.px(22.0);
                // Four chips.
                let picked = self.me_card.forge;
                let mut x = bx;
                for (k, kind) in crate::forge::Kind::ALL.iter().enumerate() {
                    let on = *kind == picked;
                    let w = self.fonts.measure(label, kind.name()) + self.px(20.0);
                    let chip = Rect::new(x, y, w, self.px(m::LABEL_PX) + self.px(14.0));
                    if on {
                        scene.rect(chip, ink);
                    } else {
                        scene.outline(chip, self.px(m::HAIRLINE), ink);
                    }
                    self.fonts.draw(scene, Style { color: if on { t.paper } else { ink }, ..label }, x + self.px(10.0), chip.y + chip.h / 2.0 + self.px(4.0), kind.name());
                    self.me_card.hits.push((chip, CardHit::ForgeKind(k as u8)));
                    x += w + self.px(8.0);
                }
                y += self.px(m::LABEL_PX) + self.px(14.0) + self.px(14.0);
                if picked == crate::forge::Kind::GitHub {
                    let signin = crate::forge::client_id().is_some();
                    let words = if signin {
                        "SIGN IN FROM HERE: A CODE TO ENTER ON GITHUB.COM, THEN NUS MAKES NUS-PROFILE, PRIVATE · OR PASTE A TOKEN WITH REPO SCOPE"
                    } else {
                        "A TOKEN WITH REPO SCOPE: GITHUB.COM › SETTINGS › DEVELOPER SETTINGS › PERSONAL ACCESS TOKENS · NUS MAKES NUS-PROFILE, PRIVATE"
                    };
                    for l in crate::reader::wrap(&self.fonts, dim, words, bw) {
                        self.fonts.draw(scene, dim, bx, y + self.px(12.0), &l);
                        y += self.px(15.0);
                    }
                    let mut x = bx;
                    if signin {
                        x += self.me_button(scene, x, foot_base, "SIGN IN WITH GITHUB", true, CardHit::ForgeSignIn) + self.px(10.0);
                        x += self.me_button(scene, x, foot_base, "A TOKEN", false, CardHit::ForgeToken) + self.px(10.0);
                    } else {
                        x += self.me_button(scene, x, foot_base, "USE A TOKEN", true, CardHit::ForgeToken) + self.px(10.0);
                    }
                    self.me_button(scene, x, foot_base, "BACK", false, CardHit::Back);
                } else {
                    y = self.me_input(scene, bx, y, bw, picked.default_host());
                    let hint = format!("THE INSTANCE · THEN A TOKEN: {}", picked.token_hint()).to_uppercase();
                    for l in crate::reader::wrap(&self.fonts, dim, &hint, bw) {
                        self.fonts.draw(scene, dim, bx, y + self.px(16.0), &l);
                        y += self.px(15.0);
                    }
                    let mut x = bx;
                    x += self.me_button(scene, x, foot_base, "USE A TOKEN", true, CardHit::ForgeToken) + self.px(10.0);
                    self.me_button(scene, x, foot_base, "BACK", false, CardHit::Back);
                }
            }
            Some(Step::ForgeToken) => {
                let kind = self.me_card.forge;
                self.fonts.draw(scene, strong, bx, y + self.px(8.0), &format!("A {} TOKEN", kind.name().to_uppercase()));
                y += self.px(22.0);
                y = self.me_input(scene, bx, y, bw, "paste it · ctrl+v");
                let hint = format!("{} · IT STAYS IN PROFILE/SYNC, NEVER IN A URL OR ON THE CARRIER", kind.token_hint()).to_uppercase();
                for l in crate::reader::wrap(&self.fonts, dim, &hint, bw) {
                    self.fonts.draw(scene, dim, bx, y + self.px(18.0), &l);
                    y += self.px(15.0);
                }
                let mut x = bx;
                x += self.me_button(scene, x, foot_base, "NEXT", true, CardHit::Next) + self.px(10.0);
                self.me_button(scene, x, foot_base, "BACK", false, CardHit::Back);
            }
            Some(Step::ForgeWait) => {
                use crate::forge::Phase;
                let phase = self.me_card.flow.as_ref().map(|f| f.phase()).unwrap_or(Phase::Failed("nothing running".into()));
                let head = Style { font: self.f.strong, px: self.px(15.0), color: ink, tracking: 0.0 };
                let isz = self.px(22.0);
                match &phase {
                    Phase::Starting => {
                        self.fonts.draw_icon(scene, icons::GITHUB, isz, bx, y, self.surface.signal);
                        self.fonts.draw(scene, head, bx + isz + self.px(12.0), y + self.px(16.0), "Asking GitHub for a code…");
                        self.dirty = true;
                    }
                    Phase::Code { user_code, uri } => {
                        self.fonts.draw_icon(scene, icons::GITHUB, isz, bx, y, self.surface.signal);
                        self.fonts.draw(scene, head, bx + isz + self.px(12.0), y + self.px(16.0), "Enter this code on GitHub");
                        y += self.px(36.0);
                        let code = Style { font: self.f.strong, px: self.px(28.0), color: ink, tracking: self.px(2.0) };
                        let cw = self.fonts.measure(code, user_code);
                        let cr = Rect::new(bx, y, cw + self.px(28.0), self.px(44.0));
                        scene.rect(Rect::new(cr.x + self.px(3.0), cr.y + self.px(3.0), cr.w, cr.h), ink);
                        scene.rect(cr, t.paper);
                        scene.outline(cr, self.px(m::STRUCTURE), ink);
                        self.fonts.draw(scene, code, cr.x + self.px(14.0), cr.y + self.px(31.0), user_code);
                        let cbw = self.fonts.measure(strong, "COPY") + self.px(24.0);
                        self.me_button(scene, cr.right() + self.px(12.0), cr.y + self.px(29.0), "COPY", false, CardHit::CopyCode);
                        let _ = cbw;
                        y += self.px(44.0) + self.px(12.0);
                        self.fonts.draw(scene, dim, bx, y + self.px(12.0), &format!("{} · NUS WAITS HERE", uri.trim_start_matches("https://").to_uppercase()));
                        self.dirty = true;
                    }
                    Phase::Verifying => {
                        self.fonts.draw_icon(scene, icons::LOCK_KEY, isz, bx, y, self.surface.signal);
                        self.fonts.draw(scene, head, bx + isz + self.px(12.0), y + self.px(16.0), "Signing in…");
                        self.dirty = true;
                    }
                    Phase::Making => {
                        self.fonts.draw_icon(scene, icons::LOCK_KEY, isz, bx, y, self.surface.signal);
                        self.fonts.draw(scene, head, bx + isz + self.px(12.0), y + self.px(16.0), "Finding nus-profile, or making it…");
                        self.dirty = true;
                    }
                    Phase::Done(f) => {
                        self.fonts.draw_icon(scene, icons::CHECK, isz, bx, y, self.surface.signal);
                        self.fonts.draw(scene, head, bx + isz + self.px(12.0), y + self.px(16.0), &format!("{} · {}/{}", f.kind.name(), f.user, f.repo));
                        y += self.px(34.0);
                        self.fonts.draw(scene, ui, bx, y + self.px(12.0), "Private, and empty until the first sync. Next, the key");
                        self.fonts.draw(scene, ui, bx, y + self.px(31.0), "that seals what goes there.");
                    }
                    Phase::Failed(e) => {
                        self.fonts.draw_icon(scene, icons::WARNING, isz, bx, y, self.surface.signal);
                        self.fonts.draw(scene, head, bx + isz + self.px(12.0), y + self.px(16.0), "That did not work.");
                        y += self.px(34.0);
                        for l in crate::reader::wrap(&self.fonts, ui, e, bw) {
                            self.fonts.draw(scene, ui, bx, y + self.px(12.0), &l);
                            y += self.px(19.0);
                        }
                    }
                }
                let mut x = bx;
                match &phase {
                    Phase::Done(_) => x += self.me_button(scene, x, foot_base, "NEXT", true, CardHit::Next) + self.px(10.0),
                    Phase::Code { .. } => x += self.me_button(scene, x, foot_base, "OPEN THAT PAGE", true, CardHit::OpenCodePage) + self.px(10.0),
                    _ => {}
                }
                self.me_button(scene, x, foot_base, "BACK", false, CardHit::Back);
            }
            Some(Step::Key) => {
                self.fonts.draw(scene, strong, bx, y + self.px(8.0), "THE KEY THAT SEALS IT");
                y += self.px(22.0);
                // Two chips: the first device makes one; the next joins with the word.
                let mode = self.me_card.key_mode;
                let mut x = bx;
                for (k, word) in [(0u8, "THE FIRST DEVICE · MAKE ONE"), (1u8, "I HAVE THE WORD")] {
                    let on = mode == k;
                    let w = self.fonts.measure(label, word) + self.px(20.0);
                    let chip = Rect::new(x, y, w, self.px(m::LABEL_PX) + self.px(14.0));
                    if on {
                        scene.rect(chip, ink);
                    } else {
                        scene.outline(chip, self.px(m::HAIRLINE), ink);
                    }
                    self.fonts.draw(scene, Style { color: if on { t.paper } else { ink }, ..label }, x + self.px(10.0), chip.y + chip.h / 2.0 + self.px(4.0), word);
                    self.me_card.hits.push((chip, CardHit::KeyMode(k)));
                    x += w + self.px(8.0);
                }
                y += self.px(m::LABEL_PX) + self.px(14.0) + self.px(14.0);
                if mode == 1 {
                    y = self.me_input(scene, bx, y, bw, "nus5-…");
                    for l in crate::reader::wrap(&self.fonts, dim, "THE WORD FROM THE OTHER DEVICE · ITS CARD SHOWS IT UNDER THE KEY", bw) {
                        self.fonts.draw(scene, dim, bx, y + self.px(18.0), &l);
                        y += self.px(15.0);
                    }
                } else {
                    let word = if self.me_card.made_key.is_empty() { crate::syncui::key().map(|k| nus_sync::encode_key(&k)).unwrap_or_default() } else { self.me_card.made_key.clone() };
                    if word.is_empty() {
                        for l in crate::reader::wrap(&self.fonts, ui, "DONE makes it: 32 random bytes, shown once as a word to copy to the next device. It never leaves your devices.", bw) {
                            self.fonts.draw(scene, ui, bx, y + self.px(14.0), &l);
                            y += self.px(19.0);
                        }
                    } else {
                        let kw = Style { font: self.f.ui, px: self.px(12.5), color: ink, tracking: 0.0 };
                        for l in crate::reader::wrap(&self.fonts, kw, &word.replace('-', "- "), bw - self.px(70.0)) {
                            self.fonts.draw(scene, kw, bx, y + self.px(14.0), &l.replace("- ", "-"));
                            y += self.px(17.0);
                        }
                        let cw2 = self.fonts.measure(strong, "COPY") + self.px(24.0);
                        self.me_button(scene, r.right() - pad - cw2, y - self.px(2.0), "COPY", false, CardHit::CopyKey);
                        for l in crate::reader::wrap(&self.fonts, dim, "COPY IT TO THE NEXT DEVICE · IT IS IN PROFILE/SYNC/KEY", bw - self.px(70.0)) {
                            self.fonts.draw(scene, dim, bx, y + self.px(14.0), &l);
                            y += self.px(15.0);
                        }
                    }
                }
                let mut x = bx;
                x += self.me_button(scene, x, foot_base, "DONE", true, CardHit::Next) + self.px(10.0);
                self.me_button(scene, x, foot_base, "BACK", false, CardHit::Back);
            }
            Some(Step::Done) => {
                let isz = self.px(22.0);
                self.fonts.draw_icon(scene, icons::HAND_WAVING, isz, bx, y, self.surface.signal);
                let head = Style { font: self.f.strong, px: self.px(15.0), color: ink, tracking: 0.0 };
                let word = self.me.as_ref().map(|m| m.day_word()).unwrap_or_else(|| "day 1".into());
                self.fonts.draw(scene, head, bx + isz + self.px(12.0), y + self.px(16.0), &format!("{}. Everything here stays here.", word.caps()));
                y += self.px(34.0);
                let how = if self.sync_ready() {
                    match crate::forge::load() {
                        Some(f) if f.clone_url == self.behavior.sync_git => format!("Sealed and carried by {}.", f.word()),
                        _ if !self.behavior.sync_folder.is_empty() => "Sealed and carried by your folder.".to_string(),
                        _ => "Sealed and carried by your git remote.".to_string(),
                    }
                } else {
                    "The avatar in the footer opens this card; MORE takes".to_string()
                };
                let lines = if self.sync_ready() {
                    [how.as_str(), "The avatar in the footer opens this card; MORE takes", "you to the full page under settings."]
                } else {
                    [how.as_str(), "you to the full page under settings — profile, sync,", "and what lives in the folder."]
                };
                for l in lines {
                    self.fonts.draw(scene, ui, bx, y + self.px(12.0), l);
                    y += self.px(19.0);
                }
                let mut x = bx;
                x += self.me_button(scene, x, foot_base, "DONE", true, CardHit::Next) + self.px(10.0);
                self.me_button(scene, x, foot_base, "MORE", false, CardHit::More);
            }
        }
        let _ = (ui_strong, foot_base);
        scene.hline(r.x, r.bottom() - foot_h, r.w, self.px(m::HAIRLINE), t.tint);
        scene.layer(None);
        // Hits are only live once the card has risen.
        if a < 0.9 {
            self.me_card.hits.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_dates_round_trip() {
        for (y, m, d) in [(1970, 1, 1), (2000, 2, 29), (2026, 9, 17), (2026, 12, 31), (1999, 3, 1)] {
            let z = days_from_civil(y, m, d);
            assert_eq!(civil_from_days(z), (y, m, d));
        }
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(1970, 1, 2), 1);
        assert_eq!(parse_date("2026-09-17"), Some(days_from_civil(2026, 9, 17)));
        assert_eq!(parse_date("nope"), None);
    }

    #[test]
    fn the_badge_counts_from_day_one() {
        let me = Me { name: "seb".into(), face: Face::Initial, created: today() };
        assert_eq!(me.days(), 1);
        assert_eq!(me.day_word(), "day 1");
        let (y, m, d) = civil_from_days(today_days() - 11);
        let me = Me { name: "seb".into(), face: Face::Initial, created: format!("{y:04}-{m:02}-{d:02}") };
        assert_eq!(me.days(), 12);
        assert_eq!(me.day_word(), "12 days");
        assert_eq!(me.initial(), "S");
    }
}
