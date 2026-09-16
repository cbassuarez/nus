//! Picture-in-picture: our own always-on-top window showing a tab's texture
//! cropped to its video, with eased moves/resizes and transport keys that
//! act on the element through the DevTools channel.

use std::sync::Arc;
use std::time::{Duration, Instant};

use nus_render::theme::metric as m;
use nus_render::{Rect, Scene, Target};
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta};
use winit::keyboard::{Key as WKey, NamedKey};
use winit::window::Window;

use crate::app::App;
use crate::browser::Video;

/// Logical-pixel rectangle on the desktop.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

pub struct Pip {
    pub window: Arc<Window>,
    pub target: Target,
    pub scene: Scene,
    /// Which tab and pane the video lives in.
    pub tab: usize,
    pub right: bool,
    pub focused: bool,
    pub cur: LRect,
    pub from: LRect,
    pub to: LRect,
    pub anim_start: Instant,
    pub last_frame: Instant,
    pub last_click: Instant,
    pub dragging: bool,
    /// Left button is down (a drag starts once the cursor moves).
    pub pressed: bool,
    /// Desktop work area (logical) for snapping.
    pub area: LRect,
}

const ANIM: Duration = Duration::from_millis(260);
const MARGIN: f64 = 24.0;

fn ease_out_cubic(t: f64) -> f64 {
    let u = 1.0 - t.clamp(0.0, 1.0);
    1.0 - u * u * u
}

impl Pip {
    pub fn new(window: Arc<Window>, target: Target, tab: usize, right: bool, area: LRect, start: LRect) -> Pip {
        Pip {
            window,
            target,
            scene: Scene::new(),
            tab,
            right,
            focused: false,
            cur: start,
            from: start,
            to: start,
            anim_start: Instant::now(),
            last_frame: Instant::now(),
            last_click: Instant::now() - Duration::from_secs(1),
            dragging: false,
            pressed: false,
            area,
        }
    }

    /// Where a `w`×`h` window should land: the nearest corner to `near`.
    pub fn snap_target(&self, w: f64, h: f64, near: (f64, f64)) -> LRect {
        let a = self.area;
        let left = a.x + MARGIN;
        let right = a.x + a.w - w - MARGIN;
        let top = a.y + MARGIN;
        let bottom = a.y + a.h - h - MARGIN;
        let x = if near.0 < a.x + a.w / 2.0 { left } else { right };
        let y = if near.1 < a.y + a.h / 2.0 { top } else { bottom };
        LRect { x, y, w, h }
    }

    pub fn animate_to(&mut self, to: LRect) {
        if to == self.to {
            return;
        }
        self.from = self.cur;
        self.to = to;
        self.anim_start = Instant::now();
    }

    /// Advance the easing; returns true while moving.
    pub fn step(&mut self) -> bool {
        if self.cur == self.to || self.dragging {
            return false;
        }
        let t = self.anim_start.elapsed().as_secs_f64() / ANIM.as_secs_f64();
        let e = ease_out_cubic(t);
        let lerp = |a: f64, b: f64| a + (b - a) * e;
        let next = if t >= 1.0 {
            self.to
        } else {
            LRect {
                x: lerp(self.from.x, self.to.x),
                y: lerp(self.from.y, self.to.y),
                w: lerp(self.from.w, self.to.w),
                h: lerp(self.from.h, self.to.h),
            }
        };
        if next != self.cur {
            self.cur = next;
            self.window.set_outer_position(LogicalPosition::new(next.x, next.y));
            let _ = self.window.request_inner_size(LogicalSize::new(next.w.max(80.0), next.h.max(45.0)));
        }
        true
    }
}

impl App {
    /// Ask the host to create a PiP window for `tab`'s web pane.
    pub fn request_pip(&mut self, tab: usize, right: bool) {
        if self.pip.is_some() || self.pip_request.is_some() {
            return;
        }
        self.pip_request = Some((tab, right));
    }

    /// Called by the host with the freshly created window.
    pub fn attach_pip(&mut self, window: Arc<Window>, tab: usize, right: bool) {
        let Ok(target) = self.gpu.target(window.clone()) else { return };
        let scale = window.scale_factor();
        let (area, mouse) = match self.window.current_monitor() {
            Some(mon) => {
                let p = mon.position();
                let s = mon.size();
                let a = LRect { x: p.x as f64 / scale, y: p.y as f64 / scale, w: s.width as f64 / scale, h: s.height as f64 / scale };
                (a, (a.x + a.w, a.y + a.h))
            }
            None => (LRect { x: 0.0, y: 0.0, w: 1920.0, h: 1080.0 }, (1920.0, 1080.0)),
        };
        let aspect = self.pane_video(tab, right).map(|v| v.w / v.h.max(1.0)).unwrap_or(16.0 / 9.0) as f64;
        let w = 480.0;
        let h = (w / aspect).round();
        let mut pip = Pip::new(window, target, tab, right, area, LRect { x: 0.0, y: 0.0, w, h });
        let to = pip.snap_target(w, h, mouse);
        // Start small and centered on the destination, then grow into place.
        pip.cur = LRect { x: to.x + w * 0.25, y: to.y + h * 0.25, w: w * 0.5, h: h * 0.5 };
        pip.from = pip.cur;
        pip.to = to;
        pip.anim_start = Instant::now();
        pip.window.set_outer_position(LogicalPosition::new(pip.cur.x, pip.cur.y));
        let _ = pip.window.request_inner_size(LogicalSize::new(pip.cur.w, pip.cur.h));
        pip.window.set_visible(true);
        if let Some(t) = self.web_tab(tab, right) {
            t.eval("__nus.reveal()");
        }
        self.pip = Some(pip);
    }

    pub fn close_pip(&mut self) {
        self.pip = None;
    }

    fn pane_video(&self, tab: usize, right: bool) -> Option<Video> {
        self.web_tab(tab, right).and_then(|t| t.video())
    }

    fn web_tab(&self, tab: usize, right: bool) -> Option<&crate::browser::BrowserTab> {
        let t = self.tabs.get(tab)?;
        let pane = if right { t.right.as_ref()? } else { &t.left };
        match pane {
            crate::app::Pane::Web(w) => Some(&w.tab),
            _ => None,
        }
    }

    /// A playing video in `tab`, if any: (pane is right, video).
    pub fn playing_video(&self, tab: usize) -> Option<bool> {
        for right in [false, true] {
            if let Some(v) = self.pane_video(tab, right) {
                if !v.paused && !v.ended {
                    return Some(right);
                }
            }
        }
        None
    }

    /// Per-frame: draw the cropped video, ease the window, retire PiP when
    /// the tab is gone.
    pub fn pip_frame(&mut self) {
        let Some(pip) = self.pip.as_mut() else { return };
        if pip.tab >= self.tabs.len() {
            self.pip = None;
            return;
        }
        pip.step();
        if pip.last_frame.elapsed() < Duration::from_millis(16) {
            return;
        }
        pip.last_frame = Instant::now();
        let (tab, right) = (pip.tab, pip.right);
        let Some(t) = self.web_tab(tab, right) else {
            self.pip = None;
            return;
        };
        let shared = t.shared.borrow();
        let (bind, video) = (shared.bind.clone(), shared.video.clone());
        drop(shared);
        let pip = self.pip.as_mut().unwrap();
        let theme = self.theme.clone();
        let scale = pip.window.scale_factor() as f32;
        let (w, h) = (pip.target.size.0 as f32, pip.target.size.1 as f32);
        pip.scene.clear();
        pip.scene.layer(None);
        let band = (4.0 * scale).round();
        if let (Some(bind), Some(v)) = (bind, video) {
            let uv = [
                (v.x / v.vw).clamp(0.0, 1.0),
                (v.y / v.vh).clamp(0.0, 1.0),
                ((v.x + v.w) / v.vw).clamp(0.0, 1.0),
                ((v.y + v.h) / v.vh).clamp(0.0, 1.0),
            ];
            pip.scene.texture_uv(Rect::new(0.0, band, w, h - band), uv, bind, None);
            pip.scene.layer(None);
            // Progress: a hairline along the bottom in the signal color.
            if v.dur > 0.0 {
                let p = (v.t / v.dur).clamp(0.0, 1.0) as f32;
                pip.scene.rect(Rect::new(0.0, h - (2.0 * scale).round(), w * p, (2.0 * scale).round()), self.surface.signal);
            }
            if v.paused {
                pip.scene.rect(Rect::new(0.0, band, w, h - band), theme.scrim);
            }
        }
        // Texture lives on the carapace only; the video stays clean.
        pip.scene.rect(Rect::new(0.0, 0.0, w, band), self.surface.signal);
        if let (Some(kind), true) = (self.surface.texture_kind.shader_kind(), self.surface.texture > 0.0) {
            pip.scene.push(nus_render::Instance::texture_kind(Rect::new(0.0, 0.0, w, band), kind, [1.0, 1.0, 1.0, self.surface.texture], self.surface.texture_scale * scale));
        }
        if pip.focused {
            let t = (m::FLOATING * scale).round();
            pip.scene.push(nus_render::Instance::stroke(Rect::new(0.0, 0.0, w, h), 0.0, t, theme.ink, None, 0.0));
        }
        pip.scene.finish();
        let clear = theme.paper;
        let Pip { target, scene, .. } = pip;
        self.gpu.render(target, scene, clear);
    }

    // --- PiP window events -------------------------------------------------

    pub fn pip_focus(&mut self, focused: bool) {
        if let Some(p) = self.pip.as_mut() {
            p.focused = focused;
        }
    }

    pub fn pip_resized(&mut self, w: u32, h: u32) {
        if let Some(p) = self.pip.as_mut() {
            p.target.resize(&self.gpu.device, w, h);
        }
    }

    pub fn pip_key(&mut self, ev: &KeyEvent) {
        if ev.state != ElementState::Pressed {
            return;
        }
        let Some(pip) = self.pip.as_ref() else { return };
        if !pip.focused {
            return;
        }
        let (tab, right) = (pip.tab, pip.right);
        let cmd = match &ev.logical_key {
            WKey::Named(NamedKey::ArrowLeft) => "__nus.seek(-10)",
            WKey::Named(NamedKey::ArrowRight) => "__nus.seek(10)",
            WKey::Named(NamedKey::ArrowUp) => "__nus.vol(0.1)",
            WKey::Named(NamedKey::ArrowDown) => "__nus.vol(-0.1)",
            WKey::Named(NamedKey::Space) => "__nus.toggle()",
            WKey::Named(NamedKey::Escape) => {
                self.return_from_pip();
                return;
            }
            WKey::Character(c) => match c.to_lowercase().as_str() {
                "k" => "__nus.toggle()",
                "j" => "__nus.seek(-10)",
                "l" => "__nus.seek(10)",
                "," => "__nus.step(-1)",
                "." => "__nus.step(1)",
                "m" => "__nus.mute()",
                _ => return,
            },
            _ => return,
        };
        if let Some(t) = self.web_tab(tab, right) {
            t.eval(cmd);
        }
    }

    pub fn pip_mouse(&mut self, button: MouseButton, state: ElementState) {
        let Some(pip) = self.pip.as_mut() else { return };
        if button != MouseButton::Left {
            return;
        }
        match state {
            ElementState::Pressed => {
                if pip.last_click.elapsed() < Duration::from_millis(350) {
                    self.return_from_pip();
                    return;
                }
                pip.last_click = Instant::now();
                pip.focused = true;
                pip.window.focus_window();
                pip.pressed = true;
            }
            ElementState::Released => {
                pip.pressed = false;
                if !pip.dragging {
                    return;
                }
                pip.dragging = false;
                let pos = pip.window.outer_position().map(|p| (p.x as f64, p.y as f64)).unwrap_or((pip.cur.x, pip.cur.y));
                let scale = pip.window.scale_factor();
                let cur = LRect { x: pos.0 / scale, y: pos.1 / scale, w: pip.cur.w, h: pip.cur.h };
                pip.cur = cur;
                let center = (cur.x + cur.w / 2.0, cur.y + cur.h / 2.0);
                let to = pip.snap_target(cur.w, cur.h, center);
                pip.animate_to(to);
            }
        }
    }

    pub fn pip_cursor_entered(&mut self) {
        if let Some(p) = self.pip.as_mut() {
            p.window.focus_window();
            p.focused = true;
        }
    }

    pub fn pip_cursor_moved(&mut self) {
        if let Some(p) = self.pip.as_mut() {
            if p.pressed && !p.dragging {
                p.dragging = true;
                let _ = p.window.drag_window();
            }
        }
    }

    /// Drag ended by the OS (the window moved): keep `cur` honest.
    pub fn pip_moved(&mut self, x: i32, y: i32) {
        if let Some(p) = self.pip.as_mut() {
            if p.dragging {
                let scale = p.window.scale_factor();
                p.cur.x = x as f64 / scale;
                p.cur.y = y as f64 / scale;
            }
        }
    }

    pub fn pip_wheel(&mut self, delta: MouseScrollDelta) {
        let Some(pip) = self.pip.as_mut() else { return };
        let dy = match delta {
            MouseScrollDelta::LineDelta(_, y) => y as f64,
            MouseScrollDelta::PixelDelta(p) => p.y / 40.0,
        };
        let factor = if dy > 0.0 { 1.1 } else { 1.0 / 1.1 };
        let aspect = pip.to.w / pip.to.h.max(1.0);
        let w = (pip.to.w * factor).clamp(240.0, pip.area.w * 0.8);
        let h = (w / aspect).round();
        // Grow around the current center, then snap so it never clips the desktop.
        let center = (pip.to.x + pip.to.w / 2.0, pip.to.y + pip.to.h / 2.0);
        let to = pip.snap_target(w, h, center);
        pip.animate_to(to);
    }

    /// Bring the video's tab back and close PiP.
    pub fn return_from_pip(&mut self) {
        if let Some(p) = self.pip.take() {
            let tab = p.tab;
            drop(p);
            self.window.focus_window();
            self.activate(tab);
        }
    }
}
