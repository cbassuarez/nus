//! A toast: one ink slip at the foot of the content, in the band's voice —
//! an icon, a few caps words, six seconds, click to act. It is how nus
//! says "copied! · github.com/…" and "behind · docs.rs ›" without giving
//! the strip words. One at a time; a new one replaces it. A slip that
//! can be clicked ends in a caret and goes signal under the pointer.

use std::time::Instant;

use nus_render::text::icons;
use nus_render::theme::metric as m;
use nus_render::{Rect, Scene, Style};

use crate::app::{fade, App};

/// An icon: (name, svg), as the text module keeps them.
pub type Icon = (&'static str, &'static str);

pub enum Act {
    /// Bring this tab (by id) to the front.
    GoTab(u64),
}

pub struct Toast {
    pub icon: Option<Icon>,
    /// The words, strong; and a tail in the same voice, dimmer (a url, a host).
    pub text: String,
    pub tail: String,
    pub act: Option<Act>,
    pub at: Instant,
    pub rect: Rect,
}

/// Seconds it stays.
const HOLD: f32 = 6.0;

impl App {
    pub(crate) fn toast(&mut self, text: impl Into<String>, act: Option<Act>) {
        self.toast_with(None, text, "", act);
    }

    /// A slip with an icon and a dimmer tail after the words.
    pub(crate) fn toast_with(&mut self, icon: Option<Icon>, text: impl Into<String>, tail: impl Into<String>, act: Option<Act>) {
        self.toast = Some(Toast { icon, text: text.into(), tail: tail.into(), act, at: crate::clock::now(), rect: Rect::new(0.0, 0.0, 0.0, 0.0) });
        let d = self.motion.dur(crate::anim::base::PALETTE);
        self.toast_anim.replay(0.0, 1.0, d);
        self.dirty = true;
    }

    /// The slip, rising from the foot of the content; it sinks back when
    /// its time is up.
    pub(crate) fn draw_toast(&mut self, scene: &mut Scene) {
        let Some(t) = self.toast.as_ref() else { return };
        let age = crate::clock::since(t.at).as_secs_f32();
        if age > HOLD {
            self.toast = None;
            self.dirty = true;
            return;
        }
        let (icon, text, tail, actionable) = (t.icon, t.text.clone(), t.tail.clone(), t.act.is_some());
        let th = self.theme.clone();
        let strong = self.label_strong();
        let leave = if age > HOLD - 0.3 { ((HOLD - age) / 0.3).clamp(0.0, 1.0) } else { 1.0 };
        let rise = self.toast_anim.value() * leave;
        let c = self.content_rect();
        let bh = self.header_h();
        let pad = self.px(m::HEADER_PAD_X);
        let gap = self.px(8.0);
        let isz = self.px(14.0);
        let csz = self.px(11.0);
        // The tail gives way before the words do.
        let room = c.w - pad * 2.0 - self.px(40.0);
        let tail = if tail.is_empty() { tail } else { self.fit(strong, &tail, (room - self.fonts.measure(strong, &text) - isz - gap * 3.0).max(self.px(60.0))) };
        let tw = self.fonts.measure(strong, &text);
        let tlw = if tail.is_empty() { 0.0 } else { self.fonts.measure(strong, &tail) + gap };
        let iw = if icon.is_some() { isz + gap } else { 0.0 };
        let cw = if actionable { csz + gap } else { 0.0 };
        let w = (pad * 2.0 + iw + tw + tlw + cw).round();
        let x = (c.x + (c.w - w) / 2.0).round();
        let rest = c.bottom() - self.px(m::HEADER_PAD_Y) * 2.0 - bh;
        let r = Rect::new(x, rest + (1.0 - rise) * (bh + self.px(m::HEADER_PAD_Y) * 2.0), w, bh);
        // Under the pointer, a slip that acts goes signal: it can be pressed.
        let (mx, my) = self.mouse;
        let hot = actionable && r.contains(mx, my);
        scene.layer(Some(c));
        scene.rect(Rect::new(r.x + self.px(3.0), r.y + self.px(3.0), r.w, r.h), fade(th.ink, 0.35 * rise));
        scene.rect(r, if hot { self.surface.signal } else { th.ink });
        let by = r.y + self.px(m::HEADER_PAD_Y) + self.px(m::UI_PX) - self.px(3.0);
        let paper = if hot { [1.0, 1.0, 1.0, 1.0] } else { th.paper };
        let mut tx = r.x + pad;
        if let Some(ic) = icon {
            self.fonts.draw_icon(scene, ic, isz, tx, by - isz + self.px(2.0), if hot { paper } else { self.surface.signal });
            tx += isz + gap;
        }
        tx += self.fonts.draw(scene, Style { color: paper, ..strong }, tx, by, &text);
        if !tail.is_empty() {
            tx += gap;
            tx += self.fonts.draw(scene, Style { color: fade(paper, 0.62), ..strong }, tx, by, &tail);
        }
        if actionable {
            tx += gap;
            self.fonts.draw_icon(scene, icons::CARET_RIGHT, csz, tx, by - csz + self.px(1.0), paper);
        }
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
            self.toast_with(Some(icons::TO_TAB), "BEHIND", host, Some(Act::GoTab(id)));
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
        self.toast_with(Some(icons::COPY), "COPIED!", url, None);
        true
    }
}
