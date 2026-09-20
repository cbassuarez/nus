//! The page's menu: a right-click on a page, or the strip's media icon.
//! CEF hands the click's context over (browser.rs builds the entries in
//! nus's words); nus draws the menu itself in the sidebar menus' voice —
//! paper, hairlines, rows that tint under the pointer — and tells CEF what
//! was picked. Save media, copy an address, open a link in a tab or beside,
//! cut · copy · paste, back · forward · reload, the page's address, the
//! source. The strip's icon lists the page's saveable media the same way.

use nus_render::theme::metric as m;
use nus_render::{Rect, Scene, Style, Theme};

use crate::anim::Anim;
use crate::app::{App, Pane};
use cef::ImplRunContextMenuCallback;

use crate::browser::{self, Media};

pub enum Source {
    /// CEF asked; the pick goes back through the callback.
    Page(cef::RunContextMenuCallback),
    /// The strip's icon: the pick is a media index to save.
    Media(Vec<Media>),
}

pub struct PageMenu {
    pub tab: u64,
    pub right: bool,
    pub at: (f32, f32),
    /// (command id, label, enabled); an empty label is a separator.
    pub items: Vec<(i32, String, bool)>,
    pub source: Source,
    pub hits: Vec<(Rect, i32)>,
    pub rise: Anim,
}

impl App {
    /// Menus the pages asked for, links they asked to open, words they
    /// asked to say — every tick.
    pub(crate) fn poll_page_menus(&mut self) {
        let scale = self.scale;
        let mut opens: Vec<(String, bool)> = Vec::new();
        let mut said: Vec<String> = Vec::new();
        let mut menu: Option<PageMenu> = None;
        for tab in &self.tabs {
            for (right, pane) in [(false, &tab.left)].into_iter().chain(tab.right.as_ref().map(|r| (true, r))) {
                let Pane::Web(w) = pane else { continue };
                let mut s = w.tab.shared.borrow_mut();
                opens.append(&mut s.opens);
                if let Some(word) = s.said.take() {
                    said.push(word);
                }
                if let Some(req) = s.menu.take() {
                    if let Some(old) = menu.take() {
                        if let Source::Page(cb) = old.source {
                            cb.cancel();
                        }
                    }
                    menu = Some(PageMenu {
                        tab: tab.id,
                        right,
                        at: (w.page.x + req.x * scale, w.page.y + req.y * scale),
                        items: req.items,
                        source: Source::Page(req.callback),
                        hits: Vec::new(),
                        rise: Anim::at(0.0),
                    });
                }
            }
        }
        for (url, beside) in opens {
            self.open_url(&url, !beside);
        }
        for word in said {
            if word == "PIP" {
                self.run(crate::app::Action::Pip);
            } else {
                self.toast(word, None);
            }
        }
        if let Some(mut m) = menu {
            self.close_page_menu();
            m.rise.replay(0.0, 1.0, self.motion.dur(120.0));
            self.page_menu = Some(m);
            self.dirty = true;
        }
    }

    /// The strip's icon: one saveable file saves at once; more list here.
    pub(crate) fn media_menu(&mut self, tab: u64, right: bool, at: (f32, f32)) {
        let Some(w) = self.tabs.iter().find(|t| t.id == tab).and_then(|t| if right { t.right.as_ref() } else { Some(&t.left) }).and_then(|p| if let Pane::Web(w) = p { Some(w) } else { None }) else { return };
        let media: Vec<Media> = w.tab.shared.borrow().media.clone();
        let saveable: Vec<&Media> = media.iter().filter(|m| !m.blob).collect();
        if saveable.len() == 1 {
            let src = saveable[0].src.clone();
            w.tab.download(&src);
            self.toast(format!("SAVING · {}", crate::app::fit_cmd(&src, 60)), None);
            return;
        }
        let mut items: Vec<(i32, String, bool)> = Vec::new();
        for (i, mm) in media.iter().enumerate() {
            let what = if mm.kind == "audio" { "AUDIO" } else { "VIDEO" };
            let size = if mm.w > 0 { format!(" · {}×{}", mm.w, mm.h) } else { String::new() };
            let ext = mm.src.rsplit('.').next().filter(|e| e.len() <= 4 && !e.contains('/')).map(|e| format!(" · {}", e.to_uppercase())).unwrap_or_default();
            if mm.blob {
                items.push((i as i32, format!("{what}{size} · STREAMS, NOTHING TO SAVE"), false));
            } else {
                items.push((i as i32, format!("SAVE {what}{size}{ext}"), true));
            }
        }
        if items.is_empty() {
            return;
        }
        self.close_page_menu();
        let mut m = PageMenu { tab, right, at, items, source: Source::Media(media), hits: Vec::new(), rise: Anim::at(0.0) };
        m.rise.replay(0.0, 1.0, self.motion.dur(120.0));
        self.page_menu = Some(m);
        self.dirty = true;
    }

    pub(crate) fn close_page_menu(&mut self) {
        if let Some(m) = self.page_menu.take() {
            if let Source::Page(cb) = m.source {
                cb.cancel();
            }
            self.dirty = true;
        }
    }

    /// The pick: back to CEF, or a save of our own.
    fn page_menu_pick(&mut self, id: i32) {
        let Some(m) = self.page_menu.take() else { return };
        match m.source {
            Source::Page(cb) => cb.cont(id, cef::EventFlags::default()),
            Source::Media(media) => {
                if let Some(mm) = media.get(id as usize).filter(|mm| !mm.blob) {
                    let src = mm.src.clone();
                    if let Some(w) = self.tabs.iter().find(|t| t.id == m.tab).and_then(|t| if m.right { t.right.as_ref() } else { Some(&t.left) }).and_then(|p| if let Pane::Web(w) = p { Some(w) } else { None }) {
                        w.tab.download(&src);
                    }
                    self.toast(format!("SAVING · {}", crate::app::fit_cmd(&src, 60)), None);
                }
            }
        }
        self.dirty = true;
    }

    /// A click while the menu is up: on a row, its pick; anywhere else, gone.
    pub(crate) fn page_menu_click(&mut self, x: f32, y: f32) -> bool {
        let Some(m) = self.page_menu.as_ref() else { return false };
        let hit = m.hits.iter().find(|(r, _)| r.contains(x, y)).map(|(_, id)| *id);
        match hit {
            Some(id) => self.page_menu_pick(id),
            None => self.close_page_menu(),
        }
        true
    }

    /// Esc closes it; ↑ ↓ and Enter walk it.
    pub(crate) fn page_menu_key(&mut self, ev: &crate::app::KeyIn) -> bool {
        use winit::keyboard::{Key as K, NamedKey};
        if self.page_menu.is_none() || ev.state != winit::event::ElementState::Pressed {
            return false;
        }
        match &ev.logical_key {
            K::Named(NamedKey::Escape) => {
                self.close_page_menu();
                true
            }
            _ => false,
        }
    }

    /// Row height, the panel, the rows; drawn last, over everything on the page.
    pub(crate) fn draw_page_menu(&mut self, scene: &mut Scene) {
        let Some(m) = self.page_menu.as_ref() else { return };
        let t = self.theme.clone();
        let ink = t.ink;
        let label = self.label();
        let row = self.px(30.0);
        let sep_h = self.px(9.0);
        let width = self.px(268.0);
        let (wx, wy) = (self.target.size.0 as f32, self.target.size.1 as f32);
        let items = m.items.clone();
        let k = m.rise.value();
        let h: f32 = items.iter().map(|(_, l, _)| if l.is_empty() { sep_h } else { row }).sum::<f32>() + self.px(2.0);
        // At the click, flipped to stay on the window.
        let mut x = m.at.0.round();
        let mut y = m.at.1.round();
        if x + width > wx - self.px(8.0) {
            x = (wx - width - self.px(8.0)).max(0.0);
        }
        if y + h > wy - self.px(8.0) {
            y = (y - h).max(self.px(8.0));
        }
        let panel = Rect::new(x, y, width, h * k);
        let (mx, my) = self.mouse;
        scene.layer(Some(panel));
        scene.rect(panel, t.paper);
        let mut hits = Vec::new();
        let mut yy = y + self.px(1.0);
        for (id, text, enabled) in &items {
            if text.is_empty() {
                scene.hline(x + self.px(10.0), yy + (sep_h / 2.0).round(), width - self.px(20.0), self.px(m::HAIRLINE), Theme::with_alpha(ink, 0.25));
                yy += sep_h;
                continue;
            }
            let cell = Rect::new(x, yy, width, row);
            let hot = *enabled && cell.contains(mx, my);
            if hot {
                scene.rect(cell, t.tint);
            }
            let c = if !*enabled { t.dim } else { ink };
            let icon = match *id {
                browser::CMD_SAVE_MEDIA => Some(nus_render::text::icons::DOWNLOAD),
                browser::CMD_COPY_MEDIA | browser::CMD_COPY_LINK | browser::CMD_COPY_PAGE | 113 => Some(nus_render::text::icons::COPY),
                browser::CMD_OPEN_MEDIA | browser::CMD_OPEN_LINK => Some(nus_render::text::icons::TO_TAB),
                browser::CMD_OPEN_LINK_BESIDE => Some(nus_render::text::icons::SWAP),
                browser::CMD_PIP => Some(nus_render::text::icons::PIP),
                100 => Some(nus_render::text::icons::BACK),
                101 => Some(nus_render::text::icons::FORWARD),
                102 => Some(nus_render::text::icons::RELOAD),
                132 => Some(nus_render::text::icons::CODE),
                _ if matches!(m.source, Source::Media(_)) => Some(nus_render::text::icons::DOWNLOAD),
                _ => None,
            };
            let isz = self.px(12.0);
            if let Some(ic) = icon {
                self.fonts.draw_icon(scene, ic, isz, x + self.px(11.0), yy + ((row - isz) / 2.0).round(), c);
            }
            let shown = self.fit(label, text, width - self.px(30.0) - self.px(12.0) - self.px(50.0));
            self.fonts.draw(scene, Style { color: c, ..label }, x + self.px(30.0), yy + self.px(19.0), &shown);
            let key = match *id {
                browser::CMD_COPY_PAGE => "CTRL+SHIFT+C",
                113 => "CTRL+C",
                114 => "CTRL+V",
                112 => "CTRL+X",
                117 => "CTRL+A",
                110 => "CTRL+Z",
                102 => "CTRL+R",
                _ => "",
            };
            if !key.is_empty() {
                let kw = self.fonts.measure(label, key);
                self.fonts.draw(scene, Style { color: t.dim, ..label }, x + width - self.px(12.0) - kw, yy + self.px(19.0), key);
            }
            if *enabled {
                hits.push((cell, *id));
            }
            yy += row;
        }
        scene.outline(panel, self.px(m::HAIRLINE), ink);
        scene.layer(None);
        if let Some(mm) = self.page_menu.as_mut() {
            mm.hits = hits;
        }
        if k < 1.0 {
            self.dirty = true;
        }
    }
}
