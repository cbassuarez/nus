//! The settings tab (Ctrl+,): a ruled nav on the left, one section at a
//! time on the right. Every control is a click target recorded in
//! `App::settings_hits` — choice chips, sliders, swatches, buttons — so the
//! page is native chrome like everything else. The Luau file is the other
//! way in; the RULES section shows it and reloads it.

use crate::anim::Anim;
use crate::app::{Caps, fade, hover_key, Hover};
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

/// How loud the prompt line's language server is.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub enum PromptLsp {
    Off,
    /// A dotted underline for a diagnostic, hover for the message;
    /// completions ride the ghost and Tab accepts.
    #[default]
    Quiet,
    /// A small completion list under the caret.
    Menu,
}

/// Where a URL typed at a prompt goes.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum PromptUrl {
    Split,
    NewTab,
}

/// The sidebar header: what it does is what it's called.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum HeaderStyle {
    /// One ruled row: the window's name and NEW TAB.
    Bar,
    /// A rail of windows along the sidebar's edge; the name above the tabs.
    Rail,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct HeaderPrefs {
    pub style: HeaderStyle,
    /// The name set large in Newsreader (masthead) instead of caps.
    pub masthead: bool,
    /// A dateline under the name: where · tabs · ports.
    pub dateline: bool,
    /// NEW TAB in the header row.
    pub header_button: bool,
    /// The ruled line after the last tab is also NEW TAB.
    pub next_row: bool,
    /// The window's name in the bar's cell (else just its square).
    pub show_name: bool,
    /// A split caret on NEW TAB that fans the kinds out.
    pub kinds_caret: bool,
    /// The rail only while the pointer is over the sidebar.
    pub rail_hover: bool,
    /// Press feedback on NEW TAB: a flash through the signal.
    pub flash: bool,
}

impl Default for HeaderPrefs {
    fn default() -> Self {
        HeaderPrefs { style: HeaderStyle::Bar, masthead: false, dateline: false, header_button: true, next_row: true, show_name: true, kinds_caret: true, rail_hover: false, flash: true }
    }
}

/// The terminal cursor, the app's way.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum CursorShapePref {
    Shell,
    Block,
    Beam,
    Underline,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Blink {
    Never,
    AfterIdle,
    Always,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum CursorColor {
    Ink,
    Signal,
    Tab,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum CursorMotion {
    Jump,
    Glide,
    Comet,
    /// Neovide's smear: the body stretches from where it was to where it
    /// goes, the tail catching up a beat behind the head.
    Smear,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Pointer {
    System,
    InkArrow,
    SignalDot,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CursorPrefs {
    pub shape: CursorShapePref,
    pub blink: Blink,
    /// Blink period, ms.
    pub period: u32,
    pub color: CursorColor,
    pub motion: CursorMotion,
    /// Beam / underline weight, logical px.
    pub weight: f32,
    pub hollow_unfocused: bool,
    pub pointer: Pointer,
    pub hide_while_typing: bool,
    /// Neovide's trail_size: how far the smear's tail lags its head, 0..1.
    #[serde(default = "default_smear")]
    pub smear: f32,
}

fn default_smear() -> f32 {
    1.0
}

impl Default for CursorPrefs {
    fn default() -> Self {
        CursorPrefs { shape: CursorShapePref::Shell, blink: Blink::Never, period: 530, color: CursorColor::Ink, motion: CursorMotion::Jump, weight: 2.0, hollow_unfocused: true, pointer: Pointer::System, hide_while_typing: true, smear: 1.0 }
    }
}

/// How the window comes up.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum WindowStart {
    Last,
    Maximized,
    Fullscreen,
    Centered,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum SplashMode {
    Draw,
    Still,
    None,
}

/// What happens once the splash has gone.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Then {
    Restore,
    Shell,
    LastPage,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum AtlasMode {
    Planet,
    AtLaunch,
    Persistent,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Outside {
    Little,
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
    #[serde(default = "default_window_start")]
    pub window_start: WindowStart,
    #[serde(default = "default_splash")]
    pub splash: SplashMode,
    /// Seconds the splash holds at least.
    #[serde(default = "default_splash_hold")]
    pub splash_hold: f32,
    #[serde(default = "default_then")]
    pub then: Then,
    #[serde(default = "default_atlas")]
    pub atlas: AtlasMode,
    #[serde(default = "default_outside")]
    pub outside: Outside,
    /// Shells get prompt marks, cwd and exit codes injected at spawn.
    #[serde(default = "default_true")]
    pub shell_integration: bool,
    /// Colour the command line's tokens as you type.
    #[serde(default = "default_true")]
    pub highlight: bool,
    /// The editor formats through the language server on Ctrl+S.
    #[serde(default = "default_true")]
    pub format_on_save: bool,
    /// The prompt line's language server: quiet, a menu, or off.
    #[serde(default)]
    pub prompt_lsp: PromptLsp,
    /// Ghost the history entry that continues what's typed; Right/End accepts.
    #[serde(default = "default_true")]
    pub predict: bool,
    /// Refuse requests to known ad and tracker hosts.
    #[serde(default = "default_true")]
    pub block_content: bool,
    /// Blank an idle page after this many minutes (0 = never).
    #[serde(default = "default_sleep")]
    pub sleep_after_min: u32,
    /// Archive an idle page tab into recently closed after this many hours (0 = never).
    #[serde(default = "default_archive")]
    pub archive_after_h: u32,
    /// The page's status at the end of its tools row: a lamp, the word, or nothing.
    #[serde(default)]
    pub status: Status,
    /// A selection in the shell copies itself as it ends.
    #[serde(default)]
    pub copy_on_select: bool,
    /// The middle button pastes into the shell.
    #[serde(default)]
    pub middle_paste: bool,
    /// What programs in the shell may do with the clipboard through OSC 52.
    #[serde(default)]
    pub osc52: Osc52,
    /// The cluster at a split pane's corner: on hover, always, never.
    #[serde(default)]
    pub pane_controls: crate::panes::Controls,
    /// The rule between a split's panes drags to resize.
    #[serde(default = "default_true")]
    pub pane_divider: bool,
    /// How the shell's view moves: neoscroll's curves, or at once.
    #[serde(default)]
    pub scroll_easing: crate::scrolling::Easing,
    /// Lines per wheel tick in the shell.
    #[serde(default = "default_wheel_lines")]
    pub wheel_lines: u32,
    /// Chromium's smooth scrolling in pages (takes a restart).
    #[serde(default = "default_true")]
    pub page_smooth_scroll: bool,
}

fn default_wheel_lines() -> u32 {
    3
}

/// OSC 52: tmux, neovim and friends setting (and reading) the clipboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Osc52 {
    Off,
    /// Programs may set the clipboard; reading it back is refused.
    #[default]
    Write,
    /// Programs may set it and read it.
    ReadWrite,
}

/// How a page says it's live, loading, local or asleep.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Status {
    /// A small lamp: signal while loading, ink when live, hazard when local, hollow asleep.
    #[default]
    Lamp,
    /// The lamp and the word.
    Both,
    /// The word alone (LIVE · LOCAL · ASLEEP).
    Word,
    None,
}

fn default_sleep() -> u32 {
    30
}
fn default_archive() -> u32 {
    12
}

fn default_true() -> bool {
    true
}

fn default_window_start() -> WindowStart {
    WindowStart::Last
}
fn default_splash() -> SplashMode {
    SplashMode::Draw
}
fn default_splash_hold() -> f32 {
    1.0
}
fn default_then() -> Then {
    Then::Shell
}
fn default_atlas() -> AtlasMode {
    AtlasMode::Planet
}
fn default_outside() -> Outside {
    Outside::Little
}

impl Default for Behavior {
    fn default() -> Self {
        Behavior {
            links: Links::Stack,
            prompt_url: PromptUrl::Split,
            close_asks: true,
            default_profile: 0,
            follow_os_theme: true,
            start_on_launch: false,
            startup_sound: false,
            window_start: WindowStart::Last,
            splash: SplashMode::Draw,
            splash_hold: 1.0,
            then: Then::Shell,
            atlas: AtlasMode::Planet,
            outside: Outside::Little,
            shell_integration: true,
            highlight: true,
            format_on_save: true,
            prompt_lsp: PromptLsp::Quiet,
            predict: true,
            block_content: true,
            sleep_after_min: 30,
            archive_after_h: 12,
            status: Status::Lamp,
            copy_on_select: false,
            middle_paste: false,
            osc52: Osc52::Write,
            pane_controls: crate::panes::Controls::Near,
            pane_divider: true,
            scroll_easing: crate::scrolling::Easing::Cubic,
            wheel_lines: 3,
            page_smooth_scroll: true,
        }
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
    SplashHold,
    Saturation,
    BlinkPeriod,
    CurWeight,
    Smear,
    Hue,
    Sat,
    Light,
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
    TexMotion(bool),
    TokPaper(Color),
    TokInk(Color),
    TokPage(Color),
    TokReset,
    AnsiSel(usize),
    AnsiSet(Color),
    Family(crate::theme_edit::Family),
    Import(usize),
    OpenThemes,
    Starter(usize),
    LookTab(usize),
    TokSel(TokSel),
    /// Set the selected token to this colour.
    TokSet(Color),
    ShellInt(bool),
    Welcome,
    Block(bool),
    StatusStyle(Status),
    SleepAfter(u32),
    ArchiveAfter(u32),
    Highlight(bool),
    CopyOnSelect(bool),
    PaneControls(crate::panes::Controls),
    ScrollEasing(crate::scrolling::Easing),
    WheelLines(u32),
    PageSmooth(bool),
    PaneDivider(bool),
    MiddlePaste(bool),
    Osc52(Osc52),
    Predict(bool),
    PromptLsp(PromptLsp),
    FormatOnSave(bool),
    HdrStyle(HeaderStyle),
    HdrMasthead(bool),
    HdrDateline(bool),
    HdrButton(bool),
    HdrNextRow(bool),
    HdrName(bool),
    HdrCaret(bool),
    HdrRailHover(bool),
    HdrFlash(bool),
    Compact(bool),
    CurShape(CursorShapePref),
    CurBlink(Blink),
    CurColor(CursorColor),
    CurMotion(CursorMotion),
    CurHollow(bool),
    CurPointer(Pointer),
    CurHide(bool),
    WindowStart(WindowStart),
    Splash(SplashMode),
    Then(Then),
    Atlas(AtlasMode),
    Outside(Outside),
    LoginItem(bool),
    SoundOn(bool),
    /// Play a cue by index into sound::NAMES.
    Play(usize),
    /// Event (index into sound::EVENTS) → cue index, or usize::MAX for quiet.
    EventCue(usize, usize),
    EventNext(usize),
}

/// The sections, grouped by what they're about: how nus looks, how it
/// feels, what you work in, and the machine.
pub const GROUPS: [(&str, std::ops::Range<usize>); 4] = [("LOOK", 0..1), ("FEEL", 1..5), ("WORK", 5..9), ("SYSTEM", 9..11)];

/// The look studio's tabs.
pub const LOOK_TABS: [&str; 5] = ["PRESETS", "SURFACE", "TOKENS", "TYPE & MOTION", "CURSOR"];
pub const LOOK_PRESETS: usize = 0;
pub const LOOK_SURFACE: usize = 1;
pub const LOOK_TOKENS: usize = 2;
pub const LOOK_TYPE: usize = 3;
pub const LOOK_CURSOR: usize = 4;

/// Which token the picker is editing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TokSel {
    Signal,
    Stop(usize),
    Paper,
    Ink,
    Page,
    Ansi(usize),
}

pub const SECTIONS: [(&str, (&str, &str)); 11] = [
    ("LOOK", icons::PALETTE),
    ("SOUND", icons::SPEAKER),
    ("STARTUP", icons::ROCKET),
    ("SIDEBAR", icons::SIDEBAR),
    ("TABS", icons::SQUARES),
    ("TERMINAL", icons::TERMINAL),
    ("BROWSER", icons::GLOBE),
    ("ASSISTANTS", icons::ASSISTANT),
    ("RULES", icons::CODE),
    ("KEYS", icons::KEYBOARD),
    ("UPDATES", icons::DOWNLOAD),
];

pub const SEC_LOOK: usize = 0;
pub const SEC_SOUND: usize = 1;
pub const SEC_STARTUP: usize = 2;
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
    /// The live proof of the current look: a miniature window.
    Studio,
    /// The studio's tab strip.
    Strip(Vec<(String, Hit, bool)>),
    /// Preset cards: name, ramp, signal, hit, current.
    Cards(Vec<(String, Vec<Color>, Color, f32, Hit, bool, Option<(Color, Color, Color, Color)>)>),
    /// Token tiles: name, colour (None = dashed "none/add"), caption, hit, selected, big.
    Tokens(Vec<(String, Option<Color>, String, Hit, bool)>, bool),
    Info(String),
    Choice(Vec<(String, Hit, bool)>),
    Slider(Slider, f32, String),
    Swatches(Vec<(Option<Color>, Hit, bool)>),
    Buttons(Vec<(String, (&'static str, &'static str), Hit)>),
    /// Coloured runs of text, as proof.
    Proof(Vec<(Color, String)>),
    /// A mini sidebar: (bg, signal, title, child) rows the rules produced.
    Tabs(Vec<(Option<Color>, Option<Color>, String, bool)>),
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
            Slider::Smear => self.cursor.smear,
            Slider::Motion => self.motion.register,
            Slider::BarThickness => (self.load_bar.thickness - 1.0) / 5.0,
            Slider::BarChase => (self.load_bar.chase - 2.0) / 14.0,
            Slider::TexScale => (self.surface.texture_scale - 1.0) / 9.0,
            Slider::Angle => self.surface.angle / 360.0,
            Slider::Drift => self.surface.drift / 0.5,
            Slider::Breath => self.surface.breath,
            Slider::Volume => self.sound.prefs.volume,
            Slider::SplashHold => (self.behavior.splash_hold - 0.4) / 2.2,
            Slider::Saturation => (self.theme_edit.saturation - 0.5) / 1.0,
            Slider::Hue => surface::to_hsl(self.tok_color()).0,
            Slider::Sat => surface::to_hsl(self.tok_color()).1,
            Slider::Light => surface::to_hsl(self.tok_color()).2,
            Slider::BlinkPeriod => (self.cursor.period as f32 - 200.0) / 1000.0,
            Slider::CurWeight => (self.cursor.weight - 1.0) / 5.0,
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
            Slider::Smear => self.cursor.smear = v.clamp(0.1, 0.9),
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
            Slider::SplashHold => self.behavior.splash_hold = (0.4 + v * 2.2 * 10.0).round() / 10.0,
            Slider::Saturation => {
                self.theme_edit.saturation = (0.5 + v * 20.0).round() / 20.0;
                self.rebuild_theme();
            }
            Slider::Hue | Slider::Sat | Slider::Light => {
                let (h, sa, l) = surface::to_hsl(self.tok_color());
                let c = match s {
                    Slider::Hue => surface::from_hsl(v, sa.max(0.02), l, 1.0),
                    Slider::Sat => surface::from_hsl(h, v, l, 1.0),
                    _ => surface::from_hsl(h, sa, v, 1.0),
                };
                self.set_tok(c);
            }
            Slider::BlinkPeriod => self.cursor.period = ((200.0 + v * 1000.0) / 10.0).round() as u32 * 10,
            Slider::CurWeight => self.cursor.weight = (1.0 + v * 5.0 * 2.0).round() / 2.0,
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
            Hit::Compact(c) => (if c { "compact sidebar" } else { "full sidebar" }).into(),
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
            Hit::Preset(k) => format!("theme {}", crate::themes::all().get(k).map(|p| p.name.clone()).unwrap_or_default()),
            Hit::SavePreset => "save the look as a theme".into(),
            Hit::OpenPresets => "open the presets folder".into(),
            Hit::StopSel(i) => format!("stop {}", i + 1),
            Hit::StopAdd => "add a stop".into(),
            Hit::StopRemove => "remove a stop".into(),
            Hit::StopColor(c) => format!("stop colour {}", surface::hex(c)),
            Hit::TokPaper(c) => format!("paper {}", surface::hex(c)),
            Hit::TokInk(c) => format!("ink {}", surface::hex(c)),
            Hit::TokPage(c) => format!("page {}", surface::hex(c)),
            Hit::TokReset => "reset this mode's tokens".into(),
            Hit::AnsiSel(i) => format!("ansi {i}"),
            Hit::AnsiSet(c) => format!("set to {}", surface::hex(c)),
            Hit::Family(f) => format!("family {}", f.name()),
            Hit::Import(k) => format!("import {}", crate::theme_edit::imports().get(k).map(|t| t.name.clone()).unwrap_or_default()),
            Hit::OpenThemes => "open the themes folder".into(),
            Hit::Starter(k) => format!("start from {}", surface::STARTERS.get(k).map(|s| s.0).unwrap_or("")),
            Hit::LookTab(k) => LOOK_TABS.get(k).map(|t| t.to_lowercase()).unwrap_or_default(),
            Hit::TokSel(t) => format!("edit {:?}", t).to_lowercase(),
            Hit::TokSet(c) => format!("set to {}", surface::hex(c)),
            Hit::ShellInt(b) => if b { "shell integration auto".into() } else { "shell integration off".into() },
            Hit::Welcome => "open the welcome page".into(),
            Hit::Block(b) => if b { "content blocking on".into() } else { "content blocking off".into() },
            Hit::StatusStyle(s) => format!("page status {:?}", s).to_lowercase(),
            Hit::SleepAfter(n) => if n == 0 { "never sleep tabs".into() } else { format!("sleep after {n} minutes") },
            Hit::ArchiveAfter(n) => if n == 0 { "never archive".into() } else { format!("archive after {n} hours") },
            Hit::Highlight(b) => if b { "highlight the command line".into() } else { "plain command line".into() },
            Hit::CopyOnSelect(b) => if b { "copy on select on".into() } else { "copy on select off".into() },
            Hit::PaneControls(c) => format!("pane controls {:?}", c).to_lowercase(),
            Hit::ScrollEasing(e) => format!("scroll {}", e.name()),
            Hit::WheelLines(n) => format!("{n} lines per wheel tick"),
            Hit::PageSmooth(b) => if b { "smooth page scrolling".into() } else { "instant page scrolling".into() },
            Hit::PaneDivider(b) => if b { "pane divider drags".into() } else { "pane divider fixed".into() },
            Hit::MiddlePaste(b) => if b { "middle click pastes".into() } else { "middle click does nothing".into() },
            Hit::Osc52(o) => format!("osc 52 {:?}", o).to_lowercase(),
            Hit::Predict(b) => if b { "predictions on".into() } else { "predictions off".into() },
            Hit::PromptLsp(m) => match m { PromptLsp::Quiet => "prompt lsp quiet".into(), PromptLsp::Menu => "prompt lsp menu".into(), PromptLsp::Off => "prompt lsp off".into() },
            Hit::FormatOnSave(b) => if b { "format on save".into() } else { "save as is".into() },
            Hit::HdrStyle(s) => format!("header {:?}", s).to_lowercase(),
            Hit::HdrMasthead(b) => if b { "masthead title".into() } else { "caps title".into() },
            Hit::HdrDateline(b) => if b { "dateline on".into() } else { "dateline off".into() },
            Hit::HdrButton(b) => if b { "new tab in the header".into() } else { "no new tab in the header".into() },
            Hit::HdrNextRow(b) => if b { "next row is new tab".into() } else { "next row off".into() },
            Hit::HdrName(b) => if b { "window name shown".into() } else { "window square only".into() },
            Hit::HdrCaret(b) => if b { "kinds caret on".into() } else { "kinds caret off".into() },
            Hit::HdrRailHover(b) => if b { "rail on hover".into() } else { "rail always".into() },
            Hit::HdrFlash(b) => if b { "press flash".into() } else { "no press flash".into() },
            Hit::CurShape(s) => format!("cursor {:?}", s).to_lowercase(),
            Hit::CurBlink(b) => format!("blink {:?}", b).to_lowercase(),
            Hit::CurColor(c) => format!("cursor colour {:?}", c).to_lowercase(),
            Hit::CurMotion(m) => format!("cursor motion {:?}", m).to_lowercase(),
            Hit::CurHollow(h) => if h { "hollow when unfocused".into() } else { "hidden when unfocused".into() },
            Hit::CurPointer(p) => format!("pointer {:?}", p).to_lowercase(),
            Hit::CurHide(h) => if h { "hide the pointer while typing".into() } else { "keep the pointer while typing".into() },
            Hit::WindowStart(w) => format!("window {:?}", w).to_lowercase(),
            Hit::Splash(m) => format!("splash {:?}", m).to_lowercase(),
            Hit::Then(t) => format!("then {:?}", t).to_lowercase(),
            Hit::Atlas(a) => format!("atlas {:?}", a).to_lowercase(),
            Hit::Outside(o) => format!("links from outside {:?}", o).to_lowercase(),
            Hit::LoginItem(on) => if on { "start with the system".into() } else { "do not start with the system".into() },
            Hit::SoundOn(b) => if b { "sound on".into() } else { "sound off".into() },
            Hit::Play(i) => format!("play {}", crate::sound::NAMES.get(i).copied().unwrap_or("")),
            Hit::EventCue(e, c) => format!("{} → {}", crate::sound::EVENTS[e].0, if c == usize::MAX { "quiet" } else { crate::sound::NAMES[c] }),
            Hit::EventNext(e) => format!("{}: next cue", crate::sound::EVENTS[e].0),
            Hit::OpacityOn(o) => format!("opacity on {:?}", o).to_lowercase(),
            Hit::TexKind(k) => format!("texture {}", k.name()),
            Hit::TexOn(o) => format!("texture on {:?}", o).to_lowercase(),
            Hit::TexMotion(b) => if b { "texture animated".into() } else { "texture still".into() },
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
                self.set_mode(if ink { nus_render::Mode::Ink } else { nus_render::Mode::Paper });
                self.refresh_icon();
            }
            Hit::Signal(c) => {
                self.surface.signal = c;
                if self.theme_edit.family == crate::theme_edit::Family::FromSignal {
                    self.rebuild_theme();
                }
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
            Hit::Compact(c) => {
                if self.sidebar_rules.compact != c {
                    self.toggle_compact();
                }
            }
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
            Hit::ReloadRules => {
                self.rules.reload();
                self.refresh_rules_folders();
            }
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
                self.refresh_rules_folders();
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
                if let Some(t) = crate::themes::all().get(k).cloned() {
                    self.apply_theme(&t);
                }
            }
            Hit::SavePreset => {
                let n = crate::themes::all().len() + 1;
                let name = format!("mine-{n}");
                let t = self.current_theme(&name);
                if crate::themes::save(&t).is_ok() {
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
            Hit::StopSel(i) => {
                self.stop_sel = i;
                self.tok_sel = TokSel::Stop(i);
            }
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
            Hit::TokPaper(c) => {
                let mode = self.theme.mode;
                self.theme_edit.edit_mut(mode).paper = Some(c);
                self.rebuild_theme();
            }
            Hit::TokInk(c) => {
                let mode = self.theme.mode;
                self.theme_edit.edit_mut(mode).ink = Some(c);
                self.rebuild_theme();
            }
            Hit::TokPage(c) => {
                let mode = self.theme.mode;
                self.theme_edit.edit_mut(mode).page = Some(c);
                self.rebuild_theme();
            }
            Hit::TokReset => {
                let mode = self.theme.mode;
                *self.theme_edit.edit_mut(mode) = Default::default();
                self.theme_edit.family = crate::theme_edit::Family::Broadsheet;
                self.theme_edit.saturation = 1.0;
                self.rebuild_theme();
            }
            Hit::AnsiSel(i) => self.ansi_sel = i.min(15),
            Hit::AnsiSet(c) => {
                let mode = self.theme.mode;
                let current: [Color; 16] = std::array::from_fn(|i| crate::theme_edit::from_rgb(self.theme.ansi[i]));
                let e = self.theme_edit.edit_mut(mode);
                let mut a = e.ansi.unwrap_or(current);
                a[self.ansi_sel.min(15)] = c;
                e.ansi = Some(a);
                self.theme_edit.family = crate::theme_edit::Family::Imported;
                self.rebuild_theme();
            }
            Hit::Family(f) => {
                self.theme_edit.family = f;
                if f == crate::theme_edit::Family::Broadsheet {
                    let mode = self.theme.mode;
                    self.theme_edit.edit_mut(mode).ansi = None;
                }
                self.rebuild_theme();
            }
            Hit::Import(k) => {
                if let Some(t) = crate::theme_edit::imports().get(k) {
                    let mode = self.theme.mode;
                    let e = self.theme_edit.edit_mut(mode);
                    e.ansi = t.ansi;
                    if let Some(p) = t.paper {
                        e.paper = Some(p);
                    }
                    if let Some(i) = t.ink {
                        e.ink = Some(i);
                    }
                    self.theme_edit.family = crate::theme_edit::Family::Imported;
                    self.rebuild_theme();
                }
            }
            Hit::Starter(k) => self.rules.write_starter(k),
            Hit::LookTab(k) => {
                self.look_tab = k;
                if let Some(Pane::Settings(s)) = self.tabs.get_mut(self.active).map(|t| &mut t.left) {
                    s.scroll = 0.0;
                }
            }
            Hit::TokSel(t) => {
                self.tok_sel = t;
                match t {
                    TokSel::Stop(i) => self.stop_sel = i,
                    TokSel::Ansi(i) => self.ansi_sel = i,
                    _ => {}
                }
            }
            Hit::TokSet(c) => self.set_tok(c),
            Hit::ShellInt(b) => self.behavior.shell_integration = b,
            Hit::Welcome => self.open_welcome(),
            Hit::Block(b) => {
                self.behavior.block_content = b;
                crate::browser::BLOCKING.store(b, std::sync::atomic::Ordering::Relaxed);
            }
            Hit::StatusStyle(s) => self.behavior.status = s,
            Hit::SleepAfter(n) => self.behavior.sleep_after_min = n,
            Hit::ArchiveAfter(n) => self.behavior.archive_after_h = n,
            Hit::Highlight(b) => self.behavior.highlight = b,
            Hit::CopyOnSelect(b) => self.behavior.copy_on_select = b,
            Hit::PaneControls(c) => self.behavior.pane_controls = c,
            Hit::ScrollEasing(e) => self.behavior.scroll_easing = e,
            Hit::WheelLines(n) => self.behavior.wheel_lines = n,
            Hit::PageSmooth(b) => {
                self.behavior.page_smooth_scroll = b;
                self.refresh_register_note();
            }
            Hit::PaneDivider(b) => self.behavior.pane_divider = b,
            Hit::MiddlePaste(b) => self.behavior.middle_paste = b,
            Hit::Osc52(o) => self.behavior.osc52 = o,
            Hit::Predict(b) => self.behavior.predict = b,
            Hit::PromptLsp(m) => self.behavior.prompt_lsp = m,
            Hit::FormatOnSave(b) => self.behavior.format_on_save = b,
            Hit::HdrStyle(s) => {
                self.header.style = s;
                if s == HeaderStyle::Rail && self.header.style != s {
                    self.header.header_button = false;
                }
            }
            Hit::HdrMasthead(b) => self.header.masthead = b,
            Hit::HdrDateline(b) => self.header.dateline = b,
            Hit::HdrButton(b) => self.header.header_button = b,
            Hit::HdrNextRow(b) => self.header.next_row = b,
            Hit::HdrName(b) => self.header.show_name = b,
            Hit::HdrCaret(b) => self.header.kinds_caret = b,
            Hit::HdrRailHover(b) => self.header.rail_hover = b,
            Hit::HdrFlash(b) => self.header.flash = b,
            Hit::CurShape(s) => self.cursor.shape = s,
            Hit::CurBlink(b) => self.cursor.blink = b,
            Hit::CurColor(c) => self.cursor.color = c,
            Hit::CurMotion(m) => self.cursor.motion = m,
            Hit::CurHollow(h) => self.cursor.hollow_unfocused = h,
            Hit::CurPointer(p) => {
                self.cursor.pointer = p;
                self.pointer_request = Some(p);
            }
            Hit::CurHide(h) => self.cursor.hide_while_typing = h,
            Hit::OpenThemes => {
                let dir = crate::theme_edit::themes_dir();
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
            Hit::WindowStart(w) => self.behavior.window_start = w,
            Hit::Splash(m) => self.behavior.splash = m,
            Hit::Then(t) => self.behavior.then = t,
            Hit::Atlas(a) => {
                self.behavior.atlas = a;
                self.behavior.start_on_launch = a != AtlasMode::Planet;
            }
            Hit::Outside(o) => self.behavior.outside = o,
            Hit::LoginItem(on) => {
                self.login_note = match crate::little::login_item(on) {
                    Ok(()) => if on { "registered · nus starts with the system".into() } else { "removed".into() },
                    Err(e) => e,
                };
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
                if ev == "launch" {
                    self.behavior.startup_sound = !cue.is_empty();
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
            Hit::TexMotion(b) => self.surface.texture_motion = b,
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

    /// The colour of the token the picker is on.
    pub(crate) fn tok_color(&self) -> Color {
        let ink = self.theme.ink;
        match self.tok_sel {
            TokSel::Signal => self.surface.signal,
            TokSel::Stop(i) => self.surface.ramp(ink).get(i).copied().unwrap_or(self.surface.signal),
            TokSel::Paper => self.theme.paper,
            TokSel::Ink => self.theme.ink,
            TokSel::Page => self.theme.page,
            TokSel::Ansi(i) => crate::theme_edit::from_rgb(self.theme.ansi[i.min(15)]),
        }
    }

    /// Set the token the picker is on.
    pub(crate) fn set_tok(&mut self, c: Color) {
        let hit = match self.tok_sel {
            TokSel::Signal => Hit::Signal(c),
            TokSel::Stop(i) => {
                self.stop_sel = i;
                Hit::StopColor(c)
            }
            TokSel::Paper => Hit::TokPaper(c),
            TokSel::Ink => Hit::TokInk(c),
            TokSel::Page => Hit::TokPage(c),
            TokSel::Ansi(i) => {
                self.ansi_sel = i;
                Hit::AnsiSet(c)
            }
        };
        self.apply_setting(hit, 0.0);
    }

    /// A neobrutal tile: hard offset shadow, 2px ink outline, the colour
    /// inside; lifts on hover; a double ring when selected. None = a
    /// dashed "nothing here" tile.
    fn draw_tile(&mut self, scene: &mut Scene, r: Rect, color: Option<Color>, on: bool, key: u64) {
        let t = self.theme.clone();
        let ink = t.ink;
        let (mx, my) = self.mouse;
        let hot = r.contains(mx, my);
        let dur = self.motion.dur(120.0);
        let h = self.hovers.entry(key).or_insert_with(|| Hover { alpha: Anim::at(0.0), pulse: Anim::at(1.0), hot: false });
        if hot != h.hot {
            h.hot = hot;
            h.alpha.go(if hot { 1.0 } else { 0.0 }, dur);
        }
        let a = h.alpha.value();
        if h.alpha.active() {
            self.dirty = true;
        }
        let lift = self.px(2.0) * a;
        let off = self.px(4.0) + lift;
        let tile = Rect::new(r.x - lift, r.y - lift, r.w, r.h);
        let sw = self.px(m::STRUCTURE);
        scene.rect(Rect::new(tile.x + off, tile.y + off, tile.w, tile.h), ink);
        match color {
            Some(c) => scene.rect(tile, c),
            None => {
                scene.rect(tile, t.paper);
                scene.push(nus_render::Instance::hazard(tile, self.px(1.0), fade(ink, 0.35), [0.0, 0.0, 0.0, 0.0], self.px(8.0)));
            }
        }
        scene.outline(tile, sw, ink);
        if on {
            // Double ring: paper inside the ink.
            let inner = Rect::new(tile.x + sw, tile.y + sw, tile.w - 2.0 * sw, tile.h - 2.0 * sw);
            scene.outline(inner, sw, t.paper);
            let inner2 = Rect::new(inner.x + sw, inner.y + sw, inner.w - 2.0 * sw, inner.h - 2.0 * sw);
            scene.outline(inner2, sw, ink);
        }
    }

    /// A preset card: the ramp as its face, the signal as a chip, the name
    /// set in Newsreader; hard shadow, ink outline.
    #[allow(clippy::too_many_arguments)]
    fn draw_card(&mut self, scene: &mut Scene, r: Rect, name: &str, ramp: &[Color], signal: Color, angle: f32, on: bool, faces: Option<(Color, Color, Color, Color)>) {
        let t = self.theme.clone();
        let ink = t.ink;
        let key = hover_key("card", r.y as usize * 4096 + r.x as usize);
        let (mx, my) = self.mouse;
        let hot = Rect::new(r.x, r.y, r.w + self.px(6.0), r.h + self.px(6.0)).contains(mx, my);
        let dur = self.motion.dur(140.0);
        let h = self.hovers.entry(key).or_insert_with(|| Hover { alpha: Anim::at(0.0), pulse: Anim::at(1.0), hot: false });
        if hot != h.hot {
            h.hot = hot;
            h.alpha.go(if hot { 1.0 } else { 0.0 }, dur);
        }
        let a = h.alpha.value();
        if h.alpha.active() {
            self.dirty = true;
        }
        let lift = self.px(3.0) * a;
        let off = self.px(5.0) + lift;
        let card = Rect::new(r.x - lift, r.y - lift, r.w, r.h);
        let sw = self.px(m::STRUCTURE);
        scene.rect(Rect::new(card.x + off, card.y + off, card.w, card.h), if on { signal } else { ink });
        if ramp.is_empty() {
            // Save-as: paper face, dashed feel via a hazard, a plus.
            scene.rect(card, t.paper);
            scene.push(nus_render::Instance::hazard(card, self.px(1.0), fade(ink, 0.25), [0.0, 0.0, 0.0, 0.0], self.px(10.0)));
            let isz = self.px(22.0);
            self.fonts.draw_icon(scene, icons::PLUS, isz, card.x + (card.w - isz) / 2.0, card.y + (card.h - isz) / 2.0 - self.px(8.0), ink);
        } else {
            scene.push(nus_render::Instance::rounded_stops(card, 0.0, ramp, angle, 0.0, false));
            // A paper strip along the bottom for the name, like a label.
            let strip_h = self.px(30.0);
            scene.rect(Rect::new(card.x, card.bottom() - strip_h, card.w, strip_h), t.paper);
            scene.hline(card.x, card.bottom() - strip_h, card.w, sw, ink);
            let chip = self.px(12.0);
            scene.rect(Rect::new(card.x + self.px(12.0), card.bottom() - strip_h + (strip_h - chip) / 2.0, chip, chip), signal);
            scene.outline(Rect::new(card.x + self.px(12.0), card.bottom() - strip_h + (strip_h - chip) / 2.0, chip, chip), self.px(1.0), ink);
            // The two faces as tiny pages: paper with an ink line, ink with a paper line.
            if let Some((pp, pi, ip, ii)) = faces {
                let pw = self.px(22.0);
                let ph = self.px(28.0);
                let mut fx = card.right() - self.px(12.0) - pw;
                for (bg, fg) in [(ip, ii), (pp, pi)] {
                    let pr = Rect::new(fx, card.y + self.px(10.0), pw, ph);
                    scene.rect(pr, bg);
                    scene.outline(pr, self.px(1.0), ink);
                    for k in 0..3 {
                        scene.rect(Rect::new(pr.x + self.px(4.0), pr.y + self.px(6.0) + k as f32 * self.px(6.0), pw - self.px(8.0) - if k == 2 { self.px(6.0) } else { 0.0 }, self.px(2.0)), fg);
                    }
                    fx -= pw + self.px(6.0);
                }
            }
        }
        let wm = Style { font: self.f.wordmark, px: self.px(19.0), color: ink, tracking: 0.0 };
        let ny = if ramp.is_empty() { card.bottom() - self.px(14.0) } else { card.bottom() - self.px(10.0) };
        let nx = if ramp.is_empty() { card.x + self.px(12.0) } else { card.x + self.px(32.0) };
        let nm = self.fit(wm, name, card.w - (nx - card.x) - self.px(10.0));
        self.fonts.draw(scene, wm, nx, ny, &nm);
        scene.outline(card, sw, ink);
        if on {
            let inner = Rect::new(card.x + sw, card.y + sw, card.w - 2.0 * sw, card.h - 2.0 * sw);
            scene.outline(inner, sw, t.paper);
        }
    }

    /// The live proof: a miniature nus window in the current look —
    /// carapace with its ramp and texture, chrome, sidebar, a shell with
    /// coloured runs and the cursor, a page.
    fn draw_studio(&mut self, scene: &mut Scene, r: Rect) {
        let t = self.theme.clone();
        let ink = t.ink;
        let paper = self.paper();
        let ramp = self.surface.ramp(ink);
        let sw = (self.px(self.surface.shell_width) * 0.6).max(self.px(3.0));
        let radius = self.px(self.surface.shell_radius) * 0.5;
        let win = r;
        // Hard shadow under the whole proof: it's a card too.
        scene.rect(Rect::new(win.x + self.px(6.0), win.y + self.px(6.0), win.w, win.h), ink);
        scene.push(nus_render::Instance::rounded(win, radius, paper));
        match self.surface.shell {
            Shell::Band => {
                scene.layer(Some(Rect::new(win.x, win.y, win.w, sw)));
                scene.push(nus_render::Instance::rounded(win, radius, self.surface.signal));
                scene.layer(None);
            }
            Shell::Stroke => scene.push(nus_render::Instance::stroke(win, radius, sw, self.surface.signal, None, 0.0)),
            Shell::Gradient => scene.push(nus_render::Instance::stroke_stops(win, radius, sw, &ramp, self.surface.angle, 0.0, false)),
            Shell::Aurora => scene.push(nus_render::Instance::stroke_stops(win, radius, sw, &ramp, self.surface.angle, self.shell_phase, true)),
        }
        if let Some(kind) = self.surface.texture_kind.shader_kind() {
            if self.surface.texture > 0.0 && self.surface.texture_on == TextureOn::Carapace {
                let gc = [1.0, 1.0, 1.0, (self.surface.texture * 3.0).min(1.0)];
                let tm = if self.surface.texture_motion { self.started.elapsed().as_secs_f32() % 3600.0 } else { 0.0 };
                let pitch = self.px(self.surface.texture_scale);
                if self.surface.shell == Shell::Band {
                    scene.layer(Some(Rect::new(win.x, win.y, win.w, sw)));
                    scene.push(nus_render::Instance::texture_stroke(win, kind, gc, pitch, tm, radius, sw));
                    scene.layer(None);
                } else {
                    scene.push(nus_render::Instance::texture_stroke(win, kind, gc, pitch, tm, radius, sw));
                }
            }
        }
        // Inside the carapace.
        let inner = Rect::new(win.x + sw, win.y + sw, win.w - 2.0 * sw, win.h - 2.0 * sw);
        scene.layer(Some(inner));
        let strip_h = self.px(20.0);
        let wm = Style { font: self.f.wordmark, px: self.px(13.0), color: ink, tracking: 0.0 };
        self.fonts.draw(scene, wm, inner.x + self.px(10.0), inner.y + self.px(14.0), "nus");
        let tiny = Style { font: self.f.ui, px: self.px(8.0), color: t.dim, tracking: 0.08 };
        self.fonts.draw(scene, tiny, inner.x + self.px(40.0), inner.y + self.px(13.5), "01 SHELL · ~/NUS");
        scene.hline(inner.x, inner.y + strip_h, inner.w, self.px(1.0), ink);
        // Sidebar.
        let sb_w = self.px(74.0);
        let sb = Rect::new(inner.x, inner.y + strip_h + 1.0, sb_w, inner.h - strip_h - 1.0);
        scene.vline(sb.right(), sb.y, sb.h, self.px(1.0), ink);
        let row_h = self.px(16.0);
        for (i, name) in ["shell", "docs", "cargo"].iter().enumerate() {
            let ry = sb.y + self.px(6.0) + i as f32 * row_h;
            if i == 0 {
                scene.rect(Rect::new(sb.x, ry, sb.w, row_h), t.tint);
                scene.rect(Rect::new(sb.x, ry, self.px(1.5), row_h), self.surface.signal);
            }
            let st = Style { font: if i == 0 { self.f.strong } else { self.f.ui }, px: self.px(8.0), color: if i == 0 { ink } else { t.dim }, tracking: 0.0 };
            self.fonts.draw(scene, st, sb.x + self.px(10.0), ry + self.px(11.0), name);
        }
        // Shell pane.
        let pane = Rect::new(sb.right() + 1.0, sb.y, (inner.w - sb_w) * 0.58, sb.h);
        let ansi: Vec<Color> = (0..16).map(|i| crate::theme_edit::from_rgb(t.ansi[i])).collect();
        let mono = |c: Color, me: &Self| Style { font: me.f.ui, px: me.px(9.0), color: c, tracking: 0.0 };
        let lines: Vec<Vec<(Color, &str)>> = vec![
            vec![(ansi[2], "seb@nus"), (ink, ":"), (ansi[4], "~/nus"), (ink, "$ cargo test")],
            vec![(ansi[3], "warning"), (ink, ": unused"), (t.dim, " · 18 passed "), (ansi[1], "0 failed")],
            vec![(ansi[5], "> "), (ansi[6], "git"), (ink, " log "), (t.dim, "9058fca")],
        ];
        let mut ly = pane.y + self.px(16.0);
        for line in lines {
            let mut lx = pane.x + self.px(10.0);
            for (c, txt) in line {
                lx += self.fonts.draw(scene, mono(c, self), lx, ly, txt);
            }
            ly += self.px(14.0);
        }
        // Prompt with the cursor as configured.
        let mut lx = pane.x + self.px(10.0);
        lx += self.fonts.draw(scene, mono(ansi[2], self), lx, ly, "$ ");
        let cur_c = match self.cursor.color { crate::settings::CursorColor::Ink => ink, _ => self.surface.signal };
        let cw = self.px(5.5);
        let chh = self.px(11.0);
        match self.cursor.shape {
            crate::settings::CursorShapePref::Beam => scene.rect(Rect::new(lx, ly - self.px(9.0), self.px(1.5), chh), cur_c),
            crate::settings::CursorShapePref::Underline => scene.rect(Rect::new(lx, ly + self.px(1.0), cw, self.px(1.5)), cur_c),
            _ => scene.rect(Rect::new(lx, ly - self.px(9.0), cw, chh), cur_c),
        }
        // Page pane.
        let page = Rect::new(pane.right(), pane.y, inner.right() - pane.right(), pane.h);
        scene.vline(page.x, page.y, page.h, self.px(1.0), ink);
        scene.rect(Rect::new(page.x + 1.0, page.y, page.w, page.h), t.page);
        let page_ink = if crate::theme_edit::contrast(ink, t.page) >= crate::theme_edit::contrast(t.paper, t.page) { ink } else { t.paper };
        let serif = Style { font: self.f.serif, px: self.px(12.0), color: page_ink, tracking: 0.0 };
        self.fonts.draw(scene, serif, page.x + self.px(12.0), page.y + self.px(24.0), "Monterey Bay");
        for k in 0..4 {
            let w = page.w - self.px(24.0) - if k == 3 { page.w * 0.3 } else { 0.0 };
            scene.rect(Rect::new(page.x + self.px(12.0), page.y + self.px(34.0) + k as f32 * self.px(9.0), w.max(0.0), self.px(3.0)), fade(page_ink, 0.25));
        }
        // ANSI strip along the bottom of the shell pane.
        let strip_y = pane.bottom() - self.px(12.0);
        let cell = (pane.w - self.px(20.0)) / 16.0;
        for i in 0..16 {
            scene.rect(Rect::new(pane.x + self.px(10.0) + i as f32 * cell, strip_y, cell - 1.0, self.px(6.0)), ansi[i]);
        }
        scene.layer(None);
        scene.outline(win, self.px(m::STRUCTURE), ink);
    }

    /// The row labels of a section, for the palette.
    pub(crate) fn settings_labels(&self, section: usize) -> Vec<(String, ())> {
        self.rows_for(section).into_iter().map(|(l, _)| (l, ())).collect()
    }

    fn rows_for(&self, section: usize) -> Vec<(String, Control)> {
        use Control::*;
        let hex = surface::hex;
        let ink = self.theme.mode == nus_render::Mode::Ink;
        match section {
            0 => {
                let tab = self.look_tab.min(LOOK_TABS.len() - 1);
                let strip: Vec<(String, Hit, bool)> = LOOK_TABS.iter().enumerate().map(|(k, n)| (n.to_string(), Hit::LookTab(k), k == tab)).collect();
                let mut v: Vec<(String, Control)> = vec![("".into(), Studio), ("".into(), Strip(strip))];
                let rest: Vec<(String, Control)> = match tab {
                    LOOK_PRESETS => {
                        let themes = crate::themes::all();
                        let card = |k: usize, t: &crate::themes::StockTheme| -> (String, Vec<Color>, Color, f32, Hit, bool, Option<(Color, Color, Color, Color)>) {
                            let ramp = t.surface.ramp(t.ink.ink);
                            (t.name.clone(), ramp, t.surface.signal, t.surface.angle, Hit::Preset(k), t.name == self.preset_name, Some((t.paper.paper, t.paper.ink, t.ink.paper, t.ink.ink)))
                        };
                        let mut originals: Vec<_> = themes.iter().enumerate().filter(|(_, t)| !t.port).map(|(k, t)| card(k, t)).collect();
                        originals.push(("save as…".into(), Vec::new(), self.surface.signal, 0.0, Hit::SavePreset, false, None));
                        let ports: Vec<_> = themes.iter().enumerate().filter(|(_, t)| t.port).map(|(k, t)| card(k, t)).collect();
                        let current = themes.iter().find(|t| t.name == self.preset_name).map(|t| t.story.clone()).unwrap_or_else(|| "edited from a theme · SAVE AS keeps it".into());
                        vec![
                            ("THEMES".into(), Cards(originals)),
                            ("".into(), Info(current)),
                            ("PORTS".into(), Cards(ports)),
                            ("".into(), Info("a theme is the whole look: both faces' tokens and sixteens, the carapace, the cursor, the bar, a few sounds · saved ones live in profile/themes".into())),
                            ("".into(), Buttons(vec![("OPEN THEMES FOLDER".into(), icons::FOLDER, Hit::OpenThemes)])),
                        ]
                    }
                    LOOK_SURFACE => {
                let ink = self.theme.ink;
                let presets = surface::presets();
                let mut preset_chips: Vec<(String, Hit, bool)> =
                    presets.iter().enumerate().map(|(k, p)| (p.name.caps(), Hit::Preset(k), p.name == self.preset_name)).collect();
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
                let _ = (&preset_chips, &sig, &fam, &stops, &stop_colors);
                // The carapace's tokens as tiles: signal, then the ramp's stops.
                let mut tiles: Vec<(String, Option<Color>, String, Hit, bool)> = vec![("SIGNAL".into(), Some(self.surface.signal), hex(self.surface.signal), Hit::TokSel(TokSel::Signal), self.tok_sel == TokSel::Signal)];
                for (i, &c) in ramp.iter().enumerate() {
                    tiles.push((format!("STOP {}", i + 1), Some(c), hex(c), Hit::TokSel(TokSel::Stop(i)), self.tok_sel == TokSel::Stop(i)));
                }
                if ramp.len() < 4 {
                    tiles.push(("ADD".into(), None, "a stop".into(), Hit::StopAdd, false));
                }
                if self.surface.stops.len() > 2 {
                    tiles.push(("REMOVE".into(), None, "last stop".into(), Hit::StopRemove, false));
                }
                let editing = matches!(self.tok_sel, TokSel::Signal | TokSel::Stop(_));
                let mut tray: Vec<(String, Option<Color>, String, Hit, bool)> = Vec::new();
                if editing {
                    let cur = self.tok_color();
                    for &c in surface::family(self.surface.signal).iter() {
                        tray.push((String::new(), Some(c), hex(c), Hit::TokSet(c), c == cur));
                    }
                    for &(_, c) in SWATCHES.iter() {
                        tray.push((String::new(), Some(c), hex(c), Hit::TokSet(c), c == cur));
                    }
                }
                let mut base_tiles: Vec<(String, Option<Color>, String, Hit, bool)> = vec![("NONE".into(), None, "paper as is".into(), Hit::Base(None), self.surface.base.is_none())];
                base_tiles.extend(SWATCHES.iter().map(|&(_, c)| (String::new(), Some(c), hex(c), Hit::Base(Some(c)), self.surface.base == Some(c))));
                let _ = base;
                let mut v = vec![
                    ("CARAPACE TOKENS".into(), Tokens(tiles, true)),
                ];
                if editing {
                    let what = match self.tok_sel { TokSel::Signal => "signal".to_string(), TokSel::Stop(i) => format!("stop {}", i + 1), _ => String::new() };
                    v.push((format!("{} HUE", what.caps()), Slider(self::Slider::Hue, self.slider_value(self::Slider::Hue), format!("{}°", (surface::to_hsl(self.tok_color()).0 * 360.0).round()))));
                    v.push(("SATURATION".into(), Slider(self::Slider::Sat, self.slider_value(self::Slider::Sat), format!("{}%", (surface::to_hsl(self.tok_color()).1 * 100.0).round()))));
                    v.push(("LIGHTNESS".into(), Slider(self::Slider::Light, self.slider_value(self::Slider::Light), format!("{}% · {}", (surface::to_hsl(self.tok_color()).2 * 100.0).round(), hex(self.tok_color())))));
                    v.push(("TRAY".into(), Tokens(tray, false)));
                }
                v.push(("".into(), Info("signal: the carapace, the window square, ticks, progress · stops: the gradient and aurora ramps".into())));
                v.push(("BASE".into(), Tokens(base_tiles, false)));
                v.extend(vec![
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
                        Choice(TextureKind::ALL.iter().map(|&k| (k.name().caps(), Hit::TexKind(k), k == self.surface.texture_kind)).collect()),
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
                        "MOTION".into(),
                        Choice(vec![
                            ("STILL".into(), Hit::TexMotion(false), !self.surface.texture_motion),
                            ("ANIMATED · GRAIN FLICKERS, PATTERNS DRIFT".into(), Hit::TexMotion(true), self.surface.texture_motion),
                        ]),
                    ),
                    (
                        "CARAPACE".into(),
                        Choice(Shell::ALL.iter().map(|&s| (s.name().caps(), Hit::Shell(s), s == self.surface.shell)).collect()),
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
                ]);
                v
            }
                    LOOK_TOKENS => {
                use crate::theme_edit::{contrast, grade, Family};
                let t = self.theme.clone();
                let ink_mode = t.mode == nus_render::Mode::Ink;
                let papers: &[u32] = if ink_mode { &[0x141414, 0x0f0f0f, 0x1b1a1a, 0x1c1b19, 0x1e2126, 0x16253a, 0x201c1c, 0x0d1117] } else { &[0xf4f1ea, 0xfffdf7, 0xf7f3e8, 0xece7da, 0xe8e4d8, 0xfbf1c7, 0xfdf6e3, 0xffffff] };
                let inks: &[u32] = if ink_mode { &[0xece7da, 0xf4f1ea, 0xffffff, 0xd8d2c4, 0xe6e1d3, 0xcdd6f4, 0xa89984, 0x93a1a1] } else { &[0x141414, 0x000000, 0x2b2a27, 0x3c3836, 0x073642, 0x1c1b19, 0x3b4252, 0x4a4740] };
                let pages: &[u32] = &[0xffffff, 0xf4f1ea, 0xfdf6e3, 0x141414, 0x1b1a1a, 0x0f0f0f];
                let sw = |list: &[u32], cur: Color, mk: fn(Color) -> Hit| -> Vec<(Option<Color>, Hit, bool)> {
                    list.iter().map(|&v| { let c = nus_render::theme::hex(v); (Some(c), mk(c), (c[0] - cur[0]).abs() < 0.004 && (c[1] - cur[1]).abs() < 0.004 && (c[2] - cur[2]).abs() < 0.004) }).collect()
                };
                let ansi: Vec<Color> = (0..16).map(|i| crate::theme_edit::from_rgb(t.ansi[i])).collect();
                let sel = self.ansi_sel.min(15);
                let row = |from: usize| -> Vec<(Option<Color>, Hit, bool)> { (from..from + 8).map(|i| (Some(ansi[i]), Hit::AnsiSel(i), i == sel)).collect() };
                let mut cands: Vec<(Option<Color>, Hit, bool)> = Vec::new();
                for c in nus_render::theme::signal::ALL {
                    for f in surface::family(c) {
                        cands.push((Some(f), Hit::AnsiSet(f), false));
                    }
                }
                cands.truncate(24);
                let c_ink = contrast(t.ink, t.paper);
                let c_dim = contrast(t.dim, t.paper);
                let c_sig = contrast(self.surface.signal, t.paper);
                let imports = crate::theme_edit::imports();
                let mut import_chips: Vec<(String, Hit, bool)> = imports.iter().enumerate().map(|(k, i)| (format!("{} · {}", i.name, i.format).caps(), Hit::Import(k), false)).collect();
                if import_chips.is_empty() {
                    import_chips.push(("DROP GHOSTTY · WINDOWS TERMINAL · VS CODE · BASE16 FILES INTO PROFILE/THEMES".into(), Hit::OpenThemes, false));
                }
                let proof: Vec<(Color, String)> = vec![
                    (ansi[2], "seb@nus".into()), (t.ink, ":".into()), (ansi[4], "~/nus".into()), (t.ink, "$ cargo test  ".into()),
                    (ansi[3], "warning".into()), (t.ink, ": unused  ".into()), (ansi[2], "ok".into()), (t.ink, " 18 passed ".into()),
                    (ansi[1], "0 failed  ".into()), (ansi[5], "> ".into()), (ansi[6], "git".into()), (t.ink, " log  ".into()), (t.dim, "9058fca".into()),
                ];
                let brights: Vec<(Color, String)> = (8..16).map(|i| (ansi[i], format!("{i} "))).collect();
                let _ = (&sw, &row, &cands);
                let hx = surface::hex;
                let tiles: Vec<(String, Option<Color>, String, Hit, bool)> = vec![
                    ("PAPER".into(), Some(t.paper), hx(t.paper), Hit::TokSel(TokSel::Paper), self.tok_sel == TokSel::Paper),
                    ("INK".into(), Some(t.ink), hx(t.ink), Hit::TokSel(TokSel::Ink), self.tok_sel == TokSel::Ink),
                    ("PAGE".into(), Some(t.page), hx(t.page), Hit::TokSel(TokSel::Page), self.tok_sel == TokSel::Page),
                    ("DIM".into(), Some(t.dim), format!("{} · derived", hx(t.dim)), Hit::TokSel(self.tok_sel), false),
                ];
                let ansi_tiles: Vec<(String, Option<Color>, String, Hit, bool)> = (0..16).map(|i| (format!("{i}"), Some(ansi[i]), hx(ansi[i]), Hit::TokSel(TokSel::Ansi(i)), self.tok_sel == TokSel::Ansi(i))).collect();
                let editing_tok = matches!(self.tok_sel, TokSel::Paper | TokSel::Ink | TokSel::Page);
                let editing_ansi = matches!(self.tok_sel, TokSel::Ansi(_));
                let cur = self.tok_color();
                let tray: Vec<(String, Option<Color>, String, Hit, bool)> = match self.tok_sel {
                    TokSel::Paper => papers.iter().map(|&v| { let c = nus_render::theme::hex(v); (String::new(), Some(c), hx(c), Hit::TokSet(c), c == cur) }).collect(),
                    TokSel::Ink => inks.iter().map(|&v| { let c = nus_render::theme::hex(v); (String::new(), Some(c), hx(c), Hit::TokSet(c), c == cur) }).collect(),
                    TokSel::Page => pages.iter().map(|&v| { let c = nus_render::theme::hex(v); (String::new(), Some(c), hx(c), Hit::TokSet(c), c == cur) }).collect(),
                    TokSel::Ansi(_) => cands.iter().map(|(c, _, _)| { let c = c.unwrap(); (String::new(), Some(c), hx(c), Hit::TokSet(c), c == cur) }).collect(),
                    _ => Vec::new(),
                };
                let mut v: Vec<(String, Control)> = vec![
                    (format!("{} TOKENS", if ink_mode { "INK" } else { "PAPER" }), Tokens(tiles, true)),
                ];
                let picker = |v: &mut Vec<(String, Control)>, what: String, me: &Self| {
                    let (h, sa, l) = surface::to_hsl(me.tok_color());
                    v.push((format!("{} HUE", what.caps()), Slider(self::Slider::Hue, me.slider_value(self::Slider::Hue), format!("{}°", (h * 360.0).round()))));
                    v.push(("SATURATION".into(), Slider(self::Slider::Sat, me.slider_value(self::Slider::Sat), format!("{}%", (sa * 100.0).round()))));
                    v.push(("LIGHTNESS".into(), Slider(self::Slider::Light, me.slider_value(self::Slider::Light), format!("{}% · {}", (l * 100.0).round(), hx(me.tok_color())))));
                };
                if editing_tok {
                    picker(&mut v, format!("{:?}", self.tok_sel), self);
                    v.push(("TRAY".into(), Tokens(tray.clone(), false)));
                }
                v.push(("CONTRAST".into(), Info(format!("ink on paper {:.1}:1 {} · dim {:.1}:1 {} · signal {:.1}:1 {}", c_ink, grade(c_ink), c_dim, grade(c_dim), c_sig, grade(c_sig)))));
                v.push(("".into(), Info(format!("dim, tint and hot follow paper and ink · editing the {} theme; TYPE & MOTION switches", if ink_mode { "ink" } else { "paper" }))));
                v.push(("".into(), Buttons(vec![("RESET TOKENS".into(), icons::WARNING, Hit::TokReset)])));
                v.push(("ANSI".into(), Tokens(ansi_tiles, false)));
                if editing_ansi {
                    picker(&mut v, format!("ansi {sel}"), self);
                    v.push(("TRAY".into(), Tokens(tray, false)));
                }
                v.extend(vec![
                    ("FAMILY".into(), Choice(Family::ALL.iter().map(|&f| (f.name().caps(), Hit::Family(f), f == self.theme_edit.family)).chain(std::iter::once(("IMPORTED".to_string(), Hit::Family(Family::Imported), self.theme_edit.family == Family::Imported))).collect())),
                    ("SATURATION".into(), Slider(self::Slider::Saturation, self.slider_value(self::Slider::Saturation), format!("{}%", (self.theme_edit.saturation * 100.0).round()))),
                    ("PROOF".into(), Proof(proof)),
                    ("BRIGHTS".into(), Proof(brights)),
                    ("IMPORT".into(), Choice(import_chips)),
                    ("".into(), Buttons(vec![("OPEN THEMES FOLDER".into(), icons::FOLDER, Hit::OpenThemes)])),
                ]);
                v
            }
                    LOOK_TYPE => vec![
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
                ("UI FONT".into(), Info("IBM Plex Mono · 13 / 1.5 · any installed mono via init.luau".into())),
                ("TERMINAL FONT".into(), Info("IBM Plex Mono · 13pt · ligatures on".into())),
                ("WORDMARK".into(), Info("Newsreader Italic".into())),
            ],
                    _ => {
                let c = &self.cursor;
                vec![
                    (
                        "SHAPE".into(),
                        Choice(vec![
                            ("THE SHELL'S".into(), Hit::CurShape(CursorShapePref::Shell), c.shape == CursorShapePref::Shell),
                            ("BLOCK".into(), Hit::CurShape(CursorShapePref::Block), c.shape == CursorShapePref::Block),
                            ("BEAM".into(), Hit::CurShape(CursorShapePref::Beam), c.shape == CursorShapePref::Beam),
                            ("UNDERLINE".into(), Hit::CurShape(CursorShapePref::Underline), c.shape == CursorShapePref::Underline),
                        ]),
                    ),
                    (
                        "UNFOCUSED".into(),
                        Choice(vec![("HOLLOW".into(), Hit::CurHollow(true), c.hollow_unfocused), ("HIDDEN".into(), Hit::CurHollow(false), !c.hollow_unfocused)]),
                    ),
                    (
                        "BLINK".into(),
                        Choice(vec![
                            ("NEVER".into(), Hit::CurBlink(Blink::Never), c.blink == Blink::Never),
                            ("AFTER 2S IDLE".into(), Hit::CurBlink(Blink::AfterIdle), c.blink == Blink::AfterIdle),
                            ("ALWAYS".into(), Hit::CurBlink(Blink::Always), c.blink == Blink::Always),
                        ]),
                    ),
                    ("PERIOD".into(), Slider(self::Slider::BlinkPeriod, self.slider_value(self::Slider::BlinkPeriod), format!("{}ms", c.period))),
                    (
                        "COLOUR".into(),
                        Choice(vec![
                            ("INK".into(), Hit::CurColor(CursorColor::Ink), c.color == CursorColor::Ink),
                            ("SIGNAL".into(), Hit::CurColor(CursorColor::Signal), c.color == CursorColor::Signal),
                            ("THE TAB'S OWN".into(), Hit::CurColor(CursorColor::Tab), c.color == CursorColor::Tab),
                        ]),
                    ),
                    ("".into(), Info("text under a block cursor inverts".into())),
                    (
                        "MOTION".into(),
                        Choice(vec![
                            ("JUMP".into(), Hit::CurMotion(CursorMotion::Jump), c.motion == CursorMotion::Jump),
                            ("GLIDE".into(), Hit::CurMotion(CursorMotion::Glide), c.motion == CursorMotion::Glide),
                            ("COMET".into(), Hit::CurMotion(CursorMotion::Comet), c.motion == CursorMotion::Comet),
                            ("SMEAR".into(), Hit::CurMotion(CursorMotion::Smear), c.motion == CursorMotion::Smear),
                        ]),
                    ),
                    ("".into(), Info("glide eases between cells on the motion register; comet leaves a short ink trail; smear stretches the body the way neovide does".into())),
                    ("TRAIL".into(), Slider(self::Slider::Smear, self.slider_value(self::Slider::Smear), format!("{:.0}% · neovide's trail_size: how far the tail lags the head (smear only)", c.smear * 100.0))),
                    ("WEIGHT".into(), Slider(self::Slider::CurWeight, self.slider_value(self::Slider::CurWeight), format!("{}px · beam and underline", c.weight))),
                    (
                        "POINTER".into(),
                        Choice(vec![
                            ("SYSTEM".into(), Hit::CurPointer(Pointer::System), c.pointer == Pointer::System),
                            ("INK ARROW".into(), Hit::CurPointer(Pointer::InkArrow), c.pointer == Pointer::InkArrow),
                            ("SIGNAL DOT".into(), Hit::CurPointer(Pointer::SignalDot), c.pointer == Pointer::SignalDot),
                        ]),
                    ),
                    ("".into(), Info("over the chrome only · pages and shells keep the system pointer".into())),
                    (
                        "WHILE TYPING".into(),
                        Choice(vec![("HIDE THE POINTER".into(), Hit::CurHide(true), c.hide_while_typing), ("KEEP IT".into(), Hit::CurHide(false), !c.hide_while_typing)]),
                    ),
                ]
            }
                };
                v.extend(rest);
                v
            }
            1 => {
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
                        Choice(chunk.iter().map(|&i| (crate::sound::NAMES[i].caps(), Hit::Play(i), false)).collect()),
                    ));
                }
                rows.push(("".into(), Info("what plays when · QUIET silences an event · NEXT walks the palette".into())));
                for (e, (ev, _, note)) in crate::sound::EVENTS.iter().enumerate() {
                    let cur = self.sound.prefs.cue_for(ev);
                    let mut chips = vec![("QUIET".into(), Hit::EventCue(e, usize::MAX), cur.is_none())];
                    if let Some(c) = &cur {
                        let ci = crate::sound::NAMES.iter().position(|n| n == c).unwrap_or(0);
                        chips.push((c.to_string(), Hit::EventCue(e, ci), true));
                    }
                    chips.push(("NEXT ▸".into(), Hit::EventNext(e), false));
                    if !note.is_empty() {
                        chips.push((note.caps(), Hit::EventNext(e), false));
                    }
                    rows.push((ev.replace('.', " · ").caps(), Choice(chips)));
                }
                rows.push(("".into(), Info("rules.luau can override any event with on_event · cues by daniel belyi (cuelume, mit)".into())));
                rows
            }
            2 => {
                let b = &self.behavior;
                let launch_cue = self.sound.prefs.cue_for("launch");
                let sound_chips: Vec<(String, Hit, bool)> = {
                    let ev = crate::sound::EVENTS.iter().position(|(e, _, _)| *e == "launch").unwrap_or(0);
                    let mut v = vec![("OFF".into(), Hit::EventCue(ev, usize::MAX), launch_cue.is_none() || !b.startup_sound)];
                    for name in ["arrival", "chime", "bloom", "ready"] {
                        let ci = crate::sound::NAMES.iter().position(|n| *n == name).unwrap_or(0);
                        v.push((name.caps(), Hit::EventCue(ev, ci), b.startup_sound && launch_cue.as_deref() == Some(name)));
                    }
                    v
                };
                vec![
                    (
                        "WINDOW".into(),
                        Choice(vec![
                            ("LAST SIZE & PLACE".into(), Hit::WindowStart(WindowStart::Last), b.window_start == WindowStart::Last),
                            ("MAXIMIZED".into(), Hit::WindowStart(WindowStart::Maximized), b.window_start == WindowStart::Maximized),
                            ("FULLSCREEN".into(), Hit::WindowStart(WindowStart::Fullscreen), b.window_start == WindowStart::Fullscreen),
                            ("CENTERED 1440×900".into(), Hit::WindowStart(WindowStart::Centered), b.window_start == WindowStart::Centered),
                        ]),
                    ),
                    (
                        "SPLASH".into(),
                        Choice(vec![
                            ("ICON · DRAWS IN".into(), Hit::Splash(SplashMode::Draw), b.splash == SplashMode::Draw),
                            ("ICON · STILL".into(), Hit::Splash(SplashMode::Still), b.splash == SplashMode::Still),
                            ("NONE".into(), Hit::Splash(SplashMode::None), b.splash == SplashMode::None),
                        ]),
                    ),
                    (
                        "HOLD".into(),
                        Slider(self::Slider::SplashHold, self.slider_value(self::Slider::SplashHold), format!("{:.1}s at least · until the first tab is ready", b.splash_hold)),
                    ),
                    (
                        "THEN".into(),
                        Choice(vec![
                            ("RESTORE LAST SESSION".into(), Hit::Then(Then::Restore), b.then == Then::Restore),
                            ("A NEW SHELL".into(), Hit::Then(Then::Shell), b.then == Then::Shell),
                            ("THE LAST PAGE".into(), Hit::Then(Then::LastPage), b.then == Then::LastPage),
                        ]),
                    ),
                    (
                        "ATLAS".into(),
                        Choice(vec![
                            ("FROM THE PLANET".into(), Hit::Atlas(AtlasMode::Planet), b.atlas == AtlasMode::Planet),
                            ("ALSO AT LAUNCH".into(), Hit::Atlas(AtlasMode::AtLaunch), b.atlas == AtlasMode::AtLaunch),
                            ("AT LAUNCH · UNTIL YOU PICK".into(), Hit::Atlas(AtlasMode::Persistent), b.atlas == AtlasMode::Persistent),
                        ]),
                    ),
                    ("SOUND".into(), Choice(sound_chips)),
                    (
                        "LINKS FROM OUTSIDE".into(),
                        Choice(vec![
                            ("LITTLE WINDOW".into(), Hit::Outside(Outside::Little), b.outside == Outside::Little),
                            ("NEW TAB HERE".into(), Hit::Outside(Outside::NewTab), b.outside == Outside::NewTab),
                        ]),
                    ),
                    (
                        "AT LOGIN".into(),
                        Choice(vec![
                            ("START WITH THE SYSTEM".into(), Hit::LoginItem(true), crate::little::login_item_registered()),
                            ("NO".into(), Hit::LoginItem(false), !crate::little::login_item_registered()),
                        ]),
                    ),
                    ("".into(), Info(if self.login_note.is_empty() { "a shortcut in the Startup folder · reversible".into() } else { self.login_note.clone() })),
                ]
            }
            3 => vec![
                (
                    "HEADER".into(),
                    Choice(vec![
                        ("BAR".into(), Hit::HdrStyle(HeaderStyle::Bar), self.header.style == HeaderStyle::Bar),
                        ("RAIL".into(), Hit::HdrStyle(HeaderStyle::Rail), self.header.style == HeaderStyle::Rail),
                    ]),
                ),
                ("".into(), Info(match self.header.style { HeaderStyle::Bar => "bar: the window's name and NEW TAB share one ruled row".into(), HeaderStyle::Rail => "rail: every window as its square along the edge; the name above the tabs".into() })),
                (
                    "TITLE".into(),
                    Choice(vec![
                        ("CAPS".into(), Hit::HdrMasthead(false), !self.header.masthead),
                        ("MASTHEAD".into(), Hit::HdrMasthead(true), self.header.masthead),
                    ]),
                ),
                (
                    "DATELINE".into(),
                    Choice(vec![
                        ("WHERE · TABS · PORTS".into(), Hit::HdrDateline(true), self.header.dateline),
                        ("OFF".into(), Hit::HdrDateline(false), !self.header.dateline),
                    ]),
                ),
                (
                    "NEW TAB".into(),
                    Choice(vec![
                        ("IN THE HEADER".into(), Hit::HdrButton(!self.header.header_button), self.header.header_button),
                        ("AS THE NEXT ROW".into(), Hit::HdrNextRow(!self.header.next_row), self.header.next_row),
                    ]),
                ),
                ("".into(), Info("both can be on; click for a shell in the default profile, hold or right-click for the kinds".into())),
                (
                    "KINDS CARET".into(),
                    Choice(vec![("ON".into(), Hit::HdrCaret(true), self.header.kinds_caret), ("OFF".into(), Hit::HdrCaret(false), !self.header.kinds_caret)]),
                ),
                (
                    "WINDOW CELL".into(),
                    Choice(vec![("SQUARE + NAME".into(), Hit::HdrName(true), self.header.show_name), ("SQUARE".into(), Hit::HdrName(false), !self.header.show_name)]),
                ),
                (
                    "RAIL".into(),
                    Choice(vec![("ALWAYS".into(), Hit::HdrRailHover(false), !self.header.rail_hover), ("ON HOVER".into(), Hit::HdrRailHover(true), self.header.rail_hover)]),
                ),
                (
                    "PRESS".into(),
                    Choice(vec![("FLASH".into(), Hit::HdrFlash(true), self.header.flash), ("NONE".into(), Hit::HdrFlash(false), !self.header.flash)]),
                ),
                (
                    "DENSITY".into(),
                    Choice(vec![
                        ("FULL".into(), Hit::Compact(false), !self.sidebar_rules.compact),
                        ("COMPACT · ICONS ONLY".into(), Hit::Compact(true), self.sidebar_rules.compact),
                    ]),
                ),
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
                    "PANE CONTROLS".into(),
                    Choice(vec![
                        ("NEAR THE CORNER".into(), Hit::PaneControls(crate::panes::Controls::Near), self.behavior.pane_controls == crate::panes::Controls::Near),
                        ("NEVER".into(), Hit::PaneControls(crate::panes::Controls::Never), self.behavior.pane_controls == crate::panes::Controls::Never),
                    ]),
                ),
                (
                    "PANE DIVIDER".into(),
                    Choice(vec![("DRAGS".into(), Hit::PaneDivider(true), self.behavior.pane_divider), ("FIXED".into(), Hit::PaneDivider(false), !self.behavior.pane_divider)]),
                ),
                ("".into(), Info("nothing shows until the pointer nears a pane's corner; then move (drag onto a sidebar row, or NEW TAB), swap, solo, to a tab of its own, close bloom out of it · the rule between the panes lights as you near it and drags".into())),
                (
                    "SLEEP IDLE PAGES".into(),
                    Choice(vec![("NEVER".into(), Hit::SleepAfter(0), self.behavior.sleep_after_min == 0), ("10 MIN".into(), Hit::SleepAfter(10), self.behavior.sleep_after_min == 10), ("30 MIN".into(), Hit::SleepAfter(30), self.behavior.sleep_after_min == 30), ("2 H".into(), Hit::SleepAfter(120), self.behavior.sleep_after_min == 120)]),
                ),
                (
                    "ARCHIVE IDLE PAGES".into(),
                    Choice(vec![("NEVER".into(), Hit::ArchiveAfter(0), self.behavior.archive_after_h == 0), ("12 H".into(), Hit::ArchiveAfter(12), self.behavior.archive_after_h == 12), ("24 H".into(), Hit::ArchiveAfter(24), self.behavior.archive_after_h == 24), ("A WEEK".into(), Hit::ArchiveAfter(168), self.behavior.archive_after_h == 168)]),
                ),
                ("".into(), Info("a sleeping page keeps its place and wakes when shown · archived pages go to recently closed · pinned tabs and shells never".into())),
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
                            .map(|(i, p)| (p.name.caps(), Hit::DefaultProfile(i), i == self.behavior.default_profile))
                            .collect(),
                    ),
                )];
                // Shell integration: what each shell gets.
                v.insert(0, ("".into(), Info("prompt marks · cwd · exit codes · new tabs open where you are · jump to prompts · copy a command's output".into())));
                v.insert(1, (
                    "COMMAND LINE".into(),
                    Choice(vec![("HIGHLIGHT".into(), Hit::Highlight(!self.behavior.highlight), self.behavior.highlight), ("PREDICT".into(), Hit::Predict(!self.behavior.predict), self.behavior.predict)]),
                ));
                v.insert(2, ("".into(), Info("terminal-side, nothing to install: tokens coloured as you type; the history entry that continues your line ghosts after the caret, Right or End accepts".into())));
                let pl = self.behavior.prompt_lsp;
                v.insert(3, (
                    "PROMPT LSP".into(),
                    Choice(vec![
                        ("QUIET".into(), Hit::PromptLsp(PromptLsp::Quiet), pl == PromptLsp::Quiet),
                        ("MENU".into(), Hit::PromptLsp(PromptLsp::Menu), pl == PromptLsp::Menu),
                        ("OFF".into(), Hit::PromptLsp(PromptLsp::Off), pl == PromptLsp::Off),
                    ]),
                ));
                v.insert(4, ("".into(), Info("bash-language-server or PowerShell Editor Services (GET them on the welcome page) read the line as you type: quiet underlines a problem and ghosts a completion, Tab accepts; menu lists them under the caret".into())));
                v.insert(5, (
                    "EDITOR".into(),
                    Choice(vec![("FORMAT ON SAVE".into(), Hit::FormatOnSave(!self.behavior.format_on_save), self.behavior.format_on_save)]),
                ));
                v.insert(3, (
                    "CLIPBOARD".into(),
                    Choice(vec![
                        ("COPY ON SELECT".into(), Hit::CopyOnSelect(!self.behavior.copy_on_select), self.behavior.copy_on_select),
                        ("MIDDLE CLICK PASTES".into(), Hit::MiddlePaste(!self.behavior.middle_paste), self.behavior.middle_paste),
                    ]),
                ));
                v.insert(4, (
                    "OSC 52".into(),
                    Choice(vec![
                        ("OFF".into(), Hit::Osc52(Osc52::Off), self.behavior.osc52 == Osc52::Off),
                        ("PROGRAMS MAY SET IT".into(), Hit::Osc52(Osc52::Write), self.behavior.osc52 == Osc52::Write),
                        ("SET AND READ".into(), Hit::Osc52(Osc52::ReadWrite), self.behavior.osc52 == Osc52::ReadWrite),
                    ]),
                ));
                v.insert(5, ("".into(), Info("tmux, neovim and ssh sessions put text on your clipboard through OSC 52; reading it back is off unless you say so · copy keeps the selection, paste is bracketed and asks when it's many lines".into())));
                v.insert(6, (
                    "SCROLL".into(),
                    Choice(crate::scrolling::Easing::ALL.iter().map(|&e| (e.name().caps(), Hit::ScrollEasing(e), e == self.behavior.scroll_easing)).collect()),
                ));
                v.insert(7, (
                    "WHEEL".into(),
                    Choice(vec![("1 LINE".into(), Hit::WheelLines(1), self.behavior.wheel_lines == 1), ("3 LINES".into(), Hit::WheelLines(3), self.behavior.wheel_lines == 3), ("5 LINES".into(), Hit::WheelLines(5), self.behavior.wheel_lines == 5), ("8 LINES".into(), Hit::WheelLines(8), self.behavior.wheel_lines == 8)]),
                ));
                v.insert(8, ("".into(), Info("neoscroll's curves: the view moves a line at a time on an eased clock, and more ticks extend the trip; Shift+PgUp/PgDn and Ctrl+Shift+Home/End ride the same curve".into())));
                v.insert(0, (
                    "SHELL INTEGRATION".into(),
                    Choice(vec![("AUTO".into(), Hit::ShellInt(true), self.behavior.shell_integration), ("OFF".into(), Hit::ShellInt(false), !self.behavior.shell_integration)]),
                ));
                for (n, p) in self.profiles.iter().enumerate().take(6) {
                    let kind = crate::shell::kind_of(&p.program);
                    v.insert(2 + n, (p.name.caps(), Info(crate::shell::describe(kind).into())));
                }
                for p in &self.profiles {
                    v.push((format!("PROFILE · {}", p.name.caps()), Info(format!("{} {}", p.program, p.args.join(" ")))));
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
                    "CONTENT BLOCKING".into(),
                    Choice(vec![("ON".into(), Hit::Block(true), self.behavior.block_content), ("OFF".into(), Hit::Block(false), !self.behavior.block_content)]),
                ),
                ("".into(), Info(format!("{} hosts refused · ads, trackers, analytics · add yours to profile/blocklist.txt", crate::browser::blocklist_len()))),
                (
                    "SCROLL".into(),
                    Choice(vec![("SMOOTH".into(), Hit::PageSmooth(true), self.behavior.page_smooth_scroll), ("INSTANT".into(), Hit::PageSmooth(false), !self.behavior.page_smooth_scroll)]),
                ),
                ("".into(), Info("Chromium's own smooth scrolling for wheels and keys; trackpads are pixel-precise either way · takes effect at the next start".into())),
                (
                    "LOADING BAR".into(),
                    Choice(BarStyle::ALL.iter().map(|&b| (b.name().caps(), Hit::BarStyle(b), b == self.load_bar.style)).collect()),
                ),
                (
                    "STATUS".into(),
                    Choice(vec![
                        ("LAMP".into(), Hit::StatusStyle(Status::Lamp), self.behavior.status == Status::Lamp),
                        ("LAMP + WORD".into(), Hit::StatusStyle(Status::Both), self.behavior.status == Status::Both),
                        ("WORD".into(), Hit::StatusStyle(Status::Word), self.behavior.status == Status::Word),
                        ("NONE".into(), Hit::StatusStyle(Status::None), self.behavior.status == Status::None),
                    ]),
                ),
                ("".into(), Info("the lamp at the end of the tools row: signal while loading, ink when live, hazard stripes when local, hollow while asleep".into())),
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
                let asks = crate::ask::backends();
                let mut v: Vec<(String, Control)> = Vec::new();
                v.push(("ASK".into(), Info(format!("Ctrl+Shift+? beside a shell · {}", if asks.is_empty() { "no assistant found · claude, codex, copilot, ollama on PATH, or ANTHROPIC_API_KEY (curl)".to_string() } else { asks.iter().map(|b| format!("{} ({})", b.name, b.how)).collect::<Vec<_>>().join(" · ") }))));
                v.extend(self.llm_tools.iter().map(|(n, c)| (format!("LOCAL · {}", n.caps()), Info(c.clone()))));
                if self.llm_tools.is_empty() {
                    v.push(("LOCAL".into(), Info("none on PATH (claude, codex, ollama are detected)".into())));
                }
                v.push(("WEB · CHATGPT".into(), Info("https://chatgpt.com/?q=…".into())));
                v.push(("WEB · CLAUDE".into(), Info("https://claude.ai/new?q=…".into())));
                v
            }
            8 => {
                // What the rules do right now: three shells, a stack child, a page.
                let theme = if self.theme.mode == nus_render::Mode::Ink { "ink" } else { "paper" };
                let mk = |kind: &str, index: usize, host: &str, parent: Option<&surface::Overrides>| {
                    self.rules.new_tab(&surface::TabCtx { kind, index, profile: "powershell", space: &self.space_name, space_signal: self.surface.signal, theme, host, parent, tab_colours: &self.tab_colours })
                };
                let a = mk("terminal", 0, "", None);
                let b = mk("terminal", 1, "", None);
                let bc = mk("page", 1, "docs.rs", Some(&b));
                let c = mk("terminal", 2, "", None);
                let d = mk("page", 3, "github.com", None);
                let preview = vec![
                    (a.bg, a.signal, "1  powershell".to_string(), false),
                    (b.bg, b.signal, "2  powershell".to_string(), false),
                    (bc.bg, bc.signal, "docs.rs".to_string(), true),
                    (c.bg, c.signal, "3  wsl · ubuntu".to_string(), false),
                    (d.bg, d.signal, "4  github".to_string(), false),
                ];
                let try4 = mk("terminal", 3, "", None);
                let tried: Vec<(Option<Color>, Hit, bool)> = vec![(try4.bg, Hit::Starter(0), false), (try4.signal, Hit::Starter(0), false)];
                vec![
                ("FILE".into(), Info(self.rules.path.to_string_lossy().to_string())),
                ("STATUS".into(), Info(self.rules.status.clone())),
                ("START FROM".into(), Choice(surface::STARTERS.iter().enumerate().map(|(k, (n, _))| (n.caps(), Hit::Starter(k), false)).collect())),
                ("".into(), Info("a starter replaces new_tab and new_space; on_page and on_event are kept".into())),
                ("NOW".into(), Tabs(preview)),
                ("TRY".into(), Info("new_tab { kind = \"terminal\", index = 4 } →".into())),
                ("".into(), Swatches(tried)),
                ("HOOKS".into(), Info("new_tab · new_space · on_page · on_event   helpers: hue · mix · hsl · family".into())),
                (
                    "".into(),
                    Buttons(vec![
                        ("RELOAD".into(), icons::RELOAD, Hit::ReloadRules),
                        ("OPEN IN EDITOR".into(), icons::OPEN_EXTERNAL, Hit::OpenRules),
                        ("RESET TO DEFAULT".into(), icons::WARNING, Hit::ResetRules),
                    ]),
                ),
            ]
            }
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
                ("WELCOME".into(), Buttons(vec![("THE TOUR · F1".into(), icons::BOOK, Hit::Welcome)])),
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
            0 => format!("{} · {} · {}", self.preset_name.to_lowercase(), if self.theme.mode == nus_render::Mode::Ink { "ink" } else { "paper" }, self.surface.shell.name()),
            1 => if self.sound.prefs.enabled { format!("on · {}%", (self.sound.prefs.volume * 100.0).round()) } else { "off".into() },
            2 => format!("{:?} · then {:?}", self.behavior.splash, self.behavior.then).to_lowercase(),
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
        let mut slot = 0usize;
        let mut gy = y;
        for (gname, range) in GROUPS.iter() {
            // A group caption, then its tiles on a fresh row.
            if slot % cols != 0 {
                slot += cols - slot % cols;
            }
            let row0 = slot / cols;
            let cy = y + row0 as f32 * (th + gap) + gy - y;
            self.fonts.draw(scene, dim, r.x + pad, cy + self.px(10.0), gname);
            gy += self.px(22.0);
            for k in range.clone() {
                let (name, icon) = &SECTIONS[k];
                let col = slot % cols;
                let row = slot / cols;
                slot += 1;
                let tile = Rect::new(r.x + pad + col as f32 * (tw + gap), y + row as f32 * (th + gap) + (gy - y), tw, th);
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
        let gh = self.px(24.0);
        let (mx, my) = self.mouse;
        let mut y = r.y;
        for (gi, (gname, range)) in GROUPS.iter().enumerate().filter(|_| !tiles) {
            // Group caption: small, dim, no rule of its own.
            if gi > 0 {
                y += self.px(6.0);
            }
            let cap = Style { color: t.dim, px: self.px(10.0), ..label };
            self.fonts.draw(scene, cap, r.x + self.px(18.0), y + self.px(16.0), gname);
            y += gh;
            for i in range.clone() {
            let (name, icon) = &SECTIONS[i];
            let sel = i == p.section;
            let row = Rect::new(r.x, y, nav_w, sh);
            if sel {
                scene.rect(Rect::new(r.x, y, nav_w, sh - self.px(m::HAIRLINE)), ink);
            } else if row.contains(mx, my) {
                scene.rect(Rect::new(r.x, y, nav_w, sh - self.px(m::HAIRLINE)), t.tint);
            }
            let col = if sel { t.paper } else { ink };
            let base = y + self.px(12.0) + self.px(m::LABEL_PX) - self.px(2.0);
            self.fonts.draw_icon(scene, *icon, isz, r.x + self.px(18.0), base - isz + self.px(2.0), col);
            self.fonts.draw(scene, Style { color: col, ..label }, r.x + self.px(18.0) + isz + self.px(10.0), base, name);
            scene.hline(r.x, y + sh - self.px(m::HAIRLINE), nav_w, self.px(m::HAIRLINE), ink);
            self.settings_hits.push((row, Hit::Section(i)));
            y += sh;
            }
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
            // Full-width controls: caption above, the control across the column.
            let full = matches!(control, Control::Studio | Control::Strip(_) | Control::Cards(_) | Control::Tokens(..));
            let cap_h = if full && !k.is_empty() { self.px(22.0) } else { 0.0 };
            let card_w = self.px(168.0);
            let card_h = self.px(104.0);
            let gap = self.px(14.0);
            let per_row = ((maxw + gap) / (card_w + gap)).floor().max(1.0) as usize;
            let rh = match &control {
                Control::Studio => self.px(180.0) + self.px(18.0),
                Control::Strip(_) => self.px(44.0) + self.px(10.0),
                Control::Cards(cards) => {
                    let rows = (cards.len() + per_row - 1) / per_row;
                    cap_h + rows as f32 * (card_h + self.px(8.0) + gap) + self.px(10.0)
                }
                Control::Tokens(items, big) => {
                    let (tw, th) = if *big { (self.px(84.0), self.px(64.0) + self.px(34.0)) } else { (self.px(34.0), self.px(34.0)) };
                    let tg = if *big { self.px(16.0) } else { self.px(10.0) };
                    let per = ((maxw + tg) / (tw + tg)).floor().max(1.0) as usize;
                    let rows = (items.len().max(1) + per - 1) / per;
                    cap_h + rows as f32 * (th + tg) + self.px(10.0)
                }
                Control::Swatches(_) => self.px(12.0) * 2.0 + self.px(18.0) + self.px(m::HAIRLINE),
                _ => self.px(10.0) * 2.0 + self.px(m::UI_PX) + self.px(m::HAIRLINE),
            };
            let control_kind = if matches!(control, Control::Studio | Control::Strip(_)) { 0 } else { 1 };
            let base = y + self.px(10.0) + self.px(m::UI_PX) - self.px(3.0);
            if full {
                if !k.is_empty() {
                    self.fonts.draw(scene, dim, cx, y + self.px(12.0), &k);
                }
            } else {
                self.fonts.draw(scene, dim, cx, base, &k);
            }
            // In the studio, unlabelled rows run the full column.
            let vx = if p.section == SEC_LOOK && k.is_empty() { cx } else { cx + label_w };
            match control {
                Control::Studio => {
                    let r = Rect::new(cx, y, maxw, self.px(180.0));
                    self.draw_studio(scene, r);
                }
                Control::Strip(items) => {
                    // Neobrutal tab strip: outlined chips, the current one filled with a hard shadow.
                    let mut x = cx;
                    let ch = self.px(34.0);
                    let sy = y + self.px(5.0);
                    for (text, hit, on) in items {
                        let w = self.fonts.measure(strong, &text) + self.px(28.0);
                        let chip = Rect::new(x, sy, w, ch);
                        let hot = chip.contains(self.mouse.0, self.mouse.1);
                        let off = if on { self.px(4.0) } else if hot { self.px(3.0) } else { 0.0 };
                        if off > 0.0 {
                            scene.rect(Rect::new(chip.x + off, chip.y + off, chip.w, chip.h), if on { self.surface.signal } else { ink });
                        }
                        scene.rect(chip, if on { ink } else { t.paper });
                        scene.outline(chip, self.px(m::STRUCTURE), ink);
                        let st = Style { color: if on { t.paper } else { ink }, ..strong };
                        self.fonts.draw(scene, st, x + self.px(14.0), sy + ch / 2.0 + self.px(4.0), &text);
                        self.settings_hits.push((chip, hit));
                        x += w + self.px(10.0);
                    }
                }
                Control::Cards(cards) => {
                    let mut x = cx;
                    let mut cy = y + cap_h;
                    let mut n = 0;
                    for (name, ramp, signal, angle, hit, on, faces) in cards {
                        if n > 0 && n % per_row == 0 {
                            x = cx;
                            cy += card_h + self.px(8.0) + gap;
                        }
                        let card = Rect::new(x, cy, card_w, card_h);
                        self.draw_card(scene, card, &name, &ramp, signal, angle, on, faces);
                        self.settings_hits.push((Rect::new(card.x, card.y, card.w + self.px(6.0), card.h + self.px(6.0)), hit));
                        x += card_w + gap;
                        n += 1;
                    }
                }
                Control::Tokens(items, big) => {
                    let (tw, th) = if big { (self.px(84.0), self.px(64.0)) } else { (self.px(34.0), self.px(34.0)) };
                    let tg = if big { self.px(16.0) } else { self.px(10.0) };
                    let per = ((maxw + tg) / (tw + tg)).floor().max(1.0) as usize;
                    let row_h = if big { th + self.px(34.0) + tg } else { th + tg };
                    let mut x = cx;
                    let mut ty = y + cap_h;
                    for (n, (name, color, caption, hit, on)) in items.into_iter().enumerate() {
                        if n > 0 && n % per == 0 {
                            x = cx;
                            ty += row_h;
                        }
                        let tile = Rect::new(x, ty, tw, th);
                        let hk = hover_key("tok", (ty as usize) * 4096 + x as usize);
                        self.draw_tile(scene, tile, color, on, hk);
                        if color.is_none() {
                            // An empty tile says what it does: add, remove, or none.
                            let icon = match name.as_str() { "ADD" => Some(icons::PLUS), "REMOVE" => Some(icons::MINIMIZE), "NONE" => Some(icons::CLOSE), _ => None };
                            if let Some(icon) = icon {
                                let isz = (th * 0.4).round();
                                self.fonts.draw_icon(scene, icon, isz, tile.x + ((tw - isz) / 2.0).round(), tile.y + ((th - isz) / 2.0).round(), ink);
                            }
                        }
                        if big {
                            let nm = self.fit(label, &name, tw + self.px(8.0));
                            self.fonts.draw(scene, Style { color: ink, ..label }, x, ty + th + self.px(18.0), &nm);
                            let cp = self.fit(dim, &caption, tw + self.px(8.0));
                            self.fonts.draw(scene, dim, x, ty + th + self.px(31.0), &cp);
                        }
                        self.settings_hits.push((Rect::new(tile.x - self.px(2.0), tile.y - self.px(2.0), tile.w + self.px(8.0), tile.h + self.px(8.0) + if big { self.px(30.0) } else { 0.0 }), hit));
                        x += tw + tg;
                    }
                }
                Control::Info(v) => {
                    let vs = self.fit(ui, &v, maxw - label_w);
                    self.fonts.draw(scene, ui, vx, base, &vs);
                }
                Control::Proof(runs) => {
                    let mut x = vx;
                    for (c, text) in runs {
                        let st = Style { color: c, ..ui };
                        x += self.fonts.draw(scene, st, x, base, &text);
                    }
                }
                Control::Tabs(rows) => {
                    // A 248px sidebar in miniature, one row per result.
                    let sw = self.px(248.0);
                    let rh_row = self.px(26.0);
                    let x0 = vx;
                    let mut ry = y + self.px(4.0);
                    scene.outline(Rect::new(x0, ry, sw, rh_row * rows.len() as f32), self.px(m::HAIRLINE), t.tint);
                    for (bg, sig, title, child) in rows {
                        let rr = Rect::new(x0, ry, sw, rh_row);
                        if let Some(b) = bg {
                            scene.rect(rr, b);
                        }
                        scene.rect(Rect::new(x0, ry, self.px(2.0), rh_row), sig.unwrap_or(t.dim));
                        let tx = x0 + if child { self.px(26.0) } else { self.px(12.0) };
                        if child {
                            scene.vline(x0 + self.px(14.0), ry, rh_row, self.px(m::HAIRLINE), t.dim);
                        }
                        let st = Style { color: ink, ..label };
                        self.fonts.draw(scene, st, tx, ry + self.px(17.0), &title);
                        ry += rh_row;
                    }
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
            if !matches!(control_kind, 0) {
                scene.hline(cx, y + rh - self.px(m::HAIRLINE), maxw, self.px(m::HAIRLINE), t.tint);
            }
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
                self.fonts.draw(scene, Style { color: t.dim, ..code }, cx, ly, &format!("{:>3}", n + 1));
                let mut x = cx + self.px(36.0);
                let limit = cx + maxw;
                for (kind, tok) in luau_tokens(line) {
                    let color = match kind {
                        Tok::Comment => t.dim,
                        Tok::Keyword => crate::theme_edit::from_rgb(t.ansi[12]),
                        Tok::Str => crate::theme_edit::from_rgb(t.ansi[10]),
                        Tok::Num => crate::theme_edit::from_rgb(t.ansi[13]),
                        Tok::Name => crate::theme_edit::from_rgb(t.ansi[11]),
                        Tok::Plain => ink,
                    };
                    if x >= limit {
                        break;
                    }
                    x += self.fonts.draw(scene, Style { color, ..code }, x, ly, &tok);
                }
                ly += lh;
            }
            scene.layer(None);
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tok {
    Comment,
    Keyword,
    Str,
    Num,
    Name,
    Plain,
}

/// A small Luau tokenizer for the RULES listing: comments, strings,
/// numbers, keywords, the name after `function`.
fn luau_tokens(line: &str) -> Vec<(Tok, String)> {
    const KW: [&str; 16] = ["function", "end", "if", "then", "else", "elseif", "return", "local", "and", "or", "not", "nil", "true", "false", "for", "in"];
    let mut out = Vec::new();
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    let mut after_function = false;
    while i < chars.len() {
        let c = chars[i];
        if c == '-' && chars.get(i + 1) == Some(&'-') {
            out.push((Tok::Comment, chars[i..].iter().collect()));
            break;
        }
        if c == '"' || c == '\'' {
            let q = c;
            let mut j = i + 1;
            while j < chars.len() && chars[j] != q {
                j += 1;
            }
            let end = (j + 1).min(chars.len());
            out.push((Tok::Str, chars[i..end].iter().collect()));
            i = end;
            continue;
        }
        if c.is_ascii_digit() {
            let mut j = i;
            while j < chars.len() && (chars[j].is_ascii_digit() || chars[j] == '.') {
                j += 1;
            }
            out.push((Tok::Num, chars[i..j].iter().collect()));
            i = j;
            continue;
        }
        if c.is_alphabetic() || c == '_' {
            let mut j = i;
            while j < chars.len() && (chars[j].is_alphanumeric() || chars[j] == '_') {
                j += 1;
            }
            let word: String = chars[i..j].iter().collect();
            let kind = if KW.contains(&word.as_str()) {
                Tok::Keyword
            } else if after_function {
                Tok::Name
            } else {
                Tok::Plain
            };
            after_function = word == "function";
            out.push((kind, word));
            i = j;
            continue;
        }
        let mut j = i;
        while j < chars.len() && !(chars[j].is_alphanumeric() || chars[j] == '_' || chars[j] == '"' || chars[j] == '\'' || (chars[j] == '-' && chars.get(j + 1) == Some(&'-'))) {
            j += 1;
        }
        if j == i {
            j = i + 1;
        }
        if !chars[i..j].iter().all(|c| c.is_whitespace()) {
            after_function = false;
        }
        out.push((Tok::Plain, chars[i..j].iter().collect()));
        i = j;
    }
    out
}
