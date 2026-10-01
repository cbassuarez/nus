//! The sky, alive: what the Home art "the sky" does once it has a place.
//!
//! The renderer paints the real sky (nus-render's `Celestial`); the almanac
//! (nus-astro) says where everything is. This is the app between them, and the
//! quiet things a person can do there — each a small switch in EXPERIMENTS:
//!
//! * click a star, a planet, the Sun or the Moon and its name settles beside it,
//!   in the sky's own layer, as plain words;
//! * hold Ctrl+Shift and the constellations are drawn in, a stroke at a time;
//! * scroll over the sky and the sky's clock turns (Shift: days, Alt: months);
//! * leave it alone for a while and the sky drifts forward, an hour a minute;
//! * when the Moon crosses the Sun, the view turns to look.
//!
//! Nothing here reaches the network, and none of it runs without a Place.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use nus_astro as astro;
use nus_render::sky::{Celestial, Mark, MAX_PLANETS};
use nus_render::text::Style;
use nus_render::{Color, Rect, Scene};

use crate::app::{fade, App, Pane};
use crate::art::{Backdrop, Cmd};

fn yes() -> bool {
    true
}

/// The quiet extras, each a switch (settings · EXPERIMENTS, found with a chord).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Eggs {
    /// The chord has been found; the page is on the list from then on.
    #[serde(default)]
    pub found: bool,
    /// Click a star (or planet, Sun, Moon) and it says its name.
    #[serde(default = "yes")]
    pub names: bool,
    /// Scroll over the sky to turn its clock.
    #[serde(default = "yes")]
    pub travel: bool,
    /// Hold Ctrl+Shift to draw the constellations in.
    #[serde(default = "yes")]
    pub figures: bool,
    /// Leave the sky alone and it drifts on.
    #[serde(default = "yes")]
    pub idle: bool,
    /// `sky`, `moon`, `tonight`, `eclipse` at the prompt; the sky's one dry line.
    #[serde(default = "yes")]
    pub almanac: bool,
    /// A dry word for exit status 42, 127, 130.
    #[serde(default = "yes")]
    pub manners: bool,
    /// Press and hold the masthead number to see the sky it was made under.
    #[serde(default = "yes")]
    pub badge: bool,
    /// Finished-work notices wait for an eclipse's totality to end.
    #[serde(default)]
    pub wait: bool,
}

impl Default for Eggs {
    fn default() -> Self {
        Self { found: false, names: true, travel: true, figures: true, idle: true, almanac: true, manners: true, badge: true, wait: false }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Egg {
    Names,
    Travel,
    Figures,
    Idle,
    Almanac,
    Manners,
    Badge,
    Wait,
}

impl Egg {
    pub const ALL: [Egg; 8] = [Egg::Names, Egg::Travel, Egg::Figures, Egg::Idle, Egg::Almanac, Egg::Manners, Egg::Badge, Egg::Wait];

    pub fn label(self) -> &'static str {
        match self {
            Egg::Names => "NAME A STAR",
            Egg::Travel => "TURN THE SKY'S CLOCK",
            Egg::Figures => "DRAW THE CONSTELLATIONS",
            Egg::Idle => "LET THE SKY DRIFT",
            Egg::Almanac => "ALMANAC AT THE PROMPT",
            Egg::Manners => "EXIT STATUS MANNERS",
            Egg::Badge => "SKY OF THIS BUILD",
            Egg::Wait => "NUS WILL WAIT",
        }
    }

    pub fn says(self) -> &'static str {
        match self {
            Egg::Names => "Click a star, a planet, the Sun or the Moon on the sky and its name settles beside it.",
            Egg::Travel => "Scroll over the sky to turn its clock: a notch is twenty minutes, Shift a day, Alt a month. Esc returns to now.",
            Egg::Figures => "Hold Ctrl and Shift and the constellations are drawn in, a stroke at a time, with their names.",
            Egg::Idle => "Leave the sky alone for a minute and it drifts forward — an hour of stars a minute. Any input brings it back.",
            Egg::Almanac => "Type sky, moon, tonight or eclipse and the prompt answers; the sky carries one dry line when something is on.",
            Egg::Manners => "A dry word beside an exit status of 42, 127 or 130.",
            Egg::Badge => "Press and hold the masthead number to see the sky as it was when this build was made.",
            Egg::Wait => "A finished command's notice holds off while an eclipse is total where you are, and comes after.",
        }
    }
}

impl Eggs {
    pub fn get(&self, egg: Egg) -> bool {
        match egg {
            Egg::Names => self.names,
            Egg::Travel => self.travel,
            Egg::Figures => self.figures,
            Egg::Idle => self.idle,
            Egg::Almanac => self.almanac,
            Egg::Manners => self.manners,
            Egg::Badge => self.badge,
            Egg::Wait => self.wait,
        }
    }

    pub fn set(&mut self, egg: Egg, on: bool) {
        match egg {
            Egg::Names => self.names = on,
            Egg::Travel => self.travel = on,
            Egg::Figures => self.figures = on,
            Egg::Idle => self.idle = on,
            Egg::Almanac => self.almanac = on,
            Egg::Manners => self.manners = on,
            Egg::Badge => self.badge = on,
            Egg::Wait => self.wait = on,
        }
    }
}

// ── the chord ───────────────────────────────────────────────────────────

/// A key, as the chord sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pressed {
    Up,
    Down,
    Left,
    Right,
    Letter(char),
    Other,
}

const CHORD: [Pressed; 10] = [
    Pressed::Up,
    Pressed::Up,
    Pressed::Down,
    Pressed::Down,
    Pressed::Left,
    Pressed::Right,
    Pressed::Left,
    Pressed::Right,
    Pressed::Letter('b'),
    Pressed::Letter('a'),
];

/// One step along the chord: the new progress, and whether it is complete.
pub fn chord_step(progress: usize, key: Pressed) -> (usize, bool) {
    if key == CHORD[progress.min(CHORD.len() - 1)] {
        let next = progress + 1;
        return if next == CHORD.len() { (0, true) } else { (next, false) };
    }
    // A wrong key may still be the first of a fresh try.
    (usize::from(key == CHORD[0]), false)
}

// ── words about the things in the sky ───────────────────────────────────

/// "red supergiant", "white main-sequence star", "orange giant", from a Hipparcos spectral type.
pub fn spectral_words(spectral: &str) -> String {
    let s = spectral.trim();
    let mut chars = s.chars();
    let colour = match chars.next() {
        Some('O') => "blue",
        Some('B') => "blue-white",
        Some('A') => "white",
        Some('F') => "yellow-white",
        Some('G') => "yellow",
        Some('K') => "orange",
        Some('M') => "red",
        _ => return String::new(),
    };
    // After the class and its subtype (digits, a dot) comes the luminosity class, in Roman numerals.
    let rest: String = chars.skip_while(|c| c.is_ascii_digit() || *c == '.' || *c == ':').collect();
    let numeral: String = rest.chars().take_while(|c| matches!(c, 'I' | 'V')).collect();
    let kind = match numeral.as_str() {
        "I" => "supergiant",
        "II" => "bright giant",
        "III" => "giant",
        "IV" => "subgiant",
        "V" => "main-sequence star",
        _ => "star",
    };
    format!("{colour} {kind}")
}

fn grouped(n: i64) -> String {
    let digits = n.abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 {
        out.insert(0, '−');
    }
    out
}

/// "8.6 light-years", "~550 light-years": two figures, a tilde when the parallax is shaky.
pub fn distance_words(ly: f32, well_measured: bool) -> String {
    let n = if ly < 10.0 {
        format!("{ly:.1}")
    } else if ly < 100.0 {
        format!("{:.0}", ly)
    } else {
        let step = if ly < 1000.0 { 10.0 } else { 50.0 };
        grouped(((ly / step).round() * step) as i64)
    };
    format!("{}{n} light-years", if well_measured { "" } else { "~" })
}

fn magnitude_words(m: f32) -> String {
    format!("magnitude {}{:.1}", if m < 0.0 { "−" } else { "" }, m.abs())
}

fn compass8(az: f32) -> &'static str {
    const N: [&str; 8] = ["north", "northeast", "east", "southeast", "south", "southwest", "west", "northwest"];
    N[((az / std::f32::consts::FRAC_PI_4).round() as i64).rem_euclid(8) as usize]
}

/// The faintest star the sky will show with the Sun this far below (negative) the horizon.
pub fn limiting_magnitude(sun_alt_deg: f32) -> f32 {
    if sun_alt_deg < 0.0 {
        6.6 - 7.1 * ((sun_alt_deg + 18.0) / 18.0).clamp(0.0, 1.0).powf(0.8)
    } else {
        -0.5 - 0.4 * sun_alt_deg
    }
}

// ── the camera, as the renderer has it ──────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub azimuth: f32,
    pub elevation: f32,
    pub fov: f32,
}

impl Camera {
    /// A East/Up/North direction as pixels from the top-left of a `w × h` view,
    /// and how far ahead of the camera it is. None if it is behind us.
    pub fn project(&self, d: [f32; 3], w: f32, h: f32) -> Option<(f32, f32)> {
        let (sa, ca) = self.azimuth.sin_cos();
        let (se, ce) = self.elevation.sin_cos();
        let right = [ca, 0.0, -sa];
        let up = [-sa * se, ce, -ca * se];
        let forward = [sa * ce, se, ca * ce];
        let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        let z = dot(d, forward);
        if z <= 0.02 {
            return None;
        }
        let t = z * (self.fov * 0.5).tan();
        let (x, y) = (dot(d, right) / (t * (w / h)), dot(d, up) / t);
        Some(((x * 0.5 + 0.5) * w, (0.5 - y * 0.5) * h))
    }
}

fn wrap_pi(a: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    let a = a.rem_euclid(tau);
    if a > std::f32::consts::PI {
        a - tau
    } else {
        a
    }
}

fn aim(d: [f32; 3]) -> (f32, f32) {
    (d[0].atan2(d[2]), d[1].clamp(-1.0, 1.0).asin())
}

/// East/Up/North of a J2000 catalogue direction.
fn rotate(m: &[[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
    [0, 1, 2].map(|i| m[i][0] * v[0] + m[i][1] * v[1] + m[i][2] * v[2])
}

// ── what a pick is, and what is drawn ───────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq)]
enum Target {
    Star(u16),
    Planet(usize),
    Sun,
    Moon,
}

struct Label {
    /// The thing's place in the view, pixels.
    at: (f32, f32),
    radius: f32,
    name: String,
    detail: String,
    alpha: f32,
}

#[derive(Default)]
pub(crate) struct Overlay {
    label: Option<Label>,
    /// A constellation name and where to hang it, with its alpha.
    figures: Vec<(String, f32, f32, f32)>,
    /// Quiet lines for the foot of the sky, first on top.
    notes: Vec<String>,
    /// Light words on a dark sky, or dark on light.
    dark: bool,
    /// A streak across the sky, 0..1 along its way, while the Unix clock turns over a round number.
    streak: Option<f32>,
}

struct ViewState {
    frame_weight: f32,
    /// The view an eclipse last asked for, kept while it eases back out.
    focus_cam: Option<Camera>,
    lines: f32,
    held_since: Option<Instant>,
    pick: Option<(Target, Instant)>,
    last: Instant,
}

struct Schedule {
    place: (f32, f32),
    from_ms: f64,
    to_ms: f64,
    events: Vec<astro::Event>,
}

/// What is coming for a place, worked out off the UI thread and kept for a while.
#[derive(Default)]
struct Almanac {
    schedule: Option<Schedule>,
    slot: Arc<Mutex<Option<Schedule>>>,
    since: Option<Instant>,
}

impl Almanac {
    /// The events around `ms` for `place`, if they are worked out; asks for them if not.
    fn events(&mut self, ms: f64, place: (f32, f32)) -> Option<&[astro::Event]> {
        if let Ok(mut slot) = self.slot.try_lock() {
            if let Some(done) = slot.take() {
                self.schedule = Some(done);
                self.since = None;
            }
        }
        let fresh = self.schedule.as_ref().is_some_and(|s| s.place == place && ms >= s.from_ms + astro::DAY_MS && ms <= s.to_ms - 60.0 * astro::DAY_MS);
        if !fresh && !self.since.is_some_and(|t| t.elapsed() < Duration::from_secs(20)) {
            self.since = Some(Instant::now());
            let slot = self.slot.clone();
            let obs = astro::Observer::new(f64::from(place.0), f64::from(place.1));
            std::thread::spawn(move || {
                let (from_ms, days) = (ms - 2.0 * astro::DAY_MS, 400.0);
                let events = astro::upcoming(from_ms, Some(&obs), days);
                if let Ok(mut s) = slot.lock() {
                    *s = Some(Schedule { place, from_ms, to_ms: from_ms + days * astro::DAY_MS, events });
                }
            });
        }
        self.schedule.as_ref().filter(|s| s.place == place).map(|s| s.events.as_slice())
    }
}

/// A finished-work notice that is waiting for an eclipse to be over.
pub(crate) enum Deferred {
    Completion(crate::hatch_work::Item),
    Attention,
    Notice(String, String),
}

pub struct SkyView {
    /// Where the viewer has turned the sky's clock, ms ahead (or behind) of the machine's.
    pub offset_ms: f64,
    /// What idling has added on top, ms; it unwinds when you come back.
    idle_ms: f64,
    last_tick: Option<Instant>,
    last_input: Instant,
    last_mouse: (f32, f32),
    chord: usize,
    views: HashMap<u64, ViewState>,
    overlays: HashMap<u64, Overlay>,
    /// The almanac around the sky time (what the foot of the sky says) and around the
    /// machine time (when nus will wait).
    shown: Almanac,
    real: Almanac,
    pub(crate) deferred: Vec<Deferred>,
    /// The prompt's almanac answers, worked out off the UI thread.
    pub(crate) memo: Arc<Mutex<Memo>>,
    /// A test hook: draw the figures as if Ctrl+Shift were held.
    pub(crate) force_figures: bool,
    /// What the sky last said, for the shot driver's assertions.
    pub(crate) last_label: Option<String>,
    pub(crate) last_notes: Vec<String>,
    /// The Space page: clicks waiting to be matched to a star, and the star picked.
    pub(crate) space_taps: Vec<(f32, f32)>,
    space_pick: Option<(u64, u16, Instant)>,
}

impl Default for SkyView {
    fn default() -> Self {
        Self {
            offset_ms: 0.0,
            idle_ms: 0.0,
            last_tick: None,
            last_input: crate::clock::now(),
            last_mouse: (0.0, 0.0),
            chord: 0,
            views: HashMap::new(),
            overlays: HashMap::new(),
            shown: Almanac::default(),
            real: Almanac::default(),
            deferred: Vec::new(),
            memo: Arc::new(Mutex::new(Memo::default())),
            force_figures: false,
            last_label: None,
            last_notes: Vec::new(),
            space_taps: Vec::new(),
            space_pick: None,
        }
    }
}

/// The machine's time, in unix ms; NUS_CLOCK pins it (for photographs of an eclipse at noon).
fn machine_now_ms() -> f64 {
    if let Some(ms) = std::env::var("NUS_CLOCK").ok().and_then(|v| v.parse::<f64>().ok()) {
        return ms;
    }
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0)
}

const IDLE_AFTER: f32 = 45.0;
const IDLE_RATE: f64 = 60.0;
const PICK_HOLD: f32 = 9.0;

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl SkyView {
    /// The time the sky shows: the machine's, turned by the viewer, carried on by idling.
    pub fn now_ms(&self) -> f64 {
        machine_now_ms() + self.offset_ms + self.idle_ms
    }

    pub fn travelling(&self) -> bool {
        self.offset_ms.abs() > 1000.0
    }

    /// Back to now.
    pub fn home(&mut self) {
        self.offset_ms = 0.0;
        self.idle_ms = 0.0;
    }

    /// Jump the sky to a moment.
    pub fn jump_to(&mut self, ms: f64) {
        self.offset_ms = ms - machine_now_ms();
        self.idle_ms = 0.0;
    }

    /// Turn the clock by `ms`, within two centuries either way.
    pub fn turn(&mut self, ms: f64) {
        let limit = 200.0 * 365.25 * astro::DAY_MS;
        self.offset_ms = (self.offset_ms + ms).clamp(-limit, limit);
    }

    /// A frame passes: idling carries the sky on, input brings it back.
    fn tick(&mut self, now: Instant, mouse: (f32, f32), last_key: Instant, idle_on: bool, still: bool) {
        let dt = self.last_tick.map_or(0.0, |t| now.saturating_duration_since(t).as_secs_f64().min(0.25));
        self.last_tick = Some(now);
        if mouse != self.last_mouse {
            self.last_mouse = mouse;
            self.last_input = now;
        }
        if last_key > self.last_input {
            self.last_input = last_key;
        }
        let idle_for = now.saturating_duration_since(self.last_input).as_secs_f32();
        if idle_on && !still && idle_for > IDLE_AFTER {
            let rate = 1.0 + (IDLE_RATE - 1.0) * f64::from(smoothstep(IDLE_AFTER, IDLE_AFTER + 10.0, idle_for));
            self.idle_ms += dt * 1000.0 * (rate - 1.0);
        } else if self.idle_ms != 0.0 {
            // Back from the drift: a quick, smooth unwinding rather than a jump.
            self.idle_ms *= (-dt / 0.35).exp();
            if self.idle_ms.abs() < 1000.0 {
                self.idle_ms = 0.0;
            }
        }
    }
}

// ── the App's side ──────────────────────────────────────────────────────

impl App {
    /// The sky's time for the art (and the prompt's almanac).
    pub(crate) fn sky_now_ms(&self) -> f64 {
        self.sky_view.now_ms()
    }

    /// Whether the focused pane is a Home on the sky with a Place, its rect, and whether its line is empty.
    fn sky_home(&self) -> Option<(Rect, bool)> {
        if self.behavior.home_art != "sky" || self.behavior.home_look != crate::settings::HomeLook::Art || self.place().is_none() {
            return None;
        }
        match self.tabs.get(self.active).map(|t| t.focused_ref())? {
            Pane::Home(h) if !h.library => Some((h.rect, h.input.is_empty())),
            _ => None,
        }
    }

    /// The chord, and Esc back to now. True when it took the key.
    pub(crate) fn sky_key(&mut self, ev: &crate::app::KeyIn) -> bool {
        use winit::keyboard::{Key, NamedKey};
        if ev.state != winit::event::ElementState::Pressed {
            return false;
        }
        let Some(Pane::Home(h)) = self.tabs.get(self.active).map(|t| t.focused_ref()) else { return false };
        let empty = h.input.is_empty() || h.input == "b";
        if self.mods.control_key() || self.mods.alt_key() || self.mods.super_key() {
            return false;
        }
        if matches!(ev.logical_key, Key::Named(NamedKey::Escape)) && h.input.is_empty() && self.sky_view.travelling() && self.sky_home().is_some() {
            self.sky_view.home();
            self.dirty = true;
            return true;
        }
        if !empty {
            self.sky_view.chord = 0;
            return false;
        }
        let key = match &ev.logical_key {
            Key::Named(NamedKey::ArrowUp) => Pressed::Up,
            Key::Named(NamedKey::ArrowDown) => Pressed::Down,
            Key::Named(NamedKey::ArrowLeft) => Pressed::Left,
            Key::Named(NamedKey::ArrowRight) => Pressed::Right,
            Key::Character(c) => c.chars().next().map_or(Pressed::Other, |c| Pressed::Letter(c.to_ascii_lowercase())),
            _ => Pressed::Other,
        };
        let (progress, done) = chord_step(self.sky_view.chord, key);
        self.sky_view.chord = progress;
        if !done {
            return false;
        }
        // The `b` is on the line already; take it and the `a` away.
        if let Some(Pane::Home(h)) = self.tabs.get_mut(self.active).map(|t| t.focused()) {
            h.input.clear();
            h.cur = Default::default();
            h.sel = 0;
        }
        self.behavior.eggs.found = true;
        self.save_prefs();
        self.open_settings_at(crate::settings::SEC_EXPERIMENTS, None);
        self.toast(nus_render::text::icons::PLANET, "Experiments", "small things, each a switch", None);
        self.dirty = true;
        true
    }

    /// The wheel over the sky turns its clock. True when it took the wheel.
    pub(crate) fn sky_wheel(&mut self, x: f32, y: f32, dy_px: f32) -> bool {
        if !self.behavior.eggs.travel {
            return false;
        }
        let Some((rect, empty)) = self.sky_home() else { return false };
        if !empty || !rect.contains(x, y) || self.palette.is_some() {
            return false;
        }
        let notches = f64::from(-dy_px / self.wheel_step().max(1.0));
        let unit = if self.mods.alt_key() {
            30.0 * astro::DAY_MS
        } else if self.mods.shift_key() {
            astro::DAY_MS
        } else {
            20.0 * 60_000.0
        };
        self.sky_view.turn(notches * unit);
        self.dirty = true;
        self.request_sky_frame();
        true
    }

    /// Whether an eclipse is total where you are, right now: the Sun wholly covered (or ringed),
    /// or the Moon wholly in the Earth shadow. By the machine clock, not the sky clock.
    pub(crate) fn sky_waiting(&mut self) -> bool {
        if !self.behavior.eggs.wait {
            return false;
        }
        let Some(place) = self.place() else { return false };
        let now = machine_now_ms();
        let Some(events) = self.sky_view.real.events(now, place) else { return false };
        events.iter().any(|e| match &e.kind {
            astro::EventKind::Solar(s) => s.central_start_ms.zip(s.central_end_ms).is_some_and(|(a, b)| now >= a && now <= b),
            astro::EventKind::Lunar(l) => l.visible && l.total_start_ms.zip(l.total_end_ms).is_some_and(|(a, b)| now >= a && now <= b),
            _ => false,
        })
    }

    /// Hold a notice back until the eclipse is over; `sky_released` hands them back after.
    pub(crate) fn sky_defer(&mut self, d: Deferred) {
        self.sky_view.deferred.push(d);
    }

    pub(crate) fn sky_released(&mut self) -> Vec<Deferred> {
        std::mem::take(&mut self.sky_view.deferred)
    }

    /// Turn the sky's clock to a moment, and make sure the Home is showing the sky to see it.
    pub(crate) fn sky_jump(&mut self, ms: f64) {
        self.sky_view.jump_to(ms);
        if self.place().is_some() && (self.behavior.home_art != "sky" || self.behavior.home_look != crate::settings::HomeLook::Art) {
            self.behavior.home_look = crate::settings::HomeLook::Art;
            self.behavior.home_art = "sky".into();
            self.save_prefs();
        }
        self.dirty = true;
        self.request_sky_frame();
    }

    /// What the prompt says when the line is `sky`, `moon`, `tonight` or `eclipse`. The
    /// answers are worked out off the UI thread (an eclipse search looks years ahead) and
    /// the rows refresh as the prompt does.
    pub(crate) fn almanac_rows(&self, q: &str) -> Vec<crate::app::PaletteRow> {
        use crate::app::{Action, PaletteRow};
        if !self.behavior.eggs.almanac {
            return Vec::new();
        }
        let word = q.trim().to_lowercase();
        if !matches!(word.as_str(), "sky" | "moon" | "tonight" | "eclipse") {
            return Vec::new();
        }
        let ms = self.sky_view.now_ms();
        let place = self.place();
        // Reports are good for a minute; an eclipse search for a few hours.
        let bucket = (ms / if word == "eclipse" { 6.0 * 3_600_000.0 } else { 60_000.0 }) as i64;
        let key = (word.clone(), place.map(|(la, lo)| ((la * 1000.0) as i32, (lo * 1000.0) as i32)), bucket);
        let say = |text: &str, how: &Say| PaletteRow {
            num: "·".into(),
            text: text.to_string(),
            action: match how {
                Say::Noop => Action::Noop,
                Say::Copy(all) => Action::SkyCopy(all.clone()),
                Say::Jump(at) => Action::SkyJump(*at),
                Say::Place => Action::SettingsAt(crate::settings::SEC_STARTUP, None),
            },
        };
        let Ok(mut memo) = self.sky_view.memo.lock() else { return Vec::new() };
        if memo.key.as_ref() == Some(&key) {
            return memo.rows.iter().map(|(t, h)| say(t, h)).collect();
        }
        if memo.working.as_ref() != Some(&key) {
            memo.working = Some(key.clone());
            let slot = self.sky_view.memo.clone();
            let tz = astro::local_offset_minutes();
            std::thread::spawn(move || {
                let obs = place.map(|(la, lo)| astro::Observer::new(f64::from(la), f64::from(lo)));
                let rows = build_almanac(&key.0, ms, obs.as_ref(), tz);
                if let Ok(mut m) = slot.lock() {
                    m.key = Some(key);
                    m.rows = rows;
                }
            });
        }
        // What was said last time stays up while the new answer is worked out.
        if memo.key.as_ref().is_some_and(|k| k.0 == word) {
            return memo.rows.iter().map(|(t, h)| say(t, h)).collect();
        }
        vec![say("Working it out…", &Say::Noop)]
    }

    /// Everything the sky does for one frame of a Home on the sky: lay the real sky
    /// into the atmosphere command, turn the view to an eclipse, find what was clicked,
    /// and decide what to say. Returns the text polarity the real sky wants.
    pub(crate) fn sky_apply(&mut self, cmds: &mut [Cmd], view_id: u64, taps: &[(f32, f32)]) -> Option<Backdrop> {
        let (lat, lon) = self.place()?;
        let idx = cmds.iter().position(|c| matches!(c, Cmd::Atmosphere(.., true)))?;
        let now = crate::clock::now();
        let still = self.art_budget() == crate::power::Budget::Still || self.motion.reduced();
        let (mouse, last_key, idle_on) = (self.mouse, self.last_key, self.behavior.eggs.idle);
        self.sky_view.tick(now, mouse, last_key, idle_on, still);
        let ms = self.sky_view.now_ms();
        let obs = astro::Observer::new(f64::from(lat), f64::from(lon));
        let frame = astro::sky_frame(ms, &obs);
        let eggs = self.behavior.eggs;
        let tz = astro::local_offset_minutes();
        let held = self.sky_view.force_figures || (self.mods.control_key() && self.mods.shift_key() && !self.mods.alt_key());
        let scale = self.scale;

        let Cmd::Atmosphere(rect, params, _, _, _) = &mut cmds[idx] else { return None };
        let (w, h) = (rect.w.max(1.0).ceil(), rect.h.max(1.0).ceil());
        let state = self.sky_view.views.entry(view_id).or_insert_with(|| ViewState { frame_weight: 0.0, focus_cam: None, lines: 0.0, held_since: None, pick: None, last: now });
        let dt = now.saturating_duration_since(state.last).as_secs_f32().min(0.5);
        state.last = now;

        // The sky itself.
        params.sun_direction = frame.sun_direction;
        params.moon_direction = frame.moon_direction;
        params.moon_illumination = frame.moon_illumination;
        params.moon_waxing = frame.moon_waxing;

        // An eclipse turns the view to look at it, and eases back when it is over.
        let focus = if frame.sun_visible < 0.995 && frame.sun_altitude > -0.01 {
            Some((frame.sun_direction, 0.20))
        } else if frame.umbral_magnitude > 0.05 && frame.moon_altitude > 0.0 {
            Some((frame.moon_direction, 0.26))
        } else {
            None
        };
        if let Some((d, fov)) = focus {
            let (az, el) = aim(d);
            // Hang it in the open sky, clear of the prompt: low in the corner when it is
            // high enough to aim under, and to one side when the horizon will not let us.
            let aspect = w / h;
            let half: f32 = fov * 0.5;
            let side = 0.78 * half.tan() * aspect / el.cos().max(0.3);
            state.focus_cam = Some(Camera { azimuth: az + side.atan(), elevation: (el - 0.30 * fov * 0.5).max(0.12), fov });
        }
        let goal = f32::from(focus.is_some());
        // Reduced motion (and a first look) takes the view at once rather than gliding to it.
        let glide = if self.motion.reduced() { 1.0 } else { 1.0 - (-dt / 1.6).exp() };
        state.frame_weight += (goal - state.frame_weight) * glide;
        if goal == 0.0 && state.frame_weight < 0.003 {
            state.frame_weight = 0.0;
            state.focus_cam = None;
        }
        if let (Some(cam), true) = (state.focus_cam, state.frame_weight > 0.0) {
            let t = state.frame_weight * state.frame_weight * (3.0 - 2.0 * state.frame_weight);
            params.view_azimuth += wrap_pi(cam.azimuth - params.view_azimuth) * t;
            params.view_elevation += (cam.elevation - params.view_elevation) * t;
            params.fov_y += (cam.fov - params.fov_y) * t;
        }

        // The constellations, drawn in while Ctrl+Shift are held.
        if eggs.figures && held {
            state.held_since.get_or_insert(now);
        } else {
            state.held_since = None;
        }
        let ready = state.held_since.is_some_and(|t| now.saturating_duration_since(t) > Duration::from_millis(350));
        let want = f32::from(ready);
        let rate = if want > state.lines { 1.0 / 2.6 } else { 1.0 / 0.9 };
        state.lines = if want > state.lines { (state.lines + dt * rate).min(want) } else { (state.lines - dt * rate).max(want) };

        let mut marks = [None; MAX_PLANETS];
        for (slot, m) in marks.iter_mut().zip(&frame.planets) {
            *slot = Some(Mark { direction: m.direction, magnitude: m.magnitude, bv: m.bv });
        }
        params.celestial = Some(Celestial {
            rotation: frame.rotation,
            planets: marks,
            sun_radius: frame.sun_radius,
            moon_radius: frame.moon_radius,
            sun_visible: frame.sun_visible,
            corona: frame.corona,
            ecliptic_north: frame.ecliptic_north,
            shadow_direction: frame.shadow_direction,
            umbra_radius: frame.umbra_radius,
            penumbra_radius: frame.penumbra_radius,
            moon_glow: frame.moon_glow,
            lines: state.lines,
            // Modelled clouds part for an eclipse; a real forecast's clouds are left as they are.
            clear_sky: if self.behavior.sky_weather { 0.0 } else { state.frame_weight },
        });
        let camera = Camera { azimuth: params.view_azimuth, elevation: params.view_elevation.clamp(0.12, 1.35), fov: params.fov_y.clamp(0.2, 1.6) };
        let lines_now = state.lines;

        // What is dark: the sky an eclipse has dimmed counts.
        let dark = frame.sun_altitude < -0.05 || frame.sun_visible < 0.45;
        let mut overlay = Overlay { dark, ..Default::default() };

        // What can be named: bright enough to see, above the horizon, in view.
        let sun_alt_deg = frame.sun_altitude.to_degrees();
        // An eclipse dips the sky toward twilight, as the shader's sun_y() does.
        let effective_alt = if frame.sun_visible < 0.999 {
            let (sy, light) = (frame.sun_altitude.sin(), frame.sun_visible.clamp(0.0, 1.0).powf(0.55));
            (sy.min(-0.075 + (sy + 0.075) * light)).clamp(-1.0, 1.0).asin().to_degrees()
        } else {
            sun_alt_deg
        };
        let limit = limiting_magnitude(effective_alt) - 0.3;
        let reach = (22.0 * scale).max(14.0);
        let mut visible: Vec<(Target, (f32, f32), f32, f32)> = Vec::new(); // target, pixels, score bias, radius
        let place_pick = |t: Target, d: [f32; 3], mag: f32, radius_rad: f32, out: &mut Vec<(Target, (f32, f32), f32, f32)>| {
            if d[1] < 0.0 {
                return;
            }
            if let Some(p) = camera.project(d, w, h) {
                let radius_px = radius_rad / (camera.fov * 0.5).tan() * 0.5 * h;
                out.push((t, p, mag, radius_px));
            }
        };
        let catalogue = nus_render::space::catalogue();
        for n in nus_render::space::star_names() {
            let star = &catalogue[n.star as usize];
            let d = rotate(&frame.rotation, star.direction);
            // Air dims what is low: the same extinction the sky paints.
            let mag = star.magnitude + 0.2 / (d[1].max(0.0) + 0.025 * (-11.0 * d[1].max(0.0)).exp());
            if mag < limit {
                place_pick(Target::Star(n.star), d, star.magnitude, 0.0, &mut visible);
            }
        }
        for (i, m) in frame.planets.iter().enumerate() {
            if m.magnitude < limit && m.planet.naked_eye() {
                place_pick(Target::Planet(i), m.direction, m.magnitude, 0.0, &mut visible);
            }
        }
        place_pick(Target::Sun, frame.sun_direction, -26.0, frame.sun_radius, &mut visible);
        place_pick(Target::Moon, frame.moon_direction, -12.0, frame.moon_radius, &mut visible);

        // A click on one of them; a click on nothing lets go.
        if eggs.names {
            for &(tx, ty) in taps {
                let mut best: Option<(f32, Target)> = None;
                for &(t, (px, py), mag, radius) in &visible {
                    let d = ((px - tx).powi(2) + (py - ty).powi(2)).sqrt();
                    if d > reach.max(radius) {
                        continue;
                    }
                    // Nearer wins; brighter breaks a tie.
                    let score = d - (6.0 - mag.max(-6.0)) * 0.9;
                    if best.is_none_or(|(s, _)| score < s) {
                        best = Some((score, t));
                    }
                }
                let state = self.sky_view.views.get_mut(&view_id).expect("view");
                state.pick = best.map(|(_, t)| (t, now));
            }
        }
        // The one that stands picked: where it is now, and what to say.
        let state = self.sky_view.views.get_mut(&view_id).expect("view");
        if let Some((target, since)) = state.pick {
            let age = now.saturating_duration_since(since).as_secs_f32();
            let alpha = smoothstep(0.0, 0.25, age) * (1.0 - smoothstep(PICK_HOLD, PICK_HOLD + 1.4, age));
            if age > PICK_HOLD + 1.4 || self.last_key > since {
                state.pick = None;
            } else if let Some(&(_, at, _, radius)) = visible.iter().find(|v| v.0 == target) {
                overlay.label = Some(describe(target, &frame, ms, &obs, at, radius, alpha));
            } else {
                state.pick = None;
            }
        }

        // The figures' names, once they are drawn in.
        if lines_now > 0.85 {
            let alpha = smoothstep(0.85, 1.0, lines_now);
            for f in nus_render::space::figures() {
                let d = rotate(&frame.rotation, f.anchor);
                if d[1] < 0.08 {
                    continue;
                }
                if let Some((x, y)) = camera.project(d, w, h) {
                    if x > w * 0.04 && x < w * 0.96 && y > h * 0.06 && y < h * 0.88 {
                        overlay.figures.push((f.name.to_uppercase(), x, y, alpha));
                    }
                }
            }
        }

        // Notes for the foot of the sky.
        if self.sky_view.travelling() {
            overlay.notes.push(format!(
                "{} · {} · {} · Esc for now",
                astro::format_date_year(ms, tz),
                astro::format_clock(ms, tz),
                relative(self.sky_view.offset_ms)
            ));
        }
        if eggs.almanac {
            if let Some(events) = self.sky_view.shown.events(ms, (lat, lon)) {
                if let Some(note) = astro::prompt_note(events, ms, tz) {
                    overlay.notes.push(note);
                }
                // The Unix clock turning over a round number: one streak across the sky, once.
                overlay.streak = events.iter().filter(|e| matches!(e.kind, astro::EventKind::Unix(_))).find_map(|e| {
                    let t = (ms - e.peak_ms + 200.0) / 1400.0;
                    (0.0..=1.0).contains(&t).then_some(t as f32)
                });
            }
        }
        self.sky_view.last_label = overlay.label.as_ref().map(|l| format!("{} · {}", l.name, l.detail));
        self.sky_view.last_notes = overlay.notes.clone();
        self.sky_view.overlays.insert(view_id, overlay);
        if lines_now > 0.0 && lines_now < 1.0 || self.sky_view.views.get(&view_id).is_some_and(|s| s.frame_weight > 0.0 && s.frame_weight < 1.0) {
            self.dirty = true;
        }
        Some(if dark { Backdrop::Dark } else { Backdrop::Light })
    }

    /// The sky as it was when this build was made — the commit's own moment — as a small
    /// chart over the masthead number: zenith at the centre, north up, east on the left, as
    /// looking up does. Held under a thumb, gone when let go.
    pub(crate) fn draw_build_sky(&mut self, scene: &mut Scene, badge: Rect, card: Rect, alpha: f32) {
        let Some(epoch) = option_env!("NUS_BUILD_EPOCH").and_then(|e| e.trim().parse::<f64>().ok()).filter(|e| *e > 0.0) else { return };
        let ms = epoch * 1000.0;
        let (place, over) = match self.place() {
            Some((la, lo)) => ((f64::from(la), f64::from(lo)), "over your place".to_string()),
            None => ((51.4769, 0.0), "over Greenwich".to_string()),
        };
        let frame = astro::sky_frame(ms, &astro::Observer::new(place.0, place.1));
        let radius = self.px(64.0);
        let mut c = (badge.right() - radius.min(badge.w / 2.0 + self.px(4.0)), badge.y + badge.h / 2.0);
        c.0 = c.0.clamp(card.x + radius + self.px(10.0), card.right() - radius - self.px(10.0));
        c.1 = c.1.clamp(card.y + radius + self.px(10.0), card.bottom() - radius - self.px(34.0));
        let day = frame.sun_altitude.to_degrees();
        let ground: Color = if day > 0.0 {
            [0.30, 0.50, 0.78, 1.0]
        } else if day > -12.0 {
            [0.12, 0.17, 0.32, 1.0]
        } else {
            [0.02, 0.03, 0.07, 1.0]
        };
        let ink: Color = [0.92, 0.95, 1.0, 1.0];
        let ring = |scene: &mut Scene, r: f32, w: f32, col: Color| {
            for i in 0..48 {
                let (a0, a1) = (i as f32 / 48.0 * std::f32::consts::TAU, (i + 1) as f32 / 48.0 * std::f32::consts::TAU);
                let p = |a: f32, rr: f32| [c.0 + a.cos() * rr, c.1 + a.sin() * rr];
                scene.poly(&[p(a0, r - w), p(a1, r - w), p(a1, r + w), p(a0, r + w)], col);
            }
        };
        // The disc, filled as a polygon, then its rim.
        let disc: Vec<[f32; 2]> = (0..64).map(|i| {
            let a = i as f32 / 64.0 * std::f32::consts::TAU;
            [c.0 + a.cos() * radius, c.1 + a.sin() * radius]
        }).collect();
        scene.layer(None);
        scene.poly(&disc, fade(ground, 0.97 * alpha));
        ring(scene, radius, self.px(0.8), fade(ink, 0.6 * alpha));
        // Stereographic about the zenith: the horizon is the rim.
        let at = |d: [f32; 3]| -> Option<(f32, f32)> {
            if d[1] < 0.0 {
                return None;
            }
            let rho = ((std::f32::consts::FRAC_PI_2 - d[1].clamp(-1.0, 1.0).asin()) * 0.5).tan();
            let az = d[0].atan2(d[2]);
            Some((c.0 - rho * radius * az.sin(), c.1 - rho * radius * az.cos()))
        };
        if day < -6.0 {
            for star in nus_render::space::catalogue().iter().filter(|s| s.magnitude < 4.6) {
                let d = rotate(&frame.rotation, star.direction);
                if let Some((x, y)) = at(d) {
                    let size = self.px(0.9 + (4.6 - star.magnitude).max(0.0) * 0.28);
                    scene.rect(Rect::new(x - size / 2.0, y - size / 2.0, size, size), fade(ink, (0.35 + (4.6 - star.magnitude) * 0.17).min(1.0) * alpha));
                }
            }
        }
        for m in &frame.planets {
            if let Some((x, y)) = at(m.direction) {
                if m.magnitude < 2.5 && m.planet.naked_eye() && day < -1.0 {
                    let size = self.px(2.6);
                    scene.rect(Rect::new(x - size / 2.0, y - size / 2.0, size, size), fade([1.0, 0.82, 0.5, 1.0], alpha));
                }
            }
        }
        if let Some((x, y)) = at(frame.moon_direction) {
            let size = self.px(5.0);
            scene.rect(Rect::new(x - size / 2.0, y - size / 2.0, size, size), fade([0.96, 0.97, 1.0, 1.0], alpha));
        }
        if let Some((x, y)) = at(frame.sun_direction) {
            let size = self.px(7.0);
            scene.rect(Rect::new(x - size / 2.0, y - size / 2.0, size, size), fade([1.0, 0.92, 0.6, 1.0], alpha));
        }
        // What it is.
        let tz = astro::local_offset_minutes();
        let words = format!("{} {} · {over}", astro::format_date_year(ms, tz), astro::format_clock(ms, tz));
        let style = Style { font: self.f.ui, px: self.px(10.0), color: fade([0.92, 0.95, 1.0, 1.0], alpha), tracking: 0.0 };
        let w = self.fonts.measure(style, &words);
        let (tx, ty) = ((c.0 - w / 2.0).clamp(card.x + self.px(6.0), card.right() - w - self.px(6.0)), c.1 + radius + self.px(16.0));
        scene.rect(Rect::new(tx - self.px(6.0), ty - self.px(12.0), w + self.px(12.0), self.px(18.0)), fade([0.02, 0.03, 0.07, 1.0], 0.85 * alpha));
        self.fonts.draw(scene, style, tx, ty, &words);
    }

    /// A ring round a picked thing and its name and facts beside it, in plain type.
    fn draw_label(&mut self, scene: &mut Scene, rr: Rect, l: &Label, light: Color, dim: Color) {
        let (cx, cy) = (rr.x + l.at.0, rr.y + l.at.1);
        let ring = (l.radius + self.px(9.0)).max(self.px(9.0));
        let segments = 28;
        for i in 0..segments {
            let (a0, a1) = (i as f32 / segments as f32 * std::f32::consts::TAU, (i + 1) as f32 / segments as f32 * std::f32::consts::TAU);
            let (r0, r1) = (ring - self.px(0.6), ring + self.px(0.6));
            let p = |a: f32, r: f32| [cx + a.cos() * r, cy + a.sin() * r];
            scene.poly(&[p(a0, r0), p(a1, r0), p(a1, r1), p(a0, r1)], fade(light, 0.55 * l.alpha));
        }
        let name = Style { font: self.f.wordmark, px: self.px(16.0), color: fade(light, 0.96 * l.alpha), tracking: 0.0 };
        let facts = Style { font: self.f.ui, px: self.px(10.5), color: fade(dim, 0.92 * l.alpha), tracking: 0.0 };
        let (wn, wf) = (self.fonts.measure(name, &l.name), self.fonts.measure(facts, &l.detail));
        let width = wn.max(wf);
        // Beside the thing; on its other side if there is no room.
        let gap = ring + self.px(7.0);
        let right = cx + gap + width < rr.right() - self.px(16.0);
        let x0 = if right { cx + gap } else { cx - gap - width };
        let y = (cy + self.px(2.0)).clamp(rr.y + self.px(28.0), rr.bottom() - self.px(110.0));
        let anchor = |w: f32| if right { x0 } else { x0 + width - w };
        self.fonts.draw(scene, name, anchor(wn), y, &l.name);
        self.fonts.draw(scene, facts, anchor(wf), y + self.px(16.0), &l.detail);
    }

    /// The Space page: a click on a named star in its fixed view names it, in the same plain
    /// words, drawn inside the star layer. Called as the page draws, with its final camera.
    pub(crate) fn draw_space_names(&mut self, scene: &mut Scene, r: Rect, p: nus_render::space::SpaceParams, size: (u32, u32), view_id: u64) {
        if !self.behavior.eggs.names {
            self.sky_view.space_taps.clear();
            return;
        }
        let now = crate::clock::now();
        let (ow, oh) = (size.0.max(1) as f32, size.1.max(1) as f32);
        let to_local = |px: [f32; 2]| (px[0] / ow * r.w, px[1] / oh * r.h);
        let catalogue = nus_render::space::catalogue();
        let place = |n: &nus_render::space::StarName| -> Option<(f32, f32)> {
            let px = nus_render::space::star_pixel(catalogue[n.star as usize].direction, size, p)?;
            // Behind the planet, or off the edge of the picture, is not clickable.
            (nus_render::space::visibility(px, size, p) > 0.5 && px[0] > 0.0 && px[0] < ow && px[1] > 0.0 && px[1] < oh).then(|| to_local(px))
        };
        let taps = std::mem::take(&mut self.sky_view.space_taps);
        let reach = (22.0 * self.scale).max(14.0);
        for (tx, ty) in taps {
            let mut best: Option<(f32, u16)> = None;
            for n in nus_render::space::star_names() {
                let Some((x, y)) = place(n) else { continue };
                let d = ((x - tx).powi(2) + (y - ty).powi(2)).sqrt();
                if d > reach {
                    continue;
                }
                let score = d - (6.0 - catalogue[n.star as usize].magnitude.max(-6.0)) * 0.9;
                if best.is_none_or(|(s, _)| score < s) {
                    best = Some((score, n.star));
                }
            }
            self.sky_view.space_pick = best.map(|(_, star)| (view_id, star, now));
        }
        let Some((id, star, since)) = self.sky_view.space_pick else { return };
        if id != view_id {
            return;
        }
        let age = now.saturating_duration_since(since).as_secs_f32();
        if age > PICK_HOLD + 1.4 || self.last_key > since {
            self.sky_view.space_pick = None;
            return;
        }
        let Some(n) = nus_render::space::star_name(star as usize) else { return };
        let Some(at) = place(n) else { return };
        let alpha = smoothstep(0.0, 0.25, age) * (1.0 - smoothstep(PICK_HOLD, PICK_HOLD + 1.4, age));
        let (name, detail) = star_words(star);
        let label = Label { at, radius: 0.0, name, detail, alpha };
        let (light, dim): (Color, Color) = ([0.90, 0.94, 1.0, 1.0], [0.66, 0.74, 0.86, 1.0]);
        let clip = scene.clip();
        scene.layer(Some(clip.map_or(r, |c| r.intersect(&c))));
        self.draw_label(scene, r, &label, light, dim);
        scene.layer(clip);
        self.dirty = true;
    }

    /// Draw what the sky has to say, in its own layer: a ring and a name beside a picked
    /// thing, the constellations' names, and the foot's quiet lines.
    pub(crate) fn draw_sky_overlay(&mut self, scene: &mut Scene, rr: Rect, view_id: u64) {
        let Some(o) = self.sky_view.overlays.remove(&view_id) else { return };
        let (light, dim): (Color, Color) = if o.dark { ([0.90, 0.94, 1.0, 1.0], [0.66, 0.74, 0.86, 1.0]) } else { ([0.06, 0.09, 0.14, 1.0], [0.20, 0.26, 0.34, 1.0]) };
        let clip = scene.clip();
        scene.layer(Some(clip.map_or(rr, |c| rr.intersect(&c))));
        for (name, x, y, a) in &o.figures {
            let style = Style { font: self.f.ui, px: self.px(10.0), color: fade(dim, 0.80 * a), tracking: self.px(1.6) };
            let wd = self.fonts.measure(style, name);
            self.fonts.draw(scene, style, rr.x + x - wd / 2.0, rr.y + y, name);
        }
        if let Some(l) = &o.label {
            self.draw_label(scene, rr, l, light, dim);
        }
        if let Some(t) = o.streak {
            // A fine line drawn from the upper right toward the lower left, its head leading
            // and its tail fading behind: there for a breath, then gone.
            let (from, to) = ((rr.x + rr.w * 0.84, rr.y + rr.h * 0.10), (rr.x + rr.w * 0.38, rr.y + rr.h * 0.40));
            let at = |u: f32| [from.0 + (to.0 - from.0) * u, from.1 + (to.1 - from.1) * u];
            let head = (t * t * (3.0 - 2.0 * t)).clamp(0.0, 1.0);
            let fade_out = 1.0 - smoothstep(0.7, 1.0, t);
            let steps = 14;
            for i in 0..steps {
                let (u0, u1) = (head - 0.16 * (i + 1) as f32 / steps as f32, head - 0.16 * i as f32 / steps as f32);
                if u1 <= 0.0 {
                    break;
                }
                let (a, b) = (at(u0.max(0.0)), at(u1));
                let n = ((b[1] - a[1]).hypot(b[0] - a[0])).max(0.001);
                let w = self.px(1.1) * (1.0 - i as f32 / steps as f32 * 0.7);
                let (nx, ny) = (-(b[1] - a[1]) / n * w * 0.5, (b[0] - a[0]) / n * w * 0.5);
                let alpha = (1.0 - i as f32 / steps as f32) * 0.8 * fade_out;
                scene.poly(&[[a[0] + nx, a[1] + ny], [b[0] + nx, b[1] + ny], [b[0] - nx, b[1] - ny], [a[0] - nx, a[1] - ny]], fade(light, alpha));
            }
        }
        let style = Style { font: self.f.ui, px: self.px(11.0), color: fade(dim, 0.90), tracking: 0.0 };
        let mut y = rr.bottom() - self.px(72.0) - self.px(16.0) * (o.notes.len().saturating_sub(1)) as f32;
        for note in &o.notes {
            self.fonts.draw(scene, style, rr.x + self.px(28.0), y, note);
            y += self.px(16.0);
        }
        scene.layer(clip);
    }
}

/// "3 days ahead", "2 h 10 min back".
fn relative(ms: f64) -> String {
    let span = astro::format_span(ms);
    if ms >= 0.0 {
        format!("{span} ahead")
    } else {
        format!("{span} back")
    }
}

/// The words for a catalogue star: its name, and what is known of it.
fn star_words(i: u16) -> (String, String) {
    let star = &nus_render::space::catalogue()[i as usize];
    match nus_render::space::star_name(i as usize) {
        Some(n) => {
            let mut parts: Vec<String> = Vec::new();
            if !n.designation.is_empty() {
                parts.push(n.designation.to_string());
            }
            let kind = spectral_words(n.spectral);
            if !kind.is_empty() {
                parts.push(kind);
            }
            if let Some((ly, good)) = n.distance_ly() {
                parts.push(distance_words(ly, good));
            }
            parts.push(magnitude_words(star.magnitude));
            (n.name.to_string(), parts.join(" · "))
        }
        None => ("A star".to_string(), magnitude_words(star.magnitude)),
    }
}

/// Words for a picked thing.
fn describe(target: Target, frame: &astro::SkyFrame, ms: f64, obs: &astro::Observer, at: (f32, f32), radius: f32, alpha: f32) -> Label {
    let (name, detail) = match target {
        Target::Star(i) => star_words(i),
        Target::Planet(i) => {
            let m = &frame.planets[i];
            let p = astro::planet(m.planet, ms);
            let minutes = p.dist_au * 8.317;
            let az = m.direction[0].atan2(m.direction[2]);
            let light = if minutes < 90.0 { format!("{:.0} light-minutes away", minutes) } else { format!("{:.1} light-hours away", minutes / 60.0) };
            (
                m.planet.name().to_string(),
                format!("{} · {} · {:.0}° up in the {}", magnitude_words(m.magnitude), light, m.altitude.to_degrees(), compass8(az)),
            )
        }
        Target::Sun => {
            let s = astro::sky_at(ms, obs);
            let mut detail = format!("{} million km · its light left {:.0} min ago", grouped((s.sun.dist_km / 1.0e6).round() as i64), s.sun.dist_km / 299_792.458 / 60.0);
            if frame.sun_visible < 0.999 {
                detail = format!("{:.0}% covered · {detail}", (1.0 - frame.sun_visible) * 100.0);
            }
            ("The Sun".to_string(), detail)
        }
        Target::Moon => {
            let s = astro::sky_at(ms, obs);
            (
                "The Moon".to_string(),
                format!("{} · {:.0}% lit · {} km away", astro::moon_phase_name(ms).to_lowercase(), frame.moon_illumination * 100.0, grouped(s.moon.dist_km.round() as i64)),
            )
        }
    };
    Label { at, radius, name, detail, alpha }
}

/// What a row of the almanac does when it is chosen.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Say {
    Noop,
    Copy(String),
    Jump(f64),
    Place,
}

/// The almanac's rows for `sky`, `moon`, `tonight` or `eclipse`: pure, so it can run anywhere.
pub(crate) fn build_almanac(word: &str, ms: f64, obs: Option<&astro::Observer>, tz: i32) -> Vec<(String, Say)> {
    let place_row = || ("Choose where you are · Start settings".to_string(), Say::Place);
    if word == "eclipse" {
        let Some(o) = obs else { return vec![("Eclipses need a place".to_string(), Say::Noop), place_row()] };
        let mut rows = Vec::new();
        if let Some(e) = astro::next_notable_solar_eclipse(ms, o, 3000.0) {
            let kind = match e.kind {
                astro::SolarKind::Total => "total",
                astro::SolarKind::Annular => "annular",
                astro::SolarKind::Partial => "partial",
            };
            let how = match e.central_seconds() {
                Some(s) if e.kind == astro::SolarKind::Total => format!("{} of totality", astro::format_span(s * 1000.0)),
                Some(s) => format!("{} of the ring", astro::format_span(s * 1000.0)),
                None => format!("{:.0}% covered", e.obscuration * 100.0),
            };
            rows.push((
                format!("Next solar eclipse here · {kind} · {} {} · {how}", astro::format_date_year(e.max_ms, tz), astro::format_clock(e.max_ms, tz)),
                Say::Jump(e.start_ms - 3.0 * 60_000.0),
            ));
        }
        if let Some(e) = astro::next_notable_lunar_eclipse(ms, o, 3000.0) {
            let kind = if e.kind == astro::LunarKind::Total { "total" } else { "partial" };
            let from = e.partial_start_ms.unwrap_or(e.penumbral_start_ms) - 5.0 * 60_000.0;
            rows.push((
                format!("Next lunar eclipse here · {kind} · {} {}", astro::format_date_year(e.max_ms, tz), astro::format_clock(e.max_ms, tz)),
                Say::Jump(from.max(ms)),
            ));
        }
        if rows.is_empty() {
            rows.push(("No eclipse is visible from here in the next eight years".to_string(), Say::Noop));
        }
        rows.push(("Enter shows it: the sky turns to the moment, and runs on from there".to_string(), Say::Noop));
        return rows;
    }
    let report = match word {
        "sky" => astro::sky_report(ms, obs, tz),
        "moon" => astro::moon_report(ms, obs, tz),
        _ => astro::tonight_report(ms, obs, tz),
    };
    let all = format!("{}\n{}", report.title, report.lines.join("\n"));
    let mut rows = vec![(report.title.clone(), Say::Copy(all.clone()))];
    rows.extend(report.lines.iter().map(|l| (l.clone(), Say::Copy(all.clone()))));
    if obs.is_none() {
        rows.push(place_row());
    }
    rows
}

/// The almanac's answer for one thing asked, and what is being worked out.
#[derive(Default)]
pub(crate) struct Memo {
    key: Option<MemoKey>,
    working: Option<MemoKey>,
    rows: Vec<(String, Say)>,
}

type MemoKey = (String, Option<(i32, i32)>, i64);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_chord_needs_all_ten_keys_in_order() {
        let keys = [Pressed::Up, Pressed::Up, Pressed::Down, Pressed::Down, Pressed::Left, Pressed::Right, Pressed::Left, Pressed::Right, Pressed::Letter('b'), Pressed::Letter('a')];
        let mut p = 0;
        for (i, k) in keys.iter().enumerate() {
            let (next, done) = chord_step(p, *k);
            assert_eq!(done, i == 9, "key {i}");
            p = next;
        }
        assert_eq!(p, 0);
        // A slip starts over; an up-arrow in the middle counts as a fresh first key.
        let (p, _) = chord_step(3, Pressed::Left);
        assert_eq!(p, 0);
        let (p, _) = chord_step(5, Pressed::Up);
        assert_eq!(p, 1);
        let (p, done) = chord_step(0, Pressed::Letter('a'));
        assert!(p == 0 && !done);
    }

    #[test]
    fn stars_are_described_in_plain_words() {
        assert_eq!(spectral_words("M2Ib"), "red supergiant");
        assert_eq!(spectral_words("B0.5IV"), "blue-white subgiant");
        assert_eq!(spectral_words("K2IIIp"), "orange giant");
        assert_eq!(spectral_words("A0m..."), "white star");
        assert_eq!(spectral_words("G2V"), "yellow main-sequence star");
        assert_eq!(spectral_words("F7:Ib-IIv SB"), "yellow-white supergiant");
        assert_eq!(spectral_words("DA2"), "");
        assert_eq!(distance_words(8.6, true), "8.6 light-years");
        assert_eq!(distance_words(36.7, true), "37 light-years");
        assert_eq!(distance_words(548.0, false), "~550 light-years");
        assert_eq!(distance_words(1260.0, true), "1,250 light-years");
        assert_eq!(grouped(1234567), "1,234,567");
    }

    #[test]
    fn the_camera_agrees_with_the_shader() {
        // Looking south and up 33°, the point straight ahead is the middle of the view.
        let cam = Camera { azimuth: std::f32::consts::PI, elevation: 33f32.to_radians(), fov: 56f32.to_radians() };
        let ahead = [0.0, 33f32.to_radians().sin(), -33f32.to_radians().cos()];
        let (x, y) = cam.project(ahead, 1100.0, 700.0).unwrap();
        assert!((x - 550.0).abs() < 0.5 && (y - 350.0).abs() < 0.5, "{x},{y}");
        // Higher in the sky is nearer the top of the picture; east (left when facing south) is on the left.
        let higher = [0.0, 40f32.to_radians().sin(), -40f32.to_radians().cos()];
        assert!(cam.project(higher, 1100.0, 700.0).unwrap().1 < 350.0);
        let east = [0.2, 33f32.to_radians().sin(), -33f32.to_radians().cos() * 0.97];
        assert!(cam.project(east, 1100.0, 700.0).unwrap().0 < 550.0);
        // Behind the camera is nowhere.
        assert!(cam.project([0.0, 0.5, 0.86], 1100.0, 700.0).is_none());
    }

    #[test]
    fn the_sky_clock_turns_and_drifts() {
        let mut v = SkyView::default();
        assert!(!v.travelling());
        v.turn(3_600_000.0);
        assert!(v.travelling());
        v.home();
        assert!(!v.travelling());
        let start = crate::clock::now();
        // Idle long enough and the sky runs ahead; a moment of input and it unwinds.
        let mut now = start;
        for _ in 0..800 {
            now += Duration::from_millis(250);
            v.tick(now, (0.0, 0.0), start, true, false);
        }
        assert!(v.idle_ms > 1_000_000.0, "{}", v.idle_ms);
        v.last_input = now;
        for _ in 0..40 {
            now += Duration::from_millis(100);
            v.tick(now, (0.0, 0.0), now, true, false);
        }
        assert_eq!(v.idle_ms, 0.0);
    }

    #[test]
    fn the_almanac_answers_for_every_word_and_asks_for_a_place_when_it_needs_one() {
        let o = astro::Observer::new(40.7, -74.0);
        let ms = 1_797_739_200_000.0;
        for word in ["sky", "moon", "tonight"] {
            let rows = build_almanac(word, ms, Some(&o), 0);
            assert!(rows.len() >= 3, "{word}: {rows:?}");
            assert!(rows.iter().all(|(_, s)| matches!(s, Say::Copy(_))), "{word}");
        }
        // No place: each points at the setting; the Moon's phase and its quarters need none.
        assert!(build_almanac("sky", ms, None, 0).iter().any(|(_, s)| *s == Say::Place));
        assert!(build_almanac("tonight", ms, None, 0).iter().any(|(_, s)| *s == Say::Place));
        let moon = build_almanac("moon", ms, None, 0);
        assert!(moon.iter().any(|(t, _)| t.contains("% lit")) && moon.iter().any(|(_, s)| *s == Say::Place));
        // The next eclipse from New York is a jump, with something to say about it.
        let rows = build_almanac("eclipse", ms, Some(&o), 0);
        assert!(rows.iter().any(|(t, s)| t.starts_with("Next solar eclipse") && matches!(s, Say::Jump(_))), "{rows:?}");
        assert_eq!(build_almanac("eclipse", ms, None, 0)[0].1, Say::Noop);
    }

    #[test]
    fn limiting_magnitude_follows_the_light() {
        assert!((limiting_magnitude(-30.0) - 6.6).abs() < 0.01);
        assert!(limiting_magnitude(-6.0) > 0.5 && limiting_magnitude(-6.0) < 2.5);
        assert!(limiting_magnitude(30.0) < -4.0);
    }
}
