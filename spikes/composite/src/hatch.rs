//! The hatch — the quick terminal. A second, borderless, always-on-top
//! window of this Space, summoned by a global hotkey onto the monitor
//! under the pointer. What it shows is a real tab of this Space that
//! lives up here instead of in the sidebar: HOIST any tab up
//! (Ctrl+Shift+↑), LAND it down (Ctrl+Shift+↓). Two looks: the SHEET,
//! 960 wide from the top edge with the Space's band as a lip you drag to
//! resize; the CARD, centred, framed by the carapace. Autohide on focus
//! loss unless pinned; Esc hides.

use std::sync::Arc;
use std::time::Instant;

use nus_render::gpu::Target;
use nus_render::text::Style;
use nus_render::theme::{metric as m, Theme};
use nus_render::{Rect, Scene};
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta};
use winit::keyboard::{Key as WKey, NamedKey};
use winit::window::Window;

use crate::app::{App, Caps, Pane};
use crate::settings::{HatchLook, HatchMonitor};

pub struct Hatch {
    pub window: Arc<Window>,
    pub target: Target,
    pub scene: Scene,
    pub look: HatchLook,
    pub visible: bool,
    pub pinned: bool,
    pub focused: bool,
    pub mods: winit::keyboard::ModifiersState,
    pub pos: (f32, f32),
    pub last_frame: Instant,
    /// 0 = away, 1 = here; the window rides it in.
    pub slide: crate::anim::Anim,
    /// Hiding: once the slide reaches 0 the window goes invisible.
    pub hiding: bool,
    /// The monitor it's on: origin and size in physical px, its scale.
    pub mon: (i32, i32, u32, u32, f32),
    /// Where the window sits when fully shown, physical px.
    pub home: (i32, i32),
    pub size: (u32, u32),
    /// Sheet: the height as a fraction of the monitor, once dragged.
    pub frac: f32,
    pub lip_drag: Option<(f32, f32)>,
    pub frame_drag: Option<(i32, i32, f32, f32)>,
    pub hits: Vec<(Rect, Hit)>,
    /// A line for the foot (a notice), and when.
    pub notice: Option<(String, Instant)>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hit {
    Land,
    Pin,
    Close,
    Lip,
    Frame,
    Corner,
}

impl App {
    /// The tab that lives in the hatch, if any.
    pub(crate) fn hatch_tab(&self) -> Option<usize> {
        self.tabs.iter().position(|t| t.hatch)
    }

    /// Make sure there's a hatch tab: a fresh shell in the active tab's
    /// cwd when none is up.
    fn ensure_hatch_tab(&mut self) -> Option<usize> {
        if let Some(i) = self.hatch_tab() {
            return Some(i);
        }
        let profile = self.behavior.default_profile;
        let cwd = self.tabs.get(self.active).and_then(|t| match &t.left {
            Pane::Term(term) => term.term.cwd.clone(),
            _ => None,
        });
        let mut t = self.new_term_pane(false, profile).ok()?;
        if let Some(c) = cwd {
            // The shell starts at home; a cd typed ahead lands it where you are.
            let _ = t.pty.write(format!("cd \"{c}\"\r").as_bytes());
        }
        let mut tab = self.make_tab(Pane::Term(t), None);
        tab.hatch = true;
        self.tabs.push(tab);
        Some(self.tabs.len() - 1)
    }

    /// The hotkey, or the chord: show it, or hide it if it's up.
    pub(crate) fn toggle_hatch(&mut self) {
        if self.hatch.as_ref().is_some_and(|h| h.visible && !h.hiding) {
            self.hide_hatch();
        } else {
            self.show_hatch();
        }
    }

    pub(crate) fn show_hatch(&mut self) {
        if self.ensure_hatch_tab().is_none() {
            return;
        }
        if self.hatch.is_none() {
            self.hatch_request = true;
            return;
        }
        self.place_hatch();
        let d = self.motion.dur(crate::anim::base::PALETTE);
        let Some(h) = self.hatch.as_mut() else { return };
        h.hiding = false;
        h.visible = true;
        h.slide.go(1.0, d);
        h.window.set_visible(true);
        h.window.focus_window();
        self.hatch_layout();
        self.dirty = true;
    }

    pub(crate) fn hide_hatch(&mut self) {
        let d = self.motion.dur(crate::anim::base::PALETTE);
        if let Some(h) = self.hatch.as_mut() {
            if !h.visible {
                return;
            }
            h.hiding = true;
            h.slide.go(0.0, d);
            if d <= 0.0 {
                h.visible = false;
                h.window.set_visible(false);
            }
        }
        self.dirty = true;
    }

    /// The window exists now: keep it, sized and placed for the look.
    pub fn attach_hatch(&mut self, window: Arc<Window>) {
        let Ok(target) = self.gpu.target(window.clone()) else { return };
        let look = self.behavior.hatch_look;
        let scale = window.scale_factor() as f32;
        self.hatch = Some(Hatch {
            window,
            target,
            scene: Scene::new(),
            look,
            visible: false,
            pinned: false,
            focused: false,
            mods: Default::default(),
            pos: (0.0, 0.0),
            last_frame: Instant::now(),
            slide: crate::anim::Anim::at(0.0),
            hiding: false,
            mon: (0, 0, 1920, 1080, scale),
            home: (0, 0),
            size: (960, 400),
            frac: self.behavior.hatch_size as f32 / 100.0,
            lip_drag: None,
            frame_drag: None,
            hits: Vec::new(),
            notice: None,
        });
        self.show_hatch();
    }

    /// Pick the monitor and size the window for the look.
    fn place_hatch(&mut self) {
        let which = self.behavior.hatch_monitor;
        let main = self.window.clone();
        let Some(h) = self.hatch.as_mut() else { return };
        let monitors: Vec<winit::monitor::MonitorHandle> = h.window.available_monitors().collect();
        let pick = match which {
            HatchMonitor::Pointer => crate::hotkey::pointer().and_then(|(x, y)| {
                monitors.iter().find(|mo| {
                    let p = mo.position();
                    let s = mo.size();
                    x >= p.x && y >= p.y && x < p.x + s.width as i32 && y < p.y + s.height as i32
                })
            }),
            HatchMonitor::Foreground => main.current_monitor().and_then(|cm| monitors.iter().find(|mo| mo.position() == cm.position())),
            HatchMonitor::Primary => None,
        };
        let mo = pick.cloned().or_else(|| h.window.primary_monitor()).or_else(|| monitors.first().cloned());
        let Some(mo) = mo else { return };
        let (mx, my) = (mo.position().x, mo.position().y);
        let (mw, mh) = (mo.size().width, mo.size().height);
        let scale = mo.scale_factor() as f32;
        h.mon = (mx, my, mw, mh, scale);
        match h.look {
            HatchLook::Sheet => {
                let w = ((960.0 * scale) as u32).min(mw.saturating_sub((32.0 * scale) as u32));
                let hh = ((mh as f32 * h.frac) as u32).clamp((160.0 * scale) as u32, mh - (40.0 * scale) as u32);
                h.size = (w, hh);
                h.home = (mx + (mw as i32 - w as i32) / 2, my);
            }
            HatchLook::Card => {
                let w = (mw as f32 * 0.7) as u32;
                let hh = (mh as f32 * 0.6) as u32;
                h.size = (w, hh);
                h.home = (mx + (mw as i32 - w as i32) / 2, my + (mh as i32 - hh as i32) / 2);
            }
        }
        let _ = h.window.request_inner_size(winit::dpi::PhysicalSize::new(h.size.0, h.size.1));
        h.window.set_outer_position(winit::dpi::PhysicalPosition::new(h.home.0, h.home.1));
    }

    /// Where the window is right now on its slide.
    fn hatch_ride(&mut self) {
        let Some(h) = self.hatch.as_mut() else { return };
        if !h.visible {
            return;
        }
        let s = h.slide.value();
        let (x, y) = match h.look {
            HatchLook::Sheet => (h.home.0, h.home.1 - ((1.0 - s) * h.size.1 as f32) as i32),
            HatchLook::Card => (h.home.0, h.home.1 + ((1.0 - s) * 12.0 * h.mon.4) as i32),
        };
        h.window.set_outer_position(winit::dpi::PhysicalPosition::new(x, y));
        if h.hiding && !h.slide.active() {
            h.visible = false;
            h.hiding = false;
            h.window.set_visible(false);
        }
    }

    /// The tab's panes get their rects inside the window.
    fn hatch_layout(&mut self) {
        let Some(i) = self.hatch_tab() else { return };
        let header = self.header_h();
        let pad_x = self.px(18.0);
        let pad_y = self.px(12.0);
        let scale = self.scale;
        let rule = self.px(m::STRUCTURE);
        let Some(h) = self.hatch.as_ref() else { return };
        let (w, hh) = (h.target.size.0 as f32, h.target.size.1 as f32);
        let content = match h.look {
            HatchLook::Sheet => {
                let edge = (2.0 * scale).round();
                let head = (34.0 * scale).round();
                let foot = (26.0 * scale).round();
                let lip = (6.0 * scale).round();
                Rect::new(edge, head, w - 2.0 * edge, hh - head - foot - lip - edge)
            }
            HatchLook::Card => {
                let frame = (22.0 * scale).round();
                let head = (22.0 * scale).round();
                let foot = (16.0 * scale).round();
                let border = (2.0 * scale).round();
                Rect::new(frame + border, frame + head + border, w - 2.0 * (frame + border), hh - 2.0 * (frame + border) - head - foot)
            }
        };
        let split_w = self.split_width(i, content.w);
        let Some(tab) = self.tabs.get_mut(i) else { return };
        if tab.right.is_some() {
            let lw = content.w - split_w - rule;
            let l = Rect::new(content.x, content.y, lw, content.h);
            let r = Rect::new(content.x + lw + rule, content.y, split_w, content.h);
            crate::app::place_pane(&mut tab.left, l, header, pad_x, pad_y, scale, true);
            if let Some(p) = tab.right.as_mut() {
                crate::app::place_pane(p, r, header, pad_x, pad_y, scale, true);
            }
        } else {
            crate::app::place_pane(&mut tab.left, content, header, pad_x, pad_y, scale, false);
        }
        if self.resize_due.is_none() {
            self.resize_due = Some(Instant::now());
        }
    }

    /// HOIST: the active tab goes up; whatever was up comes down.
    pub(crate) fn hoist(&mut self) {
        let active = self.active;
        if self.tabs.get(active).is_none_or(|t| t.hatch || t.peek.is_some()) {
            return;
        }
        if let Some(i) = self.hatch_tab() {
            self.tabs[i].hatch = false;
        }
        self.tabs[active].hatch = true;
        // The sidebar needs a new active tab.
        let next = self.mru.iter().copied().find(|&i| i != active && i < self.tabs.len() && !self.tabs[i].hatch).or_else(|| (0..self.tabs.len()).find(|&i| !self.tabs[i].hatch));
        match next {
            Some(n) => self.activate(n),
            None => {
                let p = self.behavior.default_profile;
                self.new_tab(p);
            }
        }
        self.play_event("toggle");
        self.show_hatch();
    }

    /// LAND: the hatch's tab comes down as a normal tab, focused.
    pub(crate) fn land(&mut self) {
        let Some(i) = self.hatch_tab() else { return };
        self.tabs[i].hatch = false;
        self.hide_hatch();
        self.activate(i);
        self.window.focus_window();
        self.play_event("toggle");
        self.layout();
        self.dirty = true;
    }

    /// Run `f` as if the hatch tab were active (keys, mouse), then put the
    /// main window's world back.
    fn in_hatch<R>(&mut self, f: impl FnOnce(&mut App) -> R) -> Option<R> {
        let i = self.hatch_tab()?;
        let (active, mouse) = (self.active, self.mouse);
        let pos = self.hatch.as_ref().map(|h| h.pos).unwrap_or((0.0, 0.0));
        self.active = i;
        self.mouse = pos;
        let r = f(self);
        self.active = active;
        self.mouse = mouse;
        self.hatch_layout();
        Some(r)
    }

    // --- events from the hatch window ---

    pub fn hatch_resized(&mut self, w: u32, h: u32) {
        if let Some(hat) = self.hatch.as_mut() {
            hat.target.resize(&self.gpu.device, w, h);
        }
        self.hatch_layout();
        self.apply_term_resizes(true);
    }

    pub fn hatch_focus(&mut self, f: bool) {
        let autohide = self.behavior.hatch_autohide;
        if let Some(i) = self.hatch_tab() {
            if let Pane::Web(w) = &self.tabs[i].left {
                w.tab.focus(f);
            }
        }
        let Some(h) = self.hatch.as_mut() else { return };
        h.focused = f;
        if !f && autohide && !h.pinned && h.visible && !h.hiding && h.lip_drag.is_none() && h.frame_drag.is_none() {
            self.hide_hatch();
        }
        self.dirty = true;
    }

    pub fn hatch_modifiers(&mut self, mo: winit::keyboard::ModifiersState) {
        if let Some(h) = self.hatch.as_mut() {
            h.mods = mo;
        }
        self.mods = mo;
    }

    pub fn hatch_key(&mut self, ev: &KeyEvent) {
        let pressed = ev.state == ElementState::Pressed;
        let ctrl = self.mods.control_key();
        let shift = self.mods.shift_key();
        if pressed {
            match &ev.logical_key {
                WKey::Named(NamedKey::Escape) if !ctrl => {
                    // Esc hides — unless something inside wants it (a find, hints).
                    if self.in_hatch(|a| a.term_mode_key(ev)).unwrap_or(false) {
                        self.dirty = true;
                        return;
                    }
                    self.hide_hatch();
                    return;
                }
                WKey::Named(NamedKey::ArrowDown) if ctrl && shift => return self.land(),
                WKey::Named(NamedKey::ArrowUp) if ctrl && shift => return self.toggle_pin(),
                _ => {}
            }
        }
        self.in_hatch(|a| a.key(ev));
        self.dirty = true;
    }

    pub(crate) fn toggle_pin(&mut self) {
        if let Some(h) = self.hatch.as_mut() {
            h.pinned = !h.pinned;
            h.notice = Some((if h.pinned { "pinned · stays up when you look away".into() } else { "unpinned".into() }, Instant::now()));
        }
        self.play_event("toggle");
        self.dirty = true;
    }

    pub fn hatch_cursor(&mut self, pos: (f32, f32)) {
        let Some(h) = self.hatch.as_mut() else { return };
        h.pos = pos;
        if let Some((y0, frac0)) = h.lip_drag {
            // Sheet: the lip drags the height.
            let dy = pos.1 - y0;
            let mh = h.mon.3 as f32;
            h.frac = (frac0 + dy / mh).clamp(0.15, 0.95);
            let hh = (mh * h.frac) as u32;
            h.size.1 = hh;
            let _ = h.window.request_inner_size(winit::dpi::PhysicalSize::new(h.size.0, hh));
            self.dirty = true;
            return;
        }
        if let Some((hx, hy, x0, y0)) = h.frame_drag {
            // Card: the frame drags the window.
            let (dx, dy) = ((pos.0 - x0) as i32, (pos.1 - y0) as i32);
            h.home = (hx + dx, hy + dy);
            h.window.set_outer_position(winit::dpi::PhysicalPosition::new(h.home.0, h.home.1));
            return;
        }
        self.in_hatch(|a| a.term_drag(pos.0, pos.1));
        self.in_hatch(|a| a.editor_motion(pos.0, pos.1));
    }

    pub fn hatch_mouse(&mut self, button: MouseButton, state: ElementState, pos: (f32, f32)) {
        let pressed = state == ElementState::Pressed;
        let Some(h) = self.hatch.as_mut() else { return };
        h.pos = pos;
        if !pressed && button == MouseButton::Left {
            if h.lip_drag.take().is_some() {
                self.behavior.hatch_size = (h.frac * 100.0).round() as u8;
                self.save_prefs();
                self.apply_term_resizes(true);
                return;
            }
            if h.frame_drag.take().is_some() {
                return;
            }
        }
        if pressed && button == MouseButton::Left {
            let hit = h.hits.iter().find(|(r, _)| r.contains(pos.0, pos.1)).map(|(_, k)| *k);
            match hit {
                Some(Hit::Land) => return self.land(),
                Some(Hit::Pin) => return self.toggle_pin(),
                Some(Hit::Close) => return self.hide_hatch(),
                Some(Hit::Lip) => {
                    h.lip_drag = Some((pos.1, h.frac));
                    return;
                }
                Some(Hit::Frame) => {
                    h.frame_drag = Some((h.home.0, h.home.1, pos.0, pos.1));
                    return;
                }
                Some(Hit::Corner) => {
                    h.lip_drag = Some((pos.1, h.frac));
                    return;
                }
                None => {}
            }
        }
        self.in_hatch(|a| {
            if a.editor_mouse(button, state, pos.0, pos.1) {
                return;
            }
            a.term_mouse(button, state, pos.0, pos.1);
        });
        self.dirty = true;
    }

    pub fn hatch_wheel(&mut self, delta: MouseScrollDelta, pos: (f32, f32)) {
        if let Some(h) = self.hatch.as_mut() {
            h.pos = pos;
        }
        self.in_hatch(|a| a.wheel(delta));
        self.dirty = true;
    }

    // --- the frame ---

    pub fn hatch_frame(&mut self) {
        self.hatch_ride();
        let Some(h) = self.hatch.as_ref() else { return };
        if h.slide.active() {
            self.dirty = true;
            h.window.request_redraw();
        }
        if !h.visible || h.last_frame.elapsed().as_millis() < 16 {
            return;
        }
        let Some(i) = self.hatch_tab() else { return };
        let mut h = self.hatch.take().unwrap();
        h.last_frame = Instant::now();
        h.hits.clear();
        let scale = h.window.scale_factor() as f32;
        let (w, hh) = (h.target.size.0 as f32, h.target.size.1 as f32);
        let t: Theme = self.theme.clone();
        let ink = t.ink;
        let paper = t.paper;
        let signal = self.surface.signal;
        let label = Style { font: self.f.ui, px: (m::LABEL_PX * scale).round(), color: ink, tracking: m::LABEL_PX * scale * m::LABEL_TRACKING };
        let strong = Style { font: self.f.strong, ..label };
        let dim = Style { color: t.dim, ..label };
        let (mx, my) = h.pos;
        h.scene.clear();
        h.scene.layer(None);
        h.scene.rect(Rect::new(0.0, 0.0, w, hh), paper);
        let px = |v: f32| (v * scale).round();

        // Masthead geometry per look.
        let (head_y, head_h, head_x0, head_x1, content_edge) = match h.look {
            HatchLook::Sheet => (0.0, px(34.0), px(16.0), w - px(16.0), px(2.0)),
            HatchLook::Card => {
                let frame = px(22.0);
                // The carapace frame: the one surface texture belongs on.
                h.scene.rect(Rect::new(0.0, 0.0, w, hh), crate::surface::mix(paper, ink, 0.03));
                if let (Some(kind), true) = (self.surface.texture_kind.shader_kind(), self.surface.texture > 0.0) {
                    h.scene.push(nus_render::Instance::texture_kind(Rect::new(0.0, 0.0, w, hh), kind, [1.0, 1.0, 1.0, self.surface.texture], self.surface.texture_scale * scale, 0.0));
                }
                (frame - px(14.0), px(22.0), frame, w - frame, frame + px(2.0))
            }
        };
        let _ = content_edge;
        // Wordmark · signal · space · tab title.
        let wm = Style { font: self.f.wordmark, px: px(if h.look == HatchLook::Sheet { 20.0 } else { 18.0 }), color: ink, tracking: 0.0 };
        let base = head_y + head_h / 2.0 + px(m::LABEL_PX) / 2.0 - px(2.0);
        let mut x = head_x0;
        let word = if h.look == HatchLook::Sheet { "quick" } else { "hatch" };
        x += self.fonts.draw(&mut h.scene, wm, x, base + px(1.0), word) + px(14.0);
        let sq = px(10.0);
        h.scene.rect(Rect::new(x, base - sq + px(1.0), sq, sq), signal);
        x += sq + px(10.0);
        x += self.fonts.draw(&mut h.scene, strong, x, base, &self.space_name.clone().caps()) + px(8.0);
        let title = self.tabs[i].title().caps();
        let title_w = head_x1 - x - px(260.0);
        let title = self.fit(dim, &format!("· {title}"), title_w.max(px(60.0)));
        self.fonts.draw(&mut h.scene, dim, x, base, &title);
        // Chords, right to left: ESC · PIN · LAND.
        let mut rx = head_x1;
        let chords: [(String, Hit, bool); 3] = [
            ("ESC".into(), Hit::Close, false),
            (if h.pinned { "PINNED".into() } else { "PIN · CTRL+SHIFT+↑".into() }, Hit::Pin, h.pinned),
            ("LAND · CTRL+SHIFT+↓".into(), Hit::Land, false),
        ];
        for (word, hit, on) in chords {
            let ww = self.fonts.measure(label, &word);
            rx -= ww;
            let hr = Rect::new(rx - px(6.0), head_y, ww + px(12.0), head_h);
            let hot = hr.contains(mx, my);
            let color = if on || hot { ink } else { t.dim };
            self.fonts.draw(&mut h.scene, Style { color, ..label }, rx, base, &word);
            h.hits.push((hr, hit));
            rx -= px(18.0);
        }
        match h.look {
            HatchLook::Sheet => {
                h.scene.hline(0.0, head_h - px(1.0), w, px(1.0), ink);
            }
            HatchLook::Card => {}
        }

        // The panes.
        let n = self.tab_label(i);
        let look = self.tabs[i].look.clone();
        let focus_right = self.tabs[i].focus_right;
        let has_right = self.tabs[i].right.is_some();
        let saved = (self.active, self.mouse);
        self.active = i;
        self.mouse = h.pos;
        let mut tabs = std::mem::take(&mut self.tabs);
        {
            let tab = &mut tabs[i];
            let left_focused = !(focus_right && has_right);
            let split = tab.right.is_some();
            self.draw_pane(&mut h.scene, &mut tab.left, &n, h.focused && left_focused, &look, split);
            if let Some(r) = tab.right.as_mut() {
                self.draw_pane(&mut h.scene, r, &n, h.focused && !left_focused, &look, true);
            }
        }
        self.tabs = tabs;
        self.active = saved.0;
        self.mouse = saved.1;
        h.scene.layer(None);
        if let Some(tab) = self.tabs.get(i) {
            if tab.right.is_some() {
                let lr = tab.left.rect();
                h.scene.vline(lr.right(), lr.y, lr.h, px(m::STRUCTURE), ink);
            }
        }

        // Foot and edges per look.
        match h.look {
            HatchLook::Sheet => {
                let edge = px(2.0);
                let lip = px(6.0);
                let foot = px(26.0);
                let fy = hh - lip - edge - foot;
                h.scene.hline(edge, fy, w - 2.0 * edge, px(1.0), ink);
                let fb = fy + foot / 2.0 + px(m::LABEL_PX) / 2.0 - px(2.0);
                let left = match (&h.notice, &self.board.toast) {
                    (Some((s, at)), _) if at.elapsed().as_secs() < 4 => s.clone(),
                    (_, Some((s, _, _))) => s.clone(),
                    _ => format!("{} · {}", self.behavior.hatch_hotkey.label(), if self.hotkey.as_ref().is_some_and(|k| k.status.is_empty()) { "summons from anywhere" } else { "chord inside nus only" }).to_lowercase(),
                };
                self.fonts.draw(&mut h.scene, dim, px(16.0), fb, &self.fit(dim, &left.caps(), w * 0.6));
                let hint = "DRAG THE EDGE TO RESIZE";
                let hw = self.fonts.measure(dim, hint);
                self.fonts.draw(&mut h.scene, dim, w - px(16.0) - hw, fb, hint);
                // Edges: left, right, bottom; no top. Then the lip.
                h.scene.rect(Rect::new(0.0, 0.0, edge, hh - lip), ink);
                h.scene.rect(Rect::new(w - edge, 0.0, edge, hh - lip), ink);
                h.scene.rect(Rect::new(0.0, hh - lip - edge, w, edge), ink);
                let lip_r = Rect::new(0.0, hh - lip, w, lip);
                h.scene.rect(lip_r, signal);
                if let (Some(kind), true) = (self.surface.texture_kind.shader_kind(), self.surface.texture > 0.0) {
                    h.scene.push(nus_render::Instance::texture_kind(lip_r, kind, [1.0, 1.0, 1.0, self.surface.texture], self.surface.texture_scale * scale, 0.0));
                }
                h.hits.push((Rect::new(0.0, hh - lip - px(6.0), w, lip + px(6.0)), Hit::Lip));
            }
            HatchLook::Card => {
                let frame = px(22.0);
                let border = px(2.0);
                let inner = Rect::new(frame, frame + px(22.0), w - 2.0 * frame, hh - 2.0 * frame - px(22.0) - px(16.0));
                h.scene.push(nus_render::Instance::stroke(inner, 0.0, px(m::STRUCTURE), ink, None, 0.0));
                // Frame foot: a short signal mark, the hint.
                let fy = hh - frame - px(6.0);
                h.scene.rect(Rect::new(frame, fy, px(120.0), px(4.0)), signal);
                let hint = "DRAG THE FRAME TO MOVE · THE CORNER RESIZES";
                let hw = self.fonts.measure(dim, hint);
                self.fonts.draw(&mut h.scene, dim, w - frame - hw, fy + px(5.0), hint);
                // Outer edge, hard shadow is the OS's job we don't have: 2px ink.
                h.scene.push(nus_render::Instance::stroke(Rect::new(0.0, 0.0, w, hh), 0.0, border, ink, None, 0.0));
                // Frame hits: anywhere on the frame drags; the bottom-right corner resizes.
                let corner = Rect::new(w - frame, hh - frame, frame, frame);
                h.hits.push((corner, Hit::Corner));
                for r in [Rect::new(0.0, 0.0, w, frame), Rect::new(0.0, hh - frame, w, frame), Rect::new(0.0, 0.0, frame, hh), Rect::new(w - frame, 0.0, frame, hh)] {
                    h.hits.push((r, Hit::Frame));
                }
            }
        }
        h.scene.finish();
        for (x, y, w, hgt, data) in self.fonts.uploads.drain(..) {
            self.gpu.upload_glyph(x, y, w, hgt, &data);
        }
        self.gpu.render(&mut h.target, &h.scene, paper);
        self.hatch = Some(h);
    }

    /// Called with the app's redraw: the hatch repaints when the app does.
    pub fn hatch_redraw(&mut self) {
        if let Some(h) = self.hatch.as_ref() {
            if h.visible {
                h.window.request_redraw();
            }
        }
    }

    /// The look or hotkey setting changed: re-place, re-register.
    pub(crate) fn hatch_settings_changed(&mut self) {
        if let Some(h) = self.hatch.as_mut() {
            h.look = self.behavior.hatch_look;
            h.frac = self.behavior.hatch_size as f32 / 100.0;
        }
        if self.hatch.as_ref().is_some_and(|h| h.visible) {
            self.place_hatch();
            self.hatch_layout();
            self.apply_term_resizes(true);
        }
        let chord = self.behavior.hatch_hotkey;
        if self.hotkey.as_ref().is_none_or(|k| k.chord != chord) {
            self.hotkey = None;
            self.hotkey = Some(crate::hotkey::Hotkey::register(chord, self.proxy.clone()));
        }
    }
}
