//! Tabs between windows. Every nus window is an `App` in one process, all
//! drawing through one GPU device (`nus_render::gpu` shares it), so a tab
//! moves whole: its shell keeps running, its page keeps its place. The
//! window it leaves takes it out (`take_tab`), the host hands it over, and
//! the window it joins (`receive_tab`) gives it an id of its own and
//! rebuilds what was drawn with the old window's glyph atlas.
//!
//! Ways in: the palette (`move to …`), pane mode (`w` another window,
//! `n` a new one), and dragging a tab row or a pane out of the window,
//! onto another nus window or anywhere else for a new one there. A pane of
//! a split becomes a tab of its own first. A window's only tab, the
//! quick terminal's tab, a peek and a tab in the floating player stay.
//!
//! Moves between windows are not on the layout history: that history is
//! each window's own. Sending it back is the way back.

use crate::app::{Action, App, Pane, Tab};
use crate::director::Op;

/// Where a tab goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dest {
    /// Another window, by its winit id.
    Window(u64),
    /// A new window, cascaded from this one.
    New,
    /// Wherever this screen point falls: onto a nus window there, else a
    /// new window at the point.
    At(i32, i32),
}

impl App {
    /// Why tab `i` can't leave this window, if it can't.
    fn cannot_send(&self, i: usize) -> Option<&'static str> {
        let t = self.tabs.get(i)?;
        let real = self.tabs.iter().filter(|t| !t.hatch && t.peek.is_none()).count();
        if t.hatch {
            Some("the quick terminal's tab stays with it")
        } else if t.peek.is_some() {
            Some("a peek belongs to its page")
        } else if self.pip.as_ref().is_some_and(|p| p.tab == i) {
            Some("it is in the floating player · bring it back first")
        } else if real < 2 {
            Some("it is this window's only tab")
        } else {
            None
        }
    }

    /// Ask the host to move tab `i` to `dest`.
    pub(crate) fn send_tab(&mut self, i: usize, dest: Dest) -> bool {
        if let Some(why) = self.cannot_send(i) {
            self.notice(nus_render::text::icons::TO_TAB, "Can't Move That Tab", why);
            return false;
        }
        self.send_request = Some((self.tabs[i].id, dest));
        true
    }

    /// The focused pane to `dest`: a pane of a split becomes its own tab
    /// first; a lone pane goes with its tab.
    pub(crate) fn send_focused(&mut self, dest: Dest) {
        let Some(t) = self.tabs.get(self.active) else { return };
        if t.right.is_some() {
            let (tab, right) = (t.id, t.focus_right);
            if !self.direct(Op::ToTab { tab, right }) {
                return;
            }
        }
        self.send_tab(self.active, dest);
    }

    /// The other windows, for the palette and pane mode.
    pub(crate) fn other_windows(&self) -> Vec<crate::windows::Entry> {
        let me = u64::from(self.window.id());
        self.windows.iter().filter(|w| w.id != me).cloned().collect()
    }

    /// Palette rows: the focused pane or tab to each other window, or a new one.
    pub(crate) fn send_rows(&self, hit: &dyn Fn(&str) -> bool) -> Vec<(String, Action)> {
        let mut rows = Vec::new();
        for w in self.other_windows() {
            if hit(&format!("move to window send {}", w.name.to_lowercase())) {
                rows.push((format!("move to window · {} ({} tabs)", w.name, w.tabs), Action::SendTo(Dest::Window(w.id))));
            }
        }
        if hit("move to a new window send") {
            rows.push(("move to a new window · this tab (or pane) on its own".into(), Action::SendTo(Dest::New)));
        }
        rows
    }

    /// Pane mode's `w`: straight there when there is one other window; the
    /// palette's list when there are more; a new one when there are none.
    pub(crate) fn send_pick(&mut self) {
        let others = self.other_windows();
        match others.as_slice() {
            [] => self.send_focused(Dest::New),
            [one] => self.send_focused(Dest::Window(one.id)),
            _ => {
                self.pane_mode = false;
                self.open_palette(crate::app::PaletteMode::Go);
                if let Some((_, input)) = self.palette.as_mut() {
                    *input = "move to window ".into();
                }
            }
        }
    }

    /// Where a drag was let go, when that was outside this window: the
    /// screen point, for the host to find a window there; or just "a new
    /// window" when the pointer can't be read. None while inside.
    pub(crate) fn dropped_outside(&self) -> Option<Dest> {
        let (w, h) = (self.target.size.0 as f32, self.target.size.1 as f32);
        let (x, y) = crate::pointer::in_window(&self.window).unwrap_or(self.mouse);
        if x >= 0.0 && y >= 0.0 && x < w && y < h {
            return None;
        }
        let Ok(p) = self.window.inner_position() else { return Some(Dest::New) };
        Some(Dest::At(p.x + x as i32, p.y + y as i32))
    }

    /// Take tab `id` out of this window, for another. None when it can't go.
    pub(crate) fn take_tab(&mut self, id: u64) -> Option<Tab> {
        let i = self.tabs.iter().position(|t| t.id == id)?;
        if self.cannot_send(i).is_some() {
            return None;
        }
        if self.timeline.as_ref().is_some_and(|t| t.tab_id == id) {
            self.close_timeline();
        }
        // Its stack stays here, at the top level.
        for t in self.tabs.iter_mut().filter(|t| t.parent == Some(id)) {
            t.parent = None;
        }
        let mut tab = self.tabs.remove(i);
        self.tile_forget(id);
        self.pins.live.retain(|_, live| *live != id);
        self.tab_removed(i);
        tab.parent = None;
        tab.pinned = false;
        let next = self.mru.first().copied().unwrap_or(0).min(self.tabs.len().saturating_sub(1));
        self.activate(next);
        self.play_event("tab.close");
        self.layout();
        self.save_session();
        self.dirty = true;
        Some(tab)
    }

    /// A tab from another window: an id of this window's, text redrawn
    /// with this window's glyphs, at the end, in front.
    pub(crate) fn receive_tab(&mut self, mut tab: Tab) {
        tab.id = self.next_id;
        self.next_id += 1;
        let px = self.terminal_px();
        let typo = self.behavior.typography.clone();
        for pane in std::iter::once(&mut tab.left).chain(tab.right.as_mut()) {
            if let Pane::Term(t) = pane {
                // The rows were shaped against the other window's atlas.
                t.grid.set_font(&self.fonts, self.f.term, px * t.zoom as f32 / 100.0);
                t.grid.set_spacing(&self.fonts, typo.terminal_line, typo.terminal_spacing * self.scale * t.zoom as f32 / 100.0);
                t.term.grid_mut().damage_all();
                t.view_key = None;
            }
        }
        tab.look = self.look_with(&tab.left, None, tab.shell_slot);
        Self::fit_palette(&self.theme, &mut tab);
        self.tabs.push(tab);
        let i = self.tabs.len() - 1;
        self.activate(i);
        self.play_event("tab.switch");
        self.layout();
        self.save_session();
        self.register_window();
        self.dirty = true;
    }
}
