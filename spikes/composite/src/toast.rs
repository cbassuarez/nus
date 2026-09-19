//! A toast: one ink slip at the foot of the content, in the band's voice —
//! a few caps words, six seconds, click to act. It is how nus says
//! "opened behind · docs.rs · click to go" and "copied · github.com/…"
//! without giving the strip words. One at a time; a new one replaces it.

use std::time::Instant;

use nus_render::theme::metric as m;
use nus_render::{Rect, Scene, Style};

use crate::app::App;

pub enum Act {
    /// Bring this tab (by id) to the front.
    GoTab(u64),
}

pub struct Toast {
    pub text: String,
    pub act: Option<Act>,
    pub at: Instant,
    pub rect: Rect,
}

/// Seconds it stays.
const HOLD: f32 = 6.0;

impl App {
    pub(crate) fn toast(&mut self, text: impl Into<String>, act: Option<Act>) {
        self.toast = Some(Toast { text: text.into(), act, at: Instant::now(), rect: Rect::new(0.0, 0.0, 0.0, 0.0) });
        let d = self.motion.dur(crate::anim::base::PALETTE);
        self.toast_anim.replay(0.0, 1.0, d);
        self.dirty = true;
    }

    /// The slip, rising from the foot of the content; it sinks back when
    /// its time is up.
    pub(crate) fn draw_toast(&mut self, scene: &mut Scene) {
        let Some(t) = self.toast.as_ref() else { return };
        let age = t.at.elapsed().as_secs_f32();
        if age > HOLD {
            self.toast = None;
            self.dirty = true;
            return;
        }
        let text = t.text.clone();
        let th = self.theme.clone();
        let strong = self.label_strong();
        let leave = if age > HOLD - 0.3 { ((HOLD - age) / 0.3).clamp(0.0, 1.0) } else { 1.0 };
        let rise = self.toast_anim.value() * leave;
        let c = self.content_rect();
        let bh = self.header_h();
        let tw = self.fonts.measure(strong, &text);
        let w = (tw + self.px(m::HEADER_PAD_X) * 2.0).round();
        let x = (c.x + (c.w - w) / 2.0).round();
        let rest = c.bottom() - self.px(m::HEADER_PAD_Y) * 2.0 - bh;
        let r = Rect::new(x, rest + (1.0 - rise) * (bh + self.px(m::HEADER_PAD_Y) * 2.0), w, bh);
        scene.layer(Some(c));
        scene.rect(r, th.ink);
        let by = r.y + self.px(m::HEADER_PAD_Y) + self.px(m::UI_PX) - self.px(3.0);
        self.fonts.draw(scene, Style { color: th.paper, ..strong }, r.x + self.px(m::HEADER_PAD_X), by, &text);
        scene.layer(None);
        if let Some(t) = self.toast.as_mut() {
            t.rect = r;
        }
        // It leaves on its own: keep drawing while it is up.
        self.dirty = true;
    }

    /// A click on the slip does what it offers, and dismisses it either way.
    pub(crate) fn toast_click(&mut self, x: f32, y: f32) -> bool {
        let Some(t) = self.toast.as_ref() else { return false };
        if !t.rect.contains(x, y) {
            return false;
        }
        let act = self.toast.take().and_then(|t| t.act);
        if let Some(Act::GoTab(id)) = act {
            if let Some(i) = self.tabs.iter().position(|t| t.id == id) {
                self.activate(i);
            }
        }
        self.dirty = true;
        true
    }

    /// A page opened by something other than your own hand — `nus open`
    /// from a shell, an assistant, a rule, a link handed from outside:
    /// TABS · OPENED BY OTHERS says whether it comes to the front or opens
    /// behind with a toast to go there.
    pub(crate) fn open_url_by_other(&mut self, url: &str) {
        let behind = self.behavior.opened_by_others == crate::settings::OpenedBy::Behind;
        let Some(w) = self.new_web_pane(url) else { return };
        let tab = self.make_tab(crate::app::Pane::Web(w), None);
        let id = tab.id;
        self.tabs.push(tab);
        if behind {
            let host = crate::links::host(url);
            self.toast(format!("OPENED BEHIND · {host} · CLICK TO GO"), Some(Act::GoTab(id)));
            self.layout();
        } else {
            self.activate(self.tabs.len() - 1);
        }
        self.dirty = true;
    }

    /// The focused page's URL, when a page has the focus.
    pub(crate) fn focused_page_url(&self) -> Option<String> {
        let tab = self.tabs.get(self.active)?;
        let pane = if tab.focus_right { tab.right.as_ref()? } else { &tab.left };
        match pane {
            crate::app::Pane::Web(w) => Some(w.tab.shared.borrow().url.clone()),
            _ => None,
        }
    }

    /// The focused page's URL to the clipboard, with a word.
    pub(crate) fn copy_page_url(&mut self) -> bool {
        let Some(url) = self.focused_page_url() else { return false };
        if url.is_empty() {
            return false;
        }
        if let Ok(mut cb) = arboard::Clipboard::new() {
            let _ = cb.set_text(url.clone());
        }
        self.play_event("toggle");
        self.toast(format!("COPIED · {}", crate::app::fit_cmd(&url, 60)), None);
        true
    }
}
