//! Orrery, drawn: the map over the window, its keys and its pointer.
//! orrery.rs decides what goes where; this file is the App side — what a
//! window publishes about its tabs, how the map opens (your window shrinks
//! into its own slot, then the rest arrive), and what ↵, letters, arrows
//! and typing do.
//!
//! Type: what people wrote (page titles, place names) is set in the prose
//! face, what machines say (commands, files, hosts) in the UI face, at
//! sizes matched by x-height; nothing that is read goes below the UI size,
//! and the small size is kept for figures you glance at. Colour: every
//! colour comes from the live theme; a place's tint and mark are checked
//! against what is behind them, as `legible` checks the theme's own.

use std::time::Instant;

use nus_render::text::icons;
use nus_render::theme::metric as m;
use nus_render::{Color, Rect, Scene, Style};
use winit::event::{ElementState, MouseButton};
use winit::keyboard::{Key as WKey, KeyCode, NamedKey, PhysicalKey};

use crate::app::{App, Pane};
use crate::orrery::{self as o, Card, CardKey, Form, Kind, State, WindowCards, Zone};

/// The map while it's open.
pub struct Orrery {
    pub query: String,
    pub sel: Option<CardKey>,
    /// When it opened, and the motion's length in seconds (0: reduced).
    pub opened: Instant,
    pub dur: f32,
    /// Leaving toward a card: when, how long, from which rect, and what then.
    pub leaving: Option<(Instant, f32, Rect, Go)>,
    /// Letters typed while the chord's modifiers are held.
    pub letters: String,
    /// Opened by the chord and still held: releasing goes, if you moved.
    pub held_chord: bool,
    pub moved: bool,
    /// Space: the selected card, large, without going.
    pub look: bool,
    /// Something was typed since it opened: from then on ⌫ only erases,
    /// so emptying the line can never run on into closing a tab.
    pub typed: bool,
    pub memory: o::Memory,
    /// Learned or used since it opened: saved on close, off the UI thread.
    pub memory_changed: bool,
    pub roots: o::Roots,
    /// Held shells, refreshed every few seconds (reading the holders' folder).
    pub held: Vec<Card>,
    pub held_at: Instant,
    /// Where the window's own active pane was, for it to shrink from.
    pub origin: Rect,
    /// The last frame's targets, for the pointer and the arrows.
    pub placed: Vec<Placed>,
    pub hover: Option<CardKey>,
    /// A note under the footer for a moment: why something couldn't be done.
    pub note: Option<(String, Instant)>,
}

#[derive(Clone, Debug)]
pub enum Go {
    Tab(u64, u64),
    Bring(u64, u64),
    Held(String),
    Place(String),
    Restore,
}

/// One target on the map: a card or a place with nothing open.
#[derive(Clone)]
pub struct Placed {
    pub key: CardKey,
    pub rect: Rect,
    pub letters: String,
    pub zone: usize,
    pub card: Option<Card>,
    pub matched: bool,
}

/// A zone's rect and its windows', as last laid out.
struct ZoneBox {
    zone: Zone,
    /// The last session's launcher: what it held.
    restore: Option<String>,
    rect: Rect,
    windows: Vec<(u64, String, Rect)>,
    more: Vec<(Rect, usize)>,
}

fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

fn lerp_rect(a: Rect, b: Rect, t: f32) -> Rect {
    Rect::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t, a.w + (b.w - a.w) * t, a.h + (b.h - a.h) * t)
}

impl App {
    /// This window's tabs as cards, for every window's map.
    pub(crate) fn orrery_cards(&self) -> WindowCards {
        let me = u64::from(self.window.id());
        let mut cards = Vec::with_capacity(self.tabs.len());
        for (i, tab) in self.tabs.iter().enumerate() {
            if tab.hatch || tab.peek.is_some() {
                continue;
            }
            let pane = tab.focused_ref();
            let mut c = Card {
                window: me,
                tab: tab.id,
                held: None,
                attached: None,
                kind: Kind::Other,
                state: State::None,
                title: String::new(),
                written: false,
                detail: String::new(),
                cwd: None,
                preview: None,
                aspect: 1.6,
                lines: Vec::new(),
                active: i == self.active,
                idle: crate::clock::since(tab.last_active).as_secs_f32(),
                paints: 0,
            };
            match pane {
                Pane::Term(t) => {
                    c.kind = Kind::Shell;
                    c.title = t.title.clone();
                    c.cwd = t.cwd.as_ref().map(std::path::PathBuf::from);
                    c.detail = t.cwd.as_deref().map(tilde).unwrap_or_default();
                    c.state = if t.waiting {
                        State::Waiting
                    } else if t.running_since.is_some() {
                        State::Running
                    } else {
                        match t.done {
                            Some((Some(0), _)) => State::Passed,
                            Some((Some(_), _)) => State::Failed,
                            _ => State::None,
                        }
                    };
                    let g = t.term.grid();
                    let last = t.term.cursor().row.min(g.rows().saturating_sub(1));
                    let mut lines: Vec<String> = (0..=last).map(|r| g.row(r).text()).collect();
                    while lines.last().is_some_and(|l| l.trim().is_empty()) {
                        lines.pop();
                    }
                    let keep = lines.len().saturating_sub(10);
                    c.lines = lines.split_off(keep);
                    c.attached = t.pty.held_id().map(String::from);
                }
                Pane::Web(w) => {
                    c.kind = Kind::Page;
                    c.written = true;
                    let s = w.tab.shared.borrow();
                    let host = crate::sites::host_of(&s.url);
                    c.title = if !s.title.is_empty() {
                        strip_site(&s.title, &host)
                    } else if !host.is_empty() {
                        host.clone()
                    } else {
                        // A file, or a page with no name yet: its last part.
                        s.url.trim_end_matches('/').rsplit('/').next().filter(|t| !t.is_empty()).unwrap_or("page").to_string()
                    };
                    c.detail = host;
                    if s.size.0 > 0.0 && s.size.1 > 0.0 {
                        c.aspect = s.size.0 / s.size.1;
                    }
                    c.paints = s.paints;
                    c.state = if w.asleep.is_some() {
                        State::Asleep
                    } else if s.media_playing {
                        State::Playing
                    } else {
                        State::None
                    };
                    drop(s);
                    c.preview = w.preview_texture();
                }
                Pane::Editor(e) => {
                    c.kind = Kind::Editor;
                    c.title = e.title();
                    let dir = e.buf().and_then(|b| b.path.as_ref()).and_then(|p| p.parent()).map(|p| p.to_path_buf());
                    c.detail = dir.as_deref().map(|d| tilde(&d.to_string_lossy())).unwrap_or_default();
                    c.cwd = dir;
                }
                _ => {
                    let (t, d) = tab.row_text();
                    c.title = t;
                    c.detail = d;
                }
            }
            if let Some(n) = &tab.name {
                c.title = n.clone();
                c.written = true;
            }
            cards.push(c);
        }
        WindowCards { id: me, ordinal: self.ordinal, name: self.window_name(), cards }
    }

    /// Held shells with no tab in any window.
    fn orrery_held(&self) -> Vec<Card> {
        let attached: std::collections::HashSet<String> = self.orrery_windows().iter().flat_map(|w| w.cards.iter()).filter_map(|c| c.attached.clone()).collect();
        self.held_loose()
            .into_iter()
            .filter(|i| !attached.contains(&i.id))
            .map(|i| Card {
                window: 0,
                tab: 0,
                held: Some(i.id.clone()),
                attached: None,
                kind: Kind::Held,
                state: State::Running,
                title: i.program.clone(),
                written: false,
                detail: i.cwd.as_deref().map(tilde).unwrap_or_default(),
                cwd: i.cwd.as_ref().map(std::path::PathBuf::from),
                preview: None,
                aspect: 1.6,
                lines: Vec::new(),
                active: false,
                idle: 0.0,
                paints: 0,
            })
            .collect()
    }

    /// Open the map. `chord`: by the keyboard chord, whose modifiers are
    /// still down — letting them go after moving goes there (Alt+Tab).
    pub(crate) fn open_orrery(&mut self, chord: bool) {
        let _perf = crate::perf::scope("orrery_open");
        self.palette = None;
        self.start = None;
        let me = u64::from(self.window.id());
        let sel = self.tabs.get(self.active).map(|t| CardKey::Tab(me, t.id));
        // Loaded once per window, then kept: never the disk (or the
        // keychain behind protected state) on the way to the first frame.
        let memory = self.orrery_memory.take().unwrap_or_else(o::Memory::load);
        let roots = std::mem::take(&mut self.orrery_roots);
        let origin = self.content_rect();
        let dur = self.motion.dur(240.0);
        self.orrery = Some(Orrery {
            query: String::new(),
            sel,
            opened: crate::clock::now(),
            dur,
            leaving: None,
            letters: String::new(),
            held_chord: chord,
            moved: false,
            look: false,
            typed: false,
            memory,
            memory_changed: false,
            roots,
            held: Vec::new(),
            held_at: crate::clock::now(),
            origin,
            placed: Vec::new(),
            hover: None,
            note: None,
        });
        let held = self.orrery_held();
        if let Some(or) = self.orrery.as_mut() {
            or.held = held;
        }
        self.orrery_world_wanted = true;
        self.dirty = true;
    }

    pub(crate) fn close_orrery(&mut self) {
        if let Some(or) = self.orrery.take() {
            if or.memory_changed {
                let copy = or.memory.clone();
                let _ = std::thread::Builder::new().name("orrery memory".into()).spawn(move || copy.save());
            }
            self.orrery_memory = Some(or.memory);
            self.orrery_roots = or.roots;
        }
        self.orrery_world_wanted = false;
        self.dirty = true;
    }

    /// Every window's cards: what the host shared, or this window's own
    /// while it hasn't yet.
    fn orrery_windows(&self) -> Vec<WindowCards> {
        let me = u64::from(self.window.id());
        let mut out: Vec<WindowCards> = self.orrery_world.iter().filter(|w| w.id != me).cloned().collect();
        out.push(self.orrery_cards());
        out.sort_by_key(|w| w.ordinal);
        out
    }

    fn orrery_bounds(&self) -> (Rect, Rect, Rect) {
        let (w, h) = (self.target.size.0 as f32, self.target.size.1 as f32);
        let head = self.px(48.0);
        let foot = self.px(44.0);
        let pad = self.px(16.0);
        (Rect::new(0.0, 0.0, w, head), Rect::new(pad, head + pad, w - 2.0 * pad, h - head - foot - 2.0 * pad), Rect::new(0.0, h - foot, w, foot))
    }

    /// Lay the map out: zones by the treemap, windows inside, cards inside.
    fn orrery_layout(&mut self) -> Vec<ZoneBox> {
        let windows = self.orrery_windows();
        let (_, body, _) = self.orrery_bounds();
        let gap = self.px(10.0);
        let label_h = self.px(18.0);
        let head_h = self.px(58.0);
        let scale = self.scale;
        let (px8, px6) = (self.px(8.0), self.px(6.0));
        let restore = self.last_session.as_ref().filter(|s| !s.tabs.is_empty() && !self.atlas_used).map(|s| s.summary());
        let Some(or) = self.orrery.as_mut() else { return Vec::new() };
        if crate::clock::since(or.held_at).as_secs_f32() > 3.0 {
            or.held_at = crate::clock::now();
        }
        let learned = or.memory.learn(
            windows
                .iter()
                .flat_map(|w| w.cards.iter())
                .chain(or.held.iter())
                .filter_map(|c| c.cwd.as_deref())
                .filter_map(|d| or.roots.place_of(d))
                .map(|p| p.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .iter()
                .map(|s| s.as_str()),
        );
        or.memory_changed |= learned;
        let zones = o::zones(&windows, &or.held, &mut or.roots, &or.memory);
        let mut weights: Vec<f32> = zones.iter().map(|z| o::weight(z, &or.memory)).collect();
        if restore.is_some() {
            weights.push(0.6);
        }
        let mut rects = o::treemap(&weights, body, gap);
        let restore_rect = if restore.is_some() { rects.pop() } else { None };
        let n_zones = zones.len();
        let q: Vec<String> = or.query.to_lowercase().split_whitespace().map(String::from).collect();
        let mut placed = Vec::new();
        let mut boxes = Vec::new();
        for (zi, (zone, zr)) in zones.into_iter().zip(rects).enumerate() {
            let inner = Rect::new(zr.x + px8, zr.y + head_h, zr.w - 2.0 * px8, (zr.h - head_h - px8).max(0.0));
            let mut wins = Vec::new();
            let mut more = Vec::new();
            let mut ci = 0usize;
            if zone.is_empty() {
                placed.push(Placed { key: CardKey::Place(zone.key.clone()), rect: inner, letters: o::letters(zi, None), zone: zi, card: None, matched: q.is_empty() || q.iter().all(|t| zone.name.to_lowercase().contains(t.as_str())) });
                boxes.push(ZoneBox { zone, restore: None, rect: zr, windows: wins, more });
                continue;
            }
            let held_h = if zone.held.is_empty() { 0.0 } else { (inner.h * 0.24).clamp(self_px(56.0, scale), self_px(120.0, scale)) };
            let win_area = Rect::new(inner.x, inner.y, inner.w, (inner.h - held_h - if held_h > 0.0 { gap } else { 0.0 }).max(0.0));
            let ww: Vec<f32> = zone.windows.iter().map(|w| (w.cards.len().max(1) as f32).sqrt()).collect();
            let wrects = o::treemap(&ww, win_area, gap);
            for (w, wr) in zone.windows.iter().zip(wrects) {
                let cards_r = Rect::new(wr.x + px6, wr.y + label_h, (wr.w - 2.0 * px6).max(0.0), (wr.h - label_h - px6).max(0.0));
                let (g, fit) = o::grid(w.cards.len(), cards_r, px6, o::SNIPPET_MIN * scale);
                for (c, r) in w.cards.iter().zip(g.iter()) {
                    let matched = q.is_empty() || { let h = c.haystack(); q.iter().all(|t| h.contains(t.as_str())) };
                    placed.push(Placed { key: c.key(), rect: *r, letters: o::letters(zi, Some(ci)), zone: zi, card: Some(c.clone()), matched });
                    ci += 1;
                }
                if fit < w.cards.len() {
                    if let Some(last) = g.last() {
                        more.push((*last, w.cards.len() - fit));
                    }
                }
                wins.push((w.id, w.name.clone(), wr));
            }
            if held_h > 0.0 {
                let hr = Rect::new(inner.x, inner.bottom() - held_h + label_h, inner.w, held_h - label_h);
                let (g, _) = o::grid(zone.held.len(), hr, px6, o::SNIPPET_MIN * scale);
                for (c, r) in zone.held.iter().zip(g.iter()) {
                    let matched = q.is_empty() || { let h = c.haystack(); q.iter().all(|t| h.contains(t.as_str())) };
                    placed.push(Placed { key: c.key(), rect: *r, letters: o::letters(zi, Some(ci)), zone: zi, card: Some(c.clone()), matched });
                    ci += 1;
                }
            }
            boxes.push(ZoneBox { zone, restore: None, rect: zr, windows: wins, more });
        }
        if let (Some(sum), Some(zr)) = (restore, restore_rect) {
            let inner = Rect::new(zr.x + px8, zr.y + head_h, zr.w - 2.0 * px8, (zr.h - head_h - px8).max(0.0));
            let matched = q.is_empty() || q.iter().all(|t| "last session restore".contains(t.as_str()));
            placed.push(Placed { key: CardKey::Restore, rect: inner, letters: o::letters(n_zones, None), zone: n_zones, card: None, matched });
            let zone = Zone { key: String::new(), name: "last session".into(), windows: Vec::new(), held: Vec::new(), hue: usize::MAX };
            boxes.push(ZoneBox { zone, restore: Some(sum), rect: zr, windows: Vec::new(), more: Vec::new() });
        }
        // The selection follows a filter to its first match.
        if !q.is_empty() && !placed.iter().any(|p| Some(&p.key) == or.sel.as_ref() && p.matched) {
            or.sel = placed.iter().find(|p| p.matched).map(|p| p.key.clone());
        }
        or.placed = placed;
        boxes
    }

    // ── Going ─────────────────────────────────────────────────────────────

    fn orrery_choose(&mut self, key: CardKey, bring: bool) {
        let me = u64::from(self.window.id());
        // A window keeps at least one tab: its only one can be gone to, not taken.
        if let CardKey::Tab(w, _) = &key {
            if bring && *w != me && self.orrery_world.iter().find(|x| x.id == *w).is_some_and(|x| x.cards.len() < 2) {
                self.orrery_note("it's that window's only tab · ↵ goes there");
                return;
            }
        }
        let Some(or) = self.orrery.as_mut() else { return };
        let Some(p) = or.placed.iter().find(|p| p.key == key).cloned() else { return };
        let place = p.card.as_ref().and_then(|c| c.cwd.as_deref()).and_then(|d| or.roots.place_of(d)).map(|p| p.to_string_lossy().into_owned());
        if let Some(k) = place.as_deref().or(match &key { CardKey::Place(k) => Some(k.as_str()), _ => None }) {
            or.memory.went(k);
            or.memory_changed = true;
        }
        let go = match key {
            CardKey::Tab(w, t) if bring && w != me => Go::Bring(w, t),
            CardKey::Tab(w, t) => Go::Tab(w, t),
            CardKey::Held(id) => Go::Held(id),
            CardKey::Place(k) => Go::Place(k),
            CardKey::Restore => Go::Restore,
        };
        let content = self.content_rect();
        let Some(or) = self.orrery.as_mut() else { return };
        // A card in this window grows into the window; anything else goes at once.
        let grows = matches!(go, Go::Tab(w, _) if w == me) || matches!(go, Go::Bring(..) | Go::Held(_) | Go::Place(_) | Go::Restore);
        if grows && or.dur > 0.0 {
            or.leaving = Some((crate::clock::now(), or.dur * 0.8, p.rect, go));
            let _ = content;
            self.dirty = true;
        } else {
            self.orrery_finish(go);
        }
    }

    fn orrery_finish(&mut self, go: Go) {
        let me = u64::from(self.window.id());
        self.atlas_used = true;
        self.close_orrery();
        match go {
            Go::Tab(w, t) if w == me => {
                if let Some(i) = self.tabs.iter().position(|x| x.id == t) {
                    self.activate(i);
                }
            }
            Go::Tab(w, t) => self.orrery_go = Some((w, t)),
            Go::Bring(w, t) => self.orrery_bring = Some((w, t)),
            Go::Held(id) => {
                if let Some(info) = self.held_loose().into_iter().find(|i| i.id == id) {
                    self.attach_held(info);
                }
            }
            Go::Place(k) => {
                // A place with nothing open: a shell there, in a window of its own.
                let idx = self.behavior.default_profile;
                if let Ok(t) = self.new_term_pane_at(false, idx, Some(k)) {
                    let tab = self.make_tab(Pane::Term(t), None);
                    let i = self.add_tab(tab);
                    self.activate(i);
                }
            }
            Go::Restore => self.restore_session_pub(),
        }
        self.dirty = true;
    }

    /// Sleep a page now, if it can: not one that plays, waits on a
    /// question, or is the one you're on.
    fn orrery_sleep(&mut self, key: &CardKey) -> Result<(), &'static str> {
        let me = u64::from(self.window.id());
        let CardKey::Tab(w, t) = key else { return Err("only pages sleep") };
        if *w != me {
            return Err("sleep it from its own window");
        }
        let Some(i) = self.tabs.iter().position(|x| x.id == *t) else { return Err("gone") };
        if i == self.active {
            return Err("it's the one you're on");
        }
        let media = self.pip.is_some() || self.little.is_some() || self.docked.is_some();
        let tab = &mut self.tabs[i];
        let Pane::Web(wp) = &mut tab.left else { return Err("only pages sleep") };
        if wp.asleep.is_some() {
            return Ok(());
        }
        if media || wp.tab.shared.borrow().media_playing {
            return Err("playing — stop it first");
        }
        if wp.hands.ask.is_some() || wp.reader.is_some() || !wp.tab.can_suspend() {
            return Err("it can't sleep right now");
        }
        let url = wp.tab.shared.borrow().url.clone();
        if url.is_empty() || url.starts_with("about:") {
            return Err("nothing to wake back to");
        }
        wp.asleep = Some(url);
        wp.slept = Some(crate::clock::now());
        wp.tab.suspend();
        Ok(())
    }

    /// Close a tab of this window from the map. A shell still running
    /// something is refused in words: its "are you sure" would be hidden.
    fn orrery_let_go(&mut self, id: u64) -> Result<(), &'static str> {
        let Some(i) = self.tabs.iter().position(|x| x.id == id) else { return Err("gone") };
        if self.tabs.len() < 2 {
            return Err("the window's last tab stays");
        }
        let busy = |p: &Pane| matches!(p, Pane::Term(t) if t.running_since.is_some());
        if busy(&self.tabs[i].left) || self.tabs[i].right.as_ref().is_some_and(busy) {
            return Err("running — stop it first");
        }
        let keep = self.tabs.get(self.active).map(|t| t.id).filter(|&k| k != id);
        let selected = std::mem::take(&mut self.selected);
        self.activate(i);
        self.close_tabs(true);
        self.selected = selected.into_iter().filter(|&k| k < self.tabs.len()).collect();
        if let Some(k) = keep.and_then(|k| self.tabs.iter().position(|t| t.id == k)) {
            self.activate(k);
        }
        Ok(())
    }

    fn orrery_note(&mut self, s: &str) {
        if let Some(or) = self.orrery.as_mut() {
            or.note = Some((s.to_string(), crate::clock::now()));
        }
    }

    // ── Keys and pointer ──────────────────────────────────────────────────

    /// The chord that opens the map: ⌘⇧M on macOS, Ctrl+Shift+M elsewhere.
    pub(crate) fn orrery_chord(&self, ev: &crate::app::KeyIn) -> bool {
        let app = if cfg!(target_os = "macos") { self.mods.super_key() && self.mods.shift_key() } else { self.mods.control_key() && self.mods.shift_key() };
        ev.state == ElementState::Pressed && app && !self.mods.alt_key() && ev.physical_key == PhysicalKey::Code(KeyCode::KeyM)
    }

    fn chord_held(&self) -> bool {
        if cfg!(target_os = "macos") { self.mods.super_key() } else { self.mods.control_key() }
    }

    /// Modifiers changed: letting the chord go after moving goes there.
    pub(crate) fn orrery_modifiers(&mut self) {
        let held = self.chord_held();
        let Some(or) = self.orrery.as_mut() else { return };
        if or.held_chord && !held {
            or.held_chord = false;
            or.letters.clear();
            if or.moved {
                if let Some(k) = or.sel.clone() {
                    self.orrery_choose(k, false);
                }
            }
            self.dirty = true;
        } else if !held {
            or.letters.clear();
        }
    }

    pub(crate) fn orrery_key(&mut self, ev: &crate::app::KeyIn) -> bool {
        if self.orrery.is_none() {
            return false;
        }
        if ev.state != ElementState::Pressed {
            return true;
        }
        let shift = self.mods.shift_key();
        let chord = self.chord_held();
        let me = u64::from(self.window.id());
        // The chord again while open: step to the next card, Alt+Tab style.
        if self.orrery_chord(ev) {
            self.orrery_step(1, 0);
            return true;
        }
        // Letters, while the chord's modifier is held.
        if chord {
            if let WKey::Character(s) = &ev.logical_key {
                let ch = s.to_lowercase();
                if ch.chars().all(|c| c.is_ascii_alphabetic() || c == ';') {
                    let Some(or) = self.orrery.as_mut() else { return true };
                    or.letters.push_str(&ch);
                    let typed = or.letters.clone();
                    let hit: Vec<CardKey> = or.placed.iter().filter(|p| p.letters.starts_with(&typed)).map(|p| p.key.clone()).collect();
                    match hit.len() {
                        0 => or.letters.clear(),
                        1 if or.placed.iter().any(|p| p.letters == typed) => {
                            or.letters.clear();
                            or.held_chord = false;
                            self.orrery_choose(hit[0].clone(), false);
                        }
                        _ => {}
                    }
                    self.dirty = true;
                    return true;
                }
            }
        }
        let empty = self.orrery.as_ref().is_some_and(|o| o.query.is_empty());
        let typed = self.orrery.as_ref().is_some_and(|o| o.typed);
        match &ev.logical_key {
            WKey::Named(NamedKey::Escape) => {
                let or = self.orrery.as_mut().expect("open");
                if or.look {
                    or.look = false;
                } else if !or.query.is_empty() {
                    or.query.clear();
                } else if !(self.behavior.atlas == crate::settings::AtlasMode::Persistent && !self.atlas_used) {
                    self.atlas_used = true;
                    self.close_orrery();
                }
            }
            WKey::Named(NamedKey::Enter) => {
                if let Some(k) = self.orrery.as_ref().and_then(|o| o.sel.clone()) {
                    self.orrery_choose(k, shift);
                }
            }
            WKey::Named(NamedKey::ArrowLeft) => self.orrery_step(-1, 0),
            WKey::Named(NamedKey::ArrowRight) => self.orrery_step(1, 0),
            WKey::Named(NamedKey::ArrowUp) => self.orrery_step(0, -1),
            WKey::Named(NamedKey::ArrowDown) => self.orrery_step(0, 1),
            WKey::Named(NamedKey::Tab) => self.orrery_zone_step(if shift { -1 } else { 1 }),
            WKey::Named(NamedKey::Space) if empty => {
                if let Some(or) = self.orrery.as_mut() {
                    or.look = !or.look;
                }
            }
            // ⇧⌫: sleep. Letters always type, so no action can eat the first one.
            WKey::Named(NamedKey::Backspace) if empty && !typed && !ev.repeat && shift => {
                if let Some(k) = self.orrery.as_ref().and_then(|o| o.sel.clone()) {
                    match self.orrery_sleep(&k) {
                        Ok(()) => self.orrery_note("asleep · it wakes when you go to it"),
                        Err(why) => self.orrery_note(why),
                    }
                }
            }
            WKey::Named(NamedKey::Backspace) if empty && !typed && !ev.repeat => {
                // Let go: close the tab (reopen-closed brings it back).
                let sel = self.orrery.as_ref().and_then(|o| o.sel.clone());
                match sel {
                    Some(CardKey::Tab(w, t)) if w == me => {
                        match self.orrery_let_go(t) {
                            Ok(()) => self.orrery_note("let go · reopen-closed brings it back"),
                            Err(why) => self.orrery_note(why),
                        }
                    }
                    Some(CardKey::Tab(..)) => self.orrery_note("let it go from its own window"),
                    Some(CardKey::Held(_)) => self.orrery_note("a held shell ends with its process — attach it to close it"),
                    _ => {}
                }
            }
            _ => {
                let mods = self.mods;
                if let Some(or) = self.orrery.as_mut() {
                    let took = crate::field::edit(&mut or.query, ev, mods, 200);
                    if !took.taken() {
                        return true;
                    }
                    or.typed |= took.changed();
                }
            }
        }
        self.dirty = true;
        true
    }

    fn orrery_step(&mut self, dx: i32, dy: i32) {
        let Some(or) = self.orrery.as_mut() else { return };
        let live: Vec<&Placed> = or.placed.iter().filter(|p| p.matched).collect();
        if live.is_empty() {
            return;
        }
        let rects: Vec<Rect> = live.iter().map(|p| p.rect).collect();
        let from = or.sel.as_ref().and_then(|s| live.iter().position(|p| &p.key == s));
        let next = match from {
            None => Some(0),
            Some(i) => o::nearest(&rects, i, dx, dy).or_else(|| (dy == 0 && dx != 0).then(|| ((i as i32 + dx).rem_euclid(live.len() as i32)) as usize)),
        };
        if let Some(n) = next {
            or.sel = Some(live[n].key.clone());
            or.moved = true;
        }
    }

    fn orrery_zone_step(&mut self, d: i32) {
        let Some(or) = self.orrery.as_mut() else { return };
        let zones = or.placed.iter().map(|p| p.zone).max().map(|z| z + 1).unwrap_or(0);
        if zones == 0 {
            return;
        }
        let cur = or.sel.as_ref().and_then(|s| or.placed.iter().find(|p| &p.key == s)).map(|p| p.zone).unwrap_or(0);
        let next = (cur as i32 + d).rem_euclid(zones as i32) as usize;
        if let Some(p) = or.placed.iter().find(|p| p.zone == next) {
            or.sel = Some(p.key.clone());
            or.moved = true;
        }
    }

    pub(crate) fn orrery_pointer(&mut self, x: f32, y: f32) -> bool {
        let Some(or) = self.orrery.as_mut() else { return false };
        let hover = or.placed.iter().find(|p| p.rect.contains(x, y)).map(|p| p.key.clone());
        if hover != or.hover {
            or.hover = hover;
            self.dirty = true;
        }
        true
    }

    pub(crate) fn orrery_mouse(&mut self, button: MouseButton, state: ElementState, x: f32, y: f32) -> bool {
        let Some(or) = self.orrery.as_ref() else { return false };
        if state != ElementState::Pressed || button != MouseButton::Left {
            return true;
        }
        if or.look {
            if let Some(or) = self.orrery.as_mut() {
                or.look = false;
            }
            self.dirty = true;
            return true;
        }
        if let Some(k) = or.placed.iter().find(|p| p.rect.contains(x, y)).map(|p| p.key.clone()) {
            if let Some(or) = self.orrery.as_mut() {
                or.sel = Some(k.clone());
            }
            let shift = self.mods.shift_key();
            self.orrery_choose(k, shift);
        }
        self.dirty = true;
        true
    }

    /// `fit`, with one shaping instead of one per binary-search step: the
    /// map shows dozens of new titles on its first frame, and each step of
    /// a search shapes (and caches) a different string.
    fn ofit<'a>(&self, st: Style, text: impl Into<std::borrow::Cow<'a, str>>, max_w: f32) -> std::borrow::Cow<'a, str> {
        let text = text.into();
        if st.tracking > 0.0 {
            return self.fit(st, text, max_w);
        }
        let glyphs = self.fonts.shape(st.font, st.px, &text);
        let total: f32 = glyphs.iter().map(|g| g.x_advance).sum();
        if total <= max_w + 0.01 {
            return text;
        }
        let room = max_w - self.fonts.measure(st, "…");
        let mut x = 0.0;
        let mut cut = 0usize;
        for g in glyphs.iter() {
            if x + g.x_advance > room {
                break;
            }
            x += g.x_advance;
            cut = cut.max(g.cluster as usize + 1);
        }
        while cut < text.len() && !text.is_char_boundary(cut) {
            cut += 1;
        }
        let cut = cut.min(text.len());
        format!("{}…", text[..cut].trim_end()).into()
    }

    /// The map's faces and sizes, drawn once into a scene nobody sees, a
    /// step per idle turn: the first opening then finds its glyphs ready
    /// instead of rasterizing a page of new ones in its first frame.
    pub(crate) fn orrery_warm(&mut self) {
        const ASCII: &str = " !\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefghijklmnopqrstuvwxyz{|}~…·—";
        if self.orrery_warmed >= 8 || self.orrery.is_some() || crate::clock::since(self.last_key).as_secs_f32() < 2.0 {
            return;
        }
        let ui = self.ui();
        let label = self.label();
        let serif = Style { font: self.f.serif, px: self.px(26.0), color: self.theme.ink, tracking: 0.0 };
        let mut scratch = Scene::new();
        match self.orrery_warmed {
            0 => { self.fonts.draw(&mut scratch, serif, 0.0, 0.0, ASCII); }
            1 => { self.fonts.draw(&mut scratch, Style { px: self.px(20.0), ..serif }, 0.0, 0.0, ASCII); }
            2 => { self.fonts.draw(&mut scratch, Style { px: ui.px * 16.0 / 13.0, ..serif }, 0.0, 0.0, ASCII); }
            3 => { self.fonts.draw(&mut scratch, Style { font: self.f.term, px: label.px, ..ui }, 0.0, 0.0, ASCII); }
            4 => { self.fonts.draw(&mut scratch, Style { font: self.f.term, ..ui }, 0.0, 0.0, ASCII); }
            5 => { self.fonts.draw(&mut scratch, Style { font: self.f.strong, px: label.px * 12.0 / 11.0, ..ui }, 0.0, 0.0, ASCII); }
            6 => {
                // The footer's keys come from a fallback face: finding it is the slow part.
                const KEYS: &str = "↵⇧←↑→↓⌫⌘⌃⌥␣·…—";
                self.fonts.draw(&mut scratch, Style { tracking: 0.0, ..label }, 0.0, 0.0, ASCII);
                self.fonts.draw(&mut scratch, self.label_strong(), 0.0, 0.0, KEYS);
                self.fonts.draw(&mut scratch, self.label(), 0.0, 0.0, "ORRERY HELD");
                self.fonts.draw(&mut scratch, Style { tracking: 0.0, ..label }, 0.0, 0.0, KEYS);
            }
            _ => {
                for icon in [icons::TERMINAL, icons::GLOBE, icons::CODE, icons::APP_WINDOW, icons::CIRCLE_DASHED, icons::CHECK_CIRCLE, icons::X_CIRCLE, icons::BELL, icons::SPEAKER, icons::MOON, icons::PLANET] {
                    for px in [ui.px * 16.0 / 13.0 * 0.85, ui.px * 16.0 / 13.0 * 0.9, self.px(12.0), self.px(16.0), self.px(28.0)] {
                        self.fonts.draw_icon(&mut scratch, icon, px, 0.0, 0.0, self.theme.ink);
                    }
                }
            }
        }
        self.orrery_warmed += 1;
    }

    /// Whether the map is still moving (opening, leaving, a note fading).
    pub(crate) fn orrery_animating(&self) -> bool {
        self.orrery.as_ref().is_some_and(|or| {
            crate::clock::since(or.opened).as_secs_f32() < or.dur + 0.05
                || or.leaving.is_some()
                || or.note.as_ref().is_some_and(|(_, at)| crate::clock::since(*at).as_secs_f32() < 3.0)
        })
    }

    // ── Drawing ───────────────────────────────────────────────────────────

    /// A place's colour from the theme's six, as a mark that reads on `on`.
    fn place_hue(&self, hue: usize) -> Option<Color> {
        if hue == usize::MAX {
            return None;
        }
        let six = self.theme_edit.art.unwrap_or(nus_render::theme::signal::ALL);
        Some(six[hue % 6])
    }

    pub(crate) fn draw_orrery(&mut self, scene: &mut Scene) {
        if self.orrery.is_none() {
            return;
        }
        let _perf = crate::perf::scope("orrery_draw");
        // Leaving: when the card has grown, go.
        if let Some((at, d, _, go)) = self.orrery.as_ref().and_then(|o| o.leaving.clone()) {
            if crate::clock::since(at).as_secs_f32() >= d {
                self.orrery_finish(go);
                return;
            }
        }
        let boxes = self.orrery_layout();
        let t = self.theme.clone();
        let (head, _, foot) = self.orrery_bounds();
        let (w, h) = (self.target.size.0 as f32, self.target.size.1 as f32);
        let me = u64::from(self.window.id());
        let ink_mode = t.mode == nus_render::Mode::Ink;
        let Some(or) = self.orrery.as_ref() else { return };
        let since = crate::clock::since(or.opened).as_secs_f32();
        let (s1, s2) = if or.dur <= 0.0 { (1.0, 1.0) } else { (ease(since / (or.dur * 0.56)), ease((since - or.dur * 0.375) / (or.dur * 0.625))) };
        let leave = or.leaving.as_ref().map(|(at, d, r, _)| (ease(crate::clock::since(*at).as_secs_f32() / d.max(0.001)), *r));
        let fade_all = leave.map(|(k, _)| 1.0 - k).unwrap_or(1.0);
        let sel = or.sel.clone();
        let hover = or.hover.clone();
        let look = or.look;
        let query = or.query.clone();
        let letters_typed = or.letters.clone();
        let chord = self.chord_held();
        let note = or.note.clone().filter(|(_, at)| crate::clock::since(*at).as_secs_f32() < 3.0);
        let origin = or.origin;
        let placed = or.placed.clone();
        let matches = placed.iter().filter(|p| p.matched && p.card.is_some()).count();

        scene.layer(None);
        scene.rect(Rect::new(0.0, 0.0, w, h), crate::app::fade(t.paper, s1));

        // Zones: a tinted region each (common region), the place's name as
        // the landmark, and a line of what it holds.
        let land = Style { font: self.f.serif, px: self.px(26.0), color: t.ink, tracking: 0.0 };
        let land_small = Style { px: self.px(20.0), ..land };
        let meta = Style { color: t.dim, ..self.label() };
        let meta_plain = Style { tracking: 0.0, ..meta };
        let a2 = s2 * fade_all;
        for b in &boxes {
            let hue = self.place_hue(b.zone.hue);
            let tint = match hue {
                Some(c) => crate::surface::mix(t.paper, c, if ink_mode { 0.10 } else { 0.06 }),
                None => crate::surface::mix(t.paper, t.ink, 0.03),
            };
            let r = b.rect;
            scene.rect(r, crate::app::fade(tint, a2));
            if b.zone.is_empty() {
                scene.outline(r, self.px(m::HAIRLINE), crate::app::fade(t.hot, a2));
            }
            let style = if r.w < self.px(400.0) { land_small } else { land };
            let mut x = r.x + self.px(14.0);
            if let Some(c) = hue {
                let mark = nus_render::oklch::readable(c, tint, 3.0);
                let s = self.px(10.0);
                scene.rect(Rect::new(x, r.y + self.px(14.0) + style.px * 0.55 - s, s, s), crate::app::fade(mark, a2));
                x += s + self.px(8.0);
            }
            let name = self.ofit(style, &b.zone.name, r.right() - x - self.px(10.0)).into_owned();
            self.fonts.draw(scene, Style { color: crate::app::fade(nus_render::oklch::readable(t.ink, tint, 7.0), a2), ..style }, x, r.y + self.px(14.0) + style.px * 0.8, &name);
            let n_tabs: usize = b.zone.windows.iter().map(|w| w.cards.len()).sum();
            let mut line = if let Some(sum) = &b.restore { sum.clone() } else if b.zone.key.is_empty() { "belongs nowhere yet".to_string() } else { tilde(&b.zone.key) };
            if b.restore.is_some() {
            } else if b.zone.is_empty() {
                line.push_str(" · nothing open");
            } else {
                line.push_str(&format!(" · {} tab{}", n_tabs, if n_tabs == 1 { "" } else { "s" }));
                if !b.zone.held.is_empty() {
                    line.push_str(&format!(" · {} held", b.zone.held.len()));
                }
            }
            let dimc = nus_render::oklch::readable(t.dim, tint, 4.5);
            let line = self.ofit(meta_plain, &line, r.w - self.px(28.0)).into_owned();
            self.fonts.draw(scene, Style { color: crate::app::fade(dimc, a2), ..meta_plain }, r.x + self.px(14.0), r.y + self.px(14.0) + style.px * 0.8 + self.px(17.0), &line);
            for (id, name, wr) in &b.windows {
                scene.outline(*wr, self.px(m::HAIRLINE), crate::app::fade(t.hot, a2));
                let label = if *id == me { "this window".to_string() } else { name.clone() };
                let label = self.ofit(meta_plain, &label, wr.w - self.px(12.0)).into_owned();
                self.fonts.draw(scene, Style { color: crate::app::fade(dimc, a2), ..meta_plain }, wr.x + self.px(6.0), wr.y + self.px(13.0), &label);
            }
            if !b.zone.held.is_empty() {
                if let Some(first) = placed.iter().find(|p| p.zone == boxes.iter().position(|x| std::ptr::eq(x, b)).unwrap_or(0) && matches!(p.key, CardKey::Held(_))) {
                    self.fonts.draw(scene, Style { color: crate::app::fade(dimc, a2), ..meta }, first.rect.x, first.rect.y - self.px(5.0), "HELD");
                }
            }
            for (r, n) in &b.more {
                let s = format!("+{n} more");
                self.fonts.draw(scene, Style { color: crate::app::fade(dimc, a2), ..meta_plain }, r.right() - self.fonts.measure(meta_plain, &s) - self.px(4.0), r.bottom() + self.px(12.0), &s);
            }
        }

        // Cards. Your own active card travels from where the pane was.
        for p in &placed {
            let mine = matches!(&p.key, CardKey::Tab(wid, _) if *wid == me) && p.card.as_ref().is_some_and(|c| c.active);
            let (mut r, mut alpha) = if mine { (lerp_rect(origin, p.rect, s1), fade_all.max(0.0)) } else { (Rect::new(p.rect.x, p.rect.y + (1.0 - s2) * self.px(8.0), p.rect.w, p.rect.h), a2) };
            if let Some((k, from)) = leave {
                if Some(&p.key) == sel.as_ref() {
                    r = lerp_rect(from, self.content_rect(), k);
                    alpha = 1.0;
                }
            }
            if !p.matched {
                alpha *= 0.28;
            }
            let selected = Some(&p.key) == sel.as_ref();
            let hovered = Some(&p.key) == hover.as_ref();
            self.draw_orrery_card(scene, p, r, alpha, selected, hovered, chord, &letters_typed);
        }

        // Look: the selected card, large.
        if look {
            if let Some(p) = placed.iter().find(|p| Some(&p.key) == sel.as_ref()) {
                let (_, body, _) = self.orrery_bounds();
                scene.layer(None);
                scene.rect(Rect::new(0.0, 0.0, w, h), crate::app::fade(t.scrim, 0.85));
                let big = body.inset(self.px(32.0));
                self.draw_orrery_card(scene, p, big, 1.0, true, false, chord, "");
            }
        }

        // Masthead: what's typed, how much matches.
        scene.layer(None);
        scene.rect(head, crate::app::fade(t.paper, s1));
        scene.hline(0.0, head.bottom() - self.px(m::STRUCTURE), w, self.px(m::STRUCTURE), crate::app::fade(t.ink, s1));
        let mut x = self.px(18.0);
        let by = head.y + head.h / 2.0 + self.px(4.0);
        self.fonts.draw_icon(scene, icons::PLANET, self.px(16.0), x, head.y + head.h / 2.0 - self.px(8.0), crate::app::fade(t.ink, s1));
        x += self.px(26.0);
        x += self.fonts.draw(scene, Style { color: crate::app::fade(t.ink, s1), ..self.label() }, x, by, "ORRERY") + self.px(20.0);
        let field = Style { color: crate::app::fade(if query.is_empty() { t.dim } else { t.ink }, s1), ..self.ui() };
        let shown = if query.is_empty() { "type to narrow · hold the chord for letters".to_string() } else { query.clone() };
        let tw = self.fonts.draw(scene, field, x, by, &shown);
        if !query.is_empty() {
            self.draw_line_caret(scene, x + tw + self.px(2.0), by, field.px, 1.0, self.last_key);
            let count = format!("{matches} match{}", if matches == 1 { "" } else { "es" });
            self.fonts.draw(scene, Style { color: crate::app::fade(t.dim, s1), ..meta_plain }, x + tw + self.px(16.0), by, &count);
        }
        let right = if chord && !letters_typed.is_empty() { format!("letters: {letters_typed}") } else { "esc back".to_string() };
        let rw = self.fonts.measure(meta_plain, &right);
        self.fonts.draw(scene, Style { color: crate::app::fade(t.dim, s1), ..meta_plain }, w - rw - self.px(18.0), by, &right);

        // Footer: what you can do, icon and word.
        scene.rect(foot, crate::app::fade(t.paper, s1));
        scene.hline(0.0, foot.y, w, self.px(m::STRUCTURE), crate::app::fade(t.ink, s1));
        let fy = foot.y + foot.h / 2.0 + self.px(4.0);
        let mut x = self.px(18.0);
        let key = Style { color: crate::app::fade(t.ink, s1), ..self.label_strong() };
        let word = Style { color: crate::app::fade(t.ink, s1), ..meta_plain };
        let mut acts: Vec<(&str, &str)> = vec![("↵", "go"), ("⇧↵", "bring here"), ("←↑→↓", "move"), ("tab", "next place")];
        if query.is_empty() {
            acts.extend([("space", "look"), ("⇧⌫", "sleep"), ("⌫", "let go")]);
        } else {
            acts.push(("esc", "clear"));
        }
        for (k, v) in acts {
            x += self.fonts.draw(scene, key, x, fy, k) + self.px(6.0);
            x += self.fonts.draw(scene, word, x, fy, v) + self.px(18.0);
        }
        if let Some((s, _)) = note {
            let nw = self.fonts.measure(word, &s);
            self.fonts.draw(scene, word, w - nw - self.px(18.0), fy, &s);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_orrery_card(&mut self, scene: &mut Scene, p: &Placed, r: Rect, alpha: f32, selected: bool, hovered: bool, chord: bool, typed: &str) {
        if alpha <= 0.01 || r.w < 4.0 || r.h < 4.0 {
            return;
        }
        let t = self.theme.clone();
        let f = |c: Color| crate::app::fade(c, alpha);
        let said = Style { color: f(t.ink), ..self.ui() };
        let written = Style { font: self.f.serif, px: self.ui().px * 16.0 / 13.0, color: f(t.ink), tracking: 0.0 };
        let meta = Style { color: f(t.dim), tracking: 0.0, ..self.label() };
        let clip = Some(r);
        let Some(card) = p.card.as_ref() else {
            // A place with nothing open: its letter and what going there does.
            scene.layer(None);
            let mut x = r.x + self.px(6.0);
            let y = r.y + self.px(22.0);
            x += self.draw_letters(scene, &p.letters, x, y, alpha, chord, typed) + self.px(8.0);
            let what = if p.key == CardKey::Restore { "bring it back" } else { "open a shell here" };
            let s = self.ofit(said, what, r.right() - x - self.px(4.0)).into_owned();
            self.fonts.draw(scene, said, x, y, &s);
            if selected {
                scene.rect(Rect::new(r.x - self.px(4.0), r.y, self.px(m::BAND), r.h.min(self.px(32.0))), f(t.ink));
            }
            return;
        };
        let asleep = card.state == State::Asleep;
        let alpha_c = if asleep { alpha * 0.6 } else { alpha };
        let fc = |c: Color| crate::app::fade(c, alpha_c);
        let form = o::form(r.w, self.scale);
        scene.layer(None);
        // Lift on hover: the shadow token, offset.
        if hovered && !selected {
            scene.rect(Rect::new(r.x + self.px(3.0), r.y + self.px(3.0), r.w, r.h), f(t.hot));
        }
        scene.rect(r, f(t.paper));
        let held_look = card.kind == Kind::Held;
        let face = if card.written { written } else { said };
        let icon = match card.kind {
            Kind::Shell | Kind::Held => icons::TERMINAL,
            Kind::Page => icons::GLOBE,
            Kind::Editor => icons::CODE,
            Kind::Other => icons::APP_WINDOW,
        };
        match form {
            Form::Live => {
                let band = (face.px * 1.25 + meta.px * 1.5 + self.px(12.0)).round();
                let body = Rect::new(r.x, r.y, r.w, (r.h - band).max(0.0));
                if let (Some(bind), true) = (card.preview.clone(), card.kind == Kind::Page) {
                    // Cover from the top: the page's head is what's recognised.
                    let ba = body.w / body.h.max(1.0);
                    let uv = if card.aspect > ba { let u = ba / card.aspect; [(1.0 - u) / 2.0, 0.0, (1.0 + u) / 2.0, 1.0] } else { [0.0, 0.0, 1.0, (card.aspect / ba).min(1.0)] };
                    scene.texture_uv_alpha(body, uv, bind, clip, alpha_c);
                    scene.layer(None);
                    // The wash: halfway to paper, so the title can't lose to the page.
                    scene.rect(body, crate::app::fade(t.paper, alpha * 0.5));
                } else if !card.lines.is_empty() {
                    // Glance size on a card; reading size when it's large (look).
                    let px = if r.w > self.px(600.0) { self.ui().px } else { self.label().px };
                    let ls = Style { font: self.f.term, px, color: fc(t.ink), tracking: 0.0 };
                    let lh = (ls.px * 1.4).round();
                    let rows = ((body.h - self.px(8.0)) / lh).floor().max(0.0) as usize;
                    let from = card.lines.len().saturating_sub(rows);
                    scene.layer(Some(body));
                    let mut y = body.y + self.px(6.0) + ls.px;
                    // The terminal face is monospace: one advance says how many
                    // characters fit, with no shaping of every line to find out.
                    let adv = self.fonts.measure(ls, "0").max(1.0);
                    let cols = ((body.w - self.px(12.0)) / adv).floor().max(1.0) as usize;
                    for l in &card.lines[from..] {
                        let l = clip_cols(l, cols);
                        self.fonts.draw(scene, ls, body.x + self.px(8.0), y, &l);
                        y += lh;
                    }
                    scene.layer(None);
                } else {
                    let s = self.px(28.0).min(body.h * 0.5);
                    self.fonts.draw_icon(scene, icon, s, body.x + (body.w - s) / 2.0, body.y + (body.h - s) / 2.0, fc(t.dim));
                }
                let br = Rect::new(r.x, body.bottom(), r.w, band);
                scene.rect(br, crate::app::fade(t.paper, alpha * 0.94));
                scene.hline(r.x, br.y, r.w, self.px(m::HAIRLINE), f(t.hot));
                let mut x = r.x + self.px(8.0);
                let ty = br.y + self.px(6.0) + face.px;
                x += self.draw_letters(scene, &p.letters, x, ty, alpha, chord, typed) + self.px(8.0);
                x += self.draw_state(scene, card.state, x, ty - face.px * 0.75, face.px * 0.85, alpha_c);
                let title = self.ofit(face, &card.title, r.right() - x - self.px(8.0)).into_owned();
                self.fonts.draw(scene, Style { color: fc(t.ink), ..face }, x, ty, &title);
                let line = meta_line(card);
                let line = self.ofit(meta, &line, r.w - self.px(16.0)).into_owned();
                self.fonts.draw(scene, meta, r.x + self.px(8.0), ty + meta.px * 1.5, &line);
            }
            Form::Snippet => {
                let mut x = r.x + self.px(8.0);
                let y = r.y + self.px(8.0) + face.px;
                x += self.draw_letters(scene, &p.letters, x, y, alpha, chord, typed) + self.px(6.0);
                x += self.draw_state(scene, card.state, x, y - face.px * 0.75, face.px * 0.85, alpha_c);
                if card.state == State::None {
                    self.fonts.draw_icon(scene, icon, face.px * 0.9, x, y - face.px * 0.78, fc(t.dim));
                    x += face.px * 0.9 + self.px(6.0);
                }
                let title = self.ofit(face, &card.title, r.right() - x - self.px(6.0)).into_owned();
                self.fonts.draw(scene, Style { color: fc(t.ink), ..face }, x, y, &title);
                if r.h > face.px * 2.6 {
                    let line = self.ofit(meta, meta_line(card), r.w - self.px(16.0)).into_owned();
                    self.fonts.draw(scene, meta, r.x + self.px(8.0), y + meta.px * 1.6, &line);
                }
            }
            Form::Chip => {
                let y = r.y + r.h / 2.0 + self.px(4.0);
                let x = r.x + self.px(4.0);
                let lw = self.draw_letters(scene, &p.letters, x, y, alpha, chord, typed);
                if r.w > lw + self.px(22.0) {
                    self.fonts.draw_icon(scene, icon, self.px(12.0), x + lw + self.px(5.0), y - self.px(10.0), fc(t.dim));
                }
            }
        }
        // Frames: solid open, dashed held (no tab), the 6 px band for where you are.
        if held_look {
            dashed_outline(scene, r, self.px(m::HAIRLINE), self.px(5.0), f(t.ink));
        } else {
            scene.outline(r, self.px(if hovered { m::STRUCTURE } else { m::HAIRLINE }), f(if hovered { t.ink } else { t.hot }));
        }
        if selected {
            scene.outline(r, self.px(m::FLOATING), f(t.ink));
            scene.rect(Rect::new(r.x, r.y, self.px(m::BAND), r.h), f(t.ink));
        } else if card.state == State::Waiting {
            scene.rect(Rect::new(r.x, r.y, self.px(m::BAND), r.h), f(t.ink));
        }
    }

    /// A card's letters as a small reversed chip; quiet until the chord is
    /// held, and the part already typed shown done.
    #[allow(clippy::too_many_arguments)]
    fn draw_letters(&mut self, scene: &mut Scene, letters: &str, x: f32, baseline: f32, alpha: f32, chord: bool, typed: &str) -> f32 {
        let t = self.theme.clone();
        let st = Style { font: self.f.strong, px: self.label().px * 12.0 / 11.0, color: t.paper, tracking: 0.0 };
        let wch = self.fonts.measure(st, "mm");
        let pad = self.px(3.0);
        let r = Rect::new(x, baseline - st.px - self.px(1.0), wch + 2.0 * pad, st.px + self.px(5.0));
        let on = chord && (typed.is_empty() || letters.starts_with(typed));
        let (bg, fg) = if on { (t.ink, t.paper) } else { (t.tint, t.ink) };
        let fg = nus_render::oklch::readable(fg, if on { t.ink } else { t.paper }, 4.5);
        scene.rect(r, crate::app::fade(bg, alpha));
        if !on {
            scene.outline(r, self.px(m::HAIRLINE), crate::app::fade(t.hot, alpha));
        }
        self.fonts.draw(scene, Style { color: crate::app::fade(fg, alpha), ..st }, x + pad, baseline, letters);
        r.w
    }

    /// The state as a shape (and its word lives in the meta line). Returns its width.
    fn draw_state(&mut self, scene: &mut Scene, s: State, x: f32, y: f32, size: f32, alpha: f32) -> f32 {
        let icon = match s {
            State::Running => icons::CIRCLE_DASHED,
            State::Passed => icons::CHECK_CIRCLE,
            State::Failed => icons::X_CIRCLE,
            State::Waiting => icons::BELL,
            State::Playing => icons::SPEAKER,
            State::Asleep => icons::MOON,
            State::None => return 0.0,
        };
        self.fonts.draw_icon(scene, icon, size, x, y, crate::app::fade(self.theme.ink, alpha));
        size + self.px(6.0)
    }
}

/// The first `cols` characters, with an ellipsis when some were cut.
fn clip_cols(s: &str, cols: usize) -> std::borrow::Cow<'_, str> {
    match s.char_indices().nth(cols.saturating_sub(1)) {
        Some((i, _)) if s[i..].chars().count() > 1 => format!("{}…", &s[..i]).into(),
        _ => s.into(),
    }
}

fn self_px(v: f32, scale: f32) -> f32 {
    (v * scale).round()
}

/// The line under a title: where it is, and its state in words.
fn meta_line(c: &Card) -> String {
    let state = match c.state {
        State::Running => "running",
        State::Passed => "passed",
        State::Failed => "failed",
        State::Waiting => "waiting for you",
        State::Playing => "playing",
        State::Asleep => "asleep",
        State::None => "",
    };
    let mut s = c.detail.clone();
    // Long unseen, said: these are what sleeping is for.
    let days = (c.idle / 86_400.0) as u32;
    if days >= 1 && !c.active && c.kind == Kind::Page {
        if !s.is_empty() {
            s.push_str(" · ");
        }
        s.push_str(&format!("unseen {days} day{}", if days == 1 { "" } else { "s" }));
    }
    if c.kind == Kind::Held {
        s = if s.is_empty() { "held · no tab".into() } else { format!("{s} · held · no tab") };
    }
    if !state.is_empty() {
        if !s.is_empty() {
            s.push_str(" · ");
        }
        s.push_str(state);
    }
    s
}

/// A folder as a person writes it: ~ for home.
pub(crate) fn tilde(p: &str) -> String {
    match std::env::var("HOME") {
        Ok(h) if !h.is_empty() && p.starts_with(&h) => format!("~{}", &p[h.len()..]),
        _ => p.to_string(),
    }
}

/// A page's title without its site's name, which the host line already says:
/// "Pairing · nus — GitHub" on github.com reads "Pairing · nus".
fn strip_site(title: &str, host: &str) -> String {
    let site = host.split('.').rev().nth(1).unwrap_or(host).to_lowercase();
    for sep in [" — ", " - ", " | ", " · ", " – "] {
        if let Some((a, b)) = title.rsplit_once(sep) {
            if !a.trim().is_empty() && b.to_lowercase().replace(' ', "").contains(&site) {
                return a.trim().to_string();
            }
        }
    }
    title.to_string()
}

fn dashed_outline(scene: &mut Scene, r: Rect, w: f32, dash: f32, c: Color) {
    let mut x = r.x;
    while x < r.right() {
        let l = dash.min(r.right() - x);
        scene.rect(Rect::new(x, r.y, l, w), c);
        scene.rect(Rect::new(x, r.bottom() - w, l, w), c);
        x += dash * 2.0;
    }
    let mut y = r.y;
    while y < r.bottom() {
        let l = dash.min(r.bottom() - y);
        scene.rect(Rect::new(r.x, y, w, l), c);
        scene.rect(Rect::new(r.right() - w, y, w, l), c);
        y += dash * 2.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn site_names_move_to_the_host_line() {
        assert_eq!(strip_site("Pairing, the paper key · Pull Request #91 · cbassuarez/nus — GitHub", "github.com"), "Pairing, the paper key · Pull Request #91 · cbassuarez/nus");
        assert_eq!(strip_site("Bind groups - wgpu", "docs.rs"), "Bind groups - wgpu");
        assert_eq!(strip_site("Week 6 | CalArts", "calarts.edu"), "Week 6");
        assert_eq!(strip_site("Plain", "example.com"), "Plain");
    }

    #[test]
    fn terminal_lines_clip_by_columns() {
        assert_eq!(clip_cols("abcdef", 4), "abc…");
        assert_eq!(clip_cols("abcd", 4), "abcd");
        assert_eq!(clip_cols("ab", 4), "ab");
        assert_eq!(clip_cols("héllo wörld", 6), "héllo…");
    }

    #[test]
    fn tilde_shortens_home() {
        if let Ok(h) = std::env::var("HOME") {
            assert_eq!(tilde(&format!("{h}/nus")), "~/nus");
        }
        assert_eq!(tilde("/etc"), "/etc");
    }
}
