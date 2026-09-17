//! Peek: Alt+click a link and it opens in a floating page over the tab,
//! Arc style, without leaving. Esc (or a click outside) closes it;
//! Ctrl+Enter keeps it, as a page in the tab's stack. A peek is a tab
//! that the sidebar doesn't list, so every pane path already works.

use crate::app::{App, Pane};
use nus_render::theme::metric as m;
use nus_render::{Rect, Scene, Style};

impl App {
    /// The active tab as a peek: (its index, its source's index).
    pub(crate) fn peeking(&self) -> Option<(usize, usize)> {
        let tab = self.tabs.get(self.active)?;
        let src = tab.peek?;
        let s = self.tabs.iter().position(|t| t.id == src)?;
        Some((self.active, s))
    }

    /// Where the floating page sits: centred, most of the content.
    pub(crate) fn peek_rect(&self) -> Rect {
        let c = self.content_rect();
        let w = (c.w * 0.8).round().max(320.0);
        let h = (c.h * 0.86).round().max(240.0);
        Rect::new((c.x + (c.w - w) / 2.0).round(), (c.y + (c.h - h) / 2.0 - self.px(6.0)).round(), w, h)
    }

    /// Open `url` as a peek over tab `source`.
    pub(crate) fn open_peek(&mut self, source: usize, url: &str) {
        if source >= self.tabs.len() {
            return;
        }
        // One at a time: a peek over a peek replaces it.
        if let Some((p, _)) = self.peeking() {
            self.close_peek_at(p);
        }
        let Some(w) = self.new_web_pane(url) else { return };
        let mut tab = self.make_tab(Pane::Web(w), None);
        tab.peek = Some(self.tabs[source].id);
        let parent_look = self.tabs[source].look.clone();
        tab.look = self.look_for(&tab.left, Some(&parent_look));
        self.tabs.push(tab);
        let at = self.tabs.len() - 1;
        self.peek_anim.replay(0.0, 1.0, self.motion.dur(160.0));
        self.play_event("tab.switch");
        self.activate(at);
    }

    /// Close the peek and go back to its source.
    pub(crate) fn close_peek(&mut self) {
        if let Some((p, _)) = self.peeking() {
            self.close_peek_at(p);
        }
    }

    fn close_peek_at(&mut self, p: usize) {
        let src = self.tabs[p].peek.and_then(|id| self.tabs.iter().position(|t| t.id == id));
        self.tabs.remove(p);
        self.tab_removed(p);
        let back = src.map(|s| if s > p { s - 1 } else { s }).unwrap_or(0).min(self.tabs.len().saturating_sub(1));
        if !self.tabs.is_empty() {
            self.activate(back);
        }
        self.layout();
    }

    /// Keep the peek: it becomes a page under its source, in the stack.
    pub(crate) fn keep_peek(&mut self) {
        let Some((p, s)) = self.peeking() else { return };
        let src_id = self.tabs[s].id;
        self.tabs[p].peek = None;
        self.collapsed.remove(&src_id);
        self.reparent(p, Some(s), None);
        self.play_event("tab.switch");
        self.save_session();
        self.layout();
    }

    /// Lay the peek and its source out; false when the active tab isn't one.
    pub(crate) fn layout_peek(&mut self) -> bool {
        let Some((p, s)) = self.peeking() else { return false };
        let c = self.content_rect();
        self.layout_tab(s, c);
        let r = self.peek_rect();
        self.layout_tab(p, r);
        true
    }

    /// Draw the source dimmed, a scrim, then the floating page; false when
    /// the active tab isn't a peek.
    pub(crate) fn draw_peek(&mut self, scene: &mut Scene) -> bool {
        let Some((p, s)) = self.peeking() else { return false };
        let t = self.theme.clone();
        let ink = t.ink;
        let c = self.content_rect();
        let k = self.peek_anim.value();
        let src_n = self.tab_label(s);
        let src_look = self.tabs[s].look.clone();
        let look = self.tabs[p].look.clone();
        let mut tabs = std::mem::take(&mut self.tabs);
        {
            let src = &mut tabs[s];
            let split = src.right.is_some();
            self.draw_pane(scene, &mut src.left, &src_n, false, &src_look, split);
            if let Some(r) = src.right.as_mut() {
                self.draw_pane(scene, r, &src_n, false, &src_look, true);
            }
        }
        // The scrim: the paper over everything, most of the way.
        scene.layer(None);
        scene.rect(c, crate::app::fade(self.paper(), 0.6 * k));
        let r = self.peek_rect();
        // Rises a touch as it appears.
        let lift = (1.0 - k) * self.px(12.0);
        let r = Rect::new(r.x, r.y + lift, r.w, r.h);
        scene.rect(Rect::new(r.x + self.px(4.0), r.y + self.px(4.0), r.w, r.h), crate::app::fade(ink, 0.8 * k));
        scene.rect(r, self.paper());
        scene.layer(Some(r));
        {
            let tab = &mut tabs[p];
            self.draw_pane(scene, &mut tab.left, "", true, &look, false);
        }
        scene.layer(None);
        scene.outline(r, self.px(m::STRUCTURE), ink);
        self.tabs = tabs;
        // The caption under it.
        let label = self.label();
        let cap = Style { color: t.paper, ..label };
        let text = "PEEK  ·  ESC CLOSES  ·  CTRL+ENTER KEEPS IT IN THE STACK";
        let tw = self.fonts.measure(cap, text);
        let cx = r.x + ((r.w - tw) / 2.0).round();
        let cy = r.bottom() + self.px(6.0);
        scene.rect(Rect::new(cx - self.px(10.0), cy, tw + self.px(20.0), self.px(22.0)), crate::app::fade(ink, k));
        self.fonts.draw(scene, cap, cx, cy + self.px(15.0), text);
        true
    }
}
