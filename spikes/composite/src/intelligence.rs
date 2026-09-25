//! Intelligence: one level, Instant to Max, for every assistant launch. The
//! atom shows it: the nucleus is the model (its size) and the provider (its
//! metal), the orbits and electrons are the level, the dial reads it. The
//! aperture ring sets it: the scale turns under a fixed index and coasts to
//! the nearest stop. A level only becomes flags the provider's CLI accepts;
//! a model the person chose themselves is never replaced.
use crate::app::{fade, App, PaletteMode};
use nus_render::{AtomLook, AtomMode, Instance, Rect, Scene, Style};
use std::time::Instant;

pub struct Level {
    pub name: &'static str,
    /// Claude's model alias when the model is Auto.
    pub claude: &'static str,
    /// Claude's `--effort`; None leaves extended thinking off.
    pub effort: Option<&'static str>,
    /// Codex's `model_reasoning_effort`.
    pub codex: &'static str,
}

pub const LEVELS: [Level; 5] = [
    Level { name: "Instant", claude: "haiku", effort: None, codex: "minimal" },
    Level { name: "Quick", claude: "sonnet", effort: Some("low"), codex: "low" },
    Level { name: "Balanced", claude: "sonnet", effort: Some("medium"), codex: "medium" },
    Level { name: "Deep", claude: "opus", effort: Some("high"), codex: "high" },
    // Codex has no step above high that every model accepts.
    Level { name: "Max", claude: "opus", effort: Some("max"), codex: "high" },
];
pub const DEFAULT: u8 = 3;
/// Claude's choices, "" being Auto (the level picks).
pub const CLAUDE_MODELS: [&str; 4] = ["", "opus", "sonnet", "haiku"];

pub fn level(l: u8) -> &'static Level {
    &LEVELS[l.min(4) as usize]
}

/// The Claude model a launch will use.
fn claude_model(l: u8, model: &str) -> String {
    if model.trim().is_empty() { level(l).claude.into() } else { model.trim().into() }
}

/// Arguments a launch at this level adds, after the executable.
pub fn flags(provider: usize, l: u8, model: &str) -> Vec<String> {
    let lv = level(l);
    match provider {
        0 => {
            let m = claude_model(l, model);
            let mut out = vec!["--model".into(), m.clone()];
            // Haiku has no effort control; leave it to the CLI.
            if let Some(e) = lv.effort.filter(|_| !m.to_lowercase().contains("haiku")) {
                out.extend(["--effort".into(), e.into()]);
            }
            out
        }
        1 => {
            let mut out = vec![];
            if !model.trim().is_empty() {
                out.extend(["--model".into(), model.trim().into()]);
            }
            out.extend(["-c".into(), format!("model_reasoning_effort={}", lv.codex)]);
            out
        }
        _ => vec![],
    }
}

fn title(m: &str) -> String {
    let mut c = m.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

/// What a launch sends, in words.
pub fn sends(provider: usize, l: u8, model: &str) -> String {
    let lv = level(l);
    match provider {
        0 => {
            let m = claude_model(l, model);
            let who = if model.trim().is_empty() { format!("Auto picks {}", title(&m)) } else { title(&m) };
            let effort = match lv.effort {
                Some(_) if m.to_lowercase().contains("haiku") => "the CLI's default effort".to_string(),
                Some(e) => format!("{e} effort"),
                None => "no extended thinking".into(),
            };
            format!("{who} · {effort}")
        }
        1 => format!("{} · {} reasoning", if model.trim().is_empty() { "Codex's default model" } else { model.trim() }, lv.codex),
        _ => format!("{} · local models run as installed; the level doesn't apply", if model.trim().is_empty() { "No model chosen" } else { model.trim() }),
    }
}

/// The nucleus's size: how many nucleons melt together.
pub fn nucleons(provider: usize, l: u8, model: &str) -> f32 {
    match provider {
        0 => {
            let m = claude_model(l, model).to_lowercase();
            if m.contains("opus") { 9.0 } else if m.contains("haiku") { 5.0 } else { 7.0 }
        }
        1 => 7.0,
        _ => 6.0,
    }
}

/// The provider's metal: chrome for Claude, brass for Codex, gunmetal for a
/// local model. (rgb, roughness)
pub fn metal(provider: usize) -> ([f32; 3], f32) {
    match provider {
        0 => ([0.93, 0.94, 0.96], 0.0),
        1 => ([1.0, 0.80, 0.46], 0.12),
        _ => ([0.46, 0.48, 0.52], 0.45),
    }
}

/// Near a stop the control holds on a little: detents you can feel.
fn snap(v: f32) -> f32 {
    let d = v.round();
    let o = v - d;
    if o.abs() < 0.2 { d + o * 0.25 } else { v }.clamp(0.0, 4.0)
}

/// The ring's scale is printed on a barrel: radius and turn per stop.
const RING_RADIUS: f32 = 0.44;
const RING_TURN: f32 = 0.46;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Part {
    /// The aperture ring; `per` is pixels per stop at the index.
    Ring { per: f32, mid: f32 },
    /// The instrument: the dial around, the nucleus inside.
    Atom { cx: f32, cy: f32, m: f32 },
    /// A Claude model chip.
    Model(usize),
}

#[derive(Clone, Copy)]
struct Drag {
    part: Part,
    x: f32,
    y: f32,
    from: f32,
    last: (f32, Instant),
    vx: f32,
    moved: bool,
    core: bool,
}

#[derive(Default)]
pub struct Dial {
    /// The value under the pointer while dragging.
    value: Option<f32>,
    x: f32,
    lvl: f32,
    nuc: f32,
    metal: [f32; 3],
    rough: f32,
    ready: bool,
    last: Option<Instant>,
    start: Option<Instant>,
    shown: Option<Instant>,
    pub hits: Vec<(Rect, Part)>,
    drag: Option<Drag>,
}

impl Dial {
    /// Drawn in the last frame: keep frames coming while it turns.
    pub fn animating(&self, reduced: bool) -> bool {
        self.drag.is_some()
            || self.shown.is_some_and(|at| crate::clock::since(at).as_secs_f32() < 0.1)
                && (!reduced || (self.x - self.target_or(self.lvl)).abs() > 0.002)
    }
    fn target_or(&self, v: f32) -> f32 {
        self.value.unwrap_or(v)
    }
}

impl App {
    pub(crate) fn intelligence(&self) -> u8 {
        self.behavior.assistants.intelligence.min(4)
    }

    /// The provider the atom is about: the one being launched, else the default.
    pub(crate) fn intel_provider(&self) -> usize {
        match self.palette.as_ref().map(|(m, _)| *m) {
            Some(PaletteMode::Assistant(id)) => id.min(2) as usize,
            _ => self.default_assistant().min(2) as usize,
        }
    }

    fn intel_value(&self) -> f32 {
        self.intel.value.unwrap_or(self.intelligence() as f32)
    }

    pub(crate) fn set_intelligence(&mut self, l: u8) {
        let l = l.min(4);
        if l != self.behavior.assistants.intelligence {
            self.behavior.assistants.intelligence = l;
            self.play_event("toggle");
            self.save_prefs();
        }
        self.dirty = true;
    }

    fn intel_model(&self, provider: usize) -> String {
        self.behavior.assistants.providers[provider].model.clone()
    }

    pub(crate) fn set_claude_model(&mut self, i: usize) {
        self.behavior.assistants.providers[0].model = CLAUDE_MODELS[i % CLAUDE_MODELS.len()].into();
        self.play_event("toggle");
        self.save_prefs();
        self.dirty = true;
    }

    /// Ease toward the settings. One easing, no overshoot: it settles.
    fn intel_tick(&mut self) -> AtomLook {
        let now = crate::clock::now();
        let reduced = self.motion.reduced();
        let dt = self.intel.last.map_or(0.0, |l| (now - l).as_secs_f32()).min(0.05);
        self.intel.last = Some(now);
        let start = *self.intel.start.get_or_insert(now);
        let provider = self.intel_provider();
        let model = self.intel_model(provider);
        let target = self.intel_value();
        let nuc = nucleons(provider, target.round() as u8, &model);
        let (tint, rough) = metal(provider);
        let d = &mut self.intel;
        if !d.ready || reduced {
            d.x = target;
            d.lvl = target;
            d.nuc = nuc;
            d.metal = tint;
            d.rough = rough;
            d.ready = true;
        }
        let kx = 1.0 - (-dt * if d.drag.is_some() { 16.0 } else { 9.0 }).exp();
        let k = 1.0 - (-dt * 6.0).exp();
        d.x += (target - d.x) * kx;
        d.lvl += (target - d.lvl) * k;
        d.nuc += (nuc - d.nuc) * k;
        for i in 0..3 {
            d.metal[i] += (tint[i] - d.metal[i]) * k;
        }
        d.rough += (rough - d.rough) * k;
        d.shown = Some(now);
        AtomLook {
            mode: AtomMode::Instrument,
            seconds: if reduced { 12.0 } else { (now - start).as_secs_f32() % 3600.0 },
            level: d.lvl,
            value: d.x,
            nucleons: d.nuc,
            metal: [d.metal[0], d.metal[1], d.metal[2], 1.0],
            roughness: d.rough,
            ink: self.theme.ink,
            signal: self.surface.signal,
            scale: self.px(1.0),
        }
    }

    /// The instrument, square in `r`. A click on the nucleus picks the next
    /// Claude model; a drag around the dial sets the level.
    pub(crate) fn draw_intel_atom(&mut self, scene: &mut Scene, r: Rect) {
        let look = self.intel_tick();
        let m = r.w.min(r.h);
        let sq = Rect::new(r.x + (r.w - m) / 2.0, r.y + (r.h - m) / 2.0, m, m);
        scene.push(Instance::atom(sq, look));
        let clip = scene.clip().map_or(sq, |c| c.intersect(&sq));
        self.intel.hits.push((clip, Part::Atom { cx: sq.x + m / 2.0, cy: sq.y + m / 2.0, m }));
    }

    /// The aperture ring across `r` (about 72px tall).
    pub(crate) fn draw_intel_ring(&mut self, scene: &mut Scene, r: Rect) {
        let look = self.intel_tick();
        let v = look.value;
        let heat = (v - 3.0).clamp(0.0, 1.0);
        let t = self.theme.clone();
        let cx = r.x + r.w / 2.0;
        let rc = r.w * RING_RADIUS;
        let base = r.y + self.px(22.0);
        let label = Style { px: self.px(11.0), tracking: self.px(0.9), ..self.label() };
        let hot = if heat > 0.5 { self.surface.signal } else { t.ink };
        for i in 0..=32 {
            let u = i as f32 / 8.0;
            let th = (u - v) * RING_TURN;
            if th.abs() > 1.5 {
                continue;
            }
            let x = cx + rc * th.sin();
            let c = th.cos();
            let major = i % 8 == 0;
            let near = (1.0 - (u - v).abs() * 1.6).max(0.0);
            let a = c.powf(2.2) * if major { 0.9 } else { 0.34 + 0.4 * near };
            let w = self.px(if major { 1.2 } else { 0.8 }) * (0.5 + 0.5 * c);
            let h = self.px(if major { 14.0 } else if i % 4 == 0 { 9.0 } else { 6.0 });
            let col = if major && i == 32 && heat > 0.5 { self.surface.signal } else { t.ink };
            scene.rect(Rect::new(x - w / 2.0, base, w, h), fade(col, a));
            if major {
                let k = i / 8;
                let on = (k as f32 - v).abs() < 0.5;
                let name = LEVELS[k].name.to_uppercase();
                let nw = self.fonts.measure(label, &name);
                let color = if on && heat > 0.5 { self.surface.signal } else { t.ink };
                // Names fade out before the barrel foreshortens them into each other.
                let edge = ((c - 0.62) / 0.2).clamp(0.0, 1.0);
                let st = Style { color: fade(color, edge * if on { 1.0 } else { 0.42 }), ..label };
                self.fonts.draw(scene, st, x - nw / 2.0, base + h + self.px(18.0), &name);
            }
        }
        // The barrel's edges, and the index, fixed.
        for y in [base - self.px(6.0), r.bottom() - self.px(6.0)] {
            scene.rect(Rect::new(cx - rc, y, 2.0 * rc, self.px(1.0)), fade(t.ink, 0.16));
        }
        scene.rect(Rect::new(cx - self.px(0.75), base - self.px(6.0), self.px(1.5), self.px(28.0)), hot);
        let b = self.px(14.0);
        scene.push(Instance::atom(
            Rect::new(cx - b / 2.0, base - self.px(6.0) - b + self.px(2.0), b, b),
            AtomLook { mode: AtomMode::Bead, metal: [0.93, 0.94, 0.96, 1.0], roughness: 0.0, ..look },
        ));
        let clip = scene.clip().map_or(r, |c| c.intersect(&r));
        self.intel.hits.push((clip, Part::Ring { per: rc * RING_TURN, mid: cx }));
    }

    /// Claude's models as chips, from `x` on the baseline `y`; returns the width.
    pub(crate) fn draw_intel_models(&mut self, scene: &mut Scene, x: f32, y: f32) -> f32 {
        let t = self.theme.clone();
        let label = self.label();
        let current = self.intel_model(0);
        let mut cx = x;
        for (i, m) in CLAUDE_MODELS.iter().enumerate() {
            let name = if m.is_empty() { "AUTO".to_string() } else { m.to_uppercase() };
            let w = self.fonts.measure(label, &name) + self.px(20.0);
            let chip = Rect::new(cx, y - self.px(16.0), w, self.px(23.0));
            let on = current.eq_ignore_ascii_case(m);
            if on {
                scene.rect(chip, t.ink);
            } else {
                scene.outline(chip, self.px(1.0), t.ink);
            }
            self.fonts.draw(scene, Style { color: if on { t.paper } else { t.ink }, ..label }, cx + self.px(10.0), y, &name);
            self.intel.hits.push((chip, Part::Model(i)));
            cx += w + self.px(8.0);
        }
        cx - x
    }

    /// The launch review's band: the instrument on the left; the ring, what
    /// it sends and (for Claude) the model on the right.
    pub(crate) fn draw_intel_band(&mut self, scene: &mut Scene, r: Rect) {
        let t = self.theme.clone();
        let atom = Rect::new(r.x + self.px(8.0), r.y + self.px(6.0), r.h - self.px(12.0), r.h - self.px(12.0));
        self.draw_intel_atom(scene, atom);
        let x0 = atom.right() + self.px(8.0);
        let w = r.right() - self.px(18.0) - x0;
        let cap = Style { color: t.dim, ..self.label() };
        self.fonts.draw(scene, cap, x0, r.y + self.px(26.0), "INTELLIGENCE · ⌥← ⌥→");
        let v = self.intel_value().round() as u8;
        let word = level(v).name.to_uppercase();
        let st = Style { color: if v == 4 { self.surface.signal } else { t.ink }, ..self.label() };
        let ww = self.fonts.measure(st, &word);
        self.fonts.draw(scene, st, x0 + w - ww, r.y + self.px(26.0), &word);
        self.draw_intel_ring(scene, Rect::new(x0, r.y + self.px(38.0), w, self.px(72.0)));
        let dim = Style { color: t.dim, ..self.ui() };
        let line = self.fit(dim, &self.intel_sends(), w);
        self.fonts.draw(scene, dim, x0, r.y + self.px(134.0), &line);
        if self.intel_provider() == 0 {
            self.draw_intel_models(scene, x0, r.y + self.px(176.0));
        } else {
            let p = self.intel_provider();
            let model = self.intel_model(p);
            let text = format!("MODEL · {} · CHANGE IN SETTINGS", if model.is_empty() { "DEFAULT".into() } else { model.to_uppercase() });
            let text = self.fit(cap, &text, w);
            self.fonts.draw(scene, cap, x0, r.y + self.px(176.0), &text);
        }
        scene.hline(r.x, r.bottom() - self.px(1.0), r.w, self.px(1.0), t.tint);
    }

    /// The sentence under the ring: what a launch sends.
    pub(crate) fn intel_sends(&self) -> String {
        let p = self.intel_provider();
        sends(p, self.intel_value().round() as u8, &self.intel_model(p))
    }

    pub(crate) fn intel_mouse(&mut self, pressed: bool, x: f32, y: f32) -> bool {
        let now = crate::clock::now();
        if !pressed {
            let Some(d) = self.intel.drag.take() else { return false };
            match d.part {
                Part::Ring { per, mid } => {
                    if !d.moved {
                        // A tap on either side moves one stop.
                        let step = if d.x < mid { -1.0 } else { 1.0 };
                        let l = (self.intelligence() as f32 + step).clamp(0.0, 4.0);
                        self.intel.value = None;
                        self.set_intelligence(l as u8);
                    } else {
                        // Let go and it coasts, at most one stop further.
                        let v = self.intel_value() - (d.vx * 0.18 / per).clamp(-1.0, 1.0);
                        self.intel.value = None;
                        self.set_intelligence(v.round().clamp(0.0, 4.0) as u8);
                    }
                }
                Part::Atom { .. } => {
                    let v = self.intel_value();
                    self.intel.value = None;
                    if d.moved {
                        self.set_intelligence(v.round().clamp(0.0, 4.0) as u8);
                    } else if d.core && self.intel_provider() == 0 {
                        let i = CLAUDE_MODELS.iter().position(|m| m.eq_ignore_ascii_case(&self.intel_model(0))).unwrap_or(0);
                        self.set_claude_model(i + 1);
                    }
                }
                Part::Model(_) => {}
            }
            self.dirty = true;
            return true;
        }
        let Some(&(_, part)) = self.intel.hits.iter().rev().find(|(r, _)| r.contains(x, y)) else { return false };
        let mut d = Drag { part, x, y, from: self.intel_value(), last: (x, now), vx: 0.0, moved: false, core: false };
        match part {
            Part::Model(i) => {
                self.set_claude_model(i);
                return true;
            }
            Part::Atom { cx, cy, m } => {
                d.core = (x - cx).hypot(y - cy) < m * 0.12;
                if !d.core {
                    d.moved = true;
                    self.intel.value = Some(snap(dial_value(x - cx, y - cy)));
                }
            }
            Part::Ring { .. } => {}
        }
        self.intel.drag = Some(d);
        self.dirty = true;
        true
    }

    pub(crate) fn intel_move(&mut self, x: f32, y: f32) -> bool {
        let Some(mut d) = self.intel.drag else { return false };
        let now = crate::clock::now();
        match d.part {
            Part::Ring { per, .. } => {
                let dt = (now - d.last.1).as_secs_f32().max(0.004);
                d.vx = (x - d.last.0) / dt;
                d.last = (x, now);
                if (x - d.x).abs() > self.px(3.0) {
                    d.moved = true;
                }
                if d.moved {
                    self.intel.value = Some(snap(d.from - (x - d.x) / per));
                }
            }
            Part::Atom { cx, cy, .. } => {
                if !d.moved && (x - d.x).hypot(y - d.y) > self.px(4.0) {
                    d.moved = true;
                }
                if d.moved {
                    self.intel.value = Some(snap(dial_value(x - cx, y - cy)));
                }
            }
            Part::Model(_) => {}
        }
        self.intel.drag = Some(d);
        self.dirty = true;
        true
    }
}

/// The dial runs 270° clockwise from lower left; the gap at the bottom
/// holds whichever end is nearer.
fn dial_value(dx: f32, dy: f32) -> f32 {
    let deg = (-dy).atan2(dx).to_degrees();
    let mut d = ((225.0 - deg) % 360.0 + 360.0) % 360.0;
    if d > 270.0 {
        d = if d > 315.0 { 0.0 } else { 270.0 };
    }
    d / 270.0 * 4.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_claude_follows_the_level() {
        assert_eq!(flags(0, 0, ""), ["--model", "haiku"]);
        assert_eq!(flags(0, 2, ""), ["--model", "sonnet", "--effort", "medium"]);
        assert_eq!(flags(0, 4, ""), ["--model", "opus", "--effort", "max"]);
    }

    #[test]
    fn a_chosen_model_is_kept() {
        assert_eq!(flags(0, 3, "sonnet"), ["--model", "sonnet", "--effort", "high"]);
        assert_eq!(flags(0, 4, "claude-haiku-4-5"), ["--model", "claude-haiku-4-5"]);
        assert_eq!(flags(1, 1, "gpt-5"), ["--model", "gpt-5", "-c", "model_reasoning_effort=low"]);
    }

    #[test]
    fn codex_and_local_models() {
        assert_eq!(flags(1, 0, ""), ["-c", "model_reasoning_effort=minimal"]);
        assert!(flags(2, 4, "llama3").is_empty());
    }

    #[test]
    fn the_dial_maps_angles_to_levels() {
        assert!((dial_value(-1.0, 1.0) - 0.0).abs() < 1e-3); // lower left
        assert!((dial_value(0.0, -1.0) - 2.0).abs() < 1e-3); // top
        assert!((dial_value(1.0, 1.0) - 4.0).abs() < 1e-3); // lower right
        assert!(snap(2.1) > 2.0 && snap(2.1) < 2.05);
    }
}
