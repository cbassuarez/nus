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
    /// Type a command into the focused (or a new) terminal and run it.
    RunInShell(String),
    ToggleSplit,
    CloseTab,
    ToggleSidebar,
    TogglePin,
    Reopen,
    ShellStyle,
    ShellRadius(f32),
    Pip,
}

pub use crate::surface::Shell;
use crate::anim::{base, Anim, BarColor, BarStyle, Follow, LoadBar, Motion};
use crate::surface::{Fullscreen, HoverFrom, Overrides, Rules, Side, SidebarRules, Surface, TabCtx};

pub enum Closed {
    Term(usize),
    Web(String),
}

/// Click targets in the top strip.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CrumbHit {
    Space,
    Tab,
    Url,
    Search,
    Sidebar,
    Ports,
    Assistant,
    Pip,
    Waiting,
    Close,
    Maximize,
    Minimize,
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
    /// Newsreader, for the reader.
    pub serif: FontId,
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
    /// Rang the bell while not being looked at.
    pub waiting: bool,
}

pub struct WebPane {
    pub tab: BrowserTab,
    pub page: Rect,
    pub rect: Rect,
    pub seen_paints: u64,
    pub devtools: Option<crate::browser::DevToolsView>,
    /// DevTools pane rect (below the page) when open.
    pub dt_rect: Rect,
    pub focus_devtools: bool,
    /// The loading bar chases real progress, then fades out.
    pub load: Follow,
    pub load_fade: Anim,
    /// The URL the rules' boost was last applied to.
    pub boosted: String,
    /// Reader mode over this page, and the pending extraction call.
    pub reader: Option<crate::reader::Reader>,
    pub reader_req: Option<i32>,
    /// The favicon as a texture, keyed by its URL.
    pub favicon: Option<(String, Arc<wgpu::BindGroup>)>,
    /// Which DevTools panel: 0 console, 1 network, 2 elements.
    pub dt_panel: usize,
}

pub const DT_PANELS: [(&str, (&str, &str)); 3] = [("console", nus_render::text::icons::CONSOLE), ("network", nus_render::text::icons::NETWORK), ("elements", nus_render::text::icons::CODE)];

pub struct SettingsPane {
    pub rect: Rect,
    pub section: usize,
    /// Narrow layout: tiles first, then one section with a back crumb.
    pub drill: bool,
}

/// Layout class by window width (logical px). Wide has everything;
/// Standard drops the pinned sidebar; Narrow drops the split too.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Width {
    Wide,
    Standard,
    Narrow,
}

pub const NARROW: f32 = 900.0;
pub const WIDE: f32 = 1200.0;

/// First-run panel beside the first shell: five things to try, ticked off
/// as they happen. Lives in `App::hints`; this is just its rect.
pub struct HintsPane {
    pub rect: Rect,
}

pub const HINTS: [(&str, &str); 5] = [
    ("K", "the palette · tabs, ports, ask, settings"),
    ("T", "new tab · type a URL for a page, a name for a shell"),
    ("ENTER", "at a prompt that is a URL · opens it beside"),
    ("EDGE", "hover the left edge · tabs slide in, pin with Ctrl+Shift+S"),
    ("`", "back to the last tab · Ctrl+PgUp/PgDn walk them"),
];

pub enum Pane {
    Term(TermPane),
    Web(WebPane),
    Settings(SettingsPane),
    Hints(HintsPane),
}

pub struct SidebarGeom {
    pub pinned: Vec<usize>,
    pub pinned_h: f32,
    /// (tab index, y, height) for each listed row.
    pub rows: Vec<(usize, f32, f32)>,
    pub foot_y: f32,
}

pub struct Tab {
    pub id: u64,
    /// Stack parent (a top-level tab's id). One level only: a child never
    /// has children; links from a child join the same stack.
    pub parent: Option<u64>,
    pub left: Pane,
    pub right: Option<Pane>,
    pub focus_right: bool,
    pub pinned: bool,
    /// Colours the rules gave this tab.
    pub look: Overrides,
}

impl Tab {
    pub(crate) fn waiting(&self) -> bool {
        let w = |p: &Pane| matches!(p, Pane::Term(t) if t.waiting);
        w(&self.left) || self.right.as_ref().is_some_and(w)
    }

    /// (title, detail) for a compact sidebar row: detail is cwd/host for a
    /// shell, the site for a page.
    pub(crate) fn row_text(&self) -> (String, String) {
        match &self.left {
            Pane::Term(t) => (t.title.clone(), String::new()),
            Pane::Web(w) => {
                let s = w.tab.shared.borrow();
                let host = s.url.split("//").nth(1).unwrap_or("").split('/').next().unwrap_or("").trim_start_matches("www.").to_string();
                let title = if s.title.is_empty() { host.clone() } else { s.title.clone() };
                (title, host)
            }
            Pane::Settings(_) => ("settings".into(), String::new()),
            Pane::Hints(_) => ("welcome".into(), String::new()),
        }
    }

    fn focused(&mut self) -> &mut Pane {
        if self.focus_right && self.right.is_some() {
            self.right.as_mut().unwrap()
        } else {
            &mut self.left
        }
    }
    pub(crate) fn title(&self) -> String {
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
            Pane::Settings(_) => "settings".into(),
            Pane::Hints(_) => "welcome".into(),
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
    pub target: nus_render::Target,
    pub fonts: FontSystem,
    pub f: Fonts,
    pub scene: Scene,
    pub theme: Theme,
    pub scale: f32,
    pub proxy: EventLoopProxy<UserEvent>,
    pub device: wgpu::Device,
    pub bind_texture: Rc<dyn Fn(&wgpu::Texture) -> Arc<wgpu::BindGroup>>,

    pub space_name: String,
    pub tabs: Vec<Tab>,
    pub active: usize,
    /// Sidebar pinned open (Ctrl+Shift+S). Otherwise it slides in on hover.
    pub sidebar: bool,
    pub sidebar_hover: bool,
    pub sidebar_leave: Option<Instant>,
    pub hover_row: Option<usize>,
    pub user_name: String,
    /// What the window is made of (see surface.rs) and the rules that colour
    /// new tabs; both editable from the settings tab.
    pub surface: Surface,
    pub sidebar_rules: SidebarRules,
    pub rules: Rules,
    pub settings_hits: Vec<(Rect, crate::settings::Hit)>,
    /// AccessKit node id → what activating it does (rebuilt per frame).
    pub access_map: std::collections::HashMap<u64, crate::access::Target>,
    /// Palette row rects from the last frame (clicks, AccessKit).
    pub palette_hits: Vec<Rect>,
    /// Little nus: the floating window for links from outside.
    pub little: Option<crate::little::Little>,
    pub little_request: Option<String>,
    pub little_pos: (f32, f32),
    /// URLs handed over by later launches (see little::claim).
    pub urls_rx: Option<std::sync::mpsc::Receiver<String>>,
    pub register_note: String,
    pub behavior: crate::settings::Behavior,
    pub fullscreen: bool,
    /// The pointer has moved inside the window since it last left it.
    pub pointer_inside: bool,
    pub motion: Motion,
    pub load_bar: LoadBar,
    /// 0 hidden … 1 shown, for the hover sidebar.
    pub sidebar_anim: Anim,
    /// 0 … 1 rise-and-fade for the palette.
    pub palette_anim: Anim,
    /// 0 … 1 drop for confirmation bands.
    pub band_anim: Anim,
    /// The active tint's y in the sidebar; it travels between rows.
    pub tint_anim: Anim,
    /// Crumb title alpha, replayed on every tab switch.
    pub crumb_anim: Anim,
    /// Per-tab sidebar row heights (previews grow, stacks unfold), by tab id.
    pub row_anims: std::collections::HashMap<u64, Anim>,
    /// Transient x offset applied to sidebar_rect while the slide-in draws.
    pub sidebar_shift: f32,
    pub shell_phase: f32,
    pub pip: Option<crate::pip::Pip>,
    pub pip_request: Option<(usize, bool)>,
    /// Deferred DevTools open (tab, right pane), created from the main loop.
    pub devtools_request: Option<(usize, bool)>,
    pub crumb_hits: Vec<(Rect, CrumbHit)>,
    pub next_id: u64,
    /// Onboarding ticks (see HINTS); the panel leaves once all five are set.
    pub hints: [bool; 5],
    pub hint_hits: Vec<(Rect, usize)>,
    /// Closing a stack's parent asks first: the parent's index.
    pub confirm_stack: Option<usize>,
    pub window_focused: bool,
    pub palette: Option<(PaletteMode, String)>,
    pub palette_sel: usize,
    pub profiles: Vec<nus_pty::Profile>,
    /// Tab indices, most recently used first.
    pub mru: Vec<usize>,
    pub selected: std::collections::HashSet<usize>,
    pub closed: Vec<Closed>,
    /// Local assistants on PATH: (name, command template with {q}).
    pub llm_tools: Vec<(String, String)>,
    /// Listening ports, refreshed when the palette opens.
    pub ports: Vec<nus_pty::ListeningPort>,

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
        let (gpu, target) = Gpu::new(window.clone())?;
        let scale = window.scale_factor() as f32;
        let mut fonts = FontSystem::new();
        let ui = fonts.load_bytes(nus_render::text::bundled::PLEX_MONO, 0)?;
        let strong = fonts.load_bytes(nus_render::text::bundled::PLEX_MONO_SEMIBOLD, 0)?;
        let wordmark = fonts.load_bytes(nus_render::text::bundled::NEWSREADER_ITALIC, 0)?;
        let serif = fonts.load_bytes(nus_render::text::bundled::NEWSREADER, 0)?;
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
            target,
            fonts,
            f: Fonts {
                ui,
                strong,
                wordmark,
                term: term_font,
                serif,
            },
            scene: Scene::new(),
            theme,
            scale,
            proxy,
            device,
            bind_texture,
            space_name: "nus".into(),
            tabs: Vec::new(),
            active: 0,
            sidebar: false,
            sidebar_hover: false,
            sidebar_leave: None,
            hover_row: None,
            surface: Surface::default(),
            sidebar_rules: SidebarRules::default(),
            rules: Rules::load(),
            settings_hits: Vec::new(),
            access_map: std::collections::HashMap::new(),
            palette_hits: Vec::new(),
            little: None,
            little_request: None,
            little_pos: (0.0, 0.0),
            urls_rx: None,
            register_note: String::new(),
            behavior: crate::settings::Behavior::default(),
            fullscreen: false,
            pointer_inside: false,
            motion: Motion::default(),
            load_bar: LoadBar::default(),
            sidebar_anim: Anim::at(0.0),
            palette_anim: Anim::at(1.0),
            band_anim: Anim::at(1.0),
            tint_anim: Anim::at(0.0),
            crumb_anim: Anim::at(1.0),
            row_anims: std::collections::HashMap::new(),
            sidebar_shift: 0.0,
            shell_phase: 0.0,
            pip: None,
            pip_request: None,
            devtools_request: None,
            crumb_hits: Vec::new(),
            next_id: 1,
            hints: App::load_hints(),
            hint_hits: Vec::new(),
            confirm_stack: None,
            window_focused: true,
            user_name: std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_else(|_| "you".into()),
            palette: None,
            palette_sel: 0,
            profiles: nus_pty::Profile::discover(),
            mru: vec![0],
            selected: Default::default(),
            closed: Vec::new(),
            llm_tools: discover_llm_tools(),
            ports: Vec::new(),
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
        let right = if App::onboarded() {
            app.new_web_pane("https://docs.rs/wgpu/latest/wgpu/").map(Pane::Web)
        } else {
            Some(Pane::Hints(HintsPane { rect: Rect::new(0.0, 0.0, 1.0, 1.0) }))
        };
        let first = app.make_tab(Pane::Term(term), right);
        app.tabs.push(first);
        app.layout();
        app.apply_term_resizes(true);
        app.refresh_icon();
        Ok(app)
    }

    pub(crate) fn px(&self, v: f32) -> f32 {
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
            waiting: false,
        })
    }

    pub(crate) fn new_web_pane(&mut self, url: &str) -> Option<WebPane> {
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
            seen_paints: 0,
            load: Follow::new(0.0),
            load_fade: Anim::at(0.0),
            boosted: String::new(),
            reader: None,
            reader_req: None,
            favicon: None,
            dt_panel: 0,
            devtools: None,
            dt_rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            focus_devtools: false,
        })
    }

    // --- layout ----------------------------------------------------------

    /// (top, right, bottom, left) physical px the shell takes from the window.
    fn shell_insets(&self) -> (f32, f32, f32, f32) {
        let w = self.px(self.surface.shell_width);
        match self.surface.shell {
            Shell::Band => (w, 0.0, 0.0, 0.0),
            _ => (w, w, w, w),
        }
    }

    pub fn strip_rect(&self) -> Rect {
        let (top, right, _, left) = self.shell_insets();
        Rect::new(left, top, self.target.size.0 as f32 - left - right, self.px(m::TOP_STRIP))
    }

    pub fn width_class(&self) -> Width {
        let w = self.target.size.0 as f32 / self.scale.max(0.1);
        if w >= WIDE {
            Width::Wide
        } else if w >= NARROW {
            Width::Standard
        } else {
            Width::Narrow
        }
    }

    /// Pinned open: the user's pin, or the fullscreen rule. Never below Wide.
    pub fn sidebar_pinned(&self) -> bool {
        if self.width_class() != Width::Wide {
            return false;
        }
        if self.fullscreen {
            return match self.sidebar_rules.fullscreen {
                Fullscreen::Pinned => true,
                Fullscreen::Hidden => false,
                Fullscreen::Hover => self.sidebar,
            };
        }
        self.sidebar
    }

    /// Whether a hover may reveal it at all.
    fn sidebar_hoverable(&self) -> bool {
        !(self.fullscreen && self.sidebar_rules.fullscreen == Fullscreen::Hidden)
    }

    pub fn sidebar_right(&self) -> bool {
        self.sidebar_rules.side == Side::Right
    }

    pub(crate) fn content_rect(&self) -> Rect {
        let (st, sr, sb, sl) = self.shell_insets();
        let top = st + self.px(m::TOP_STRIP) + self.px(m::STRUCTURE);
        let taken = if self.sidebar_pinned() { self.px(m::SIDEBAR) + self.px(m::STRUCTURE) } else { self.px(4.0) };
        let (left, right) = if self.sidebar_right() { (sl, sr + taken) } else { (sl + taken, sr) };
        Rect::new(left, top, self.target.size.0 as f32 - left - right, self.target.size.1 as f32 - top - sb)
    }

    pub(crate) fn sidebar_rect(&self) -> Rect {
        let c = self.content_rect();
        let (_, sr, _, sl) = self.shell_insets();
        let w = self.px(m::SIDEBAR);
        let x = if self.sidebar_right() { self.target.size.0 as f32 - sr - w } else { sl };
        Rect::new(x + self.sidebar_shift, c.y, w, c.h)
    }

    pub(crate) fn sidebar_visible(&self) -> bool {
        self.sidebar_pinned() || self.sidebar_hover
    }

    /// The paper the chrome draws on (theme paper tinted by the surface).
    pub fn paper(&self) -> nus_render::Color {
        self.surface.paper(self.theme.paper)
    }

    pub fn toggle_fullscreen(&mut self) {
        self.fullscreen = !self.fullscreen;
        self.window.set_fullscreen(if self.fullscreen { Some(winit::window::Fullscreen::Borderless(None)) } else { None });
        self.sidebar_hover = false;
        self.layout();
    }

    pub fn cursor_left(&mut self) {
        self.pointer_inside = false;
    }

    pub(crate) fn header_h(&self) -> f32 {
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
        let narrow = self.width_class() == Width::Narrow;
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        let off = Rect::new(-4.0 * c.w - c.x, c.y, c.w, c.h);
        let (left_rect, right_rect) = if tab.right.is_some() && narrow {
            // No split below 900px: the focused pane takes the content, the
            // other keeps its size off screen so it never reflows.
            if tab.focus_right {
                (off, Some(c))
            } else {
                (c, Some(off))
            }
        } else if tab.right.is_some() {
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
            Pane::Settings(s) => s.rect = r,
            Pane::Hints(h) => h.rect = r,
            Pane::Web(w) => {
                w.rect = r;
                let url_row = (6.0 * 2.0 + 22.0) * scale;
                let tools_row = (8.0 * 2.0 + 13.0 + 1.0) * scale;
                let avail = r.h - url_row.round() - 1.0 - tools_row.round();
                let dt_h = if w.devtools.is_some() { (avail * 0.42).round() } else { 0.0 };
                w.page = Rect::new(r.x, r.y + url_row.round() + 1.0, r.w, avail - dt_h);
                w.dt_rect = Rect::new(r.x, w.page.bottom() + 1.0, r.w, dt_h - 1.0);
                {
                    let mut s = w.tab.shared.borrow_mut();
                    s.origin = (w.page.x, w.page.y);
                    s.scale = scale;
                }
                w.tab.resized((w.page.w / scale).floor(), (w.page.h / scale).floor());
                if let Some(d) = &w.devtools {
                    {
                        let mut s = d.shared.borrow_mut();
                        s.origin = (w.dt_rect.x, w.dt_rect.y);
                        s.scale = scale;
                    }
                    d.resized((w.dt_rect.w / scale).floor(), (w.dt_rect.h.max(1.0) / scale).floor());
                }
            }
        };
        place(&mut tab.left, left_rect);
        if let (Some(r), Some(rr)) = (tab.right.as_mut(), right_rect) {
            place(r, rr);
        }
        self.dirty = true;
    }

    pub fn resize(&mut self, w: u32, h: u32) {
        self.target.resize(&self.gpu.device, w, h);
        self.layout();
        self.resize_due = Some(Instant::now() + std::time::Duration::from_millis(80));
    }

    /// Time-based housekeeping, once per loop iteration.
    pub fn tick(&mut self) {
        self.drain_popups();
        self.apply_boosts();
        self.poll_reader();
        self.sync_favicons();
        // Links from outside.
        let handed: Vec<String> = self.urls_rx.as_ref().map(|rx| rx.try_iter().collect()).unwrap_or_default();
        for u in handed {
            self.open_little(&u);
            self.dirty = true;
        }
        self.sync_anims();
        if self.anims_active() {
            self.dirty = true;
        }
        if self.surface.shell == Shell::Aurora {
            self.shell_phase = (self.shell_phase + 0.0015) % 1.0;
            self.dirty = true;
        }
        if let Some(t) = self.sidebar_leave {
            if Instant::now() >= t {
                self.sidebar_leave = None;
                self.sidebar_hover = false;
                self.hover_row = None;
                self.dirty = true;
            }
        }
    }

    // ── Motion ───────────────────────────────────────────────────────────

    /// Point every animation at its current target (rows, tint, sidebar,
    /// loading bars). Cheap; runs each loop.
    fn sync_anims(&mut self) {
        let hover_dur = self.motion.dur(base::SIDEBAR);
        let row_dur = self.motion.dur(base::ROW);
        // Hover sidebar: shown while hovered and not pinned.
        let want = if self.sidebar_hover && !self.sidebar_pinned() { 1.0 } else { 0.0 };
        self.sidebar_anim.go(want, hover_dur);
        // Row heights.
        let compact = self.px(9.0) * 2.0 + self.px(m::UI_PX) + self.px(m::HAIRLINE);
        let expanded = compact + self.px(8.0) + self.px(m::PREVIEW_H) + self.px(3.0);
        let ids: Vec<(u64, f32)> = (0..self.tabs.len())
            .map(|i| {
                let t = &self.tabs[i];
                let hidden = t.pinned || (t.parent.is_some() && !self.stack_open(self.stack_root(i)));
                let h = if hidden {
                    0.0
                } else if self.hover_row == Some(i) || t.waiting() {
                    expanded
                } else {
                    compact
                };
                (t.id, h)
            })
            .collect();
        for (id, h) in ids {
            let a = self.row_anims.entry(id).or_insert_with(|| Anim::at(0.0));
            a.go(h, row_dur);
        }
        let live: std::collections::HashSet<u64> = self.tabs.iter().map(|t| t.id).collect();
        self.row_anims.retain(|id, _| live.contains(id));
        // Active tint travels to the active row.
        let g = self.sidebar_geometry();
        if let Some(&(_, y, _)) = g.rows.iter().find(|&&(i, _, _)| i == self.active) {
            let d = self.motion.dur(base::TINT);
            if self.tint_anim.target() == 0.0 && !self.tint_anim.active() {
                self.tint_anim = Anim::at(y);
            } else {
                self.tint_anim.go(y, d);
            }
        }
        // Loading bars chase progress; on arrival they fade out.
        let chase = self.load_bar.chase;
        let out = self.motion.dur(base::LOAD_OUT);
        for tab in self.tabs.iter_mut() {
            for p in std::iter::once(&mut tab.left).chain(tab.right.as_mut()) {
                if let Pane::Web(w) = p {
                    let (loading, progress) = {
                        let s = w.tab.shared.borrow();
                        (s.loading, s.progress as f32)
                    };
                    if loading {
                        // Real progress, with a trickle so a stalled bar still breathes.
                        let trickle = (w.load.target + 0.002).min(0.92);
                        w.load.target = progress.max(trickle).max(0.08);
                        w.load_fade.go(1.0, 0.0);
                    } else if w.load.target < 1.0 || w.load.value < 0.999 {
                        w.load.target = 1.0;
                    } else if w.load_fade.target() > 0.0 {
                        w.load_fade.go(0.0, out);
                    } else if !w.load_fade.active() && w.load.value >= 0.999 {
                        // Rest for the next navigation.
                        w.load = Follow::new(0.0);
                    }
                    if w.load.step(chase) {
                        self.dirty = true;
                    }
                }
            }
        }
    }

    /// Apply `on_page` boosts to pages whose address changed. Runs at the
    /// address change and again once loaded, so late documents get it too.
    fn apply_boosts(&mut self) {
        let mut jobs: Vec<(usize, bool, String, crate::surface::Boost)> = Vec::new();
        for (i, tab) in self.tabs.iter().enumerate() {
            for (right, p) in std::iter::once((false, &tab.left)).chain(tab.right.as_ref().map(|r| (true, r))) {
                if let Pane::Web(w) = p {
                    let (url, loading) = {
                        let s = w.tab.shared.borrow();
                        (s.url.clone(), s.loading)
                    };
                    let key = format!("{url}#{}", if loading { "loading" } else { "loaded" });
                    if url.is_empty() || w.boosted == key {
                        continue;
                    }
                    let boost = self.rules.on_page(&url);
                    jobs.push((i, right, key, boost));
                }
            }
        }
        for (i, right, key, boost) in jobs {
            let Some(tab) = self.tabs.get_mut(i) else { continue };
            let pane = if right { tab.right.as_mut() } else { Some(&mut tab.left) };
            if let Some(Pane::Web(w)) = pane {
                w.boosted = key;
                if let Some(css) = &boost.css {
                    let js = format!(
                        "(function(){{var s=document.getElementById('nus-boost');if(!s){{s=document.createElement('style');s.id='nus-boost';(document.head||document.documentElement).appendChild(s);}}s.textContent={};}})()",
                        serde_json::to_string(css).unwrap_or_default()
                    );
                    w.tab.eval(&js);
                }
                if let Some(js) = &boost.js {
                    w.tab.eval(js);
                }
            }
        }
    }

    // ── Reader ───────────────────────────────────────────────────────────

    /// Ctrl+Shift+R: extract the focused page's article and set it in
    /// Newsreader over the page; again to go back.
    fn toggle_reader(&mut self) {
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        let pane = match (&tab.left, tab.focus_right) {
            (_, true) if tab.right.is_some() => tab.right.as_mut().unwrap(),
            (Pane::Web(_), _) => &mut tab.left,
            _ => match tab.right.as_mut() {
                Some(r) => r,
                None => return,
            },
        };
        if let Pane::Web(w) = pane {
            if w.reader.take().is_none() {
                w.reader_req = Some(w.tab.eval_reply(crate::reader::EXTRACT_JS));
            }
            self.dirty = true;
        }
    }

    fn poll_reader(&mut self) {
        for tab in self.tabs.iter_mut() {
            for p in std::iter::once(&mut tab.left).chain(tab.right.as_mut()) {
                if let Pane::Web(w) = p {
                    let Some(id) = w.reader_req else { continue };
                    let Some(v) = w.tab.take_reply(id) else { continue };
                    w.reader_req = None;
                    let json = v.pointer("/result/value").and_then(|x| x.as_str()).unwrap_or("");
                    match crate::reader::Article::parse(json) {
                        Some(a) if !a.blocks.is_empty() => w.reader = Some(crate::reader::Reader::new(a)),
                        _ => tracing::info!("reader: nothing to extract"),
                    }
                    self.dirty = true;
                }
            }
        }
    }

    fn reader_fonts(&self) -> crate::reader::ReaderFonts {
        crate::reader::ReaderFonts { serif: self.f.serif, serif_italic: self.f.wordmark, mono: self.f.ui, mono_strong: self.f.strong }
    }

    /// The window icon follows the surface: the n in the base colour (ink
    /// when there is none), the band in the signal.
    pub(crate) fn refresh_icon(&self) {
        let n = self.surface.base.unwrap_or(self.theme.ink);
        let rgba = nus_render::icon::app_icon(64, n, self.surface.signal);
        if let Ok(icon) = winit::window::Icon::from_rgba(rgba, 64, 64) {
            self.window.set_window_icon(Some(icon.clone()));
            if let Some(l) = &self.little {
                l.window.set_window_icon(Some(icon.clone()));
            }
            if let Some(p) = &self.pip {
                p.window.set_window_icon(Some(icon));
            }
        }
    }

    /// Favicons that arrived since last frame become small textures.
    fn sync_favicons(&mut self) {
        let device = self.device.clone();
        let queue = self.gpu.queue.clone();
        let binder = self.bind_texture.clone();
        let mut changed = false;
        for tab in self.tabs.iter_mut() {
            for p in std::iter::once(&mut tab.left).chain(tab.right.as_mut()) {
                let Pane::Web(w) = p else { continue };
                let fav = w.tab.shared.borrow().favicon.clone();
                let Some(f) = fav else { continue };
                if w.favicon.as_ref().is_some_and(|(u, _)| *u == f.url) {
                    continue;
                }
                let tex = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("favicon"),
                    size: wgpu::Extent3d { width: f.w, height: f.h, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Bgra8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                });
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                    &f.bgra,
                    wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(f.w * 4), rows_per_image: Some(f.h) },
                    wgpu::Extent3d { width: f.w, height: f.h, depth_or_array_layers: 1 },
                );
                w.favicon = Some((f.url.clone(), binder(&tex)));
                changed = true;
            }
        }
        if changed {
            self.dirty = true;
        }
    }

    fn anims_active(&self) -> bool {
        self.sidebar_anim.active()
            || self.palette_anim.active()
            || self.band_anim.active()
            || self.tint_anim.active()
            || self.crumb_anim.active()
            || self.row_anims.values().any(|a| a.active())
            || self.tabs.iter().any(|t| {
                std::iter::once(&t.left)
                    .chain(t.right.as_ref())
                    .any(|p| matches!(p, Pane::Web(w) if w.load_fade.active() || w.load.value != w.load.target))
            })
    }

    /// The loading bar for a page: chases progress, fades when done.
    fn draw_load_bar(&mut self, scene: &mut Scene, page: Rect, w: &WebPane, tab_signal: Option<nus_render::Color>) {
        let alpha = w.load_fade.value();
        if alpha <= 0.001 {
            return;
        }
        let v = w.load.value.clamp(0.0, 1.0);
        let base_color = match self.load_bar.color {
            BarColor::Signal => self.surface.signal,
            BarColor::Tab => tab_signal.unwrap_or(self.surface.signal),
            BarColor::Ink => self.theme.ink,
        };
        let col = |a: f32| [base_color[0], base_color[1], base_color[2], base_color[3] * a * alpha];
        let th = self.px(self.load_bar.thickness);
        match self.load_bar.style {
            BarStyle::Rule => scene.rect(Rect::new(page.x, page.y, page.w * v, th), col(1.0)),
            BarStyle::Comet => {
                let head = page.x + page.w * v;
                let tail = (page.w * 0.28).min(head - page.x);
                let segs = 12;
                for k in 0..segs {
                    let f0 = k as f32 / segs as f32;
                    let f1 = (k + 1) as f32 / segs as f32;
                    let x0 = head - tail * (1.0 - f0);
                    let x1 = head - tail * (1.0 - f1);
                    scene.rect(Rect::new(x0, page.y, x1 - x0 + 0.5, th), col(0.12 + 0.88 * f1 * f1));
                }
                scene.rect(Rect::new(head - th * 2.0, page.y, th * 2.0, th), col(1.0));
            }
            BarStyle::Carapace => {
                let sw = self.px(self.surface.shell_width).max(th);
                let win_w = self.target.size.0 as f32;
                let fill = [1.0, 1.0, 1.0, 0.55 * alpha];
                scene.layer(None);
                scene.rect(Rect::new(0.0, 0.0, win_w * v, sw), fill);
            }
        }
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
                        match ev {
                            nus_vt::Event::Title(title) => {
                                t.title = short_title(&title);
                                changed = true;
                            }
                            nus_vt::Event::Bell => {
                                if i != self.active || !self.window.has_focus() {
                                    t.waiting = true;
                                    changed = true;
                                }
                            }
                            _ => {}
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
        for tab in self.tabs.iter_mut() {
            for p in std::iter::once(&mut tab.left).chain(tab.right.as_mut()) {
                if let Pane::Web(w) = p {
                    let paints = w.tab.shared.borrow().paints + w.devtools.as_ref().map(|d| d.shared.borrow().paints).unwrap_or(0);
                    if paints != w.seen_paints {
                        w.seen_paints = paints;
                        changed = true;
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
        if let Some(p) = &self.pip {
            if let Some(tab) = self.tabs.get(p.tab) {
                for pane in std::iter::once(&tab.left).chain(tab.right.as_ref()) {
                    if let Pane::Web(w) = pane {
                        w.tab.begin_frame();
                        if let Some(d) = &w.devtools {
                            d.begin_frame();
                        }
                    }
                }
            }
        }
        if let Some(tab) = self.tabs.get(self.active) {
            for p in std::iter::once(&tab.left).chain(tab.right.as_ref()) {
                if let Pane::Web(w) = p {
                    w.tab.begin_frame();
                    if let Some(d) = &w.devtools {
                        d.begin_frame();
                    }
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
        // Outside a rounded shell: the opposite theme's paper until the window
        // itself is transparent (v1).
        let clear = if self.surface.shell_radius > 0.0 && !self.target.translucent() {
            if self.theme.mode == nus_render::Mode::Ink { Theme::paper().paper } else { Theme::ink().paper }
        } else {
            let p = self.paper();
            let a = if self.target.translucent() { self.surface.opacity } else { 1.0 };
            [p[0] * a, p[1] * a, p[2] * a, a]
        };
        self.gpu.render(&mut self.target, &self.scene, clear);
        self.frames += 1;
    }

    // --- drawing ---------------------------------------------------------

    pub(crate) fn label(&self) -> Style {
        Style {
            font: self.f.ui,
            px: self.px(m::LABEL_PX),
            color: self.theme.ink,
            tracking: self.px(m::LABEL_PX) * m::LABEL_TRACKING,
        }
    }
    pub(crate) fn label_strong(&self) -> Style {
        Style {
            font: self.f.strong,
            ..self.label()
        }
    }
    pub(crate) fn ui(&self) -> Style {
        Style {
            font: self.f.ui,
            px: self.px(m::UI_PX),
            color: self.theme.ink,
            tracking: 0.0,
        }
    }
    pub(crate) fn ui_strong(&self) -> Style {
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
        let w = self.target.size.0 as f32;
        let h = self.target.size.1 as f32;

        // Shell + top strip.
        scene.layer(None);
        let win = Rect::new(0.0, 0.0, w, h);
        let radius = self.px(self.surface.shell_radius);
        let sw = self.px(self.surface.shell_width);
        if radius > 0.0 {
            // The paper is a rounded card; the corners outside it show the clear color.
            scene.push(nus_render::Instance::rounded(win, radius, self.paper()));
        }
        match self.surface.shell {
            Shell::Band => {
                if radius > 0.0 {
                    scene.layer(Some(Rect::new(0.0, 0.0, w, sw)));
                    scene.push(nus_render::Instance::rounded(win, radius, self.surface.signal));
                    scene.layer(None);
                } else {
                    scene.rect(Rect::new(0.0, 0.0, w, sw), self.surface.signal);
                }
            }
            Shell::Stroke => scene.push(nus_render::Instance::stroke(win, radius, sw, self.surface.signal, None, 0.0)),
            Shell::Gradient => scene.push(nus_render::Instance::stroke(win, radius, sw, self.surface.signal, Some(ink), 0.0)),
            Shell::Aurora => scene.push(nus_render::Instance::stroke(win, radius, sw, self.surface.signal, Some(signal::VIOLET), self.shell_phase)),
        }
        // Texture lives on the carapace, never on content.
        if self.surface.texture > 0.0 {
            let g = [1.0, 1.0, 1.0, self.surface.texture];
            let frame = if self.surface.shell == Shell::Band {
                vec![Rect::new(0.0, 0.0, w, sw)]
            } else {
                vec![Rect::new(0.0, 0.0, w, sw), Rect::new(0.0, h - sw, w, sw), Rect::new(0.0, 0.0, sw, h), Rect::new(w - sw, 0.0, sw, h)]
            };
            for r in frame {
                scene.push(nus_render::Instance::grain(r, g, self.scale));
            }
        }
        let strip = self.strip_rect();
        scene.hline(strip.x, strip.bottom(), strip.w, self.px(m::STRUCTURE), ink);
        let ic = self.px(16.0);
        let iy = strip.y + ((strip.h - ic) / 2.0).round();
        let mut x = strip.x + self.px(18.0);
        let base = strip.y + self.px(21.0);
        let wm = Style {
            font: self.f.wordmark,
            px: self.px(m::WORDMARK_PX),
            color: ink,
            tracking: 0.0,
        };
        x += self.fonts.draw(&mut scene, wm, x, base, "nus") + self.px(18.0);
        let label = self.label();
        let dim = Style { color: t.dim, ..label };
        let lbase = strip.y + self.px(19.0);
        // Crumb: Space chip · tab · cwd/host — each a click target (self.crumb_hits).
        self.crumb_hits.clear();
        let (p4, p6, p12, p18, p8) = (self.px(4.0), self.px(6.0), self.px(12.0), self.px(18.0), self.px(8.0));
        let segment = move |hits: &mut Vec<(Rect, CrumbHit)>, x: &mut f32, w: f32, hit: CrumbHit| {
            let r = Rect::new(*x - p6, strip.y + p4, w + p12, strip.h - p8);
            hits.push((r, hit));
            *x += w + p18;
        };
        {
            let sw = self.px(10.0);
            scene.rect(Rect::new(x, strip.y + ((strip.h - sw) / 2.0).round(), sw, sw), self.surface.signal);
            let name = self.space_name.to_uppercase();
            let w = sw + self.px(8.0) + self.fonts.measure(label, &name);
            self.fonts.draw(&mut scene, label, x + sw + self.px(8.0), lbase, &name);
            segment(&mut self.crumb_hits, &mut x, w, CrumbHit::Space);
        }
        let (focused_web, url) = {
            let tab = &self.tabs[self.active];
            let pane = if tab.focus_right && tab.right.is_some() { tab.right.as_ref().unwrap() } else { &tab.left };
            match pane {
                Pane::Web(w) => (true, w.tab.shared.borrow().url.clone()),
                _ => (false, String::new()),
            }
        };
        if focused_web {
            // The crumb is the site: favicon, title, host. Click to edit the URL.
            let (title, fav) = {
                let tab = &self.tabs[self.active];
                let pane = if tab.focus_right && tab.right.is_some() { tab.right.as_ref().unwrap() } else { &tab.left };
                match pane {
                    Pane::Web(w) => (w.tab.shared.borrow().title.clone(), w.favicon.as_ref().map(|(_, b)| b.clone())),
                    _ => (String::new(), None),
                }
            };
            let host = url.split("//").nth(1).unwrap_or(&url).split('/').next().unwrap_or("").trim_start_matches("www.").to_string();
            let ui = self.ui();
            let dim_ui = Style { color: t.dim, ..ui };
            let maxw = (strip.w * 0.42).min(self.px(640.0));
            let start = x;
            match fav {
                Some(b) => {
                    scene.texture(Rect::new(x, iy, ic, ic), b, None);
                    scene.layer(None);
                }
                None => {
                    self.fonts.draw_icon(&mut scene, nus_render::text::icons::GLOBE, ic, x, iy, ink);
                }
            }
            x += ic + self.px(8.0);
            let shown_title = if title.is_empty() { host.clone() } else { title.clone() };
            let host_text = if title.is_empty() || host.is_empty() { String::new() } else { format!(" · {host}") };
            let hw = self.fonts.measure(dim_ui, &host_text);
            let fade = Style { color: Theme::with_alpha(ink, self.crumb_anim.value()), ..ui };
            let tfit = self.fit(fade, &shown_title, maxw - (x - start) - hw);
            x += self.fonts.draw(&mut scene, fade, x, lbase, &tfit);
            if !host_text.is_empty() {
                x += self.fonts.draw(&mut scene, dim_ui, x, lbase, &host_text);
            }
            let field = Rect::new(start, strip.y, x - start + self.px(8.0), strip.h);
            if is_local(&url) {
                scene.hline(start, strip.bottom() - self.px(3.0), x - start, self.px(2.0), self.surface.signal);
            }
            self.crumb_hits.push((field, CrumbHit::Url));
            x += self.px(18.0);
        } else {
            let tab = &self.tabs[self.active];
            let (icon, title) = match &tab.left {
                Pane::Term(p) => (nus_render::text::icons::TERMINAL, p.title.clone()),
                Pane::Web(_) => (nus_render::text::icons::GLOBE, tab.title()),
                Pane::Settings(_) => (nus_render::text::icons::SETTINGS, "settings".into()),
                Pane::Hints(_) => (nus_render::text::icons::HOME, "welcome".into()),
            };
            let title = format!("{} {}", self.tab_label(self.active), title).to_uppercase();
            let tw = self.fonts.measure(label, &title);
            let fade = Style { color: Theme::with_alpha(ink, self.crumb_anim.value()), ..label };
            self.fonts.draw_icon(&mut scene, icon, ic, x, iy, ink);
            self.fonts.draw(&mut scene, fade, x + ic + self.px(8.0), lbase, &title);
            segment(&mut self.crumb_hits, &mut x, ic + p8 + tw, CrumbHit::Tab);
        }
        // Right side: status cluster, search, sidebar, window controls.
        let mut rx = strip.right() - self.px(18.0);
        let gap = self.px(14.0);
        if !cfg!(target_os = "macos") {
            for (icon, hit) in [
                (nus_render::text::icons::CLOSE, CrumbHit::Close),
                (nus_render::text::icons::MAXIMIZE, CrumbHit::Maximize),
                (nus_render::text::icons::MINIMIZE, CrumbHit::Minimize),
            ] {
                rx -= ic;
                self.fonts.draw_icon(&mut scene, icon, ic, rx, iy, ink);
                self.crumb_hits.push((Rect::new(rx - p6, strip.y, ic + p12, strip.h), hit));
                rx -= gap;
            }
            rx -= self.px(6.0);
        }
        rx -= ic;
        self.fonts.draw_icon(&mut scene, nus_render::text::icons::SIDEBAR, ic, rx, iy, if self.sidebar { ink } else { t.dim });
        self.crumb_hits.push((Rect::new(rx - self.px(4.0), strip.y, ic + self.px(8.0), strip.h), CrumbHit::Sidebar));
        rx -= gap + ic;
        self.fonts.draw_icon(&mut scene, nus_render::text::icons::SEARCH, ic, rx, iy, ink);
        self.crumb_hits.push((Rect::new(rx - self.px(4.0), strip.y, ic + self.px(8.0), strip.h), CrumbHit::Search));
        rx -= gap;
        // status cluster: waiting · pip · assistant · ports (one icon when narrow)
        let waiting = self.tabs.iter().filter(|t| t.waiting()).count();
        let mut cluster: Vec<((&'static str, &'static str), String, CrumbHit, bool)> = Vec::new();
        if self.width_class() == Width::Narrow {
            let lit = waiting > 0 || self.pip.is_some();
            let count = if waiting > 0 { waiting.to_string() } else { String::new() };
            cluster.push((nus_render::text::icons::MORE, count, CrumbHit::Waiting, lit));
        } else {
        if !self.ports.is_empty() || true {
            cluster.push((nus_render::text::icons::PORTS, if self.ports.is_empty() { String::new() } else { self.ports.len().to_string() }, CrumbHit::Ports, !self.ports.is_empty()));
        }
        cluster.push((nus_render::text::icons::ASSISTANT, String::new(), CrumbHit::Assistant, !self.llm_tools.is_empty()));
        if self.pip.is_some() {
            cluster.push((nus_render::text::icons::PIP, String::new(), CrumbHit::Pip, true));
        }
        cluster.push((if waiting > 0 { nus_render::text::icons::BELL_BOLD } else { nus_render::text::icons::BELL }, if waiting > 0 { waiting.to_string() } else { String::new() }, CrumbHit::Waiting, waiting > 0));
        }
        for (icon, count, hit, lit) in cluster {
            let color = if lit { ink } else { t.dim };
            if !count.is_empty() {
                let cw = self.fonts.measure(label, &count);
                rx -= cw;
                self.fonts.draw(&mut scene, Style { color: if hit == CrumbHit::Waiting { self.surface.signal } else { color }, ..label }, rx, lbase, &count);
                rx -= self.px(4.0);
            }
            rx -= ic;
            self.fonts.draw_icon(&mut scene, icon, ic, rx, iy, if hit == CrumbHit::Waiting && lit { self.surface.signal } else { color });
            self.crumb_hits.push((Rect::new(rx - self.px(4.0), strip.y, ic + self.px(8.0), strip.h), hit));
            rx -= gap;
        }
        let _ = dim;

        // Sidebar or hot edge.
        let c = self.content_rect();
        let right_side = self.sidebar_right();
        if self.sidebar_pinned() {
            self.draw_sidebar(&mut scene);
            let x = if right_side { c.right() } else { c.x - self.px(m::STRUCTURE) };
            scene.vline(x, c.y, c.h, self.px(m::STRUCTURE), ink);
        } else if !self.sidebar_hover && self.sidebar_hoverable() {
            let x = if right_side { c.right() } else { c.x - self.px(4.0) };
            scene.rect(Rect::new(x, c.y, self.px(4.0), c.h), t.hot);
        }

        // Panes.
        let active = self.active;
        let focus_right = self.tabs[active].focus_right;
        let has_right = self.tabs[active].right.is_some();
        // Split rule.
        let narrow = self.width_class() == Width::Narrow;
        if has_right && !narrow {
            let r = match &self.tabs[active].right {
                Some(Pane::Term(p)) => p.rect,
                Some(Pane::Web(p)) => p.rect,
                Some(Pane::Settings(p)) => p.rect,
                Some(Pane::Hints(p)) => p.rect,
                None => unreachable!(),
            };
            scene.vline(r.x - self.px(m::STRUCTURE), r.y, r.h, self.px(m::STRUCTURE), ink);
        }
        let n = self.tab_label(active);
        let look = self.tabs[active].look.clone();
        let mut tabs = std::mem::take(&mut self.tabs);
        {
            let tab = &mut tabs[active];
            let left_focused = !(focus_right && has_right);
            if !(narrow && has_right && !left_focused) {
                self.draw_pane(&mut scene, &mut tab.left, &n, left_focused, &look);
            }
            if let Some(r) = tab.right.as_mut() {
                if !(narrow && left_focused) {
                    self.draw_pane(&mut scene, r, &n, !left_focused, &look);
                }
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

        // Stack close confirmation: a band across the content.
        if let Some(root) = self.confirm_stack {
            let c = self.content_rect();
            let hh = self.header_h();
            let drop = self.band_anim.value();
            let cr = Rect::new(c.x, c.y - (1.0 - drop) * hh, c.w, hh);
            scene.layer(Some(Rect::new(c.x, c.y, c.w, hh)));
            scene.rect(cr, ink);
            let inv = Style { color: t.paper, ..self.label_strong() };
            let inv_l = Style { color: t.paper, ..label };
            let by = cr.y + self.px(m::HEADER_PAD_Y) + self.px(m::UI_PX) - self.px(3.0);
            let mut x = cr.x + self.px(m::HEADER_PAD_X);
            let n = self.children(root).len();
            x += self.fonts.draw(&mut scene, inv, x, by, &format!("STACK OF {}", n + 1)) + self.px(14.0);
            x += self.fonts.draw(&mut scene, inv_l, x, by, &format!("CLOSE THIS TAB AND ITS {n} PAGE{}?", if n == 1 { "" } else { "S" })) + self.px(14.0);
            x += self.fonts.draw(&mut scene, inv, x, by, "ENTER") + self.px(14.0);
            self.fonts.draw(&mut scene, inv_l, x, by, "· ESC KEEPS THEM");
        }

        // Hover-revealed sidebar slides over the content.
        let slide = self.sidebar_anim.value();
        if slide > 0.001 && !self.sidebar_pinned() {
            let sb = self.sidebar_rect();
            let off = (1.0 - slide) * (sb.w + self.px(12.0));
            let sb = if self.sidebar_right() { Rect::new(sb.x + off, sb.y, sb.w, sb.h) } else { Rect::new(sb.x - off, sb.y, sb.w, sb.h) };
            let shadow = if self.sidebar_right() { -self.px(8.0) } else { self.px(8.0) };
            scene.layer(None);
            scene.rect(Rect::new(sb.x + shadow, sb.y, sb.w, sb.h), Theme::with_alpha(ink, 0.18 * slide));
            scene.rect(sb, self.paper());
            self.sidebar_shift = sb.x - self.sidebar_rect().x;
            self.draw_sidebar(&mut scene);
            self.sidebar_shift = 0.0;
            scene.layer(None);
            let x = if self.sidebar_right() { sb.x - self.px(m::STRUCTURE) } else { sb.right() };
            scene.vline(x, sb.y, sb.h, self.px(m::STRUCTURE), ink);
        }

        // Palette.
        if let Some((mode, input)) = self.palette.clone() {
            scene.layer(None);
            let rise = self.palette_anim.value();
            scene.rect(Rect::new(0.0, 0.0, w, h), Theme::with_alpha(t.scrim, t.scrim[3] * rise));
            let pw = self.px(m::PALETTE).min(w - 2.0 * self.px(16.0));
            let bx = ((w - pw) / 2.0).round();
            let by = self.px(220.0).min(h * 0.12) + (1.0 - rise) * self.px(10.0);
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
            // Long input keeps the caret in view: the head scrolls off, clipped.
            let esc = self.label();
            let ew = self.fonts.measure(esc, "ESC");
            let avail = r.right() - self.px(18.0) - ew - self.px(18.0) - self.px(12.0) - px;
            let iw = self.fonts.measure(big, &input);
            let field = Rect::new(px, r.y, avail, head_h);
            scene.layer(Some(field));
            let shift = (iw - avail).max(0.0);
            px += self.fonts.draw(&mut scene, big, px - shift, base, &input) - shift;
            scene.layer(None);
            scene.rect(Rect::new(px + self.px(2.0), base - self.px(14.0), self.px(9.0), self.px(18.0)), ink);
            self.fonts.draw(&mut scene, esc, r.right() - self.px(18.0) - ew, base - self.px(2.0), "ESC");
            scene.hline(r.x, r.y + head_h - self.px(2.0), r.w, self.px(2.0), ink);
            // rows
            let mut y = r.y + head_h;
            self.palette_hits.clear();
            for (i, PaletteRow { num, text, .. }) in rows.iter().enumerate() {
                self.palette_hits.push(Rect::new(r.x, y, r.w, row_h));
                let sel = i == self.palette_sel;
                let (fg, bg) = if sel { (t.paper, Some(ink)) } else { (ink, None) };
                if let Some(bg) = bg {
                    scene.rect(Rect::new(r.x, y, r.w, row_h), bg);
                }
                let strong = Style { color: fg, ..self.ui_strong() };
                let ui = Style { color: fg, ..self.ui() };
                let base = y + self.px(10.0) + self.px(m::UI_PX) - self.px(3.0);
                let mut px = r.x + self.px(18.0);
                let icon = match num.as_str() {
                    "?" => Some(nus_render::text::icons::SEARCH),
                    "→" => Some(nus_render::text::icons::GLOBE),
                    ">" => Some(nus_render::text::icons::TERMINAL),
                    "::" => Some(nus_render::text::icons::PORTS),
                    "·" => Some(nus_render::text::icons::COMMAND),
                    "*" => Some(nus_render::text::icons::ASSISTANT),
                    _ => None,
                };
                match icon {
                    Some(icon) => {
                        let isz = self.px(16.0);
                        self.fonts.draw_icon(&mut scene, icon, isz, px, base - isz + self.px(3.0), fg);
                        px += self.px(24.0) + self.px(12.0);
                    }
                    None => px += self.fonts.draw(&mut scene, strong, px, base, num) + self.px(12.0),
                }
                let text = self.fit(ui, text, r.right() - self.px(18.0) - px);
                self.fonts.draw(&mut scene, ui, px, base, &text);
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

    /// Sidebar layout: the pinned row, then one entry per listed tab with its
    /// y and height (previews expand under the hovered row and waiting tabs).
    pub(crate) fn sidebar_geometry(&self) -> SidebarGeom {
        let sb = self.sidebar_rect();
        let space_row = self.px(9.0) * 2.0 + self.px(m::LABEL_PX) + self.px(m::STRUCTURE);
        let pinned: Vec<usize> = (0..self.tabs.len()).filter(|&i| self.tabs[i].pinned).collect();
        let pinned_h = if pinned.is_empty() { 0.0 } else { self.px(8.0) * 2.0 + self.px(m::UI_PX) + self.px(m::STRUCTURE) };
        let compact = self.px(9.0) * 2.0 + self.px(m::UI_PX) + self.px(m::HAIRLINE);
        let expanded = compact + self.px(8.0) + self.px(m::PREVIEW_H) + self.px(3.0);
        let mut y = sb.y + space_row + pinned_h;
        let mut rows = Vec::new();
        for i in 0..self.tabs.len() {
            if self.tabs[i].pinned {
                continue;
            }
            let hidden = self.tabs[i].parent.is_some() && !self.stack_open(self.stack_root(i));
            let waiting = self.tabs[i].waiting();
            let want = if hidden {
                0.0
            } else if self.hover_row == Some(i) || waiting {
                expanded
            } else {
                compact
            };
            // Animated height when one is running; otherwise the target.
            let h = match self.row_anims.get(&self.tabs[i].id) {
                Some(a) if a.active() => a.value(),
                _ => want,
            };
            if h < 0.5 {
                continue;
            }
            rows.push((i, y, h));
            y += h;
        }
        let foot_rows = 4.0;
        let foot_h = self.px(10.0) * 2.0 + self.px(22.0) + self.px(m::HAIRLINE)
            + (foot_rows - 1.0) * (self.px(8.0) * 2.0 + self.px(m::LABEL_PX) + self.px(m::HAIRLINE))
            + self.px(m::STRUCTURE);
        SidebarGeom { pinned, pinned_h, rows, foot_y: sb.bottom() - foot_h }
    }

    fn draw_sidebar(&mut self, scene: &mut Scene) {
        let t = self.theme.clone();
        let ink = t.ink;
        let sb = self.sidebar_rect();
        let label = self.label();
        let strong = self.label_strong();
        let ui = self.ui();
        let ui_strong = self.ui_strong();
        let dim = Style { color: t.dim, ..label };

        // Space row: a window switcher (Spaces are windows).
        let row_h = self.px(9.0) * 2.0 + self.px(m::LABEL_PX) + self.px(m::STRUCTURE);
        let cell_w = (sb.w / 3.0).floor();
        scene.rect(Rect::new(sb.x, sb.y, cell_w, row_h - self.px(m::STRUCTURE)), ink);
        scene.rect(Rect::new(sb.x + self.px(10.0), sb.y + self.px(10.0), self.px(10.0), self.px(10.0)), self.surface.signal);
        let sel = Style { color: t.paper, ..label };
        self.fonts.draw(scene, sel, sb.x + self.px(28.0), sb.y + self.px(19.0), &self.space_name.to_uppercase());
        scene.vline(sb.x + cell_w, sb.y, row_h, self.px(m::HAIRLINE), ink);
        {
            let isz = self.px(12.0);
            let x = sb.x + cell_w + self.px(10.0);
            self.fonts.draw_icon(scene, nus_render::text::icons::PLUS, isz, x, sb.y + self.px(19.0) - isz + self.px(2.0), t.dim);
            self.fonts.draw(scene, dim, x + isz + self.px(6.0), sb.y + self.px(19.0), "SPACE");
        }
        scene.hline(sb.x, sb.y + row_h - self.px(m::STRUCTURE), sb.w, self.px(m::STRUCTURE), ink);

        let g = self.sidebar_geometry();
        let tabs = std::mem::take(&mut self.tabs);

        // Pinned row.
        if !g.pinned.is_empty() {
            let py = sb.y + row_h;
            let cell_w = (sb.w / g.pinned.len() as f32).floor();
            for (k, &i) in g.pinned.iter().enumerate() {
                let cx = sb.x + k as f32 * cell_w;
                let cell = Rect::new(cx, py, cell_w, g.pinned_h - self.px(m::STRUCTURE));
                let active = i == self.active;
                if active {
                    scene.rect(cell, ink);
                }
                let st = Style { color: if active { t.paper } else { ink }, ..ui_strong };
                let base = py + self.px(8.0) + self.px(m::UI_PX) - self.px(3.0);
                let mut x = cx + self.px(10.0);
                let isz = self.px(12.0);
                self.fonts.draw_icon(scene, nus_render::text::icons::PIN, isz, x, base - isz + self.px(2.0), st.color);
                x += isz + self.px(8.0);
                let _ = k;
                let title = self.fit(st, &tabs[i].title(), cell_w - (x - cx) - self.px(10.0));
                self.fonts.draw(scene, Style { font: self.f.ui, ..st }, x, base, &title);
                if self.selected.contains(&i) {
                    scene.outline(cell, self.px(m::STRUCTURE), ink);
                }
                if k + 1 < g.pinned.len() {
                    scene.vline(cx + cell_w, py, g.pinned_h, self.px(m::HAIRLINE), ink);
                }
            }
            scene.hline(sb.x, py + g.pinned_h - self.px(m::STRUCTURE), sb.w, self.px(m::STRUCTURE), ink);
        }

        // Tab rows.
        let pad_x = self.px(m::ROW_PAD_X);
        let labels: Vec<String> = g.rows.iter().map(|&(i, _, _)| self.tab_label_of(&tabs, i)).collect();
        for (k, &(i, y, h)) in g.rows.iter().enumerate() {
            let tab = &tabs[i];
            let waiting = tab.waiting();
            let child = tab.parent.is_some();
            let stack: Vec<usize> = if child { Vec::new() } else { (0..tabs.len()).filter(|&j| tabs[j].parent == Some(tab.id)).collect() };
            let open = !stack.is_empty() && (i == self.active || stack.contains(&self.active));
            if i == self.active {
                let ty = if self.tint_anim.active() { self.tint_anim.value() } else { y };
                scene.rect(Rect::new(sb.x, ty, sb.w, h), t.tint);
            }
            scene.layer(Some(Rect::new(sb.x, y, sb.w, h)));
            if self.selected.contains(&i) {
                scene.outline(Rect::new(sb.x, y, sb.w, h - self.px(m::HAIRLINE)), self.px(m::STRUCTURE), ink);
            }
            let base = y + self.px(9.0) + self.px(m::UI_PX) - self.px(3.0);
            let mut x = sb.x + pad_x;
            if child {
                // Children hang off a rule under the parent's number.
                let cx = x + self.px(6.0);
                scene.vline(cx, y, h - self.px(m::HAIRLINE), self.px(m::HAIRLINE), ink);
                x += self.px(18.0);
                x += self.fonts.draw(scene, dim, x, base, &labels[k][labels[k].len() - 1..]) + self.px(10.0);
            } else {
                let num = Style { color: tab.look.signal.unwrap_or(ink), ..ui_strong };
                x += self.fonts.draw(scene, num, x, base, &labels[k]) + self.px(10.0);
            }
            let icon = match &tab.left {
                Pane::Term(_) => nus_render::text::icons::TERMINAL,
                Pane::Web(_) => nus_render::text::icons::GLOBE,
                Pane::Settings(_) => nus_render::text::icons::SETTINGS,
                Pane::Hints(_) => nus_render::text::icons::HOME,
            };
            let isz = self.px(14.0);
            let fav = match &tab.left {
                Pane::Web(w) => w.favicon.as_ref().map(|(_, b)| b.clone()),
                _ => None,
            };
            match fav {
                Some(b) => {
                    scene.texture(Rect::new(x, base - isz + self.px(2.0), isz, isz), b, None);
                    scene.layer(Some(Rect::new(sb.x, y, sb.w, h)));
                    x += isz + self.px(10.0);
                }
                None => x += self.fonts.draw_icon(scene, icon, isz, x, base - isz + self.px(2.0), ink) + self.px(10.0),
            }
            let (title, detail) = tab.row_text();
            let tag = if waiting {
                "WAITING".to_string()
            } else if !stack.is_empty() && !open {
                format!("+{}", stack.len())
            } else {
                detail.to_uppercase()
            };
            let tag_w = if tag.is_empty() { 0.0 } else { self.fonts.measure(label, &tag) + self.px(12.0) };
            let st = if i == self.active { ui_strong } else { ui };
            let title = self.fit(st, &title, sb.w - (x - sb.x) - pad_x - tag_w);
            self.fonts.draw(scene, st, x, base, &title);
            if waiting {
                let w = self.fonts.measure(strong, &tag) + self.px(12.0);
                let r = Rect::new(sb.right() - pad_x - w, base - self.px(m::LABEL_PX) - self.px(1.0), w, self.px(m::LABEL_PX) + self.px(4.0));
                scene.rect(r, self.surface.signal);
                self.fonts.draw(scene, Style { color: [1.0, 1.0, 1.0, 1.0], ..strong }, r.x + self.px(6.0), base, &tag);
            } else if !stack.is_empty() && !open {
                // Collapsed stack: count and a caret.
                let isz = self.px(12.0);
                let tx = sb.right() - pad_x - tag_w + self.px(12.0);
                self.fonts.draw(scene, dim, tx, base, &tag);
                self.fonts.draw_icon(scene, nus_render::text::icons::CARET_RIGHT, isz, tx - isz - self.px(4.0), base - isz + self.px(2.0), t.dim);
            } else if !tag.is_empty() {
                self.fonts.draw(scene, dim, sb.right() - pad_x - tag_w + self.px(12.0), base, &tag);
            }
            // Preview, when expanded.
            if h > self.px(40.0) {
                let pr = Rect::new(sb.x + pad_x, base + self.px(8.0), sb.w - 2.0 * pad_x, self.px(m::PREVIEW_H));
                scene.outline(pr, self.px(m::HAIRLINE), ink);
                match &tab.left {
                    Pane::Term(tp) => {
                        let small = Style { font: self.f.ui, px: self.px(7.5), color: ink, tracking: 0.0 };
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
                    Pane::Settings(_) | Pane::Hints(_) => {}
                }
            }
            scene.hline(sb.x, y + h - self.px(m::HAIRLINE), sb.w, self.px(m::HAIRLINE), ink);
            scene.layer(None);
        }
        self.tabs = tabs;

        // Footer: identity, shell, assistants, new tab / settings.
        let fy = g.foot_y;
        scene.hline(sb.x, fy, sb.w, self.px(m::STRUCTURE), ink);
        let mut y = fy + self.px(m::STRUCTURE);
        // identity
        let id_h = self.px(10.0) * 2.0 + self.px(22.0);
        let av = Rect::new(sb.x + pad_x, y + self.px(10.0), self.px(22.0), self.px(22.0));
        scene.rect(av, self.surface.signal);
        let initial = self.user_initial();
        let iw = self.fonts.measure(ui_strong, &initial);
        self.fonts.draw(scene, Style { color: [1.0, 1.0, 1.0, 1.0], ..ui_strong }, av.x + (av.w - iw) / 2.0, av.y + self.px(16.0), &initial);
        let tx = av.right() + self.px(10.0);
        self.fonts.draw(scene, ui_strong, tx, y + self.px(10.0) + self.px(11.0), &self.space_name);
        {
            let by = y + self.px(10.0) + self.px(24.0);
            let mut x = tx;
            x += self.fonts.draw(scene, dim, x, by, &format!("{} ·", self.user_name.to_uppercase())) + self.px(6.0);
            let isz = self.px(11.0);
            self.fonts.draw_icon(scene, nus_render::text::icons::COOKIE, isz, x, by - isz + self.px(2.0), t.dim);
            self.fonts.draw(scene, dim, x + isz + self.px(4.0), by, &self.space_name.to_uppercase());
        }
        let isz = self.px(16.0);
        self.fonts.draw_icon(scene, nus_render::text::icons::MORE, isz, sb.right() - pad_x - isz, y + self.px(10.0) + self.px(3.0), ink);
        y += id_h;
        scene.hline(sb.x, y, sb.w, self.px(m::HAIRLINE), ink);
        y += self.px(m::HAIRLINE);
        // shell row
        let lr = self.px(8.0) * 2.0 + self.px(m::LABEL_PX);
        let base = y + self.px(8.0) + self.px(m::LABEL_PX) - self.px(2.0);
        let mut x = sb.x + pad_x;
        {
            let isz = self.px(12.0);
            self.fonts.draw_icon(scene, nus_render::text::icons::TERMINAL, isz, x, base - isz + self.px(2.0), t.dim);
            x += isz + self.px(10.0);
        }
        let default = self.profiles.first().map(|p| p.name.clone()).unwrap_or_default();
        x += self.fonts.draw(scene, strong, x, base, &default.to_uppercase()) + self.px(10.0);
        let others: Vec<String> = self.profiles.iter().skip(1).map(|p| p.name.to_uppercase()).collect();
        if !others.is_empty() {
            let s = self.fit(dim, &format!("· {}", others.join(" · ")), sb.right() - pad_x - x);
            self.fonts.draw(scene, dim, x, base, &s);
        }
        y += lr;
        scene.hline(sb.x, y, sb.w, self.px(m::HAIRLINE), ink);
        y += self.px(m::HAIRLINE);
        // assistants row
        let base = y + self.px(8.0) + self.px(m::LABEL_PX) - self.px(2.0);
        let mut x = sb.x + pad_x;
        {
            let isz = self.px(12.0);
            self.fonts.draw_icon(scene, nus_render::text::icons::ASSISTANT, isz, x, base - isz + self.px(2.0), t.dim);
            x += isz + self.px(10.0);
        }
        let mut names: Vec<String> = self.llm_tools.iter().map(|(n, _)| n.to_uppercase()).collect();
        names.push("CHATGPT".into());
        names.push("CLAUDE.AI".into());
        x += self.fonts.draw(scene, strong, x, base, &names[0]) + self.px(10.0);
        let s = self.fit(dim, &format!("· {}", names[1..].join(" · ")), sb.right() - pad_x - x);
        self.fonts.draw(scene, dim, x, base, &s);
        y += lr;
        scene.hline(sb.x, y, sb.w, self.px(m::HAIRLINE), ink);
        y += self.px(m::HAIRLINE);
        // new tab / settings
        let base = y + self.px(8.0) + self.px(m::LABEL_PX) - self.px(2.0);
        let isz = self.px(14.0);
        self.fonts.draw_icon(scene, nus_render::text::icons::PLUS, isz, sb.x + pad_x, base - isz + self.px(2.0), ink);
        self.fonts.draw(scene, label, sb.x + pad_x + isz + self.px(8.0), base, "NEW TAB");
        let ks = key(",", false);
        let kw = self.fonts.measure(label, &ks);
        self.fonts.draw(scene, dim, sb.right() - pad_x - kw, base, &ks);
        self.fonts.draw_icon(scene, nus_render::text::icons::SETTINGS, isz, sb.right() - pad_x - kw - self.px(8.0) - isz, base - isz + self.px(2.0), ink);
    }

    fn user_initial(&self) -> String {
        self.user_name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "?".into())
    }

    /// Ctrl+, — open (or switch to) the settings tab.
    fn open_settings(&mut self) {
        self.refresh_register_note();
        if let Some(i) = self.tabs.iter().position(|t| matches!(t.left, Pane::Settings(_))) {
            return self.activate(i);
        }
        let tab = self.make_tab(Pane::Settings(SettingsPane { rect: Rect::new(0.0, 0.0, 1.0, 1.0), section: 0, drill: false }), None);
        self.tabs.push(tab);
        self.activate(self.tabs.len() - 1);
    }

    // ── Onboarding ─────────────────────────────────────────────────────
    // No wizard: the first launch is a real shell with this panel beside it.

    fn onboarded_marker() -> std::path::PathBuf {
        std::env::current_dir().unwrap_or_default().join("profile").join("onboarded")
    }

    /// The marker holds the ticks ("10110") or "skip"; done when all five.
    fn onboarded() -> bool {
        if std::env::var_os("NUS_ONBOARD").is_some() {
            return false;
        }
        let s = std::fs::read_to_string(App::onboarded_marker()).unwrap_or_default();
        s.trim() == "skip" || s.trim() == "11111"
    }

    fn load_hints() -> [bool; 5] {
        let s = std::fs::read_to_string(App::onboarded_marker()).unwrap_or_default();
        let mut h = [false; 5];
        for (k, c) in s.trim().chars().take(5).enumerate() {
            h[k] = c == '1';
        }
        h
    }

    fn save_hints(&self) {
        let s: String = self.hints.iter().map(|&h| if h { '1' } else { '0' }).collect();
        let _ = std::fs::write(App::onboarded_marker(), s);
    }

    fn tick_hint(&mut self, k: usize) {
        if self.hints[k] || !self.hints_open() {
            return;
        }
        self.hints[k] = true;
        self.dirty = true;
        self.save_hints();
    }

    fn hints_open(&self) -> bool {
        self.tabs.iter().any(|t| matches!(t.right, Some(Pane::Hints(_))) || matches!(t.left, Pane::Hints(_)))
    }

    /// Remove the panel (skip, or done). The marker is written either way.
    pub(crate) fn dismiss_hints(&mut self) {
        if !self.hints.iter().all(|&h| h) {
            let _ = std::fs::write(App::onboarded_marker(), b"skip");
        }
        for t in self.tabs.iter_mut() {
            if matches!(t.right, Some(Pane::Hints(_))) {
                t.right = None;
                t.focus_right = false;
            }
        }
        self.layout();
        self.dirty = true;
    }

    fn draw_hints(&mut self, scene: &mut Scene, r: Rect) {
        let t = self.theme.clone();
        let ink = t.ink;
        let label = self.label();
        let strong = self.label_strong();
        let ui = self.ui();
        let dim = Style { color: t.dim, ..label };
        self.hint_hits.clear();
        let pad = self.px(28.0);
        let mut y = r.y + self.px(30.0);
        // Masthead.
        let wm = Style { font: self.f.wordmark, px: self.px(44.0), color: ink, tracking: 0.0 };
        self.fonts.draw(scene, wm, r.x + pad, y + self.px(36.0), "nus");
        let done = self.hints.iter().filter(|&&h| h).count();
        let tally = format!("{done} OF 5");
        let tw = self.fonts.measure(label, &tally);
        self.fonts.draw(scene, dim, r.right() - pad - tw, y + self.px(30.0), &tally);
        y += self.px(58.0);
        self.fonts.draw(scene, strong, r.x + pad, y, "FIVE THINGS TO TRY");
        y += self.px(12.0);
        scene.hline(r.x + pad, y, r.w - 2.0 * pad, self.px(m::STRUCTURE), ink);
        y += self.px(m::STRUCTURE);
        // Rows: a box, the chord, then what it does. Ticked rows dim.
        let row_h = self.px(60.0);
        let box_sz = self.px(14.0);
        for (k, (chord, what)) in HINTS.iter().enumerate() {
            let ticked = self.hints[k];
            let ry = y;
            let base = ry + self.px(24.0);
            let bx = r.x + pad;
            let by = base - box_sz + self.px(2.0);
            if ticked {
                scene.rect(Rect::new(bx, by, box_sz, box_sz), self.surface.signal);
                self.fonts.draw_icon(scene, nus_render::text::icons::CHECK, box_sz, bx, by, [1.0, 1.0, 1.0, 1.0]);
            } else {
                scene.outline(Rect::new(bx, by, box_sz, box_sz), self.px(m::HAIRLINE), ink);
            }
            let chord = match *chord {
                "EDGE" => "HOVER THE EDGE".to_string(),
                "ENTER" => "ENTER".to_string(),
                c => key(c, c != "`"),
            };
            let cs = Style { color: if ticked { t.dim } else { ink }, ..strong };
            let x = bx + box_sz + self.px(14.0);
            self.fonts.draw(scene, cs, x, base, &chord);
            let ws = Style { color: if ticked { t.dim } else { ink }, ..ui };
            let what = self.fit(ws, what, r.right() - pad - x);
            self.fonts.draw(scene, ws, x, base + self.px(20.0), &what);
            scene.hline(r.x + pad, ry + row_h - self.px(m::HAIRLINE), r.w - 2.0 * pad, self.px(m::HAIRLINE), ink);
            self.hint_hits.push((Rect::new(r.x, ry, r.w, row_h), k));
            y += row_h;
        }
        // Foot: skip, or close when done.
        let fy = r.bottom() - self.px(44.0);
        scene.hline(r.x + pad, fy, r.w - 2.0 * pad, self.px(m::STRUCTURE), ink);
        let base = fy + self.px(26.0);
        let word = if done == 5 { "ALL FIVE · CLOSE THIS PANEL" } else { "SKIP THE TOUR" };
        let st = if done == 5 { Style { color: self.surface.signal, ..strong } } else { dim };
        let ww = self.fonts.draw(scene, st, r.x + pad, base, word);
        self.hint_hits.push((Rect::new(r.x + pad, fy, ww + self.px(20.0), self.px(44.0)), usize::MAX));
        let note = "SETTINGS REMEMBER YOU SKIPPED";
        let nw = self.fonts.measure(dim, note);
        if r.w > nw + ww + 3.0 * pad {
            self.fonts.draw(scene, dim, r.right() - pad - nw, base, note);
        }
    }

    fn draw_pane(&mut self, scene: &mut Scene, pane: &mut Pane, n: &str, focused: bool, look: &Overrides) {
        let t = self.theme.clone();
        let ink = t.ink;
        let label = self.label();
        let strong = self.label_strong();
        match pane {
            Pane::Settings(p) => {
                let p = SettingsPane { rect: p.rect, section: p.section, drill: p.drill };
                self.draw_settings(scene, &p);
            }
            Pane::Hints(p) => {
                let r = p.rect;
                self.draw_hints(scene, r);
            }
            Pane::Term(p) => {
                let r = p.rect;
                let hh = self.header_h();
                let base = r.y + self.px(m::HEADER_PAD_Y) + self.px(m::UI_PX) - self.px(3.0);
                let mut x = r.x + self.px(m::HEADER_PAD_X);
                let isz = self.px(13.0);
                self.fonts.draw_icon(scene, nus_render::text::icons::TERMINAL, isz, x, base - isz + self.px(2.0), ink);
                x += isz + self.px(8.0);
                x += self.fonts.draw(scene, strong, x, base, &format!("{} · {}", n, p.title).to_uppercase()) + self.px(14.0);
                let dims = format!("{}×{}", p.term.cols(), p.term.rows());
                let dw = self.fonts.measure(label, &dims);
                let dx = r.right() - self.px(m::HEADER_PAD_X) - dw;
                self.fonts.draw(scene, label, dx, base, &dims);
                self.fonts.draw_icon(scene, nus_render::text::icons::EXPAND, isz, dx - isz - self.px(6.0), base - isz + self.px(2.0), t.dim);
                let _ = x;
                scene.hline(r.x, r.y + hh - self.px(m::HAIRLINE), r.w, self.px(m::HAIRLINE), ink);
                if let Some(proc_name) = p.confirm_close.clone() {
                    let drop = self.band_anim.value();
                    let cr = Rect::new(r.x, r.y + hh - (1.0 - drop) * hh, r.w, hh);
                    scene.layer(Some(Rect::new(r.x, r.y + hh, r.w, hh)));
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
                // The rules' paper for this tab; carries the window opacity so
                // a translucent window stays translucent.
                if let Some(bg) = look.bg {
                    let a = if self.target.translucent() { self.surface.opacity } else { 1.0 };
                    scene.rect(clip, [bg[0], bg[1], bg[2], a]);
                }
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
                let ui = self.ui();
                let isz = self.px(15.0);
                let iy = base - isz + self.px(2.0);
                for icon in [nus_render::text::icons::BACK, nus_render::text::icons::FORWARD, nus_render::text::icons::RELOAD] {
                    self.fonts.draw_icon(scene, icon, isz, x, iy, ink);
                    x += self.px(m::NAV_SLOT);
                }
                x += self.px(4.0);
                // Reader: the book, lit while on. DevTools: the bug, lit while open.
                let dw = isz * 2.0 + self.px(14.0);
                let bug_x = r.right() - self.px(14.0) - isz;
                let book_x = bug_x - self.px(14.0) - isz;
                if p.reader.is_some() {
                    scene.rect(Rect::new(book_x - self.px(6.0), r.y + self.px(6.0), isz + self.px(12.0), self.px(22.0)), ink);
                    self.fonts.draw_icon(scene, nus_render::text::icons::BOOK_TEXT, isz, book_x, iy, t.paper);
                } else if p.reader_req.is_some() {
                    self.fonts.draw_icon(scene, nus_render::text::icons::BOOK, isz, book_x, iy, t.dim);
                } else {
                    self.fonts.draw_icon(scene, nus_render::text::icons::BOOK, isz, book_x, iy, ink);
                }
                if p.devtools.is_some() {
                    scene.rect(Rect::new(bug_x - self.px(6.0), r.y + self.px(6.0), isz + self.px(12.0), self.px(22.0)), ink);
                    self.fonts.draw_icon(scene, nus_render::text::icons::BUG, isz, bug_x, iy, t.paper);
                } else {
                    self.fonts.draw_icon(scene, nus_render::text::icons::BUG, isz, bug_x, iy, ink);
                }
                let field = Rect::new(x, r.y + self.px(6.0), r.right() - self.px(14.0) - dw - self.px(18.0) - x, self.px(22.0));
                let local = is_local(&url);
                if local {
                    scene.push(nus_render::Instance::hazard(field, self.px(2.0), self.surface.signal, ink, self.px(8.0)));
                } else {
                    scene.outline(field, self.px(m::HAIRLINE), ink);
                }
                let shown = self.fit(ui, url.trim_start_matches("https://").trim_start_matches("http://").trim_end_matches('/'), field.w - self.px(16.0));
                let small = Style { px: self.px(12.0), ..ui };
                self.fonts.draw(scene, small, field.x + self.px(8.0), base, &shown);
                scene.hline(r.x, p.page.y - 1.0, r.w, self.px(m::HAIRLINE), ink);
                // Page — or the reader set over it.
                scene.rect(p.page, t.page);
                if let Some(reader) = p.reader.as_mut() {
                    let rf = self.reader_fonts();
                    let paper = self.paper();
                    reader.draw(scene, &mut self.fonts, &rf, p.page, self.scale, ink, t.dim, paper, self.surface.signal);
                } else if let Some(bind) = bind {
                    scene.texture(p.page, bind, Some(p.page));
                    scene.layer(None);
                }
                if local {
                    scene.push(nus_render::Instance::hazard(p.page, self.px(5.0), self.surface.signal, ink, self.px(10.0)));
                }
                let _ = loading;
                self.draw_load_bar(scene, p.page, p, look.signal);
                if let Some(d) = &p.devtools {
                    {
                        let s = d.shared.borrow();
                        if s.paints < 3 || s.paints % 300 == 0 {
                            tracing::info!("devtools view: paints={} bind={} size={:?} url={} title={}", s.paints, s.bind.is_some(), s.size, s.url, s.title);
                        }
                    }
                    scene.hline(r.x, p.page.bottom(), r.w, self.px(m::STRUCTURE), ink);
                    scene.rect(p.dt_rect, t.page);
                    if let Some(b) = d.shared.borrow().bind.clone() {
                        scene.texture(p.dt_rect, b, Some(p.dt_rect));
                        scene.layer(None);
                    }
                }
                // Devtools row.
                let ty = p.dt_rect.bottom().max(p.page.bottom());
                scene.hline(r.x, ty, r.w, self.px(m::HAIRLINE), ink);
                let base = ty + self.px(8.0) + self.px(m::LABEL_PX);
                let mut x = r.x + self.px(14.0);
                let tsz = self.px(15.0);
                for (i, (_, icon)) in DT_PANELS.iter().enumerate() {
                    let on = i == p.dt_panel && p.devtools.is_some();
                    self.fonts.draw_icon(scene, *icon, tsz, x, base - tsz + self.px(3.0), if on { ink } else { t.dim });
                    if on {
                        scene.hline(x, base + self.px(5.0), tsz, self.px(1.5), ink);
                    }
                    x += tsz + self.px(m::NAV_SLOT) - tsz + self.px(2.0);
                }
                let _ = (strong, label);
                let isz = self.px(13.0);
                let mut rx = r.right() - self.px(14.0);
                if focused {
                    rx -= isz;
                    self.fonts.draw_icon(scene, nus_render::text::icons::CURSOR, isz, rx, base - isz + self.px(2.0), ink);
                    rx -= self.px(12.0);
                }
                let words = p.reader.as_ref().map(|r| r.article.words());
                let reading = words.map(|n| format!("{n} WORDS"));
                let (icon, word) = if let Some(rw) = reading.as_deref() {
                    (nus_render::text::icons::BOOK_TEXT, rw)
                } else if local {
                    (nus_render::text::icons::HARD_HAT, "LOCAL")
                } else {
                    (nus_render::text::icons::BROADCAST, "LIVE")
                };
                let lw = self.fonts.measure(label, word);
                rx -= lw;
                self.fonts.draw(scene, label, rx, base, word);
                rx -= isz + self.px(6.0);
                self.fonts.draw_icon(scene, icon, isz, rx, base - isz + self.px(2.0), if local { self.surface.signal } else { ink });
            }
        }
    }

    pub(crate) fn fit(&self, style: Style, text: &str, max_w: f32) -> String {
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

    pub(crate) fn palette_rows(&self, mode: PaletteMode, input: &str) -> Vec<PaletteRow> {
        let q = input.trim().to_lowercase();
        let hit = |s: &str| q.is_empty() || s.to_lowercase().contains(&q);
        let mut rows = Vec::new();
        let row = |num: &str, text: String, action: Action| PaletteRow { num: num.into(), text, action };
        match mode {
            PaletteMode::Go => {
                for (i, t) in self.tabs.iter().enumerate() {
                    if hit(&t.title()) {
                        rows.push(row(&self.tab_label(i), format!("{} · switch to tab", t.title()), Action::SwitchTab(i)));
                    }
                }
                for p in &self.ports {
                    let label = format!("port {} · {}", p.port, if p.process.is_empty() { "?" } else { &p.process });
                    if q.is_empty() || hit(&label) || q == "local" || q == "ports" {
                        rows.push(row("::", format!("{label} → open localhost:{} in the split", p.port), Action::OpenInPane(format!("http://localhost:{}/", p.port))));
                    }
                }
                let actions: [(String, Action); 11] = [
                    (format!("new terminal tab · {}", key("T", true)), Action::NewTerminal(self.behavior.default_profile)),
                    (format!("new browser tab · {} then a URL", key("T", true)), Action::NewBrowser(String::new())),
                    (format!("split with a browser · {}", key("D", true)), Action::ToggleSplit),
                    (format!("close tab · {}", key("W", true)), Action::CloseTab),
                    (format!("sidebar · {}", key("S", true)), Action::ToggleSidebar),
                    (
                        format!("{} this tab", if self.tabs.get(self.active).is_some_and(|t| t.pinned) { "unpin" } else { "pin" }),
                        Action::TogglePin,
                    ),
                    (format!("reopen closed tab · {}", key("Z", true)), Action::Reopen),
                    (format!("carapace · {:?} → next", self.surface.shell).to_lowercase(), Action::ShellStyle),
                    (format!("corner radius {} → +2", self.surface.shell_radius), Action::ShellRadius(2.0)),
                    (format!("corner radius {} → −2", self.surface.shell_radius), Action::ShellRadius(-2.0)),
                    ("picture in picture · this tab's video".into(), Action::Pip),
                ];
                for (label, a) in actions {
                    if hit(&label) {
                        rows.push(row("·", label, a));
                    }
                }
                if !q.is_empty() {
                    self.query_rows(input, &mut rows, false);
                }
            }
            PaletteMode::New => {
                for (i, p) in self.profiles.iter().enumerate() {
                    if hit(&p.name) {
                        rows.push(row(">", format!("terminal · {}", p.name), Action::NewTerminal(i)));
                    }
                }
                for p in &self.ports {
                    let label = format!("port {} · {}", p.port, if p.process.is_empty() { "?" } else { &p.process });
                    if q.is_empty() || hit(&label) {
                        rows.push(row("::", format!("{label} → localhost:{}", p.port), Action::NewBrowser(format!("http://localhost:{}/", p.port))));
                    }
                }
                if q.is_empty() {
                    rows.push(row("→", "browser · type a URL or search terms".into(), Action::NewBrowser(String::new())));
                } else {
                    self.query_rows(input, &mut rows, true);
                }
            }
            PaletteMode::Url => {
                if !q.is_empty() {
                    self.query_rows(input, &mut rows, false);
                }
            }
        }
        rows
    }

    /// What typed text can become: a page, a search, a question to a web
    /// assistant, or a question to a local CLI run in the shell.
    fn query_rows(&self, input: &str, rows: &mut Vec<PaletteRow>, new_tab: bool) {
        let q = input.trim();
        let open = |url: String| if new_tab { Action::NewBrowser(url) } else { Action::OpenInPane(url) };
        let row = |num: &str, text: String, action: Action| PaletteRow { num: num.into(), text, action };
        let enc = |s: &str| s.replace(' ', "+").replace('&', "%26").replace('#', "%23");
        let is_url = strict_url(q).is_some() || q.contains("://");
        let (url, text) = Self::url_or_search(q);
        if is_url {
            rows.push(row("→", text, open(url)));
            rows.push(row("?", format!("search “{q}”"), open(format!("https://www.google.com/search?q={}", enc(q)))));
        } else {
            rows.push(row("?", text, open(url)));
        }
        rows.push(row("*", format!("ask chatgpt “{q}”"), open(format!("https://chatgpt.com/?q={}", enc(q)))));
        rows.push(row("*", format!("ask claude “{q}”"), open(format!("https://claude.ai/new?q={}", enc(q)))));
        for (name, template) in &self.llm_tools {
            let cmd = template.replace("{q}", &q.replace('"', "\\\""));
            rows.push(row("*", format!("ask {name} in this shell · {cmd}"), Action::RunInShell(cmd)));
        }
    }

    /// Type `cmd` + Enter into the focused terminal, or a fresh one.
    pub(crate) fn run_in_shell(&mut self, cmd: &str) {
        let has_term = self.tabs.get_mut(self.active).is_some_and(|t| matches!(t.focused(), Pane::Term(_)) || matches!(t.left, Pane::Term(_)));
        if !has_term {
            let p = self.behavior.default_profile;
            self.new_tab(p);
        }
        let tab = &mut self.tabs[self.active];
        let pane = if matches!(tab.focused(), Pane::Term(_)) { tab.focused() } else { &mut tab.left };
        if let Pane::Term(t) = pane {
            let _ = t.pty.write(format!("{cmd}\r").as_bytes());
            t.line.clear();
            t.line_ok = true;
        }
        tab.focus_right = false;
    }

    fn open_palette(&mut self, mode: PaletteMode) {
        if mode == PaletteMode::Go {
            self.tick_hint(0);
        }
        if matches!(mode, PaletteMode::Go | PaletteMode::New) {
            self.ports = nus_pty::listening_ports()
                .into_iter()
                .filter(|p| p.port >= 1024 && !SYSTEM_PROCS.contains(&p.process.to_lowercase().as_str()))
                .collect();
        }
        self.palette = Some((mode, String::new()));
        self.palette_sel = 0;
        self.palette_anim.replay(0.0, 1.0, self.motion.dur(base::PALETTE));
        self.dirty = true;
    }

    fn run(&mut self, action: Action) {
        match action {
            Action::SwitchTab(i) => self.activate(i),
            Action::NewTerminal(p) => self.new_tab(p),
            Action::NewBrowser(url) if url.is_empty() => self.open_palette(PaletteMode::New),
            Action::NewBrowser(url) => {
                self.tick_hint(1);
                self.open_url(&url, true)
            }
            Action::OpenInPane(url) => self.open_url(&url, false),
            Action::RunInShell(cmd) => self.run_in_shell(&cmd),
            Action::ToggleSplit => self.toggle_split(),
            Action::CloseTab => self.close_tabs(false),
            Action::TogglePin => {
                let t = &mut self.tabs[self.active];
                t.pinned = !t.pinned;
            }
            Action::Reopen => self.reopen_closed(),
            Action::ShellStyle => {
                self.surface.shell = self.surface.shell.next();
                self.layout();
            }
            Action::Pip => {
                let tab = self.active;
                let right = match (&self.tabs[tab].left, &self.tabs[tab].right) {
                    (Pane::Web(_), _) => Some(false),
                    (_, Some(Pane::Web(_))) => Some(true),
                    _ => None,
                };
                if let Some(right) = right {
                    self.request_pip(tab, right);
                }
            }
            Action::ShellRadius(d) => {
                self.surface.shell_radius = (self.surface.shell_radius + d).clamp(0.0, 24.0);
                self.layout();
            }
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
        if pressed && self.confirm_stack.is_some() {
            match ev.logical_key {
                WKey::Named(NamedKey::Enter) => {
                    self.confirm_stack = None;
                    self.close_tabs(true);
                }
                WKey::Named(NamedKey::Escape) => self.confirm_stack = None,
                _ => {}
            }
            self.dirty = true;
            return;
        }
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

        // Chords match the physical key: with Ctrl held, Windows reports no
        // character for many keys, so the logical key is unreliable here.
        let code = match ev.physical_key {
            PhysicalKey::Code(c) => Some(c),
            _ => None,
        };
        if pressed && code == Some(KeyCode::F11) && !ctrl && !shift {
            return self.toggle_fullscreen();
        }
        if pressed && app {
            match code {
                Some(KeyCode::KeyT) => return self.open_palette(PaletteMode::New),
                Some(KeyCode::KeyK) => return self.open_palette(PaletteMode::Go),
                Some(KeyCode::KeyL) => return self.open_palette(PaletteMode::Url),
                Some(KeyCode::KeyW) => return self.close_tabs(false),
                Some(KeyCode::KeyZ) => return self.reopen_closed(),
                Some(KeyCode::KeyD) => return self.toggle_split(),
                Some(KeyCode::KeyR) => return self.toggle_reader(),
                Some(KeyCode::KeyS) => {
                    self.sidebar = !self.sidebar;
                    return self.layout();
                }
                _ => {}
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
            let digit = match code {
                Some(KeyCode::Digit1) => Some(0),
                Some(KeyCode::Digit2) => Some(1),
                Some(KeyCode::Digit3) => Some(2),
                Some(KeyCode::Digit4) => Some(3),
                Some(KeyCode::Digit5) => Some(4),
                Some(KeyCode::Digit6) => Some(5),
                Some(KeyCode::Digit7) => Some(6),
                Some(KeyCode::Digit8) => Some(7),
                Some(KeyCode::Digit9) => Some(8),
                _ => None,
            };
            if let Some(n) = digit {
                self.activate_stack(n);
                return;
            }
            match code {
                Some(KeyCode::Comma) => return self.open_settings(),
                Some(KeyCode::Backquote) => {
                    self.tick_hint(4);
                    if let Some(&prev) = self.mru.get(1) {
                        self.activate(prev);
                    }
                    return;
                }
                _ => {}
            }
            match &ev.logical_key {
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
                        self.tick_hint(2);
                        let new_tab = self.behavior.prompt_url == crate::settings::PromptUrl::NewTab;
                        self.open_url(&url, new_tab);
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
            Pane::Settings(_) | Pane::Hints(_) => {}
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
                                    let new_tab = self.behavior.prompt_url == crate::settings::PromptUrl::NewTab;
                                    self.open_url(&url, new_tab);
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
            Pane::Web(w) if w.focus_devtools && w.devtools.is_some() => {
                if pressed && matches!(ev.logical_key, WKey::Named(NamedKey::F12)) {
                    return self.toggle_devtools();
                }
                let flags = cef_mods(self.mods);
                let vk = vk_code(&ev.physical_key, &ev.logical_key);
                let d = w.devtools.as_ref().unwrap();
                let mut e = cef::KeyEvent { windows_key_code: vk, native_key_code: vk, modifiers: flags, ..Default::default() };
                if pressed {
                    e.type_ = cef::KeyEventType::RAWKEYDOWN;
                    d.key(&e);
                    if let Some(text) = &ev.text {
                        if !ctrl || alt {
                            for ch in text.encode_utf16() {
                                let mut c = cef::KeyEvent { ..e };
                                c.type_ = cef::KeyEventType::CHAR;
                                c.character = ch;
                                c.unmodified_character = ch;
                                c.windows_key_code = ch as i32;
                                d.key(&c);
                            }
                        }
                    }
                } else {
                    e.type_ = cef::KeyEventType::KEYUP;
                    d.key(&e);
                }
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
                            self.toggle_devtools();
                            return;
                        }
                        _ => {}
                    }
                }
                forward_key(&w.tab, ev, self.mods);
            }
        }
    }

    fn make_tab(&mut self, left: Pane, right: Option<Pane>) -> Tab {
        let id = self.next_id;
        self.next_id += 1;
        let look = self.look_for(&left, None);
        Tab { id, parent: None, left, right, focus_right: false, pinned: false, look }
    }

    /// Ask the rules what a new tab looks like.
    fn look_for(&self, left: &Pane, parent: Option<&Overrides>) -> Overrides {
        let (kind, profile) = match left {
            Pane::Term(t) => ("terminal", self.profiles.get(t.profile).map(|p| p.name.as_str()).unwrap_or("")),
            Pane::Web(_) => ("page", ""),
            Pane::Settings(_) => ("settings", ""),
            Pane::Hints(_) => ("welcome", ""),
        };
        let index = self.top_level().len();
        self.rules.new_tab(&TabCtx {
            kind,
            index,
            profile,
            space: &self.space_name,
            space_signal: self.surface.signal,
            theme: if self.theme.mode == nus_render::Mode::Ink { "ink" } else { "paper" },
            parent,
        })
    }

    // ── Stacks ─────────────────────────────────────────────────────────
    // A stack is a top-level tab plus the tabs pages opened from it (popups,
    // target=_blank). Children sit right after their parent in `tabs`, show
    // in the sidebar only while the stack is active, and share the parent's
    // number: Ctrl+N goes to whichever member was used last.

    /// Index of the stack's top-level tab.
    fn stack_root(&self, i: usize) -> usize {
        match self.tabs[i].parent {
            Some(pid) => self.tabs.iter().position(|t| t.id == pid).unwrap_or(i),
            None => i,
        }
    }

    fn children(&self, root: usize) -> Vec<usize> {
        let id = self.tabs[root].id;
        (0..self.tabs.len()).filter(|&j| self.tabs[j].parent == Some(id)).collect()
    }

    /// Stacks unfold only while one of their members is active.
    fn stack_open(&self, root: usize) -> bool {
        self.stack_root(self.active) == root
    }

    fn top_level(&self) -> Vec<usize> {
        (0..self.tabs.len()).filter(|&j| self.tabs[j].parent.is_none()).collect()
    }

    /// Sidebar / crumb label: top-level tabs count 01, 02, …; a child carries
    /// its parent's number and a letter (03·b).
    pub(crate) fn tab_label(&self, i: usize) -> String {
        self.tab_label_of(&self.tabs, i)
    }

    fn tab_label_of(&self, tabs: &[Tab], i: usize) -> String {
        let root = match tabs[i].parent {
            Some(pid) => tabs.iter().position(|t| t.id == pid).unwrap_or(i),
            None => i,
        };
        let n = (0..tabs.len()).filter(|&j| tabs[j].parent.is_none()).position(|j| j == root).map(|p| p + 1).unwrap_or(0);
        if root == i {
            format!("{n:02}")
        } else {
            let id = tabs[root].id;
            let k = (0..tabs.len()).filter(|&j| tabs[j].parent == Some(id)).position(|j| j == i).unwrap_or(0);
            format!("{n:02}·{}", (b'a' + (k % 26) as u8) as char)
        }
    }

    /// Ctrl+N: the n-th stack, at its most recently used member.
    fn activate_stack(&mut self, n: usize) {
        let Some(&root) = self.top_level().get(n) else { return };
        let members: Vec<usize> = std::iter::once(root).chain(self.children(root)).collect();
        let target = self.mru.iter().copied().find(|t| members.contains(t)).unwrap_or(root);
        self.activate(target);
    }

    /// Open `url` as a page in the stack of tab `source`.
    pub(crate) fn open_in_stack(&mut self, source: usize, url: &str) {
        let root = self.stack_root(source);
        let Some(w) = self.new_web_pane(url) else { return };
        let mut tab = self.make_tab(Pane::Web(w), None);
        tab.parent = Some(self.tabs[root].id);
        let parent_look = self.tabs[root].look.clone();
        tab.look = self.look_for(&tab.left, Some(&parent_look));
        let at = self.children(root).last().copied().unwrap_or(root) + 1;
        self.tabs.insert(at, tab);
        // Indices after `at` shifted by one.
        for t in self.mru.iter_mut() {
            if *t >= at {
                *t += 1;
            }
        }
        self.selected = self.selected.iter().map(|&t| if t >= at { t + 1 } else { t }).collect();
        if let Some(p) = self.pip.as_mut() {
            if p.tab >= at {
                p.tab += 1;
            }
        }
        if self.active >= at {
            self.active += 1;
        }
        self.activate(at);
    }

    /// Pages that asked for a new window since the last frame.
    pub(crate) fn drain_popups(&mut self) {
        let mut opens = Vec::new();
        for (i, tab) in self.tabs.iter().enumerate() {
            for p in std::iter::once(&tab.left).chain(tab.right.as_ref()) {
                if let Pane::Web(w) = p {
                    if let Some(url) = w.tab.shared.borrow_mut().popup.take() {
                        opens.push((i, url));
                    }
                }
            }
        }
        for (i, url) in opens {
            match self.behavior.links {
                crate::settings::Links::Stack => self.open_in_stack(i, &url),
                crate::settings::Links::Split => {
                    self.activate(i);
                    self.open_url(&url, false);
                }
                crate::settings::Links::NewTab => self.open_url(&url, true),
            }
            self.dirty = true;
        }
    }

    /// Make tab `i` active and record it as most recently used.
    pub fn activate(&mut self, i: usize) {
        if i >= self.tabs.len() {
            return;
        }
        let prev = self.active;
        if let Some(p) = &self.pip {
            if p.tab == i {
                self.pip = None;
            }
        }
        if prev != i && self.pip.is_none() {
            if let Some(right) = self.playing_video(prev) {
                self.request_pip(prev, right);
            }
        }
        if prev != i {
            self.crumb_anim.replay(0.0, 1.0, self.motion.dur(base::CRUMB));
        }
        self.active = i;
        let tab = &mut self.tabs[i];
        for p in std::iter::once(&mut tab.left).chain(tab.right.as_mut()) {
            if let Pane::Term(t) = p {
                t.waiting = false;
            }
        }
        self.mru.retain(|&t| t != i);
        self.mru.insert(0, i);
        self.layout();
    }

    /// Keep `mru`/`selected` valid after `tabs[i]` was removed.
    fn tab_removed(&mut self, i: usize) {
        if let Some(p) = self.pip.as_mut() {
            if p.tab == i {
                self.pip = None;
            } else if p.tab > i {
                p.tab -= 1;
            }
        }
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

    /// Open or close DevTools for the focused browser pane.
    fn toggle_devtools(&mut self) {
        let device = self.device.clone();
        let binder = self.bind_texture.clone();
        let scale = self.scale;
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        let pane = match (&tab.left, tab.focus_right) {
            (_, true) if tab.right.is_some() => tab.right.as_mut().unwrap(),
            (Pane::Web(_), _) => &mut tab.left,
            _ => match tab.right.as_mut() {
                Some(r) => r,
                None => return,
            },
        };
        if let Pane::Web(w) = pane {
            if w.devtools.is_some() {
                w.tab.close_devtools();
                w.devtools = None;
                w.focus_devtools = false;
            } else {
                let right = tab.focus_right && tab.right.is_some();
                let _ = (device, binder, scale);
                self.devtools_request = Some((self.active, right));
                return;
            }
        }
        self.layout();
    }

    /// Pick a DevTools panel; reopens the frontend on that panel.
    fn switch_devtools_panel(&mut self, right: bool, k: usize) {
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        let pane = if right { tab.right.as_mut() } else { Some(&mut tab.left) };
        if let Some(Pane::Web(w)) = pane {
            w.dt_panel = k.min(DT_PANELS.len() - 1);
            if w.devtools.is_some() {
                w.tab.close_devtools();
                w.devtools = None;
            }
            self.devtools_request = Some((self.active, right));
        }
    }

    /// Create a requested DevTools browser outside of event handling.
    pub fn process_requests(&mut self) {
        let Some((tab, right)) = self.devtools_request.take() else { return };
        let device = self.device.clone();
        let binder = self.bind_texture.clone();
        let scale = self.scale;
        let Some(t) = self.tabs.get_mut(tab) else { return };
        let pane = if right { t.right.as_mut() } else { Some(&mut t.left) };
        if let Some(Pane::Web(w)) = pane {
            let panel = DT_PANELS[w.dt_panel.min(DT_PANELS.len() - 1)].0;
            w.devtools = w.tab.open_devtools(device, binder, scale, panel);
            w.focus_devtools = w.devtools.is_some();
            tracing::info!("devtools opened: {}", w.devtools.is_some());
        }
        self.layout();
    }

    pub(crate) fn palette_commit(&mut self) {
        let Some((mode, input)) = self.palette.take() else { return };
        let rows = self.palette_rows(mode, &input);
        let action = match rows.get(self.palette_sel) {
            Some(r) => r.action.clone(),
            None if mode == PaletteMode::New => Action::NewTerminal(self.behavior.default_profile),
            None => return,
        };
        self.run(action);
    }

    pub(crate) fn open_url(&mut self, url: &str, new_tab: bool) {
        if new_tab {
            if let Some(w) = self.new_web_pane(url) {
                let tab = self.make_tab(Pane::Web(w), None);
                self.tabs.push(tab);
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

    pub(crate) fn new_tab(&mut self, profile: usize) {
        if let Ok(t) = self.new_term_pane(false, profile) {
            let tab = self.make_tab(Pane::Term(t), None);
            self.tabs.push(tab);
            self.activate(self.tabs.len() - 1);
        }
    }

    /// Close the selection (or the active tab). Unless `force`, a terminal
    /// with a foreground process asks first.
    pub(crate) fn close_tabs(&mut self, force: bool) {
        let force = force || !self.behavior.close_asks;
        let mut targets: Vec<usize> = if self.selected.is_empty() {
            vec![self.active]
        } else {
            let mut v: Vec<usize> = self.selected.iter().copied().collect();
            if !v.contains(&self.active) {
                v.push(self.active);
            }
            v
        };
        // A parent takes its children along.
        for i in targets.clone() {
            if self.tabs[i].parent.is_none() {
                targets.extend(self.children(i));
            }
        }
        targets.sort_unstable();
        targets.dedup();
        if !force {
            if let Some(&root) = targets.iter().find(|&&i| self.tabs[i].parent.is_none() && !self.children(i).is_empty()) {
                self.confirm_stack = Some(root);
                self.active = root;
                self.band_anim.replay(0.0, 1.0, self.motion.dur(base::BAND));
                self.dirty = true;
                return;
            }
        }
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
                        self.band_anim.replay(0.0, 1.0, self.motion.dur(base::BAND));
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
                Pane::Settings(_) | Pane::Hints(_) => {
                    self.tab_removed(i);
                    continue;
                }
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

    pub fn focus_changed(&mut self, focused: bool) {
        self.window_focused = focused;
        if !focused {
            if self.pip.is_none() {
                if let Some(right) = self.playing_video(self.active) {
                    self.request_pip(self.active, right);
                }
            }
        } else if self.pip.as_ref().is_some_and(|p| p.tab == self.active) {
            self.pip = None;
        }
        self.dirty = true;
    }

    pub fn modifiers(&mut self, m: ModifiersState) {
        self.mods = m;
    }

    pub fn mouse_moved(&mut self, x: f32, y: f32) {
        self.mouse = (x, y);
        if !self.sidebar_pinned() && self.sidebar_hoverable() {
            let c = self.content_rect();
            let sb = self.sidebar_rect();
            let inside = sb.contains(x, y);
            let edge = self.px(6.0);
            let at_edge = if self.sidebar_right() { x > self.target.size.0 as f32 - edge } else { x < edge };
            // "Inside window" means the pointer must have travelled from inside
            // to the edge; arriving straight from the desktop doesn't count.
            let allowed = match self.sidebar_rules.hover_from {
                HoverFrom::ScreenEdge => true,
                HoverFrom::InsideWindow => self.pointer_inside,
            };
            if !self.sidebar_hover && at_edge && y >= c.y && allowed {
                self.sidebar_hover = true;
                self.tick_hint(3);
                self.sidebar_leave = None;
                self.dirty = true;
            } else if self.sidebar_hover {
                if inside {
                    self.sidebar_leave = None;
                } else if self.sidebar_leave.is_none() {
                    self.sidebar_leave = Some(Instant::now() + std::time::Duration::from_millis(self.sidebar_rules.grace_ms));
                }
            }
            if !at_edge {
                self.pointer_inside = true;
            }
        }
        if self.sidebar_visible() {
            let g = self.sidebar_geometry();
            let row = g.rows.iter().find(|&&(_, ry, rh)| self.sidebar_rect().contains(x, y) && y >= ry && y < ry + rh).map(|&(i, _, _)| i);
            if row != self.hover_row {
                self.hover_row = row;
                self.dirty = true;
            }
        } else if self.hover_row.is_some() {
            self.hover_row = None;
        }
        let flags = cef_mods(self.mods) | if self.mouse_down_in_web { 16 } else { 0 };
        if let Some(tab) = self.tabs.get(self.active) {
            for p in std::iter::once(&tab.left).chain(tab.right.as_ref()) {
                if let Pane::Web(w) = p {
                    if w.page.contains(x, y) || self.mouse_down_in_web {
                        let (lx, ly) = ((x - w.page.x) / self.scale, (y - w.page.y) / self.scale);
                        w.tab.mouse_move(lx as i32, ly as i32, flags, false);
                    }
                    if let Some(d) = &w.devtools {
                        if w.dt_rect.contains(x, y) {
                            let (lx, ly) = ((x - w.dt_rect.x) / self.scale, (y - w.dt_rect.y) / self.scale);
                            d.mouse_move(lx as i32, ly as i32, flags, false);
                        }
                    }
                }
            }
        }
    }

    /// What a header icon does; shared by the mouse and AccessKit.
    pub(crate) fn crumb_action(&mut self, hit: CrumbHit) {
        match hit {
            CrumbHit::Close => std::process::exit(0),
            CrumbHit::Maximize => self.window.set_maximized(!self.window.is_maximized()),
            CrumbHit::Minimize => self.window.set_minimized(true),
            CrumbHit::Space | CrumbHit::Tab | CrumbHit::Search => self.open_palette(PaletteMode::Go),
            CrumbHit::Url => self.open_palette(PaletteMode::Url),
            CrumbHit::Sidebar => {
                self.sidebar = !self.sidebar;
                self.layout();
            }
            CrumbHit::Ports => {
                self.open_palette(PaletteMode::Go);
                if let Some((_, input)) = self.palette.as_mut() {
                    input.push_str("port");
                }
            }
            CrumbHit::Assistant => {
                self.open_palette(PaletteMode::Go);
                if let Some((_, input)) = self.palette.as_mut() {
                    input.push_str("ask ");
                }
            }
            CrumbHit::Pip => self.return_from_pip(),
            CrumbHit::Waiting => {
                if let Some(i) = self.tabs.iter().position(|t| t.waiting()) {
                    self.activate(i);
                }
            }
        }
    }

    pub fn mouse_button(&mut self, button: MouseButton, state: ElementState) {
        let (x, y) = self.mouse;
        let pressed = state == ElementState::Pressed;
        let strip = self.strip_rect();

        if pressed && button == MouseButton::Left && self.palette.is_some() {
            if let Some(i) = self.palette_hits.iter().position(|r| r.contains(x, y)) {
                self.palette_sel = i;
                self.palette_commit();
            } else {
                self.palette = None;
            }
            self.dirty = true;
            return;
        }

        // Top strip: window controls, else drag.
        if pressed && button == MouseButton::Left && strip.contains(x, y) {
            let hit = self.crumb_hits.iter().find(|(r, _)| r.contains(x, y)).map(|(_, h)| *h);
            match hit {
                Some(h) => self.crumb_action(h),
                None => {
                    let _ = self.window.drag_window();
                }
            }
            return;
        }

        // Sidebar: pinned cells, tab rows, footer. Ctrl-click selects, Shift-click ranges.
        if pressed && button == MouseButton::Left && self.sidebar_visible() && self.sidebar_rect().contains(x, y) {
            let sb = self.sidebar_rect();
            let g = self.sidebar_geometry();
            let foot_row = self.px(8.0) * 2.0 + self.px(m::LABEL_PX) + self.px(m::HAIRLINE);
            if y > sb.bottom() - foot_row {
                if x > sb.x + sb.w / 2.0 {
                    self.open_settings();
                } else {
                    self.open_palette(PaletteMode::New);
                }
                return;
            }
            if y > g.foot_y {
                return;
            }
            let space_row = self.px(9.0) * 2.0 + self.px(m::LABEL_PX) + self.px(m::STRUCTURE);
            let pinned_y = sb.y + space_row;
            let hit = if !g.pinned.is_empty() && y >= pinned_y && y < pinned_y + g.pinned_h {
                let k = ((x - sb.x) / (sb.w / g.pinned.len() as f32).floor()) as usize;
                g.pinned.get(k).copied()
            } else {
                g.rows.iter().find(|&&(_, ry, rh)| y >= ry && y < ry + rh).map(|&(i, _, _)| i)
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

        // Onboarding foot: skip / close.
        if pressed && button == MouseButton::Left && self.hints_open() {
            if let Some(&(_, k)) = self.hint_hits.iter().find(|(r, _)| r.contains(x, y)) {
                if k == usize::MAX {
                    return self.dismiss_hints();
                }
            }
        }

        // Settings: chips, sliders, swatches, buttons.
        if pressed && button == MouseButton::Left && self.settings_click(x, y) {
            return;
        }

        // Panes: focus, and forward to the browser.
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        let mut hit_right = None;
        let in_left = match &tab.left {
            Pane::Term(t) => t.rect.contains(x, y),
            Pane::Web(w) => w.rect.contains(x, y),
            Pane::Settings(s) => s.rect.contains(x, y),
            Pane::Hints(s) => s.rect.contains(x, y),
        };
        if let Some(r) = &tab.right {
            let hit = match r {
                Pane::Term(t) => t.rect.contains(x, y),
                Pane::Web(w) => w.rect.contains(x, y),
                Pane::Settings(s) => s.rect.contains(x, y),
            Pane::Hints(s) => s.rect.contains(x, y),
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
        let mods = cef_mods(self.mods);
        let mut down_in_web = self.mouse_down_in_web;
        let mut open_url_palette = false;
        let mut toggle_devtools = false;
        let mut toggle_reader = false;
        let mut switch_panel: Option<(bool, usize)> = None;
        let mut focus_dt: Option<(bool, bool)> = None;
        for (is_right, p) in std::iter::once((false, &tab.left)).chain(tab.right.as_ref().map(|r| (true, r))) {
            match p {
                Pane::Web(w) => {
                    let url_row = Rect::new(w.rect.x, w.rect.y, w.rect.w, w.page.y - w.rect.y);
                    if pressed && button == MouseButton::Left && url_row.contains(x, y) {
                        let slot = m::NAV_SLOT * scale;
                        let nav_x = x - (w.rect.x + 14.0 * scale);
                        if nav_x < 3.0 * slot {
                            match (nav_x / slot) as usize {
                                0 => w.tab.back(),
                                1 => w.tab.forward(),
                                _ => w.tab.reload(),
                            }
                        } else if x > w.rect.right() - 44.0 * scale {
                            toggle_devtools = true;
                        } else if x > w.rect.right() - 74.0 * scale {
                            toggle_reader = true;
                        } else {
                            open_url_palette = true;
                        }
                        continue;
                    }
                    let tools_row = Rect::new(w.rect.x, w.dt_rect.bottom().max(w.page.bottom()), w.rect.w, w.rect.bottom() - w.dt_rect.bottom().max(w.page.bottom()));
                    if pressed && button == MouseButton::Left && tools_row.contains(x, y) {
                        let nav_x = x - (w.rect.x + 14.0 * scale);
                        let slot = (m::NAV_SLOT + 2.0) * scale;
                        if nav_x >= 0.0 && nav_x < 3.0 * slot {
                            let k = (nav_x / slot) as usize;
                            // Same panel with DevTools open closes it; another switches.
                            if w.devtools.is_some() && w.dt_panel == k {
                                toggle_devtools = true;
                            } else {
                                switch_panel = Some((is_right, k));
                            }
                            continue;
                        }
                        if x > w.rect.right() - 160.0 * scale && w.devtools.is_some() {
                            toggle_devtools = true;
                            continue;
                        }
                    }
                    if let Some(d) = &w.devtools {
                        if w.dt_rect.contains(x, y) {
                            let (lx, ly) = ((x - w.dt_rect.x) / scale, (y - w.dt_rect.y) / scale);
                            let b = match button {
                                MouseButton::Left => cef::MouseButtonType::LEFT,
                                MouseButton::Right => cef::MouseButtonType::RIGHT,
                                MouseButton::Middle => cef::MouseButtonType::MIDDLE,
                                _ => continue,
                            };
                            d.mouse_click(lx as i32, ly as i32, mods, b, !pressed, 1);
                            d.focus(true);
                            w.tab.focus(false);
                            focus_dt = Some((is_right, true));
                            continue;
                        }
                    }
                    let inside = w.page.contains(x, y) && w.reader.is_none();
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
                Pane::Term(_) | Pane::Settings(_) | Pane::Hints(_) => {}
            }
        }
        self.mouse_down_in_web = down_in_web;
        if let Some((is_right, on)) = focus_dt {
            let tab = &mut self.tabs[self.active];
            let pane = if is_right { tab.right.as_mut() } else { Some(&mut tab.left) };
            if let Some(Pane::Web(w)) = pane {
                w.focus_devtools = on;
            }
        } else if pressed && button == MouseButton::Left {
            let tab = &mut self.tabs[self.active];
            for p in std::iter::once(&mut tab.left).chain(tab.right.as_mut()) {
                if let Pane::Web(w) = p {
                    if w.page.contains(x, y) {
                        w.focus_devtools = false;
                    }
                }
            }
        }
        if toggle_devtools {
            self.toggle_devtools();
        }
        if let Some((right, k)) = switch_panel {
            self.switch_devtools_panel(right, k);
        }
        if toggle_reader {
            self.toggle_reader();
        }
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
                Pane::Web(w) if w.devtools.is_some() && w.dt_rect.contains(x, y) => {
                    let (dx, dy) = match delta {
                        MouseScrollDelta::LineDelta(x, y) => ((x * 40.0) as i32, (y * 40.0) as i32),
                        MouseScrollDelta::PixelDelta(p) => (p.x as i32, p.y as i32),
                    };
                    let (lx, ly) = ((x - w.dt_rect.x) / self.scale, (y - w.dt_rect.y) / self.scale);
                    w.devtools.as_ref().unwrap().wheel(lx as i32, ly as i32, cef_mods(self.mods), dx, dy);
                }
                Pane::Web(w) if w.page.contains(x, y) && w.reader.is_some() => {
                    let dy = match delta {
                        MouseScrollDelta::LineDelta(_, y) => y * 60.0 * self.scale,
                        MouseScrollDelta::PixelDelta(p) => p.y as f32,
                    };
                    if let Some(rd) = w.reader.as_mut() {
                        rd.scroll -= dy;
                    }
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

fn on_path(exe: &str) -> bool {
    let names: Vec<String> = if cfg!(windows) {
        vec![format!("{exe}.exe"), format!("{exe}.cmd"), format!("{exe}.bat")]
    } else {
        vec![exe.to_string()]
    };
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| names.iter().any(|n| d.join(n).is_file())))
        .unwrap_or(false)
}

/// Local CLIs that take a prompt as an argument. The router is config later.
fn discover_llm_tools() -> Vec<(String, String)> {
    let mut v = Vec::new();
    if on_path("claude") {
        v.push(("claude".to_string(), "claude \"{q}\"".to_string()));
    }
    if on_path("codex") {
        v.push(("codex".to_string(), "codex \"{q}\"".to_string()));
    }
    if on_path("ollama") {
        v.push(("ollama".to_string(), "ollama run llama3.2 \"{q}\"".to_string()));
    }
    v
}

const SYSTEM_PROCS: &[&str] = &["system", "svchost", "lsass", "wininit", "services", "spoolsv", "dns", "rpcbind", "systemd", "cupsd", "launchd", "rapportd", "controlce", "sharingd"];

/// Local/private destinations get the safety tape.
fn is_local(url: &str) -> bool {
    let host = url.split("//").nth(1).unwrap_or(url).split('/').next().unwrap_or("");
    let host = host.trim_start_matches('[').split([']', ':']).next().unwrap_or("");
    if host == "localhost" || host == "127.0.0.1" || host == "::1" || host.ends_with(".local") || host.ends_with(".localhost") {
        return true;
    }
    let oct: Vec<u8> = host.split('.').filter_map(|o| o.parse().ok()).collect();
    oct.len() == 4 && (oct[0] == 10 || (oct[0] == 192 && oct[1] == 168) || (oct[0] == 172 && (16..=31).contains(&oct[1])))
}

fn short_title(t: &str) -> String {
    let t = t.rsplit(['\\', '/']).next().unwrap_or(t);
    t.trim_end_matches(".exe").to_string()
}

pub(crate) fn last_lines(term: &Term, n: usize) -> Vec<String> {
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
pub(crate) fn strict_url(line: &str) -> Option<String> {
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

pub(crate) fn cef_mods(m: ModifiersState) -> u32 {
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
/// Forward a winit key event to a browser: raw down, chars, or up.
pub(crate) fn forward_key(tab: &BrowserTab, ev: &WKeyEvent, mods: ModifiersState) {
    let pressed = ev.state == ElementState::Pressed;
    let (ctrl, alt) = (mods.control_key(), mods.alt_key());
    let flags = cef_mods(mods);
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
        tab.key(&e);
        if let Some(text) = &ev.text {
            if !ctrl || alt {
                for ch in text.encode_utf16() {
                    let mut c = cef::KeyEvent { ..e };
                    c.type_ = cef::KeyEventType::CHAR;
                    c.character = ch;
                    c.unmodified_character = ch;
                    c.windows_key_code = ch as i32;
                    tab.key(&c);
                }
            }
        }
    } else {
        e.type_ = cef::KeyEventType::KEYUP;
        tab.key(&e);
    }
}

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
