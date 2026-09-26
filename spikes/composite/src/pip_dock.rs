//! Picture in picture, pinned to a shell: the same cropped video PiP
//! draws in its own window, drawn instead over a corner of a terminal
//! pane, so a tutorial or the game plays beside the work.
//!
//! It floats over old scrollback and never takes the keyboard: the shell
//! keeps every key. When the cursor's row comes under it (new output, a
//! prompt, a full-screen program moving about), it slides to the other
//! corner. The terminal never resizes for it, so nothing reflows.
//!
//! Its place is kept relative to the pane (a corner and a width), so it
//! moves with splits and tiles. While its shell isn't on screen (another
//! tab, the window minimized) the video goes to the floating PiP window
//! and comes back when the shell does.
//!
//! In: the pin mark in PiP's controls, dragging the PiP window onto a
//! shell, or the palette. Transport while you type: ⌘⌥K play or pause,
//! ⌘⌥J / ⌘⌥L back and on, ⌘⌥M mute, ⌘⌥- / ⌘⌥= size, ⌘⌥P float it
//! again (Ctrl+Alt elsewhere).

use nus_render::text::icons;
use nus_render::theme::metric as m;
use nus_render::{Rect, Scene};
use winit::keyboard::{KeyCode, PhysicalKey};

use crate::anim::Anim;
use crate::app::{App, Pane};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Corner {
    TopRight,
    BottomRight,
    TopLeft,
    BottomLeft,
}

impl Corner {
    fn top(self) -> bool {
        matches!(self, Corner::TopRight | Corner::TopLeft)
    }
    fn right(self) -> bool {
        matches!(self, Corner::TopRight | Corner::BottomRight)
    }
    fn of(top: bool, right: bool) -> Corner {
        match (top, right) {
            (true, true) => Corner::TopRight,
            (false, true) => Corner::BottomRight,
            (true, false) => Corner::TopLeft,
            (false, false) => Corner::BottomLeft,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Play,
    Back,
    Forward,
    Mute,
    Smaller,
    Larger,
    /// Out of the shell, into the floating window.
    Float,
    /// To the video's own tab.
    ToTab,
    Close,
}

pub struct Docked {
    /// The page the video plays in.
    pub src_tab: u64,
    pub src_right: bool,
    /// The shell it's pinned to.
    pub host_tab: u64,
    pub host_right: bool,
    /// Where you put it; it leaves only to dodge the cursor.
    pub corner: Corner,
    /// Width as a share of the pane's.
    pub width: f32,
    pub x: Anim,
    pub y: Anim,
    /// Drawn last frame: where it is, and its controls.
    pub rect: Option<Rect>,
    pub hits: Vec<(Rect, Hit)>,
    /// A press on the picture, and where in it: a drag moves it.
    pub drag: Option<(f32, f32)>,
    /// Handed to the floating window while its shell is out of sight.
    pub parked: bool,
}

/// How much of the pane it may take, and the step ⌘⌥-/= takes.
const MIN_W: f32 = 0.16;
const MAX_W: f32 = 0.6;
const STEP: f32 = 1.15;

impl App {
    fn term_pane(&self, tab: u64, right: bool) -> Option<&crate::app::TermPane> {
        let t = self.tabs.iter().find(|t| t.id == tab)?;
        match if right { t.right.as_ref()? } else { &t.left } {
            Pane::Term(p) => Some(p),
            _ => None,
        }
    }

    fn dock_source(&self) -> Option<&crate::browser::BrowserTab> {
        let d = self.docked.as_ref()?;
        let t = self.tabs.iter().find(|t| t.id == d.src_tab)?;
        match if d.src_right { t.right.as_ref()? } else { &t.left } {
            Pane::Web(w) => Some(&w.tab),
            _ => None,
        }
    }

    /// The shell a video would pin to: the active tab's focused one, else
    /// any shell in it.
    fn dock_host(&self) -> Option<(u64, bool)> {
        let t = self.tabs.get(self.active)?;
        let focused = t.focus_right && t.right.is_some();
        [focused, !focused].into_iter().find_map(|right| {
            let p = if right { t.right.as_ref()? } else { &t.left };
            matches!(p, Pane::Term(_)).then_some((t.id, right))
        })
    }

    /// Pin the video of (`tab`, `right`) to `host`.
    pub(crate) fn dock_video(&mut self, src: (u64, bool), host: (u64, bool)) {
        let corner = self.docked.as_ref().map(|d| d.corner).unwrap_or(Corner::TopRight);
        let width = self.docked.as_ref().map(|d| d.width).unwrap_or(0.34);
        self.pip = None;
        self.pip_request = None;
        self.docked = Some(Docked { src_tab: src.0, src_right: src.1, host_tab: host.0, host_right: host.1, corner, width, x: Anim::at(f32::NAN), y: Anim::at(f32::NAN), rect: None, hits: Vec::new(), drag: None, parked: false });
        if let Some(t) = self.dock_source() {
            t.prepare_pip();
        }
        self.dirty = true;
    }

    /// From the floating window into this tab's shell.
    pub(crate) fn dock_from_pip(&mut self) -> bool {
        let Some((tab, right)) = self.pip.as_ref().map(|p| (p.tab_id, p.right)) else { return false };
        let Some(host) = self.dock_host() else {
            self.notice(icons::TERMINAL, "No Shell Here", "open a shell in this tab, then pin the video to it");
            return false;
        };
        self.dock_video((tab, right), host);
        true
    }

    /// The palette's "pin video to this shell": the floating video, else
    /// the first page playing one.
    pub(crate) fn dock_any(&mut self) {
        if self.pip.is_some() {
            self.dock_from_pip();
            return;
        }
        let Some(host) = self.dock_host() else {
            self.notice(icons::TERMINAL, "No Shell Here", "open a shell in this tab, then pin the video to it");
            return;
        };
        let playing = (0..self.tabs.len()).find_map(|i| self.playing_video(i).map(|right| (self.tabs[i].id, right)));
        match playing {
            Some(src) => self.dock_video(src, host),
            None => self.notice(icons::PIP, "No Video Playing", "start a video in a tab, then pin it"),
        }
    }

    /// Out of the shell: into the floating window, or (`to_pip` false) gone.
    pub(crate) fn undock(&mut self, to_pip: bool) {
        let Some(d) = self.docked.take() else { return };
        if to_pip {
            if let Some(i) = self.tabs.iter().position(|t| t.id == d.src_tab) {
                self.request_pip(i, d.src_right);
            }
        }
        self.dirty = true;
    }

    /// Is this pane on screen right now?
    fn pane_shown(&self, tab: u64, right: bool) -> bool {
        let Some(i) = self.tabs.iter().position(|t| t.id == tab) else { return false };
        if i != self.active || self.hatch_state.main_hidden || self.window.is_minimized().unwrap_or(false) {
            return false;
        }
        let t = &self.tabs[i];
        let has_right = t.right.is_some();
        let narrow = self.width_class() == crate::app::Width::Narrow || t.solo;
        if right { has_right && !(narrow && !t.focus_right) } else { !(narrow && has_right && t.focus_right) }
    }

    /// Per frame: follow the tabs, and hand the video to the floating
    /// window while its shell is out of sight.
    pub(crate) fn tend_docked(&mut self) {
        let Some(d) = self.docked.as_ref() else { return };
        let (src, host, parked) = ((d.src_tab, d.src_right), (d.host_tab, d.host_right), d.parked);
        if self.dock_source().is_none() {
            self.docked = None;
            return;
        }
        if self.term_pane(host.0, host.1).is_none() {
            // The shell closed (or became something else): float it.
            return self.undock(true);
        }
        let shown = self.pane_shown(host.0, host.1);
        if !shown && !parked {
            if let Some(i) = self.tabs.iter().position(|t| t.id == src.0) {
                if self.pip.is_none() {
                    self.request_pip(i, src.1);
                }
            }
            if let Some(d) = self.docked.as_mut() {
                d.parked = true;
            }
        } else if shown && parked {
            // Back from the floating window, unless you moved it to
            // another video meanwhile.
            if self.pip.as_ref().is_none_or(|p| p.tab_id == src.0) {
                self.pip = None;
                self.pip_request = None;
            }
            if let Some(d) = self.docked.as_mut() {
                d.parked = false;
                d.x = Anim::at(f32::NAN);
                d.y = Anim::at(f32::NAN);
            }
            self.dirty = true;
        }
    }

    /// The pinned video, over its shell. After the panes, so over them.
    pub(crate) fn draw_docked(&mut self, scene: &mut Scene) {
        let Some(d) = self.docked.as_ref() else { return };
        if d.parked {
            return;
        }
        let (host, width, corner, dragging) = ((d.host_tab, d.host_right), d.width, d.corner, d.drag.is_some());
        let Some(t) = self.term_pane(host.0, host.1) else { return };
        let (cw, ch) = t.grid.cell_size();
        let area = Rect::new(t.rect.x, t.origin.1, t.rect.w, (t.rect.bottom() - t.origin.1).max(1.0));
        let cursor = t.term.cursor();
        let cursor_row = Rect::new(area.x, t.origin.1 + cursor.row as f32 * ch - ch, area.w, ch * 3.0);
        let _ = cw;
        let Some(src) = self.dock_source() else { return };
        let s = src.shared.borrow();
        let (bind, video) = (s.bind.clone(), s.video.clone());
        drop(s);
        let aspect = video.as_ref().and_then(|v| (v.video_width > 0.0 && v.video_height > 0.0).then(|| (v.video_width / v.video_height) as f32)).unwrap_or(16.0 / 9.0);
        let margin = self.px(12.0);
        let w = (area.w * width).clamp(self.px(160.0), (area.w - margin * 2.0).max(self.px(80.0)));
        let h = (w / aspect).min(area.h - margin * 2.0);
        let w = h * aspect;
        let at = |c: Corner| {
            let x = if c.right() { area.right() - margin - w } else { area.x + margin };
            let y = if c.top() { area.y + margin } else { area.bottom() - margin - h };
            Rect::new(x, y, w, h)
        };
        // Where you put it, unless the cursor's rows are under it.
        let mut target = at(corner);
        if target.intersect(&cursor_row).h > 0.0 {
            let flipped = at(Corner::of(!corner.top(), corner.right()));
            if flipped.intersect(&cursor_row).h <= 0.0 {
                target = flipped;
            }
        }
        let dur = if self.motion.reduced() { 0.0 } else { 0.24 };
        let (mx, my) = self.mouse;
        let d = self.docked.as_mut().unwrap();
        if dragging {
            let (ox, oy) = d.drag.unwrap();
            d.x = Anim::at((mx - ox).clamp(area.x, area.right() - w));
            d.y = Anim::at((my - oy).clamp(area.y, area.bottom() - h));
        } else if d.x.value().is_nan() {
            d.x = Anim::at(target.x);
            d.y = Anim::at(target.y);
        } else {
            d.x.go(target.x, dur);
            d.y.go(target.y, dur);
        }
        let r = Rect::new(d.x.value(), d.y.value(), w, h);
        d.rect = Some(r);
        let moving = d.x.active() || d.y.active();
        let ink = self.theme.ink;
        scene.layer(Some(area));
        // A floating thing: the hard shadow, a black ground, the picture,
        // the band, a 2px edge.
        let sh = self.px(6.0);
        scene.rect(Rect::new(r.x + sh, r.y + sh, r.w, r.h), ink);
        scene.rect(r, [0.0, 0.0, 0.0, 1.0]);
        if let (Some(bind), Some(v)) = (bind, video.clone()) {
            if v.w > 0.0 && v.h > 0.0 && v.vw > 0.0 && v.vh > 0.0 {
                let x = v.x.max(0.0);
                let y = v.y.max(0.0);
                let cw = (v.x + v.w).min(v.vw) - x;
                let chh = (v.y + v.h).min(v.vh) - y;
                if cw > 0.0 && chh > 0.0 {
                    let [dx, dy, dw, dh] = v.picture;
                    let dest = Rect::new(r.x + (dx + (x - v.x) / v.w * dw) * r.w, r.y + (dy + (y - v.y) / v.h * dh) * r.h, cw / v.w * dw * r.w, chh / v.h * dh * r.h);
                    scene.texture_uv(dest, [x / v.vw, y / v.vh, (x + cw) / v.vw, (y + chh) / v.vh], bind, Some(r.intersect(&area)));
                }
            }
        }
        scene.layer(Some(area));
        if self.behavior.pip_band {
            scene.rect(Rect::new(r.x, r.y, r.w, self.px(3.0)), self.surface.signal);
        }
        if self.behavior.pip_progress {
            if let Some(v) = video.as_ref().filter(|v| v.dur > 0.0) {
                let p = (v.t / v.dur).clamp(0.0, 1.0) as f32;
                scene.rect(Rect::new(r.x, r.bottom() - self.px(2.0), r.w * p, self.px(2.0)), self.surface.signal);
            }
        }
        scene.push(nus_render::Instance::stroke(r, 0.0, self.px(m::FLOATING), ink, None, 0.0));
        // The controls: up while the pointer is on it, or it's paused.
        let paused = video.as_ref().is_none_or(|v| v.paused);
        let mut hits = Vec::new();
        if r.contains(mx, my) || (paused && !dragging) {
            hits = self.draw_dock_controls(scene, r, video.as_ref());
        }
        scene.layer(None);
        let d = self.docked.as_mut().unwrap();
        d.hits = hits;
        if moving {
            self.dirty = true;
        }
    }

    fn draw_dock_controls(&mut self, scene: &mut Scene, r: Rect, v: Option<&crate::browser::Video>) -> Vec<(Rect, Hit)> {
        let white: nus_render::Color = [1.0, 1.0, 1.0, 1.0];
        let (mx, my) = self.mouse;
        scene.rect(r, [0.0, 0.0, 0.0, 0.45]);
        let mut hits = Vec::new();
        let small = r.w < self.px(260.0);
        let mut mark = |app: &mut App, scene: &mut Scene, icon: (&'static str, &'static str), size: f32, cx: f32, cy: f32, hit: Hit| {
            let b = Rect::new(cx - size / 2.0, cy - size / 2.0, size, size);
            let reach = crate::touch::grown(b, app.px(8.0));
            let hot = reach.contains(mx, my);
            if hot {
                scene.rect(reach, crate::app::fade(white, 0.16));
            }
            app.fonts.draw_icon(scene, icon, size, b.x, b.y, crate::app::fade(white, if hot { 1.0 } else { 0.86 }));
            hits.push((reach, hit));
        };
        let isz = self.px(13.0);
        let top = r.y + self.px(16.0);
        // Top: size on the left; float, to tab, close on the right.
        let mut x = r.x + self.px(14.0) + isz / 2.0;
        mark(self, scene, icons::MINUS, isz, x, top, Hit::Smaller);
        x += isz + self.px(14.0);
        mark(self, scene, icons::PLUS, isz, x, top, Hit::Larger);
        let mut x = r.right() - self.px(14.0) - isz / 2.0;
        mark(self, scene, icons::CLOSE, isz, x, top, Hit::Close);
        x -= isz + self.px(14.0);
        mark(self, scene, icons::TO_TAB, isz, x, top, Hit::ToTab);
        x -= isz + self.px(14.0);
        mark(self, scene, icons::PIP, isz, x, top, Hit::Float);
        // Middle: back, play or pause, on.
        let cy = r.y + r.h / 2.0;
        let cx = r.x + r.w / 2.0;
        let big = self.px(if small { 22.0 } else { 28.0 });
        let side = self.px(if small { 15.0 } else { 18.0 });
        let gap = self.px(if small { 30.0 } else { 40.0 });
        let paused = v.is_none_or(|v| v.paused);
        mark(self, scene, if paused { icons::PLAY_FILL } else { icons::PAUSE_FILL }, big, cx, cy, Hit::Play);
        mark(self, scene, icons::BACK_10, side, cx - gap, cy, Hit::Back);
        mark(self, scene, icons::FORWARD_10, side, cx + gap, cy, Hit::Forward);
        // Foot: the speaker.
        let muted = v.is_some_and(|v| v.muted);
        mark(self, scene, if muted { icons::SPEAKER_OFF } else { icons::SPEAKER }, isz, r.right() - self.px(14.0) - isz / 2.0, r.bottom() - self.px(16.0), Hit::Mute);
        hits
    }

    fn dock_act(&mut self, hit: Hit) {
        let skip = self.behavior.pip_skip_seconds.clamp(1, 120);
        let cmd = match hit {
            Hit::Play => "__nus.toggle()".to_string(),
            Hit::Back => format!("__nus.seek(-{skip})"),
            Hit::Forward => format!("__nus.seek({skip})"),
            Hit::Mute => "__nus.mute()".to_string(),
            Hit::Smaller | Hit::Larger => {
                if let Some(d) = self.docked.as_mut() {
                    d.width = (d.width * if hit == Hit::Larger { STEP } else { 1.0 / STEP }).clamp(MIN_W, MAX_W);
                }
                self.dirty = true;
                return;
            }
            Hit::Float => return self.undock(true),
            Hit::Close => return self.undock(false),
            Hit::ToTab => {
                let src = self.docked.as_ref().map(|d| d.src_tab);
                self.undock(false);
                if let Some(i) = src.and_then(|id| self.tabs.iter().position(|t| t.id == id)) {
                    self.activate(i);
                }
                return;
            }
        };
        if let Some(t) = self.dock_source() {
            t.eval(&cmd);
        }
        self.dirty = true;
    }

    /// A press over the pinned video: a control, or the start of a drag.
    /// The shell beneath never sees it.
    pub(crate) fn dock_press(&mut self, x: f32, y: f32) -> bool {
        let Some(d) = self.docked.as_mut().filter(|d| !d.parked) else { return false };
        let Some(r) = d.rect.filter(|r| r.contains(x, y)) else { return false };
        if let Some(hit) = d.hits.iter().find(|(h, _)| h.contains(x, y)).map(|(_, h)| *h) {
            self.dock_act(hit);
            return true;
        }
        d.drag = Some((x - r.x, y - r.y));
        self.dirty = true;
        true
    }

    /// Let go: it settles in the nearest corner, which becomes its own.
    pub(crate) fn dock_release(&mut self) {
        let Some(d) = self.docked.as_mut() else { return };
        if d.drag.take().is_none() {
            return;
        }
        let host = (d.host_tab, d.host_right);
        let Some(r) = d.rect else { return };
        let Some(t) = self.term_pane(host.0, host.1) else { return };
        let mid = (t.rect.x + t.rect.w / 2.0, t.origin.1 + (t.rect.bottom() - t.origin.1) / 2.0);
        let corner = Corner::of(r.y + r.h / 2.0 < mid.1, r.x + r.w / 2.0 > mid.0);
        if let Some(d) = self.docked.as_mut() {
            d.corner = corner;
        }
        self.dirty = true;
    }

    pub(crate) fn dock_moved(&mut self) {
        if self.docked.as_ref().is_some_and(|d| d.drag.is_some() || d.rect.is_some()) {
            self.dirty = true;
        }
    }

    /// ⌘⌥ (Ctrl+Alt elsewhere) with K J L M - = P: the pinned video, while
    /// the shell keeps the plain keys.
    pub(crate) fn dock_key(&mut self, ev: &crate::app::KeyIn) -> bool {
        if ev.state != winit::event::ElementState::Pressed || self.docked.is_none() {
            return false;
        }
        let chord = self.mods.alt_key() && !self.mods.shift_key() && if cfg!(target_os = "macos") { self.mods.super_key() } else { self.mods.control_key() };
        if !chord {
            return false;
        }
        let hit = match ev.physical_key {
            PhysicalKey::Code(KeyCode::KeyK) => Hit::Play,
            PhysicalKey::Code(KeyCode::KeyJ) => Hit::Back,
            PhysicalKey::Code(KeyCode::KeyL) => Hit::Forward,
            PhysicalKey::Code(KeyCode::KeyM) => Hit::Mute,
            PhysicalKey::Code(KeyCode::Minus) => Hit::Smaller,
            PhysicalKey::Code(KeyCode::Equal) => Hit::Larger,
            PhysicalKey::Code(KeyCode::KeyP) => Hit::Float,
            _ => return false,
        };
        self.dock_act(hit);
        true
    }

    /// The floating window let go over one of this window's shells.
    pub(crate) fn pip_dropped(&mut self) -> bool {
        let Some(p) = self.pip.as_ref() else { return false };
        let (cx, cy) = (p.cur.x + p.cur.w / 2.0, p.cur.y + p.cur.h / 2.0);
        let scale = self.window.scale_factor();
        let Ok(origin) = self.window.inner_position() else { return false };
        // Into this window's own pixels, which the panes are laid out in.
        let (x, y) = ((cx * scale - origin.x as f64) as f32, (cy * scale - origin.y as f64) as f32);
        let Some(t) = self.tabs.get(self.active) else { return false };
        let id = t.id;
        let over = [false, true].into_iter().find(|&right| {
            let pane = if right { t.right.as_ref() } else { Some(&t.left) };
            matches!(pane, Some(Pane::Term(tp)) if tp.rect.contains(x, y)) && self.pane_shown(id, right)
        });
        let Some(right) = over else { return false };
        let src = (p.tab_id, p.right);
        self.dock_video(src, (id, right));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corners_flip_and_round_trip() {
        for c in [Corner::TopRight, Corner::BottomRight, Corner::TopLeft, Corner::BottomLeft] {
            assert_eq!(Corner::of(c.top(), c.right()), c);
            let flipped = Corner::of(!c.top(), c.right());
            assert_ne!(flipped.top(), c.top());
            assert_eq!(flipped.right(), c.right());
        }
    }
}
