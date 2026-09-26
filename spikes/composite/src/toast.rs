//! A toast: one ink slip at the foot of the content — a tone cell with
//! an icon, a few Capitalized words, a dimmer detail, and when it can do
//! something, a chip that says what. It is how nus says "Copied ·
//! github.com/…" and "Opened Behind · docs.rs [Go To Tab]".
//!
//! The words name what happened and are drawn as written: Capitalized,
//! all caps only for what is all caps (URL, HTTP, WDJB-MJHT, ⌘⇧O). The
//! detail is the thing itself — a URL, a file, a code, an error — also as
//! written. `scripts/check-toasts.py` holds the call sites to that.
//!
//! One at a time; a new one replaces it after six seconds. A problem is
//! different: its cell fills signal, it stays until dismissed (× or a
//! click), and what arrives meanwhile waits behind it.

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
    /// Show this download (by key) in its folder.
    RevealDownload(u64),
    /// Open this port, as O does while it glows.
    OpenPort(crate::ports::Key),
    /// Try installing this tool (by bundle id) again.
    RetryInstall(String),
    /// Install this tool (by bundle id), from a notice that it is missing.
    Install(String),
    /// Save the sign-in a page just sent (passwords.rs).
    SavePassword,
    /// Fill the saved sign-in into the page that asked (passwords.rs).
    FillPassword,
    /// Delete every saved sign-in, confirmed (passwords.rs).
    ForgetPasswords,
    /// Show this file in its folder.
    RevealPath(std::path::PathBuf),
    /// Hand an address to the app that owns its scheme (mailto:, zoommtg:).
    OpenExternal(String),
    /// Take back what a hunk's chip just did, in that folder.
    UndoHunk(crate::diffs::Hunk, crate::diffs::Do, std::path::PathBuf),
    /// Take a page back from WebKit into Chromium: (tab id, right half).
    LeaveWebKit(u64, bool),
}

impl Act {
    /// The chip's words, and the key that does the same when there is one.
    fn chip(&self) -> (&'static str, String) {
        match self {
            Act::GoTab(_) => ("Go To Tab", String::new()),
            Act::RevealDownload(_) => ("Show In Folder", String::new()),
            Act::OpenPort(_) => ("Open", crate::app::key("O", true)),
            Act::RetryInstall(_) => ("Retry", String::new()),
            Act::Install(_) => ("Get", String::new()),
            Act::SavePassword => ("Save", String::new()),
            Act::FillPassword => ("Fill", String::new()),
            Act::ForgetPasswords => ("Forget", String::new()),
            Act::UndoHunk(..) => ("Undo", String::new()),
            Act::LeaveWebKit(..) => ("Open In Chromium", String::new()),
            Act::OpenExternal(_) => ("Open", String::new()),
            Act::RevealPath(_) => ("Show In Folder", String::new()),
        }
    }
}

pub struct Toast {
    pub icon: Icon,
    /// What happened, strong; and the thing it happened to, dimmer.
    pub words: String,
    pub detail: String,
    pub act: Option<Act>,
    /// Signal cell, no timeout, a ×.
    pub problem: bool,
    pub at: Instant,
    pub rect: Rect,
    pub chip: Rect,
}

/// Seconds a toast stays; a problem stays until dismissed.
const HOLD: f32 = 6.0;

impl App {
    /// A toast: what happened, and what it happened to.
    pub(crate) fn toast(&mut self, icon: Icon, words: impl Into<String>, detail: impl Into<String>, act: Option<Act>) {
        let t = Toast { icon, words: words.into(), detail: detail.into(), act, problem: false, at: crate::clock::now(), rect: Rect::new(0.0, 0.0, 0.0, 0.0), chip: Rect::new(0.0, 0.0, 0.0, 0.0) };
        // A problem on screen is not pushed off by good news: it waits.
        if self.toast.as_ref().is_some_and(|s| s.problem) {
            self.toast_held = Some(t);
            return;
        }
        self.show_toast(t);
    }

    /// Something went wrong: the signal cell, a sound, and it stays.
    pub(crate) fn toast_problem(&mut self, words: impl Into<String>, detail: impl Into<String>, act: Option<Act>) {
        self.play_event("error");
        let t = Toast { icon: icons::WARNING, words: words.into(), detail: detail.into(), act, problem: true, at: crate::clock::now(), rect: Rect::new(0.0, 0.0, 0.0, 0.0), chip: Rect::new(0.0, 0.0, 0.0, 0.0) };
        self.show_toast(t);
    }

    /// A word on what just happened: the editor's status row when an
    /// editor has the focus, else a toast.
    pub(crate) fn notice(&mut self, icon: Icon, words: impl Into<String>, detail: impl Into<String>) {
        let (words, detail) = (words.into(), detail.into());
        if let Some(e) = self.focused_editor() {
            e.notice = Some((status_line(&words, &detail), crate::clock::now()));
        } else {
            self.toast(icon, words, detail, None);
        }
        self.dirty = true;
    }

    /// `notice`, for something that went wrong.
    pub(crate) fn notice_problem(&mut self, words: impl Into<String>, detail: impl Into<String>) {
        let (words, detail) = (words.into(), detail.into());
        if let Some(e) = self.focused_editor() {
            e.notice = Some((status_line(&words, &detail), crate::clock::now()));
            self.play_event("error");
        } else {
            self.toast_problem(words, detail, None);
        }
        self.dirty = true;
    }

    fn show_toast(&mut self, t: Toast) {
        self.toast = Some(t);
        let d = self.motion.dur(crate::anim::base::PALETTE);
        self.toast_anim.replay(0.0, 1.0, d);
        self.dirty = true;
    }

    /// A problem went away: what waited behind it shows, if still fresh.
    fn show_held(&mut self) {
        if let Some(mut t) = self.toast_held.take() {
            if crate::clock::since(t.at).as_secs_f32() < HOLD {
                t.at = crate::clock::now();
                self.show_toast(t);
            }
        }
    }

    /// The slip, rising from the foot of the content; it sinks back when
    /// its time is up.
    pub(crate) fn draw_toast(&mut self, scene: &mut Scene) {
        let Some(t) = self.toast.as_ref() else { return };
        let age = crate::clock::since(t.at).as_secs_f32();
        if !t.problem && age > HOLD {
            self.toast = None;
            self.dirty = true;
            return;
        }
        let (icon, words, detail, chip, problem) = (t.icon, t.words.clone(), t.detail.clone(), t.act.as_ref().map(Act::chip), t.problem);
        let th = self.theme.clone();
        let strong = self.label_strong();
        let signal = self.surface.signal;
        let leave = if !problem && age > HOLD - 0.3 { ((HOLD - age) / 0.3).clamp(0.0, 1.0) } else { 1.0 };
        let rise = self.toast_anim.value() * leave;
        let c = self.content_rect();
        let bh = self.header_h();
        let pad = self.px(m::HEADER_PAD_X);
        let inner = self.px(10.0);
        let gap = self.px(8.0);
        let isz = self.px(14.0);
        let xsz = self.px(11.0);
        let chip_pad = self.px(9.0);
        let paper = th.paper;
        let dim = fade(paper, 0.62);
        // What does not give way: the cell, the chip, the ×, the ends.
        let chip_w = chip.as_ref().map(|(w, k)| {
            let kw = if k.is_empty() { 0.0 } else { self.px(6.0) + self.fonts.measure_as_is(strong, k) };
            chip_pad * 2.0 + self.fonts.measure_as_is(strong, w) + kw
        });
        let fixed = bh + inner
            + chip_w.map_or(0.0, |w| self.px(12.0) + w)
            + if problem { inner + xsz } else { 0.0 }
            + if chip.is_some() || problem { inner } else { pad };
        // The detail gives way before the words do.
        let room = (c.w - self.px(40.0) - fixed).max(self.px(60.0));
        let words = self.fit_as_is(strong, &words, room);
        let ww = self.fonts.measure_as_is(strong, &words);
        let droom = room - ww - gap;
        let detail = if detail.is_empty() || droom < self.px(48.0) { String::new() } else { self.fit_as_is(strong, &detail, droom) };
        let dw = if detail.is_empty() { 0.0 } else { gap + self.fonts.measure_as_is(strong, &detail) };
        let w = (fixed + ww + dw).round();
        let x = (c.x + (c.w - w) / 2.0).round();
        let rest = c.bottom() - self.px(m::HEADER_PAD_Y) * 2.0 - bh;
        let r = Rect::new(x, rest + (1.0 - rise) * (bh + self.px(m::HEADER_PAD_Y) * 2.0), w, bh);
        let (mx, my) = self.mouse;
        scene.layer(Some(c));
        scene.rect(Rect::new(r.x + self.px(3.0), r.y + self.px(3.0), r.w, r.h), fade(th.ink, 0.35 * rise));
        scene.rect(r, th.ink);
        // The tone cell: ink with a hairline, or signal for a problem.
        let cell = Rect::new(r.x, r.y, bh, bh);
        if problem {
            scene.rect(cell, signal);
        } else {
            scene.rect(Rect::new(cell.right() - self.px(1.0), r.y, self.px(1.0), bh), fade(paper, 0.16));
        }
        let white = [1.0, 1.0, 1.0, 1.0];
        self.fonts.draw_icon(scene, icon, isz, (cell.x + (bh - isz) / 2.0).round(), (cell.y + (bh - isz) / 2.0).round(), if problem { white } else { signal });
        let by = r.y + self.px(m::HEADER_PAD_Y) + self.px(m::UI_PX) - self.px(3.0);
        let mut tx = r.x + bh + inner;
        tx += self.fonts.draw_as_is(scene, Style { color: paper, ..strong }, tx, by, &words);
        if !detail.is_empty() {
            tx += gap;
            tx += self.fonts.draw_as_is(scene, Style { color: dim, ..strong }, tx, by, &detail);
        }
        // The chip says what a press does; only it goes signal.
        let mut chip_rect = Rect::new(0.0, 0.0, 0.0, 0.0);
        if let (Some((cw, key)), Some(chw)) = (chip, chip_w) {
            tx += self.px(12.0);
            let ch = bh - self.px(10.0);
            chip_rect = Rect::new(tx.round(), (r.y + (bh - ch) / 2.0).round(), chw.round(), ch.round());
            let hot = chip_rect.contains(mx, my);
            if hot {
                scene.rect(chip_rect, signal);
            } else {
                scene.outline(chip_rect, self.px(1.0), fade(paper, 0.45));
            }
            let ink = if hot { white } else { paper };
            let kx = chip_rect.x + chip_pad + self.fonts.draw_as_is(scene, Style { color: ink, ..strong }, chip_rect.x + chip_pad, by, cw);
            if !key.is_empty() {
                self.fonts.draw_as_is(scene, Style { color: fade(ink, 0.62), ..strong }, kx + self.px(6.0), by, &key);
            }
            tx = chip_rect.right();
        }
        if problem {
            tx += inner;
            let hot = Rect::new(tx - self.px(6.0), r.y, xsz + self.px(12.0), bh).contains(mx, my);
            self.fonts.draw_icon(scene, icons::CLOSE, xsz, tx, (r.y + (bh - xsz) / 2.0).round(), if hot { paper } else { dim });
        }
        scene.layer(None);
        if let Some(t) = self.toast.as_mut() {
            t.rect = r;
            t.chip = chip_rect;
        }
        // It leaves on its own: keep drawing while it is up. A problem
        // stays still once it has risen, and waits for the pointer.
        if !problem || self.toast_anim.active() {
            self.dirty = true;
        }
    }

    /// A click on the chip does what it says; a click anywhere else on
    /// the slip (the × included) puts it away.
    pub(crate) fn toast_click(&mut self, x: f32, y: f32) -> bool {
        let Some(t) = self.toast.as_ref() else { return false };
        if !t.rect.contains(x, y) {
            return false;
        }
        let on_chip = t.chip.contains(x, y);
        let act = self.toast.take().and_then(|t| t.act).filter(|_| on_chip);
        match act {
            Some(Act::GoTab(id)) => {
                if let Some(i) = self.tabs.iter().position(|t| t.id == id) {
                    self.activate(i);
                }
            }
            Some(Act::RevealDownload(key)) => self.download_action(crate::downloads::Hit::Reveal(key)),
            Some(Act::OpenPort(key)) => {
                self.board.toast = None;
                self.ports_act(&key, crate::ports::Act::Open);
            }
            Some(Act::RetryInstall(id) | Act::Install(id)) => self.bundle_toggle(&id),
            Some(Act::SavePassword) => self.save_offered_password(),
            Some(Act::FillPassword) => self.fill_offered_password(),
            Some(Act::ForgetPasswords) => self.forget_passwords(),
            Some(Act::UndoHunk(hunk, what, cwd)) => self.undo_hunk(hunk, what, cwd),
            Some(Act::OpenExternal(url)) => crate::app::open_with_os(std::path::Path::new(&url)),
            Some(Act::RevealPath(path)) => crate::downloads::reveal(&path, true),
            Some(Act::LeaveWebKit(id, right)) => {
                if let Some(t) = self.tabs.iter().find(|t| t.id == id) {
                    let p = if right { t.right.as_ref() } else { Some(&t.left) };
                    if let Some(crate::app::Pane::Web(w)) = p {
                        w.tab.leave_native();
                    }
                }
            }
            None => {}
        }
        self.show_held();
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
            self.toast(icons::TO_TAB, "Opened Behind", host, Some(Act::GoTab(id)));
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
        self.toast(icons::COPY, "Copied", url, None);
        true
    }
}

/// The same words for the editor's one-line status row.
fn status_line(words: &str, detail: &str) -> String {
    if detail.is_empty() { words.to_string() } else { format!("{words} · {detail}") }
}
