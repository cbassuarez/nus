//! The pane director. Every change to how panes are laid out is one `Op`
//! through `App::direct`, whatever asked for it: the corner cluster, a
//! drag, a key, the palette, a rule. Applying an op returns the op that
//! takes it back, and that goes on the history, so a layout is never lost
//! to a slip: Ctrl+Alt+Z undoes, Ctrl+Alt+Shift+Z redoes, and the palette
//! has both ("pane undo", "pane redo").
//!
//! Ops name tabs by id, never by index; indexes shift as tabs come and go
//! and ids don't. An op whose tabs have gone since does nothing and says
//! so, and the history moves past it.
//!
//! Kill is the one op with no way back: a stopped process doesn't restart
//! where it was. Everything else (swap, solo, to a tab, join, widths, the
//! tiling) moves live panes around and can move them back.

use crate::app::{App, Pane};
use crate::tiles::Tiling;

/// How far back the history goes.
const DEPTH: usize = 100;

#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    /// A browser pane beside the tab's only pane, at the home search.
    Split { tab: u64 },
    /// A shell beside the tab's only pane: the same profile, in the same
    /// folder when that pane is a shell. How a terminal multiplexes.
    SplitShell { tab: u64 },
    /// The tab's two panes change sides.
    Swap { tab: u64 },
    /// One pane alone (`solo`), the other waiting off screen; or both.
    Solo { tab: u64, solo: bool, right: bool },
    /// Close one pane of a split, or the tab when the pane is alone. What
    /// runs there stops.
    Kill { tab: u64, right: bool },
    /// A pane of a split becomes a tab of its own, right after its old one.
    ToTab { tab: u64, right: bool },
    /// A pane joins a tab that has one pane, on the left or the right.
    /// From a split it leaves the other pane behind; a lone pane brings
    /// its tab to an end (the pane lives on; nothing is closed).
    Join { from: u64, right: bool, to: u64, side_right: bool },
    /// The right pane's width, logical px; None is the default.
    SplitWidth { tab: u64, w: Option<f32> },
    /// The tiling, whole: which tabs, in what order, where its rules sit.
    Tiling(Option<Tiling>),
    /// Several ops as one step (a drop that makes a tab, then tiles it).
    Batch(Vec<Op>),
}

impl Op {
    /// A few words for the notice after an undo or redo.
    fn words(&self) -> &'static str {
        match self {
            Op::Split { .. } | Op::SplitShell { .. } => "split",
            Op::Swap { .. } => "swap",
            Op::Solo { .. } => "solo",
            Op::Kill { .. } => "close",
            Op::ToTab { .. } => "to a tab",
            Op::Join { .. } => "move",
            Op::SplitWidth { .. } => "resize",
            Op::Tiling(_) => "tiling",
            Op::Batch(ops) => ops.first().map(Op::words).unwrap_or("layout"),
        }
    }
}

#[derive(Default)]
pub struct Director {
    undo: Vec<Op>,
    redo: Vec<Op>,
    /// While a group is open, inverses gather here and land as one step.
    group: Option<Vec<Op>>,
}

impl Director {
    fn push(stack: &mut Vec<Op>, op: Op) {
        if stack.len() == DEPTH {
            stack.remove(0);
        }
        stack.push(op);
    }

    /// Something was done by hand: remember how to take it back, and the
    /// redo trail no longer leads anywhere.
    pub fn record(&mut self, inverse: Op) {
        if let Some(g) = self.group.as_mut() {
            g.push(inverse);
            return;
        }
        Self::push(&mut self.undo, inverse);
        self.redo.clear();
    }

    /// Everything done until `end_group` undoes as one step.
    pub fn begin_group(&mut self) {
        self.group.get_or_insert_with(Vec::new);
    }

    pub fn end_group(&mut self) {
        let Some(mut g) = self.group.take() else { return };
        match g.len() {
            0 => {}
            1 => self.record(g.remove(0)),
            _ => {
                // Undone last-first.
                g.reverse();
                self.record(Op::Batch(g));
            }
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}

impl App {
    /// Do `op` and remember how to take it back. False when it couldn't
    /// be done (its tabs are gone, or there was nothing to do).
    pub(crate) fn direct(&mut self, op: Op) -> bool {
        match self.apply_op(op) {
            Some(inverse) => {
                if let Some(inverse) = inverse {
                    self.director.record(inverse);
                }
                true
            }
            None => false,
        }
    }

    /// Something already happened (a divider was dragged): remember how to
    /// take it back without doing anything now.
    pub(crate) fn directed(&mut self, inverse: Op) {
        self.director.record(inverse);
    }

    /// A divider was let go: if it moved, the drag goes on the history.
    pub(crate) fn divider_dragged(&mut self) {
        let Some(from) = self.drag_from.take() else { return };
        let moved = match &from {
            Op::SplitWidth { tab, w } => self.tab_at(*tab).is_some_and(|i| self.tabs[i].split_w != *w),
            Op::Tiling(t) => &self.tiling != t,
            _ => false,
        };
        if moved {
            self.directed(from);
        }
    }

    /// The palette's pane rows for the active tab: what can be done here.
    pub(crate) fn pane_rows(&self, hit: &dyn Fn(&str) -> bool) -> Vec<(String, crate::app::Action)> {
        use crate::app::Action;
        let mut rows = Vec::new();
        if self.director.can_undo() && hit("pane undo layout back") {
            rows.push(("pane undo · take the last layout change back (ctrl+alt+z)".into(), Action::PaneUndo));
        }
        if self.director.can_redo() && hit("pane redo layout again") {
            rows.push(("pane redo · the layout change again (ctrl+alt+shift+z)".into(), Action::PaneRedo));
        }
        let Some(t) = self.tabs.get(self.active) else { return rows };
        let tab = t.id;
        if t.right.is_none() {
            if !crate::private::enabled() && hit("pane split shell terminal beside") {
                rows.push(("pane split shell · a shell beside this pane, in the same folder".into(), Action::Pane(Op::SplitShell { tab })));
            }
            if hit("pane split browser beside") {
                rows.push(("pane split · a browser beside this pane".into(), Action::Pane(Op::Split { tab })));
            }
            return rows;
        }
        let side = |r: bool| if r { "right" } else { "left" };
        if hit("pane swap sides") {
            rows.push(("pane swap · the two sides change places".into(), Action::Pane(Op::Swap { tab })));
        }
        for right in [false, true] {
            let s = side(right);
            if hit(&format!("pane solo {s} alone")) {
                rows.push((format!("pane solo · the {s} pane alone"), Action::Pane(Op::Solo { tab, solo: true, right })));
            }
            if hit(&format!("pane to a tab {s} own tab detach")) {
                rows.push((format!("pane to a tab · the {s} pane on its own"), Action::Pane(Op::ToTab { tab, right })));
            }
            if hit(&format!("pane close kill {s}")) {
                rows.push((format!("pane close · the {s} pane (what runs there stops)"), Action::Pane(Op::Kill { tab, right })));
            }
        }
        if t.solo && hit("pane both unsolo") {
            rows.push(("pane both · show both panes again".into(), Action::Pane(Op::Solo { tab, solo: false, right: t.focus_right })));
        }
        rows
    }

    pub(crate) fn pane_undo(&mut self) {
        self.pane_history(true);
    }

    pub(crate) fn pane_redo(&mut self) {
        self.pane_history(false);
    }

    /// One step back (or forward). An entry whose tabs have gone is
    /// skipped; the next one is tried.
    fn pane_history(&mut self, back: bool) {
        loop {
            let op = if back { self.director.undo.pop() } else { self.director.redo.pop() };
            let Some(op) = op else {
                self.notice(nus_render::text::icons::UNDO, if back { "Nothing To Undo" } else { "Nothing To Redo" }, "in the layout");
                return;
            };
            let words = op.words();
            // Undoing a split closes the pane it opened; redoing opens one again.
            let resplit = match op {
                Op::Kill { tab, right: true } if back => {
                    let shell = self.tab_at(tab).and_then(|i| self.tabs[i].right.as_ref()).is_some_and(|p| matches!(p, Pane::Term(_)));
                    Some(if shell { Op::SplitShell { tab } } else { Op::Split { tab } })
                }
                _ => None,
            };
            if let Some(inverse) = self.apply_op(op) {
                if let Some(inverse) = inverse.or(resplit) {
                    let other = if back { &mut self.director.redo } else { &mut self.director.undo };
                    Director::push(other, inverse);
                }
                self.notice(nus_render::text::icons::UNDO, if back { "Undid" } else { "Redid" }, words);
                return;
            }
        }
    }

    fn tab_at(&self, id: u64) -> Option<usize> {
        self.tabs.iter().position(|t| t.id == id)
    }

    /// Apply an op. None: it couldn't be done. Some(None): done, and there
    /// is no way back. Some(Some(inverse)): done, and this undoes it.
    fn apply_op(&mut self, op: Op) -> Option<Option<Op>> {
        let out = match op {
            Op::SplitShell { tab } => {
                let i = self.tab_at(tab)?;
                if self.tabs[i].right.is_some() {
                    return None;
                }
                let (profile, cwd) = match &self.tabs[i].left {
                    Pane::Term(t) => (t.profile, t.cwd.clone()),
                    _ => (self.behavior.default_profile, None),
                };
                let pane = match self.new_term_pane_at(true, profile, cwd) {
                    Ok(p) => p,
                    Err(e) => {
                        self.notice_problem("Could Not Split", e.to_string());
                        return None;
                    }
                };
                let t = &mut self.tabs[i];
                t.right = Some(Pane::Term(pane));
                t.focus_right = true;
                Some(Op::Kill { tab, right: true })
            }
            Op::Split { tab } => {
                let i = self.tab_at(tab)?;
                if self.tabs[i].right.is_some() {
                    return None;
                }
                let w = self.new_web_pane(&self.behavior.prompt.search_home())?;
                let t = &mut self.tabs[i];
                t.right = Some(Pane::Web(w));
                t.focus_right = true;
                Some(Op::Kill { tab, right: true })
            }
            Op::Swap { tab } => {
                let i = self.tab_at(tab)?;
                let t = &mut self.tabs[i];
                let r = t.right.take()?;
                let l = std::mem::replace(&mut t.left, r);
                t.right = Some(l);
                t.focus_right = !t.focus_right;
                self.play_event("toggle");
                Some(Op::Swap { tab })
            }
            Op::Solo { tab, solo, right } => {
                let i = self.tab_at(tab)?;
                let t = &mut self.tabs[i];
                t.right.as_ref()?;
                let was = Op::Solo { tab, solo: t.solo, right: t.focus_right };
                t.solo = solo;
                t.focus_right = right;
                self.play_event("toggle");
                Some(was)
            }
            Op::Kill { tab, right } => {
                let i = self.tab_at(tab)?;
                if self.tabs[i].right.is_none() {
                    // A lone pane: the tab closes, the way closing tabs does
                    // (asking first when something is running).
                    self.activate(i);
                    self.close_tabs(false);
                    return Some(None);
                }
                let t = &mut self.tabs[i];
                if right {
                    t.right = None;
                } else if let Some(r) = t.right.take() {
                    t.left = r;
                }
                t.focus_right = false;
                t.solo = false;
                self.play_event("tab.close");
                None
            }
            Op::ToTab { tab, right } => {
                let i = self.tab_at(tab)?;
                let pane = self.take_pane(i, right)?;
                let mut new = self.make_tab(pane, None);
                new.parent = self.tabs[i].parent;
                let id = new.id;
                let at = self.subtree(i).last().copied().unwrap_or(i) + 1;
                self.insert_tab_at(at, new);
                self.activate(at);
                Some(Op::Join { from: id, right: false, to: tab, side_right: right })
            }
            Op::Join { from, right, to, side_right } => {
                let (fi, ti) = (self.tab_at(from)?, self.tab_at(to)?);
                if fi == ti || self.tabs[ti].right.is_some() {
                    return None;
                }
                let lone = self.tabs[fi].right.is_none();
                // A lone tab with a stack under it keeps its place: its
                // pages would lose their parent.
                if lone && self.tabs.iter().any(|t| t.parent == Some(from)) {
                    self.notice(nus_render::text::icons::STACK, "Move Its Pages First", "that tab has pages under it");
                    return None;
                }
                let pane = if lone {
                    let tab = self.tabs.remove(fi);
                    self.tile_forget(tab.id);
                    self.tab_removed(fi);
                    tab.left
                } else {
                    self.take_pane(fi, right)?
                };
                let ti = self.tab_at(to)?;
                let t = &mut self.tabs[ti];
                if side_right {
                    t.right = Some(pane);
                } else {
                    let old = std::mem::replace(&mut t.left, pane);
                    t.right = Some(old);
                }
                t.focus_right = side_right;
                t.solo = false;
                self.play_event("tab.switch");
                self.activate(ti);
                Some(if lone {
                    Op::ToTab { tab: to, right: side_right }
                } else {
                    Op::Join { from: to, right: side_right, to: from, side_right: right }
                })
            }
            Op::SplitWidth { tab, w } => {
                let i = self.tab_at(tab)?;
                let was = std::mem::replace(&mut self.tabs[i].split_w, w);
                if was == w {
                    return None;
                }
                self.resize_due = Some(crate::clock::now() + std::time::Duration::from_millis(60));
                Some(Op::SplitWidth { tab, w: was })
            }
            Op::Batch(ops) => {
                let mut back = Vec::new();
                for op in ops {
                    match self.apply_op(op) {
                        Some(Some(inv)) => back.push(inv),
                        Some(None) => {}
                        None => break,
                    }
                }
                if back.is_empty() {
                    return None;
                }
                back.reverse();
                Some(Op::Batch(back))
            }
            Op::Tiling(t) => {
                if self.tiling == t {
                    return None;
                }
                let shown = self.tiling_shown();
                let was = std::mem::replace(&mut self.tiling, t);
                // The tiling was on screen and the tab in front has left it
                // (an undone drop): the tiling stays on screen, fronted by
                // the first of its tabs, rather than the lone tab.
                if shown && !self.tiling_shown() {
                    let first = self.tiling.as_ref().and_then(|t| t.ids().into_iter().find_map(|id| self.tab_at(id)));
                    if let Some(i) = first {
                        self.activate(i);
                    }
                }
                for i in self.tiled() {
                    self.wake_tab(i);
                }
                self.play_event("tab.switch");
                Some(Op::Tiling(was))
            }
        };
        self.layout();
        self.save_session();
        self.dirty = true;
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_is_bounded_and_redo_clears_on_new_work() {
        let mut d = Director::default();
        for k in 0..(DEPTH + 10) {
            d.record(Op::Swap { tab: k as u64 });
        }
        assert_eq!(d.undo.len(), DEPTH);
        assert_eq!(d.undo[0], Op::Swap { tab: 10 });
        d.redo.push(Op::Swap { tab: 1 });
        d.record(Op::Swap { tab: 2 });
        assert!(!d.can_redo());
    }

    #[test]
    fn a_group_undoes_as_one_step_last_first() {
        let mut d = Director::default();
        d.begin_group();
        d.record(Op::Swap { tab: 1 });
        d.record(Op::Tiling(None));
        assert!(!d.can_undo(), "nothing lands until the group closes");
        d.end_group();
        assert_eq!(d.undo, vec![Op::Batch(vec![Op::Tiling(None), Op::Swap { tab: 1 }])]);
        d.begin_group();
        d.end_group();
        assert_eq!(d.undo.len(), 1, "an empty group records nothing");
    }
}
