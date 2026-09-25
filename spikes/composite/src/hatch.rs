//! The hatch — the quick terminal. A second, borderless, always-on-top
//! window of this Space, summoned by a global hotkey onto the monitor
//! under the pointer. What it shows is a real tab of this Space that
//! lives up here instead of in the sidebar: HOIST any tab up
//! (Ctrl+Shift+↑), LAND it down (Ctrl+Shift+↓). Two looks: the SHEET,
//! 960 wide from the top edge with the Space's band as a lip you drag to
//! resize; the CARD, centred, framed by the carapace. Autohide on focus
//! loss unless pinned. Escape belongs to the live terminal.

use std::sync::Arc;
use std::time::Instant;

use nus_render::gpu::Target;
use nus_render::text::Style;
use nus_render::theme::metric as m;
use nus_render::{Rect, Scene};
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta};
use winit::keyboard::{Key as WKey, NamedKey};
use winit::window::Window;

use crate::app::{App, Pane};
use crate::settings::{HatchLook, HatchMonitor};
use crate::hatch_work::{Item, Target as WorkTarget};
#[path = "hatch_ui.rs"]
mod ui;

pub struct Overlay {
    pub window: Arc<Window>,
    pub target: Target,
    pub scene: Scene,
    pub visible: bool,
    pub text: String,
    pub position: Option<(i32, i32)>,
}

pub struct State {
    pub work: Vec<Item>,
    pub overview: bool,
    pub selected: usize,
    pub scroll: usize,
    pub main_hidden: bool,
    pub summoned: Option<Instant>,
    pub badge_suppressed: bool,
    pub access: std::collections::HashMap<u64, Hit>,
    pub badge: Option<Overlay>,
    pub shade: Option<Overlay>,
    pub foreground: crate::hatch_native::Foreground,
    pub completion: Option<(Item, Instant)>,
    pub island_open: Option<(i32,i32,u32,u32,f32)>,
}

impl Default for State {
    fn default() -> Self {
        Self { work: Vec::new(), overview: true, selected: 0, scroll: 0, main_hidden: false, summoned: None, badge_suppressed: false, access: Default::default(), badge: None, shade: None, foreground: Default::default(), completion: None, island_open: None }
    }
}

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
    pub position: (i32, i32),
    pub notch: Option<crate::hatch_native::Notch>,
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
    Work,
    Terminal,
    New,
    Job(WorkTarget),
    Previous,
    Next,
    Main,
    Quit,
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
        let t = self.new_term_pane_at(false, profile, cwd).ok()?;
        let mut tab = self.make_tab(Pane::Term(t), None);
        tab.hatch = true;
        self.tabs.push(tab);
        Some(self.tabs.len() - 1)
    }

    /// The hotkey, or the chord: show it, or hide it if it's up.
    pub(crate) fn toggle_hatch(&mut self) {
        if crate::private::enabled() { self.notice(nus_render::text::icons::EYE_SLASH, "Not In Incognito", "the hatch works in regular nus windows"); return; }
        if self.hatch.as_ref().is_some_and(|h| h.visible && !h.hiding) {
            self.hide_hatch();
        } else {
            self.show_hatch();
        }
    }

    pub(crate) fn show_hatch(&mut self) {
        self.hatch_state.summoned = Some(crate::clock::now());
        if !self.hatch_state.overview && self.ensure_hatch_tab().is_none() {
            return;
        }
        if self.hatch.as_ref().is_none_or(|h| !h.visible) {
            self.hatch_state.foreground = crate::hatch_native::Foreground::capture();
        }
        if self.hatch.is_none() {
            let (home, size, _) = self.hatch_geometry();
            self.hatch_request = Some((home, size));
            return;
        }
        self.place_hatch();
        let d = self.motion.dur(crate::anim::base::PALETTE);
        let Some(h) = self.hatch.as_mut() else { return };
        h.hiding = false;
        h.visible = true;
        h.slide.go(1.0, d);
        h.window.set_visible(true);
        if crate::hatch_native::interactive() { h.window.focus_window(); }
        self.hatch_shade();
        self.hatch_layout();
        self.dirty = true;
    }

    pub(crate) fn hide_hatch(&mut self) {
        self.hide_hatch_inner(true);
    }

    pub(crate) fn hide_hatch_inner(&mut self, restore: bool) {
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
        if let Some(shade) = &mut self.hatch_state.shade { shade.window.set_visible(false); shade.visible = false; }
        if restore && crate::hatch_native::interactive() { self.hatch_state.foreground.restore(); }
        self.dirty = true;
    }

    /// The window exists now: keep it, sized and placed for the look.
    pub fn attach_hatch(&mut self, window: Arc<Window>) {
        crate::hatch_native::configure(&window);
        crate::macos::prepare_window(&window);
        window.set_ime_allowed(true);
        let Ok(target) = self.gpu.target(window.clone()) else { return };
        let look = self.behavior.hatch_look;
        let scale = window.scale_factor() as f32;
        self.hatch = Some(Hatch {
            window: window.clone(),
            target,
            scene: Scene::new(),
            look,
            visible: false,
            pinned: false,
            focused: false,
            mods: Default::default(),
            pos: (0.0, 0.0),
            last_frame: crate::clock::now(),
            slide: crate::anim::Anim::at(0.0),
            hiding: false,
            mon: (0, 0, 1920, 1080, scale),
            home: window.outer_position().map(|p| (p.x, p.y)).unwrap_or((0, 0)),
            notch: None,
            position: window.outer_position().map(|p| (p.x, p.y)).unwrap_or((0, 0)),
            size: (window.inner_size().width, window.inner_size().height),
            frac: self.behavior.hatch_size as f32 / 100.0,
            lip_drag: None,
            frame_drag: None,
            hits: Vec::new(),
            notice: None,
        });
        self.show_hatch();
    }

    /// Where the hatch goes and how big, for the look and the monitor
    /// setting: (home, size, monitor) in physical px.
    pub(crate) fn hatch_geometry(&self) -> ((i32, i32), (u32, u32), (i32, i32, u32, u32, f32)) {
        let which = self.behavior.hatch_monitor;
        let main = self.window.clone();
        let look = self.behavior.hatch_look;
        let frac = self.hatch.as_ref().map(|h| h.frac).unwrap_or(self.behavior.hatch_size as f32 / 100.0);
        let monitors: Vec<winit::monitor::MonitorHandle> = main.available_monitors().collect();
        let pick = match which {
            HatchMonitor::Pointer => crate::hotkey::pointer().and_then(|(x, y)| {
                monitors.iter().find(|mo| {
                    let p = mo.position();
                    let s = mo.size();
                    // CoreGraphics uses logical desktop points; winit's
                    // monitor rectangles are physical pixels.
                    #[cfg(target_os = "macos")]
                    let (x,y)=((x as f64*mo.scale_factor()) as i32,(y as f64*mo.scale_factor()) as i32);
                    x >= p.x && y >= p.y && x < p.x + s.width as i32 && y < p.y + s.height as i32
                })
            }),
            HatchMonitor::Foreground => main.current_monitor().and_then(|cm| monitors.iter().find(|mo| mo.position() == cm.position())),
            HatchMonitor::Primary => None,
        };
        let mo = pick.cloned().or_else(|| main.primary_monitor()).or_else(|| monitors.first().cloned());
        let Some(mo) = mo else { return ((0, 0), (960, 400), (0, 0, 1920, 1080, 1.0)) };
        let (mx, my) = (mo.position().x, mo.position().y);
        let (mw, mh) = (mo.size().width, mo.size().height);
        let scale = mo.scale_factor() as f32;
        let top = crate::hatch_native::top_area(mx, my, scale);
        let safe = top.content_top();
        let (size, home) = match look {
            HatchLook::Sheet => {
                let w = ((960.0 * scale) as u32).min(mw.saturating_sub((32.0 * scale) as u32));
                let max_h = mh.saturating_sub(safe as u32 + (32.0 * scale) as u32).max(1);
                let hh = ((mh as f32 * frac) as u32).clamp(((200.0 * scale) as u32).min(max_h), max_h);
                ((w.max(1), hh), (mx + top.notch.map(|n|n.left+n.width as i32/2-w as i32/2).unwrap_or((mw as i32-w as i32)/2).clamp(0,mw.saturating_sub(w) as i32), my + safe))
            }
            HatchLook::Card => {
                let w = (mw as f32 * 0.7) as u32;
                let max_h = mh.saturating_sub((40.0 * scale) as u32).max(1);
                let hh = ((mh as f32 * frac) as u32).clamp(((200.0 * scale) as u32).min(max_h), max_h);
                ((w, hh), (mx + (mw as i32 - w as i32) / 2, my + (mh as i32 - hh as i32) / 2))
            }
        };
        (home, size, (mx, my, mw, mh, scale))
    }

    /// Pick the monitor and size the window for the look.
    fn place_hatch(&mut self) {
        let (home, size, mon) = self.hatch_geometry();
        let Some(h) = self.hatch.as_mut() else { return };
        h.mon = mon;
        h.notch = if h.look==HatchLook::Sheet {crate::hatch_native::top_area(mon.0,mon.1,mon.4).notch} else {None};
        crate::hatch_native::island_level(&h.window,h.notch.is_some());
        h.home = home;
        h.size = size;
        h.target.resize(&self.gpu.device,size.0,size.1);
        let _ = h.window.request_inner_size(winit::dpi::PhysicalSize::new(size.0, size.1));
        h.window.set_outer_position(winit::dpi::PhysicalPosition::new(home.0, home.1));
        h.position = home;
    }

    /// Where the window is right now on its slide.
    pub(crate) fn hatch_ride(&mut self) {
        let Some(h) = self.hatch.as_mut() else { return };
        if !h.visible {
            return;
        }
        let s = h.slide.value();
        let (x, y) = match h.look {
            HatchLook::Sheet if h.notch.is_some() => h.home,
            HatchLook::Sheet => (h.home.0, h.home.1 - ((1.0 - s) * h.size.1 as f32) as i32),
            HatchLook::Card => (h.home.0, h.home.1 + ((1.0 - s) * 12.0 * h.mon.4) as i32),
        };
        if h.position != (x, y) {
            h.window.set_outer_position(winit::dpi::PhysicalPosition::new(x, y));
            h.position = (x, y);
        }
        if h.hiding && !h.slide.active() {
            h.visible = false;
            h.hiding = false;
            h.window.set_visible(false);
        }
    }

    /// The tab's panes get their rects inside the window.
    fn hatch_layout(&mut self) {
        let Some(i) = self.hatch_tab() else { return };
        let scale = self.hatch.as_ref().map(|h| h.window.scale_factor() as f32).unwrap_or(self.scale);
        let header = self.header_h() * scale / self.scale;
        let pad_x = 18.0 * scale;
        let pad_y = 12.0 * scale;
        let rule = m::STRUCTURE * scale;
        let Some(h) = self.hatch.as_ref() else { return };
        let (w, hh) = (h.target.size.0 as f32, h.target.size.1 as f32);
        let edge = if h.look == HatchLook::Card { 16.0 } else { 2.0 } * scale;
        let content = Rect::new(edge, 82.0 * scale, (w-2.0*edge).max(1.0), (hh-116.0*scale).max(1.0));
        let min = (180.0 * scale).min(content.w / 2.0);
        let split_w = (self.tabs[i].split_w.unwrap_or(m::SPLIT) * scale).clamp(min, (content.w-min-rule).max(min));
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
            self.resize_due = Some(crate::clock::now());
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
            None => self.open_home(),
        }
        self.play_event("toggle");
        self.hatch_state.overview = false;
        self.show_hatch();
    }

    /// LAND: the hatch's tab comes down as a normal tab, focused.
    pub(crate) fn land(&mut self) {
        let Some(i) = self.hatch_tab() else { return };
        self.tabs[i].hatch = false;
        self.hide_hatch_inner(false);
        self.activate(i);
        self.hatch_state.main_hidden = false;
        self.window.set_visible(true);
        if crate::hatch_native::interactive() { self.window.focus_window(); }
        self.play_event("toggle");
        self.layout();
        self.dirty = true;
    }

    /// Run `f` as if the hatch tab were active (keys, mouse), then put the
    /// main window's world back.
    pub(crate) fn in_hatch<R>(&mut self, f: impl FnOnce(&mut App) -> R) -> Option<R> {
        let i = self.hatch_tab()?;
        let (active, mouse, mods, scale) = (self.tabs.get(self.active).map(|t| t.id), self.mouse, self.mods, self.scale);
        let pos = self.hatch.as_ref().map(|h| h.pos).unwrap_or((0.0, 0.0));
        self.active = i;
        self.mouse = pos;
        if let Some(h) = &self.hatch { self.mods = h.mods; self.scale = h.window.scale_factor() as f32; }
        let r = f(self);
        self.active = active.and_then(|id| self.tabs.iter().position(|t| t.id == id)).unwrap_or(0).min(self.tabs.len().saturating_sub(1));
        self.mouse = mouse;
        self.mods = mods;
        self.scale = scale;
        self.hatch_layout();
        Some(r)
    }

    // --- events from the hatch window ---

    pub fn hatch_resized(&mut self, w: u32, h: u32) {
        if let Some(hat) = self.hatch.as_mut() {
            hat.target.resize(&self.gpu.device, w, h);
            hat.size = (w, h);
            if let Some(notch)=hat.notch {
                hat.home.0=hat.mon.0+notch.left+(notch.width as i32-w as i32)/2;
            }
        }
        self.hatch_layout();
        self.apply_term_resizes(true);
    }

    pub fn hatch_focus(&mut self, f: bool) {
        let autohide = self.behavior.hatch_autohide;
        if let Some(i) = self.hatch_tab() {
            let tab=&mut self.tabs[i];
            for (right,p) in std::iter::once((false,&mut tab.left)).chain(tab.right.as_mut().map(|p|(true,p))) {
                let focused=f && !self.hatch_state.overview && right==tab.focus_right;
                match p {
                    Pane::Web(w)=>w.tab.focus(focused),
                    Pane::Term(t)=>{if focused {t.waiting=false;} if t.term.modes().contains(nus_vt::Modes::FOCUS_EVENTS) {let _=t.pty.write(if focused {b"\x1b[I"} else {b"\x1b[O"});}},
                    _=>{}
                }
            }
        }
        let Some(h) = self.hatch.as_mut() else { return };
        h.focused = f;
        if !f {h.mods=Default::default();}
        // Summoned from another app, macOS activates nus and hands key status
        // to the main window a moment after the hatch took it. That blur is
        // the summons settling, not the user leaving: take focus back.
        let settling = self.hatch_state.summoned.is_some_and(|t| crate::clock::since(t) < std::time::Duration::from_millis(700));
        if !f && settling && h.visible && !h.hiding {
            if crate::hatch_native::interactive() { h.window.focus_window(); }
        } else if !f && autohide && !h.pinned && h.visible && !h.hiding && h.lip_drag.is_none() && h.frame_drag.is_none() {
            self.hide_hatch_inner(false);
        }
        self.dirty = true;
    }

    pub fn hatch_modifiers(&mut self, mo: winit::keyboard::ModifiersState) {
        if let Some(h) = self.hatch.as_mut() {
            h.mods = mo;
        }
    }

    pub fn hatch_key(&mut self, ev: &KeyEvent) {
        let mods = self.hatch.as_ref().map(|h| h.mods).unwrap_or_default();
        let pressed = ev.state == ElementState::Pressed;
        if pressed && self.behavior.hatch_hotkey.matches(&ev.into(), mods) {
            // A successfully registered OS shortcut is delivered only by the OS.
            if self.hotkey.as_ref().is_none_or(|k| !k.status.is_empty()) { self.toggle_hatch(); }
            return;
        }
        if pressed && mods.control_key() && mods.shift_key() {
            match &ev.logical_key {
                WKey::Named(NamedKey::ArrowDown) => return self.land(),
                WKey::Named(NamedKey::ArrowUp) => return self.toggle_pin(),
                WKey::Character(c) if c.eq_ignore_ascii_case("o") => return self.hatch_click(Hit::Work),
                _ => {}
            }
        }
        if self.hatch_state.overview {
            if pressed {
                let count = self.hatch_state.work.len();
                match ev.logical_key {
                    WKey::Named(NamedKey::Escape) => { self.hide_hatch(); return; }
                    WKey::Named(NamedKey::Tab) if mods.shift_key() => self.hatch_state.selected = self.hatch_state.selected.saturating_sub(1),
                    WKey::Named(NamedKey::ArrowDown) | WKey::Named(NamedKey::Tab) => self.hatch_state.selected = (self.hatch_state.selected + 1).min(count.saturating_sub(1)),
                    WKey::Named(NamedKey::ArrowUp) => self.hatch_state.selected = self.hatch_state.selected.saturating_sub(1),
                    WKey::Named(NamedKey::Home) => self.hatch_state.selected = 0,
                    WKey::Named(NamedKey::End) => self.hatch_state.selected = count.saturating_sub(1),
                    WKey::Character(ref c) if mods.control_key() && c.eq_ignore_ascii_case("n") => return self.hatch_click(Hit::New),
                    WKey::Character(ref c) if mods.control_key() && c.eq_ignore_ascii_case("p") => return self.toggle_pin(),
                    WKey::Named(NamedKey::Enter) => {
                        if let Some(item) = self.hatch_state.work.get(self.hatch_state.selected) { self.hatch_click(Hit::Job(item.target)); }
                    }
                    _ => {}
                }
                if self.hatch_state.selected < self.hatch_state.scroll { self.hatch_state.scroll = self.hatch_state.selected; }
                let shown = self.hatch_visible_rows();
                if self.hatch_state.selected >= self.hatch_state.scroll + shown { self.hatch_state.scroll = self.hatch_state.selected + 1 - shown; }
                self.dirty = true;
            }
            return;
        }
        // Escape is encoded by the terminal (including Vim, tmux, and agents).
        // It is not a universal dismiss key for a live shell.
        self.in_hatch(|a| a.key(ev));
        self.dirty = true;
    }

    pub fn hatch_ime(&mut self, text: &str) {
        if self.hatch_state.overview { return; }
        self.in_hatch(|a| { if let Some(Pane::Term(t)) = a.tabs.get_mut(a.active).map(|tab| tab.focused()) { let _ = t.pty.write(text.as_bytes()); } });
        self.dirty = true;
    }

    pub(crate) fn toggle_pin(&mut self) {
        if let Some(h) = self.hatch.as_mut() {
            h.pinned = !h.pinned;
            h.notice = Some((if h.pinned { "pinned · stays up when you look away".into() } else { "unpinned".into() }, crate::clock::now()));
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
        if self.hatch_state.overview { self.dirty = true; return; }
        self.in_hatch(|a| a.term_drag(pos.0, pos.1));
        self.in_hatch(|a| a.editor_motion(pos.0, pos.1));
        self.in_hatch(|a| {
            if let Some(tab)=a.tabs.get(a.active) {for p in std::iter::once(&tab.left).chain(tab.right.as_ref()) {
                if let Pane::Web(w)=p {if w.page.contains(pos.0,pos.1) {w.tab.mouse_move(((pos.0-w.page.x)/a.scale) as i32,((pos.1-w.page.y)/a.scale) as i32,crate::app::cef_mods(a.mods),false);}}
            }}
        });
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
                Some(hit) => return self.hatch_click(hit),
                None => {}
            }
        }
        if self.hatch_state.overview { return; }
        self.in_hatch(|a| {
            if let Some(tab)=a.tabs.get_mut(a.active) {
                if pressed {if tab.right.as_ref().is_some_and(|p|p.rect().contains(pos.0,pos.1)){tab.focus_right=true;} else if tab.left.rect().contains(pos.0,pos.1){tab.focus_right=false;}}
                for p in std::iter::once(&tab.left).chain(tab.right.as_ref()) {
                    if let Pane::Web(w)=p {if w.page.contains(pos.0,pos.1) {
                        let b=match button {MouseButton::Left=>cef::MouseButtonType::LEFT,MouseButton::Right=>cef::MouseButtonType::RIGHT,MouseButton::Middle=>cef::MouseButtonType::MIDDLE,_=>return};
                        w.tab.mouse_click(((pos.0-w.page.x)/a.scale) as i32,((pos.1-w.page.y)/a.scale) as i32,crate::app::cef_mods(a.mods),b,!pressed,1);w.tab.focus(true);return;
                    }}
                }
            }
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
        if self.hatch_state.overview {
            let y = match delta { MouseScrollDelta::LineDelta(_, y) => y, MouseScrollDelta::PixelDelta(p) => p.y as f32 / 30.0 };
            if y < 0.0 { self.hatch_state.scroll = (self.hatch_state.scroll + 1).min(self.hatch_state.work.len().saturating_sub(self.hatch_visible_rows())); }
            else if y > 0.0 { self.hatch_state.scroll = self.hatch_state.scroll.saturating_sub(1); }
        } else { self.in_hatch(|a| a.wheel(delta)); }
        self.dirty = true;
    }

    // --- the frame ---

    pub fn hatch_frame(&mut self) {
        let Some(h) = self.hatch.as_ref() else { return };
        if !h.visible || crate::clock::since(h.last_frame).as_millis() < 16 { return; }
        let mut h = self.hatch.take().unwrap();
        h.last_frame = crate::clock::now();
        self.draw_hatch(&mut h);
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
    /// The recorder's key: Esc cancels, a lone modifier waits for the rest.
    pub(crate) fn record_hotkey(&mut self, ev: &crate::app::KeyIn) {
        use winit::keyboard::{KeyCode, PhysicalKey};
        let PhysicalKey::Code(code) = ev.physical_key else { return };
        if matches!(code, KeyCode::ControlLeft | KeyCode::ControlRight | KeyCode::ShiftLeft | KeyCode::ShiftRight | KeyCode::AltLeft | KeyCode::AltRight | KeyCode::SuperLeft | KeyCode::SuperRight | KeyCode::CapsLock | KeyCode::Fn) {
            return;
        }
        self.dirty = true;
        if code == KeyCode::Escape {
            self.hotkey_recording = false;
            self.notice(nus_render::text::icons::KEYBOARD, "Hotkey Unchanged", "");
            return;
        }
        match crate::hotkey::Chord::record(code, self.mods) {
            Ok(chord) => {
                self.hotkey_recording = false;
                self.behavior.hatch_hotkey = chord;
                self.hatch_settings_changed();
                self.save_prefs();
                let status = self.hotkey.as_ref().map(|k| k.status.clone()).unwrap_or_default();
                if status.is_empty() {
                    self.notice(nus_render::text::icons::KEYBOARD, "Hatch Hotkey Set", chord.label());
                } else {
                    self.notice(nus_render::text::icons::KEYBOARD, "Hatch Hotkey", format!("{} · {status}", chord.label()));
                }
            }
            Err(why) => self.notice_problem("Could Not Set Hotkey", why),
        }
    }

    pub(crate) fn hatch_settings_changed(&mut self) {
        if let Some(h) = self.hatch.as_mut() {
            h.look = self.behavior.hatch_look;
            h.frac = self.behavior.hatch_size as f32 / 100.0;
        }
        if self.hatch.as_ref().is_some_and(|h| h.visible) {
            self.place_hatch();
            self.hatch_layout();
            self.apply_term_resizes(true);
            self.hatch_shade();
        }
        let chord = self.behavior.hatch_hotkey;
        if (self.ordinal == 0 || self.hotkey.is_some()) && self.hotkey.as_ref().is_none_or(|k| k.chord != chord) {
            self.hotkey = None;
            self.hotkey = Some(crate::hotkey::Hotkey::register(chord, self.proxy.clone()));
        }
    }
}
