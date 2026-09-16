//! The composite window: Broadsheet chrome around terminal and browser panes.

use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use nus_render::theme::metric as m;
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
    /// F2: name this window.
    Rename,
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
    Start,
    Pip,
    RenameWindow(String),
    NewWindow,
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
    Start,
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
    /// The pane strip (profile · size) shows only when the tab is split;
    /// alone, the header crumb already says it.
    pub show_header: bool,
    /// Where the drawn cursor is, in cells, easing toward the real one.
    pub cur_x: Anim,
    pub cur_y: Anim,
    /// Recent cursor positions (cells) for the comet, newest last.
    pub trail: Vec<(f32, f32, Instant)>,
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
    /// The URL last written to the recent list.
    pub remembered: String,
    /// When the current load began (for the ready cue).
    pub load_since: Option<Instant>,
}

pub const DT_PANELS: [(&str, (&str, &str)); 3] = [("console", nus_render::text::icons::CONSOLE), ("network", nus_render::text::icons::NETWORK), ("elements", nus_render::text::icons::CODE)];

pub struct SettingsPane {
    pub rect: Rect,
    pub section: usize,
    /// Content scroll, physical px.
    pub scroll: f32,
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

/// Sidebar click targets besides tab rows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SideHit {
    Close(usize),
    Profile,
    NewTab,
    /// The header's primary button: a shell in the default profile.
    NewShell,
    /// The window cell: opens the list of windows.
    Window,
    /// The split caret: the kinds of tab.
    Kinds,
    /// A kind in the fan-out: a shell profile, or a page.
    Kind(usize),
    KindPage,
    /// A window in the list (index into the registry list).
    WinFront(usize),
    Rename,
    NewWindow,
    /// A square on the rail.
    Rail(usize),
    /// The look chip in the footer.
    Look,
    RailNew,
    Closed,
    Downloads,
    Settings,
}

/// What an icon does when the pointer arrives.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum IconMotion {
    /// Just the hover overlay.
    Still,
    /// Turns by this many degrees while hovered, back when left.
    Spin(f32),
    /// Rises and settles: a small pop in scale.
    Pop,
    /// Dips and returns, like a download landing.
    Bob,
    /// A bell's swing, decaying.
    Swing,
}

/// One icon button's hover state.
pub struct Hover {
    pub alpha: Anim,
    pub pulse: Anim,
    pub hot: bool,
}

pub fn hover_key(name: &str, n: usize) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    name.hash(&mut h);
    n.hash(&mut h);
    h.finish()
}

pub struct SidebarGeom {
    pub pinned: Vec<usize>,
    pub pinned_h: f32,
    /// Where the row after the last tab would start.
    pub next_y: f32,
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
    pub login_note: String,
    pub theme_edit: crate::theme_edit::ThemeEdit,
    pub ansi_sel: usize,
    pub cursor: crate::settings::CursorPrefs,
    pub header: crate::settings::HeaderPrefs,
    /// The user's name for this window (None = auto).
    pub window_named: Option<String>,
    pub instance_port: u16,
    /// Other nus windows, refreshed when the list opens.
    pub windows: Vec<crate::windows::Entry>,
    pub win_menu: bool,
    pub win_anim: Anim,
    pub kinds_menu: bool,
    pub kinds_anim: Anim,
    /// NEW TAB pressed and not yet released: (when, hit) — a hold fans out.
    pub press: Option<(Instant, SideHit)>,
    pub flash_anim: Anim,
    pub rail_anim: Anim,
    pub next_row_hot: bool,
    pub registered_tabs: usize,
    pub hovers: std::collections::HashMap<u64, Hover>,
    /// Last keystroke into a shell, for blink-after-idle and pointer hiding.
    pub last_key: Instant,
    pub pointer_hidden: bool,
    pub pointer_request: Option<crate::settings::Pointer>,
    pub blink_half: u64,
    /// The launch sequence's "then" has run.
    pub then_done: bool,
    pub window_rect: Option<(i32, i32, u32, u32)>,
    /// A row was picked in a persistent Atlas, so Esc may close it now.
    pub atlas_used: bool,
    /// Save the rect a beat after the last move, not on every pixel.
    pub window_rect_dirty: Option<Instant>,
    /// Surface page state: which preset is on, which stop is selected.
    pub preset_name: String,
    pub stop_sel: usize,
    /// How tall the settings column's content was last frame.
    pub settings_reach: f32,
    pub started: Instant,
    pub sound: crate::sound::Sound,
    /// The Start modal, the session it can restore, and recent places.
    pub start: Option<crate::start::Start>,
    pub splash: Option<crate::splash::Splash>,
    pub start_shown: bool,
    pub last_session: Option<crate::start::Session>,
    pub recent: Vec<crate::start::Recent>,
    /// Signature of the last saved session, to save only on change.
    pub session_sig: String,
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
    pub side_hits: Vec<(Rect, SideHit)>,
    /// profile/avatar.png as a texture, when there is one.
    pub avatar: Option<Arc<wgpu::BindGroup>>,
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
            login_note: String::new(),
            theme_edit: crate::theme_edit::ThemeEdit::default(),
            ansi_sel: 1,
            cursor: crate::settings::CursorPrefs::default(),
            header: crate::settings::HeaderPrefs::default(),
            window_named: None,
            instance_port: 0,
            windows: Vec::new(),
            win_menu: false,
            win_anim: Anim::at(0.0),
            kinds_menu: false,
            kinds_anim: Anim::at(0.0),
            press: None,
            flash_anim: Anim::at(0.0),
            rail_anim: Anim::at(0.0),
            next_row_hot: false,
            registered_tabs: usize::MAX,
            hovers: std::collections::HashMap::new(),
            last_key: Instant::now(),
            pointer_hidden: false,
            pointer_request: None,
            blink_half: 0,
            then_done: false,
            window_rect: None,
            atlas_used: false,
            window_rect_dirty: None,
            preset_name: "broadsheet".into(),
            stop_sel: 0,
            settings_reach: 0.0,
            started: Instant::now(),
            sound: crate::sound::Sound::new(crate::sound::SoundPrefs::default()),
            start: None,
            splash: Some(crate::splash::Splash::new()),
            start_shown: false,
            last_session: crate::start::Session::load(),
            recent: crate::start::load_recent(),
            session_sig: String::new(),
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
            side_hits: Vec::new(),
            avatar: None,
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
        app.apply_prefs(crate::prefs::Prefs::load());
        let mode = app.theme.mode;
        app.set_mode(mode);
        app.layout();
        app.apply_term_resizes(true);
        app.refresh_icon();
        app.load_avatar();
        if app.behavior.startup_sound {
            app.play_event("launch");
        }
        Ok(app)
    }

    pub(crate) fn px(&self, v: f32) -> f32 {
        (v * self.scale).round()
    }

    /// Spawn a shell sized for the left pane (split or not), so ConPTY never
    /// sees a resize during startup.
    pub(crate) fn new_term_pane(&mut self, split: bool, profile: usize) -> anyhow::Result<TermPane> {
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
            show_header: split,
            cur_x: Anim::at(0.0),
            cur_y: Anim::at(0.0),
            trail: Vec::new(),
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
            remembered: String::new(),
            load_since: None,
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
        // Nothing is hovered once the pointer is gone.
        self.mouse = (-1.0, -1.0);
        self.dirty = true;
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
        let split = tab.right.is_some();
        let place = |pane: &mut Pane, r: Rect| match pane {
            Pane::Term(t) => {
                t.rect = r;
                t.show_header = split;
                let header = if split { header } else { 0.0 };
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
        self.remember_window();
        self.layout();
        self.resize_due = Some(Instant::now() + std::time::Duration::from_millis(80));
    }

    /// Time-based housekeeping, once per loop iteration.
    pub fn tick(&mut self) {
        self.drain_popups();
        self.apply_boosts();
        self.poll_reader();
        self.sync_favicons();
        // Once the splash has gone: the "then" step, then Atlas if asked.
        if !self.start_shown && self.splash.is_none() {
            self.start_shown = true;
            if !self.then_done {
                self.then_done = true;
                match self.behavior.then {
                    crate::settings::Then::Restore => self.restore_session_pub(),
                    crate::settings::Then::LastPage => {
                        if let Some(crate::start::Saved::Page { url, .. }) = self.recent.iter().find(|r| matches!(r.item, crate::start::Saved::Page { .. })).map(|r| r.item.clone()) {
                            self.open_url(&url, true);
                        }
                    }
                    crate::settings::Then::Shell => {}
                }
            }
            if self.behavior.atlas != crate::settings::AtlasMode::Planet {
                self.open_start();
            }
        }
        self.track_session();
        if self.window_rect_dirty.is_some_and(|t| t.elapsed().as_millis() > 500) {
            self.window_rect_dirty = None;
            self.save_prefs();
        }
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
        if self.surface.texture_motion && self.surface.texture > 0.0 && self.surface.texture_kind != crate::surface::TextureKind::None {
            self.dirty = true;
        }
        if self.registered_tabs != usize::MAX && self.registered_tabs != self.tabs.len() {
            self.register_window();
        }
        // A held NEW TAB fans the kinds out.
        if let Some((at, SideHit::NewShell)) = self.press {
            if at.elapsed().as_millis() >= 240 && !self.kinds_menu {
                self.press = None;
                self.open_kinds_menu();
            }
        }
        if self.header.rail_hover {
            let want = if self.sidebar_hover || (self.sidebar_pinned() && self.sidebar_rect().contains(self.mouse.0, self.mouse.1)) { 1.0 } else { 0.0 };
            if (self.rail_anim.target() - want).abs() > 0.01 {
                self.rail_anim.go(want, self.motion.dur(120.0));
            }
        }
        if self.win_anim.active() || self.kinds_anim.active() || self.flash_anim.active() || self.rail_anim.active() {
            self.dirty = true;
        }
        // A blinking cursor wants a frame at each half period.
        let blinking = match self.cursor.blink {
            crate::settings::Blink::Never => false,
            crate::settings::Blink::AfterIdle => self.last_key.elapsed().as_secs_f32() > 2.0,
            crate::settings::Blink::Always => true,
        };
        if blinking {
            let period = self.cursor.period.max(100) as u128;
            let half = (self.started.elapsed().as_millis() / period) as u64;
            if half != self.blink_half {
                self.blink_half = half;
                self.dirty = true;
            }
        }
        if self.surface.shell == Shell::Aurora {
            // Drift is turns per second; the loop runs at ~60 frames.
            self.shell_phase = (self.shell_phase + self.surface.drift / 60.0) % 1.0;
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
        let compact = self.px(m::ROW_H);
        let ids: Vec<(u64, f32)> = (0..self.tabs.len())
            .map(|i| {
                let t = &self.tabs[i];
                let hidden = t.pinned || (t.parent.is_some() && !self.stack_open(self.stack_root(i)));
                (t.id, if hidden { 0.0 } else { compact })
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
        let mut ready_cue = false;
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
                        if w.load_since.is_none() {
                            w.load_since = Some(Instant::now());
                        }
                    } else if w.load.target < 1.0 || w.load.value < 0.999 {
                        w.load.target = 1.0;
                        if let Some(t0) = w.load_since.take() {
                            if t0.elapsed().as_secs_f32() > 1.0 {
                                ready_cue = true;
                            }
                        }
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
        if ready_cue {
            self.play_event("page.ready");
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

    /// A 32×32 pointer: an ink arrow with a paper edge, or a signal dot.
    pub fn pointer_image(&self, p: crate::settings::Pointer) -> (Vec<u8>, (u16, u16)) {
        let ink = self.theme.ink;
        let paper = self.theme.paper;
        let sig = self.surface.signal;
        let mut px = vec![0u8; 32 * 32 * 4];
        let put = |px: &mut Vec<u8>, x: i32, y: i32, c: nus_render::Color, a: f32| {
            if !(0..32).contains(&x) || !(0..32).contains(&y) {
                return;
            }
            let i = ((y * 32 + x) * 4) as usize;
            px[i] = (c[0] * 255.0) as u8;
            px[i + 1] = (c[1] * 255.0) as u8;
            px[i + 2] = (c[2] * 255.0) as u8;
            px[i + 3] = (a * 255.0) as u8;
        };
        match p {
            crate::settings::Pointer::SignalDot => {
                for y in 0..32 {
                    for x in 0..32 {
                        let d = (((x - 8) as f32).powi(2) + ((y - 8) as f32).powi(2)).sqrt();
                        if d < 7.5 {
                            put(&mut px, x, y, sig, 1.0);
                        } else if d < 9.0 {
                            put(&mut px, x, y, paper, (9.0 - d).clamp(0.0, 1.0));
                        }
                    }
                }
                (px, (8, 8))
            }
            _ => {
                // A classic arrow: left edge vertical, hypotenuse, a tail.
                for y in 0..24i32 {
                    for x in 0..18i32 {
                        let inside = x <= y * 2 / 3 && y <= 18 || (y > 12 && y < 24 && (x as f32 - (y - 12) as f32 * 0.6).abs() < 2.5 && x > 4);
                        let edge = inside && (x == 0 || x >= y * 2 / 3 - 1 || y >= 17 && y <= 18);
                        if inside {
                            put(&mut px, x, y, if edge { paper } else { ink }, 1.0);
                        }
                    }
                }
                (px, (0, 0))
            }
        }
    }

    /// The cursor as the prefs want it, for one pane.
    fn cursor_look(&self, p: &TermPane, focused: bool, tab_signal: Option<nus_render::Color>) -> nus_render::CursorLook {
        use crate::settings::{Blink, CursorColor, CursorShapePref};
        let shape = match self.cursor.shape {
            CursorShapePref::Shell => None,
            CursorShapePref::Block => Some(nus_vt::CursorShape::Block),
            CursorShapePref::Beam => Some(nus_vt::CursorShape::Beam),
            CursorShapePref::Underline => Some(nus_vt::CursorShape::Underline),
        };
        let color = match self.cursor.color {
            CursorColor::Ink => None,
            CursorColor::Signal => Some(self.surface.signal),
            CursorColor::Tab => tab_signal.or(Some(self.surface.signal)),
        };
        let idle = self.last_key.elapsed().as_secs_f32();
        let blinking = match self.cursor.blink {
            Blink::Never => false,
            Blink::AfterIdle => idle > 2.0,
            Blink::Always => true,
        };
        let visible = if blinking && focused {
            let period = self.cursor.period.max(100) as f32 / 1000.0;
            ((self.started.elapsed().as_secs_f32() / period) as u64) % 2 == 0
        } else {
            true
        };
        let _ = p;
        nus_render::CursorLook { shape, color, weight: self.px(self.cursor.weight), visible, hollow_unfocused: self.cursor.hollow_unfocused }
    }

    /// The gliding / comet cursor, drawn by the app between cells.
    fn draw_moving_cursor(&mut self, scene: &mut Scene, p: &mut TermPane, look: nus_render::CursorLook) {
        use crate::settings::CursorMotion;
        let (cw, ch) = p.grid.cell_size();
        let cur = *p.term.cursor();
        let (tx, ty) = (cur.col as f32, cur.row as f32);
        let dur = self.motion.dur(60.0);
        if (p.cur_x.target() - tx).abs() > 0.01 || (p.cur_y.target() - ty).abs() > 0.01 {
            p.cur_x.go(tx, dur);
            p.cur_y.go(ty, dur);
            if self.cursor.motion == CursorMotion::Comet {
                p.trail.push((p.cur_x.value(), p.cur_y.value(), Instant::now()));
                if p.trail.len() > 6 {
                    p.trail.remove(0);
                }
            }
        }
        let (x, y) = (p.cur_x.value(), p.cur_y.value());
        let gliding = p.cur_x.active() || p.cur_y.active();
        let color = look.color.unwrap_or(self.theme.ink);
        let shape = look.shape.unwrap_or(p.term.cursor_style().shape);
        let rect_at = |cx: f32, cy: f32| {
            let px = p.origin.0 + cx * cw;
            let py = p.origin.1 + cy * ch;
            match shape {
                nus_vt::CursorShape::Beam => Rect::new(px, py, look.weight, ch),
                nus_vt::CursorShape::Underline => Rect::new(px, py + ch - look.weight, cw, look.weight),
                _ => Rect::new(px, py, cw, ch),
            }
        };
        // Comet: the trail fades over 240ms.
        if self.cursor.motion == CursorMotion::Comet {
            p.trail.retain(|(_, _, t)| t.elapsed().as_millis() < 240);
            for (cx, cy, t) in &p.trail {
                let a = 1.0 - t.elapsed().as_millis() as f32 / 240.0;
                scene.rect(rect_at(*cx, *cy), Theme::with_alpha(color, 0.35 * a));
            }
            if !p.trail.is_empty() {
                self.dirty = true;
            }
        }
        if gliding {
            scene.rect(rect_at(x, y), Theme::with_alpha(color, 0.9));
            self.dirty = true;
        }
    }

    /// Rebuild the theme for a mode from Broadsheet plus the user's edits.
    pub(crate) fn set_mode(&mut self, mode: nus_render::Mode) {
        let t = self.theme_edit.build(mode, self.surface.signal);
        self.set_theme(t);
    }

    /// Re-apply the current mode (after an edit, a family change, a signal).
    pub(crate) fn rebuild_theme(&mut self) {
        let mode = self.theme.mode;
        self.set_mode(mode);
        self.refresh_icon();
    }

    /// A sound for something that happened, after rules.luau has had its say.
    pub(crate) fn play_event(&mut self, event: &str) {
        let over = self.rules.on_event(event);
        self.sound.event(event, over);
    }

    /// profile/avatar.png → a texture (any size; drawn at 22px).
    pub(crate) fn load_avatar(&mut self) {
        let path = std::env::current_dir().unwrap_or_default().join("profile").join("avatar.png");
        let Ok(file) = std::fs::File::open(&path) else {
            self.avatar = None;
            return;
        };
        let decoder = png::Decoder::new(std::io::BufReader::new(file));
        let Ok(mut reader) = decoder.read_info() else { return };
        let mut buf = vec![0; reader.output_buffer_size()];
        let Ok(info) = reader.next_frame(&mut buf) else { return };
        let (w, h) = (info.width, info.height);
        let rgba: Vec<u8> = match info.color_type {
            png::ColorType::Rgba => buf[..info.buffer_size()].to_vec(),
            png::ColorType::Rgb => buf[..info.buffer_size()].chunks(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
            png::ColorType::Grayscale => buf[..info.buffer_size()].iter().flat_map(|&g| [g, g, g, 255]).collect(),
            png::ColorType::GrayscaleAlpha => buf[..info.buffer_size()].chunks(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
            _ => return,
        };
        // BGRA for the quad pipeline.
        let bgra: Vec<u8> = rgba.chunks(4).flat_map(|p| [p[2], p[1], p[0], p[3]]).collect();
        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("avatar"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Bgra8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            &bgra,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 4), rows_per_image: Some(h) },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        self.avatar = Some((self.bind_texture)(&tex));
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

    /// Save the session when tabs change; remember pages as they settle.
    fn track_session(&mut self) {
        let mut sig = String::new();
        let mut remember: Vec<crate::start::Saved> = Vec::new();
        for t in self.tabs.iter_mut() {
            for p in std::iter::once(&mut t.left).chain(t.right.as_mut()) {
                match p {
                    Pane::Term(tp) => sig.push_str(&format!("t{}|", tp.profile)),
                    Pane::Web(w) => {
                        let (url, title, loading) = {
                            let s = w.tab.shared.borrow();
                            (s.url.clone(), s.title.clone(), s.loading)
                        };
                        sig.push_str(&format!("w{url}|"));
                        if !loading && !url.is_empty() && w.remembered != url {
                            w.remembered = url.clone();
                            remember.push(crate::start::Saved::Page { url, title });
                        }
                    }
                    _ => sig.push('x'),
                }
            }
            sig.push_str(&format!("{}{}|", t.pinned as u8, t.parent.unwrap_or(0)));
        }
        sig.push_str(&self.active.to_string());
        for r in remember {
            self.remember(r);
        }
        if sig != self.session_sig && !self.tabs.is_empty() {
            self.session_sig = sig;
            self.save_session();
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
                    let header = if t.show_header { header } else { 0.0 };
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
        let mut bell = false;
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
                                    bell = true;
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
        if bell {
            self.play_event("bell");
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
        let stops = self.surface.ramp(ink);
        let angle = self.surface.angle;
        // Aurora breathes: the stroke swells and thins with the drift.
        let breath = 1.0 + self.surface.breath * 0.6 * (self.shell_phase * std::f32::consts::TAU * 2.0).sin();
        let sw_live = if self.surface.shell == Shell::Aurora { (sw * breath).max(1.0) } else { sw };
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
            Shell::Gradient => scene.push(nus_render::Instance::stroke_stops(win, radius, sw, &stops, angle, 0.0, false)),
            Shell::Aurora => scene.push(nus_render::Instance::stroke_stops(win, radius, sw_live, &stops, angle, self.shell_phase, true)),
        }
        // Texture: on the carapace, the chrome, or the panes — never on a page or a video.
        if let Some(kind) = self.surface.texture_kind.shader_kind() {
            if self.surface.texture > 0.0 {
                let g = [1.0, 1.0, 1.0, self.surface.texture];
                let pitch = self.px(self.surface.texture_scale);
                let tm = if self.surface.texture_motion { self.started.elapsed().as_secs_f32() % 3600.0 } else { 0.0 };
                let rects: Vec<Rect> = match self.surface.texture_on {
                    crate::surface::TextureOn::Carapace => {
                        // A thin band needs a heavier hand: the texture follows the
                        // carapace's own rounded stroke and is drawn at triple strength.
                        let gc = [1.0, 1.0, 1.0, (self.surface.texture * 3.0).min(1.0)];
                        if self.surface.shell == Shell::Band {
                            if radius > 0.0 {
                                scene.layer(Some(Rect::new(0.0, 0.0, w, sw)));
                                scene.push(nus_render::Instance::texture_stroke(win, kind, gc, pitch, tm, radius, sw));
                                scene.layer(None);
                            } else {
                                scene.push(nus_render::Instance::texture_kind(Rect::new(0.0, 0.0, w, sw), kind, gc, pitch, tm));
                            }
                        } else {
                            scene.push(nus_render::Instance::texture_stroke(win, kind, gc, pitch, tm, radius, sw_live));
                        }
                        Vec::new()
                    }
                    crate::surface::TextureOn::Chrome => {
                        let c = self.content_rect();
                        let st = self.strip_rect();
                        let mut v = vec![st];
                        if self.sidebar_pinned() {
                            v.push(self.sidebar_rect());
                        }
                        let _ = c;
                        v
                    }
                    crate::surface::TextureOn::Panes => vec![self.content_rect()],
                };
                for r in rects {
                    scene.push(nus_render::Instance::texture_kind(r, kind, g, pitch, tm));
                }
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
        // The Space lives in the sidebar; the header keeps the wordmark once.
        let _ = CrumbHit::Space;
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
            // Split tabs read as "left | right"; the pane strips name each side.
            let (icon, title) = match &tab.left {
                Pane::Term(p) if tab.right.is_none() => (nus_render::text::icons::TERMINAL, p.title.clone()),
                Pane::Term(_) => (nus_render::text::icons::TERMINAL, tab.title()),
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
                let hr = Rect::new(rx - p6, strip.y, ic + p12, strip.h);
                self.icon_button(&mut scene, icon, ic, rx, iy, ink, hr, hover_key("winctl", hit as usize), IconMotion::Still);
                self.crumb_hits.push((hr, hit));
                rx -= gap;
            }
            rx -= self.px(6.0);
        }
        rx -= ic;
        let hr = Rect::new(rx - self.px(4.0), strip.y, ic + self.px(8.0), strip.h);
        self.icon_button(&mut scene, nus_render::text::icons::SIDEBAR, ic, rx, iy, if self.sidebar { ink } else { t.dim }, hr, hover_key("sidebar", 0), IconMotion::Pop);
        self.crumb_hits.push((hr, CrumbHit::Sidebar));
        rx -= gap + ic;
        let hr = Rect::new(rx - self.px(4.0), strip.y, ic + self.px(8.0), strip.h);
        self.icon_button(&mut scene, nus_render::text::icons::SEARCH, ic, rx, iy, ink, hr, hover_key("search", 0), IconMotion::Pop);
        self.crumb_hits.push((hr, CrumbHit::Search));
        rx -= gap + ic;
        let hr = Rect::new(rx - self.px(4.0), strip.y, ic + self.px(8.0), strip.h);
        let pc = if self.start.is_some() { self.surface.signal } else { ink };
        self.icon_button(&mut scene, nus_render::text::icons::PLANET, ic, rx, iy, pc, hr, hover_key("atlas", 0), IconMotion::Spin(-25.0));
        self.crumb_hits.push((hr, CrumbHit::Start));
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
            let hr = Rect::new(rx - self.px(4.0), strip.y, ic + self.px(8.0), strip.h);
            let motion = match hit {
                CrumbHit::Waiting => IconMotion::Swing,
                CrumbHit::Assistant => IconMotion::Pop,
                CrumbHit::Ports => IconMotion::Bob,
                _ => IconMotion::Still,
            };
            self.icon_button(&mut scene, icon, ic, rx, iy, if hit == CrumbHit::Waiting && lit { self.surface.signal } else { color }, hr, hover_key("cluster", hit as usize), motion);
            self.crumb_hits.push((hr, hit));
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
            let word = match mode { PaletteMode::Go => "go", PaletteMode::New => "new", PaletteMode::Url => "url", PaletteMode::Rename => "name" };
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
        self.draw_start(&mut scene);
        self.draw_splash(&mut scene);
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
    /// Every icon button goes through here: a soft rounded overlay fades in
    /// under the pointer (so a button reads as a button) and the important
    /// ones move. `hit` is the button's click rect; `key` identifies it
    /// across frames.
    pub(crate) fn icon_button(
        &mut self,
        scene: &mut Scene,
        icon: (&'static str, &'static str),
        px: f32,
        x: f32,
        y: f32,
        color: nus_render::Color,
        hit: Rect,
        key: u64,
        motion: IconMotion,
    ) {
        let (mx, my) = self.mouse;
        let hot = hit.contains(mx, my);
        let dur_in = self.motion.dur(80.0);
        let dur_out = self.motion.dur(140.0);
        let pulse_dur = self.motion.dur(260.0);
        let h = self.hovers.entry(key).or_insert_with(|| Hover { alpha: Anim::at(0.0), pulse: Anim::at(1.0), hot: false });
        if hot != h.hot {
            h.hot = hot;
            h.alpha.go(if hot { 1.0 } else { 0.0 }, if hot { dur_in } else { dur_out });
            if hot {
                h.pulse.replay(0.0, 1.0, pulse_dur);
            }
        }
        let a = h.alpha.value();
        let p = h.pulse.value();
        if h.alpha.active() || h.pulse.active() {
            self.dirty = true;
        }
        // The overlay: ink at a whisper, rounded, a little larger than the glyph.
        if a > 0.005 {
            let pad = self.px(5.0);
            let r = Rect::new(x - pad, y - pad, px + pad * 2.0, px + pad * 2.0);
            scene.push(nus_render::Instance::rounded(r, self.px(4.0), fade(self.theme.ink, 0.10 * a)));
        }
        // Motion: a bump that rises and settles over the pulse.
        let bump = (p * std::f32::consts::PI).sin();
        let (angle, scale, dy) = match motion {
            IconMotion::Still => (0.0, 1.0, 0.0),
            IconMotion::Spin(deg) => (deg.to_radians() * a, 1.0, 0.0),
            IconMotion::Pop => (0.0, 1.0 + 0.16 * bump, -self.px(1.0) * bump),
            IconMotion::Bob => (0.0, 1.0, self.px(2.5) * bump),
            IconMotion::Swing => ((p * std::f32::consts::TAU * 1.5).sin() * (1.0 - p) * 0.28, 1.0, 0.0),
        };
        if self.motion.reduced() || (angle == 0.0 && scale == 1.0 && dy == 0.0) {
            self.fonts.draw_icon(scene, icon, px, x, y, color);
        } else {
            self.fonts.draw_icon_moved(scene, icon, px, x, y + dy, color, angle, scale);
        }
    }

    /// The rail's width right now (0 when off or hidden).
    pub(crate) fn rail_w(&self) -> f32 {
        if self.header.style != crate::settings::HeaderStyle::Rail {
            return 0.0;
        }
        let full = self.px(30.0);
        if self.header.rail_hover { full * self.rail_anim.value() } else { full }
    }

    /// The sidebar minus the rail: where the header, tabs and footer go.
    pub(crate) fn list_rect(&self) -> Rect {
        let sb = self.sidebar_rect();
        let rw = self.rail_w();
        if self.sidebar_right() {
            Rect::new(sb.x, sb.y, sb.w - rw, sb.h)
        } else {
            Rect::new(sb.x + rw, sb.y, sb.w - rw, sb.h)
        }
    }

    /// Height of the sidebar header for the current prefs.
    pub(crate) fn side_header_h(&self) -> f32 {
        use crate::settings::HeaderStyle;
        let row = self.px(9.0) * 2.0 + self.px(m::LABEL_PX) + self.px(m::STRUCTURE);
        let title = if self.header.masthead { self.px(44.0) } else { row };
        let dateline = if self.header.dateline { self.px(16.0) } else { 0.0 };
        match self.header.style {
            HeaderStyle::Bar => {
                if self.header.masthead {
                    title + dateline + if self.header.header_button { row } else { 0.0 }
                } else {
                    row + dateline
                }
            }
            HeaderStyle::Rail => title + dateline + if self.header.header_button { row } else { 0.0 },
        }
    }

    /// The window's name: the user's, else where we are (git root, dominant host), else nus.
    pub(crate) fn window_name(&self) -> String {
        if let Some(n) = &self.window_named {
            if !n.trim().is_empty() {
                return n.clone();
            }
        }
        if let Some(root) = git_root_name() {
            return self.unique_name(root);
        }
        let mut hosts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for t in &self.tabs {
            let (_, host) = t.row_text();
            if !host.is_empty() {
                *hosts.entry(host).or_default() += 1;
            }
        }
        let base = hosts.into_iter().max_by_key(|(_, n)| *n).map(|(h, _)| h).unwrap_or_else(|| "nus".into());
        self.unique_name(base)
    }

    /// Another window already called that? Number this one.
    fn unique_name(&self, base: String) -> String {
        let me = std::process::id();
        let taken = self.windows.iter().filter(|e| e.pid != me && e.pid < me && (e.name == base || e.name.starts_with(&format!("{base} ")))).count();
        if taken == 0 { base } else { format!("{base} {}", taken + 1) }
    }

    /// Where this window is, for the dateline.
    pub(crate) fn dateline(&self) -> String {
        let cwd = std::env::current_dir().unwrap_or_default();
        let home = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).unwrap_or_default();
        let mut here = cwd.to_string_lossy().replace('\\', "/");
        if !home.is_empty() {
            let h = home.replace('\\', "/");
            if here.starts_with(&h) {
                here = format!("~{}", &here[h.len()..]);
            }
        }
        let tabs = self.tabs.len();
        let mut s = format!("{here} · {tabs} tab{}", if tabs == 1 { "" } else { "s" });
        if !self.ports.is_empty() {
            s.push_str(&format!(" · {} port{}", self.ports.len(), if self.ports.len() == 1 { "" } else { "s" }));
        }
        s
    }

    pub(crate) fn register_window(&mut self) {
        self.windows = crate::windows::list();
        self.registered_tabs = self.tabs.len();
        crate::windows::register(&self.window_name(), self.instance_port, self.tabs.len());
        let name = self.window_name();
        self.window.set_title(&if name == "nus" { "nus".to_string() } else { format!("{name} · nus") });
    }

    pub(crate) fn sidebar_geometry(&self) -> SidebarGeom {
        let sb = self.list_rect();
        let space_row = self.side_header_h();
        let pinned: Vec<usize> = (0..self.tabs.len()).filter(|&i| self.tabs[i].pinned).collect();
        let pinned_h = if pinned.is_empty() { 0.0 } else { self.px(8.0) * 2.0 + self.px(m::UI_PX) + self.px(m::STRUCTURE) };
        let row = self.px(m::ROW_H);
        let mut y = sb.y + space_row + pinned_h;
        let mut rows = Vec::new();
        for i in 0..self.tabs.len() {
            if self.tabs[i].pinned {
                continue;
            }
            let hidden = self.tabs[i].parent.is_some() && !self.stack_open(self.stack_root(i));
            let want = if hidden { 0.0 } else { row };
            // Animated height while a stack unfolds; otherwise the target.
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
        SidebarGeom { pinned, pinned_h, rows, foot_y: sb.bottom() - self.px(m::FOOT_H), next_y: y }
    }

    fn draw_sidebar(&mut self, scene: &mut Scene) {
        let t = self.theme.clone();
        let ink = t.ink;
        let full = self.sidebar_rect();
        let sb = self.list_rect();
        let label = self.label();
        let strong = self.label_strong();
        let ui = self.ui();
        let ui_strong = self.ui_strong();
        let dim = Style { color: t.dim, ..label };

        self.side_hits.clear();
        self.draw_rail(scene, full);
        self.draw_sidebar_header(scene, sb);
        let row_h = self.side_header_h();

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

        // Tab rows: icon · title · (hover ×). Waiting shows a signal dot.
        let pad_x = self.px(m::ROW_PAD_X);
        let labels: Vec<String> = g.rows.iter().map(|&(i, _, _)| self.tab_label_of(&tabs, i)).collect();
        let row_h = self.px(m::ROW_H);
        for (k, &(i, y, h)) in g.rows.iter().enumerate() {
            let tab = &tabs[i];
            let waiting = tab.waiting();
            let child = tab.parent.is_some();
            let stack: Vec<usize> = if child { Vec::new() } else { (0..tabs.len()).filter(|&j| tabs[j].parent == Some(tab.id)).collect() };
            let open = !stack.is_empty() && (i == self.active || stack.contains(&self.active));
            let active = i == self.active;
            let hovered = self.hover_row == Some(i);
            if active {
                let ty = if self.tint_anim.active() { self.tint_anim.value() } else { y };
                scene.rect(Rect::new(sb.x, ty, sb.w, row_h), t.tint);
                scene.rect(Rect::new(sb.x, ty, self.px(2.0), row_h), tab.look.signal.unwrap_or(self.surface.signal));
            } else if hovered {
                scene.rect(Rect::new(sb.x, y, sb.w, row_h), fade(t.tint, 0.5));
            }
            scene.layer(Some(Rect::new(sb.x, y, sb.w, h)));
            if self.selected.contains(&i) {
                scene.outline(Rect::new(sb.x, y, sb.w, row_h), self.px(m::STRUCTURE), ink);
            }
            let base = y + (row_h + self.px(m::UI_PX)) / 2.0 - self.px(2.0);
            let mut x = sb.x + pad_x;
            // A dim numeral for the first nine stacks; children hang off a rule.
            let numw = self.px(18.0);
            if child {
                scene.vline(x + self.px(5.0), y, row_h, self.px(m::HAIRLINE), t.dim);
                x += numw;
            } else {
                let n: usize = labels[k].parse().unwrap_or(99);
                if n <= 9 {
                    let ns = Style { color: if active { tab.look.signal.unwrap_or(t.dim) } else { t.dim }, ..label };
                    self.fonts.draw(scene, ns, x, base - self.px(1.0), &n.to_string());
                }
                x += numw;
            }
            let icon = match &tab.left {
                Pane::Term(_) => nus_render::text::icons::TERMINAL,
                Pane::Web(_) => nus_render::text::icons::GLOBE,
                Pane::Settings(_) => nus_render::text::icons::SETTINGS,
                Pane::Hints(_) => nus_render::text::icons::HOME,
            };
            let isz = self.px(15.0);
            let iy = y + ((row_h - isz) / 2.0).round();
            let fav = match &tab.left {
                Pane::Web(w) => w.favicon.as_ref().map(|(_, b)| b.clone()),
                _ => None,
            };
            match fav {
                Some(b) => {
                    scene.texture(Rect::new(x, iy, isz, isz), b, None);
                    scene.layer(Some(Rect::new(sb.x, y, sb.w, h)));
                }
                None => {
                    self.fonts.draw_icon(scene, icon, isz, x, iy, if active { ink } else { t.dim });
                }
            }
            x += isz + self.px(10.0);
            // Right side: × on hover, else a signal dot when waiting, else a
            // collapsed stack's count.
            let mut right = sb.right() - pad_x;
            if hovered {
                let cx = right - isz;
                self.fonts.draw_icon(scene, nus_render::text::icons::CLOSE, isz, cx, iy, ink);
                self.side_hits.push((Rect::new(cx - self.px(6.0), y, isz + self.px(12.0), row_h), SideHit::Close(i)));
                right = cx - self.px(8.0);
            } else if waiting {
                let d = self.px(7.0);
                scene.rect(Rect::new(right - d, y + (row_h - d) / 2.0, d, d), self.surface.signal);
                right -= d + self.px(8.0);
            } else if !stack.is_empty() && !open {
                let tag = format!("{}", stack.len());
                let tw = self.fonts.measure(label, &tag);
                let csz = self.px(11.0);
                self.fonts.draw(scene, dim, right - tw, base - self.px(1.0), &tag);
                self.fonts.draw_icon(scene, nus_render::text::icons::CARET_RIGHT, csz, right - tw - csz - self.px(2.0), y + (row_h - csz) / 2.0, t.dim);
                right -= tw + csz + self.px(10.0);
            }
            let (title, _) = tab.row_text();
            let st = if active { ui_strong } else { ui };
            let st = Style { color: if active { ink } else { Theme::with_alpha(ink, 0.82) }, ..st };
            let title = self.fit(st, &title, right - x);
            self.fonts.draw(scene, st, x, base, &title);
            scene.layer(None);
        }
        self.tabs = tabs;

        // The next ruled row is NEW TAB: a ghost plus where the tab will appear.
        if self.header.next_row && g.next_y + row_h <= g.foot_y {
            let r = Rect::new(sb.x, g.next_y, sb.w, row_h);
            let (mx, my) = self.mouse;
            let hot = r.contains(mx, my) && self.sidebar_visible() && !self.win_menu && !self.kinds_menu;
            let pressing = matches!(self.press, Some((_, SideHit::NewShell)));
            if hot {
                scene.rect(r, fade(t.tint, if pressing { 1.0 } else { 0.6 }));
            }
            let isz = self.px(15.0);
            let iy = g.next_y + ((row_h - isz) / 2.0).round();
            let c = if hot { ink } else { t.dim };
            let x = sb.x + pad_x + self.px(18.0);
            self.fonts.draw_icon(scene, nus_render::text::icons::PLUS, isz, x, iy, c);
            let base = g.next_y + (row_h + self.px(m::UI_PX)) / 2.0 - self.px(2.0);
            if hot {
                self.fonts.draw(scene, Style { color: ink, ..strong }, x + isz + self.px(10.0), base, "NEW TAB");
            } else {
                // A dashed rule where the title would sit.
                let dash = self.px(3.0);
                let mut dx = x + isz + self.px(10.0);
                let end = sb.right() - pad_x;
                while dx < end {
                    scene.hline(dx, g.next_y + row_h / 2.0, dash, self.px(m::HAIRLINE), Theme::with_alpha(t.dim, 0.6));
                    dx += dash * 2.0;
                }
            }
            self.side_hits.push((r, SideHit::NewShell));
        }

        // Footer: one row of verbs. Avatar · new tab · recently closed · downloads · settings.
        let fy = g.foot_y;
        scene.hline(sb.x, fy, sb.w, self.px(m::STRUCTURE), ink);
        let fh = self.px(m::FOOT_H);
        let isz = self.px(16.0);
        let iy = fy + ((fh - isz) / 2.0).round();
        // Avatar: profile/avatar.png, else the initial in the signal square.
        let av = self.px(22.0);
        let ar = Rect::new(sb.x + pad_x, fy + ((fh - av) / 2.0).round(), av, av);
        match self.avatar.clone() {
            Some(b) => {
                scene.texture(ar, b, None);
                scene.layer(None);
            }
            None => {
                scene.rect(ar, self.surface.signal);
                let initial = self.user_initial();
                let iw = self.fonts.measure(strong, &initial);
                self.fonts.draw(scene, Style { color: [1.0, 1.0, 1.0, 1.0], ..strong }, ar.x + (ar.w - iw) / 2.0, ar.y + av / 2.0 + self.px(4.0), &initial);
            }
        }
        self.side_hits.push((Rect::new(sb.x, fy, ar.right() + self.px(8.0) - sb.x, fh), SideHit::Profile));
        let mut x = ar.right() + self.px(14.0);
        let hr = Rect::new(x - self.px(8.0), fy, isz + self.px(16.0), fh);
        self.icon_button(scene, nus_render::text::icons::PLUS, isz, x, iy, ink, hr, hover_key("foot", 0), IconMotion::Pop);
        self.side_hits.push((hr, SideHit::NewTab));
        x += isz + self.px(18.0);
        // The look chip: paper · ink · signal, fanned on hover; opens the look pages.
        self.draw_look_chip(scene, x, fy, fh);
        // Right cluster.
        let mut rx = sb.right() - pad_x;
        for (icon, hit, lit, motion, k) in [
            (nus_render::text::icons::SETTINGS, SideHit::Settings, true, IconMotion::Spin(30.0), 1),
            (nus_render::text::icons::DOWNLOAD, SideHit::Downloads, false, IconMotion::Bob, 2),
            (nus_render::text::icons::HISTORY, SideHit::Closed, !self.closed.is_empty(), IconMotion::Spin(-40.0), 3),
        ] {
            rx -= isz;
            let hr = Rect::new(rx - self.px(8.0), fy, isz + self.px(16.0), fh);
            self.icon_button(scene, icon, isz, rx, iy, if lit { ink } else { t.dim }, hr, hover_key("foot", k), motion);
            self.side_hits.push((hr, hit));
            rx -= self.px(14.0);
        }
        let _ = (label, ui, dim);
        self.draw_sidebar_menus(scene, sb);
    }

    /// Three swatches — paper, ink, signal — stacked like a hand of cards.
    /// On hover they fan out and the signal one lifts; click opens the look pages.
    fn draw_look_chip(&mut self, scene: &mut Scene, x: f32, fy: f32, fh: f32) {
        let sw = self.px(12.0);
        let hit = Rect::new(x - self.px(6.0), fy, sw * 2.2 + self.px(12.0), fh);
        let (mx, my) = self.mouse;
        let hot = hit.contains(mx, my);
        let key = hover_key("look", 0);
        let dur_in = self.motion.dur(160.0);
        let dur_out = self.motion.dur(200.0);
        let h = self.hovers.entry(key).or_insert_with(|| Hover { alpha: Anim::at(0.0), pulse: Anim::at(1.0), hot: false });
        if hot != h.hot {
            h.hot = hot;
            h.alpha.go(if hot { 1.0 } else { 0.0 }, if hot { dur_in } else { dur_out });
        }
        let a = h.alpha.value();
        if h.alpha.active() {
            self.dirty = true;
        }
        let ink = self.theme.ink;
        let paper = self.paper();
        let signal = self.surface.signal;
        let cy = fy + fh / 2.0;
        // At rest: three squares overlapping by half, ink outline on the paper one.
        // Hovered: they spread apart, tilt a little, and rise.
        let step = sw * 0.5 + sw * 0.55 * a;
        let rise = self.px(2.0) * a;
        let radius = self.px(2.0);
        let mut i = 0.0;
        for (c, outline) in [(paper, true), (ink, false), (signal, false)] {
            let tilt = (i - 1.0) * 0.18 * a;
            let r = Rect::new(x + i * step, cy - sw / 2.0 - rise * (i * 0.5 + 0.5), sw, sw);
            if outline {
                scene.push(nus_render::Instance::rounded(r, radius, fade(ink, 0.55)));
                let inner = Rect::new(r.x + self.px(1.0), r.y + self.px(1.0), r.w - self.px(2.0), r.h - self.px(2.0));
                scene.push(nus_render::Instance::rounded(inner, radius - self.px(1.0), c));
            } else {
                scene.push(nus_render::Instance::rounded(r, radius, c));
            }
            let _ = tilt;
            i += 1.0;
        }
        self.side_hits.push((hit, SideHit::Look));
    }

    /// The rail: every window as its square along the sidebar's outer edge.
    fn draw_rail(&mut self, scene: &mut Scene, full: Rect) {
        let rw = self.rail_w();
        if rw < 0.5 {
            return;
        }
        let t = self.theme.clone();
        let ink = t.ink;
        let x = if self.sidebar_right() { full.right() - rw } else { full.x };
        let r = Rect::new(x, full.y, rw, full.h - self.px(m::FOOT_H));
        scene.layer(Some(r));
        let edge_x = if self.sidebar_right() { x } else { x + rw - self.px(m::HAIRLINE) };
        scene.vline(edge_x, r.y, r.h, self.px(m::HAIRLINE), ink);
        let row = self.px(m::ROW_H);
        let sq = self.px(10.0);
        let me = std::process::id();
        let entries = if self.windows.is_empty() { vec![crate::windows::Entry { pid: me, name: self.window_name(), port: self.instance_port, tabs: self.tabs.len() }] } else { self.windows.clone() };
        let (mx, my) = self.mouse;
        for (k, e) in entries.iter().enumerate() {
            let cy = r.y + k as f32 * row;
            let cell = Rect::new(x, cy, rw, row);
            let on = e.pid == me;
            let hot = cell.contains(mx, my) && self.sidebar_visible();
            if on {
                scene.rect(cell, t.tint);
                let bar_x = if self.sidebar_right() { x } else { x + rw - self.px(2.0) };
                scene.rect(Rect::new(bar_x, cy, self.px(2.0), row), self.surface.signal);
            } else if hot {
                scene.rect(cell, fade(t.tint, 0.5));
            }
            let color = if on { self.surface.signal } else { Theme::with_alpha(self.surface.signal, 0.55) };
            scene.rect(Rect::new(x + ((rw - sq) / 2.0).round(), cy + ((row - sq) / 2.0).round(), sq, sq), color);
            self.side_hits.push((cell, SideHit::Rail(k)));
        }
        let cy = r.y + entries.len() as f32 * row;
        let isz = self.px(12.0);
        let cell = Rect::new(x, cy, rw, row);
        let hot = cell.contains(mx, my) && self.sidebar_visible();
        self.fonts.draw_icon(scene, nus_render::text::icons::PLUS, isz, x + ((rw - isz) / 2.0).round(), cy + ((row - isz) / 2.0).round(), if hot { ink } else { t.dim });
        self.side_hits.push((cell, SideHit::RailNew));
        scene.layer(None);
    }

    /// The header: Bar (one ruled row) or Rail (name above the tabs), with
    /// the masthead title and dateline folded in.
    fn draw_sidebar_header(&mut self, scene: &mut Scene, sb: Rect) {
        use crate::settings::HeaderStyle;
        let t = self.theme.clone();
        let ink = t.ink;
        let label = self.label();
        let strong = self.label_strong();
        let row = self.px(9.0) * 2.0 + self.px(m::LABEL_PX) + self.px(m::STRUCTURE);
        let (mx, my) = self.mouse;
        let vis = self.sidebar_visible() && !self.win_menu && !self.kinds_menu;
        let name = self.window_name();
        let mut y = sb.y;
        let bar = self.header.style == HeaderStyle::Bar;
        let masthead = self.header.masthead;

        // Title line (Rail always; Bar when masthead): name, caret on hover.
        if !bar || masthead {
            let th = if masthead { self.px(44.0) } else { row };
            let cell = Rect::new(sb.x, y, sb.w, th - self.px(m::STRUCTURE));
            let hot = cell.contains(mx, my) && vis;
            if hot || self.win_menu {
                scene.rect(cell, fade(t.tint, 0.6));
            }
            if masthead {
                let wm = Style { font: self.f.wordmark, px: self.px(26.0), color: ink, tracking: 0.0 };
                let text = self.fit(wm, &name, sb.w - self.px(52.0));
                self.fonts.draw(scene, wm, sb.x + self.px(12.0), y + self.px(31.0), &text);
            } else {
                scene.rect(Rect::new(sb.x + self.px(12.0), y + self.px(10.0), self.px(10.0), self.px(10.0)), self.surface.signal);
                let text = self.fit(strong, &name.to_uppercase(), sb.w - self.px(60.0));
                self.fonts.draw(scene, strong, sb.x + self.px(30.0), y + self.px(19.0), &text);
            }
            let csz = self.px(12.0);
            let ca = if hot || self.win_menu { 1.0 } else { 0.0 };
            if ca > 0.0 {
                self.fonts.draw_icon(scene, nus_render::text::icons::CARET_DOWN, csz, sb.right() - self.px(12.0) - csz, y + ((th - csz) / 2.0).round() - self.px(1.0), Theme::with_alpha(t.dim, ca));
            }
            self.side_hits.push((cell, SideHit::Window));
            y += th - self.px(m::STRUCTURE);
            if !self.header.dateline && (!bar || !self.header.header_button) {
                scene.hline(sb.x, y, sb.w, self.px(m::STRUCTURE), ink);
                y += self.px(m::STRUCTURE);
            } else if !self.header.dateline {
                scene.hline(sb.x, y, sb.w, self.px(m::HAIRLINE), ink);
                y += self.px(m::STRUCTURE);
            } else {
                y += self.px(m::STRUCTURE);
            }
        }
        // Dateline: where · tabs · ports, dim caps under the title.
        if self.header.dateline {
            let dh = self.px(16.0);
            let dl = Style { color: t.dim, px: self.px(10.0), ..label };
            let text = self.fit(dl, &self.dateline().to_uppercase(), sb.w - self.px(24.0));
            let dy = if !bar || masthead { y - self.px(6.0) } else { y };
            self.fonts.draw(scene, dl, sb.x + self.px(12.0), dy + self.px(11.0), &text);
            if !bar || masthead {
                y = dy + dh;
                scene.hline(sb.x, y, sb.w, if self.header.header_button && bar { self.px(m::HAIRLINE) } else { self.px(m::STRUCTURE) }, ink);
                y += self.px(m::STRUCTURE);
            }
        }
        // The bar row: [■ NAME ▾ | + NEW TAB | ▾], or just NEW TAB under a masthead / rail.
        let button_row = bar || self.header.header_button;
        if button_row {
            let rh = row - self.px(m::STRUCTURE);
            let mut x = sb.x;
            if bar && !masthead {
                let cw = if self.header.show_name { (sb.w * 0.4).floor() } else { self.px(34.0) };
                let cell = Rect::new(x, y, cw, rh);
                let hot = cell.contains(mx, my) && vis;
                if hot || self.win_menu {
                    scene.rect(cell, fade(t.tint, 0.6));
                }
                scene.rect(Rect::new(x + self.px(12.0), y + self.px(10.0), self.px(10.0), self.px(10.0)), self.surface.signal);
                if self.header.show_name {
                    let text = self.fit(label, &name.to_uppercase(), cw - self.px(48.0));
                    self.fonts.draw(scene, label, x + self.px(30.0), y + self.px(19.0), &text);
                    let csz = self.px(11.0);
                    self.fonts.draw_icon(scene, nus_render::text::icons::CARET_DOWN, csz, x + cw - self.px(10.0) - csz, y + ((rh - csz) / 2.0).round(), t.dim);
                }
                self.side_hits.push((cell, SideHit::Window));
                scene.vline(x + cw, y, rh, self.px(m::HAIRLINE), ink);
                x += cw;
            }
            if self.header.header_button || bar {
                let caret_w = if self.header.kinds_caret { self.px(26.0) } else { 0.0 };
                let cell = Rect::new(x, y, sb.right() - x - caret_w, rh);
                let hot = cell.contains(mx, my) && vis;
                let pressing = matches!(self.press, Some((_, SideHit::NewShell)));
                let flash = self.flash_anim.value();
                // Press: ink → signal → ink; hover: ink.
                let fill = if flash > 0.0 {
                    let k = (flash * std::f32::consts::PI).sin();
                    Some(crate::surface::mix(ink, self.surface.signal, k))
                } else if hot || pressing {
                    Some(ink)
                } else {
                    None
                };
                if let Some(f) = fill {
                    scene.rect(cell, f);
                }
                let c = if fill.is_some() { t.paper } else { ink };
                let isz = self.px(12.0);
                let ix = x + self.px(if bar && !masthead { 10.0 } else { 12.0 });
                self.fonts.draw_icon(scene, nus_render::text::icons::PLUS, isz, ix, y + self.px(19.0) - isz + self.px(2.0), c);
                self.fonts.draw(scene, Style { color: c, ..strong }, ix + isz + self.px(6.0), y + self.px(19.0), "NEW TAB");
                if !bar || masthead {
                    let k = "CTRL T";
                    let kw = self.fonts.measure(label, k);
                    self.fonts.draw(scene, Style { color: Theme::with_alpha(c, 0.6), ..label }, cell.right() - self.px(12.0) - kw, y + self.px(19.0), k);
                }
                self.side_hits.push((cell, SideHit::NewShell));
                if self.header.kinds_caret {
                    let cc = Rect::new(cell.right(), y, caret_w, rh);
                    let chot = cc.contains(mx, my) && vis;
                    scene.vline(cc.x, y, rh, self.px(m::HAIRLINE), ink);
                    if chot || self.kinds_menu {
                        scene.rect(Rect::new(cc.x + self.px(m::HAIRLINE), y, cc.w - self.px(m::HAIRLINE), rh), ink);
                    }
                    let csz = self.px(11.0);
                    self.fonts.draw_icon(scene, nus_render::text::icons::CARET_DOWN, csz, cc.x + ((cc.w - csz) / 2.0).round(), y + ((rh - csz) / 2.0).round(), if chot || self.kinds_menu { t.paper } else { t.dim });
                    self.side_hits.push((cc, SideHit::Kinds));
                }
            }
            y += rh;
            scene.hline(sb.x, y, sb.w, self.px(m::STRUCTURE), ink);
        }
    }

    /// The window list and the kinds fan-out, drawn last so they sit over the rows.
    fn draw_sidebar_menus(&mut self, scene: &mut Scene, sb: Rect) {
        let t = self.theme.clone();
        let ink = t.ink;
        let label = self.label();
        let strong = self.label_strong();
        let row = self.px(30.0);
        let (mx, my) = self.mouse;
        let top = sb.y + self.side_header_h();
        if self.win_menu || self.win_anim.active() {
            let k = self.win_anim.value();
            let me = std::process::id();
            let n = self.windows.len().max(1) + 2;
            let h = n as f32 * row * k;
            let r = Rect::new(sb.x, top, sb.w, h);
            scene.layer(Some(r));
            scene.rect(r, t.paper);
            let mut y = top;
            let entries = if self.windows.is_empty() { vec![crate::windows::Entry { pid: me, name: self.window_name(), port: self.instance_port, tabs: self.tabs.len() }] } else { self.windows.clone() };
            for (i, e) in entries.iter().enumerate() {
                let cell = Rect::new(sb.x, y, sb.w, row);
                let hot = cell.contains(mx, my);
                if hot {
                    scene.rect(cell, t.tint);
                }
                let sq = self.px(10.0);
                scene.rect(Rect::new(sb.x + self.px(12.0), y + ((row - sq) / 2.0).round(), sq, sq), if e.pid == me { self.surface.signal } else { Theme::with_alpha(self.surface.signal, 0.55) });
                let base = y + self.px(19.0);
                let st = if e.pid == me { strong } else { label };
                let tabs = format!("{} TAB{}", e.tabs, if e.tabs == 1 { "" } else { "S" });
                let tw = self.fonts.measure(label, &tabs);
                let mut right = sb.right() - self.px(12.0);
                if e.pid == me {
                    let csz = self.px(12.0);
                    self.fonts.draw_icon(scene, nus_render::text::icons::CHECK, csz, right - csz, y + ((row - csz) / 2.0).round(), ink);
                    right -= csz + self.px(8.0);
                }
                self.fonts.draw(scene, Style { color: t.dim, ..label }, right - tw, base, &tabs);
                let text = self.fit(st, &e.name.to_uppercase(), right - tw - self.px(8.0) - (sb.x + self.px(30.0)));
                self.fonts.draw(scene, st, sb.x + self.px(30.0), base, &text);
                scene.hline(sb.x, y + row - self.px(m::HAIRLINE), sb.w, self.px(m::HAIRLINE), Theme::with_alpha(ink, 0.18));
                if self.win_menu {
                    self.side_hits.push((cell, if e.pid == me { SideHit::Window } else { SideHit::WinFront(i) }));
                }
                y += row;
            }
            for (icon, text, key, hit) in [
                (nus_render::text::icons::PENCIL, "RENAME", "F2", SideHit::Rename),
                (nus_render::text::icons::PLUS, "NEW WINDOW", "CTRL N", SideHit::NewWindow),
            ] {
                let cell = Rect::new(sb.x, y, sb.w, row);
                let hot = cell.contains(mx, my);
                if hot {
                    scene.rect(cell, t.tint);
                }
                let isz = self.px(12.0);
                let c = if hot { ink } else { t.dim };
                self.fonts.draw_icon(scene, icon, isz, sb.x + self.px(11.0), y + ((row - isz) / 2.0).round(), c);
                self.fonts.draw(scene, Style { color: c, ..label }, sb.x + self.px(30.0), y + self.px(19.0), text);
                let kw = self.fonts.measure(label, key);
                self.fonts.draw(scene, Style { color: t.dim, ..label }, sb.right() - self.px(12.0) - kw, y + self.px(19.0), key);
                if self.win_menu {
                    self.side_hits.push((cell, hit));
                }
                y += row;
            }
            scene.hline(sb.x, top + h - self.px(m::STRUCTURE), sb.w, self.px(m::STRUCTURE), ink);
            scene.layer(None);
        }
        if self.kinds_menu || self.kinds_anim.active() {
            let k = self.kinds_anim.value();
            let n = self.profiles.len() + 1;
            let h = n as f32 * row * k;
            let r = Rect::new(sb.x, top, sb.w, h);
            scene.layer(Some(r));
            scene.rect(r, t.paper);
            let mut y = top;
            let profiles = self.profiles.clone();
            for (i, p) in profiles.iter().enumerate() {
                let cell = Rect::new(sb.x, y, sb.w, row);
                let hot = cell.contains(mx, my);
                if hot {
                    scene.rect(cell, t.tint);
                }
                let sq = self.px(10.0);
                let ctx = TabCtx { kind: "terminal", index: self.tabs.len(), profile: &p.name, space: &self.space_name, space_signal: self.surface.signal, theme: if self.theme.mode == nus_render::Mode::Ink { "ink" } else { "paper" }, host: "", parent: None };
                let color = self.rules.new_tab(&ctx).signal.unwrap_or(self.surface.signal);
                scene.rect(Rect::new(sb.x + self.px(12.0), y + ((row - sq) / 2.0).round(), sq, sq), color);
                let st = if i == self.behavior.default_profile { strong } else { label };
                self.fonts.draw(scene, st, sb.x + self.px(30.0), y + self.px(19.0), &p.name.to_uppercase());
                if i == self.behavior.default_profile {
                    let d = "DEFAULT";
                    let dw = self.fonts.measure(label, d);
                    self.fonts.draw(scene, Style { color: t.dim, ..label }, sb.right() - self.px(12.0) - dw, y + self.px(19.0), d);
                }
                scene.hline(sb.x, y + row - self.px(m::HAIRLINE), sb.w, self.px(m::HAIRLINE), Theme::with_alpha(ink, 0.18));
                if self.kinds_menu {
                    self.side_hits.push((cell, SideHit::Kind(i)));
                }
                y += row;
            }
            let cell = Rect::new(sb.x, y, sb.w, row);
            let hot = cell.contains(mx, my);
            if hot {
                scene.rect(cell, t.tint);
            }
            let isz = self.px(12.0);
            self.fonts.draw_icon(scene, nus_render::text::icons::GLOBE, isz, sb.x + self.px(11.0), y + ((row - isz) / 2.0).round(), ink);
            self.fonts.draw(scene, label, sb.x + self.px(30.0), y + self.px(19.0), "PAGE");
            let k2 = "CTRL L";
            let kw = self.fonts.measure(label, k2);
            self.fonts.draw(scene, Style { color: t.dim, ..label }, sb.right() - self.px(12.0) - kw, y + self.px(19.0), k2);
            if self.kinds_menu {
                self.side_hits.push((cell, SideHit::KindPage));
            }
            scene.hline(sb.x, top + h - self.px(m::STRUCTURE), sb.w, self.px(m::STRUCTURE), ink);
            scene.layer(None);
        }
    }

    pub(crate) fn open_win_menu(&mut self) {
        self.kinds_menu = false;
        self.windows = crate::windows::list();
        self.win_menu = true;
        self.win_anim.replay(0.0, 1.0, self.motion.dur(160.0));
        self.dirty = true;
    }

    pub(crate) fn open_kinds_menu(&mut self) {
        self.win_menu = false;
        self.kinds_menu = true;
        self.kinds_anim.replay(0.0, 1.0, self.motion.dur(140.0));
        self.dirty = true;
    }

    pub(crate) fn close_menus(&mut self) {
        if self.win_menu {
            self.win_menu = false;
            self.win_anim.go(0.0, self.motion.dur(100.0));
        }
        if self.kinds_menu {
            self.kinds_menu = false;
            self.kinds_anim.go(0.0, self.motion.dur(100.0));
        }
        self.dirty = true;
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
        let tab = self.make_tab(Pane::Settings(SettingsPane { rect: Rect::new(0.0, 0.0, 1.0, 1.0), section: 0, scroll: 0.0, drill: false }), None);
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
        self.play_event("onboarding.tick");
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
                let p = SettingsPane { rect: p.rect, section: p.section, scroll: p.scroll, drill: p.drill };
                self.draw_settings(scene, &p);
            }
            Pane::Hints(p) => {
                let r = p.rect;
                self.draw_hints(scene, r);
            }
            Pane::Term(p) => {
                let r = p.rect;
                let hh = if p.show_header { self.header_h() } else { 0.0 };
                if p.show_header {
                    // Split: this pane's strip names its shell and grid.
                    let base = r.y + self.px(m::HEADER_PAD_Y) + self.px(m::UI_PX) - self.px(3.0);
                    let mut x = r.x + self.px(m::HEADER_PAD_X);
                    let isz = self.px(13.0);
                    self.fonts.draw_icon(scene, nus_render::text::icons::TERMINAL, isz, x, base - isz + self.px(2.0), ink);
                    x += isz + self.px(8.0);
                    let _ = n;
                    x += self.fonts.draw(scene, strong, x, base, &p.title.to_uppercase()) + self.px(14.0);
                    let dims = format!("{}×{}", p.term.cols(), p.term.rows());
                    let dw = self.fonts.measure(label, &dims);
                    let dx = r.right() - self.px(m::HEADER_PAD_X) - dw;
                    self.fonts.draw(scene, label, dx, base, &dims);
                    self.fonts.draw_icon(scene, nus_render::text::icons::EXPAND, isz, dx - isz - self.px(6.0), base - isz + self.px(2.0), t.dim);
                    let _ = x;
                    scene.hline(r.x, r.y + hh - self.px(m::HAIRLINE), r.w, self.px(m::HAIRLINE), ink);
                }
                if let Some(proc_name) = p.confirm_close.clone() {
                    let drop = self.band_anim.value();
                    let bh = self.header_h();
                    let cr = Rect::new(r.x, r.y + hh - (1.0 - drop) * bh, r.w, bh);
                    scene.layer(Some(Rect::new(r.x, r.y + hh, r.w, bh)));
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
                let look = self.cursor_look(p, focused, look.signal);
                let gliding = !matches!(self.cursor.motion, crate::settings::CursorMotion::Jump) && (p.cur_x.active() || p.cur_y.active());
                let mut lk = look;
                if gliding {
                    lk.visible = false;
                }
                p.grid.draw_with(scene, &mut self.fonts, &p.term, p.origin, focused, lk);
                if look.visible && focused && !matches!(self.cursor.motion, crate::settings::CursorMotion::Jump) {
                    self.draw_moving_cursor(scene, p, look);
                }
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
                let actions: [(String, Action); 12] = [
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
                    ("atlas · last session, recent pages and shells".to_string(), Action::Start),
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
            PaletteMode::Rename => {
                if q.is_empty() {
                    rows.push(row("·", format!("name this window · now “{}” · empty = automatic", self.window_name()), Action::RenameWindow(String::new())));
                } else {
                    rows.push(row("→", format!("call this window “{q}”"), Action::RenameWindow(q.to_string())));
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
        self.play_event("palette.open");
        self.dirty = true;
    }

    fn run(&mut self, action: Action) {
        match action {
            Action::SwitchTab(i) => self.activate(i),
            Action::NewTerminal(p) => self.new_tab(p),
            Action::RenameWindow(n) => {
                self.window_named = if n.trim().is_empty() { None } else { Some(n.trim().to_string()) };
                self.register_window();
                self.save_prefs();
                self.dirty = true;
            }
            Action::NewWindow => crate::windows::spawn(),
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
            Action::Start => self.open_start(),
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

        if self.splash.is_some() {
            return;
        }
        // Atlas owns the keyboard while open; then the palette.
        if self.start_key(ev) {
            return;
        }
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
                WKey::Named(NamedKey::ArrowDown) => {
                    self.palette_sel += 1;
                    self.play_event("palette.move");
                }
                WKey::Named(NamedKey::ArrowUp) => {
                    self.palette_sel = self.palette_sel.saturating_sub(1);
                    self.play_event("palette.move");
                }
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
        if pressed && code == Some(KeyCode::F2) && !ctrl && !shift && self.palette.is_none() {
            return self.open_palette(PaletteMode::Rename);
        }
        if pressed && code == Some(KeyCode::KeyN) && ctrl && !shift && self.palette.is_none() {
            return self.run(Action::NewWindow);
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
        if pressed {
            self.last_key = Instant::now();
            if self.cursor.hide_while_typing && !self.pointer_hidden && matches!(self.tabs.get_mut(self.active).map(|t| t.focused()), Some(Pane::Term(_))) {
                self.window.set_cursor_visible(false);
                self.pointer_hidden = true;
            }
        }
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

    pub(crate) fn make_tab(&mut self, left: Pane, right: Option<Pane>) -> Tab {
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
        let host = match left {
            Pane::Web(w) => {
                let s = w.tab.shared.borrow();
                s.url.split("//").nth(1).unwrap_or("").split('/').next().unwrap_or("").trim_start_matches("www.").to_string()
            }
            _ => String::new(),
        };
        let index = self.top_level().len();
        self.rules.new_tab(&TabCtx {
            kind,
            index,
            profile,
            space: &self.space_name,
            space_signal: self.surface.signal,
            theme: if self.theme.mode == nus_render::Mode::Ink { "ink" } else { "paper" },
            host: &host,
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
            self.play_event("tab.switch");
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
        if let Some(p) = self.profiles.get(profile) {
            let name = p.name.clone();
            self.remember(crate::start::Saved::Shell { profile: name });
        }
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
        self.play_event("tab.close");
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
        let was = self.mouse;
        self.mouse = (x, y);
        if (was.0 - x).abs() + (was.1 - y).abs() > 0.0 {
            let strip = self.strip_rect();
            if strip.contains(x, y) || strip.contains(was.0, was.1) || (self.sidebar_visible() && (self.sidebar_rect().contains(x, y) || self.sidebar_rect().contains(was.0, was.1))) {
                self.dirty = true;
            }
        }
        if self.pointer_hidden {
            self.window.set_cursor_visible(true);
            self.pointer_hidden = false;
        }
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
                self.play_event("sidebar.reveal");
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
            CrumbHit::Start => self.open_start(),
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

        if self.start_mouse(button, state, x, y) {
            return;
        }
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

        // A menu is up: a click elsewhere closes it.
        if pressed && (self.win_menu || self.kinds_menu) {
            let on_menu = self.side_hits.iter().any(|(r, h)| r.contains(x, y) && matches!(h, SideHit::WinFront(_) | SideHit::Rename | SideHit::NewWindow | SideHit::Kind(_) | SideHit::KindPage | SideHit::Window | SideHit::Kinds));
            if !on_menu {
                self.close_menus();
            }
        }
        // Right-click on NEW TAB fans out the kinds.
        if pressed && button == MouseButton::Right && self.sidebar_visible() && self.sidebar_rect().contains(x, y) {
            if self.side_hits.iter().any(|(r, h)| *h == SideHit::NewShell && r.contains(x, y)) {
                self.open_kinds_menu();
            }
            return;
        }
        // NEW TAB acts on release: a hold fans the kinds out instead.
        if !pressed && button == MouseButton::Left {
            if let Some((at, SideHit::NewShell)) = self.press.take() {
                let still = self.side_hits.iter().any(|(r, h)| *h == SideHit::NewShell && r.contains(x, y));
                if still && at.elapsed().as_millis() < 240 && !self.kinds_menu {
                    if self.header.flash {
                        self.flash_anim.replay(0.0, 1.0, self.motion.dur(120.0));
                        self.flash_anim.go(0.0, self.motion.dur(120.0));
                    }
                    let p = self.behavior.default_profile;
                    self.new_tab(p);
                    self.dirty = true;
                }
                return;
            }
        }
        // Sidebar: pinned cells, tab rows, footer. Ctrl-click selects, Shift-click ranges.
        if pressed && button == MouseButton::Left && self.sidebar_visible() && self.sidebar_rect().contains(x, y) {
            let sb = self.sidebar_rect();
            let g = self.sidebar_geometry();
            if let Some(&(_, hit)) = self.side_hits.iter().find(|(r, _)| r.contains(x, y)) {
                match hit {
                    SideHit::Close(i) => {
                        self.selected.clear();
                        self.activate(i);
                        self.close_tabs(false);
                    }
                    SideHit::Profile => {
                        self.open_settings();
                        if let Some(Pane::Settings(s)) = self.tabs.get_mut(self.active).map(|t| &mut t.left) {
                            s.section = crate::settings::SEC_TERMINAL;
                        }
                    }
                    SideHit::NewTab => self.open_palette(PaletteMode::New),
                    SideHit::NewShell => {
                        self.press = Some((Instant::now(), SideHit::NewShell));
                        self.play_event("control.press");
                    }
                    SideHit::Window => {
                        if self.win_menu {
                            self.close_menus();
                        } else {
                            self.open_win_menu();
                        }
                    }
                    SideHit::Kinds => {
                        if self.kinds_menu {
                            self.close_menus();
                        } else {
                            self.open_kinds_menu();
                        }
                    }
                    SideHit::Kind(p) => {
                        self.close_menus();
                        self.new_tab(p);
                    }
                    SideHit::KindPage => {
                        self.close_menus();
                        self.open_palette(PaletteMode::New);
                    }
                    SideHit::WinFront(i) => {
                        self.close_menus();
                        if let Some(e) = self.windows.get(i).cloned() {
                            crate::windows::front(&e);
                        }
                    }
                    SideHit::Rail(k) => {
                        if self.windows.is_empty() {
                            self.windows = crate::windows::list();
                        }
                        if let Some(e) = self.windows.get(k).cloned() {
                            if e.pid != std::process::id() {
                                crate::windows::front(&e);
                            }
                        }
                    }
                    SideHit::RailNew | SideHit::NewWindow => {
                        self.close_menus();
                        self.run(Action::NewWindow);
                    }
                    SideHit::Rename => {
                        self.close_menus();
                        self.open_palette(PaletteMode::Rename);
                    }
                    SideHit::Look => {
                        self.open_settings();
                        if let Some(Pane::Settings(s)) = self.tabs.get_mut(self.active).map(|t| &mut t.left) {
                            s.section = 1;
                        }
                    }
                    SideHit::Closed => {
                        self.open_palette(PaletteMode::Go);
                        if let Some((_, input)) = self.palette.as_mut() {
                            input.push_str("reopen");
                        }
                    }
                    SideHit::Downloads => {}
                    SideHit::Settings => self.open_settings(),
                }
                self.dirty = true;
                return;
            }
            if y > g.foot_y {
                return;
            }
            let space_row = self.header_h();
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
                Pane::Settings(s) if s.rect.contains(x, y) => {
                    let dy = match delta {
                        MouseScrollDelta::LineDelta(_, y) => y * 60.0 * self.scale,
                        MouseScrollDelta::PixelDelta(p) => p.y as f32,
                    };
                    let max = (self.settings_reach - s.rect.h).max(0.0);
                    s.scroll = (s.scroll - dy).clamp(0.0, max);
                    self.dirty = true;
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

    /// The window's outer rect, remembered for "last size & place".
    pub(crate) fn remember_window(&mut self) {
        if self.fullscreen || self.window.is_maximized() {
            return;
        }
        if let (Ok(p), s) = (self.window.outer_position(), self.window.inner_size()) {
            let r = (p.x, p.y, s.width, s.height);
            if self.window_rect != Some(r) && s.width > 200 && s.height > 200 {
                self.window_rect = Some(r);
                self.window_rect_dirty = Some(Instant::now());
            }
        }
    }

    pub fn window_moved(&mut self, x: i32, y: i32) {
        self.remember_window();
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

/// The name of the git repository the app was launched in, if any.
fn git_root_name() -> Option<String> {
    let mut d = std::env::current_dir().ok()?;
    loop {
        if d.join(".git").exists() {
            return d.file_name().map(|n| n.to_string_lossy().to_string());
        }
        if !d.pop() {
            return None;
        }
    }
}

/// A colour at a fraction of its own alpha.
fn fade(c: nus_render::Color, k: f32) -> nus_render::Color {
    [c[0], c[1], c[2], c[3] * k]
}
