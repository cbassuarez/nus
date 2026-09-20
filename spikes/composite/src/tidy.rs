//! Tab tidy: suggestions, never actions. Three pieces. **Dedupe** — a
//! page already open in another tab gets a band on the newer one: switch
//! there, or keep both (the one automatic thing). **Grouping** — a
//! `group(tab)` rule names a group for a tab (host by default); nothing
//! moves on its own. **TIDY** — a sheet (the palette, or on a timer)
//! proposes groups with MAKE STACK · ARCHIVE · SKIP per group, drawn like
//! the ports board; each is a tap.

use std::collections::HashMap;
use std::time::Instant;

use nus_render::text::Style;
use nus_render::{Rect, Scene};

use crate::app::{fade, App, IconMotion, Pane};
use crate::settings::TidyEvery;
use nus_render::theme::metric as m;

/// A proposed group: its name, the tabs in it, and what the user picked.
#[derive(Clone, Debug)]
pub struct Group {
    pub name: String,
    pub tabs: Vec<usize>,
    pub done: Option<GroupAct>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupAct {
    Stack,
    Archive,
    Skip,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TidyHit {
    Act(usize, GroupAct),
    Close,
}

pub struct Tidy {
    pub open: bool,
    pub groups: Vec<Group>,
    pub hits: Vec<(Rect, TidyHit)>,
    pub rect: Rect,
    pub rise: crate::anim::Anim,
    pub last_run: Option<Instant>,
    pub scroll: f32,
}

impl Default for Tidy {
    fn default() -> Self {
        Tidy { open: false, groups: Vec::new(), hits: Vec::new(), rect: Rect::new(0.0, 0.0, 1.0, 1.0), rise: crate::anim::Anim::at(0.0), last_run: None, scroll: 0.0 }
    }
}

/// The host of a URL, without www.
pub fn host_of(url: &str) -> String {
    url.split("//").nth(1).unwrap_or(url).split('/').next().unwrap_or("").trim_start_matches("www.").to_string()
}

impl App {
    /// The group a tab belongs to: the rule's answer, else its host (pages)
    /// or its project folder (shells); None for the rest.
    pub(crate) fn group_of(&self, i: usize) -> Option<String> {
        let t = self.tabs.get(i)?;
        if t.peek.is_some() || t.hatch {
            return None;
        }
        let (kind, title, url, cwd) = match &t.left {
            Pane::Web(w) => {
                let s = w.tab.shared.borrow();
                ("page", s.title.clone(), s.url.clone(), String::new())
            }
            Pane::Term(term) => ("shell", term.title.clone(), String::new(), term.term.cwd.clone().unwrap_or_default()),
            Pane::Editor(e) => ("editor", e.title(), String::new(), e.buf().and_then(|b| b.path.as_ref()).and_then(|p| p.parent()).map(|p| p.display().to_string()).unwrap_or_default()),
            _ => return None,
        };
        if let Some(g) = self.rules.group(kind, &title, &url, &cwd, t.parent.is_some()) {
            return if g.is_empty() { None } else { Some(g) };
        }
        match kind {
            "page" => {
                let h = host_of(&url);
                (!h.is_empty() && h != "localhost" && !h.starts_with("localhost:")).then_some(h)
            }
            _ => {
                let p = std::path::Path::new(&cwd);
                p.file_name().map(|n| n.to_string_lossy().to_string()).filter(|n| !n.is_empty())
            }
        }
    }

    /// Propose groups: tabs sharing a name, two or more, not already in one
    /// stack together, not pinned.
    pub(crate) fn propose_groups(&self) -> Vec<Group> {
        let mut by: HashMap<String, Vec<usize>> = HashMap::new();
        for i in 0..self.tabs.len() {
            if self.tabs[i].pinned {
                continue;
            }
            if let Some(g) = self.group_of(i) {
                by.entry(g).or_default().push(i);
            }
        }
        let mut out: Vec<Group> = by
            .into_iter()
            .filter(|(_, tabs)| tabs.len() >= 2)
            .filter(|(_, tabs)| {
                // Already one stack: every tab shares a root.
                let roots: std::collections::HashSet<usize> = tabs.iter().map(|&i| self.root_of(i)).collect();
                roots.len() > 1
            })
            .map(|(name, tabs)| Group { name, tabs, done: None })
            .collect();
        out.sort_by(|a, b| b.tabs.len().cmp(&a.tabs.len()).then(a.name.cmp(&b.name)));
        out
    }

    /// The top-level tab a tab hangs from (itself when it's a root).
    pub(crate) fn root_of(&self, i: usize) -> usize {
        let mut cur = i;
        for _ in 0..64 {
            match self.tabs[cur].parent.and_then(|pid| self.tabs.iter().position(|t| t.id == pid)) {
                Some(p) if p != cur => cur = p,
                _ => break,
            }
        }
        cur
    }

    pub(crate) fn open_tidy(&mut self) {
        self.tidy.groups = self.propose_groups();
        self.tidy.open = true;
        self.tidy.scroll = 0.0;
        self.tidy.last_run = Some(crate::clock::now());
        let d = self.motion.dur(crate::anim::base::PALETTE);
        self.tidy.rise.replay(0.0, 1.0, d);
        self.rules.on_tidy(&self.tidy.groups);
        self.dirty = true;
    }

    pub(crate) fn close_tidy(&mut self) {
        self.tidy.open = false;
        self.dirty = true;
    }

    /// The timer: suggest on the hour or the day, only when there's
    /// something to suggest, only when the window has focus.
    pub(crate) fn tidy_tick(&mut self) {
        let every = match self.behavior.tidy_every {
            TidyEvery::Off => return,
            TidyEvery::Hourly => 3600,
            TidyEvery::Daily => 86400,
        };
        let due = self.tidy.last_run.map(|t| crate::clock::since(t).as_secs() >= every).unwrap_or(crate::clock::since(self.started).as_secs() >= 120);
        if !due || self.tidy.open || self.palette.is_some() || self.board.open || !self.window.has_focus() {
            return;
        }
        self.tidy.last_run = Some(crate::clock::now());
        if !self.propose_groups().is_empty() {
            self.open_tidy();
        }
    }

    /// Act on a group: stack its tabs under the first, archive them, or skip.
    fn tidy_act(&mut self, gi: usize, act: GroupAct) {
        let Some(g) = self.tidy.groups.get(gi).cloned() else { return };
        match act {
            GroupAct::Stack => {
                // Tabs by id, so indices moving under us don't matter.
                let ids: Vec<u64> = g.tabs.iter().filter_map(|&i| self.tabs.get(i).map(|t| t.id)).collect();
                let Some(&root_id) = ids.first() else { return };
                let Some(root) = self.tabs.iter().position(|t| t.id == root_id) else { return };
                self.tabs[root].parent = None;
                for id in ids.iter().skip(1) {
                    if let Some(i) = self.tabs.iter().position(|t| t.id == *id) {
                        self.tabs[i].parent = Some(root_id);
                    }
                }
                if let Some(t) = self.tabs.get_mut(root) {
                    if t.name.is_none() {
                        t.name = Some(g.name.clone());
                    }
                }
                self.reorder_stacks();
                self.play_event("toggle");
            }
            GroupAct::Archive => {
                let ids: Vec<u64> = g.tabs.iter().filter_map(|&i| self.tabs.get(i).map(|t| t.id)).collect();
                for id in ids {
                    if let Some(i) = self.tabs.iter().position(|t| t.id == id) {
                        // Into RECENTLY CLOSED, reopenable; never a hard delete.
                        self.selected.clear();
                        self.activate(i);
                        self.close_tabs(true);
                    }
                }
                self.play_event("tab.close");
            }
            GroupAct::Skip => {}
        }
        if let Some(g) = self.tidy.groups.get_mut(gi) {
            g.done = Some(act);
        }
        // Everything answered: the sheet goes.
        if self.tidy.groups.iter().all(|g| g.done.is_some()) {
            self.close_tidy();
        }
        self.save_session();
        self.dirty = true;
    }

    /// Children sit right after their root in the list; a stack made from
    /// scattered tabs is gathered.
    fn reorder_stacks(&mut self) {
        let mut order: Vec<usize> = Vec::new();
        let n = self.tabs.len();
        let mut seen = vec![false; n];
        for i in 0..n {
            if seen[i] || self.tabs[i].parent.is_some() {
                continue;
            }
            order.push(i);
            seen[i] = true;
            let id = self.tabs[i].id;
            for j in 0..n {
                if !seen[j] && self.tabs[j].parent == Some(id) {
                    order.push(j);
                    seen[j] = true;
                }
            }
        }
        for i in 0..n {
            if !seen[i] {
                order.push(i);
            }
        }
        if order.iter().enumerate().all(|(a, &b)| a == b) {
            return;
        }
        let active_id = self.tabs[self.active].id;
        let tabs = std::mem::take(&mut self.tabs);
        let mut slots: Vec<Option<crate::app::Tab>> = tabs.into_iter().map(Some).collect();
        let new: Vec<crate::app::Tab> = order.iter().filter_map(|&i| slots[i].take()).collect();
        self.tabs = new;
        self.active = self.tabs.iter().position(|t| t.id == active_id).unwrap_or(0);
        self.mru = vec![self.active];
    }

    pub(crate) fn tidy_key(&mut self, ev: &crate::app::KeyIn) -> bool {
        use winit::keyboard::{Key as WKey, NamedKey};
        if !self.tidy.open {
            return false;
        }
        if ev.state != winit::event::ElementState::Pressed {
            return true;
        }
        if matches!(ev.logical_key, WKey::Named(NamedKey::Escape)) {
            self.close_tidy();
        }
        true
    }

    pub(crate) fn tidy_mouse(&mut self, button: winit::event::MouseButton, state: winit::event::ElementState, x: f32, y: f32) -> bool {
        if !self.tidy.open {
            return false;
        }
        if state != winit::event::ElementState::Pressed || button != winit::event::MouseButton::Left {
            return true;
        }
        let hit = self.tidy.hits.iter().find(|(r, _)| r.contains(x, y)).map(|(_, h)| *h);
        match hit {
            Some(TidyHit::Act(gi, a)) => self.tidy_act(gi, a),
            Some(TidyHit::Close) => self.close_tidy(),
            None => {
                if !self.tidy.rect.contains(x, y) {
                    self.close_tidy();
                }
            }
        }
        true
    }

    /// The sheet: groups as ruled rows, three icon chips each.
    pub(crate) fn draw_tidy(&mut self, scene: &mut Scene, w: f32, h: f32) {
        if !self.tidy.open {
            return;
        }
        let t = self.theme.clone();
        let (ink, paper) = (t.ink, t.paper);
        let label = self.label();
        let strong = self.label_strong();
        let dim = Style { color: t.dim, ..label };
        let ui = self.ui();
        let signal = self.surface.signal;
        let (mx, my) = self.mouse;
        let rise = self.tidy.rise.value();
        if self.tidy.rise.active() {
            self.dirty = true;
        }
        scene.layer(None);
        scene.rect(Rect::new(0.0, 0.0, w, h), nus_render::theme::Theme::with_alpha(t.scrim, t.scrim[3] * rise));
        let groups = self.tidy.groups.clone();
        let pad = self.px(22.0);
        let row_h = self.px(34.0);
        let bw = self.px(m::PALETTE).min(w - 2.0 * self.px(16.0));
        let body = groups.len() as f32 * row_h + groups.iter().map(|g| g.tabs.len().min(4) as f32 * self.px(18.0)).sum::<f32>();
        let bh = (self.px(96.0) + body).min(h * 0.8);
        let bx = ((w - bw) / 2.0).round();
        let by = ((h - bh) / 2.0 * 0.8).round() + (1.0 - rise) * self.px(10.0);
        let r = Rect::new(bx, by, bw, bh);
        scene.rect(Rect::new(r.x + self.px(8.0), r.y + self.px(8.0), r.w, r.h), ink);
        scene.rect(r, paper);
        scene.outline(r, self.px(m::STRUCTURE), ink);
        self.tidy.rect = r;
        self.tidy.hits.clear();
        // Masthead.
        let wm = Style { font: self.f.wordmark, px: self.px(30.0), color: ink, tracking: 0.0 };
        let mut y = r.y + self.px(18.0);
        let tw = self.fonts.draw(scene, wm, r.x + pad, y + self.px(26.0), "tidy");
        let n = groups.iter().filter(|g| g.done.is_none()).count();
        self.fonts.draw(scene, dim, r.x + pad + tw + self.px(16.0), y + self.px(24.0), &format!("{n} GROUP{} · NOTHING MOVES UNTIL YOU SAY", if n == 1 { "" } else { "S" }));
        let close = "ESC";
        let cw = self.fonts.measure(label, close);
        let cr = Rect::new(r.right() - pad - cw - self.px(6.0), y + self.px(8.0), cw + self.px(12.0), self.px(24.0));
        self.fonts.draw(scene, Style { color: if cr.contains(mx, my) { signal } else { t.dim }, ..label }, cr.x + self.px(6.0), y + self.px(24.0), close);
        self.tidy.hits.push((cr, TidyHit::Close));
        y += self.px(44.0);
        scene.hline(r.x + pad, y, r.w - 2.0 * pad, self.px(m::STRUCTURE), ink);
        y += self.px(m::STRUCTURE) + self.px(4.0);
        if groups.is_empty() {
            self.fonts.draw(scene, dim, r.x + pad, y + self.px(26.0), "nothing to group · tabs that share a host or a project folder land here");
        }
        scene.layer(Some(Rect::new(r.x, y, r.w, r.bottom() - y - self.px(8.0))));
        for (gi, g) in groups.iter().enumerate() {
            let base = y + self.px(22.0);
            let done = g.done;
            let color = if done.is_some() { t.dim } else { ink };
            // The group's name and its count.
            let name = g.name.to_uppercase();
            let nw = self.fonts.draw(scene, Style { color, ..strong }, r.x + pad, base, &name);
            self.fonts.draw(scene, dim, r.x + pad + nw + self.px(8.0), base, &format!("{}", g.tabs.len()));
            if let Some(d) = done {
                let word = match d {
                    GroupAct::Stack => "STACKED",
                    GroupAct::Archive => "ARCHIVED",
                    GroupAct::Skip => "SKIPPED",
                };
                let ww = self.fonts.measure(label, word);
                self.fonts.draw(scene, dim, r.right() - pad - ww, base, word);
            } else {
                // Chips: stack · archive · skip, as icons.
                let isz = self.px(14.0);
                let mut cx = r.right() - pad - isz;
                for (k, icon, words, act) in [
                    (2usize, nus_render::text::icons::CLOSE, "skip", GroupAct::Skip),
                    (1, nus_render::text::icons::HISTORY, "archive these", GroupAct::Archive),
                    (0, nus_render::text::icons::STACK, "make a stack", GroupAct::Stack),
                ] {
                    let hit = Rect::new(cx - self.px(6.0), y + self.px(4.0), isz + self.px(12.0), row_h - self.px(8.0));
                    let hot = hit.contains(mx, my);
                    self.icon_button(scene, icon, isz, cx, y + (row_h - isz) / 2.0, if hot && act == GroupAct::Stack { signal } else { ink }, hit, crate::app::hover_key("tidy", gi * 10 + k), IconMotion::Pop);
                    if hot {
                        self.tip_words(hit, words);
                    }
                    self.tidy.hits.push((hit, TidyHit::Act(gi, act)));
                    cx -= isz + self.px(18.0);
                }
            }
            y += row_h;
            // The tabs, dim, up to four.
            for &ti in g.tabs.iter().take(4) {
                if let Some(tab) = self.tabs.get(ti) {
                    let title = self.fit(dim, &format!("{}  {}", self.tab_label(ti), tab.title()), r.w - 2.0 * pad - self.px(20.0));
                    self.fonts.draw(scene, Style { color: fade(t.dim, if done.is_some() { 0.6 } else { 1.0 }), ..ui }, r.x + pad + self.px(14.0), y + self.px(13.0), &title);
                }
                y += self.px(18.0);
            }
            if g.tabs.len() > 4 {
                self.fonts.draw(scene, dim, r.x + pad + self.px(14.0), y + self.px(13.0), &format!("+ {} MORE", g.tabs.len() - 4));
                y += self.px(18.0);
            }
            y += self.px(4.0);
            scene.hline(r.x + pad, y, r.w - 2.0 * pad, self.px(m::HAIRLINE), fade(ink, 0.35));
            y += self.px(4.0);
        }
        scene.layer(None);
    }

    /// Dedupe: the page under `i` is already open in an earlier tab. Returns
    /// that tab's index.
    pub(crate) fn duplicate_of(&self, i: usize) -> Option<usize> {
        if !self.behavior.dedupe {
            return None;
        }
        let url = match &self.tabs.get(i)?.left {
            Pane::Web(w) => w.tab.shared.borrow().url.clone(),
            _ => return None,
        };
        if url.is_empty() || url == "about:blank" {
            return None;
        }
        let key = url.trim_end_matches('/');
        self.tabs.iter().enumerate().find(|(j, t)| {
            *j != i && t.peek.is_none() && t.id < self.tabs[i].id && matches!(&t.left, Pane::Web(w) if w.tab.shared.borrow().url.trim_end_matches('/') == key)
        }).map(|(j, _)| j)
    }
}
