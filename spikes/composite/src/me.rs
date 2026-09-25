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

/// The fine print's version. Raise it when the privacy notes or the terms
/// change in a way people should see again; the walk then asks once more.
pub const TERMS_VERSION: u32 = 1;

fn agreed_path() -> PathBuf {
    profile_dir().join("agreed")
}

/// Whether this profile has agreed to the current fine print.
pub fn agreed() -> bool {
    std::fs::read_to_string(agreed_path()).ok().and_then(|s| s.split_whitespace().next().and_then(|v| v.parse::<u32>().ok())).is_some_and(|v| v >= TERMS_VERSION)
}

fn agree() {
    let _ = std::fs::create_dir_all(profile_dir());
    let _ = crate::store::write_atomic(&agreed_path(), format!("{TERMS_VERSION} {}\n", today()).as_bytes());
}

/// The fine print, as shipped: the repository's own documents, compiled in,
/// so what you agree to is exactly what the source says.
const PRIVACY: &str = include_str!("../../../docs/PRIVACY_AND_DIAGNOSTICS.md");
const LICENSE: &str = include_str!("../../../LICENSE");
const NOTICE: &str = include_str!("../../../NOTICE");

/// Plain lines from Markdown: headings, emphasis, code marks and table
/// rules dropped; a table row read as its cells.
fn plain(md: &str) -> Vec<String> {
    // Source lines are hard-wrapped; a paragraph is read back as one line
    // so it wraps to the card. Headings, list items and table rows stand alone.
    let mut out: Vec<String> = Vec::new();
    let mut para = String::new();
    let flush = |para: &mut String, out: &mut Vec<String>| {
        if !para.is_empty() {
            out.push(std::mem::take(para));
        }
    };
    for line in md.lines() {
        let t = line.trim();
        let clean = |s: &str| s.replace("**", "").replace('`', "");
        if t.is_empty() {
            flush(&mut para, &mut out);
            out.push(String::new());
        } else if t.starts_with('|') {
            flush(&mut para, &mut out);
            if !t.chars().all(|c| matches!(c, '|' | '-' | ':' | ' ')) {
                out.push(clean(&t.trim_matches('|').split('|').map(str::trim).filter(|c| !c.is_empty()).collect::<Vec<_>>().join(" — ")));
            }
        } else if t.starts_with('#') {
            flush(&mut para, &mut out);
            out.push(clean(t.trim_start_matches('#').trim()).to_uppercase());
        } else if t.starts_with("- ") || t.starts_with("* ") {
            flush(&mut para, &mut out);
            para = format!("· {}", clean(&t[2..]));
        } else {
            if !para.is_empty() {
                para.push(' ');
            }
            para.push_str(&clean(t));
        }
    }
    flush(&mut para, &mut out);
    out.dedup_by(|a, b| a.is_empty() && b.is_empty());
    out
}

/// The three documents, with a line on top that says what each is.
fn fine_print(tab: u8) -> (&'static str, Vec<String>) {
    match tab {
        0 => ("nus has no account and sends no usage data. Your profile stays on this machine unless you turn on sync, which encrypts it first. The full notes follow.", plain(PRIVACY)),
        1 => ("nus is free software under the MIT License. These are its terms: use it, change it, share it; it comes with no warranty.", plain(LICENSE)),
        _ => ("nus is built with work by others, each under its own license:", plain(NOTICE)),
    }
}

fn device_path() -> PathBuf {
    profile_dir().join("sync").join("device")
}

impl Me {
    /// The profile, or as much of it as this nus can read (store.rs): a
    /// face from a newer nus costs the face, not the name.
    pub fn load() -> Option<Me> {
        if !me_path().is_file() {
            return None;
        }
        crate::store::read_json::<Option<Me>>(&me_path()).value
    }

    pub fn save(&self) {
        let _ = std::fs::create_dir_all(profile_dir());
        let _ = crate::store::write_json(&me_path(), self);
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
    Import,
    ImportSources,
    ImportReview,
    /// Privacy, terms and licenses: read, then agreed to, once.
    Terms,
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
    ImportOpen,
    ImportSource(usize),
    ImportPick,
    ImportApply,
    MercuryDone,
    Next,
    Back,
    NotNow,
    Close,
    More,
    Face(u8),
    /// Browse for a picture for the face.
    PickPicture,
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
    /// The fine print's tabs: 0 privacy, 1 terms, 2 licenses.
    TermsTab(u8),
    /// Agree to the fine print and finish the first walk.
    Agree,
}

pub struct MeCard {
    pub import: crate::import_flow::Flow,
    pub mercury_reveal: Option<crate::mercury::Reveal>,
    pub mercury_art: Option<std::sync::Arc<wgpu::BindGroup>>,
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
    /// The first walk, on a fresh install: the big card in the middle, no
    /// way out but through, and Welcome after it.
    pub first: bool,
    /// The first walk just finished: the card falls away as Welcome opens.
    pub leaving: bool,
    pub terms_tab: u8,
    /// The fine print's scroll, and how far it can go (physical px).
    pub terms_scroll: f32,
    pub terms_reach: f32,
    /// Set once the fall has begun (the frame it started on).
    pub leave_from: Option<std::time::Instant>,
}

impl Default for MeCard {
    fn default() -> Self {
        MeCard {
            import: Default::default(),
            mercury_reveal: None,
            mercury_art: None,
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
            first: false,
            leaving: false,
            terms_tab: 0,
            terms_scroll: 0.0,
            terms_reach: 0.0,
            leave_from: None,
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

    /// A fresh install: the walk in the big card, before Welcome. A profile
    /// that exists but hasn't agreed to the fine print starts there.
    pub(crate) fn open_first_walk(&mut self) {
        self.open_me_card();
        let c = &mut self.me_card;
        c.first = true;
        c.leaving = false;
        c.step = Some(if self.me.is_none() { Step::Hello } else { Step::Terms });
    }

    /// The first walk is done: the card falls away and Welcome takes its
    /// place. Onboarding is complete from here.
    fn finish_first_walk(&mut self) {
        let c = &mut self.me_card;
        c.first = false;
        c.leaving = true;
        c.open = false;
        c.hits.clear();
        // The fall starts on the first frame that draws it: opening Welcome
        // can hold that frame back longer than the fall itself lasts.
        c.rise = Anim::at(1.0);
        c.leave_from = None;
        if let Some(f) = c.flow.take() {
            f.cancel();
        }
        // The untouched prompt the window was born with gives way.
        let birth = self.tabs.len() == 1 && matches!(&self.tabs[0].left, crate::app::Pane::Home(h) if h.input.is_empty() && !h.library);
        if birth {
            self.replace_birth(crate::app::Pane::Hints(crate::app::HintsPane { rect: Rect::new(0.0, 0.0, 1.0, 1.0), scroll: 0.0 }));
        } else {
            self.open_welcome();
        }
        self.save_hints();
        crate::install::complete();
        self.play_event("onboarding.tick");
        self.dirty = true;
    }

    /// For the shot driver: finish the first walk as someone clicking through
    /// would, with the defaults (the OS user's name, the initial, kept here).
    pub(crate) fn finish_first_walk_now(&mut self) {
        if self.me.is_none() {
            self.pending_name = os_user();
            self.me_finish();
        }
        agree();
        self.finish_first_walk();
    }

    /// The wheel over the fine print.
    pub(crate) fn me_wheel(&mut self, x: f32, y: f32, dy: f32) -> bool {
        if !self.me_card.open || self.me_card.step != Some(Step::Terms) || !self.me_card.rect.contains(x, y) {
            return false;
        }
        let c = &mut self.me_card;
        c.terms_scroll = (c.terms_scroll - dy).clamp(0.0, c.terms_reach.max(0.0));
        self.dirty = true;
        true
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
        if step==Step::Import {c.import.focus=0;c.import.at=crate::clock::now();}
        // The sync steps are a walk of their own, never a one-field edit.
        c.editing = self.me.is_some() && !matches!(step, Step::Sync | Step::Folder | Step::Forge | Step::ForgeToken | Step::ForgeWait | Step::Key | Step::Import | Step::ImportSources | Step::ImportReview);
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
        self.me_card.import.picker=None;
        self.me_card.import.pending=None;
        self.me_card.open = false;
        self.me_card.mercury_reveal = None;
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
                        self.notice(nus_render::text::icons::LOCK_KEY, "Not A nus Key", "keys start nus5-");
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
            Step::Import => self.me_card.step = Some(if self.me_card.first { Step::Terms } else { Step::Done }),
            Step::Terms => {
                agree();
                if self.me_card.first {
                    self.finish_first_walk();
                    return;
                }
                self.me_card.step = Some(Step::Done);
            }
            Step::ImportSources => self.import_hit(CardHit::ImportPick),
            Step::ImportReview => self.import_hit(CardHit::ImportApply),
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
        self.me_card.step = Some(Step::Import);
        self.me_card.import = Default::default();
        self.me_card.input.clear();
        self.play_event("onboarding.tick");
    }

    fn me_back(&mut self) {
        let c = &mut self.me_card;
        c.import.picker=None;
        c.import.pending=None;
        if c.editing {
            c.step = None;
            c.editing = false;
            return;
        }
        if let Some(f) = c.flow.take() {
            f.cancel();
        }
        c.step = match c.step {
            Some(Step::ImportSources) => Some(Step::Import),
            Some(Step::Terms) => Some(Step::Import),
            Some(Step::ImportReview) => Some(Step::ImportSources),
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

    pub(crate) fn me_hit(&mut self, hit: CardHit) {
        match hit {
            CardHit::ImportOpen | CardHit::ImportSource(_) | CardHit::ImportPick | CardHit::ImportApply => self.import_hit(hit),
            CardHit::MercuryDone => self.mercury_action(hit),
            CardHit::Next => self.me_next(),
            CardHit::Back => self.me_back(),
            CardHit::NotNow if self.me_card.first => {}
            CardHit::NotNow => {
                if self.me_card.step == Some(Step::Sync) {
                    self.me_finish();
                } else {
                    self.close_me_card();
                }
            }
            CardHit::Close if self.me_card.first => {}
            CardHit::Close => self.close_me_card(),
            CardHit::PickPicture => {
                self.me_card.face = Face::Picture;
                self.pick_avatar();
            }
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
                // A picture, and there is none yet (or you picked it
                // again): browse for one. Every platform has a dialog.
                if k == 2 {
                    self.pick_avatar();
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
                    self.toast(icons::COPY, "Copied", user_code.clone(), None);
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
                self.toast(icons::COPY, "Copied", "the key · paste it on the other device", None);
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
            CardHit::TermsTab(k) => {
                self.me_card.terms_tab = k.min(2);
                self.me_card.terms_scroll = 0.0;
            }
            CardHit::Agree => self.me_next(),
        }
        self.dirty = true;
    }

    /// Keys while the card is up. Returns true when consumed.
    pub(crate) fn me_key(&mut self, ev: &crate::app::KeyIn) -> bool {
        if !self.me_card.open {
            return false;
        }
        if ev.state != ElementState::Pressed {
            return true;
        }
        if self.me_card.mercury_reveal.is_some(){
            self.mercury_key(&ev.logical_key);return true;
        }
        if matches!(self.me_card.step,Some(Step::Import|Step::ImportSources|Step::ImportReview)) {
            match &ev.logical_key {
                WKey::Named(NamedKey::Tab) if self.me_card.step==Some(Step::Import)=>{self.me_card.import.focus=1-self.me_card.import.focus;self.dirty=true;return true;}
                WKey::Named(NamedKey::Enter) if self.me_card.step==Some(Step::Import)&&self.me_card.import.focus==1=>{self.import_hit(CardHit::ImportOpen);return true;}
                WKey::Named(NamedKey::ArrowRight|NamedKey::ArrowDown|NamedKey::ArrowLeft|NamedKey::ArrowUp|NamedKey::Tab) if self.me_card.step==Some(Step::ImportSources)=>{
                    let back=matches!(ev.logical_key,WKey::Named(NamedKey::ArrowLeft|NamedKey::ArrowUp));
                    let i=self.me_card.import.source;self.import_hit(CardHit::ImportSource((i+if back{11}else{1})%12));return true;
                }
                WKey::Named(NamedKey::Escape) if self.me_card.step!=Some(Step::Import)=>{self.me_back();self.dirty=true;return true;}
                _=>{}
            }
        }
        if self.me_card.step == Some(Step::Terms) {
            let page = self.me_card.rect.h * 0.4;
            let step = match &ev.logical_key {
                WKey::Named(NamedKey::ArrowDown) => Some(self.px(40.0)),
                WKey::Named(NamedKey::ArrowUp) => Some(-self.px(40.0)),
                WKey::Named(NamedKey::PageDown) | WKey::Named(NamedKey::Space) => Some(page),
                WKey::Named(NamedKey::PageUp) => Some(-page),
                WKey::Named(NamedKey::Tab) => {
                    self.me_card.terms_tab = (self.me_card.terms_tab + if self.mods.shift_key() { 2 } else { 1 }) % 3;
                    self.me_card.terms_scroll = 0.0;
                    Some(0.0)
                }
                _ => None,
            };
            if let Some(d) = step {
                let c = &mut self.me_card;
                c.terms_scroll = (c.terms_scroll + d).clamp(0.0, c.terms_reach.max(0.0));
                self.dirty = true;
                return true;
            }
        }
        let typing = matches!(self.me_card.step, Some(Step::Name) | Some(Step::Device) | Some(Step::Folder) | Some(Step::ForgeToken))
            || (self.me_card.step == Some(Step::Face) && matches!(self.me_card.face, Face::Emoji(_)))
            || (self.me_card.step == Some(Step::Forge) && self.me_card.forge != crate::forge::Kind::GitHub)
            || (self.me_card.step == Some(Step::Key) && self.me_card.key_mode == 1);
        // The line's own editing: typing, erasing, a pasted path, token or
        // key — one line of it (field.rs). A name is short; the rest have room.
        if typing {
            let room = if matches!(self.me_card.step, Some(Step::Name) | Some(Step::Device) | Some(Step::Face)) { 40 } else { 200 };
            if crate::field::edit(&mut self.me_card.input, ev, self.mods, room).taken() {
                self.dirty = true;
                return true;
            }
        }
        match &ev.logical_key {
            WKey::Named(NamedKey::Escape) => {
                if self.me_card.editing || self.me_card.first {
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
        let pad = self.touch_pad();
        if let Some((_, h)) = self.me_card.hits.iter().find(|(r, _)| crate::touch::grown(*r, pad).contains(x, y)).copied() {
            self.me_hit(h);
        } else if !self.me_card.rect.contains(x, y) && !self.me_card.first {
            self.close_me_card();
        }
        self.dirty = true;
        true
    }

    /// A rest under the pointer, for a tooltip with words of its own.
    fn me_tip(&mut self, key: u64, hit: Rect, words: String) {
        self.offer_tip(key, hit, words);
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
    pub(crate) fn me_button(&mut self, scene: &mut Scene, x: f32, base: f32, word: &str, primary: bool, hit: CardHit) -> f32 {
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
        if self.me_card.mercury_reveal.is_some(){return;}
        if self.me_card.leaving && self.me_card.leave_from.is_none() {
            self.me_card.leave_from = Some(crate::clock::now());
            self.me_card.rise.replay(1.0, 0.0, self.motion.dur(base::PALETTE) * 3.6);
        }
        if !self.me_card.open && !self.me_card.rise.active() {
            self.me_card.leaving = false;
            self.me_card.leave_from = None;
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
        let wide = matches!(step, Some(Step::Sync) | Some(Step::Folder) | Some(Step::Forge) | Some(Step::ForgeToken) | Some(Step::ForgeWait) | Some(Step::Key) | Some(Step::Import) | Some(Step::ImportSources) | Some(Step::ImportReview) | Some(Step::Terms));
        // The first walk is a room of its own: bigger, centered, with the
        // steps down its left side so you can see where you are.
        let roomy = self.me_card.first || self.me_card.leaving;
        let rail_w = if roomy && w > self.px(640.0) { self.px(196.0) } else { 0.0 };
        let cw = if roomy { self.px(800.0).min(w - self.px(48.0)) } else { self.px(if wide { 440.0 } else { 380.0 }).min(w - self.px(32.0)) };
        self.me_card.hits.clear();

        // Height by what's on the card.
        let editing = self.me_card.editing;
        let has_me = self.me.is_some();
        let head_h = self.px(76.0);
        let foot_h = self.px(52.0);
        let row_h = if step.is_none(){((h-head_h-foot_h-self.px(32.0))/6.0).clamp(self.px(24.0),self.px(40.0))}else{self.px(40.0)};
        let body_h = match step {
            Some(Step::Import) => self.px(if self.me_card.import.message.is_empty(){172.0}else{224.0}),
            Some(Step::ImportSources) => self.px(if self.me_card.import.message.is_empty(){296.0}else{338.0}),
            Some(Step::ImportReview) => self.px(312.0),
            None => row_h * 6.0 + self.px(12.0),
            Some(Step::Hello) => self.px(150.0),
            Some(Step::Name) | Some(Step::Device) => self.px(126.0),
            Some(Step::Face) => self.px(176.0),
            Some(Step::Sync) => self.px(196.0),
            Some(Step::Folder) => self.px(150.0),
            Some(Step::Forge) => self.px(212.0),
            Some(Step::ForgeToken) => self.px(150.0),
            Some(Step::ForgeWait) => self.px(170.0),
            Some(Step::Key) => self.px(196.0),
            Some(Step::Terms) => self.px(300.0),
            Some(Step::Done) => self.px(126.0),
        };
        let body_h=if matches!(step,Some(Step::Import|Step::ImportSources|Step::ImportReview)){body_h.min((h-head_h-foot_h-self.px(20.0)).max(self.px(150.0)))}else{body_h};
        let ch = if roomy { (h - self.px(48.0)).min(self.px(580.0)).max(head_h + body_h + foot_h).min(h - self.px(16.0)) } else { head_h + body_h + foot_h };

        // Where: over the avatar, or the middle.
        let sb = self.sidebar_rect();
        let anchored = !roomy && self.sidebar_visible() && sb.w > self.px(60.0);
        let (cx, cy) = if anchored {
            let x = (sb.x + self.px(8.0)).min(w - cw - self.px(8.0)).max(self.px(8.0));
            let y = (sb.bottom() - self.px(m::FOOT_H) - self.px(10.0) - ch).max(self.px(8.0));
            (x, y)
        } else {
            (((w - cw) / 2.0).round(), ((h - ch) * 0.4).round())
        };
        let leaving = self.me_card.leaving;
        if leaving && !self.me_card.rise.active() {
            // Gone: Welcome has the room now.
            self.me_card.leaving = false;
            self.me_card.leave_from = None;
            return;
        }
        // Rising, the card lifts a little into place; leaving, it drops out
        // of sight, gathering speed, as Welcome opens behind it.
        let cy = if leaving { cy + (1.0 - rise).powi(2) * (h - cy + self.px(24.0)) } else { cy + (1.0 - rise) * self.px(12.0) };
        let r = Rect::new(cx.round(), cy.round(), cw, ch);
        self.me_card.rect = r;

        scene.layer(None);
        let a = if leaving { rise.sqrt() } else { rise };
        scene.rect(Rect::new(r.x + self.px(8.0), r.y + self.px(8.0), r.w, r.h), fade(ink, a));
        scene.rect(r, fade(t.paper, a));
        scene.outline(r, self.px(m::FLOATING), fade(ink, a));
        if a < 0.999 && !leaving {
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
            // The masthead number: the day as the issue number — DAY in small
            // caps over an italic numeral, a hairline above and below, no box.
            let days = me.days();
            let word = format!("No. {days}");
            let num = Style { font: self.f.wordmark, px: self.px(18.0), color: ink, tracking: 0.0 };
            let cap = Style { color: t.dim, px: self.px(9.5), tracking: self.px(1.4), ..label };
            let bw = self.fonts.measure(num, &word).max(self.fonts.measure(cap, "DAY")) + self.px(4.0);
            let bh = self.px(38.0);
            let br = Rect::new(r.right() - pad - bw, r.y + ((head_h - bh) / 2.0).round(), bw, bh);
            scene.hline(br.x, br.y, br.w, self.px(m::HAIRLINE), ink);
            scene.hline(br.x, br.bottom() - self.px(m::HAIRLINE), br.w, self.px(m::HAIRLINE), ink);
            let cw = self.fonts.measure(cap, "DAY");
            self.fonts.draw(scene, cap, br.right() - self.px(2.0) - cw, br.y + self.px(13.0), "DAY");
            let nw = self.fonts.measure(num, &word);
            self.fonts.draw(scene, num, br.right() - self.px(2.0) - nw, br.bottom() - self.px(7.0), &word);
            let tip = format!("{} with nus · since {}", me.day_word(), me.created);
            self.me_tip(hover_key("me-badge", 0), br, tip);
            self.me_card.hits.push((br, CardHit::Badge));
        }
        scene.hline(r.x, r.y + head_h - self.px(m::STRUCTURE), r.w, self.px(m::STRUCTURE), ink);

        // The steps, down the left of the first walk.
        if rail_w > 0.0 {
            let stages = ["Hello", "Your name", "Your face", "This device", "Where it lives", "Bring things over", "The fine print"];
            let at = match step {
                Some(Step::Hello) | None => 0,
                Some(Step::Name) => 1,
                Some(Step::Face) => 2,
                Some(Step::Device) => 3,
                Some(Step::Sync | Step::Folder | Step::Forge | Step::ForgeToken | Step::ForgeWait | Step::Key) => 4,
                Some(Step::Import | Step::ImportSources | Step::ImportReview) => 5,
                Some(Step::Terms | Step::Done) => 6,
            };
            let rail = Rect::new(r.x, r.y + head_h, rail_w, r.h - head_h);
            scene.rect(rail, fade(t.tint, 0.6));
            scene.vline(rail.right(), rail.y, rail.h, self.px(m::HAIRLINE), ink);
            let mut ry = rail.y + self.px(22.0);
            for (k, name) in stages.iter().enumerate() {
                let done = k < at;
                let now = k == at;
                let dot = Rect::new(rail.x + pad, ry - self.px(9.0), self.px(10.0), self.px(10.0));
                if done {
                    self.fonts.draw_icon(scene, icons::CHECK, self.px(11.0), dot.x, dot.y - self.px(0.5), ink);
                } else if now {
                    scene.rect(dot, self.surface.signal);
                } else {
                    scene.outline(dot, self.px(m::HAIRLINE), t.dim);
                }
                let st = Style { color: if now || done { ink } else { t.dim }, ..if now { ui_strong } else { ui } };
                self.fonts.draw(scene, st, dot.right() + self.px(12.0), ry, name);
                ry += self.px(34.0);
            }
            let st = Style { color: t.dim, ..label };
            let word = format!("STEP {} OF {}", at + 1, stages.len());
            self.fonts.draw(scene, st, rail.x + pad, rail.bottom() - self.px(18.0), &word);
        }

        // Body.
        let bx = r.x + rail_w + pad;
        let bw = r.w - rail_w - 2.0 * pad;
        let mut y = r.y + head_h + self.px(if roomy { 26.0 } else { 14.0 });
        let foot_base = r.bottom() - foot_h / 2.0 + self.px(4.0);
        if roomy {
            scene.hline(r.x + rail_w, r.bottom() - foot_h, r.w - rail_w, self.px(m::HAIRLINE), t.tint);
        }
        match step {
            Some(Step::Import|Step::ImportSources|Step::ImportReview) => self.draw_import_page(scene,bx,y,bw,foot_base),
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
                    ("PRIVATE", "no usage telemetry".into(), icons::EYE_SLASH, CardHit::Folder),
                    ("USED SINCE", me.created.clone(), icons::CALENDAR, CardHit::Badge),
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
                    let vx = bx + isz + self.px(10.0) + self.fonts.measure(label,"USED SINCE") + self.px(12.0);
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
                let head = Style { font: self.f.strong, px: self.px(15.0), color: ink, tracking: 0.0 };
                if !roomy {
                    self.fonts.draw_icon(scene, icons::SHIELD, isz, bx, y, self.surface.signal);
                }
                if roomy {
                    let title = Style { font: self.f.serif, px: self.px(30.0), color: ink, tracking: 0.0 };
                    self.fonts.draw(scene, title, bx, y + self.px(26.0), "First, a little about you.");
                    y += self.px(52.0);
                    let lead = "A minute to set up the profile that's yours on this machine. Everything here can be changed later from the avatar in the footer.";
                    for l in crate::reader::wrap(&self.fonts, ui, lead, bw) {
                        self.fonts.draw(scene, ui, bx, y + self.px(12.0), &l);
                        y += self.px(19.0);
                    }
                    y += self.px(16.0);
                    let points: [((&'static str, &'static str), &str, &str); 3] = [
                        (icons::SHIELD, "It stays here", "A folder on this machine. No nus account, no usage data sent anywhere."),
                        (icons::BROADCAST, "It can travel, sealed", "If you choose sync, it's encrypted before it leaves, with a key only you hold."),
                        (icons::DOWNLOAD, "It can start full", "Bring bookmarks, history and settings over from the browser and terminal you use now."),
                    ];
                    for (icon, what, why) in points {
                        let isz = self.px(16.0);
                        self.fonts.draw_icon(scene, icon, isz, bx, y + self.px(1.0), self.surface.signal);
                        self.fonts.draw(scene, ui_strong, bx + isz + self.px(12.0), y + self.px(13.0), what);
                        let mut ly = y + self.px(31.0);
                        for l in crate::reader::wrap(&self.fonts, Style { color: t.dim, ..ui }, why, bw - isz - self.px(12.0)) {
                            self.fonts.draw(scene, Style { color: t.dim, ..ui }, bx + isz + self.px(12.0), ly, &l);
                            ly += self.px(18.0);
                        }
                        y = ly + self.px(10.0);
                    }
                    self.me_button(scene, bx, foot_base, "LET'S BEGIN", true, CardHit::Next);
                } else {
                    self.fonts.draw(scene, head, bx + isz + self.px(12.0), y + self.px(16.0), "This is your profile.");
                    y += self.px(34.0);
                    let words = "Your profile lives on this machine. No nus account or usage telemetry. Optional sync encrypts profile data. Automatic GitHub update checks can be disabled in Settings.";
                    for l in crate::reader::wrap(&self.fonts, ui, words, bw) {
                        self.fonts.draw(scene, ui, bx, y + self.px(12.0), &l);
                        y += self.px(19.0);
                    }
                    let mut x = bx;
                    x += self.me_button(scene, x, foot_base, "BEGIN", true, CardHit::Next) + self.px(10.0);
                    self.me_button(scene, x, foot_base, "NOT NOW", false, CardHit::NotNow);
                }
            }
            Some(Step::Name) => {
                self.fonts.draw(scene, strong, bx, y + self.px(8.0), if editing { "YOUR NAME" } else { "WHAT NUS CALLS YOU" });
                y += self.px(22.0);
                y = self.me_input(scene, bx, y, bw, "a name");
                for l in crate::reader::wrap(&self.fonts, dim, "THE INITIAL IS YOUR FACE UNTIL YOU PICK ONE · ENTER GOES ON", bw) {
                    self.fonts.draw(scene, dim, bx, y + self.px(18.0), &l);
                    y += self.px(15.0);
                }
                let mut x = bx;
                x += self.me_button(scene, x, foot_base, if editing { "SAVE" } else { "NEXT" }, true, CardHit::Next) + self.px(10.0);
                self.me_button(scene, x, foot_base, "BACK", false, CardHit::Back);
            }
            Some(Step::Face) => {
                self.fonts.draw(scene, strong, bx, y + self.px(8.0), "YOUR FACE");
                y += self.px(22.0);
                // Three tiles: the initial, an emoji, the picture.
                let gap = self.px(12.0);
                let tw = ((bw - 2.0 * gap) / 3.0).floor();
                let th = self.px(64.0);
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
                    let ws = Style { color: if on { ink } else { t.dim }, ..label };
                    let wf = self.fit(ws, word, tr.right() - fr.right() - self.px(14.0));
                    self.fonts.draw(scene, ws, fr.right() + self.px(8.0), tr.y + th / 2.0 + self.px(4.0), &wf);
                    self.me_card.hits.push((tr, CardHit::Face(k)));
                }
                y += th + self.px(14.0);
                match &picked {
                    Face::Emoji(_) => {
                        y = self.me_input(scene, bx, y, bw, "type an emoji");
                        let _ = y;
                    }
                    Face::Picture => {
                        let words = if self.avatar_pick.is_some() {
                            "CHOOSING…"
                        } else if self.avatar.is_some() {
                            "SQUARED OFF AND KEPT AS PROFILE/AVATAR.PNG"
                        } else {
                            "PICK ONE FROM ANYWHERE ON THIS MACHINE"
                        };
                        self.fonts.draw(scene, dim, bx, y + self.px(14.0), words);
                        let word = if self.avatar.is_some() { "CHOOSE ANOTHER" } else { "CHOOSE A PICTURE" };
                        let fw = self.fonts.measure(strong, word) + self.px(24.0);
                        self.me_button(scene, r.right() - pad - fw, y + self.px(18.0), word, false, CardHit::PickPicture);
                    }
                    Face::Initial => {
                        for l in crate::reader::wrap(&self.fonts, dim, "THE FIRST LETTER OF YOUR NAME, IN THE SPACE'S SIGNAL", bw) {
                            self.fonts.draw(scene, dim, bx, y + self.px(14.0), &l);
                            y += self.px(15.0);
                        }
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
                for l in crate::reader::wrap(&self.fonts, dim, "SYNC NAMES WHAT THIS MACHINE WROTE BY IT · IT NEVER SYNCS ITSELF", bw) {
                    self.fonts.draw(scene, dim, bx, y + self.px(18.0), &l);
                    y += self.px(15.0);
                }
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
                if !editing && !has_me && !self.me_card.first {
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
                    self.fonts.draw(scene, Style { color: if on { self.on_fill(ink) } else { ink }, ..label }, x + self.px(10.0), chip.y + chip.h / 2.0 + self.px(4.0), kind.name());
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
                    self.fonts.draw(scene, Style { color: if on { self.on_fill(ink) } else { ink }, ..label }, x + self.px(10.0), chip.y + chip.h / 2.0 + self.px(4.0), word);
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
            Some(Step::Terms) => {
                let title = Style { font: self.f.serif, px: self.px(if roomy { 26.0 } else { 20.0 }), color: ink, tracking: 0.0 };
                self.fonts.draw(scene, title, bx, y + self.px(20.0), "The fine print");
                y += self.px(38.0);
                // Three tabs: privacy, terms, licenses.
                let mut tx = bx;
                for (k, word) in ["PRIVACY", "TERMS", "LICENSES"].iter().enumerate() {
                    let on = self.me_card.terms_tab == k as u8;
                    let tw = self.fonts.measure(label, word) + self.px(22.0);
                    let chip = Rect::new(tx, y, tw, self.px(24.0));
                    if on { scene.rect(chip, ink); } else { scene.outline(chip, self.px(m::HAIRLINE), ink); }
                    self.fonts.draw(scene, Style { color: if on { t.paper } else { ink }, ..label }, tx + self.px(11.0), chip.y + self.px(16.0), word);
                    self.me_card.hits.push((chip, CardHit::TermsTab(k as u8)));
                    tx += tw + self.px(8.0);
                }
                y += self.px(36.0);
                let (lead, lines) = fine_print(self.me_card.terms_tab);
                for l in crate::reader::wrap(&self.fonts, ui_strong, lead, bw) {
                    self.fonts.draw(scene, ui_strong, bx, y + self.px(12.0), &l);
                    y += self.px(19.0);
                }
                y += self.px(8.0);
                // The document itself, scrolling in its own box.
                let boxr = Rect::new(bx, y, bw, (foot_base - self.px(34.0) - y).max(self.px(60.0)));
                scene.outline(boxr, self.px(m::HAIRLINE), t.dim);
                let inner = Rect::new(boxr.x + self.px(12.0), boxr.y + self.px(4.0), boxr.w - self.px(24.0), boxr.h - self.px(8.0));
                let body = Style { px: self.px(12.0), ..ui };
                let lh = self.px(17.0);
                let mut wrapped: Vec<String> = Vec::new();
                for line in &lines {
                    if line.is_empty() { wrapped.push(String::new()); continue; }
                    wrapped.extend(crate::reader::wrap(&self.fonts, body, line, inner.w));
                }
                let total = wrapped.len() as f32 * lh + self.px(16.0);
                self.me_card.terms_reach = (total - inner.h).max(0.0);
                self.me_card.terms_scroll = self.me_card.terms_scroll.min(self.me_card.terms_reach);
                scene.layer(Some(inner));
                let mut ly = inner.y + self.px(16.0) - self.me_card.terms_scroll;
                for l in &wrapped {
                    if ly > inner.y - lh && ly < inner.bottom() + lh {
                        self.fonts.draw(scene, body, inner.x, ly, l);
                    }
                    ly += lh;
                }
                scene.layer(None);
                if self.me_card.terms_reach > 0.0 {
                    let k = self.me_card.terms_scroll / self.me_card.terms_reach;
                    let th = (inner.h * inner.h / total).max(self.px(24.0));
                    scene.rect(Rect::new(boxr.right() - self.px(4.0), inner.y + (inner.h - th) * k, self.px(2.0), th), t.dim);
                }
                let mut x = bx;
                x += self.me_button(scene, x, foot_base, "AGREE AND FINISH", true, CardHit::Agree) + self.px(10.0);
                self.me_button(scene, x, foot_base, "BACK", false, CardHit::Back);
                let st = Style { color: t.dim, ..label };
                let note = "SCROLL TO READ · ALSO IN SETTINGS › PROFILE";
                let nw = self.fonts.measure(st, note);
                if x + self.px(120.0) + nw < r.right() - pad {
                    self.fonts.draw(scene, st, r.right() - pad - nw, foot_base, note);
                }
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
                let words = if self.sync_ready() {
                    format!("{how} The avatar in the footer opens this card; MORE takes you to the full page under settings.")
                } else {
                    format!("{how} you to the full page under settings — profile, sync, and what lives in the folder.")
                };
                for l in crate::reader::wrap(&self.fonts, ui, &words, bw) {
                    self.fonts.draw(scene, ui, bx, y + self.px(12.0), &l);
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
