//! The settings tab (Ctrl+,): a ruled nav on the left, one section at a
//! time on the right. Every control is a click target recorded in
//! `App::settings_hits` — choice chips, sliders, swatches, buttons — so the
//! page is native chrome like everything else. The Luau file is the other
//! way in; the RULES section shows it and reloads it.

use crate::app::{App, Pane, SettingsPane};
use crate::anim::{BarColor, BarStyle};
use crate::surface::{self, Fullscreen, HoverFrom, OpacityOn, Shell, Side, TextureKind, TextureOn, SWATCHES};
use nus_render::text::icons;
use nus_render::Style;
use nus_render::theme::metric as m;
use nus_render::{Color, Rect, Scene};

/// Where links a page opens go (target=_blank, window.open).
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Links {
    Stack,
    Split,
    NewTab,
}

/// Where a URL typed at a prompt goes.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum PromptUrl {
    Split,
    NewTab,
}

/// Tab and terminal behaviour the settings page edits.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Behavior {
    pub links: Links,
    pub prompt_url: PromptUrl,
    /// Closing a tab with a foreground process asks first.
    pub close_asks: bool,
    pub default_profile: usize,
    pub follow_os_theme: bool,
    /// The Start modal at launch, and its chime.
    pub start_on_launch: bool,
    pub startup_sound: bool,
}

impl Default for Behavior {
    fn default() -> Self {
        Behavior { links: Links::Stack, prompt_url: PromptUrl::Split, close_asks: true, default_profile: 0, follow_os_theme: true, start_on_launch: false, startup_sound: false }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Slider {
    Tint,
    Texture,
    Opacity,
    ShellWidth,
    Radius,
    Grace,
    Motion,
    BarThickness,
    BarChase,
    TexScale,
    Angle,
    Drift,
    Breath,
    Volume,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Hit {
    Section(usize),
    Theme(Option<bool>),
    Signal(Color),
    Base(Option<Color>),
    Shell(Shell),
    /// A slider bar: kind, bar x, bar width.
    Slider(Slider, f32, f32),
    Side(Side),
    HoverFrom(HoverFrom),
    Fullscreen(Fullscreen),
    Pin(bool),
    Links(Links),
    PromptUrl(PromptUrl),
    CloseAsks(bool),
    DefaultProfile(usize),
    ReloadRules,
    OpenRules,
    ResetRules,
    Reduce(Option<bool>),
    BarStyle(BarStyle),
    BarColor(BarColor),
    /// Tile grid → section, and back.
    Tile(usize),
    Back,
    MakeDefault,
    Unregister,
    StartOnLaunch(bool),
    StartupSound(bool),
    ReloadAvatar,
    OpenProfileDir,
    Preset(usize),
    SavePreset,
    OpenPresets,
    StopSel(usize),
    StopAdd,
    StopRemove,
    StopColor(Color),
    OpacityOn(OpacityOn),
    TexKind(TextureKind),
    TexOn(TextureOn),
    SoundOn(bool),
    /// Play a cue by index into sound::NAMES.
    Play(usize),
    /// Event (index into sound::EVENTS) → cue index, or usize::MAX for quiet.
    EventCue(usize, usize),
    EventNext(usize),
}

pub const SECTIONS: [(&str, (&str, &str)); 11] = [
    ("APPEARANCE", icons::BRUSH),
    ("SURFACE", icons::PALETTE),
    ("SOUND", icons::SPEAKER),
    ("SIDEBAR", icons::SIDEBAR),
    ("TABS", icons::SQUARES),
    ("TERMINAL", icons::TERMINAL),
    ("BROWSER", icons::GLOBE),
    ("ASSISTANTS", icons::ASSISTANT),
    ("RULES", icons::CODE),
    ("KEYS", icons::KEYBOARD),
    ("UPDATES", icons::DOWNLOAD),
];

pub const SEC_SOUND: usize = 2;
pub const SEC_TERMINAL: usize = 5;
pub const SEC_BROWSER: usize = 6;
pub const RULES: usize = 8;

fn key(k: &str, shift: bool) -> String {
    if cfg!(target_os = "macos") {
        format!("⌘{}{}", if shift { "⇧" } else { "" }, k)
    } else {
        format!("CTRL+{}{}", if shift { "SHIFT+" } else { "" }, k)
    }
}

/// One row's control.
enum Control {
    Info(String),
    Choice(Vec<(String, Hit, bool)>),
    Slider(Slider, f32, String),
    Swatches(Vec<(Option<Color>, Hit, bool)>),
    Buttons(Vec<(String, (&'static str, &'static str), Hit)>),
}

impl App {
    pub(crate) fn slider_value(&self, s: Slider) -> f32 {
        match s {
            Slider::Tint => self.surface.tint,
            Slider::Texture => self.surface.texture / 0.3,
            Slider::Opacity => (self.surface.opacity - 0.5) / 0.5,
            Slider::ShellWidth => (self.surface.shell_width - 1.0) / 11.0,
            Slider::Radius => self.surface.shell_radius / 24.0,
            Slider::Grace => self.sidebar_rules.grace_ms as f32 / 1000.0,
            Slider::Motion => self.motion.register,
            Slider::BarThickness => (self.load_bar.thickness - 1.0) / 5.0,
            Slider::BarChase => (self.load_bar.chase - 2.0) / 14.0,
            Slider::TexScale => (self.surface.texture_scale - 1.0) / 9.0,
            Slider::Angle => self.surface.angle / 360.0,
            Slider::Drift => self.surface.drift / 0.5,
            Slider::Breath => self.surface.breath,
            Slider::Volume => self.sound.prefs.volume,
        }
    }

    fn set_slider(&mut self, s: Slider, v: f32) {
        let v = v.clamp(0.0, 1.0);
        match s {
            Slider::Tint => self.surface.tint = v,
            Slider::Texture => self.surface.texture = v * 0.3,
            Slider::Opacity => self.surface.opacity = 0.5 + v * 0.5,
            Slider::ShellWidth => self.surface.shell_width = (1.0 + v * 11.0).round(),
            Slider::Radius => self.surface.shell_radius = (v * 24.0).round(),
            Slider::Grace => self.sidebar_rules.grace_ms = (v * 1000.0).round() as u64,
            Slider::Motion => self.motion.register = v,
            Slider::BarThickness => self.load_bar.thickness = (1.0 + v * 5.0).round(),
            Slider::BarChase => self.load_bar.chase = (2.0 + v * 14.0).round(),
            Slider::TexScale => self.surface.texture_scale = (1.0 + v * 9.0 * 2.0).round() / 2.0,
            Slider::Angle => self.surface.angle = (v * 360.0 / 15.0).round() * 15.0 % 360.0,
            Slider::Drift => self.surface.drift = (v * 0.5 * 100.0).round() / 100.0,
            Slider::Breath => self.surface.breath = (v * 20.0).round() / 20.0,
            Slider::Volume => {
                self.sound.prefs.volume = (v * 20.0).round() / 20.0;
                self.sound.cue("tick");
            }
        }
        self.layout();
    }

    /// Registration is a `reg query` away; ask once per visit, not per frame.
    pub(crate) fn refresh_register_note(&mut self) {
        self.register_note = if crate::little::registered() {
            "registered as a browser · links from other apps open little".into()
        } else if cfg!(target_os = "windows") {
            "not registered · links from other apps would open elsewhere".into()
        } else {
            "registration needs an app bundle (macOS) or .desktop file (Linux) · v1".into()
        };
    }

    /// A click inside the settings pane. Returns true when it was handled.
    pub(crate) fn settings_click(&mut self, x: f32, y: f32) -> bool {
        let Some(tab) = self.tabs.get(self.active) else { return false };
        let Pane::Settings(s) = &tab.left else { return false };
        if !s.rect.contains(x, y) {
            return false;
        }
        let Some(&(_, hit)) = self.settings_hits.iter().find(|(r, _)| r.contains(x, y)) else { return true };
        match hit {
            Hit::Play(_) | Hit::EventCue(..) | Hit::EventNext(_) | Hit::SoundOn(_) | Hit::Slider(..) => {}
            Hit::ReloadRules | Hit::OpenRules | Hit::ResetRules | Hit::MakeDefault | Hit::Unregister | Hit::ReloadAvatar | Hit::OpenProfileDir | Hit::SavePreset | Hit::OpenPresets | Hit::StopAdd | Hit::StopRemove => {
                self.play_event("control.press")
            }
            _ => self.play_event("toggle"),
        }
        self.apply_setting(hit, x);
        self.save_prefs();
        self.dirty = true;
        true
    }

    /// A spoken label for a control (AccessKit).
    pub(crate) fn setting_label(&self, hit: Hit) -> String {
        match hit {
            Hit::Section(k) | Hit::Tile(k) => SECTIONS[k].0.to_lowercase(),
            Hit::Back => "back to settings".into(),
            Hit::Theme(None) => "theme follows the OS".into(),
            Hit::Theme(Some(true)) => "ink theme".into(),
            Hit::Theme(Some(false)) => "paper theme".into(),
            Hit::Signal(c) => format!("signal {}", surface::hex(c)),
            Hit::Base(None) => "no base".into(),
            Hit::Base(Some(c)) => format!("base {}", surface::hex(c)),
            Hit::Shell(s) => format!("carapace {}", s.name()),
            Hit::Slider(k, _, _) => format!("{:?}", k).to_lowercase(),
            Hit::Side(s) => format!("sidebar {:?}", s).to_lowercase(),
            Hit::HoverFrom(h) => format!("reveal from {:?}", h).to_lowercase(),
            Hit::Fullscreen(f) => format!("fullscreen {:?}", f).to_lowercase(),
            Hit::Pin(p) => if p { "pin sidebar".into() } else { "sidebar on hover".into() },
            Hit::Links(l) => format!("links {:?}", l).to_lowercase(),
            Hit::PromptUrl(p) => format!("url at prompt {:?}", p).to_lowercase(),
            Hit::CloseAsks(a) => if a { "ask before closing a busy tab".into() } else { "never ask".into() },
            Hit::DefaultProfile(i) => format!("default shell {}", self.profiles.get(i).map(|p| p.name.as_str()).unwrap_or("")),
            Hit::ReloadRules => "reload rules".into(),
            Hit::OpenRules => "open rules in editor".into(),
            Hit::ResetRules => "reset rules to default".into(),
            Hit::Reduce(None) => "reduce motion follows the OS".into(),
            Hit::Reduce(Some(r)) => format!("reduce motion {}", if r { "on" } else { "off" }),
            Hit::Preset(k) => format!("preset {}", surface::presets().get(k).map(|p| p.name.clone()).unwrap_or_default()),
            Hit::SavePreset => "save this surface as a preset".into(),
            Hit::OpenPresets => "open the presets folder".into(),
            Hit::StopSel(i) => format!("stop {}", i + 1),
            Hit::StopAdd => "add a stop".into(),
            Hit::StopRemove => "remove a stop".into(),
            Hit::StopColor(c) => format!("stop colour {}", surface::hex(c)),
            Hit::SoundOn(b) => if b { "sound on".into() } else { "sound off".into() },
            Hit::Play(i) => format!("play {}", crate::sound::NAMES.get(i).copied().unwrap_or("")),
            Hit::EventCue(e, c) => format!("{} → {}", crate::sound::EVENTS[e].0, if c == usize::MAX { "quiet" } else { crate::sound::NAMES[c] }),
            Hit::EventNext(e) => format!("{}: next cue", crate::sound::EVENTS[e].0),
            Hit::OpacityOn(o) => format!("opacity on {:?}", o).to_lowercase(),
            Hit::TexKind(k) => format!("texture {}", k.name()),
            Hit::TexOn(o) => format!("texture on {:?}", o).to_lowercase(),
            Hit::ReloadAvatar => "reload avatar".into(),
            Hit::OpenProfileDir => "open the profile folder".into(),
            Hit::StartOnLaunch(b) => if b { "atlas also at launch".into() } else { "atlas from the planet".into() },
            Hit::StartupSound(b) => if b { "startup sound on".into() } else { "startup sound off".into() },
            Hit::MakeDefault => "make nus the default browser".into(),
            Hit::Unregister => "unregister nus as a browser".into(),
            Hit::BarStyle(b) => format!("loading bar {}", b.name()),
            Hit::BarColor(c) => format!("bar colour {:?}", c).to_lowercase(),
        }
    }

    pub(crate) fn apply_setting(&mut self, hit: Hit, x: f32) {
        match hit {
            Hit::Section(k) => {
                if let Some(Pane::Settings(s)) = self.tabs.get_mut(self.active).map(|t| &mut t.left) {
                    s.section = k;
                    s.scroll = 0.0;
                }
                if k == SEC_BROWSER {
                    self.refresh_register_note();
                }
            }
            Hit::Theme(None) => self.behavior.follow_os_theme = true,
            Hit::Theme(Some(ink)) => {
                self.behavior.follow_os_theme = false;
                self.set_theme(if ink { nus_render::Theme::ink() } else { nus_render::Theme::paper() });
                self.refresh_icon();
            }
            Hit::Signal(c) => {
                self.surface.signal = c;
                self.refresh_icon();
            }
            Hit::Base(b) => {
                self.surface.base = b;
                if b.is_some() && self.surface.tint == 0.0 {
                    self.surface.tint = 0.35;
                }
                self.refresh_icon();
            }
            Hit::Shell(sh) => {
                self.surface.shell = sh;
                self.layout();
            }
            Hit::Slider(kind, x0, w) => self.set_slider(kind, (x - x0) / w),
            Hit::Side(side) => {
                self.sidebar_rules.side = side;
                self.sidebar_hover = false;
                self.layout();
            }
            Hit::HoverFrom(h) => self.sidebar_rules.hover_from = h,
            Hit::Fullscreen(f) => {
                self.sidebar_rules.fullscreen = f;
                self.layout();
            }
            Hit::Pin(p) => {
                self.sidebar = p;
                self.sidebar_hover = false;
                self.layout();
            }
            Hit::Links(l) => self.behavior.links = l,
            Hit::PromptUrl(p) => self.behavior.prompt_url = p,
            Hit::CloseAsks(a) => self.behavior.close_asks = a,
            Hit::DefaultProfile(i) => self.behavior.default_profile = i,
            Hit::ReloadRules => self.rules.reload(),
            Hit::OpenRules => {
                let path = self.rules.path.to_string_lossy().to_string();
                let cmd = if cfg!(target_os = "windows") {
                    format!("start \"\" \"{path}\"")
                } else if cfg!(target_os = "macos") {
                    format!("open \"{path}\"")
                } else {
                    format!("xdg-open \"{path}\"")
                };
                self.run_in_shell(&cmd);
            }
            Hit::ResetRules => {
                let _ = std::fs::write(&self.rules.path, surface::DEFAULT_RULES);
                self.rules.reload();
            }
            Hit::Tile(k) => {
                if let Some(Pane::Settings(s)) = self.tabs.get_mut(self.active).map(|t| &mut t.left) {
                    s.section = k;
                    s.drill = true;
                }
                if k == SEC_BROWSER {
                    self.refresh_register_note();
                }
            }
            Hit::Back => {
                if let Some(Pane::Settings(s)) = self.tabs.get_mut(self.active).map(|t| &mut t.left) {
                    s.drill = false;
                }
            }
            Hit::MakeDefault => match crate::little::register() {
                Ok(()) => self.register_note = "registered · pick nus in Windows Settings".into(),
                Err(e) => {
                    tracing::warn!("register: {e}");
                    self.register_note = e;
                }
            },
            Hit::Unregister => {
                let _ = crate::little::unregister();
                self.register_note = "unregistered".into();
            }
            Hit::Preset(k) => {
                if let Some(p) = surface::presets().get(k) {
                    self.surface = p.surface.clone();
                    self.preset_name = p.name.clone();
                    self.refresh_icon();
                    self.layout();
                }
            }
            Hit::SavePreset => {
                let n = surface::presets().len() + 1;
                let name = format!("mine-{n}");
                let p = surface::Preset { name: name.clone(), surface: self.surface.clone() };
                if surface::save_preset(&p).is_ok() {
                    self.preset_name = name;
                }
            }
            Hit::OpenPresets => {
                let dir = std::env::current_dir().unwrap_or_default().join("profile").join("surfaces");
                let _ = std::fs::create_dir_all(&dir);
                let cmd = if cfg!(target_os = "windows") {
                    format!("start \"\" \"{}\"", dir.display())
                } else if cfg!(target_os = "macos") {
                    format!("open \"{}\"", dir.display())
                } else {
                    format!("xdg-open \"{}\"", dir.display())
                };
                self.run_in_shell(&cmd);
            }
            Hit::StopSel(i) => self.stop_sel = i,
            Hit::StopAdd => {
                if self.surface.stops.len() < 4 {
                    let ink = self.theme.ink;
                    let mut stops = self.surface.ramp(ink);
                    let last = *stops.last().unwrap();
                    stops.push(surface::rotate_hue(last, 0.12));
                    self.surface.stops = stops;
                    self.stop_sel = self.surface.stops.len() - 1;
                }
            }
            Hit::StopRemove => {
                if self.surface.stops.len() > 2 {
                    self.surface.stops.pop();
                    self.stop_sel = self.stop_sel.min(self.surface.stops.len() - 1);
                } else {
                    self.surface.stops.clear();
                    self.stop_sel = 0;
                }
            }
            Hit::StopColor(c) => {
                let ink = self.theme.ink;
                if self.surface.stops.len() < 2 {
                    self.surface.stops = self.surface.ramp(ink);
                }
                let i = self.stop_sel.min(self.surface.stops.len() - 1);
                self.surface.stops[i] = c;
            }
            Hit::SoundOn(b) => {
                self.sound.prefs.enabled = b;
                if b {
                    self.sound.cue("chime");
                }
            }
            Hit::Play(i) => {
                if let Some(n) = crate::sound::NAMES.get(i) {
                    self.sound.cue(n);
                }
            }
            Hit::EventCue(e, c) => {
                let ev = crate::sound::EVENTS[e].0.to_string();
                let cue = if c == usize::MAX { String::new() } else { crate::sound::NAMES[c].to_string() };
                if !cue.is_empty() {
                    self.sound.cue(&cue);
                }
                self.sound.prefs.map.insert(ev, cue);
            }
            Hit::EventNext(e) => {
                let ev = crate::sound::EVENTS[e].0;
                let cur = self.sound.prefs.cue_for(ev);
                let idx = cur.as_deref().and_then(|c| crate::sound::NAMES.iter().position(|n| *n == c));
                let next = match idx {
                    Some(i) if i + 1 < crate::sound::NAMES.len() => Some(i + 1),
                    Some(_) => None,
                    None => Some(0),
                };
                let cue = next.map(|i| crate::sound::NAMES[i].to_string()).unwrap_or_default();
                if !cue.is_empty() {
                    self.sound.cue(&cue);
                }
                self.sound.prefs.map.insert(ev.to_string(), cue);
            }
            Hit::OpacityOn(o) => self.surface.opacity_on = o,
            Hit::TexKind(k) => {
                self.surface.texture_kind = k;
                if k != TextureKind::None && self.surface.texture == 0.0 {
                    self.surface.texture = 0.08;
                }
            }
            Hit::TexOn(o) => self.surface.texture_on = o,
            Hit::ReloadAvatar => self.load_avatar(),
            Hit::OpenProfileDir => {
                let dir = std::env::current_dir().unwrap_or_default().join("profile");
                let cmd = if cfg!(target_os = "windows") {
                    format!("start \"\" \"{}\"", dir.display())
                } else if cfg!(target_os = "macos") {
                    format!("open \"{}\"", dir.display())
                } else {
                    format!("xdg-open \"{}\"", dir.display())
                };
                self.run_in_shell(&cmd);
            }
            Hit::StartOnLaunch(b) => self.behavior.start_on_launch = b,
            Hit::StartupSound(b) => {
                self.behavior.startup_sound = b;
                if b {
                    self.play_event("launch");
                }
            }
            Hit::Reduce(r) => self.motion.reduce = r,
            Hit::BarStyle(b) => self.load_bar.style = b,
            Hit::BarColor(c) => self.load_bar.color = c,
        }
    }

    fn rows_for(&self, section: usize) -> Vec<(String, Control)> {
        use Control::*;
        let hex = surface::hex;
        let ink = self.theme.mode == nus_render::Mode::Ink;
        match section {
            0 => vec![
                (
                    "THEME".into(),
                    Choice(vec![
                        ("FOLLOW OS".into(), Hit::Theme(None), self.behavior.follow_os_theme),
                        ("PAPER".into(), Hit::Theme(Some(false)), !self.behavior.follow_os_theme && !ink),
                        ("INK".into(), Hit::Theme(Some(true)), !self.behavior.follow_os_theme && ink),
                    ]),
                ),
                (
                    "MOTION".into(),
                    Slider(
                        self::Slider::Motion,
                        self.slider_value(self::Slider::Motion),
                        format!("{} · snappy ← → cinematic · sidebar {}ms", self.motion.name(), (self.motion.dur(crate::anim::base::SIDEBAR) * 1000.0).round()),
                    ),
                ),
                (
                    "REDUCE MOTION".into(),
                    Choice(vec![
                        (format!("FOLLOW OS · {}", if crate::anim::os_reduce_motion() { "ON" } else { "OFF" }), Hit::Reduce(None), self.motion.reduce.is_none()),
                        ("OFF".into(), Hit::Reduce(Some(false)), self.motion.reduce == Some(false)),
                        ("ON".into(), Hit::Reduce(Some(true)), self.motion.reduce == Some(true)),
                    ]),
                ),
                (
                    "ATLAS".into(),
                    Choice(vec![
                        ("FROM THE PLANET".into(), Hit::StartOnLaunch(false), !self.behavior.start_on_launch),
                        ("ALSO AT LAUNCH".into(), Hit::StartOnLaunch(true), self.behavior.start_on_launch),
                    ]),
                ),
                (
                    "STARTUP SOUND".into(),
                    Choice(vec![
                        ("OFF".into(), Hit::StartupSound(false), !self.behavior.startup_sound),
                        ("ON · THE LAUNCH CUE".into(), Hit::StartupSound(true), self.behavior.startup_sound),
                    ]),
                ),
                ("UI FONT".into(), Info("IBM Plex Mono · 13 / 1.5 · any installed mono via init.luau".into())),
                ("TERMINAL FONT".into(), Info("IBM Plex Mono · 13pt · ligatures on".into())),
                ("WORDMARK".into(), Info("Newsreader Italic".into())),
                ("CURSOR".into(), Info("block · no blink".into())),
            ],
            1 => {
                let ink = self.theme.ink;
                let presets = surface::presets();
                let mut preset_chips: Vec<(String, Hit, bool)> =
                    presets.iter().enumerate().map(|(k, p)| (p.name.to_uppercase(), Hit::Preset(k), p.name == self.preset_name)).collect();
                preset_chips.push(("+ SAVE AS…".into(), Hit::SavePreset, false));
                let sig: Vec<(Option<Color>, Hit, bool)> =
                    SWATCHES[..6].iter().map(|&(_, c)| (Some(c), Hit::Signal(c), c == self.surface.signal)).collect();
                let fam: Vec<(Option<Color>, Hit, bool)> = surface::family(self.surface.signal).iter().map(|&c| (Some(c), Hit::Signal(c), false)).collect();
                let ramp = self.surface.ramp(ink);
                let stop_sel = self.stop_sel.min(ramp.len() - 1);
                let mut stops: Vec<(Option<Color>, Hit, bool)> = ramp.iter().enumerate().map(|(i, &c)| (Some(c), Hit::StopSel(i), i == stop_sel)).collect();
                stops.push((None, Hit::StopAdd, false));
                let mut stop_colors: Vec<(Option<Color>, Hit, bool)> =
                    SWATCHES.iter().map(|&(_, c)| (Some(c), Hit::StopColor(c), ramp.get(stop_sel) == Some(&c))).collect();
                stop_colors.extend(surface::family(self.surface.signal).iter().map(|&c| (Some(c), Hit::StopColor(c), false)));
                let mut base: Vec<(Option<Color>, Hit, bool)> = vec![(None, Hit::Base(None), self.surface.base.is_none())];
                base.extend(SWATCHES.iter().map(|&(_, c)| (Some(c), Hit::Base(Some(c)), self.surface.base == Some(c))));
                let translucent = self.target.translucent();
                vec![
                    ("PRESET".into(), Choice(preset_chips)),
                    ("".into(), Buttons(vec![("OPEN PRESETS FOLDER".into(), icons::FOLDER, Hit::OpenPresets)])),
                    ("SIGNAL".into(), Swatches(sig)),
                    ("FAMILY".into(), Swatches(fam)),
                    ("SIGNAL HEX".into(), Info(format!("{} · carapace, Space square, ticks, progress · family: tints and shades", hex(self.surface.signal)))),
                    ("STOPS".into(), Swatches(stops)),
                    (
                        format!("STOP {} COLOUR", stop_sel + 1),
                        Swatches(stop_colors),
                    ),
                    (
                        "".into(),
                        Buttons(vec![("REMOVE LAST STOP".into(), icons::MINIMIZE, Hit::StopRemove)]),
                    ),
                    ("BASE".into(), Swatches(base)),
                    (
                        "TINT".into(),
                        Slider(
                            self::Slider::Tint,
                            self.slider_value(self::Slider::Tint),
                            match self.surface.base {
                                Some(_) => format!("{}% toward {}", (self.surface.tint * 100.0).round(), hex(self.paper())),
                                None => "pick a base first".into(),
                            },
                        ),
                    ),
                    (
                        "OPACITY".into(),
                        Slider(
                            self::Slider::Opacity,
                            self.slider_value(self::Slider::Opacity),
                            if translucent {
                                format!("{}%", (self.surface.opacity * 100.0).round())
                            } else {
                                "opaque swapchain on this compositor · v1".into()
                            },
                        ),
                    ),
                    (
                        "".into(),
                        Choice(vec![
                            ("PANES".into(), Hit::OpacityOn(OpacityOn::Panes), self.surface.opacity_on == OpacityOn::Panes),
                            ("CHROME TOO".into(), Hit::OpacityOn(OpacityOn::Chrome), self.surface.opacity_on == OpacityOn::Chrome),
                            ("WHOLE WINDOW".into(), Hit::OpacityOn(OpacityOn::Window), self.surface.opacity_on == OpacityOn::Window),
                        ]),
                    ),
                    (
                        "TEXTURE".into(),
                        Choice(TextureKind::ALL.iter().map(|&k| (k.name().to_uppercase(), Hit::TexKind(k), k == self.surface.texture_kind)).collect()),
                    ),
                    (
                        "STRENGTH".into(),
                        Slider(self::Slider::Texture, self.slider_value(self::Slider::Texture), format!("{}%", (self.surface.texture * 100.0).round())),
                    ),
                    (
                        "SCALE".into(),
                        Slider(self::Slider::TexScale, self.slider_value(self::Slider::TexScale), format!("{}px pitch", self.surface.texture_scale)),
                    ),
                    (
                        "ON".into(),
                        Choice(vec![
                            ("CARAPACE".into(), Hit::TexOn(TextureOn::Carapace), self.surface.texture_on == TextureOn::Carapace),
                            ("CHROME".into(), Hit::TexOn(TextureOn::Chrome), self.surface.texture_on == TextureOn::Chrome),
                            ("PANES".into(), Hit::TexOn(TextureOn::Panes), self.surface.texture_on == TextureOn::Panes),
                        ]),
                    ),
                    (
                        "CARAPACE".into(),
                        Choice(Shell::ALL.iter().map(|&s| (s.name().to_uppercase(), Hit::Shell(s), s == self.surface.shell)).collect()),
                    ),
                    (
                        "WIDTH".into(),
                        Slider(self::Slider::ShellWidth, self.slider_value(self::Slider::ShellWidth), format!("{}px", self.surface.shell_width)),
                    ),
                    (
                        "RADIUS".into(),
                        Slider(self::Slider::Radius, self.slider_value(self::Slider::Radius), format!("{}px corners", self.surface.shell_radius)),
                    ),
                    (
                        "ANGLE".into(),
                        Slider(self::Slider::Angle, self.slider_value(self::Slider::Angle), format!("{}° · gradient and aurora", self.surface.angle)),
                    ),
                    (
                        "DRIFT".into(),
                        Slider(self::Slider::Drift, self.slider_value(self::Slider::Drift), format!("{} turns/s · aurora", self.surface.drift)),
                    ),
                    (
                        "BREATH".into(),
                        Slider(self::Slider::Breath, self.slider_value(self::Slider::Breath), format!("{}% · the aurora stroke swells", (self.surface.breath * 100.0).round())),
                    ),
                ]
            }
            2 => {
                let on = self.sound.prefs.enabled;
                let mut rows: Vec<(String, Control)> = vec![
                    (
                        "SOUND".into(),
                        Choice(vec![("ON".into(), Hit::SoundOn(true), on), ("OFF".into(), Hit::SoundOn(false), !on)]),
                    ),
                    (
                        "VOLUME".into(),
                        Slider(self::Slider::Volume, self.slider_value(self::Slider::Volume), format!("{}%", (self.sound.prefs.volume * 100.0).round())),
                    ),
                    (
                        "".into(),
                        Info(if self.sound.player.is_some() { "cuelume's seventeen cues · synthesized here · click one to hear it".into() } else { "no audio output device found".into() }),
                    ),
                ];
                // The palette, in rows of six.
                for chunk in (0..crate::sound::NAMES.len()).collect::<Vec<_>>().chunks(6) {
                    rows.push((
                        if chunk[0] == 0 { "THE PALETTE".into() } else { "".into() },
                        Choice(chunk.iter().map(|&i| (crate::sound::NAMES[i].to_uppercase(), Hit::Play(i), false)).collect()),
                    ));
                }
                rows.push(("".into(), Info("what plays when · QUIET silences an event · NEXT walks the palette".into())));
                for (e, (ev, _, note)) in crate::sound::EVENTS.iter().enumerate() {
                    let cur = self.sound.prefs.cue_for(ev);
                    let mut chips = vec![("QUIET".into(), Hit::EventCue(e, usize::MAX), cur.is_none())];
                    if let Some(c) = &cur {
                        let ci = crate::sound::NAMES.iter().position(|n| n == c).unwrap_or(0);
                        chips.push((c.to_uppercase(), Hit::EventCue(e, ci), true));
                    }
                    chips.push(("NEXT ▸".into(), Hit::EventNext(e), false));
                    if !note.is_empty() {
                        chips.push((note.to_uppercase(), Hit::EventNext(e), false));
                    }
                    rows.push((ev.replace('.', " · ").to_uppercase(), Choice(chips)));
                }
                rows.push(("".into(), Info("rules.luau can override any event with on_event · cues by daniel belyi (cuelume, mit)".into())));
                rows
            }
            3 => vec![
                (
                    "SIDE".into(),
                    Choice(vec![
                        ("LEFT".into(), Hit::Side(Side::Left), self.sidebar_rules.side == Side::Left),
                        ("RIGHT".into(), Hit::Side(Side::Right), self.sidebar_rules.side == Side::Right),
                    ]),
                ),
                (
                    "REVEAL".into(),
                    Choice(vec![
                        ("SCREEN EDGE".into(), Hit::HoverFrom(HoverFrom::ScreenEdge), self.sidebar_rules.hover_from == HoverFrom::ScreenEdge),
                        ("INSIDE WINDOW ONLY".into(), Hit::HoverFrom(HoverFrom::InsideWindow), self.sidebar_rules.hover_from == HoverFrom::InsideWindow),
                    ]),
                ),
                (
                    "GRACE".into(),
                    Slider(self::Slider::Grace, self.slider_value(self::Slider::Grace), format!("{}ms after the pointer leaves", self.sidebar_rules.grace_ms)),
                ),
                (
                    "FULLSCREEN".into(),
                    Choice(vec![
                        ("HOVER".into(), Hit::Fullscreen(Fullscreen::Hover), self.sidebar_rules.fullscreen == Fullscreen::Hover),
                        ("HIDDEN".into(), Hit::Fullscreen(Fullscreen::Hidden), self.sidebar_rules.fullscreen == Fullscreen::Hidden),
                        ("PINNED".into(), Hit::Fullscreen(Fullscreen::Pinned), self.sidebar_rules.fullscreen == Fullscreen::Pinned),
                    ]),
                ),
                (
                    "NOW".into(),
                    Choice(vec![
                        ("PINNED".into(), Hit::Pin(true), self.sidebar),
                        (format!("HOVER · {} PINS", key("S", true)), Hit::Pin(false), !self.sidebar),
                    ]),
                ),
                ("ROWS".into(), Info("compact · preview on hover and while waiting".into())),
            ],
            4 => vec![
                (
                    "LINKS FROM PAGES".into(),
                    Choice(vec![
                        ("IN THE STACK".into(), Hit::Links(Links::Stack), self.behavior.links == Links::Stack),
                        ("IN THE SPLIT".into(), Hit::Links(Links::Split), self.behavior.links == Links::Split),
                        ("NEW TAB".into(), Hit::Links(Links::NewTab), self.behavior.links == Links::NewTab),
                    ]),
                ),
                (
                    "URL AT A PROMPT".into(),
                    Choice(vec![
                        ("OPENS BESIDE".into(), Hit::PromptUrl(PromptUrl::Split), self.behavior.prompt_url == PromptUrl::Split),
                        ("NEW TAB".into(), Hit::PromptUrl(PromptUrl::NewTab), self.behavior.prompt_url == PromptUrl::NewTab),
                    ]),
                ),
                (
                    "CLOSING".into(),
                    Choice(vec![
                        ("ASK WHEN BUSY".into(), Hit::CloseAsks(true), self.behavior.close_asks),
                        ("NEVER ASK".into(), Hit::CloseAsks(false), !self.behavior.close_asks),
                    ]),
                ),
                ("STACKS".into(), Info("one level · collapse when not active · closing the parent asks".into())),
                ("NUMBERS".into(), Info(format!("{} → the stack, at its last-used member", key("1–9", false)))),
                ("COLOURS".into(), Info("new tabs are coloured by rules.luau → RULES".into())),
            ],
            5 => {
                let mut v: Vec<(String, Control)> = vec![(
                    "DEFAULT SHELL".into(),
                    Choice(
                        self.profiles
                            .iter()
                            .enumerate()
                            .map(|(i, p)| (p.name.to_uppercase(), Hit::DefaultProfile(i), i == self.behavior.default_profile))
                            .collect(),
                    ),
                )];
                for p in &self.profiles {
                    v.push((format!("PROFILE · {}", p.name.to_uppercase()), Info(format!("{} {}", p.program, p.args.join(" ")))));
                }
                v.push((
                    "AVATAR".into(),
                    Buttons(vec![("RELOAD".into(), icons::RELOAD, Hit::ReloadAvatar), ("OPEN PROFILE FOLDER".into(), icons::FOLDER, Hit::OpenProfileDir)]),
                ));
                v.push(("".into(), Info(if self.avatar.is_some() { "profile/avatar.png · shown in the sidebar".into() } else { "drop a PNG at profile/avatar.png, then reload".into() })));
                v.push(("SCROLLBACK".into(), Info("10 000 lines · restored with the session".into())));
                v.push(("ATTENTION".into(), Info("BEL and OSC 133 mark a tab WAITING while it is not active".into())));
                v.push(("ENV".into(), Info("TERM=xterm-256color · COLORTERM=truecolor · TERM_PROGRAM=nus".into())));
                v
            }
            6 => vec![
                (
                    "LOADING BAR".into(),
                    Choice(BarStyle::ALL.iter().map(|&b| (b.name().to_uppercase(), Hit::BarStyle(b), b == self.load_bar.style)).collect()),
                ),
                (
                    "BAR COLOUR".into(),
                    Choice(vec![
                        ("SIGNAL".into(), Hit::BarColor(BarColor::Signal), self.load_bar.color == BarColor::Signal),
                        ("TAB".into(), Hit::BarColor(BarColor::Tab), self.load_bar.color == BarColor::Tab),
                        ("INK".into(), Hit::BarColor(BarColor::Ink), self.load_bar.color == BarColor::Ink),
                    ]),
                ),
                (
                    "BAR WEIGHT".into(),
                    Slider(self::Slider::BarThickness, self.slider_value(self::Slider::BarThickness), format!("{}px", self.load_bar.thickness)),
                ),
                (
                    "BAR CHASE".into(),
                    Slider(
                        self::Slider::BarChase,
                        self.slider_value(self::Slider::BarChase),
                        format!("{} · how eagerly it follows real progress", self.load_bar.chase),
                    ),
                ),
                (
                    "DEFAULT BROWSER".into(),
                    Buttons(vec![("MAKE DEFAULT".into(), icons::GLOBE, Hit::MakeDefault), ("UNREGISTER".into(), icons::CLOSE, Hit::Unregister)]),
                ),
                (
                    "".into(),
                    Info(self.register_note.clone()),
                ),
                ("SEARCH".into(), Info("google · configurable".into())),
                ("NEW TAB".into(), Info("opens the palette; no new-tab page".into())),
                ("COOKIES".into(), Info("one jar per Space · third-party blocked (v1)".into())),
                ("DOWNLOADS".into(), Info("~/Downloads · silent · ruled toast (v1)".into())),
                ("PASSWORDS".into(), Info("1Password via op (v1)".into())),
                ("ENGINE".into(), Info(format!("Chromium {}", crate::chromium_version()))),
            ],
            7 => {
                let mut v: Vec<(String, Control)> =
                    self.llm_tools.iter().map(|(n, c)| (format!("LOCAL · {}", n.to_uppercase()), Info(c.clone()))).collect();
                if v.is_empty() {
                    v.push(("LOCAL".into(), Info("none on PATH (claude, codex, ollama are detected)".into())));
                }
                v.push(("WEB · CHATGPT".into(), Info("https://chatgpt.com/?q=…".into())));
                v.push(("WEB · CLAUDE".into(), Info("https://claude.ai/new?q=…".into())));
                v
            }
            8 => vec![
                ("FILE".into(), Info(self.rules.path.to_string_lossy().to_string())),
                ("STATUS".into(), Info(self.rules.status.clone())),
                (
                    "".into(),
                    Buttons(vec![
                        ("RELOAD".into(), icons::RELOAD, Hit::ReloadRules),
                        ("OPEN IN EDITOR".into(), icons::OPEN_EXTERNAL, Hit::OpenRules),
                        ("RESET TO DEFAULT".into(), icons::WARNING, Hit::ResetRules),
                    ]),
                ),
            ],
            9 => vec![
                ("NEW TAB".into(), Info(key("T", true))),
                ("GO".into(), Info(key("K", true))),
                ("URL".into(), Info(key("L", true))),
                ("CLOSE".into(), Info(key("W", true))),
                ("REOPEN CLOSED".into(), Info(key("Z", true))),
                ("SPLIT".into(), Info(key("D", true))),
                ("SIDEBAR".into(), Info(key("S", true))),
                ("DEVTOOLS".into(), Info(key("I", true))),
                ("TAB N".into(), Info(key("1–9", false))),
                ("MRU".into(), Info(key("`", false))),
                ("PREV / NEXT".into(), Info(key("PGUP / PGDN", false))),
                ("SETTINGS".into(), Info(key(",", false))),
                ("FULLSCREEN".into(), Info("F11".into())),
            ],
            _ => vec![
                ("CHANNEL".into(), Info("GitHub Releases · self-update (v1)".into())),
                ("TELEMETRY".into(), Info("none".into())),
                ("VERSION".into(), Info(format!("nus spike 4 · CEF {}", crate::chromium_version()))),
            ],
        }
    }

    /// One-line hint under each tile.
    fn tile_hint(&self, k: usize) -> String {
        match k {
            0 => format!("{} · {}", if self.theme.mode == nus_render::Mode::Ink { "ink" } else { "paper" }, self.motion.name()),
            1 => format!("{} · {}", surface::hex(self.surface.signal), self.surface.shell.name()),
            2 => if self.sound.prefs.enabled { format!("on · {}%", (self.sound.prefs.volume * 100.0).round()) } else { "off".into() },
            3 => format!("{:?} · {:?}", self.sidebar_rules.side, self.sidebar_rules.fullscreen).to_lowercase(),
            4 => format!("links → {:?}", self.behavior.links).to_lowercase(),
            5 => self.profiles.get(self.behavior.default_profile).map(|p| p.name.clone()).unwrap_or_default(),
            6 => format!("{} bar · google", self.load_bar.style.name()),
            7 => format!("{} local · chatgpt · claude", self.llm_tools.len()),
            8 => self.rules.status.clone(),
            9 => "chords".into(),
            _ => "github releases".into(),
        }
    }

    /// The tile grid: icon, name, a one-line state; 44px+ targets.
    fn draw_tiles(&mut self, scene: &mut Scene, r: Rect) {
        let t = self.theme.clone();
        let ink = t.ink;
        let label = self.label();
        let strong = self.label_strong();
        let dim = Style { color: t.dim, ..label };
        let pad = self.px(18.0);
        let mut y = r.y + self.px(28.0);
        let wm = Style { font: self.f.wordmark, px: self.px(34.0), color: ink, tracking: 0.0 };
        self.fonts.draw(scene, wm, r.x + pad, y + self.px(30.0), "settings");
        y += self.px(58.0);
        let cols = if r.w / self.scale < 560.0 { 2 } else { 3 };
        let gap = self.px(12.0);
        let tw = ((r.w - 2.0 * pad - gap * (cols as f32 - 1.0)) / cols as f32).floor();
        let th = self.px(96.0);
        let isz = self.px(22.0);
        for (k, (name, icon)) in SECTIONS.iter().enumerate() {
            let col = k % cols;
            let row = k / cols;
            let tile = Rect::new(r.x + pad + col as f32 * (tw + gap), y + row as f32 * (th + gap), tw, th);
            if tile.bottom() > r.bottom() {
                break;
            }
            scene.outline(tile, self.px(m::HAIRLINE), ink);
            self.fonts.draw_icon(scene, *icon, isz, tile.x + self.px(16.0), tile.y + self.px(16.0), ink);
            let base = tile.y + self.px(16.0) + isz + self.px(22.0);
            self.fonts.draw(scene, strong, tile.x + self.px(16.0), base, name);
            let hint = self.fit(dim, &self.tile_hint(k), tw - self.px(32.0));
            self.fonts.draw(scene, dim, tile.x + self.px(16.0), base + self.px(18.0), &hint);
            self.settings_hits.push((tile, Hit::Tile(k)));
        }
    }

    pub(crate) fn draw_settings(&mut self, scene: &mut Scene, p: &SettingsPane) {
        let t = self.theme.clone();
        let ink = t.ink;
        let label = self.label();
        let strong = self.label_strong();
        let ui = self.ui();
        let dim = Style { color: t.dim, ..label };
        let r = p.rect;
        self.settings_hits.clear();

        // Narrow panes get tiles instead of a nav: a grid first, then one
        // section under a back crumb. Width decides; there is no manual mode.
        let tiles = r.w / self.scale < crate::app::NARROW;
        if tiles && !p.drill {
            return self.draw_tiles(scene, r);
        }

        // Nav (or, drilled in, a back crumb).
        let nav_w = if tiles { 0.0 } else { self.px(220.0) };
        let mut top = r.y;
        if tiles {
            let bh = self.px(12.0) * 2.0 + self.px(m::LABEL_PX) + self.px(m::HAIRLINE);
            let isz = self.px(14.0);
            let base = r.y + self.px(12.0) + self.px(m::LABEL_PX) - self.px(2.0);
            self.fonts.draw_icon(scene, icons::BACK, isz, r.x + self.px(18.0), base - isz + self.px(2.0), ink);
            self.fonts.draw(scene, label, r.x + self.px(18.0) + isz + self.px(10.0), base, "SETTINGS");
            scene.hline(r.x, r.y + bh - self.px(m::HAIRLINE), r.w, self.px(m::HAIRLINE), ink);
            self.settings_hits.push((Rect::new(r.x, r.y, r.w, bh), Hit::Back));
            top += bh;
        } else {
            scene.vline(r.x + nav_w, r.y, r.h, self.px(m::STRUCTURE), ink);
        }
        let sh = self.px(12.0) * 2.0 + self.px(m::LABEL_PX) + self.px(m::HAIRLINE);
        let isz = self.px(14.0);
        for (i, (name, icon)) in SECTIONS.iter().enumerate().filter(|_| !tiles) {
            let y = r.y + i as f32 * sh;
            let sel = i == p.section;
            let row = Rect::new(r.x, y, nav_w, sh);
            if sel {
                scene.rect(Rect::new(r.x, y, nav_w, sh - self.px(m::HAIRLINE)), ink);
            }
            let col = if sel { t.paper } else { ink };
            let base = y + self.px(12.0) + self.px(m::LABEL_PX) - self.px(2.0);
            self.fonts.draw_icon(scene, *icon, isz, r.x + self.px(18.0), base - isz + self.px(2.0), col);
            self.fonts.draw(scene, Style { color: col, ..label }, r.x + self.px(18.0) + isz + self.px(10.0), base, name);
            scene.hline(r.x, y + sh - self.px(m::HAIRLINE), nav_w, self.px(m::HAIRLINE), ink);
            self.settings_hits.push((row, Hit::Section(i)));
        }
        if !tiles {
            let cfg = "~/.config/nus/init.luau";
            self.fonts.draw(scene, dim, r.x + self.px(18.0), r.bottom() - self.px(14.0), cfg);
        }

        // Content, scrolling within its column.
        let cx = r.x + nav_w + if tiles { self.px(18.0) } else { self.px(40.0) };
        let content = Rect::new(r.x + nav_w, top, r.w - nav_w, r.bottom() - top);
        let scroll = p.scroll.max(0.0);
        scene.layer(Some(content));
        let mut y = top + self.px(28.0) - scroll;
        let wm = Style { font: self.f.wordmark, px: self.px(34.0), color: ink, tracking: 0.0 };
        self.fonts.draw(scene, wm, cx, y + self.px(30.0), &SECTIONS[p.section].0.to_lowercase());
        y += self.px(58.0);
        let maxw = (r.w - nav_w - if tiles { self.px(36.0) } else { self.px(80.0) }).min(self.px(760.0));
        let label_w = if tiles { self.px(140.0) } else { self.px(200.0) };
        let rows = self.rows_for(p.section);
        for (k, control) in rows {
            let rh = match &control {
                Control::Swatches(_) => self.px(12.0) * 2.0 + self.px(18.0) + self.px(m::HAIRLINE),
                _ => self.px(10.0) * 2.0 + self.px(m::UI_PX) + self.px(m::HAIRLINE),
            };
            let base = y + self.px(10.0) + self.px(m::UI_PX) - self.px(3.0);
            self.fonts.draw(scene, dim, cx, base, &k);
            let vx = cx + label_w;
            match control {
                Control::Info(v) => {
                    let vs = self.fit(ui, &v, maxw - label_w);
                    self.fonts.draw(scene, ui, vx, base, &vs);
                }
                Control::Choice(opts) => {
                    let mut x = vx;
                    for (text, hit, on) in opts {
                        let w = self.fonts.measure(label, &text) + self.px(20.0);
                        let chip = Rect::new(x, base - self.px(m::LABEL_PX) - self.px(6.0), w, self.px(m::LABEL_PX) + self.px(12.0));
                        if on {
                            scene.rect(chip, ink);
                        } else {
                            scene.outline(chip, self.px(m::HAIRLINE), ink);
                        }
                        let st = Style { color: if on { t.paper } else { ink }, ..label };
                        self.fonts.draw(scene, st, x + self.px(10.0), base - self.px(1.0), &text);
                        self.settings_hits.push((chip, hit));
                        x += w + self.px(8.0);
                    }
                }
                Control::Slider(kind, v, text) => {
                    let bw = self.px(200.0);
                    let bar = Rect::new(vx, base - self.px(6.0), bw, self.px(2.0));
                    scene.rect(bar, t.tint);
                    scene.rect(Rect::new(vx, bar.y, bw * v, bar.h), self.surface.signal);
                    let knob = self.px(10.0);
                    scene.rect(Rect::new(vx + bw * v - knob / 2.0, bar.y - knob / 2.0 + bar.h / 2.0, knob, knob), ink);
                    self.settings_hits.push((Rect::new(vx - knob, bar.y - self.px(12.0), bw + 2.0 * knob, self.px(26.0)), Hit::Slider(kind, vx, bw)));
                    let ts = self.fit(dim, &text, maxw - label_w - bw - self.px(20.0));
                    self.fonts.draw(scene, dim, vx + bw + self.px(20.0), base, &ts);
                }
                Control::Swatches(items) => {
                    let sz = self.px(18.0);
                    let mut x = vx;
                    let sy = y + self.px(12.0);
                    for (c, hit, on) in items {
                        let sw = Rect::new(x, sy, sz, sz);
                        match c {
                            Some(c) => scene.rect(sw, c),
                            None => {
                                scene.outline(sw, self.px(m::HAIRLINE), ink);
                                // "none": a diagonal hairline.
                                scene.push(nus_render::Instance::hazard(sw, sz, t.tint, t.paper, sz * 2.0));
                            }
                        }
                        if on {
                            scene.outline(Rect::new(x - self.px(3.0), sy - self.px(3.0), sz + self.px(6.0), sz + self.px(6.0)), self.px(m::STRUCTURE), ink);
                        }
                        self.settings_hits.push((Rect::new(x - self.px(4.0), sy - self.px(4.0), sz + self.px(8.0), sz + self.px(8.0)), hit));
                        x += sz + self.px(12.0);
                    }
                }
                Control::Buttons(items) => {
                    let mut x = vx;
                    for (text, icon, hit) in items {
                        let isz = self.px(13.0);
                        let w = self.fonts.measure(strong, &text) + self.px(24.0) + isz + self.px(8.0);
                        let b = Rect::new(x, base - self.px(m::LABEL_PX) - self.px(8.0), w, self.px(m::LABEL_PX) + self.px(16.0));
                        scene.rect(Rect::new(b.x + self.px(3.0), b.y + self.px(3.0), b.w, b.h), ink);
                        scene.rect(b, t.paper);
                        scene.outline(b, self.px(m::STRUCTURE), ink);
                        self.fonts.draw_icon(scene, icon, isz, x + self.px(12.0), base - isz + self.px(2.0), ink);
                        self.fonts.draw(scene, strong, x + self.px(12.0) + isz + self.px(8.0), base, &text);
                        self.settings_hits.push((b, hit));
                        x += w + self.px(14.0);
                    }
                }
            }
            scene.hline(cx, y + rh - self.px(m::HAIRLINE), maxw, self.px(m::HAIRLINE), t.tint);
            y += rh;
        }
        // Remember the reach so the wheel can clamp.
        self.settings_reach = (y + scroll - top + self.px(40.0)).max(0.0);
        scene.layer(None);
        // Hits above or below the column are unreachable.
        self.settings_hits.retain(|(hr, h)| matches!(h, Hit::Section(_) | Hit::Back | Hit::Tile(_)) || (hr.bottom() > content.y && hr.y < content.bottom()));

        // RULES: the file itself, as far as it fits.
        if p.section == RULES {
            y += self.px(16.0);
            let code = Style { font: self.f.ui, px: self.px(11.5), color: ink, tracking: 0.0 };
            let lh = self.px(11.5 * 1.55);
            let src = self.rules.source.clone();
            let clip = Rect::new(cx, y, maxw, r.bottom() - y - self.px(20.0));
            scene.layer(Some(clip));
            let mut ly = y + self.px(12.0);
            for (n, line) in src.lines().enumerate() {
                if ly > clip.bottom() {
                    break;
                }
                let comment = line.trim_start().starts_with("--");
                let st = if comment { Style { color: t.dim, ..code } } else { code };
                self.fonts.draw(scene, Style { color: t.dim, ..code }, cx, ly, &format!("{:>3}", n + 1));
                let text = self.fit(st, line, maxw - self.px(40.0));
                self.fonts.draw(scene, st, cx + self.px(36.0), ly, &text);
                ly += lh;
            }
            scene.layer(None);
        }
    }
}
