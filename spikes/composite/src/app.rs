//! The composite window: Broadsheet chrome around terminal and browser panes.

use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use nus_render::theme::{metric as m, signal};
use nus_render::{FontId, FontSystem, Gpu, GridRenderer, Rect, Scene, Style, Theme};
use nus_vt::input::{self, Key, KeyAction, Mods};
use nus_vt::Term;
use winit::event::{ElementState, KeyEvent as WKeyEvent, MouseButton, MouseScrollDelta};
use winit::event_loop::EventLoopProxy;
use winit::keyboard::{Key as WKey, KeyCode, ModifiersState, NamedKey, PhysicalKey};
use winit::window::Window;

use crate::browser::{BrowserTab, Shared, SharedRef};
use crate::UserEvent;

const SCROLLBACK: usize = 10_000;

/// Platform key label: "⌘K" on macOS, "CTRL+K" elsewhere. `shift` adds ⇧ / SHIFT+.
fn key(k: &str, shift: bool) -> String {
    if cfg!(target_os = "macos") {
        format!("⌘{}{}", if shift { "⇧" } else { "" }, k)
    } else {
        format!("CTRL+{}{}", if shift { "SHIFT+" } else { "" }, k)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PaletteMode {
    /// ⌘K: tabs, actions, then URL/search.
    Go,
    /// ⌘T: profiles for a terminal tab, or a URL/search for a browser tab.
    New,
    /// ⌘L: navigate the browser pane.
    Url,
}

#[derive(Clone, PartialEq, Debug)]
pub enum Action {
    SwitchTab(usize),
    NewTerminal(usize),
    NewBrowser(String),
    OpenInPane(String),
    ToggleSplit,
    CloseTab,
    ToggleSidebar,
    TogglePin,
    Reopen,
}

pub enum Closed {
    Term(usize),
    Web(String),
}

pub struct PaletteRow {
    pub num: String,
    pub text: String,
    pub action: Action,
}

pub struct Fonts {
    pub ui: FontId,
    pub strong: FontId,
    pub wordmark: FontId,
    pub term: FontId,
}

pub struct TermPane {
    pub term: Term,
    pub pty: nus_pty::Pty,
    pub grid: GridRenderer,
    pub title: String,
    pub profile: usize,
    pub rect: Rect,
    pub origin: (f32, f32),
    /// What has been typed at the current prompt, for the URL rule.
    pub line: String,
    /// False once an editing key made `line` unreliable; reset on Enter.
    pub line_ok: bool,
    /// Name of the running process when a close is awaiting confirmation.
    pub confirm_close: Option<String>,
}

pub struct WebPane {
    pub tab: BrowserTab,
    pub page: Rect,
    pub rect: Rect,
}

pub enum Pane {
    Term(TermPane),
    Web(WebPane),
}

pub struct Tab {
    pub left: Pane,
    pub right: Option<Pane>,
    pub focus_right: bool,
    pub pinned: bool,
}

impl Tab {
    fn focused(&mut self) -> &mut Pane {
        if self.focus_right && self.right.is_some() {
            self.right.as_mut().unwrap()
        } else {
            &mut self.left
        }
    }
    fn title(&self) -> String {
        let name = |p: &Pane| match p {
            Pane::Term(t) => t.title.clone(),
            Pane::Web(w) => {
                let s = w.tab.shared.borrow();
                if s.title.is_empty() {
                    s.url.clone()
                } else {
                    s.title.clone()
                }
            }
        };
        match &self.right {
            Some(r) => format!("{} | {}", name(&self.left), name(r)),
            None => name(&self.left),
        }
    }
}

pub struct App {
    pub window: Arc<Window>,
    pub gpu: Gpu,
    pub fonts: FontSystem,
    pub f: Fonts,
    pub scene: Scene,
    pub theme: Theme,
    pub scale: f32,
    pub proxy: EventLoopProxy<UserEvent>,
    pub device: wgpu::Device,
    pub bind_texture: Rc<dyn Fn(&wgpu::Texture) -> Arc<wgpu::BindGroup>>,

    pub space_name: String,
    pub signal: nus_render::Color,
    pub tabs: Vec<Tab>,
    pub active: usize,
    pub sidebar: bool,
    pub palette: Option<(PaletteMode, String)>,
    pub palette_sel: usize,
    pub profiles: Vec<nus_pty::Profile>,
    /// Tab indices, most recently used first.
    pub mru: Vec<usize>,
    pub selected: std::collections::HashSet<usize>,
    pub closed: Vec<Closed>,

    pub mods: ModifiersState,
    pub mouse: (f32, f32),
    pub mouse_down_in_web: bool,
    pub detected: Option<(usize, usize, String)>,
    pub dirty: bool,
    /// Terminal resizes are applied once the window size has been stable
    /// for a moment; winit emits a burst of sizes on creation and conhost
    /// scrambles its buffer if it sees every one of them.
    pub resize_due: Option<Instant>,
    pub last_begin_frame: Instant,
    pub frames: u64,
}

impl App {
    pub fn new(window: Arc<Window>, proxy: EventLoopProxy<UserEvent>) -> anyhow::Result<App> {
        let gpu = Gpu::new(window.clone())?;
        let scale = window.scale_factor() as f32;
        let mut fonts = FontSystem::new();
        let ui = fonts.load_bytes(nus_render::text::bundled::PLEX_MONO, 0)?;
        let strong = fonts.load_bytes(nus_render::text::bundled::PLEX_MONO_SEMIBOLD, 0)?;
        let wordmark = fonts.load_bytes(nus_render::text::bundled::NEWSREADER_ITALIC, 0)?;
        let term_font = ui;
        let theme = match window.theme() {
            Some(winit::window::Theme::Light) => Theme::paper(),
            _ => Theme::ink(),
        };
        let device = gpu.device.clone();
        let bind_texture: Rc<dyn Fn(&wgpu::Texture) -> Arc<wgpu::BindGroup>> = {
            // Gpu::bind_texture needs &Gpu; we recreate the pieces it needs via a
            // closure over a second handle built at init.
            let g = gpu.texture_binder();
            Rc::new(move |t: &wgpu::Texture| g.bind(t))
        };
        let mut app = App {
            window,
            gpu,
            fonts,
            f: Fonts {
                ui,
                strong,
                wordmark,
                term: term_font,
            },
            scene: Scene::new(),
            theme,
            scale,
            proxy,
            device,
            bind_texture,
            space_name: "nus".into(),
            signal: signal::RED,
            tabs: Vec::new(),
            active: 0,
            sidebar: true,
            palette: None,
            palette_sel: 0,
            profiles: nus_pty::Profile::discover(),
            mru: vec![0],
            selected: Default::default(),
            closed: Vec::new(),
            mods: ModifiersState::empty(),
            mouse: (0.0, 0.0),
            mouse_down_in_web: false,
            detected: None,
            dirty: true,
            resize_due: None,
            last_begin_frame: Instant::now(),
            frames: 0,
        };
        let term = app.new_term_pane(true, 0)?;
        let web = app.new_web_pane("https://docs.rs/wgpu/latest/wgpu/");
        app.tabs.push(Tab {
            left: Pane::Term(term),
            right: web.map(Pane::Web),
            focus_right: false,
            pinned: false,
        });
        app.layout();
        app.apply_term_resizes(true);
        Ok(app)
    }

    fn px(&self, v: f32) -> f32 {
        (v * self.scale).round()
    }

    /// Spawn a shell sized for the left pane (split or not), so ConPTY never
    /// sees a resize during startup.
    fn new_term_pane(&mut self, split: bool, profile: usize) -> anyhow::Result<TermPane> {
        let term_px = 13.0 * self.scale * 96.0 / 72.0;
        let grid = GridRenderer::new(&self.fonts, self.f.term, term_px);
        let c = self.content_rect();
        let w = if split { c.w - self.px(m::SPLIT) - self.px(m::STRUCTURE) } else { c.w };
        let area = Rect::new(
            c.x + self.px(18.0),
            c.y + self.header_h() + self.px(16.0),
            w - 2.0 * self.px(18.0),
            c.h - self.header_h() - 2.0 * self.px(16.0),
        );
        let (cols, rows) = grid.grid_size(area);
        let mut term = Term::new(cols, rows, SCROLLBACK);
        self.theme.apply(&mut term.palette);
        let (cw, ch) = grid.cell_size();
        term.cell_px = (cw as u16, ch as u16);
        let profile_index = profile;
        let profile = self.profiles.get(profile).cloned().unwrap_or_else(nus_pty::Profile::default_shell);
        let proxy = self.proxy.clone();
        let pty = nus_pty::Pty::spawn(&profile, cols as u16, rows as u16, move || {
            let _ = proxy.send_event(UserEvent::Wake);
        })?;
        Ok(TermPane {
            term,
            pty,
            grid,
            title: profile.name,
            profile: profile_index,
            rect: Rect::new(0.0, 0.0, 1.0, 1.0),
            origin: (0.0, 0.0),
            line: String::new(),
            line_ok: true,
            confirm_close: None,
        })
    }

    fn new_web_pane(&mut self, url: &str) -> Option<WebPane> {
        let shared: SharedRef = Rc::new(std::cell::RefCell::new(Shared {
            scale: self.scale,
            size: (100.0, 100.0),
            ..Default::default()
        }));
        let tab = BrowserTab::create(url, shared, self.device.clone(), self.bind_texture.clone())?;
        Some(WebPane {
            tab,
            page: Rect::new(0.0, 0.0, 1.0, 1.0),
            rect: Rect::new(0.0, 0.0, 1.0, 1.0),
        })
    }

    // --- layout ----------------------------------------------------------

    pub fn strip_rect(&self) -> Rect {
        Rect::new(0.0, self.px(m::BAND), self.gpu.size.0 as f32, self.px(m::TOP_STRIP))
    }

    fn content_rect(&self) -> Rect {
        let top = self.px(m::BAND) + self.px(m::TOP_STRIP) + self.px(m::STRUCTURE);
        let left = if self.sidebar {
            self.px(m::SIDEBAR) + self.px(m::STRUCTURE)
        } else {
            self.px(4.0)
        };
        Rect::new(left, top, self.gpu.size.0 as f32 - left, self.gpu.size.1 as f32 - top)
    }

    fn sidebar_rect(&self) -> Rect {
        let c = self.content_rect();
        Rect::new(0.0, c.y, self.px(m::SIDEBAR), c.h)
    }

    fn header_h(&self) -> f32 {
        self.px(m::HEADER_PAD_Y) * 2.0 + self.px(m::UI_PX) + self.px(m::HAIRLINE)
    }

    pub fn layout(&mut self) {
        let c = self.content_rect();
        let split_w = self.px(m::SPLIT);
        let rule = self.px(m::STRUCTURE);
        let header = self.header_h();
        let pad_x = self.px(18.0);
        let pad_y = self.px(16.0);
        let scale = self.scale;
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        let (left_rect, right_rect) = if tab.right.is_some() {
            (
                Rect::new(c.x, c.y, c.w - split_w - rule, c.h),
                Some(Rect::new(c.right() - split_w, c.y, split_w, c.h)),
            )
        } else {
            (c, None)
        };
        let place = |pane: &mut Pane, r: Rect| match pane {
            Pane::Term(t) => {
                t.rect = r;
                let area = Rect::new(r.x + pad_x, r.y + header + pad_y, r.w - 2.0 * pad_x, r.h - header - 2.0 * pad_y);
                t.origin = (area.x, area.y);
                let _ = area; // terminal size is applied by `apply_term_resizes`
            }
            Pane::Web(w) => {
                w.rect = r;
                let url_row = (6.0 * 2.0 + 22.0) * scale;
                let tools_row = (8.0 * 2.0 + 13.0 + 1.0) * scale;
                w.page = Rect::new(r.x, r.y + url_row.round() + 1.0, r.w, r.h - url_row.round() - 1.0 - tools_row.round());
                {
                    let mut s = w.tab.shared.borrow_mut();
                    s.origin = (w.page.x, w.page.y);
                    s.scale = scale;
                }
                w.tab.resized((w.page.w / scale).floor(), (w.page.h / scale).floor());
            }
        };
        place(&mut tab.left, left_rect);
        if let (Some(r), Some(rr)) = (tab.right.as_mut(), right_rect) {
            place(r, rr);
        }
        self.dirty = true;
    }

    pub fn resize(&mut self, w: u32, h: u32) {
        self.gpu.resize(w, h);
        self.layout();
        self.resize_due = Some(Instant::now() + std::time::Duration::from_millis(80));
    }

    /// Resize terminals to their panes once the window has settled.
    pub fn apply_term_resizes(&mut self, force: bool) {
        match self.resize_due {
            Some(t) if force || Instant::now() >= t => self.resize_due = None,
            Some(_) => return,
            None if force => {}
            None => return,
        }
        let header = self.header_h();
        let pad_x = self.px(18.0);
        let pad_y = self.px(16.0);
        let mut changed = false;
        for tab in &mut self.tabs {
            for p in std::iter::once(&mut tab.left).chain(tab.right.as_mut()) {
                if let Pane::Term(t) = p {
                    let r = t.rect;
                    let area = Rect::new(r.x + pad_x, r.y + header + pad_y, r.w - 2.0 * pad_x, r.h - header - 2.0 * pad_y);
                    let (cols, rows) = t.grid.grid_size(area);
                    if (cols, rows) != (t.term.cols(), t.term.rows()) {
                        t.term.resize(cols, rows);
                        let (cw, ch) = t.grid.cell_size();
                        let _ = t.pty.resize(cols as u16, rows as u16, (cw as u16, ch as u16));
                        changed = true;
                    }
                }
            }
        }
        if changed {
            self.dirty = true;
        }
    }

    pub fn set_scale(&mut self, scale: f32) {
        self.scale = scale;
        let term_px = 13.0 * scale * 96.0 / 72.0;
        for tab in &mut self.tabs {
            for p in std::iter::once(&mut tab.left).chain(tab.right.as_mut()) {
                if let Pane::Term(t) = p {
                    t.grid.set_font(&self.fonts, self.f.term, term_px);
                }
            }
        }
        self.layout();
    }

    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
        for tab in &mut self.tabs {
            for p in std::iter::once(&mut tab.left).chain(tab.right.as_mut()) {
                if let Pane::Term(t) = p {
                    self.theme.apply(&mut t.term.palette);
                    t.term.grid_mut().damage_all();
                }
            }
        }
        self.dirty = true;
    }

    // --- per-frame -------------------------------------------------------

    /// Drain PTYs, tick terminals, detect URLs. Returns true if anything changed.
    pub fn pump(&mut self) -> bool {
        let mut changed = false;
        let mut detected = None;
        for (i, tab) in self.tabs.iter_mut().enumerate() {
            for p in std::iter::once(&mut tab.left).chain(tab.right.as_mut()) {
                if let Pane::Term(t) = p {
                    let out = t.pty.take_output();
                    if !out.is_empty() {
                        if let Ok(p) = std::env::var("NUS_DUMP") {
                            use std::io::Write;
                            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {
                                let _ = f.write_all(&out);
                            }
                        }
                        t.term.advance(&out);
                        changed = true;
                    }
                    t.term.tick();
                    let r = t.term.take_responses();
                    if !r.is_empty() {
                        let _ = t.pty.write(&r);
                    }
                    for ev in t.term.take_events() {
                        if let nus_vt::Event::Title(title) = ev {
                            t.title = short_title(&title);
                            changed = true;
                        }
                    }
                    if t.term.grid().is_damaged() {
                        changed = true;
                    }
                    if i == self.active {
                        detected = detect_localhost(&t.term);
                    }
                }
            }
        }
        if detected != self.detected {
            self.detected = detected;
            changed = true;
        }
        changed
    }

    pub fn begin_frames(&mut self) {
        if self.last_begin_frame.elapsed().as_millis() < 16 {
            return;
        }
        self.last_begin_frame = Instant::now();
        if let Some(tab) = self.tabs.get(self.active) {
            for p in std::iter::once(&tab.left).chain(tab.right.as_ref()) {
                if let Pane::Web(w) = p {
                    w.tab.begin_frame();
                }
            }
        }
    }

    pub fn redraw(&mut self) {
        let changed = self.pump();
        if !(changed || self.dirty || self.frames == 0) {
            return;
        }
        self.dirty = false;
        self.build();
        self.scene.finish();
        for (x, y, w, h, data) in self.fonts.uploads.drain(..) {
            self.gpu.upload_glyph(x, y, w, h, &data);
        }
        self.window.pre_present_notify();
        let clear = self.theme.paper;
        self.gpu.render(&self.scene, clear);
        self.frames += 1;
    }

    // --- drawing ---------------------------------------------------------

    fn label(&self) -> Style {
        Style {
            font: self.f.ui,
            px: self.px(m::LABEL_PX),
            color: self.theme.ink,
            tracking: self.px(m::LABEL_PX) * m::LABEL_TRACKING,
        }
    }
    fn label_strong(&self) -> Style {
        Style {
            font: self.f.strong,
            ..self.label()
        }
    }
    fn ui(&self) -> Style {
        Style {
            font: self.f.ui,
            px: self.px(m::UI_PX),
            color: self.theme.ink,
            tracking: 0.0,
        }
    }
    fn ui_strong(&self) -> Style {
        Style {
            font: self.f.strong,
            ..self.ui()
        }
    }

    fn build(&mut self) {
        let mut scene = std::mem::take(&mut self.scene);
        scene.clear();
        let t = self.theme.clone();
        let ink = t.ink;
        let w = self.gpu.size.0 as f32;
        let h = self.gpu.size.1 as f32;

        // Signal band + top strip.
        scene.layer(None);
        scene.rect(Rect::new(0.0, 0.0, w, self.px(m::BAND)), self.signal);
        let strip = self.strip_rect();
        scene.hline(0.0, strip.bottom(), w, self.px(m::STRUCTURE), ink);
        let mut x = self.px(18.0);
        let base = strip.y + self.px(21.0);
        let wm = Style {
            font: self.f.wordmark,
            px: self.px(m::WORDMARK_PX),
            color: ink,
            tracking: 0.0,
        };
        x += self.fonts.draw(&mut scene, wm, x, base, "nus") + self.px(18.0);
        let crumb = {
            let tab = &self.tabs[self.active];
            let cwd = match &tab.left {
                Pane::Term(t) => t.title.clone(),
                Pane::Web(_) => String::new(),
            };
            format!("{} · {:02} {} · {}", self.space_name, self.active + 1, tab.title(), cwd)
        };
        let label = self.label();
        self.fonts.draw(&mut scene, label, x, strip.y + self.px(19.0), &crumb.to_uppercase(), );
        // Right side: ⌘K and window controls.
        let ui = self.ui();
        let mut rx = w - self.px(18.0);
        for glyph in ["✕", "▢", "—"] {
            let gw = self.fonts.measure(ui, glyph);
            rx -= gw;
            self.fonts.draw(&mut scene, ui, rx, strip.y + self.px(20.0), glyph);
            rx -= self.px(14.0);
        }
        rx -= self.px(4.0);
        let k = key("K", false);
        rx -= self.fonts.measure(label, &k);
        self.fonts.draw(&mut scene, label, rx, strip.y + self.px(19.0), &k);

        // Sidebar or hot edge.
        let c = self.content_rect();
        if self.sidebar {
            self.draw_sidebar(&mut scene);
            scene.vline(c.x - self.px(m::STRUCTURE), c.y, c.h, self.px(m::STRUCTURE), ink);
        } else {
            scene.rect(Rect::new(0.0, c.y, self.px(4.0), c.h), t.hot);
        }

        // Panes.
        let active = self.active;
        let focus_right = self.tabs[active].focus_right;
        let has_right = self.tabs[active].right.is_some();
        // Split rule.
        if has_right {
            let r = match &self.tabs[active].right {
                Some(Pane::Term(p)) => p.rect,
                Some(Pane::Web(p)) => p.rect,
                None => unreachable!(),
            };
            scene.vline(r.x - self.px(m::STRUCTURE), r.y, r.h, self.px(m::STRUCTURE), ink);
        }
        let mut tabs = std::mem::take(&mut self.tabs);
        {
            let tab = &mut tabs[active];
            let n = active + 1;
            let left_focused = !(focus_right && has_right);
            self.draw_pane(&mut scene, &mut tab.left, n, left_focused);
            if let Some(r) = tab.right.as_mut() {
                self.draw_pane(&mut scene, r, n, !left_focused);
            }
        }
        self.tabs = tabs;

        // localhost chip, anchored under the detected line.
        if let Some((row, col, url)) = self.detected.clone() {
            if let Pane::Term(t) = &self.tabs[active].left {
                if row == t.term.cursor().row && t.line_ok && strict_url(&t.line).is_some() {
                    // handled by the typed-line hint below
                } else {
                let (cw, ch) = t.grid.cell_size();
                let x = t.origin.0 + col as f32 * cw;
                let y = t.origin.1 + (row as f32 + 1.0) * ch + self.px(6.0);
                let (k1, k2) = (key("ENTER", false), key("ENTER", true));
                let short = url.trim_start_matches("http://").to_string();
                let parts = [(short, true), ("open split".into(), false), (k1, true), ("·".into(), false), ("new tab".into(), false), (k2, true)];
                self.chip(&mut scene, x, y, &parts);
                }
            }
        }
        // URL-at-a-prompt hint, under the cursor line.
        if let Pane::Term(t) = &self.tabs[active].left {
            if t.line_ok && strict_url(&t.line).is_some() && !(focus_right && has_right) {
                let (cw, ch) = t.grid.cell_size();
                let c = t.term.cursor();
                let x = t.origin.0 + (c.col.saturating_sub(t.line.chars().count())) as f32 * cw;
                let y = t.origin.1 + (c.row as f32 + 1.0) * ch + self.px(6.0);
                let parts = [
                    ("enter".into(), true),
                    ("opens in browser".into(), false),
                    ("·".into(), false),
                    (key("ENTER", false), true),
                    ("runs in shell".into(), false),
                ];
                self.chip(&mut scene, x, y, &parts);
            }
        }

        // Palette.
        if let Some((mode, input)) = self.palette.clone() {
            scene.layer(None);
            scene.rect(Rect::new(0.0, 0.0, w, h), t.scrim);
            let pw = self.px(m::PALETTE);
            let bx = ((w - pw) / 2.0).round();
            let by = self.px(220.0);
            let rows = self.palette_rows(mode, &input);
            let row_h = self.px(10.0) * 2.0 + self.px(m::UI_PX) + self.px(m::HAIRLINE);
            let head_h = self.px(14.0) * 2.0 + self.px(16.0) + self.px(2.0);
            let bh = head_h + row_h * rows.len().max(1) as f32;
            let r = Rect::new(bx, by, pw, bh);
            scene.rect(Rect::new(r.x + self.px(8.0), r.y + self.px(8.0), r.w, r.h), ink);
            scene.rect(r, t.paper);
            scene.outline(r, self.px(m::FLOATING), ink);
            // input row
            let wm = Style {
                font: self.f.wordmark,
                px: self.px(20.0),
                color: ink,
                tracking: 0.0,
            };
            let base = r.y + self.px(14.0) + self.px(16.0);
            let mut px = r.x + self.px(18.0);
            let word = match mode { PaletteMode::Go => "go", PaletteMode::New => "new", PaletteMode::Url => "url" };
            px += self.fonts.draw(&mut scene, wm, px, base, word) + self.px(12.0);
            let big = Style {
                font: self.f.ui,
                px: self.px(16.0),
                color: ink,
                tracking: 0.0,
            };
            px += self.fonts.draw(&mut scene, big, px, base, &input);
            scene.rect(Rect::new(px + self.px(2.0), base - self.px(14.0), self.px(9.0), self.px(18.0)), ink);
            let esc = self.label();
            let ew = self.fonts.measure(esc, "ESC");
            self.fonts.draw(&mut scene, esc, r.right() - self.px(18.0) - ew, base - self.px(2.0), "ESC");
            scene.hline(r.x, r.y + head_h - self.px(2.0), r.w, self.px(2.0), ink);
            // rows
            let mut y = r.y + head_h;
            for (i, PaletteRow { num, text, .. }) in rows.iter().enumerate() {
                let sel = i == self.palette_sel;
                let (fg, bg) = if sel { (t.paper, Some(ink)) } else { (ink, None) };
                if let Some(bg) = bg {
                    scene.rect(Rect::new(r.x, y, r.w, row_h), bg);
                }
                let strong = Style { color: fg, ..self.ui_strong() };
                let ui = Style { color: fg, ..self.ui() };
                let base = y + self.px(10.0) + self.px(m::UI_PX) - self.px(3.0);
                let mut px = r.x + self.px(18.0);
                px += self.fonts.draw(&mut scene, strong, px, base, num) + self.px(12.0);
                self.fonts.draw(&mut scene, ui, px, base, text);
                if !sel {
                    scene.hline(r.x, y + row_h - self.px(m::HAIRLINE), r.w, self.px(m::HAIRLINE), t.tint);
                }
                y += row_h;
            }
        }
        self.scene = scene;
    }

    /// A ruled caps chip with a hard 4px shadow. `parts`: (text, strong).
    fn chip(&mut self, scene: &mut Scene, x: f32, y: f32, parts: &[(String, bool)]) {
        let t = self.theme.clone();
        let label = self.label();
        let strong = self.label_strong();
        let gap = self.px(14.0);
        let padx = self.px(12.0);
        let pady = self.px(8.0);
        scene.layer(None);
        let text_w: f32 = parts
            .iter()
            .map(|(s, b)| self.fonts.measure(if *b { strong } else { label }, &s.to_uppercase()))
            .sum::<f32>()
            + gap * (parts.len() as f32 - 1.0);
        let box_h = pady * 2.0 + self.px(m::LABEL_PX);
        let r = Rect::new(x, y, text_w + padx * 2.0, box_h);
        scene.rect(Rect::new(r.x + self.px(4.0), r.y + self.px(4.0), r.w, r.h), t.ink);
        scene.rect(r, t.paper);
        scene.outline(r, self.px(m::STRUCTURE), t.ink);
        let mut px = r.x + padx;
        let by = r.y + pady + self.px(m::LABEL_PX) - self.px(2.0);
        for (s, b) in parts {
            px += self.fonts.draw(scene, if *b { strong } else { label }, px, by, &s.to_uppercase()) + gap;
        }
    }

    /// Sidebar rows: (pinned tab indices, listed tab indices, pinned row height, list row height, list top y).
    fn sidebar_geometry(&self) -> (Vec<usize>, Vec<usize>, f32, f32, f32) {
        let sb = self.sidebar_rect();
        let space_row = self.px(9.0) * 2.0 + self.px(m::LABEL_PX) + self.px(m::STRUCTURE);
        let pinned: Vec<usize> = (0..self.tabs.len()).filter(|&i| self.tabs[i].pinned).collect();
        let listed: Vec<usize> = (0..self.tabs.len()).filter(|&i| !self.tabs[i].pinned).collect();
        let pinned_h = if pinned.is_empty() { 0.0 } else { self.px(9.0) * 2.0 + self.px(m::UI_PX) + self.px(m::STRUCTURE) };
        let row_h = self.px(m::ROW_PAD_Y) * 2.0 + self.px(m::UI_PX) + self.px(8.0) + self.px(m::PREVIEW_H) + self.px(m::HAIRLINE);
        (pinned, listed, pinned_h, row_h, sb.y + space_row + pinned_h)
    }

    fn draw_sidebar(&mut self, scene: &mut Scene) {
        let t = self.theme.clone();
        let ink = t.ink;
        let sb = self.sidebar_rect();
        let label = self.label();
        let strong = self.label_strong();
        // Space row (one real Space; "+" cell for the next).
        let row_h = self.px(9.0) * 2.0 + self.px(m::LABEL_PX) + self.px(m::STRUCTURE);
        let cell_w = (sb.w / 3.0).floor();
        scene.rect(Rect::new(sb.x, sb.y, cell_w, row_h - self.px(m::STRUCTURE)), ink);
        scene.rect(Rect::new(sb.x + self.px(10.0), sb.y + self.px(10.0), self.px(10.0), self.px(10.0)), self.signal);
        let sel = Style { color: t.paper, ..label };
        self.fonts.draw(scene, sel, sb.x + self.px(28.0), sb.y + self.px(19.0), &self.space_name.to_uppercase());
        scene.vline(sb.x + cell_w, sb.y, row_h, self.px(m::HAIRLINE), ink);
        let dimmed = Style { color: t.dim, ..label };
        self.fonts.draw(scene, dimmed, sb.x + cell_w + self.px(10.0), sb.y + self.px(19.0), "+ SPACE");
        scene.hline(sb.x, sb.y + row_h - self.px(m::STRUCTURE), sb.w, self.px(m::STRUCTURE), ink);

        // Pinned row: compact cells, one per pinned tab.
        let (pinned, listed, pinned_h, row_h, list_y) = self.sidebar_geometry();
        let pad_x = self.px(m::ROW_PAD_X);
        let pad_y = self.px(m::ROW_PAD_Y);
        let preview_h = self.px(m::PREVIEW_H);
        let ui = self.ui();
        let ui_strong = self.ui_strong();
        let tabs = std::mem::take(&mut self.tabs);
        if !pinned.is_empty() {
            let py = sb.y + row_h;
            let cell_w = (sb.w / pinned.len() as f32).floor();
            for (k, &i) in pinned.iter().enumerate() {
                let cx = sb.x + k as f32 * cell_w;
                let cell = Rect::new(cx, py, cell_w, pinned_h - self.px(m::STRUCTURE));
                let sel_fill = i == self.active;
                if sel_fill {
                    scene.rect(cell, ink);
                }
                let st = Style { color: if sel_fill { t.paper } else { ink }, ..ui_strong };
                let title = self.fit(st, &tabs[i].title(), cell_w - self.px(20.0) - self.px(28.0));
                let base = py + self.px(9.0) + self.px(m::UI_PX) - self.px(3.0);
                let mut x = cx + self.px(10.0);
                x += self.fonts.draw(scene, st, x, base, &format!("{:02}", i + 1)) + self.px(8.0);
                self.fonts.draw(scene, st, x, base, &title);
                if self.selected.contains(&i) {
                    scene.outline(cell, self.px(m::STRUCTURE), ink);
                }
                if k + 1 < pinned.len() {
                    scene.vline(cx + cell_w, py, pinned_h, self.px(m::HAIRLINE), ink);
                }
            }
            scene.hline(sb.x, py + pinned_h - self.px(m::STRUCTURE), sb.w, self.px(m::STRUCTURE), ink);
        }
        // Tab rows.
        let mut y = list_y;
        for &i in &listed {
            let tab = &tabs[i];
            if i == self.active {
                scene.rect(Rect::new(sb.x, y, sb.w, row_h), t.tint);
            }
            if self.selected.contains(&i) {
                scene.outline(Rect::new(sb.x, y, sb.w, row_h - self.px(m::HAIRLINE)), self.px(m::STRUCTURE), ink);
            }
            let base = y + pad_y + self.px(m::UI_PX) - self.px(3.0);
            let mut x = sb.x + pad_x;
            x += self.fonts.draw(scene, ui_strong, x, base, &format!("{:02}", i + 1)) + self.px(10.0);
            let title = tab.title();
            let max_w = sb.w - (x - sb.x) - pad_x - self.px(60.0);
            let title = self.fit(ui_strong, &title, max_w);
            self.fonts.draw(scene, ui_strong, x, base, &title);
            let tag = if tab.right.is_some() { "SPLIT →" } else { "" };
            if !tag.is_empty() {
                let tw = self.fonts.measure(label, tag);
                self.fonts.draw(scene, label, sb.right() - pad_x - tw, base, tag);
            }
            // Preview box.
            let pr = Rect::new(sb.x + pad_x, base + self.px(8.0), sb.w - 2.0 * pad_x, preview_h);
            scene.outline(pr, self.px(m::HAIRLINE), ink);
            match &tab.left {
                Pane::Term(tp) => {
                    let small = Style {
                        font: self.f.ui,
                        px: self.px(7.5),
                        color: ink,
                        tracking: 0.0,
                    };
                    let lines = last_lines(&tp.term, 4);
                    let lh = self.px(7.5 * 1.5);
                    let mut ly = pr.y + self.px(6.0) + self.px(7.5);
                    scene.layer(Some(pr.inset(1.0)));
                    for l in lines {
                        self.fonts.draw(scene, small, pr.x + self.px(8.0), ly, &l);
                        ly += lh;
                    }
                    scene.layer(None);
                }
                Pane::Web(wp) => {
                    if let Some(bind) = wp.tab.shared.borrow().bind.clone() {
                        let inner = pr.inset(1.0);
                        let aspect = wp.page.w / wp.page.h.max(1.0);
                        let tw = (inner.h * aspect).min(inner.w);
                        scene.texture(Rect::new(inner.x, inner.y, tw, inner.h), bind, Some(inner));
                        scene.layer(None);
                    }
                }
            }
            y += row_h;
            scene.hline(sb.x, y - self.px(m::HAIRLINE), sb.w, self.px(m::HAIRLINE), ink);
        }
        self.tabs = tabs;
        // + new tab, pinned to bottom.
        let foot_h = pad_y * 2.0 + self.px(m::LABEL_PX);
        let fy = sb.bottom() - foot_h;
        scene.hline(sb.x, fy, sb.w, self.px(m::STRUCTURE), ink);
        let base = fy + pad_y + self.px(m::LABEL_PX) - self.px(2.0);
        self.fonts.draw(scene, label, sb.x + pad_x, base, "+ NEW TAB");
        let kt = key("T", true);
        let kw = self.fonts.measure(strong, &kt);
        self.fonts.draw(scene, strong, sb.right() - pad_x - kw, base, &kt);
        let _ = ui;
    }

    fn draw_pane(&mut self, scene: &mut Scene, pane: &mut Pane, n: usize, focused: bool) {
        let t = self.theme.clone();
        let ink = t.ink;
        let label = self.label();
        let strong = self.label_strong();
        match pane {
            Pane::Term(p) => {
                let r = p.rect;
                let hh = self.header_h();
                let base = r.y + self.px(m::HEADER_PAD_Y) + self.px(m::UI_PX) - self.px(3.0);
                let mut x = r.x + self.px(m::HEADER_PAD_X);
                x += self.fonts.draw(scene, strong, x, base, &format!("{:02} · {}", n, p.title).to_uppercase()) + self.px(14.0);
                let dims = format!("{}×{}", p.term.cols(), p.term.rows());
                let dw = self.fonts.measure(label, &dims);
                self.fonts.draw(scene, label, r.right() - self.px(m::HEADER_PAD_X) - dw, base, &dims);
                let _ = x;
                scene.hline(r.x, r.y + hh - self.px(m::HAIRLINE), r.w, self.px(m::HAIRLINE), ink);
                if let Some(proc_name) = p.confirm_close.clone() {
                    let cr = Rect::new(r.x, r.y + hh, r.w, hh);
                    scene.rect(cr, ink);
                    let inv = Style { color: t.paper, ..strong };
                    let inv_l = Style { color: t.paper, ..label };
                    let by = cr.y + self.px(m::HEADER_PAD_Y) + self.px(m::UI_PX) - self.px(3.0);
                    let mut x = cr.x + self.px(m::HEADER_PAD_X);
                    x += self.fonts.draw(scene, inv, x, by, &format!("{} IS RUNNING", proc_name.to_uppercase())) + self.px(14.0);
                    x += self.fonts.draw(scene, inv_l, x, by, "CLOSE ANYWAY?") + self.px(14.0);
                    x += self.fonts.draw(scene, inv, x, by, "ENTER") + self.px(14.0);
                    self.fonts.draw(scene, inv_l, x, by, "· ESC CANCELS");
                }
                let clip = Rect::new(r.x, r.y + hh, r.w, r.h - hh);
                scene.layer(Some(clip));
                p.grid.draw(scene, &mut self.fonts, &p.term, p.origin, focused);
                let _ = p.term.grid_mut().take_damage();
                scene.layer(None);
            }
            Pane::Web(p) => {
                let r = p.rect;
                let s = p.tab.shared.borrow();
                let (url, bind, loading) = (s.url.clone(), s.bind.clone(), s.loading);
                drop(s);
                // URL row.
                let base = r.y + self.px(6.0) + self.px(22.0) - self.px(6.0);
                let mut x = r.x + self.px(14.0);
                let ui_strong = self.ui_strong();
                let ui = self.ui();
                x += self.fonts.draw(scene, ui_strong, x, base, "←") + self.px(14.0);
                let dt = "DEVTOOLS";
                let dw = self.fonts.measure(label, dt);
                let field = Rect::new(x, r.y + self.px(6.0), r.right() - self.px(14.0) - dw - self.px(14.0) - x, self.px(22.0));
                scene.outline(field, self.px(m::HAIRLINE), ink);
                let shown = self.fit(ui, url.trim_start_matches("https://").trim_start_matches("http://").trim_end_matches('/'), field.w - self.px(16.0));
                let small = Style { px: self.px(12.0), ..ui };
                self.fonts.draw(scene, small, field.x + self.px(8.0), base, &shown);
                self.fonts.draw(scene, label, r.right() - self.px(14.0) - dw, base - self.px(1.0), dt);
                scene.hline(r.x, p.page.y - 1.0, r.w, self.px(m::HAIRLINE), ink);
                // Page.
                scene.rect(p.page, t.page);
                if let Some(bind) = bind {
                    scene.texture(p.page, bind, Some(p.page));
                    scene.layer(None);
                }
                if loading {
                    scene.rect(Rect::new(p.page.x, p.page.y, p.page.w * 0.5, self.px(2.0)), self.signal);
                }
                // Devtools row.
                let ty = p.page.bottom();
                scene.hline(r.x, ty, r.w, self.px(m::HAIRLINE), ink);
                let base = ty + self.px(8.0) + self.px(m::LABEL_PX);
                let mut x = r.x + self.px(14.0);
                for (i, s) in ["CONSOLE", "NETWORK", "ELEMENTS"].iter().enumerate() {
                    let st = if i == 0 { strong } else { label };
                    let w = self.fonts.draw(scene, st, x, base, s);
                    if i == 0 {
                        scene.hline(x, base + self.px(3.0), w, self.px(1.5), ink);
                    }
                    x += w + self.px(16.0);
                }
                let live = if focused { "■ LIVE · FOCUSED" } else { "■ LIVE" };
                let lw = self.fonts.measure(label, live);
                self.fonts.draw(scene, label, r.right() - self.px(14.0) - lw, base, live);
            }
        }
    }

    fn fit(&self, style: Style, text: &str, max_w: f32) -> String {
        if self.fonts.measure(style, text) <= max_w {
            return text.to_string();
        }
        let mut s: String = text.chars().collect();
        while !s.is_empty() && self.fonts.measure(style, &format!("{s}…")) > max_w {
            s.pop();
        }
        format!("{s}…")
    }

    fn url_or_search(input: &str) -> (String, String) {
        let q = input.trim();
        if q.contains("://") {
            (q.to_string(), format!("open {q}"))
        } else if q.contains('.') && !q.contains(' ') || q.starts_with("localhost") {
            (format!("http://{q}"), format!("open {q}"))
        } else {
            (
                format!("https://www.google.com/search?q={}", q.replace(' ', "+")),
                format!("search \u{201c}{q}\u{201d}"),
            )
        }
    }

    fn palette_rows(&self, mode: PaletteMode, input: &str) -> Vec<PaletteRow> {
        let q = input.trim().to_lowercase();
        let hit = |s: &str| q.is_empty() || s.to_lowercase().contains(&q);
        let mut rows = Vec::new();
        let row = |num: &str, text: String, action: Action| PaletteRow { num: num.into(), text, action };
        match mode {
            PaletteMode::Go => {
                for (i, t) in self.tabs.iter().enumerate() {
                    if hit(&t.title()) {
                        rows.push(row(&format!("{:02}", i + 1), format!("{} · switch to tab", t.title()), Action::SwitchTab(i)));
                    }
                }
                let actions: [(String, Action); 7] = [
                    (format!("new terminal tab · {}", key("T", true)), Action::NewTerminal(0)),
                    (format!("new browser tab · {} then a URL", key("T", true)), Action::NewBrowser(String::new())),
                    (format!("split with a browser · {}", key("D", true)), Action::ToggleSplit),
                    (format!("close tab · {}", key("W", true)), Action::CloseTab),
                    (format!("sidebar · {}", key("S", true)), Action::ToggleSidebar),
                    (
                        format!("{} this tab", if self.tabs.get(self.active).is_some_and(|t| t.pinned) { "unpin" } else { "pin" }),
                        Action::TogglePin,
                    ),
                    (format!("reopen closed tab · {}", key("Z", true)), Action::Reopen),
                ];
                for (label, a) in actions {
                    if hit(&label) {
                        rows.push(row("·", label, a));
                    }
                }
                if !q.is_empty() {
                    let (url, text) = Self::url_or_search(input);
                    rows.push(row("→", format!("{text} · in the browser pane"), Action::OpenInPane(url)));
                }
            }
            PaletteMode::New => {
                for (i, p) in self.profiles.iter().enumerate() {
                    if hit(&p.name) {
                        rows.push(row(">", format!("terminal · {}", p.name), Action::NewTerminal(i)));
                    }
                }
                if q.is_empty() {
                    rows.push(row("→", "browser · type a URL or search terms".into(), Action::NewBrowser(String::new())));
                } else {
                    let (url, text) = Self::url_or_search(input);
                    rows.push(row("→", format!("browser · {text}"), Action::NewBrowser(url)));
                }
            }
            PaletteMode::Url => {
                if !q.is_empty() {
                    let (url, text) = Self::url_or_search(input);
                    rows.push(row("→", text, Action::OpenInPane(url)));
                }
            }
        }
        rows
    }

    fn open_palette(&mut self, mode: PaletteMode) {
        self.palette = Some((mode, String::new()));
        self.palette_sel = 0;
        self.dirty = true;
    }

    fn run(&mut self, action: Action) {
        match action {
            Action::SwitchTab(i) => self.activate(i),
            Action::NewTerminal(p) => self.new_tab(p),
            Action::NewBrowser(url) if url.is_empty() => self.open_palette(PaletteMode::New),
            Action::NewBrowser(url) => self.open_url(&url, true),
            Action::OpenInPane(url) => self.open_url(&url, false),
            Action::ToggleSplit => self.toggle_split(),
            Action::CloseTab => self.close_tabs(false),
            Action::TogglePin => {
                let t = &mut self.tabs[self.active];
                t.pinned = !t.pinned;
            }
            Action::Reopen => self.reopen_closed(),
            Action::ToggleSidebar => {
                self.sidebar = !self.sidebar;
                self.layout();
            }
        }
        self.dirty = true;
    }

    // --- input -----------------------------------------------------------

    pub fn key(&mut self, ev: &WKeyEvent) {
        let pressed = ev.state == ElementState::Pressed;
        let ctrl = self.mods.control_key();
        let shift = self.mods.shift_key();
        let alt = self.mods.alt_key();
        let sup = self.mods.super_key();
        // App chords: ⌘ on macOS, Ctrl+Shift elsewhere — never reaches the shell.
        let app = if cfg!(target_os = "macos") { sup } else { ctrl && shift };

        // Palette owns the keyboard while open.
        if let Some((_, input)) = self.palette.as_mut() {
            if !pressed {
                return;
            }
            match &ev.logical_key {
                WKey::Named(NamedKey::Escape) => self.palette = None,
                WKey::Named(NamedKey::Enter) => self.palette_commit(),
                WKey::Named(NamedKey::Backspace) => {
                    input.pop();
                    self.palette_sel = 0;
                }
                WKey::Named(NamedKey::ArrowDown) => self.palette_sel += 1,
                WKey::Named(NamedKey::ArrowUp) => self.palette_sel = self.palette_sel.saturating_sub(1),
                WKey::Named(NamedKey::Space) => input.push(' '),
                WKey::Character(c) if app => {
                    if c.eq_ignore_ascii_case("k") || c.eq_ignore_ascii_case("t") || c.eq_ignore_ascii_case("l") {
                        self.palette = None;
                    }
                }
                WKey::Character(c) if !ctrl && !sup => {
                    input.push_str(c);
                    self.palette_sel = 0;
                }
                _ => {}
            }
            self.dirty = true;
            return;
        }

        // A close confirmation owns Enter / Esc.
        if pressed {
            if let Some(Pane::Term(t)) = self.tabs.get_mut(self.active).map(|t| t.focused()) {
                if t.confirm_close.is_some() {
                    match ev.logical_key {
                        WKey::Named(NamedKey::Enter) => {
                            t.confirm_close = None;
                            self.close_tabs(true);
                        }
                        WKey::Named(NamedKey::Escape) => t.confirm_close = None,
                        _ => {}
                    }
                    self.dirty = true;
                    return;
                }
            }
        }

        if pressed && app {
            if let WKey::Character(c) = &ev.logical_key {
                match c.to_lowercase().as_str() {
                    "t" => return self.open_palette(PaletteMode::New),
                    "k" => return self.open_palette(PaletteMode::Go),
                    "l" => return self.open_palette(PaletteMode::Url),
                    "w" => return self.close_tabs(false),
                    "z" => return self.reopen_closed(),
                    "d" => return self.toggle_split(),
                    "s" => {
                        self.sidebar = !self.sidebar;
                        return self.layout();
                    }
                    _ => {}
                }
            }
            if let WKey::Named(NamedKey::Enter) = ev.logical_key {
                if let Some((_, _, url)) = self.detected.clone() {
                    self.open_url(&url, true);
                }
                return;
            }
        }

        // Plain Ctrl chords shells don't use: tab by number, MRU, prev/next.
        let tab_mod = if cfg!(target_os = "macos") { sup } else { ctrl && !shift };
        if pressed && tab_mod {
            match &ev.logical_key {
                WKey::Character(d) if d.len() == 1 && d.as_bytes()[0].is_ascii_digit() && d != "0" => {
                    let n = (d.as_bytes()[0] - b'1') as usize;
                    if n < self.tabs.len() {
                        self.activate(n);
                    }
                    return;
                }
                WKey::Character(c) if c == "`" => {
                    if let Some(&prev) = self.mru.get(1) {
                        self.activate(prev);
                    }
                    return;
                }
                WKey::Named(NamedKey::PageUp) => {
                    let n = self.tabs.len();
                    return self.activate((self.active + n - 1) % n);
                }
                WKey::Named(NamedKey::PageDown) => {
                    let n = self.tabs.len();
                    return self.activate((self.active + 1) % n);
                }
                WKey::Named(NamedKey::Enter) => {
                    if let Some((_, _, url)) = self.detected.clone() {
                        self.open_url(&url, false);
                        return;
                    }
                    // Otherwise falls through: Ctrl+Enter at a URL line runs it in the shell.
                }
                _ => {}
            }
        }

        // Route to the focused pane.
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        match tab.focused() {
            Pane::Term(t) => {
                let action = match (ev.state, ev.repeat) {
                    (ElementState::Released, _) => KeyAction::Release,
                    (ElementState::Pressed, true) => KeyAction::Repeat,
                    (ElementState::Pressed, false) => KeyAction::Press,
                };
                let mut mods = Mods::empty();
                mods.set(Mods::SHIFT, shift);
                mods.set(Mods::CTRL, ctrl);
                mods.set(Mods::ALT, alt);
                mods.set(Mods::SUPER, sup);
                let Some(key) = vt_key(&ev.logical_key) else {
                    // Modifier keys are not editing keys; anything else unknown is.
                    let modifier = matches!(
                        ev.logical_key,
                        WKey::Named(NamedKey::Shift | NamedKey::Control | NamedKey::Alt | NamedKey::Super | NamedKey::Meta | NamedKey::CapsLock | NamedKey::NumLock | NamedKey::ScrollLock | NamedKey::Fn)
                    );
                    if pressed && !modifier {
                        t.line_ok = false;
                    }
                    return;
                };

                // The URL-at-a-prompt rule: a whole-line URL at a fresh prompt
                // opens in the split instead of running. Ctrl+Enter runs it.
                if pressed {
                    match (key, ctrl || alt || sup) {
                        (Key::Enter, false) => {
                            if t.line_ok {
                                if let Some(url) = strict_url(&t.line) {
                                    let clear: &[u8] = if t.title.to_lowercase().contains("powershell")
                                        || t.title.to_lowercase().contains("pwsh")
                                        || t.title.to_lowercase() == "cmd"
                                    {
                                        b"\x1b"
                                    } else {
                                        b"\x15"
                                    };
                                    let _ = t.pty.write(clear);
                                    t.line.clear();
                                    t.line_ok = true;
                                    self.open_url(&url, false);
                                    return;
                                }
                            }
                            t.line.clear();
                            t.line_ok = true;
                        }
                        (Key::Enter, true) => {
                            t.line.clear();
                            t.line_ok = true;
                        }
                        (Key::Char(c), false) => t.line.push(c),
                        (Key::Backspace, false) => {
                            t.line.pop();
                        }
                        _ => t.line_ok = false,
                    }
                }
                let mods = if key == Key::Enter && ctrl && strict_url(&t.line).is_some() {
                    Mods::empty() // run the URL line in the shell as plain Enter
                } else {
                    mods
                };
                let bytes = input::encode(key, mods, action, t.term.modes(), t.term.keyboard_mode());
                if bytes.is_empty() {
                    return;
                }
                if t.term.grid().display_offset != 0 {
                    t.term.grid_mut().scroll_display(-(SCROLLBACK as isize));
                }
                let _ = t.pty.write(&bytes);
            }
            Pane::Web(w) => {
                // Chrome-compatible keys while a browser pane is focused.
                if pressed {
                    match (&ev.logical_key, ctrl, alt) {
                        (WKey::Character(c), true, false) if c.eq_ignore_ascii_case("l") => {
                            return self.open_palette(PaletteMode::Url);
                        }
                        (WKey::Character(c), true, false) if c.eq_ignore_ascii_case("r") => return w.tab.reload(),
                        (WKey::Named(NamedKey::F5), _, _) => return w.tab.reload(),
                        (WKey::Named(NamedKey::ArrowLeft), false, true) => return w.tab.back(),
                        (WKey::Named(NamedKey::ArrowRight), false, true) => return w.tab.forward(),
                        (WKey::Character(c), true, false) if c == "=" || c == "+" => return w.tab.zoom(1),
                        (WKey::Character(c), true, false) if c == "-" => return w.tab.zoom(-1),
                        (WKey::Character(c), true, false) if c == "0" => return w.tab.zoom(0),
                        (WKey::Named(NamedKey::F12), _, _) => {
                            tracing::info!("devtools: not composited in this spike (see README)");
                            return;
                        }
                        _ => {}
                    }
                }
                let flags = cef_mods(self.mods);
                let vk = vk_code(&ev.physical_key, &ev.logical_key);
                let mut e = cef::KeyEvent {
                    windows_key_code: vk,
                    native_key_code: vk,
                    modifiers: flags,
                    is_system_key: 0,
                    focus_on_editable_field: 0,
                    ..Default::default()
                };
                if pressed {
                    e.type_ = cef::KeyEventType::RAWKEYDOWN;
                    w.tab.key(&e);
                    if let Some(text) = &ev.text {
                        if !ctrl || alt {
                            for ch in text.encode_utf16() {
                                let mut c = cef::KeyEvent { ..e };
                                c.type_ = cef::KeyEventType::CHAR;
                                c.character = ch;
                                c.unmodified_character = ch;
                                c.windows_key_code = ch as i32;
                                w.tab.key(&c);
                            }
                        }
                    }
                } else {
                    e.type_ = cef::KeyEventType::KEYUP;
                    w.tab.key(&e);
                }
            }
        }
    }

    /// Make tab `i` active and record it as most recently used.
    fn activate(&mut self, i: usize) {
        if i >= self.tabs.len() {
            return;
        }
        self.active = i;
        self.mru.retain(|&t| t != i);
        self.mru.insert(0, i);
        self.layout();
    }

    /// Keep `mru`/`selected` valid after `tabs[i]` was removed.
    fn tab_removed(&mut self, i: usize) {
        self.mru.retain(|&t| t != i);
        for t in self.mru.iter_mut() {
            if *t > i {
                *t -= 1;
            }
        }
        self.selected = self
            .selected
            .iter()
            .filter(|&&t| t != i)
            .map(|&t| if t > i { t - 1 } else { t })
            .collect();
    }

    fn palette_commit(&mut self) {
        let Some((mode, input)) = self.palette.take() else { return };
        let rows = self.palette_rows(mode, &input);
        let action = match rows.get(self.palette_sel) {
            Some(r) => r.action.clone(),
            None if mode == PaletteMode::New => Action::NewTerminal(0),
            None => return,
        };
        self.run(action);
    }

    fn open_url(&mut self, url: &str, new_tab: bool) {
        if new_tab {
            if let Some(w) = self.new_web_pane(url) {
                self.tabs.push(Tab {
                    left: Pane::Web(w),
                    right: None,
                    focus_right: false,
                    pinned: false,
                });
                self.activate(self.tabs.len() - 1);
            }
        } else {
            let tab = &mut self.tabs[self.active];
            match &tab.right {
                Some(Pane::Web(w)) => w.tab.load(url),
                _ => {
                    if let Some(w) = self.new_web_pane(url) {
                        let tab = &mut self.tabs[self.active];
                        tab.right = Some(Pane::Web(w));
                    }
                }
            }
            self.tabs[self.active].focus_right = true;
        }
        self.layout();
    }

    fn new_tab(&mut self, profile: usize) {
        if let Ok(t) = self.new_term_pane(false, profile) {
            self.tabs.push(Tab {
                left: Pane::Term(t),
                right: None,
                focus_right: false,
                pinned: false,
            });
            self.activate(self.tabs.len() - 1);
        }
    }

    /// Close the selection (or the active tab). Unless `force`, a terminal
    /// with a foreground process asks first.
    fn close_tabs(&mut self, force: bool) {
        let mut targets: Vec<usize> = if self.selected.is_empty() {
            vec![self.active]
        } else {
            let mut v: Vec<usize> = self.selected.iter().copied().collect();
            if !v.contains(&self.active) {
                v.push(self.active);
            }
            v
        };
        targets.sort_unstable();
        targets.dedup();
        if targets.len() >= self.tabs.len() {
            // Never close the last tab; keep one.
            targets.retain(|&t| t != self.active);
            if targets.is_empty() {
                return;
            }
        }
        if !force {
            for &i in &targets {
                if let Pane::Term(t) = &mut self.tabs[i].left {
                    if let Some(p) = t.pty.foreground_process() {
                        self.active = i;
                        if let Pane::Term(t) = &mut self.tabs[i].left {
                            t.confirm_close = Some(p);
                        }
                        self.dirty = true;
                        return;
                    }
                }
            }
        }
        for &i in targets.iter().rev() {
            let tab = self.tabs.remove(i);
            self.closed.push(match &tab.left {
                Pane::Term(t) => Closed::Term(t.profile),
                Pane::Web(w) => Closed::Web(w.tab.shared.borrow().url.clone()),
            });
            self.tab_removed(i);
        }
        self.selected.clear();
        let next = self.mru.first().copied().unwrap_or(0).min(self.tabs.len() - 1);
        self.activate(next);
    }

    fn reopen_closed(&mut self) {
        match self.closed.pop() {
            Some(Closed::Term(p)) => self.new_tab(p),
            Some(Closed::Web(url)) => self.open_url(&url, true),
            None => {}
        }
    }

    fn toggle_split(&mut self) {
        let tab = &mut self.tabs[self.active];
        if tab.right.is_some() {
            tab.right = None;
            tab.focus_right = false;
        } else if let Some(w) = self.new_web_pane("https://www.google.com/") {
            let tab = &mut self.tabs[self.active];
            tab.right = Some(Pane::Web(w));
            tab.focus_right = true;
        }
        self.layout();
    }

    pub fn modifiers(&mut self, m: ModifiersState) {
        self.mods = m;
    }

    pub fn mouse_moved(&mut self, x: f32, y: f32) {
        self.mouse = (x, y);
        let flags = cef_mods(self.mods) | if self.mouse_down_in_web { 16 } else { 0 };
        if let Some(tab) = self.tabs.get(self.active) {
            for p in std::iter::once(&tab.left).chain(tab.right.as_ref()) {
                if let Pane::Web(w) = p {
                    if w.page.contains(x, y) || self.mouse_down_in_web {
                        let (lx, ly) = ((x - w.page.x) / self.scale, (y - w.page.y) / self.scale);
                        w.tab.mouse_move(lx as i32, ly as i32, flags, false);
                    }
                }
            }
        }
    }

    pub fn mouse_button(&mut self, button: MouseButton, state: ElementState) {
        let (x, y) = self.mouse;
        let pressed = state == ElementState::Pressed;
        let strip = self.strip_rect();

        if pressed && button == MouseButton::Left && self.palette.is_some() {
            self.palette = None;
            self.dirty = true;
            return;
        }

        // Top strip: window controls, else drag.
        if pressed && button == MouseButton::Left && strip.contains(x, y) {
            let w = self.gpu.size.0 as f32;
            let right = w - self.px(18.0);
            let btn_w = self.px(28.0);
            if x > right - btn_w {
                self.window.set_minimized(false);
                std::process::exit(0);
            } else if x > right - 2.0 * btn_w {
                self.window.set_maximized(!self.window.is_maximized());
            } else if x > right - 3.0 * btn_w {
                self.window.set_minimized(true);
            } else {
                let _ = self.window.drag_window();
            }
            return;
        }

        // Sidebar: pinned cells, tab rows, footer. Ctrl-click selects, Shift-click ranges.
        if pressed && button == MouseButton::Left && self.sidebar && self.sidebar_rect().contains(x, y) {
            let sb = self.sidebar_rect();
            let (pinned, listed, pinned_h, row_h, list_y) = self.sidebar_geometry();
            let foot = self.px(m::ROW_PAD_Y) * 2.0 + self.px(m::LABEL_PX);
            if y > sb.bottom() - foot {
                self.open_palette(PaletteMode::New);
                return;
            }
            let hit = if !pinned.is_empty() && y >= list_y - pinned_h && y < list_y {
                let k = ((x - sb.x) / (sb.w / pinned.len() as f32).floor()) as usize;
                pinned.get(k).copied()
            } else if y >= list_y {
                listed.get(((y - list_y) / row_h) as usize).copied()
            } else {
                None
            };
            if let Some(i) = hit {
                if self.mods.control_key() || self.mods.super_key() {
                    if !self.selected.remove(&i) {
                        self.selected.insert(i);
                    }
                } else if self.mods.shift_key() {
                    let (a, b) = (self.active.min(i), self.active.max(i));
                    self.selected.extend(a..=b);
                } else {
                    self.selected.clear();
                    self.activate(i);
                }
                self.dirty = true;
            }
            return;
        }

        // Panes: focus, and forward to the browser.
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        let mut hit_right = None;
        let in_left = match &tab.left {
            Pane::Term(t) => t.rect.contains(x, y),
            Pane::Web(w) => w.rect.contains(x, y),
        };
        if let Some(r) = &tab.right {
            let hit = match r {
                Pane::Term(t) => t.rect.contains(x, y),
                Pane::Web(w) => w.rect.contains(x, y),
            };
            if hit {
                hit_right = Some(true);
            }
        }
        if pressed {
            if hit_right == Some(true) {
                tab.focus_right = true;
                self.dirty = true;
            } else if in_left {
                tab.focus_right = false;
                self.dirty = true;
            }
        }
        let focus_right = tab.focus_right;
        let scale = self.scale;
        let back_w = 40.0 * scale;
        let mods = cef_mods(self.mods);
        let mut down_in_web = self.mouse_down_in_web;
        let mut open_url_palette = false;
        for (is_right, p) in std::iter::once((false, &tab.left)).chain(tab.right.as_ref().map(|r| (true, r))) {
            match p {
                Pane::Web(w) => {
                    let url_row = Rect::new(w.rect.x, w.rect.y, w.rect.w, w.page.y - w.rect.y);
                    if pressed && button == MouseButton::Left && url_row.contains(x, y) {
                        if x < w.rect.x + back_w {
                            w.tab.back();
                        } else {
                            open_url_palette = true;
                        }
                        continue;
                    }
                    let inside = w.page.contains(x, y);
                    if inside || (!pressed && down_in_web) {
                        let (lx, ly) = ((x - w.page.x) / scale, (y - w.page.y) / scale);
                        let b = match button {
                            MouseButton::Left => cef::MouseButtonType::LEFT,
                            MouseButton::Right => cef::MouseButtonType::RIGHT,
                            MouseButton::Middle => cef::MouseButtonType::MIDDLE,
                            _ => continue,
                        };
                        w.tab.mouse_click(lx as i32, ly as i32, mods, b, !pressed, 1);
                        if button == MouseButton::Left {
                            down_in_web = pressed;
                        }
                    }
                    w.tab.focus(is_right == focus_right);
                }
                Pane::Term(_) => {}
            }
        }
        self.mouse_down_in_web = down_in_web;
        if open_url_palette {
            self.open_palette(PaletteMode::Url);
        }
    }

    pub fn wheel(&mut self, delta: MouseScrollDelta) {
        let (x, y) = self.mouse;
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        for p in std::iter::once(&mut tab.left).chain(tab.right.as_mut()) {
            match p {
                Pane::Term(t) if t.rect.contains(x, y) => {
                    let lines = match delta {
                        MouseScrollDelta::LineDelta(_, y) => (y * 3.0) as isize,
                        MouseScrollDelta::PixelDelta(p) => (p.y as f32 / t.grid.cell_size().1) as isize,
                    };
                    t.term.grid_mut().scroll_display(lines);
                    self.dirty = true;
                }
                Pane::Web(w) if w.page.contains(x, y) => {
                    let (dx, dy) = match delta {
                        MouseScrollDelta::LineDelta(x, y) => ((x * 40.0) as i32, (y * 40.0) as i32),
                        MouseScrollDelta::PixelDelta(p) => (p.x as i32, p.y as i32),
                    };
                    let (lx, ly) = ((x - w.page.x) / self.scale, (y - w.page.y) / self.scale);
                    w.tab.wheel(lx as i32, ly as i32, cef_mods(self.mods), dx, dy);
                }
                _ => {}
            }
        }
    }

    pub fn window_moved(&mut self, x: i32, y: i32) {
        for tab in &self.tabs {
            for p in std::iter::once(&tab.left).chain(tab.right.as_ref()) {
                if let Pane::Web(w) = p {
                    w.tab.shared.borrow_mut().window_pos = (x, y);
                }
            }
        }
    }
}

fn short_title(t: &str) -> String {
    let t = t.rsplit(['\\', '/']).next().unwrap_or(t);
    t.trim_end_matches(".exe").to_string()
}

fn last_lines(term: &Term, n: usize) -> Vec<String> {
    let g = term.grid();
    let mut lines: Vec<String> = (0..g.rows()).map(|r| g.visible_row(r).text()).collect();
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    let start = lines.len().saturating_sub(n);
    lines[start..].to_vec()
}

/// Find the last `localhost:PORT` on screen → (row, col, url).
fn detect_localhost(term: &Term) -> Option<(usize, usize, String)> {
    let g = term.grid();
    for r in (0..g.rows()).rev() {
        let text = g.visible_row(r).text();
        if let Some(pos) = text.find("localhost:") {
            let rest = &text[pos + "localhost:".len()..];
            let port: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            if !port.is_empty() {
                let col = text[..pos].chars().count();
                let path: String = rest[port.len()..].chars().take_while(|c| !c.is_whitespace()).collect();
                return Some((r, col, format!("http://localhost:{port}{path}")));
            }
        }
    }
    None
}

/// The whole line is a URL: a scheme, `localhost[:port]`, or `host.tld` with a
/// known TLD. Bare words and anything with shell syntax never qualify.
fn strict_url(line: &str) -> Option<String> {
    let s = line.trim();
    if s.is_empty() || s.contains(char::is_whitespace) || s.contains(|c| "|&;<>$`'\"()".contains(c)) {
        return None;
    }
    let lower = s.to_lowercase();
    if let Some(rest) = lower.split_once("://").map(|(scheme, rest)| (scheme, rest)) {
        let (scheme, rest) = rest;
        if !scheme.is_empty() && scheme.chars().all(|c| c.is_ascii_alphabetic()) && !rest.is_empty() {
            return Some(s.to_string());
        }
        return None;
    }
    let host = lower.split(['/', '?', '#']).next().unwrap_or("");
    let (host, port) = match host.split_once(':') {
        Some((h, p)) => (h, Some(p)),
        None => (host, None),
    };
    if port.is_some_and(|p| p.is_empty() || !p.chars().all(|c| c.is_ascii_digit())) {
        return None;
    }
    if host == "localhost" || host == "127.0.0.1" {
        return Some(format!("http://{s}"));
    }
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() < 2 || labels.iter().any(|l| l.is_empty() || !l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')) {
        return None;
    }
    const TLDS: &[&str] = &[
        "com", "org", "net", "io", "dev", "rs", "sh", "app", "ai", "co", "me", "gg", "tv", "edu", "gov",
        "info", "xyz", "uk", "de", "fr", "ca", "us", "jp", "cn", "nl", "se", "no", "fi", "es", "it",
        "ch", "at", "au", "nz", "br", "mx", "in", "ru", "pl", "cz", "eu", "fm", "to", "ly", "is", "so",
        "cc", "ws", "page", "site", "tech", "cloud", "design", "studio", "zone", "run", "wiki", "news",
    ];
    let tld = labels.last().unwrap();
    if !TLDS.contains(tld) && !(labels[0] == "www" && tld.len() >= 2) {
        return None;
    }
    Some(format!("http://{s}"))
}

fn vt_key(k: &WKey) -> Option<Key> {
    Some(match k {
        WKey::Named(n) => match n {
            NamedKey::Enter => Key::Enter,
            NamedKey::Tab => Key::Tab,
            NamedKey::Backspace => Key::Backspace,
            NamedKey::Escape => Key::Escape,
            NamedKey::ArrowUp => Key::Up,
            NamedKey::ArrowDown => Key::Down,
            NamedKey::ArrowLeft => Key::Left,
            NamedKey::ArrowRight => Key::Right,
            NamedKey::Home => Key::Home,
            NamedKey::End => Key::End,
            NamedKey::PageUp => Key::PageUp,
            NamedKey::PageDown => Key::PageDown,
            NamedKey::Insert => Key::Insert,
            NamedKey::Delete => Key::Delete,
            NamedKey::Space => Key::Char(' '),
            NamedKey::F1 => Key::F(1),
            NamedKey::F2 => Key::F(2),
            NamedKey::F3 => Key::F(3),
            NamedKey::F4 => Key::F(4),
            NamedKey::F5 => Key::F(5),
            NamedKey::F6 => Key::F(6),
            NamedKey::F7 => Key::F(7),
            NamedKey::F8 => Key::F(8),
            NamedKey::F9 => Key::F(9),
            NamedKey::F10 => Key::F(10),
            NamedKey::F11 => Key::F(11),
            NamedKey::F12 => Key::F(12),
            _ => return None,
        },
        WKey::Character(s) => Key::Char(s.chars().next()?),
        _ => return None,
    })
}

fn cef_mods(m: ModifiersState) -> u32 {
    let mut f = 0;
    if m.shift_key() {
        f |= 2;
    }
    if m.control_key() {
        f |= 4;
    }
    if m.alt_key() {
        f |= 8;
    }
    if m.super_key() {
        f |= 128;
    }
    f
}

/// Windows virtual-key code for a winit key (what CEF expects on Windows).
fn vk_code(phys: &PhysicalKey, logical: &WKey) -> i32 {
    if let PhysicalKey::Code(c) = phys {
        let v = match c {
            KeyCode::Enter => 0x0D,
            KeyCode::Tab => 0x09,
            KeyCode::Backspace => 0x08,
            KeyCode::Escape => 0x1B,
            KeyCode::Space => 0x20,
            KeyCode::ArrowLeft => 0x25,
            KeyCode::ArrowUp => 0x26,
            KeyCode::ArrowRight => 0x27,
            KeyCode::ArrowDown => 0x28,
            KeyCode::Home => 0x24,
            KeyCode::End => 0x23,
            KeyCode::PageUp => 0x21,
            KeyCode::PageDown => 0x22,
            KeyCode::Delete => 0x2E,
            KeyCode::Insert => 0x2D,
            KeyCode::ShiftLeft | KeyCode::ShiftRight => 0x10,
            KeyCode::ControlLeft | KeyCode::ControlRight => 0x11,
            KeyCode::AltLeft | KeyCode::AltRight => 0x12,
            KeyCode::F5 => 0x74,
            KeyCode::F12 => 0x7B,
            _ => 0,
        };
        if v != 0 {
            return v;
        }
    }
    match logical {
        WKey::Character(s) => {
            let c = s.chars().next().unwrap_or('\0').to_ascii_uppercase();
            if c.is_ascii_alphanumeric() {
                c as i32
            } else {
                0
            }
        }
        _ => 0,
    }
}
