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

#[path = "tooltip_state.rs"]
pub(crate) mod tooltip_state;

// Scrollback per shell is TERMINAL · SCROLLBACK (behavior.scrollback).

/// Platform key label: "⌘K" on macOS, "CTRL+K" elsewhere. `shift` adds ⇧ / SHIFT+.
pub(crate) fn key(k: &str, shift: bool) -> String {
    if cfg!(target_os = "macos") {
        format!("⌘{}{}", if shift { "⇧" } else { "" }, k)
    } else {
        format!("CTRL+{}{}", if shift { "SHIFT+" } else { "" }, k)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PaletteMode {
    Application,
    /// ⌘K: tabs, actions, then URL/search.
    Go,
    /// ⌘T: profiles for a terminal tab, or a URL/search for a browser tab.
    New,
    /// ⌘L: navigate the browser pane.
    Url,
    /// Shift+F2: name this window.
    Rename,
    /// SAVE LAYOUT: a name for profile/layouts/<name>.nus.luau.
    SaveLayout,
    /// SYNC: a folder path, a git remote, or a key to join.
    SyncFolder,
    SyncGit,
    SyncJoin,
    /// STARTUP · PLACE: lat, lon.
    Place,
    Settings,
    Preference(crate::assistants::Field),
    Assistant(u8),
    PromptPin,
    SavedEdit(usize),
    SavedName(usize),
    /// Name a tab; its icon (an emoji, or any short string).
    RenameTab(usize),
    IconTab(usize),
    /// Pick a folder for tab i's page, or name a new one.
    Folder(usize),
}

#[derive(Clone, PartialEq, Debug)]
pub enum Action {
    Library,
    SaveReading,
    RefreshReading,
    ReadingControl(crate::library::Hit),
    Application(crate::application_menu::Command),
    Preference(crate::assistants::Field, String),
    AssistantDraft(u8, String),
    AssistantStart(u8, String),
    PromptShell(String),
    PromptPin(String),
    SavedEdit(usize, String),
    SavedName(usize, String),
    SavedUse(usize, bool),
    SwitchTab(usize),
    NewTerminal(usize),
    NewBrowser(String),
    OpenInPane(String),
    /// A file, in the editor.
    OpenFile(String),
    /// The ports board.
    Board,
    Hatch,
    Hoist,
    BlockMarkdown(String),
    BlockGist(String),
    /// A skill, with a subject typed after its name.
    Skill(usize, String),
    OpenLayout(String),
    SaveLayout(String),
    OpenPalette(PaletteMode),
    Tidy,
    Noop,
    SyncNow,
    SyncKey,
    SyncJoin(String),
    SyncFolder(String),
    SyncGit(String),
    /// Dedupe: switch to the tab that has this page (and close this one), or keep both.
    DedupeSwitch(usize, usize),
    DedupeKeep(u64),
    /// Type a command into the focused (or a new) terminal and run it.
    RunInShell(String),
    ToggleSplit,
    CloseTab,
    ToggleSidebar,
    Downloads,
    /// The sidebar's FILES page.
    ToggleFiles,
    /// Bind this window to a folder (None: let it follow the shell).
    Workspace(Option<String>),
    /// Make this window the folder's: bound, a shell born there in this
    /// tab (the prompt gives way), FILES on its tree.
    OpenFolder(String),
    TogglePin,
    Reopen,
    ShellStyle,
    ShellRadius(f32),
    Start,
    Pip,
    /// Pin the playing video to this tab's shell (pip_dock.rs).
    DockVideo,
    RenameWindow(String),
    NewWindow,
    NewPrivateWindow,
    Report(crate::support::Kind),
    Welcome,
    FoldAll,
    NameTab(usize, String),
    IconTab(usize, String),
    Tile,
    Untile,
    TileSwap,
    KeepPeek,
    Compact,
    Focus,
    Ask,
    /// This window's container; a new one; the page again in one.
    Container(String),
    NewContainer(String),
    ReopenIn(String),
    /// Run a named chain from rules.luau.
    Chain(String),
    /// The journal of the focused shell's folder, as a page beside it.
    JournalPage,
    /// The timeline over the active tab.
    Timeline,
    /// The prompt, as a tab.
    Home,
    /// The focused page's address to the clipboard.
    CopyUrl,
    /// STARTUP · PLACE.
    SetPlace(Option<[f32; 2]>),
    /// A shell in this folder, as a tab (a stop on the plate).
    ShellAt(String),
    /// Back to a held shell by its id (a stop on the plate).
    AttachHeld(String),
    /// STARTUP · THEN · HOME PAGE, set from the palette.
    SetHome(String),
    /// A page pinned to the sidebar by its address, without visiting it.
    PinUrl(String),
    /// The active tab, pinned (the palette's `pin` with no address).
    PinActive,
    /// Any open tab, pinned by id, without going to it (`pin <its name>`).
    PinTab(u64),
    /// The layout's history (the pane director).
    PaneUndo,
    PaneRedo,
    /// A pane op from the palette, on the active tab.
    Pane(crate::director::Op),
    /// The focused pane (or tab) to another window, or a new one.
    SendTo(crate::send::Dest),
    /// PROMPT · SEARCH ENGINE: a template of your own, from the palette.
    SetSearch(String),
    /// What is open now becomes the launch tabs.
    SetLaunchTabs,
    /// The active tab as one HTML file that replays anywhere.
    ShareReplay,
    SaveToFolder(usize, usize),
    OpenItem(usize, usize),
    SaveToNewFolder(usize, String),
    /// Open settings at a section (and a LOOK tab).
    SettingsAt(usize, Option<usize>),
    SettingsRow(usize, usize, usize),
    ColourTab(usize, Option<nus_render::Color>),
}

pub use crate::surface::Shell;
use crate::anim::{base, Anim, BarColor, BarStyle, Follow, LoadBar, Motion};
use crate::surface::{Fullscreen, HoverFrom, Overrides, Rules, Side, SidebarRules, Surface, TabCtx};

pub enum Closed {
    Term(usize),
    Web(String),
    /// A held shell that was detached: reopen attaches to it.
    Held(nus_pty::hold::Info),
}

/// Click targets in the top strip.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CrumbHit {
    Menu,
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
    Updates,
    /// The wordmark: home, latched (home.rs).
    Nus,
    /// The strip's assistant count: to the one waiting (ledger.rs).
    Agents,
    /// Finish Work: the coffee, on laptops (finish_work.rs).
    FinishWork,
}

#[derive(Clone)]
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
    pub editor: FontId,
    /// Newsreader, for the reader.
    pub serif: FontId,
}

pub struct TermPane {
    pub zoom: u32,
    pub term: Term,
    pub pty: nus_pty::Pty,
    pub grid: GridRenderer,
    pub title: String,
    pub profile: usize,
    /// The ask panel beside this shell, while open.
    pub ask: Option<crate::ask::Ask>,
    pub profile_name: String,
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
    /// The smeared caret's four corners (Neovide's cursor, ported).
    pub smear: crate::smear::Smear,
    /// An eased scroll in flight (neoscroll-style).
    pub trip: Option<crate::scrolling::Trip>,
    /// What has been typed at the current prompt, for the URL rule.
    pub line: String,
    /// False once an editing key made `line` unreliable; reset on Enter.
    pub line_ok: bool,
    /// The column the line started at: the hint anchors here, so the
    /// shell's echo lag and caret moves never shift it.
    pub line_col: Option<usize>,
    /// Shell integration: where the shell is, what it's doing.
    pub cwd: Option<String>,
    pub progress: Option<(u8, u8)>,
    pub running_since: Option<Instant>,
    /// The same moment on the wall clock, for the journal.
    pub running_at: Option<u64>,
    pub last_exit: Option<i32>,
    /// Most recent shell-marked command; updated on marks, not polled from scrollback.
    pub work_command: String,
    pub work_completed: bool,
    /// The running command's generation (finish_work.rs), from the shell's
    /// start mark to its end mark; and how the user started it, if they did.
    pub work_id: Option<u64>,
    pub work_origin: Option<crate::finish_work::Origin>,
    /// The user's own submission, waiting for the shell to say a command
    /// started: Enter at the prompt, or a click that typed the command.
    pub armed: Option<(Instant, crate::finish_work::Origin)>,
    /// Who asked for `type_at_prompt`: the user (Home, Run again, a saved
    /// command, a port action) or nobody (a restore re-running something).
    pub type_origin: Option<crate::finish_work::Origin>,
    /// A command that took a while just finished: (exit, when) for the badge.
    pub done: Option<(Option<i32>, Instant)>,
    /// A paste waiting for a yes (many lines, or control characters).
    pub confirm_paste: Option<String>,
    /// Interaction layer (termui): selection, search, hints, scrollbar, block chips.
    pub sel: Option<crate::termui::Selection>,
    pub search: Option<crate::termui::Search>,
    pub hints: Option<crate::termui::Hints>,
    pub clicks: Option<crate::termui::Clicks>,
    pub scrollbar: Option<Rect>,
    pub scroll_drag: bool,
    /// Mouse reporting: the button the application was told is down, and
    /// the last cell it heard about, so motion reports once per cell.
    pub mouse_held: Option<nus_vt::input::MouseButton>,
    pub mouse_last: Option<(usize, usize)>,
    /// The prompt line's language server, once tried.
    pub plsp: Option<crate::prompt_lsp::LineLsp>,
    pub plsp_tried: bool,
    /// The program running here ("claude", "nvim"), empty at a prompt;
    /// the mark count it was read at.
    pub program: String,
    pub program_marks: usize,
    /// The assistant at work here, as the Ledger shows it (agent.rs).
    pub agent: Option<crate::agent::Agent>,
    /// A program's last notification (OSC 9 / 777), until it is seen.
    pub notice: Option<String>,
    /// This pane's name for `nus hook`: exported to its shell as NUS_PANE.
    pub pane_uid: String,
    /// An ssh profile's host: this whole pane is somewhere else.
    pub ssh_host: Option<String>,
    /// The folder the shell opened in, for SHELL COLORS · BY FOLDER.
    pub opened_in: Option<String>,
    pub chip_hits: Vec<(Rect, usize)>,
    /// The command a restart killed, drawn at the seam after the snapshot.
    pub cutoff: Option<crate::cutoff::CutOff>,
    pub cutoff_hit: Option<Rect>,
    /// Typed into the shell the next time it is at a prompt (restore's run-again).
    pub type_at_prompt: Option<String>,
    /// The timeline is showing this pane a moment, not now.
    pub replay: bool,
    /// The link under the pointer, and the one a click is asking about.
    pub link_hover: Option<crate::links::LinkAt>,
    pub link_ask: Option<crate::links::LinkAt>,
    /// Blocks: folded output ranges (absolute lines), the display list they
    /// make, the block walked to, the filter, the lamps' hit rects.
    /// A shell set its colours (OSC 10/11): offer them for the look, since when.
    pub colour_offer: Option<Instant>,
    pub colour_offer_hit: Option<Rect>,
    pub folds: Vec<(u64, u64)>,
    pub view: Vec<nus_vt::grid::Display>,
    pub view_key: Option<(u64, usize, usize, Option<(u64, u64)>)>,
    pub block_sel: Option<u64>,
    pub block_filter: Option<String>,
    pub lamp_hits: Vec<(Rect, u64)>,
    /// A diff block's hunk chips: (rect, block start, hunk index, what).
    pub hunk_hits: Vec<(Rect, u64, usize, crate::diffs::Do)>,
    /// Parsed diffs by block start: (the block's end line, the hunks).
    pub diff_cache: std::collections::HashMap<u64, (u64, Vec<crate::diffs::Hunk>)>,
    pub select_all_at: Option<Instant>,
    pub hover_block: u64,
    /// Terminal images as textures, by image id; rebuilt when the term's
    /// images_gen moves.
    pub image_tex: std::collections::HashMap<u32, Arc<wgpu::BindGroup>>,
    pub images_gen: u64,
    /// Commands run under this profile, oldest first (profile/history).
    pub history: Vec<String>,
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
    pub load_reported: f32,
    /// Find in page, while the band is up.
    pub find: Option<crate::webui::Find>,
    /// Dedupe: (this tab, the earlier tab with the same page), and the band's chips.
    pub dedupe: Option<(usize, usize)>,
    /// Resolved before drawing takes the tab list out of App.
    pub dedupe_label: String,
    pub dedupe_hits: Vec<(Rect, bool)>,
    /// The assistant's hands on this page: a band while one waits, chips after.
    pub hands: crate::hands::Hands,
    /// The timeline's still of this page, drawn instead of the live texture.
    pub still: Option<Arc<wgpu::BindGroup>>,
    /// The permission band's ALLOW / DENY chips.
    pub perm_hits: Vec<(Rect, bool)>,
    /// The container this page lives in.
    pub container: String,
    /// Focus mode: no URL row, no tools row, the page alone.
    pub bare: bool,
    /// The site panel (the gear at the end of the URL row), and its controls.
    pub site_panel: bool,
    pub site_hits: Vec<(Rect, crate::sites::SiteHit)>,
    /// Blanked for being idle; the URL to come back to.
    pub asleep: Option<String>,
    /// Wheel fractions not yet handed to the page (scrolling.rs).
    pub wheel_carry: (f32, f32),
    /// The last wheel came from a trackpad (precise deltas): only those
    /// stretch the page at its end, as on macOS.
    pub wheel_precise: bool,
    /// The page pulled past its end (overscroll.rs), and where it's drawn.
    pub bounce: crate::overscroll::Bounce,
    pub bounce_y: f32,
    /// A sideways swipe in progress (swipe.rs).
    pub swipe: Option<crate::swipe::Swipe>,
    /// When it went to sleep, for the waking transcript.
    pub slept: Option<Instant>,
    /// When it was woken: a page back within a moment never shows the
    /// waking transcript, so it doesn't flicker.
    pub woke: Option<Instant>,
    /// The overlay transcript's highlighted command, and its rows as drawn.
    pub overlay_sel: usize,
    pub overlay_hits: Vec<(Rect, String)>,
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
    /// Content scroll, physical px.
    pub scroll: f32,
}

pub const HINTS: [(&str, &str); 5] = [
    ("K", "the palette · tabs, ports, ask, settings"),
    ("T", "new tab · the start page selected in Start/New Tab"),
    ("ENTER", "at a prompt that is a URL · opens it beside"),
    ("EDGE", "hover the left edge · tabs slide in, pin with Ctrl+Shift+S"),
    ("`", "back to the last tab · Ctrl+PgUp/PgDn walk them"),
];

pub enum Pane {
    Term(TermPane),
    Web(WebPane),
    Settings(SettingsPane),
    Hints(HintsPane),
    Editor(crate::editor::EditorPane),
    Ports(crate::ports::PortsPane),
    Downloads(crate::downloads::DownloadsPane),
    /// The prompt: nus's home, a terminal with no PTY (home.rs).
    Home(crate::home::HomePane),
}

/// Sidebar click targets besides tab rows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SideHit {
    /// The Ledger: an answer to the tab's assistant (ledger.rs).
    Answer(usize, crate::agent::Answer),
    Close(usize),
    Profile,
    NewTab,
    /// The header's primary button: the configured start page.
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
    /// The footer's folder: the sidebar's FILES page, on or off.
    Files,
    /// The tree's head: keep this folder (bind the window), or let it follow.
    FilesPin,
    /// The tree's root, up one.
    FilesUp,
    /// A row of the tree.
    FileRow(usize),
    /// A square on the rail.
    Rail(usize),
    /// The look chip in the footer: opens the hot swapper.
    Look,
    /// The tab menu's rows.
    TabRename(usize),
    TabIcon(usize),
    /// A swatch by number; 0 is none.
    TabColour(usize, usize),
    TabPin(usize),
    Pinned(crate::pins::Act),
    TabClose(usize),
    /// Tile this tab with the selection, or untile it.
    TabTile(usize),
    /// Save this tab's page into a folder (the palette picks which).
    TabFolder(usize),
    /// A folder's head (fold), an item (open), an item's × (remove).
    Folder(usize),
    FolderItem(usize, usize),
    FolderDrop(usize, usize),
    /// The caret on a node: fold or unfold its subtree.
    Fold(usize),
    /// Hot swapper rows.
    LookPreset(usize),
    LookQuick(Quick),
    LookStudio,
    RailNew,
    Closed,
    Downloads,
    MenuDrawer,
    Settings,
}

/// Quick changes to the look, from the chip: immediate, whole-window,
/// and nothing to do with the rules that colour tabs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Quick {
    /// Another signal from the swatches.
    Shuffle,
    /// Signal and stops a twelfth of a turn round the wheel.
    Rotate,
    /// Paper ↔ ink.
    Flip,
    /// The next texture.
    Texture,
    /// Back to the preset as saved.
    Reset,
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
    /// When the pointer arrived, for the tooltip's rest.
    pub since: Instant,
}

/// A tooltip offered by a visible control in the current scene build.
/// Dwell, visibility and dismissal belong to the window's tooltip controller.
#[derive(Clone, Debug)]
pub struct Tip {
    pub key: u64,
    pub anchor: Rect,
    pub text: String,
}

/// What each icon says when the pointer rests on it. Keys as `hover_key`
/// makes them; an icon without words shows nothing.
pub fn tip_for(key: u64) -> Option<&'static str> {
    let table: &[(&str, usize, &str)] = &[
        ("winctl", CrumbHit::Close as usize, "close the window"),
        ("winctl", CrumbHit::Maximize as usize, "maximize"),
        ("winctl", CrumbHit::Minimize as usize, "minimize"),
        ("sidebar", 0, "sidebar · ctrl+shift+s"),
        ("search", 0, "search · ctrl+k"),
        ("atlas", 0, "atlas · windows and spaces"),
        ("updates", 0, "updates · check for a new version"),
        ("updates", 1, "update ready · review and install"),
        ("updates", 2, "update in progress · view status"),
        ("cluster", CrumbHit::Ports as usize, "ports · ctrl+shift+p"),
        ("cluster", CrumbHit::Assistant as usize, "ask · ctrl+shift+?"),
        ("cluster", CrumbHit::Pip as usize, "picture in picture"),
        ("cluster", CrumbHit::Waiting as usize, "tabs waiting on you"),
        ("foot", 0, "new tab · ctrl+shift+t"),
        ("foot", 1, "settings · ctrl+shift+,"),
        ("foot", 2, "downloads"),
        ("foot", 3, "recently closed"),
        ("look", 0, "the look studio"),
        ("quick", 0, "shuffle the look"),
        ("quick", 1, "rotate the ramp"),
        ("quick", 2, "ink · paper"),
        ("quick", 3, "texture"),
        ("quick", 4, "reset the look"),
        ("compact-new", 0, "new tab"),
        ("compact-settings", 0, "settings"),
        ("blockchip", 0, "copy this block's output"),
        ("blockchip", 1, "run this command again"),
        ("pane", 0, "move · drag onto a tab"),
        ("pane", 1, "swap the panes"),
        ("pane", 2, "solo this pane"),
        ("pane", 3, "to its own tab"),
        ("pane", 4, "close this pane"),
        ("pane", 10, "move · drag onto a tab"),
        ("pane", 11, "swap the panes"),
        ("pane", 12, "solo this pane"),
        ("pane", 13, "to its own tab"),
        ("pane", 14, "close this pane"),
        ("hatch", 0, "land · ctrl+shift+↓"),
        ("hatch", 1, "pin"),
        ("hatch", 2, "hide · esc"),
        ("media", 0, "save the video on this page"),
        ("media", 1, "this player streams · nothing to save"),
    ];
    table.iter().find(|(n, i, _)| hover_key(n, *i) == key).map(|(_, _, w)| *w)
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
    /// Where the list begins: under the header and the pinned row. Rows
    /// scrolled above it are under those, not over them.
    pub top: f32,
    /// How tall the list is when nothing is cut: the rows, NEW TAB and the
    /// folders. More than `foot_y - top` and the wheel scrolls it.
    pub reach: f32,
}

impl SidebarGeom {
    /// The list's window, between the header and the footer.
    pub fn list(&self, sb: Rect) -> Rect {
        Rect::new(sb.x, self.top, sb.w, (self.foot_y - self.top).max(0.0))
    }
    /// How far the list can scroll.
    pub fn scroll_max(&self) -> f32 {
        (self.reach - (self.foot_y - self.top)).max(0.0)
    }
    /// A row's rect cut to the list's window, or nothing when it is out of it.
    pub fn clip(&self, sb: Rect, y: f32, h: f32) -> Option<Rect> {
        let top = y.max(self.top);
        let bottom = (y + h).min(self.foot_y);
        (bottom > top).then(|| Rect::new(sb.x, top, sb.w, bottom - top))
    }
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
    /// When this tab was last shown, for sleeping and archiving.
    pub last_active: Instant,
    /// The user's name for the tab (beats the pane's title) and icon (an
    /// emoji or any short string, drawn in place of the favicon).
    pub name: Option<String>,
    pub emoji: Option<String>,
    /// A colour the user chose; it beats the rules' and survives a theme.
    pub tint: Option<nus_render::Color>,
    /// A peek: a floating page over the tab with this id; not in the sidebar.
    pub peek: Option<u64>,
    /// Lives in the hatch (the quick terminal), not the sidebar.
    pub hatch: bool,
    /// The right pane's width in logical px, once dragged.
    pub split_w: Option<f32>,
    /// One pane alone; the other waits off screen.
    pub solo: bool,
    /// Colours the rules gave this tab.
    pub look: Overrides,
    /// A shell stack's place in the signal's family (shell_colors.rs); the
    /// color is worked out from it and the theme.
    pub shell_slot: Option<u8>,
}

impl Tab {
    pub(crate) fn waiting(&self) -> bool {
        let w = |p: &Pane| matches!(p, Pane::Term(t) if t.waiting);
        w(&self.left) || self.right.as_ref().is_some_and(w)
    }

    /// Attention, named: who is waiting for you, or which assistant is
    /// working. `(name, waiting)` — the running command's program (claude,
    /// codex, …); a plain command only counts once it waits.
    pub(crate) fn attention(&self) -> Option<(String, bool)> {
        let one = |p: &Pane| -> Option<(String, bool)> {
            let Pane::Term(t) = p else { return None };
            let cmd = t.term.marks.iter().rev().find(|m| m.kind == nus_vt::MarkKind::CommandStart).map(|b| t.term.command_text(b)).unwrap_or_default();
            let name = crate::cutoff::program(&cmd);
            if t.waiting {
                return Some((if name.is_empty() { "shell".into() } else { name }, true));
            }
            if t.running_since.is_some() && crate::cutoff::is_assistant(&name) {
                return Some((name, false));
            }
            None
        };
        one(&self.left).or_else(|| self.right.as_ref().and_then(one))
    }

    /// The shell's OSC 9;4 progress: (state, percent), the left pane's first.
    pub(crate) fn progress(&self) -> Option<(u8, u8)> {
        let p = |p: &Pane| match p {
            Pane::Term(t) => t.progress,
            _ => None,
        };
        p(&self.left).or_else(|| self.right.as_ref().and_then(p))
    }

    /// (title, detail) for a compact sidebar row: detail is cwd/host for a
    /// shell, the site for a page.
    pub(crate) fn row_text(&self) -> (String, String) {
        // The focused pane names the row; a split's other pane is the detail.
        let text = |p: &Pane| -> (String, String) {
            match p {
                Pane::Term(t) => (t.title.clone(), String::new()),
                Pane::Web(w) => {
                    let s = w.tab.shared.borrow();
                    let host = crate::sites::host_of(&s.url);
                    let title = if s.title.is_empty() { host.clone() } else { s.title.clone() };
                    (title, host)
                }
                Pane::Settings(_) => ("settings".into(), String::new()),
                Pane::Hints(_) => ("welcome".into(), String::new()),
                Pane::Home(h) if h.library => ("Reading list".into(), String::new()),
                Pane::Home(_) => ("home".into(), String::new()),
                Pane::Editor(e) => (e.title(), e.buf().and_then(|b| b.path.as_ref()).and_then(|p| p.parent()).map(|p| p.display().to_string()).unwrap_or_default()),
                Pane::Ports(_) => ("ports".into(), String::new()),
            Pane::Downloads(_) => ("downloads".into(), String::new()),
            }
        };
        let (main, other) = self.panes();
        let (title, detail) = text(main);
        let title = self.name.clone().unwrap_or(title);
        match other {
            Some(o) => (title, text(o).0),
            None => (title, detail),
        }
    }

    pub(crate) fn focused(&mut self) -> &mut Pane {
        if self.focus_right && self.right.is_some() {
            self.right.as_mut().unwrap()
        } else {
            &mut self.left
        }
    }

    pub(crate) fn focused_ref(&self) -> &Pane {
        if self.focus_right && self.right.is_some() {
            self.right.as_ref().unwrap()
        } else {
            &self.left
        }
    }

    /// The focused pane, and the other one when the tab is split.
    pub(crate) fn panes(&self) -> (&Pane, Option<&Pane>) {
        match (&self.right, self.focus_right) {
            (Some(r), true) => (r, Some(&self.left)),
            (Some(r), false) => (&self.left, Some(r)),
            (None, _) => (&self.left, None),
        }
    }
    pub(crate) fn title(&self) -> String {
        if let Some(n) = &self.name {
            return n.clone();
        }
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
            Pane::Home(h) if h.library => "Reading list".into(),
            Pane::Home(_) => "home".into(),
            Pane::Editor(e) => e.title(),
            Pane::Ports(_) => "ports".into(),
            Pane::Downloads(_) => "downloads".into(),
        };
        let (main, other) = self.panes();
        match other {
            Some(o) => format!("{} | {}", name(main), name(o)),
            None => name(main),
        }
    }
}

pub struct App {
    pub library: crate::library::Library,
    pub window: Arc<Window>,
    pub traffic_lights: crate::macos::TrafficLights,
    pub gpu: Gpu,
    pub target: nus_render::Target,
    pub fonts: FontSystem,
    pub assistants: crate::assistants::State,
    pub system_font_cache: Vec<(String,u16,FontId)>,
    pub prompt_cache: std::cell::RefCell<Option<(Instant, String, Vec<PaletteRow>)>>,
    pub font_cache: Vec<(crate::fonts::Family,crate::fonts::Weight,FontId)>,
    pub f: Fonts,
    pub scene: Scene,
    pub theme: Theme,
    pub scale: f32,
    pub ui_zoom: u32,
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
    /// How far the sidebar's list is scrolled (physical px), when the
    /// rows and folders outgrow the space between the header and the footer.
    pub sidebar_scroll: f32,
    pub pins: crate::pins::Pins,
    /// Trips in flight for the surfaces that scroll in pixels (scrolling.rs).
    pub glides: std::collections::HashMap<crate::scrolling::Glider, crate::scrolling::Trip>,
    /// The tab whose panes are being drawn, while the tab list is out of `App`.
    pub drawing_tab: u64,
    pub user_name: String,
    /// What the window is made of (see surface.rs) and the rules that colour
    /// new tabs; both editable from the settings tab.
    pub surface: Surface,
    pub sidebar_rules: SidebarRules,
    pub rules: Rules,
    pub settings_focus: Option<usize>,
    pub settings_hits: Vec<(Rect, crate::settings::Hit)>,
    /// AccessKit node id → what activating it does (rebuilt per frame).
    pub access_map: std::collections::HashMap<u64, crate::access::Target>,
    /// Palette row rects from the last frame (clicks, AccessKit).
    pub palette_hits: Vec<Rect>,
    pub intel: crate::intelligence::Dial,
    pub page_dialog: crate::page_dialog::State,
    pub palette_scroll: f32,
    pub palette_scroll_max: f32,
    pub palette_reveal: bool,
    /// Little nus: the floating window for links from outside.
    pub little: Option<crate::little::Little>,
    pub little_request: Option<String>,
    /// The hatch (quick terminal) window, and the ask to make one.
    pub hatch: Option<crate::hatch::Hatch>,
    pub hatch_state: crate::hatch::State,
    pub menu_drawer: crate::menu_drawer::State,
    /// Ask the host for the hatch window: where and how big (physical px).
    pub hatch_request: Option<((i32, i32), (u32, u32))>,
    pub hotkey: Option<crate::hotkey::Hotkey>,
    /// HATCH · HOTKEY · RECORD: the next chord pressed becomes the hotkey.
    pub hotkey_recording: bool,
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
    /// Creation order among this process's windows.
    pub ordinal: usize,
    /// Every window in the process, kept fresh by the host.
    pub windows: Arc<Vec<crate::windows::Entry>>,
    /// Asks the host answers: front that window; open a new one.
    pub front_request: Option<u64>,
    pub new_window_request: bool,
    /// A window that came up as the prompt and has not been given a folder
    /// or a tab yet: its rows offer folders.
    pub fresh: bool,
    /// The folder the asking window worked in, for a shell born here.
    pub born_in: Option<String>,
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
    /// Tabs whose subtree is folded, by id; and a row being dragged:
    /// (tab index, grab offset, current y).
    pub collapsed: std::collections::HashSet<u64>,
    /// A right-clicked row's menu: (tab index, y), and its rise.
    pub tab_menu: Option<(usize, f32)>,
    pub tab_menu_anim: Anim,
    pub tab_menu_last: Option<(usize, f32)>,
    pub drag: Option<(usize, f32, f32)>,
    pub drag_armed: Option<(usize, f32, f32)>,
    /// Two to four tabs sharing the content, and a divider being dragged.
    pub tiling: Option<crate::tiles::Tiling>,
    /// Every layout change goes through here, and can be taken back.
    pub director: crate::director::Director,
    /// What a divider drag started from, recorded for undo when it ends.
    pub drag_from: Option<crate::director::Op>,
    /// Pane mode (Ctrl+Alt+P): single keys drive the director.
    pub pane_mode: bool,
    /// A tiling set aside by zoom, to come back on the next zoom.
    pub pane_zoomed: Option<crate::tiles::Tiling>,
    /// A sidebar row press: the tab that was active before it, and where
    /// the press was across, so a drag can show that tab to drop beside.
    pub row_host: Option<(usize, f32)>,
    /// A tab (by id) this window gives up, and where to; the host moves it.
    pub send_request: Option<(u64, crate::send::Dest)>,
    pub tile_drag: Option<crate::tiles::Grab>,
    /// The pointer is a resize arrow over a divider.
    pub resize_cursor: Option<crate::tiles::Divider>,
    pub peek_anim: Anim,
    /// The compact column's hovered row, for its tooltip after the panes.
    pub compact_tip: Option<(usize, f32)>,
    /// Split pane controls (this frame), a pane in hand, the rule being dragged.
    pub pane_hits: Vec<(Rect, crate::panes::PaneHit)>,
    pub pane_drag: Option<(usize, bool, f32, f32)>,
    pub split_drag: bool,
    pub sidebar_resize: Option<crate::sidebar::Resize>,
    pub settings_drag: Option<crate::settings::Hit>,
    /// This window's container (new pages open in it), and the list.
    pub container: String,
    pub containers: Vec<crate::containers::Container>,
    /// Focus mode: no strip, no sidebar, no rows — the panes alone.
    pub focus: bool,
    pub focus_hint: Option<Instant>,
    /// Optional tools being fetched, and how they ended.
    pub jobs: crate::bundles::Jobs,
    /// Folders under the tabs: plain ones (saved pages) and live ones.
    pub folders: Vec<crate::folders::Folder>,
    /// Language servers, and the editor's FILES folder root and open dirs.
    pub lsp: crate::lsp_host::Servers,
    /// The ports board (airport control).
    pub board: crate::ports::Board,
    pub files_root: Option<std::path::PathBuf>,
    pub files_listing: Option<crate::work::Task<(std::path::PathBuf, std::collections::HashSet<std::path::PathBuf>, Vec<crate::folders::Item>)>>,
    pub files_open: std::collections::HashSet<std::path::PathBuf>,
    /// Editor click counting: when, where, how many.
    pub click_at: Option<(Instant, (f32, f32), u32)>,
    pub next_folder_id: u64,
    pub live: crate::folders::Live,
    /// A right-click asked for a paste; answered in tick.
    pub paste_request: bool,
    /// Downloads list in the footer.
    pub dl_menu: bool,
    pub download_ui: crate::downloads::Ui,
    pub dl_anim: Anim,
    pub last_tend: Instant,
    /// When the window was last resized, for the cols × rows overlay.
    pub resized_at: Option<Instant>,
    pub hovers: std::collections::HashMap<u64, Hover>,
    pub memory_tended: Instant,
    /// The icon under the pointer this frame, with its words; drawn last.
    pub tip: Option<Tip>,
    pub tooltips: tooltip_state::State,
    // The last icon's logical and clipped hit rects, for tip_words callers.
    pub tip_icon: Option<(u64, Rect, Rect)>,
    pub page_menu_buttons: std::collections::HashSet<MouseButton>,
    pub page_menu_keys: std::collections::HashSet<PhysicalKey>,
    /// The automatic window name and what it was computed from.
    pub auto_name: std::cell::RefCell<Option<String>>,
    pub auto_name_key: Option<(Option<String>, Vec<String>)>,
    /// What the taskbar button last showed, so it's only told on change.
    pub taskbar_shown: Option<(u8, u8)>,
    /// Block pages we wrote, by their file URL, for copy-as-markdown and gist.
    pub block_pages: std::collections::HashMap<String, crate::blockpage::BlockPage>,
    pub look_tab: usize,
    /// The theme's tab-colour rule, read by rules.luau as ctx.tab_colours.
    pub tab_colours: String,
    pub look_menu: bool,
    pub look_anim: Anim,
    /// The callout's rect while shown, and when the pointer left it.
    pub look_rect: Option<Rect>,
    pub look_scroll: f32,
    pub look_scroll_max: f32,
    pub look_leave: Option<Instant>,
    pub tok_sel: crate::settings::TokSel,
    /// Last keystroke into a shell, for blink-after-idle and pointer hiding.
    pub last_key: Instant,
    pub pointer_hidden: bool,
    pub pointer_reset: bool,
    pub blink_half: u64,
    /// A slow clock the last frame asked for, in ms (a breathing mark, a
    /// ticking count): App::tick redraws when it comes round, instead of
    /// the drawing asking for every frame. And the slot it last fired.
    pub beat_want: Option<u64>,
    pub beat_slot: u64,
    /// The launch sequence's "then" has run.
    pub then_done: bool,
    pub window_rect: Option<(i32, i32, u32, u32)>,
    /// A row was picked in a persistent Atlas, so Esc may close it now.
    pub atlas_used: bool,
    /// Save the rect a beat after the last move, not on every pixel.
    pub window_rect_dirty: Option<Instant>,
    pub prefs_baseline: std::cell::RefCell<serde_json::Value>,
    pub prefs_revision: std::cell::Cell<u64>,
    /// Surface page state: which preset is on, which stop is selected.
    pub preset_name: String,
    pub stop_sel: usize,
    /// How tall the settings column's content was last frame.
    pub settings_reach: f32,
    pub settings_target: Option<(usize,usize,usize)>,
    pub settings_highlight: Option<(usize,usize,usize,Instant)>,
    pub started: Instant,
    pub sound: crate::sound::Sound,
    /// The Start modal, the session it can restore, and recent places.
    pub start: Option<crate::start::Start>,
    /// You, on this machine (profile/me.json), and the card that shows it.
    pub me: Option<crate::me::Me>,
    pub me_card: crate::me::MeCard,
    /// The name typed during the walk, before the file exists.
    pub pending_name: String,
    pub splash: Option<crate::splash::Splash>,
    /// The plate's icon, sampled once at its size (plate.rs).
    pub plate: Option<crate::plate::PlateArt>,
    /// One slip of words at the foot of the content (toast.rs).
    pub toast: Option<crate::toast::Toast>,
    /// The page's right-click menu, while up (page_menu.rs).
    pub page_menu: Option<crate::page_menu::PageMenu>,
    /// The art behind the prompt, running (art.rs); the picker's cards, alive.
    pub art: Option<crate::art::Art>,
    pub art_previews: std::collections::HashMap<String, crate::art::Art>,
    /// The app icon for settings' picture cards, by (size, band progress in
    /// hundredths, ink, signal): the plate's own texture holds one at a time.
    pub pic_icons: std::collections::HashMap<(u32, u16, [u8; 4], [u8; 4]), Arc<wgpu::BindGroup>>,
    /// The machine's processes, sampled once an art asks (procs.rs).
    pub procs: Option<crate::procs::Shared>,
    pub toast_anim: Anim,
    /// A toast that arrived while a problem was up, waiting for it to go.
    pub toast_held: Option<crate::toast::Toast>,
    /// A page has the whole screen: which tab, and how the window, focus
    /// mode and the tab's split were before, to put back after.
    pub page_fullscreen: Option<(u64, bool, bool, bool)>,
    /// The nus button, held down over home (home.rs).
    pub home_latch: Option<crate::home::Latch>,
    /// NUS_SHOT: the app photographing itself (a test hook, see shot.rs).
    pub shot: Option<crate::shot::Shot>,
    /// Remote answers waiting on the page (see remote.rs).
    pub deferred: Vec<crate::remote::Deferred>,
    /// Replay: the session's recorder, checkpoints waiting on a draw, the timeline.
    /// The loop's probe out on a page: (tab, right?, cdp id, since).
    pub loop_probe: Option<(usize, bool, i32, Instant)>,
    /// Sessions for other windows, to be spawned by the host (restore).
    pub spawn_sessions: Vec<crate::start::Session>,
    /// A window restored from a session: its birth shell goes once it is at a prompt.
    pub drop_birth: bool,
    /// The other windows' sessions, gathered at quit for the file.
    pub other_sessions: Vec<crate::start::Session>,
    pub recorder: Option<crate::replay::Recorder>,
    pub checkpoints: Vec<crate::replay::Pending>,
    pub timeline: Option<crate::replay::Timeline>,
    pub start_shown: bool,
    /// NUS_TYPE: text typed into the first shell once it has a prompt (a test hook).
    pub typed_once: bool,
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
    /// A pin on its way in: it slides over the content as the hover sidebar
    /// does, and the layout makes room once, when it lands, rather than
    /// resizing every page on every frame.
    pub sidebar_arriving: bool,
    /// The surface tint the theme was last made legible on (base, amount).
    pub legible_on: (Option<nus_render::Color>, f32),
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
    /// The sliding sidebar and its shadow, this frame (for WebKit's pages, which sit above nus's drawing).
    pub sidebar_over: Option<Rect>,
    pub shell_phase: f32,
    pub pip: Option<crate::pip::Pip>,
    /// Picture in picture pinned over a shell instead (pip_dock.rs).
    pub docked: Option<crate::pip_dock::Docked>,
    pub pip_request: Option<(usize, bool)>,
    pub viewer_config_seen: Option<crate::file_viewer::Config>,
    pub pip_away_pending: Option<Instant>,
    pub pip_was_minimized: bool,
    pub update_revision: u64,
    /// Deferred DevTools open (tab, right pane), created from the main loop.
    pub devtools_request: Option<(usize, bool)>,
    pub crumb_hits: Vec<(Rect, CrumbHit)>,
    pub side_hits: Vec<(Rect, SideHit)>,
    /// A finger on the screen: long-press is hover, a tap is a click (touch.rs).
    pub touch: crate::touch::Touch,
    /// While you were away (news.rs).
    pub news: crate::news::News,
    /// The instance port's request channel, for the phone's page.
    pub inbound: Option<std::sync::mpsc::Sender<crate::little::Inbound>>,
    /// Which page the sidebar shows, and the tree behind FILES.
    pub side_page: crate::files::SidePage,
    pub tree: crate::files::Tree,
    /// The folder this window is bound to, when it is: its name, its
    /// tree, where new shells are born.
    pub workspace: Option<std::path::PathBuf>,
    /// profile/avatar.png as a texture, when there is one.
    pub avatar: Option<Arc<wgpu::BindGroup>>,
    /// A file dialog is up, waiting on a picture for the face.
    pub avatar_pick: Option<crate::pick::Pending>,
    pub next_id: u64,
    /// Onboarding ticks (see HINTS); the panel leaves once all five are set.
    pub hints: [bool; 5],
    pub hint_hits: Vec<(Rect, usize)>,
    /// The welcome page's controls, the icon texture, a demo waiting.
    pub welcome_hits: Vec<(Rect, crate::welcome::Act)>,
    pub icon_previews: Vec<crate::app_icon::Preview>,
    pub welcome_icon_tex: Option<((nus_render::Mode, nus_render::Color), Arc<wgpu::BindGroup>)>,
    pub welcome_art: Option<(Instant, crate::art::Art)>,
    pub welcome_shapes: Vec<Rect>,
    pub welcome_taps: Vec<(f32,f32)>,
    pub welcome_anim_until: Option<Instant>,
    pub previous_install: Option<std::path::PathBuf>,
    pub welcome_pending: Option<(crate::welcome::Act, Instant)>,
    pub welcome_reach: f32,
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
    /// A layout file found in a shell's cwd, offered as a chip; folders already offered.
    pub layout_offer: Option<(std::path::PathBuf, Instant)>,
    pub layout_offer_hit: Option<Rect>,
    pub layout_offered: std::collections::HashSet<std::path::PathBuf>,
    /// Sync: the worker and the last report.
    pub sync: crate::syncui::SyncState,
    /// Tab tidy: the sheet and its proposals.
    pub tidy: crate::tidy::Tidy,
    /// Dedupe bands dismissed with KEEP BOTH, by tab id.
    pub dedupe_kept: std::collections::HashSet<u64>,
    /// Skills from rules.luau: saved prompts with their own context.
    pub skills: Vec<crate::askctx::Skill>,
    pub skills_src_len: usize,
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
    pub fn new(window: Arc<Window>, proxy: EventLoopProxy<UserEvent>, secondary: bool, ordinal: usize, born_in: Option<String>) -> anyhow::Result<App> {
        let traffic_lights = crate::macos::TrafficLights::new(&window, &proxy);
        crate::webkit::set_host(&window);
        let (gpu, target) = Gpu::new(window.clone())?;
        let scale = window.scale_factor() as f32;
        let mut fonts = FontSystem::new();
        let ui = fonts.load_bytes(nus_render::text::bundled::PLEX_MONO, 0)?;
        let strong = fonts.load_bytes(nus_render::text::bundled::PLEX_MONO_SEMIBOLD, 0)?;
        let wordmark = fonts.load_bytes(nus_render::text::bundled::NEWSREADER_ITALIC, 0)?;
        let serif = fonts.load_bytes(nus_render::text::bundled::NEWSREADER, 0)?;
        let term_font = ui;
        // NUS_MODE=paper|ink fixes the face regardless of the OS (a test hook,
        // so screenshots can be taken in both without flipping Windows).
        let theme = match std::env::var("NUS_MODE").ok().as_deref() {
            Some("paper") => Theme::paper(),
            Some("ink") => Theme::ink(),
            _ => match window.theme() {
                Some(winit::window::Theme::Light) => Theme::paper(),
                _ => Theme::ink(),
            },
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
            traffic_lights,
            gpu,
            target,
            fonts,
            assistants: Default::default(),
            system_font_cache: Vec::new(),
            prompt_cache: Default::default(),
            font_cache: Vec::new(),
            f: Fonts {
                ui,
                strong,
                wordmark,
                term: term_font,
                editor: term_font,
                serif,
            },
            scene: Scene::new(),
            theme,
            scale,
            ui_zoom: 100,
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
            sidebar_scroll: 0.0,
            pins: Default::default(),
            glides: Default::default(),
            drawing_tab: 0,
            surface: Surface::default(),
            sidebar_rules: SidebarRules::default(),
            rules: Rules::load(),
            settings_focus: None,
            settings_hits: Vec::new(),
            access_map: std::collections::HashMap::new(),
            palette_hits: Vec::new(),
            intel: Default::default(),
            page_dialog: Default::default(),
            palette_scroll: 0.0,
            palette_scroll_max: 0.0,
            palette_reveal: true,
            little: None,
            little_request: None,
            hatch: None,
            hatch_state: crate::hatch::State::default(),
            menu_drawer: crate::menu_drawer::State::default(),
            hatch_request: None,
            hotkey: None,
            hotkey_recording: false,
            little_pos: (0.0, 0.0),
            urls_rx: None,
            register_note: String::new(),
            login_note: String::new(),
            theme_edit: crate::theme_edit::ThemeEdit::default(),
            ansi_sel: 1,
            cursor: crate::settings::CursorPrefs::default(),
            header: crate::settings::HeaderPrefs::default(),
            window_named: None,
            ordinal: 0,
            front_request: None,
            new_window_request: false,
            fresh: false,
            born_in: None,
            windows: Arc::new(Vec::new()),
            win_menu: false,
            win_anim: Anim::at(0.0),
            kinds_menu: false,
            kinds_anim: Anim::at(0.0),
            press: None,
            flash_anim: Anim::at(0.0),
            rail_anim: Anim::at(0.0),
            next_row_hot: false,
            registered_tabs: usize::MAX,
            collapsed: std::collections::HashSet::new(),
            tab_menu: None,
            tab_menu_anim: Anim::at(0.0),
            tab_menu_last: None,
            tiling: None,
            director: Default::default(),
            drag_from: None,
            pane_mode: false,
            pane_zoomed: None,
            row_host: None,
            send_request: None,
            tile_drag: None,
            resize_cursor: None,
            peek_anim: Anim::at(0.0),
            compact_tip: None,
            pane_hits: Vec::new(),
            pane_drag: None,
            split_drag: false,
            sidebar_resize:None,
            settings_drag: None,
            container: crate::containers::PERSONAL.to_string(),
            containers: crate::containers::load(),
            focus: false,
            focus_hint: None,
            jobs: crate::bundles::Jobs::new(),
            folders: Vec::new(),
            lsp: Default::default(),
            board: crate::ports::Board::new(),
            files_root: None,
            files_listing: None,
            files_open: Default::default(),
            click_at: None,
            next_folder_id: 100,
            live: crate::folders::start(),
            drag: None,
            drag_armed: None,
            paste_request: false,
            dl_menu: false,
            download_ui: Default::default(),
            dl_anim: Anim::at(0.0),
            last_tend: crate::clock::now(),
            resized_at: None,
            hovers: std::collections::HashMap::new(),
            memory_tended: crate::clock::now(),
            tip: None,
            tooltips: Default::default(),
            tip_icon: None,
            page_menu_buttons: Default::default(),
            page_menu_keys: Default::default(),
            auto_name: std::cell::RefCell::new(None),
            auto_name_key: None,
            taskbar_shown: None,
            block_pages: std::collections::HashMap::new(),
            look_tab: 0,
            tab_colours: "family".into(),
            look_menu: false,
            look_anim: Anim::at(0.0),
            look_rect: None,
            look_scroll: 0.0,
            look_scroll_max: 0.0,
            look_leave: None,
            tok_sel: crate::settings::TokSel::Signal,
            last_key: crate::clock::now(),
            pointer_hidden: false,
            pointer_reset: false,
            blink_half: 0,
            beat_want: None,
            beat_slot: 0,
            then_done: false,
            window_rect: None,
            atlas_used: false,
            window_rect_dirty: None,
            prefs_baseline: std::cell::RefCell::new(serde_json::Value::Null),
            prefs_revision: std::cell::Cell::new(0),
            preset_name: "broadsheet".into(),
            stop_sel: 0,
            settings_reach: 0.0,
            settings_target: None,
            settings_highlight: None,
            started: crate::clock::now(),
            sound: crate::sound::Sound::new(crate::sound::SoundPrefs::default()),
            start: None,
            me: crate::me::Me::load(),
            me_card: crate::me::MeCard::default(),
            pending_name: String::new(),
            splash: Some(crate::splash::Splash::new()),
            library: crate::library::Library::default(),
            plate: None,
            toast: None,
            toast_anim: Anim::at(0.0),
            toast_held: None,
            page_fullscreen: None,
            home_latch: None,
            page_menu: None,
            art: None,
            art_previews: std::collections::HashMap::new(),
            pic_icons: std::collections::HashMap::new(),
            procs: None,
            shot: if secondary { crate::shot::Shot::from_env_secondary() } else { crate::shot::Shot::from_env() },
            deferred: Vec::new(),
            loop_probe: None,
            spawn_sessions: Vec::new(),
            drop_birth: false,
            other_sessions: Vec::new(),
            recorder: None,
            checkpoints: Vec::new(),
            timeline: None,
            start_shown: false,
            last_session: crate::start::Session::load(),
            recent: crate::start::load_recent(),
            session_sig: String::new(),
            behavior: crate::settings::Behavior::default(),
            fullscreen: false,
            pointer_inside: false,
            motion: Motion::default(),
            load_bar: LoadBar::default(),
            sidebar_anim: Anim::at_on(0.0, crate::anim::Curve::Glide),
            sidebar_arriving: false,
            legible_on: (None, 0.0),
            palette_anim: Anim::at(1.0),
            band_anim: Anim::at(1.0),
            tint_anim: Anim::at(0.0),
            crumb_anim: Anim::at(1.0),
            row_anims: std::collections::HashMap::new(),
            sidebar_shift: 0.0,
            sidebar_over: None,
            shell_phase: 0.0,
            pip: None,
            docked: None,
            pip_request: None,
            viewer_config_seen: None,
            pip_away_pending: None,
            pip_was_minimized: false,
            update_revision: 0,
            devtools_request: None,
            crumb_hits: Vec::new(),
            side_hits: Vec::new(),
            touch: Default::default(),
            news: Default::default(),
            inbound: None,
            side_page: Default::default(),
            tree: Default::default(),
            workspace: None,
            avatar: None,
            avatar_pick: None,
            next_id: 1,
            hints: App::load_hints(),
            hint_hits: Vec::new(),
            welcome_hits: Vec::new(),
            icon_previews: Vec::new(),
            welcome_icon_tex: None,
            welcome_art: None,
            welcome_shapes: Vec::new(), welcome_taps: Vec::new(), welcome_anim_until: None,
            previous_install: crate::install::previous(),
            welcome_pending: None,
            welcome_reach: 0.0,
            confirm_stack: None,
            window_focused: true,
            user_name: std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_else(|_| "you".into()),
            palette: None,
            palette_sel: 0,
            profiles: nus_pty::Profile::discover(),
            typed_once: false,
            mru: vec![0],
            selected: Default::default(),
            closed: Vec::new(),
            llm_tools: discover_llm_tools(),
            sync: Default::default(),
            tidy: Default::default(),
            dedupe_kept: Default::default(),
            layout_offer: None,
            layout_offer_hit: None,
            layout_offered: std::collections::HashSet::new(),
            skills: Vec::new(),
            skills_src_len: usize::MAX,
            ports: Vec::new(),
            mods: ModifiersState::empty(),
            mouse: (0.0, 0.0),
            mouse_down_in_web: false,
            detected: None,
            dirty: true,
            resize_due: None,
            last_begin_frame: crate::clock::now(),
            frames: 0,
        };
        app.ordinal = ordinal;
        let prefs = crate::prefs::Prefs::load();
        app.behavior = prefs.behavior.clone().unwrap_or_default();
        // A second window: one shell, no splash, no session restore, no name.
        let onboarded = App::onboarded();
        // Profiles welcomed before the ledger existed count for their version.
        if onboarded && !secondary { crate::install::mark_version_welcomed(); }
        if let Some(sp)=app.splash.as_mut() {sp.arrival=!secondary && !onboarded && !std::path::Path::new("profile/arrival-seen").is_file();}
        // Do not start a held shell just to replace it with a page. Those
        // hidden shells survive the app and used to accumulate on every launch.
        let needs_shell = if secondary {
            app.behavior.new_window == crate::settings::NewWindow::Shell ||
            (app.behavior.new_window == crate::settings::NewWindow::Launch && app.behavior.then == crate::settings::Then::Shell)
        } else { app.behavior.then == crate::settings::Then::Shell };
        let needs_shell = needs_shell && (secondary || !crate::compatibility::recovery_launch());
        // NUS_SHELL=<profile name> picks the first shell (a test hook).
        let first = std::env::var("NUS_SHELL").ok().and_then(|n| app.profiles.iter().position(|p| p.name.eq_ignore_ascii_case(&n))).unwrap_or(app.behavior.default_profile.min(app.profiles.len().saturating_sub(1)));
        // A second window's shell is born in the asking window's folder.
        // A fresh install walks through the profile first (me.rs), in the
        // big card the arrival draws; Welcome comes when that is done. A
        // profile that finished the walk before only needs Welcome.
        let first_walk = !secondary && !onboarded && (app.me.is_none() || !crate::me::agreed());
        if !secondary && !onboarded && !first_walk {
            let welcome = app.make_tab(Pane::Hints(HintsPane { rect: Rect::new(0.0, 0.0, 1.0, 1.0), scroll: 0.0 }), None);
            app.tabs.push(welcome);
            app.then_done = true;
            app.start_shown = true;
            // Welcome is shown once. Quitting without Get started must not
            // bring it back in place of the start page and the kept session;
            // it stays one click away in Settings.
            app.save_hints();
            crate::install::complete();
        } else if first_walk {
            let home = app.make_tab(Pane::Home(crate::home::HomePane::new()), None);
            app.tabs.push(home);
            app.then_done = true;
            app.start_shown = true;
        } else {
            let pane = if needs_shell {
                Pane::Term(app.new_term_pane_at(false, first, if secondary { born_in.clone() } else { None })?)
            } else { Pane::Home(crate::home::HomePane::new()) };
            let first = app.make_tab(pane, None);
            app.tabs.push(first);
        }
        app.apply_prefs(prefs);
        // The primary fresh/incomplete installation enters the existing profile
        // flow. The arrival is drawn above it; secondary windows never repeat it.
        if first_walk {
            app.open_first_walk();
        }
        // The hatch's global hotkey: the first window registers it; a
        // second Space shares it (main routes the event to the focused one).
        if !secondary && !crate::private::enabled() {
            app.news_at_launch();
            if app.hotkey.is_none() { app.hotkey = Some(crate::hotkey::Hotkey::register(app.behavior.hatch_hotkey, app.proxy.clone())); }
            if let Some(k) = &app.hotkey {
                if !k.status.is_empty() {
                    tracing::warn!("hatch hotkey: {}", k.status);
                }
            }
        }
        if secondary {
            // A window of its own: no session restore, no name yet; what
            // it is born as is STARTUP · A NEW WINDOW.
            app.window_named = None;
            match app.behavior.new_window {
                crate::settings::NewWindow::Launch => {
                    // As the first window did: the splash, then THEN — but never
                    // the session, which is the first window's.
                    app.then_done = false;
                    if app.behavior.then == crate::settings::Then::Restore {
                        // This window keeps its prompt placeholder. Do not
                        // rewrite the shared launch preference to fall back.
                        app.then_done = true;
                    }
                }
                crate::settings::NewWindow::Prompt => {
                    app.splash = None;
                    app.start_shown = true;
                    app.then_done = true;
                    app.replace_birth(Pane::Home(crate::home::HomePane::new()));
                    app.fresh = true;
                }
                crate::settings::NewWindow::Shell => {
                    app.splash = None;
                    app.start_shown = true;
                    app.then_done = true;
                }
            }
        }
        let mode = app.theme.mode;
        app.set_mode(mode);
        app.layout();
        app.apply_term_resizes(true);
        app.load_folders();
        app.refresh_icon();
        app.load_avatar();
        if let Some(days) = app.behavior.replay.days() {
            app.recorder = crate::replay::Recorder::new(days);
        }
        app.user_name = app.me_name();
        if app.behavior.startup_sound {
            app.play_event("launch");
        }
        app.prepare_arrival();
        Ok(app)
    }

    pub(crate) fn px(&self, v: f32) -> f32 {
        (v * self.scale).round()
    }

    /// Where the holders' files live.
    pub(crate) fn hold_dir() -> std::path::PathBuf {
        std::env::current_dir().unwrap_or_default().join("profile").join("hold")
    }

    /// Holders alive right now that no tab of ours is attached to.
    pub(crate) fn held_loose(&self) -> Vec<nus_pty::hold::Info> {
        let dir = Self::hold_dir();
        let attached: std::collections::HashSet<String> = self
            .tabs
            .iter()
            .flat_map(|t| std::iter::once(&t.left).chain(t.right.as_ref()))
            .filter_map(|p| match p {
                Pane::Term(t) => t.pty.held_id().map(String::from),
                _ => None,
            })
            .collect();
        nus_pty::hold::Info::all(&dir).into_iter().filter(|i| !attached.contains(&i.id)).filter(|i| i.alive(&dir)).collect()
    }

    /// Back to a held shell: a pane over the holder's socket. The ring
    /// replays through the VT core, so the screen is what it would have been.
    pub(crate) fn new_term_pane_attached(&mut self, split: bool, info: nus_pty::hold::Info) -> anyhow::Result<TermPane> {
        anyhow::ensure!(!crate::private::enabled(), "Shells are unavailable in incognito windows");
        let profile_index = self.profiles.iter().position(|p| p.program.eq_ignore_ascii_case(&info.program)).unwrap_or(self.behavior.default_profile);
        let mut pane = self.new_term_pane_prepared(split, profile_index)?;
        let proxy = self.proxy.clone();
        let (cols, rows) = (pane.term.cols() as u16, pane.term.rows() as u16);
        pane.pty = nus_pty::Pty::attach(info, cols, rows, move || {
            let _ = proxy.send_event(UserEvent::Wake);
        })?;
        Ok(pane)
    }

    /// A pane like `new_term_pane` makes, but whose pty is a throwaway local
    /// shell to be replaced — attach swaps in the holder's. (The local one
    /// is killed on the swap; it never gets a prompt in.)
    fn new_term_pane_prepared(&mut self, split: bool, profile: usize) -> anyhow::Result<TermPane> {
        let keep = self.behavior.keep_alive;
        self.behavior.keep_alive = crate::settings::KeepAlive::Off;
        let r = self.new_term_pane_at(split, profile, None);
        self.behavior.keep_alive = keep;
        r
    }

    /// Spawn a shell sized for the left pane (split or not), so ConPTY never
    /// sees a resize during startup.
    pub(crate) fn new_term_pane(&mut self, split: bool, profile: usize) -> anyhow::Result<TermPane> {
        self.new_term_pane_at(split, profile, None)
    }

    /// The same, starting in `cwd` when one is given (session restore).
    pub(crate) fn new_term_pane_at(&mut self, split: bool, profile: usize, cwd: Option<String>) -> anyhow::Result<TermPane> {
        anyhow::ensure!(!crate::private::enabled(), "Shells are unavailable in incognito windows");
        let term_px = self.terminal_px();
        let mut grid = GridRenderer::new(&self.fonts, self.f.term, term_px);
        grid.set_spacing(&self.fonts,self.behavior.typography.terminal_line,self.behavior.typography.terminal_spacing*self.scale);
        let c = self.content_rect();
        let w = if split { c.w - self.px(m::SPLIT) - self.px(m::STRUCTURE) } else { c.w };
        let area = Rect::new(
            c.x + self.px(18.0),
            c.y + self.header_h() + self.px(16.0),
            w - 2.0 * self.px(18.0),
            c.h - self.header_h() - 2.0 * self.px(16.0),
        );
        let (cols, rows) = grid.grid_size(area);
        let mut term = Term::new(cols, rows, (self.behavior.scrollback as usize).clamp(200, 1_000_000));
        self.theme.apply(&mut term.palette);
        let (cw, ch) = grid.cell_size();
        term.cell_px = (cw as u16, ch as u16);
        let profile_index = profile;
        let mut profile = self.profiles.get(profile).cloned().unwrap_or_else(nus_pty::Profile::default_shell);
        if let Some(c) = cwd.filter(|c| std::path::Path::new(c).is_dir()) {
            profile.cwd = Some(c);
        }
        // New shells open where the focused one is — within the window's
        // folder when it has one, else at that folder.
        if profile.cwd.is_none() {
            let focused = self.focused_cwd();
            profile.cwd = match (&self.workspace, focused) {
                (Some(w), Some(c)) if std::path::Path::new(&c).starts_with(w) => Some(c),
                (Some(w), _) => Some(w.to_string_lossy().to_string()),
                (None, c) => c,
            };
        }
        let is_ssh = profile.program.rsplit(['/', '\\']).next().unwrap_or("").trim_end_matches(".exe") == "ssh";
        let ssh_host = if is_ssh { crate::ssh::host_of(&profile.args) } else { None };
        let opened_in = profile.cwd.clone();
        let on = if is_ssh { self.behavior.shell_integration && self.behavior.ssh_integration } else { self.behavior.shell_integration };
        let mut profile = crate::shell::integrate(profile, on);
        // Who this shell is, for whatever runs in it: `nus hook` finds its
        // way home with these (agent.rs). Not over ssh — they mean nothing there.
        let pane_uid = crate::agent::mint_pane();
        if !is_ssh {
            profile.env.push(("NUS_PANE".into(), pane_uid.clone()));
            profile.env.push(("TERM_PROGRAM".into(), "nus".into()));
            if let Ok(dir) = std::env::current_dir() {
                profile.env.push(("NUS_INSTANCE".into(), dir.join("profile").join("instance").to_string_lossy().into_owned()));
            }
            if let Some(cli) = crate::assistants::nus_cli() {
                profile.env.push(("NUS_CLI".into(), cli.to_string_lossy().into_owned()));
            }
        }
        let proxy = self.proxy.clone();
        // Held when the setting says so and the holder is there; else ours.
        let held = self.behavior.keep_alive == crate::settings::KeepAlive::On && nus_pty::hold::holder_exe().is_some();
        let pty = if held {
            let p2 = proxy.clone();
            match nus_pty::Pty::spawn_held(&profile, cols as u16, rows as u16, &Self::hold_dir(), move || {
                let _ = p2.send_event(UserEvent::Wake);
            }) {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!("hold: {e:#}; running the shell locally");
                    nus_pty::Pty::spawn(&profile, cols as u16, rows as u16, move || {
                        let _ = proxy.send_event(UserEvent::Wake);
                    })?
                }
            }
        } else {
            nus_pty::Pty::spawn(&profile, cols as u16, rows as u16, move || {
                let _ = proxy.send_event(UserEvent::Wake);
            })?
        };
        Ok(TermPane {
            zoom: 100,
            term,
            pty,
            grid,
            title: profile.name.clone(),
            profile: profile_index,
            profile_name: profile.name.clone(),
            rect: Rect::new(0.0, 0.0, 1.0, 1.0),
            origin: (0.0, 0.0),
            show_header: split,
            cur_x: Anim::at(0.0),
            cur_y: Anim::at(0.0),
            trail: Vec::new(),
            smear: crate::smear::Smear::new(),
            trip: None,
            line_col: None,
            cwd: None,
            progress: None,
            running_since: None,
            running_at: None,
            last_exit: None,
            work_command: String::new(),
            work_completed: false,
            work_id: None,
            work_origin: None,
            armed: None,
            type_origin: None,
            done: None,
            confirm_paste: None,
            ask: None,
            sel: None,
            search: None,
            hints: None,
            clicks: None,
            scrollbar: None,
            scroll_drag: false,
            mouse_held: None,
            mouse_last: None,
            plsp: None,
            plsp_tried: false,
            program: String::new(),
            program_marks: usize::MAX,
            agent: None,
            notice: None,
            pane_uid,
            ssh_host,
            opened_in,
            chip_hits: Vec::new(),
            cutoff: None,
            cutoff_hit: None,
            type_at_prompt: None,
            replay: false,
            link_hover: None,
            link_ask: None,
            colour_offer: None,
            colour_offer_hit: None,
            folds: Vec::new(),
            view: Vec::new(),
            view_key: None,
            block_sel: None,
            block_filter: None,
            lamp_hits: Vec::new(),
            hunk_hits: Vec::new(),
            diff_cache: std::collections::HashMap::new(),
            select_all_at: None,
            hover_block: 0,
            image_tex: std::collections::HashMap::new(),
            images_gen: 0,
            history: crate::predict::load_history(&profile.name),
            line: String::new(),
            line_ok: true,
            confirm_close: None,
            waiting: false,
        })
    }

    pub(crate) fn new_web_pane(&mut self, url: &str) -> Option<WebPane> {
        let c = self.container.clone();
        self.new_web_pane_in(url, &c)
    }

    /// A page in a named container.
    pub(crate) fn new_web_pane_in(&mut self, url: &str, container: &str) -> Option<WebPane> {
        let shared: SharedRef = Rc::new(std::cell::RefCell::new(Shared {
            scale: self.scale,
            size: (100.0, 100.0),
            ..Default::default()
        }));
        if let Ok(mut config)=shared.borrow().viewer.write(){*config=self.viewer_config();}
        let tab = BrowserTab::create_in(url, shared, self.device.clone(), self.bind_texture.clone(), container)?;
        Some(self.pane_for(tab, container))
    }

    /// A pane around a page that already has its browser.
    /// WebKit's pages (webkit.rs) are native views over the window: lay
    /// each over its pane's page when that page is on screen and nothing
    /// of nus's is over it; hide it otherwise.
    fn sync_webkit(&self) {
        let covered = self.palette.is_some() || self.start.is_some() || self.me_card.open || self.timeline.is_some()
            || self.splash.is_some() || self.board.open || self.page_menu.is_some() || self.peeking().is_some();
        let narrow = self.width_class() == Width::Narrow;
        for (k, tab) in self.tabs.iter().enumerate() {
            let narrow = narrow || tab.solo;
            let split = tab.right.is_some();
            for (right, p) in std::iter::once((false, &tab.left)).chain(tab.right.as_ref().map(|p| (true, p))) {
                let Pane::Web(w) = p else { continue };
                let s = w.tab.shared.borrow();
                let Some(n) = &s.native else { continue };
                let hidden_half = narrow && split && right != tab.focus_right;
                let on = k == self.active && !covered && !hidden_half && w.asleep.is_none() && s.overlay.is_none() && w.page.w > 1.0;
                if on {
                    // What of the page nus's own drawing isn't over: the
                    // sliding sidebar takes a strip off one side.
                    let mut shown = w.page;
                    if let Some(sb) = self.sidebar_over {
                        let (l, r) = (shown.x.max(if sb.x <= shown.x { sb.right() } else { shown.x }), shown.right().min(if sb.right() >= shown.right() { sb.x } else { shown.right() }));
                        shown = Rect::new(l, shown.y, (r - l).max(0.0), shown.h);
                    }
                    n.place(w.page, shown);
                }
                n.show(on);
            }
        }
    }

    pub(crate) fn pane_for(&self, tab: BrowserTab, container: &str) -> WebPane {
        WebPane {
            tab,
            container: container.to_string(),
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
            find: None,
            perm_hits: Vec::new(),
            dedupe: None,
            dedupe_label: String::new(),
            dedupe_hits: Vec::new(), hands: Default::default(), still: None, bare: false, site_panel: false, site_hits: Vec::new(),
            asleep: None,
            slept: None,
            woke: None,
            overlay_sel: 0,
            overlay_hits: Vec::new(),
            wheel_carry: (0.0, 0.0),
            wheel_precise: false,
            bounce: Default::default(),
            bounce_y: 0.0,
            swipe: None,
            load_since: None,
            load_reported: 0.0,
            devtools: None,
            dt_rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            focus_devtools: false,
        }
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
        if self.sidebar_arriving || self.width_class() != Width::Wide || self.focus {
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
        !self.focus && !(self.fullscreen && self.sidebar_rules.fullscreen == Fullscreen::Hidden)
    }

    /// Focus mode: the panes alone in the window. Ctrl+Shift+F11 both ways.
    pub(crate) fn toggle_focus(&mut self) {
        self.focus = !self.focus;
        self.focus_hint = if self.focus { Some(crate::clock::now()) } else { None };
        self.sidebar_hover = false;
        self.close_menus();
        self.play_event("toggle");
        self.layout();
        self.dirty = true;
    }

    /// The hint that fades after entering focus mode.
    fn draw_focus_hint(&mut self, scene: &mut Scene) {
        let Some(at) = self.focus_hint else { return };
        let age = crate::clock::since(at).as_secs_f32();
        if age > 3.0 {
            self.focus_hint = None;
            return;
        }
        self.dirty = true;
        let k = if age < 0.3 { age / 0.3 } else if age > 2.4 { ((3.0 - age) / 0.6).max(0.0) } else { 1.0 };
        let t = self.theme.clone();
        let label = self.label();
        let text = "FOCUS  ·  CTRL+SHIFT+F11 LEAVES";
        let tw = self.fonts.measure(label, text);
        let c = self.content_rect();
        let w = tw + self.px(24.0);
        let r = Rect::new(c.x + ((c.w - w) / 2.0).round(), c.bottom() - self.px(44.0), w, self.px(26.0));
        scene.layer(None);
        scene.rect(r, fade(t.ink, k));
        self.fonts.draw(scene, Style { color: fade(t.paper, k), ..label }, r.x + self.px(12.0), r.y + self.px(17.0), text);
    }

    pub fn sidebar_right(&self) -> bool {
        self.sidebar_rules.side == Side::Right
    }

    pub(crate) fn content_rect(&self) -> Rect {
        let (st, sr, sb, sl) = self.shell_insets();
        let top = st + if self.focus { 0.0 } else { self.px(m::TOP_STRIP) + self.px(m::STRUCTURE) };
        let taken = if self.sidebar_pinned() { self.sidebar_w() + self.px(m::STRUCTURE) } else if self.focus { 0.0 } else { self.px(4.0) };
        let (left, right) = if self.sidebar_right() { (sl, sr + taken) } else { (sl + taken, sr) };
        Rect::new(left, top, self.target.size.0 as f32 - left - right, self.target.size.1 as f32 - top - sb)
    }

    pub(crate) fn sidebar_rect(&self) -> Rect {
        let c = self.content_rect();
        let (_, sr, _, sl) = self.shell_insets();
        let w = self.sidebar_w();
        let x = if self.sidebar_right() { self.target.size.0 as f32 - sr - w } else { sl };
        Rect::new(x + self.sidebar_shift, c.y, w, c.h)
    }

    /// The sidebar's width: the column in compact mode, else the full list.
    pub(crate) fn sidebar_w(&self) -> f32 {
        if self.compact() { self.px(crate::compact::COMPACT_W) } else { self.px(crate::sidebar::width(self.sidebar_rules.width,self.target.size.0 as f32/self.scale)) }
    }

    pub(crate) fn sidebar_visible(&self) -> bool {
        self.sidebar_pinned() || self.sidebar_hover || self.sidebar_arriving
    }

    /// Pin or unpin. Opening slides in over the content and makes room when
    /// it lands; closing gives the room back at once and slides out over it.
    pub(crate) fn toggle_sidebar(&mut self) {
        let arriving = std::mem::take(&mut self.sidebar_arriving);
        let was = arriving || self.sidebar_pinned();
        self.sidebar = !self.sidebar;
        let now = self.sidebar_pinned();
        if !was && now {
            self.sidebar_arriving = true;
        } else if was && !now && !arriving {
            // Mid-arrival it simply turns round from where it is.
            self.sidebar_anim = Anim::at_on(1.0, crate::anim::Curve::Glide);
        }
        self.layout();
        self.dirty = true;
    }

    /// The paper the chrome draws on (theme paper tinted by the surface).
    pub fn paper(&self) -> nus_render::Color {
        // Already tinted by the surface: see `set_mode` and `theme_edit::legible`.
        self.theme.paper
    }

    /// The colour for words on a `fill`: paper or ink, whichever reads
    /// better, made to reach 4.5:1. A button that fills on hover asks this
    /// so its label flips with the fill, whatever the fill turns out to be.
    pub(crate) fn on_fill(&self, fill: nus_render::Color) -> nus_render::Color {
        nus_render::oklch::on(fill, self.theme.paper, self.theme.ink, 4.5)
    }

    pub fn toggle_fullscreen(&mut self) {
        self.fullscreen = !self.fullscreen;
        self.window.set_fullscreen(if self.fullscreen { Some(winit::window::Fullscreen::Borderless(None)) } else { None });
        self.sidebar_hover = false;
        self.layout();
    }

    pub fn cursor_left(&mut self) {
        self.tooltips.leave();
        self.tip = None;
        self.tip_icon = None;
        self.compact_tip = None;
        self.pointer_inside = false;
        if self.sidebar_hover {self.sidebar_leave=Some(crate::clock::now()+std::time::Duration::from_millis(self.sidebar_rules.grace_ms));}
        // Nothing is hovered once the pointer is gone.
        self.mouse = (-1.0, -1.0);
        self.dirty = true;
    }

    pub(crate) fn header_h(&self) -> f32 {
        self.px(m::PANE_HEADER)
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
        // Shells follow their pane a beat later (a size only if it changed).
        if self.resize_due.is_none() {
            self.resize_due = Some(crate::clock::now() + std::time::Duration::from_millis(80));
        }
        if self.layout_tiles() || self.layout_peek() {
            return;
        }
        let _ = (split_w, rule, header, pad_x, pad_y, scale, narrow);
        let active = self.active;
        self.layout_tab(active, c);
    }

    /// Lay tab `i` out in `c`: its pane, or its two panes with the split.
    pub(crate) fn layout_tab(&mut self, i: usize, c: Rect) {
        let split_w = self.split_width(i, c.w);
        let rule = self.px(m::STRUCTURE);
        let header = self.header_h();
        let pad_x = self.px(18.0);
        let pad_y = self.px(16.0);
        let scale = self.scale;
        let narrow = self.width_class() == Width::Narrow || self.tabs.get(i).is_some_and(|t| t.solo);
        let Some(tab) = self.tabs.get_mut(i) else { return };
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
        let bare = self.focus;
        place_pane_bare(&mut tab.left, left_rect, header, pad_x, pad_y, scale, split, bare);
        if let (Some(r), Some(rr)) = (tab.right.as_mut(), right_rect) {
            place_pane_bare(r, rr, header, pad_x, pad_y, scale, split, bare);
        }
        self.dirty = true;
    }

    pub fn resize(&mut self, w: u32, h: u32) {
        self.sync_native_fullscreen();
        if self.target.size != (w, h) {
            self.dismiss_tip();
            self.close_page_menu();
            self.resized_at = Some(crate::clock::now());
        }
        self.target.resize(&self.gpu.device, w, h);
        self.remember_window();
        self.layout();
        self.resize_due = Some(crate::clock::now() + std::time::Duration::from_millis(80));
    }

    /// Time-based housekeeping, once per loop iteration.
    pub fn tick(&mut self) {
        // The page's tint moved (a slider, a preset, a rule): the words
        // are made to read on the new paper.
        if self.legible_on != (self.surface.base, self.surface.tint) {
            self.set_mode(self.theme.mode);
        }
        self.sync_native_fullscreen();
        // AppKit can relayout its titlebar after a resize/fullscreen animation,
        // even when our GPU scene has no reason to redraw.
        self.sync_traffic_lights();
        // First arrival owns input and the visible surface. Defer background
        // discovery, profile polling and maintenance until it hands over.
        if self.arriving() { self.dirty=true; return; }
        self.tend_zoom();
        if self.assistants.poll() { self.dirty = true; }
        let revision=crate::downloads::REVISION.load(std::sync::atomic::Ordering::Relaxed);
        if revision!=self.download_ui.revision {self.download_ui.revision=revision;self.dirty=true;}
        self.tend_downloads();
        self.refresh_shared_prefs();
        self.tend_pip_focus();
        self.sync_viewer_preferences(false);
        self.trim_memory();
        self.drain_popups();
        self.poll_lsp();
        self.editor_tick();
        self.poll_files_folder();
        self.prompt_lsp_tick();
        self.ports_tick();
        self.sync_taskbar_progress();
        self.offer_layout_here();
        self.tidy_tick();
        self.refresh_auto_name();
        self.sync_tick();
        // Dedupe: does the active tab's page live elsewhere already?
        let active = self.active;
        let dup = self.duplicate_of(active).filter(|_| !self.dedupe_kept.contains(&self.tabs[active].id));
        let duplicate_label = dup.map(|i| self.tab_label(i)).unwrap_or_default();
        if let Some(Pane::Web(w)) = self.tabs.get_mut(active).map(|t| &mut t.left) {
            let want = dup.map(|there| (active, there));
            if w.dedupe != want || w.dedupe_label != duplicate_label {
                w.dedupe = want;
                w.dedupe_label = duplicate_label;
                self.dirty = true;
            }
        }
        if self.layout_offer.as_ref().is_some_and(|(_, at)| crate::clock::since(at).as_secs() > 12) {
            self.layout_offer = None;
            self.dirty = true;
        }
        // Skills follow the rules file.
        if self.skills_src_len != self.rules.source.len() {
            self.skills_src_len = self.rules.source.len();
            self.skills = self.rules.skills();
        }
        // Commands the rules asked for.
        let queued: Vec<(String, serde_json::Value)> = std::mem::take(&mut *self.rules.queued.borrow_mut());
        for (cmd, args) in queued {
            if let Err(e) = self.remote(&cmd, &args) {
                tracing::warn!("rules nus.run({cmd}): {e}");
            }
        }
        if self.drop_birth {
            self.drop_birth_shell();
        }
        // What restore asked to type once the shell is at a prompt.
        for tab in self.tabs.iter_mut() {
            for p in std::iter::once(&mut tab.left).chain(tab.right.as_mut()) {
                if let Pane::Term(t) = p {
                    if t.type_at_prompt.is_some() && t.term.at_prompt() {
                        if let Some(text) = t.type_at_prompt.take() {
                            let _ = t.pty.write(text.as_bytes());
                            // Only what the user asked for may later keep the
                            // machine awake; a restore's re-run may not.
                            if let Some(origin) = t.type_origin.take() {
                                t.armed = Some((crate::clock::now(), origin));
                            }
                        }
                    }
                }
            }
        }
        // NUS_TYPE="text" types into the first shell at its first prompt;
        // NUS_SHELL=<profile> picks which shell the first tab runs.
        if !self.typed_once {
            if let Ok(text) = std::env::var("NUS_TYPE") {
                let text = text.replace("\n", "\r");
                // The first shell at a prompt, brought to the front.
                let at = self.tabs.iter().position(|t| matches!(&t.left, Pane::Term(t) if t.term.at_prompt()));
                if let Some(i) = at {
                    if i != self.active {
                        self.activate(i);
                    }
                    if let Some(Pane::Term(t)) = self.tabs.get_mut(i).map(|t| &mut t.left) {
                        let _ = t.pty.write(text.as_bytes());
                    }
                    self.typed_once = true;
                }
            } else {
                self.typed_once = true;
            }
        }
        self.apply_boosts();
        self.poll_reader();
        self.poll_library();
        self.sync_favicons();
        // Once the splash has gone: the "then" step, then Atlas if asked.
        if !self.start_shown && self.splash.is_none() {
            self.start_shown = true;
            if !self.then_done {
                self.then_done = true;
                self.open_start_page(true);
            }
            if self.behavior.atlas != crate::settings::AtlasMode::Planet {
                self.open_start();
            }
        }
        self.track_session();
        if self.window_rect_dirty.is_some_and(|t| crate::clock::since(t).as_millis() > 500) {
            self.window_rect_dirty = None;
            if !self.window.is_maximized() {
                self.save_prefs();
            }
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
        if !self.motion.reduced() && self.surface.texture_motion && self.surface.texture > 0.0 && self.surface.texture_kind != crate::surface::TextureKind::None {
            self.dirty = true;
        }
        if self.registered_tabs != usize::MAX && self.registered_tabs != self.tabs.len() {
            self.register_window();
        }
        self.tend_idle_tabs();
        self.welcome_tick();
        if self.paste_request {
            self.paste_request = false;
            self.paste_into_shell();
        }
        self.tend_folders();
        self.tend_shells();
        self.tend_ask();
        self.tend_scrolling();
        self.tend_bundles();
        self.tend_avatar_pick();
        self.tend_import();
        // A held NEW TAB fans the kinds out.
        if let Some((at, SideHit::NewShell)) = self.press {
            if crate::clock::since(at).as_millis() >= 240 && !self.kinds_menu {
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
        // The look callout follows the pointer: over the chip or the callout
        // it stays; a short grace after leaving, it goes.
        {
            let (mx, my) = self.mouse;
            let over_chip = self.side_hits.iter().any(|(r, h)| *h == SideHit::Look && r.contains(mx, my));
            let over_call = self.look_rect.is_some_and(|r| r.contains(mx, my));
            if (over_chip || over_call) && self.sidebar_visible() && self.sidebar_resize.is_none() && self.sidebar_resize_at(mx,my)!=Some(crate::sidebar::Resize::Footer) {
                self.look_leave = None;
                if !self.look_menu {
                    self.open_look_menu();
                }
            } else if self.look_menu {
                match self.look_leave {
                    None => self.look_leave = Some(crate::clock::now()),
                    Some(t) if crate::clock::since(t).as_millis() > 220 => {
                        self.look_menu = false;
                        self.look_anim.go(0.0, self.motion.dur(120.0));
                        self.look_leave = None;
                        self.dirty = true;
                    }
                    _ => {}
                }
            }
        }
        if self.win_anim.active() || self.kinds_anim.active() || self.flash_anim.active() || self.rail_anim.active() || self.look_anim.active() || self.dl_anim.active() || self.tab_menu_anim.active() {
            self.dirty = true;
        }
        // A slow clock the last frame asked for: a frame when it turns.
        if let Some(ms) = self.beat_want {
            let slot = (crate::clock::since(self.started).as_millis() / ms.max(16) as u128) as u64;
            if slot != self.beat_slot {
                self.beat_slot = slot;
                self.dirty = true;
            }
        }
        // A blinking cursor wants a frame at each half period.
        let blinking = match self.cursor.blink {
            crate::settings::Blink::Never => false,
            crate::settings::Blink::AfterIdle => crate::clock::since(self.last_key).as_secs_f32() > 2.0,
            crate::settings::Blink::Always => true,
        };
        if blinking {
            let period = self.cursor.period.max(100) as u128;
            let half = (crate::clock::since(self.started).as_millis() / period) as u64;
            if half != self.blink_half {
                self.blink_half = half;
                self.dirty = true;
            }
        }
        if self.surface.shell == Shell::Aurora && !self.motion.reduced() && self.surface.drift.abs() > f32::EPSILON {
            // Drift is time based, independent of event-loop and monitor rate.
            self.shell_phase = (crate::clock::since(self.started).as_secs_f32() * self.surface.drift).rem_euclid(1.0);
            self.dirty = true;
        }
        if let Some(t) = self.sidebar_leave {
            if crate::clock::now() >= t {
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
        let hover_dur = self.motion.travel(base::SIDEBAR);
        let row_dur = self.motion.dur(base::ROW);
        // Hover sidebar: shown while hovered (or arriving to pin) and not pinned.
        let want = if (self.sidebar_hover || self.sidebar_arriving) && !self.sidebar_pinned() { 1.0 } else { 0.0 };
        self.sidebar_anim.go(want, hover_dur);
        if self.sidebar_arriving && !self.sidebar_anim.active() && self.sidebar_anim.value() >= 0.999 {
            // Landed: the pin takes over and the content makes room once.
            self.sidebar_arriving = false;
            if self.sidebar_pinned() { self.sidebar_anim = Anim::at_on(0.0, crate::anim::Curve::Glide); }
            self.layout();
            self.dirty = true;
        }
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
        let (reduced, scale) = (self.motion.reduced(), self.scale);
        let mut ready_cue = false;
        for tab in self.tabs.iter_mut() {
            for p in std::iter::once(&mut tab.left).chain(tab.right.as_mut()) {
                if let Pane::Web(w) = p {
                    // Past the page's end: stretch, then spring back.
                    let pulled = std::mem::take(&mut w.tab.shared.borrow_mut().overscroll);
                    if reduced || w.reader.is_some() {
                        w.bounce.stop();
                    } else if pulled != 0.0 && w.wheel_precise {
                        w.bounce.push(pulled);
                    }
                    let (y, moving) = w.bounce.step(w.page.h / scale * 0.18);
                    w.bounce_y = y * scale;
                    self.dirty |= moving;
                    let (loading, progress) = {
                        let s = w.tab.shared.borrow();
                        (s.loading, s.progress as f32)
                    };
                    if loading {
                        let progress = if progress.is_finite() { progress.clamp(0.0,1.0) } else { 0.0 };
                        if w.load_since.is_none() || progress + 0.001 < w.load_reported {
                            w.load = Follow::new(0.0);
                        }
                        w.load_reported = progress;
                        // Hold at reported progress: stalls never invent completion.
                        w.load.target = progress.max(w.load.target);
                        w.load_fade.go(1.0, 0.0);
                        if w.load_since.is_none() {
                            w.load_since = Some(crate::clock::now());
                        }
                    } else {
                        if let Some(t0)=w.load_since.take() {
                            ready_cue |= crate::clock::since(t0).as_secs_f32()>1.0;
                            w.load.target=1.0;
                        }
                        if w.load_fade.target()>0.0 && w.load.value>=0.999 {w.load_fade.go(0.0,out);self.dirty=true;}
                        else if w.load_fade.target()==0.0 && !w.load_fade.active() && w.load.value!=0.0 {w.load=Follow::new(0.0);self.dirty=true;}
                    }
                    if self.motion.reduced() && w.load.value != w.load.target {
                        w.load = Follow::new(w.load.target);
                        self.dirty = true;
                    } else if w.load.step(chase) {
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
        if crate::private::enabled() { return; }
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
                    let host = crate::sites::host_of(&url);
                    let sp = crate::sites::prefs(&host);
                    self.apply_site(w, &url, loading);
                    let boost = if sp.boosts { self.rules.on_page(&url) } else { crate::surface::Boost::default() };
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

    /// The page this tab shows: the focused one, else the page beside the shell.
    fn shown_page(&mut self) -> Option<&mut WebPane> {
        let tab = self.tabs.get_mut(self.active)?;
        let pane = match (&tab.left, tab.focus_right) {
            (_, true) if tab.right.is_some() => tab.right.as_mut().unwrap(),
            (Pane::Web(_), _) => &mut tab.left,
            _ => tab.right.as_mut()?,
        };
        match pane {
            Pane::Web(w) => Some(w),
            _ => None,
        }
    }

    /// ⌘R · Ctrl+Shift+R · F5: the page again; `hard` goes past the cache.
    pub(crate) fn reload_page(&mut self, hard: bool) {
        if let Some(w) = self.shown_page() {
            if hard {
                w.tab.reload_ignore_cache();
            } else {
                w.tab.reload();
            }
        }
    }

    /// Ctrl+Alt+R (⌘⌥R): extract the focused page's article and set it in
    /// Newsreader over the page; again to go back.
    pub(crate) fn toggle_reader(&mut self) {
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

    pub(crate) fn reader_fonts(&self) -> crate::reader::ReaderFonts {
        crate::reader::ReaderFonts { serif: self.f.serif, serif_italic: self.f.wordmark, mono: self.f.term, mono_strong: self.f.term }
    }

    /// Terminal images (Kitty / iTerm2): uploaded once per image, drawn at
    /// their placements' cells.
    fn draw_term_images(&mut self, scene: &mut Scene, p: &mut TermPane) {
        if p.term.placements.is_empty() && p.image_tex.is_empty() {
            return;
        }
        if p.images_gen != p.term.images_gen {
            p.images_gen = p.term.images_gen;
            // Drop textures of images that are gone; upload new ones.
            let live: std::collections::HashSet<u32> = p.term.images.iter().map(|im| im.id).collect();
            p.image_tex.retain(|id, _| live.contains(id));
            for im in &p.term.images {
                if p.image_tex.contains_key(&im.id) {
                    continue;
                }
                let bgra: Vec<u8> = im.rgba.chunks(4).flat_map(|px| [px[2], px[1], px[0], px[3]]).collect();
                let tex = self.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("term image"),
                    size: wgpu::Extent3d { width: im.width, height: im.height, depth_or_array_layers: 1 },
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
                    wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(im.width * 4), rows_per_image: Some(im.height) },
                    wgpu::Extent3d { width: im.width, height: im.height, depth_or_array_layers: 1 },
                );
                p.image_tex.insert(im.id, (self.bind_texture)(&tex));
            }
        }
        let (cw, ch) = p.grid.cell_size();
        let grid = p.term.grid();
        let rows = grid.rows();
        let top = grid.abs_of_display(0);
        let bottom = top + rows as u64;
        let folds = p.folds.clone();
        for pl in &p.term.placements {
            if pl.line >= bottom || pl.line + pl.rows as u64 <= top {
                continue;
            }
            if folds.iter().any(|&(s, e)| pl.line >= s && pl.line < e) {
                continue;
            }
            let Some(bg) = p.image_tex.get(&pl.image) else { continue };
            let x = p.origin.0 + pl.col as f32 * cw;
            let y = p.origin.1 + (pl.line as i64 - top as i64) as f32 * ch;
            let r = Rect::new(x, y, pl.cols as f32 * cw, pl.rows as f32 * ch);
            scene.texture(r, bg.clone(), None);
        }
        if !p.term.placements.is_empty() {
            // Texture draws start a new layer; restore the pane clip.
            scene.layer(Some(Rect::new(p.rect.x, p.rect.y, p.rect.w, p.rect.h)));
        }
    }

    /// Blocks: a hairline where each prompt begins, an exit badge on a
    /// command that failed, a "done" badge on one that took a while, the
    /// shell's progress as a bar, and the paste band.
    fn draw_blocks(&mut self, scene: &mut Scene, p: &mut TermPane, r: Rect, hh: f32) {
        let t = self.theme.clone();
        let ink = t.ink;
        let (cw, ch) = p.grid.cell_size();
        let grid = p.term.grid();
        let rows = grid.rows();
        let label = self.label();
        if self.behavior.shell_integration && !p.term.marks.is_empty() {
            let marks: Vec<nus_vt::Mark> = p.term.marks.clone();
            for m in &marks {
                let Some(row) = p.row_of_line(m.line) else { continue };
                let y = p.origin.1 + row as f32 * ch;
                match m.kind {
                    nus_vt::MarkKind::PromptStart if row > 0 => {
                        scene.hline(r.x + self.px(18.0), y - self.px(3.0), r.w - self.px(36.0), self.px(m::HAIRLINE), fade(ink, 0.16));
                    }
                    nus_vt::MarkKind::CommandEnd(Some(code)) if code != 0 => {
                        // The failed command's line is the previous B mark's.
                        if let Some(b) = marks.iter().rev().find(|x| x.kind == nus_vt::MarkKind::CommandStart && x.line < m.line) {
                            if let Some(brow) = p.row_of_line(b.line) {
                                let by = p.origin.1 + brow as f32 * ch;
                                let text = format!("× {code}");
                                let tw = self.fonts.measure(label, &text);
                                let bx = r.right() - self.px(18.0) - tw;
                                self.fonts.draw(scene, Style { color: self.surface.signal, ..label }, bx, by + ch * 0.72, &text);
                            }
                        }
                    }
                    _ => {}
                }
            }
            let _ = (cw, rows);
        }
        // The shell set its colours: a palette icon offers them for the look.
        p.colour_offer_hit = None;
        if let Some(at) = p.colour_offer {
            if crate::clock::since(at).as_secs() > 20 {
                p.colour_offer = None;
            } else {
                let isz = self.px(14.0);
                let chip = Rect::new(r.right() - self.px(18.0) - isz - self.px(10.0), r.y + hh + self.px(8.0), isz + self.px(20.0), isz + self.px(12.0));
                scene.push(nus_render::Instance::rounded(chip, self.px(4.0), fade(self.paper(), 0.92)));
                scene.outline(chip, self.px(m::HAIRLINE), fade(ink, 0.5));
                let (mx, my) = self.mouse;
                self.icon_button(scene, nus_render::text::icons::PALETTE, isz, chip.x + self.px(10.0), chip.y + self.px(6.0), self.surface.signal, chip, hover_key("colour-offer", p.pty.pid().unwrap_or(0) as usize), IconMotion::Pop);
                if chip.contains(mx, my) {
                    self.tip_words(chip, "the shell set colors · apply them to the look");
                }
                p.colour_offer_hit = Some(chip);
            }
        }
        // A long command just finished: a badge that fades over four seconds.
        if let Some((exit, when)) = p.done {
            let age = crate::clock::since(when).as_secs_f32();
            if age < 4.0 {
                let a = (1.0 - (age - 3.0).max(0.0)).clamp(0.0, 1.0);
                let text = match exit { Some(0) | None => "DONE".to_string(), Some(c) => format!("FAILED · {c}") };
                let color = match exit { Some(0) | None => ink, _ => self.surface.signal };
                let tw = self.fonts.measure(label, &text);
                let bx = r.right() - self.px(18.0) - tw;
                let by = r.y + hh + self.px(14.0);
                scene.rect(Rect::new(bx - self.px(8.0), by - self.px(11.0), tw + self.px(16.0), self.px(18.0)), fade(self.paper(), a));
                self.fonts.draw(scene, Style { color: fade(color, a), ..label }, bx, by + self.px(2.0), &text);
                self.dirty = true;
            } else {
                p.done = None;
            }
        }
        // Progress from the shell (OSC 9;4), as the loading bar along the pane's top.
        if let Some((state, pct)) = p.progress {
            let v = if state == 3 { (crate::clock::since(self.started).as_secs_f32() * 0.5) % 1.0 } else { pct as f32 / 100.0 };
            let color = match state { 2 => self.surface.signal, 4 => crate::theme_edit::from_rgb(t.ansi[3]), _ => ink };
            let th = self.px(self.load_bar.thickness);
            scene.rect(Rect::new(r.x, r.y + hh, r.w * v, th), fade(color, 0.9));
            if state == 3 {
                self.dirty = true;
            }
        }
        // Paste band: how many lines, Enter sends, Esc drops.
        if let Some(text) = p.confirm_paste.clone() {
            let drop = self.band_anim.value();
            let bh = self.header_h();
            let cr = Rect::new(r.x, r.y + hh - (1.0 - drop) * bh, r.w, bh);
            scene.layer(Some(Rect::new(r.x, r.y + hh, r.w, bh)));
            scene.rect(cr, ink);
            let inv = Style { color: t.paper, ..self.label_strong() };
            let inv_l = Style { color: t.paper, ..self.label() };
            let by = cr.y + self.px(m::HEADER_PAD_Y) + self.px(m::UI_PX) - self.px(3.0);
            let mut x = cr.x + self.px(m::HEADER_PAD_X);
            let n = text.lines().count();
            x += self.fonts.draw(scene, inv, x, by, &format!("PASTE {n} LINE{}?", if n == 1 { "" } else { "S" })) + self.px(14.0);
            x += self.fonts.draw(scene, inv_l, x, by, "IT MAY RUN AS TYPED") + self.px(14.0);
            x += self.fonts.draw(scene, inv, x, by, "ENTER") + self.px(14.0);
            self.fonts.draw(scene, inv_l, x, by, "· ESC DROPS IT");
            scene.layer(None);
        }
    }

    /// The working directory the focused shell reported, if any.
    pub(crate) fn focused_cwd(&self) -> Option<String> {
        let tab = self.tabs.get(self.active)?;
        let panes: Vec<&Pane> = if tab.focus_right && tab.right.is_some() { vec![tab.right.as_ref().unwrap(), &tab.left] } else { std::iter::once(&tab.left).chain(tab.right.as_ref()).collect() };
        panes.into_iter().find_map(|p| match p {
            Pane::Term(t) => t.cwd.clone(),
            _ => None,
        })
    }

    /// The focused terminal pane, if the focus is on one.
    pub(crate) fn focused_term(&mut self) -> Option<&mut TermPane> {
        let tab = self.tabs.get_mut(self.active)?;
        match tab.focused() {
            Pane::Term(t) => Some(t),
            _ => None,
        }
    }

    /// Scroll to the previous (-1) or next (+1) prompt.
    pub(crate) fn jump_prompt(&mut self, dir: i32) {
        let Some(t) = self.focused_term() else { return };
        let grid = t.term.grid();
        let top = grid.abs_of_display(0);
        let prompts: Vec<u64> = t.term.marks.iter().filter(|m| m.kind == nus_vt::MarkKind::PromptStart).map(|m| m.line).collect();
        let target = if dir < 0 { prompts.iter().rev().find(|&&l| l < top).copied() } else { prompts.iter().find(|&&l| l > top).copied() };
        if let Some(l) = target {
            t.term.grid_mut().scroll_to_abs(l);
            self.dirty = true;
        } else if dir > 0 {
            t.term.grid_mut().scroll_to_abs(u64::MAX);
            self.dirty = true;
        }
    }

    /// Ctrl+Shift+C: the selection if there is one, else the last output.
    pub(crate) fn copy_selection_or_output(&mut self) {
        // On a page the chord copies its address; the terminal chord is untouched.
        if self.focused_term().is_none() && self.copy_page_url() {
            return;
        }
        let text = self.focused_term().filter(|t| t.sel.is_some()).map(|t| t.selection_text());
        match text {
            Some(text) if !text.is_empty() => {
                if let Ok(mut cb) = arboard::Clipboard::new() {
                    let _ = cb.set_text(text);
                }
                if let Some(t) = self.focused_term() {
                    t.sel = None;
                }
                self.play_event("toggle");
                self.dirty = true;
            }
            _ => self.copy_last_output(),
        }
    }

    /// The last command's output (or the command being typed) to the clipboard.
    pub(crate) fn copy_last_output(&mut self) {
        let Some(t) = self.focused_term() else { return };
        let c = t.term.marks.iter().rev().find(|m| m.kind == nus_vt::MarkKind::OutputStart).copied();
        let text = match c {
            Some(c) => t.term.output_text(&c),
            None => String::new(),
        };
        if text.is_empty() {
            return;
        }
        if let Ok(mut cb) = arboard::Clipboard::new() {
            let _ = cb.set_text(text);
        }
        self.play_event("toggle");
    }

    /// Paste into the focused shell; many lines or control characters ask first.
    pub(crate) fn paste_into_shell(&mut self) {
        let Ok(mut cb) = arboard::Clipboard::new() else { return };
        let Ok(text) = cb.get_text() else { return };
        if text.is_empty() {
            return;
        }
        let risky = text.lines().count() > 1 || text.chars().any(|c| c.is_control() && c != '\t');
        let Some(t) = self.focused_term() else { return };
        if risky {
            t.confirm_paste = Some(text);
            self.band_anim.replay(0.0, 1.0, self.motion.dur(base::BAND));
        } else {
            t.write_paste(&text);
        }
        self.dirty = true;
    }

    /// What a pane's colours go through: the settings' grade and
    /// truecolour rule, then whatever `program(p)` says for what's running.
    fn pane_policy(&self, program: &str, cwd: &Option<String>) -> nus_render::Policy {
        let mut pol = nus_render::Policy { min_contrast: self.behavior.grade.ratio(), snap: self.behavior.truecolour == crate::settings::Truecolour::Snapped, ansi: None, remap: Vec::new() };
        if program.is_empty() {
            return pol;
        }
        let Some(look) = self.rules.program(program, program, cwd.as_deref().unwrap_or(""), &self.theme, self.surface.signal) else { return pol };
        let word = |s: &str| -> Option<nus_vt::Rgb> {
            let c = match s.trim() {
                "ink" => self.theme.ink,
                "paper" => self.theme.paper,
                "signal" => self.surface.signal,
                "dim" => self.theme.dim,
                h => crate::surface::parse_hex(h)?,
            };
            Some(crate::theme_edit::to_rgb(c))
        };
        if let Some(c) = look.contrast {
            pol.min_contrast = c.max(0.0);
        }
        if let Some(s) = look.snap {
            pol.snap = s;
        }
        if let Some(a) = &look.ansi {
            let parsed: Vec<Option<nus_vt::Rgb>> = a.iter().map(|s| word(s)).collect();
            if parsed.iter().all(|c| c.is_some()) {
                pol.ansi = Some(std::array::from_fn(|i| parsed[i].unwrap()));
            }
        }
        pol.remap = look.remap.iter().filter_map(|(from, to)| Some((word(from)?, word(to)?))).collect();
        pol
    }

    /// The caret's colour outside a shell (the editor, the prompt line):
    /// the rule, resolved with the window's signal for the tab's own.
    pub(crate) fn caret_color(&self) -> nus_render::Color {
        match self.cursor.color {
            crate::settings::CursorColor::Theme => self.theme.caret,
            _ => self.surface.signal,
        }
    }

    /// Ask for the next frame in `ms`, not now: slow marks (a breathing
    /// square, a ticking count) need a handful of frames a second, not
    /// every one the display offers.
    pub(crate) fn want_beat(&mut self, ms: u64) {
        self.beat_want = Some(self.beat_want.map_or(ms, |w| w.min(ms)));
    }

    /// A working mark's ink: breathing at ten frames a second, or still
    /// when motion is reduced.
    pub(crate) fn breath(&mut self) -> f32 {
        if self.motion.reduced() {
            return 0.6;
        }
        self.want_beat(100);
        0.35 + 0.45 * (0.5 + 0.5 * (crate::clock::since(self.started).as_secs_f32() * 2.2).sin())
    }

    /// The caret of a line you type into outside a shell — the home line,
    /// the palette — as CURSOR says: its shape (SHELL is the bar a prompt
    /// shows), its color, its blink, its weight. `x` is where the next
    /// character goes; `size` the text's px; `since` the last keystroke.
    pub(crate) fn draw_line_caret(&self, scene: &mut Scene, x: f32, baseline: f32, size: f32, alpha: f32, since: Instant) {
        use crate::settings::{Blink, CursorShapePref};
        let blinking = match self.cursor.blink {
            Blink::Never => false,
            Blink::AfterIdle => crate::clock::since(since).as_secs_f32() > 2.0,
            Blink::Always => true,
        };
        // App::tick asks for a frame when the blink phase changes.
        if blinking && (crate::clock::since(self.started).as_millis() / self.cursor.period.max(100) as u128) % 2 == 1 {
            return;
        }
        let color = fade(self.caret_color(), alpha);
        let (top, h) = (baseline - size * 0.8, size * 0.98);
        let weight = self.px(self.cursor.weight.clamp(1.0, 6.0));
        let r = match self.cursor.shape {
            CursorShapePref::Block => Rect::new(x, top, size * 0.56, h),
            CursorShapePref::Underline => Rect::new(x, baseline + size * 0.14, size * 0.56, weight),
            CursorShapePref::Beam | CursorShapePref::Shell => Rect::new(x, top, weight, h),
        };
        scene.rect(r, color);
    }

    /// The cursor as the prefs want it, for one pane.
    fn cursor_look(&self, p: &TermPane, focused: bool, tab_signal: Option<nus_render::Color>) -> nus_render::CursorLook {
        use crate::settings::{Blink, CursorColor, CursorShapePref};
        let shape = match self.cursor.shape {
            // At a prompt the cursor is a bar: the line is text being edited.
            CursorShapePref::Shell if p.term.at_prompt() && self.behavior.shell_integration => Some(nus_vt::CursorShape::Beam),
            CursorShapePref::Shell => None,
            CursorShapePref::Block => Some(nus_vt::CursorShape::Block),
            CursorShapePref::Beam => Some(nus_vt::CursorShape::Beam),
            CursorShapePref::Underline => Some(nus_vt::CursorShape::Underline),
        };
        let color = match self.cursor.color {
            // The theme's caret, unless the program set one (OSC 12).
            CursorColor::Theme if p.term.palette.override_of(nus_vt::palette::CURSOR).is_some() => None,
            CursorColor::Theme => Some(self.theme.caret),
            CursorColor::Signal => Some(self.surface.signal),
            CursorColor::Tab => tab_signal.or(Some(self.surface.signal)),
        };
        let idle = crate::clock::since(self.last_key).as_secs_f32();
        let blinking = match self.cursor.blink {
            Blink::Never => false,
            Blink::AfterIdle => idle > 2.0,
            Blink::Always => true,
        };
        let visible = if blinking && focused {
            let period = self.cursor.period.max(100) as f32 / 1000.0;
            ((crate::clock::since(self.started).as_secs_f32() / period) as u64) % 2 == 0
        } else {
            true
        };
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
                p.trail.push((p.cur_x.value(), p.cur_y.value(), crate::clock::now()));
                if p.trail.len() > 6 {
                    p.trail.remove(0);
                }
            }
        }
        let (x, y) = (p.cur_x.value(), p.cur_y.value());
        let gliding = p.cur_x.active() || p.cur_y.active();
        let color = look.color.unwrap_or(self.theme.caret);
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
            p.trail.retain(|(_, _, t)| crate::clock::since(t).as_millis() < 240);
            for (cx, cy, t) in &p.trail {
                let a = 1.0 - crate::clock::since(t).as_millis() as f32 / 240.0;
                scene.rect(rect_at(*cx, *cy), Theme::with_alpha(color, 0.35 * a));
            }
            if !p.trail.is_empty() {
                self.dirty = true;
            }
        }
        if self.cursor.motion == CursorMotion::Smear {
            // Neovide's cursor: four springs, a quad. The trail slider is
            // its trail_size; the lengths follow the motion register.
            let (sh, pct) = match shape {
                nus_vt::CursorShape::Beam => (crate::smear::Shape::Vertical, (look.weight / cw).clamp(0.05, 1.0)),
                nus_vt::CursorShape::Underline => (crate::smear::Shape::Horizontal, (look.weight / ch).clamp(0.05, 1.0)),
                _ => (crate::smear::Shape::Block, 1.0),
            };
            p.smear.set_destination((p.origin.0 + tx * cw, p.origin.1 + ty * ch));
            let s = crate::smear::Settings {
                animation_length: self.motion.dur(150.0).max(0.001),
                short_animation_length: self.motion.dur(40.0).max(0.001),
                trail_size: self.cursor.smear.clamp(0.0, 1.0),
            };
            let animating = p.smear.animate(&s, sh, pct, (cw, ch), self.motion.reduced());
            if animating {
                p.smear.draw(scene, Theme::with_alpha(color, 0.95));
                self.dirty = true;
            }
            return;
        }
        if gliding {
            scene.rect(rect_at(x, y), Theme::with_alpha(color, 0.9));
            self.dirty = true;
        }
    }

    /// Put a whole theme on: surface, both faces, cursor colour, bar, sounds.
    pub(crate) fn apply_theme(&mut self, t: &crate::themes::StockTheme) {
        use crate::theme_edit::{Family, ModeEdit};
        self.surface = t.surface.clone();
        let face = |f: &crate::themes::Face| ModeEdit { paper: Some(f.paper), ink: Some(f.ink), page: Some(f.page), ansi: f.ansi, caret: f.caret, selection: f.selection };
        self.theme_edit.paper = face(&t.paper);
        self.theme_edit.ink = face(&t.ink);
        self.theme_edit.family = Family::Imported;
        self.theme_edit.saturation = 1.0;
        self.cursor.color = t.cursor;
        if let Some(m) = t.cursor_motion {
            self.cursor.motion = m;
        }
        self.load_bar.style = t.bar;
        self.load_bar.color = t.bar_color;
        for (event, cue) in &t.sounds {
            self.sound.prefs.map.insert(event.clone(), cue.clone());
        }
        self.tab_colours = t.tab_colours.clone();
        self.preset_name = t.name.clone();
        // A theme drawn for one face comes up in that face unless the OS is followed.
        if !self.behavior.follow_os_theme {
            let mode = if t.prefers_ink { nus_render::Mode::Ink } else { nus_render::Mode::Paper };
            self.set_mode(mode);
        }
        self.rebuild_theme();
        self.refresh_icon();
        self.layout();
        self.save_prefs();
        self.dirty = true;
    }

    /// The look right now as a theme, for SAVE AS.
    pub(crate) fn current_theme(&self, name: &str) -> crate::themes::StockTheme {
        let paper = self.theme_edit.build(nus_render::Mode::Paper, self.surface.signal);
        let ink = self.theme_edit.build(nus_render::Mode::Ink, self.surface.signal);
        let face = |t: &Theme, e: &crate::theme_edit::ModeEdit| crate::themes::Face { paper: t.paper, ink: t.ink, page: t.page, ansi: Some(t.ansi.map(crate::theme_edit::from_rgb)), caret: e.caret, selection: e.selection };
        crate::themes::StockTheme {
            name: name.to_string(),
            story: format!("saved from {} on {}", self.preset_name, chrono_date()),
            port: false,
            cursor_motion: Some(self.cursor.motion),
            paper: face(&paper, &self.theme_edit.paper),
            ink: face(&ink, &self.theme_edit.ink),
            surface: self.surface.clone(),
            cursor: self.cursor.color,
            bar: self.load_bar.style,
            bar_color: self.load_bar.color,
            sounds: Vec::new(),
            tab_colours: self.tab_colours.clone(),
            prefers_ink: self.theme.mode == nus_render::Mode::Ink,
        }
    }

    /// Rebuild the theme for a mode from Broadsheet plus the user's edits.
    pub(crate) fn set_mode(&mut self, mode: nus_render::Mode) {
        let t = self.theme_edit.build(mode, self.surface.signal);
        self.legible_on = (self.surface.base, self.surface.tint);
        let paper = self.surface.paper(t.paper);
        self.set_theme(crate::theme_edit::legible(t, paper));
        // Shell colors come from the face and the signal: worked out again.
        self.recolor_shells();
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

    /// Browse for a picture for the face. The dialog is the system's;
    /// `tend_avatar_pick` asks it for an answer once a loop.
    pub(crate) fn pick_avatar(&mut self) {
        if self.avatar_pick.is_some() {
            return;
        }
        let dest=std::env::current_dir().unwrap_or_default().join("profile/avatar.png");
        match crate::pick::image_file(&self.window,"Choose a picture for your profile",dest) {
            Ok(pending)=>{self.avatar_pick=Some(pending);self.notice(nus_render::text::icons::IMAGE,"Choose A Picture","for your profile");},
            Err(e)=>self.notice_problem("Could Not Open Chooser",e.to_string()),
        }
    }

    /// Once a loop: a picture that was picked becomes the face.
    pub(crate) fn tend_avatar_pick(&mut self) {
        let Some(pending) = self.avatar_pick.as_mut() else { return };
        let Some(picked) = pending.poll() else { return };
        self.avatar_pick = None;
        match picked {
            Ok(false)=>{self.dirty=true;return;},
            Ok(true) => {
                self.load_avatar();
                // A picture is only the face once the face says so.
                if let Some(me) = self.me.as_mut() {
                    me.face = crate::me::Face::Picture;
                    me.save();
                }
                self.me_card.face = crate::me::Face::Picture;
                self.play_event("page.ready");
                self.notice(nus_render::text::icons::CHECK, "Picture Set", "profile/avatar.png");
            }
            Err(e) => self.notice_problem("Could Not Save Picture", e.to_string()),
        }
        self.dirty = true;
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

    /// Desktop mark: a white n above the theme's orbit, with contrast shadow.
    /// macOS uses the process-wide Dock controller; winit covers other desktops.
    pub(crate) fn refresh_icon(&self) {
        if cfg!(target_os="macos") {return;}
        let rgba = crate::app_icon::render(64,self.surface.signal);
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
                    Pane::Term(tp) => sig.push_str(&format!("t{}|{}|{}|", tp.profile, tp.term.cwd.as_deref().unwrap_or(""), tp.running_at.unwrap_or(0))),
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
                let Some(f) = fav else {
                    if w.favicon.take().is_some() {
                        changed = true;
                    }
                    continue;
                };
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
            || self.intel.animating(self.motion.reduced())
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
        if is_local(&w.tab.shared.borrow().url) {return;}
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
            BarStyle::Radiance => {
                let r = page;
                scene.layer(Some(page));
                let scale = self.px(self.load_bar.thickness * 0.5).max(0.5);
                scene.push(nus_render::Instance::loading_light(
                    Rect::new(r.x, r.y-self.px(7.0), r.w, self.px(16.0)), v, scale, col(1.0)));
                scene.layer(None);
            }
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
            Some(t) if force || crate::clock::now() >= t => self.resize_due = None,
            Some(_) => return,
            None if force => {}
            None => return,
        }
        let header = self.header_h();
        let pad_x = self.px(18.0);
        let pad_y = self.px(16.0);
        let mut changed = false;
        for tab in &mut self.tabs {
            for (right,p) in std::iter::once((false,&mut tab.left)).chain(tab.right.as_mut().map(|p|(true,p))) {
                if let Pane::Term(t) = p {
                    let r = t.rect;
                    let header = if t.show_header { header } else { 0.0 };
                    let ask_w = if t.ask.is_some() { (crate::ask::PANEL_W * self.scale).min(r.w * 0.6) } else { 0.0 };
                    let area = Rect::new(r.x + pad_x, r.y + header + pad_y, r.w - 2.0 * pad_x - ask_w, r.h - header - 2.0 * pad_y);
                    let (cols, rows) = t.grid.grid_size(area);
                    if (cols, rows) != (t.term.cols(), t.term.rows()) {
                        t.term.resize(cols, rows);
                        if let Some(rec) = self.recorder.as_mut() {
                            rec.resize(crate::replay::stream_id(tab.id,right), cols, rows);
                        }
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
        if self.scale != scale { self.dismiss_tip(); self.close_page_menu(); }
        self.scale = scale;
        let term_px = self.behavior.typography.terminal_size * scale * 96.0 / 72.0;
        for tab in &mut self.tabs {
            for p in std::iter::once(&mut tab.left).chain(tab.right.as_mut()) {
                if let Pane::Term(t) = p {
                    t.grid.set_font(&self.fonts, self.f.term, term_px * t.zoom as f32 / 100.0);
                    t.grid.set_spacing(&self.fonts,self.behavior.typography.terminal_line,self.behavior.typography.terminal_spacing*scale*t.zoom as f32/100.0);
                }
            }
        }
        self.layout();
    }

    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
        self.rules.forget_programs();
        for tab in &mut self.tabs {
            Self::fit_palette(&self.theme, tab);
        }
        self.dirty = true;
    }

    // --- per-frame -------------------------------------------------------

    /// Drain PTYs, tick terminals, detect URLs. Returns true if anything changed.
    pub fn pump(&mut self) -> bool {
        let fold_over = self.behavior.fold_over;
        let (journal_on, journal_keep) = (self.behavior.journal, self.behavior.journal_keep);
        let shell_colours = self.behavior.shell_colours;
        let mut apply_colours: Vec<(Option<nus_vt::palette::Rgb>, Option<nus_vt::palette::Rgb>)> = Vec::new();
        let mut checkpoints: Vec<(usize, serde_json::Value)> = Vec::new();
        let mut changed = false;
        let mut detected = None;
        let mut bell = false;
        for (i, tab) in self.tabs.iter_mut().enumerate() {
            let hatch_focused = tab.hatch && !self.hatch_state.overview && self.hatch.as_ref().is_some_and(|h| h.visible && h.focused);
            let looked_at = hatch_focused || (!self.hatch_state.main_hidden && !tab.hatch && i == self.active && self.window.has_focus());
            for (right,p) in std::iter::once((false,&mut tab.left)).chain(tab.right.as_mut().map(|p|(true,p))) {
                if let Pane::Term(t) = p {
                    let out = t.pty.take_output();
                    if !out.is_empty() {
                        if let Some(rec) = self.recorder.as_mut() {
                            rec.output(crate::replay::stream_id(tab.id,right), t.term.cols(), t.term.rows(), &out);
                        }
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
                            nus_vt::Event::ClipboardStore(_, b64) => {
                                if self.behavior.osc52 != crate::settings::Osc52::Off {
                                    let bytes = nus_vt::images::base64_decode(&b64);
                                    if let Ok(text) = String::from_utf8(bytes) {
                                        if !text.is_empty() {
                                            if let Ok(mut cb) = arboard::Clipboard::new() {
                                                let _ = cb.set_text(text);
                                            }
                                        }
                                    }
                                }
                            }
                            nus_vt::Event::ClipboardLoad(which) => {
                                if self.behavior.osc52 == crate::settings::Osc52::ReadWrite {
                                    let text = arboard::Clipboard::new().ok().and_then(|mut cb| cb.get_text().ok()).unwrap_or_default();
                                    let sel = if which == 0 { 'c' } else { which as char };
                                    let reply = format!("\x1b]52;{sel};{}\x1b\\", base64_encode(text.as_bytes()));
                                    let _ = t.pty.write(reply.as_bytes());
                                }
                            }
                            nus_vt::Event::Bell => {
                                t.tend_program();
                                crate::agent::notified(&mut t.agent, None, crate::clock::now());
                                if !looked_at {
                                    t.waiting = true;
                                    changed = true;
                                    bell = true;
                                }
                            }
                            nus_vt::Event::Notify { title, body } => {
                                let words = match title { Some(t) if !body.is_empty() => format!("{t} · {body}"), Some(t) => t, None => body };
                                t.tend_program();
                                crate::agent::notified(&mut t.agent, Some(&words), crate::clock::now());
                                t.notice = Some(words);
                                changed = true;
                                if !looked_at {
                                    t.waiting = true;
                                    bell = true;
                                }
                            }
                            nus_vt::Event::Mark(kind) => {
                                tracing::debug!("shell mark {kind:?}");
                                match kind {
                                    nus_vt::MarkKind::OutputStart => {
                                        // A new command generation. It is the user's if
                                        // they submitted it moments ago; otherwise it
                                        // runs, but can't hold the machine awake.
                                        t.work_id = Some(crate::finish_work::next_command());
                                        t.work_origin = t.armed.take().filter(|(at, _)| crate::clock::since(*at) < std::time::Duration::from_secs(10)).map(|(_, o)| o);
                                        t.waiting = false;
                                        t.work_completed = false;
                                        t.work_command = t.blocks().last().map(|b| crate::journal::oneline(&b.cmd)).unwrap_or_default();
                                        t.running_since = Some(crate::clock::now());
                                        t.running_at = Some(crate::journal::now());
                                        t.done = None;
                                    }
                                    nus_vt::MarkKind::CommandEnd(exit) => {
                                        t.work_id = None;
                                        t.work_origin = None;
                                        t.last_exit = exit;
                                        // The command that just finished. Not the last B mark:
                                        // the chunk that carried this D usually carries the
                                        // next prompt's A and B too, and that B has no text yet.
                                        let block=t.blocks().into_iter().rev().find(|b|!b.running);
                                        let finished=block.as_ref().map(|b|crate::journal::oneline(&b.cmd)).unwrap_or_default();
                                        let output=block.as_ref().map(|b|t.block_output_text(b.start)).unwrap_or_default();
                                        t.work_completed = !finished.is_empty();
                                        t.work_command = finished.clone();
                                        // Remember the command for predictions.
                                        if !finished.is_empty() && t.history.last() != Some(&finished) {
                                            crate::predict::append_history(&t.profile_name, &finished);
                                            t.history.push(finished.clone());
                                            if t.history.len() > crate::storage::HISTORY_LINES { t.history.drain(..t.history.len() - crate::storage::HISTORY_LINES); }
                                        }
                                        // Replay: a checkpoint at the block's end (its still on the next draw).
                                        if self.recorder.is_some() {
                                            let ms = t.running_since.map(|s| crate::clock::since(s).as_millis() as u64).unwrap_or(0);
                                            checkpoints.push((i, serde_json::json!({ "cmd": finished, "output":output,"right":right,"exit": exit, "cwd": t.term.cwd, "ms": ms, "at": crate::journal::now() })));
                                        }
                                        // The journal: what ran here, when, how long, how it ended.
                                        if journal_on {
                                            if let (Some(since), Some(at)) = (t.running_since, t.running_at.take()) {
                                                let e = crate::journal::Entry { cmd: finished.clone(), cwd: t.term.cwd.clone().unwrap_or_default(), start: at, ms: crate::clock::since(since).as_millis() as u64, exit, tab: t.title.clone(), shell: t.profile_name.clone() };
                                                if crate::journal::worth(&e) {
                                                    crate::journal::append(&e, journal_keep);
                                                }
                                            }
                                        }
                                        if let Some(since) = t.running_since.take() {
                                            // A command that took a while: say so when
                                            // the user is elsewhere, badge it either way.
                                            if crate::clock::since(since).as_secs_f32() > 2.0 {
                                                t.done = Some((exit, crate::clock::now()));
                                                if !looked_at {
                                                    t.waiting = true;
                                                    bell = true;
                                                }
                                            }
                                        }
                                        if exit.is_some_and(|code|code!=0) && !looked_at { t.waiting=true; bell=true; }
                                        t.progress = None;
                                        // The block is complete: auto-fold long output, ask the rules.
                                        if let Some(b) = t.blocks().last().cloned() {
                                            let verdict = self.rules.on_block(&b, t.term.cwd.as_deref().unwrap_or(""));
                                            let fold = verdict.fold.unwrap_or(fold_over > 0 && b.lines() > fold_over as u64);
                                            if fold && !t.is_folded(&b) {
                                                t.toggle_fold(&b);
                                            }
                                            if verdict.notify && (!looked_at) {
                                                t.waiting = true;
                                                bell = true;
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                                changed = true;
                            }
                            nus_vt::Event::Cwd(path) => {
                                t.cwd = Some(path);
                                changed = true;
                            }
                            nus_vt::Event::ColorsChanged => {
                                // OSC 10/11 set the pane's fg/bg: per the setting, offer or apply.
                                let fg = t.term.palette.override_of(nus_vt::palette::FG);
                                let bg = t.term.palette.override_of(nus_vt::palette::BG);
                                if fg.is_some() || bg.is_some() {
                                    match shell_colours {
                                        crate::settings::ShellColours::Chip => t.colour_offer = Some(crate::clock::now()),
                                        crate::settings::ShellColours::Always => apply_colours.push((fg, bg)),
                                        crate::settings::ShellColours::PaneOnly => {}
                                    }
                                } else {
                                    t.colour_offer = None;
                                }
                                t.term.grid_mut().damage_all();
                                changed = true;
                            }
                            nus_vt::Event::Progress(state, pct) => {
                                let was = t.progress;
                                t.progress = if state == 0 { None } else { Some((state, pct)) };
                                // The rules hear a finish or an error once.
                                let finished = matches!(t.progress, Some((1, 100))) && was != Some((1, 100));
                                let errored = matches!(t.progress, Some((2, _))) && !matches!(was, Some((2, _)));
                                if finished || errored {
                                    self.rules.on_progress(if errored { "error" } else { "done" }, &t.title);
                                }
                                changed = true;
                            }
                            _ => {}
                        }
                    }
                    // What runs here, for tabs that are not drawn too: the
                    // Ledger knows an assistant by it (agent.rs).
                    t.tend_program();
                    if t.term.grid().is_damaged() {
                        changed = true;
                    }
                    if i == self.active {
                        detected = detect_localhost(&t.term);
                    }
                }
            }
        }
        for (i, payload) in checkpoints {
            self.checkpoint(i, payload);
        }
        crate::interstitial::set_colors(crate::interstitial::Colors { paper: self.theme.paper, ink: self.theme.ink, dim: self.theme.dim, signal: self.surface.signal, dark: self.theme.mode == nus_render::theme::Mode::Ink });
        let mut interstitial_acts: Vec<(u64, bool, String)> = Vec::new();
        let mut externals: Vec<String> = Vec::new();
        let mut printed: Vec<Result<std::path::PathBuf, String>> = Vec::new();
        let mut fullscreens: Vec<(u64, bool)> = Vec::new();
        let mut webkit_began: Vec<(u64, bool)> = Vec::new();
        for (k, tab) in self.tabs.iter_mut().enumerate() {
            let id = tab.id;
            let shown = k == self.active;
            for (right, p) in std::iter::once((false, &mut tab.left)).chain(tab.right.as_mut().map(|p| (true, p))) {
                if let Pane::Web(w) = p {
                    interstitial_acts.extend(w.tab.tend_interstitial().into_iter().map(|v| (id, right, v)));
                    if let Some(url) = w.tab.shared.borrow_mut().external.take() {
                        externals.push(url);
                    }
                    if let Some(on) = w.tab.shared.borrow_mut().page_fullscreen.take() {
                        fullscreens.push((id, on));
                    }
                    w.tab.tend_native();
                    if std::mem::take(&mut w.tab.shared.borrow_mut().native_began) {
                        webkit_began.push((id, right));
                        changed = true;
                    }
                    // `window.print()`: the page, as a PDF, where downloads go.
                    let asked = std::mem::take(&mut w.tab.shared.borrow_mut().print_asked);
                    if asked && w.tab.shared.borrow().print_msg.is_none() {
                        let id = w.tab.devtools("Page.printToPDF", serde_json::json!({ "printBackground": true }));
                        w.tab.shared.borrow_mut().print_msg = Some(id);
                    }
                    if let Some(outcome) = w.tab.shared.borrow_mut().print_saved.take() {
                        printed.push(outcome);
                    }
                    if shown && w.asleep.is_none() && w.devtools.is_none() {
                        w.tab.watch();
                    }
                    crate::interstitial_ui::settle_waking(w);
                    let paints = w.tab.shared.borrow().paints + w.devtools.as_ref().map(|d| d.shared.borrow().paints).unwrap_or(0);
                    if paints != w.seen_paints {
                        w.seen_paints = paints;
                        changed = true;
                    }
                }
            }
        }
        for (id, right, verb) in interstitial_acts {
            self.interstitial_act(id, right, &verb);
            changed = true;
        }
        self.tend_docked();
        // A page's fullscreen: the screen, with nus's chrome and the other
        // half of a split out of the way; all of it back when it's over.
        for (id, on) in fullscreens {
            if on && self.page_fullscreen.is_none() {
                if let Some(i) = self.tabs.iter().position(|t| t.id == id) {
                    self.page_fullscreen = Some((id, self.fullscreen, self.focus, self.tabs[i].solo));
                    self.activate(i);
                    self.tabs[i].solo = self.tabs[i].right.is_some();
                    self.focus = true;
                    if !self.fullscreen { self.toggle_fullscreen(); }
                    self.layout();
                }
            } else if !on {
                if let Some((was_id, window, focus, solo)) = self.page_fullscreen.take() {
                    if let Some(t) = self.tabs.iter_mut().find(|t| t.id == was_id) { t.solo = solo; }
                    self.focus = focus;
                    if self.fullscreen != window { self.toggle_fullscreen(); }
                    self.layout();
                }
            }
            changed = true;
        }
        // Its tab closed while it had the screen: put everything back.
        if let Some((id, window, focus, _)) = self.page_fullscreen {
            if !self.tabs.iter().any(|t| t.id == id) {
                self.page_fullscreen = None;
                self.focus = focus;
                if self.fullscreen != window { self.toggle_fullscreen(); }
                self.layout();
                changed = true;
            }
        }
        for (id, right) in webkit_began {
            self.toast(nus_render::text::icons::GLOBE, "Protected Video", "shown by WebKit", Some(crate::toast::Act::LeaveWebKit(id, right)));
        }
        for outcome in printed {
            match outcome {
                Ok(path) => {
                    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    self.toast(nus_render::text::icons::DOWNLOAD, "Saved As PDF", name, Some(crate::toast::Act::RevealPath(path)));
                }
                Err(why) => self.toast_problem("Could Not Print", why, None),
            }
            changed = true;
        }
        // A page tried to open another app: it stays, and nus asks first —
        // what a page names is not always what you'd want launched.
        for url in externals {
            let scheme = url.split(':').next().unwrap_or("").to_string();
            let detail = if scheme.eq_ignore_ascii_case("mailto") { url.trim_start_matches("mailto:").split('?').next().unwrap_or("").to_string() } else { format!("{scheme}: · {}", crate::app::fit_cmd(&url, 48)) };
            let words = if scheme.eq_ignore_ascii_case("mailto") { "Write An Email" } else { "Open In Another App" };
            self.toast(nus_render::text::icons::OPEN_EXTERNAL, words, detail, Some(crate::toast::Act::OpenExternal(url)));
            changed = true;
        }
        if detected != self.detected {
            self.detected = detected;
            changed = true;
        }
        if bell {
            self.play_event("bell");
        }
        for (fg, bg) in apply_colours {
            self.apply_shell_colours(fg, bg);
            changed = true;
        }
        changed
    }

    /// The shell's colours become the look: ink or paper by the background's
    /// lightness, the signal from the foreground when it's a colour (not
    /// grey), the pane's own overrides cleared so it follows the look.
    pub(crate) fn apply_shell_colours(&mut self, fg: Option<nus_vt::palette::Rgb>, bg: Option<nus_vt::palette::Rgb>) {
        if let Some(bg) = bg {
            let lum = 0.2126 * bg.r as f32 + 0.7152 * bg.g as f32 + 0.0722 * bg.b as f32;
            let mode = if lum < 128.0 { nus_render::Mode::Ink } else { nus_render::Mode::Paper };
            if self.theme.mode != mode {
                self.behavior.follow_os_theme = false;
                self.set_mode(mode);
            }
        }
        if let Some(fg) = fg {
            let (r, g, b) = (fg.r as f32 / 255.0, fg.g as f32 / 255.0, fg.b as f32 / 255.0);
            let max = r.max(g).max(b);
            let min = r.min(g).min(b);
            // A grey foreground says nothing about an accent; a coloured one does.
            if max - min > 0.18 {
                self.surface.signal = [r, g, b, 1.0];
                self.rebuild_theme();
                self.refresh_icon();
            }
        }
        for tab in &mut self.tabs {
            for p in std::iter::once(&mut tab.left).chain(tab.right.as_mut()) {
                if let Pane::Term(t) = p {
                    t.term.palette.reset_override(nus_vt::palette::FG);
                    t.term.palette.reset_override(nus_vt::palette::BG);
                    t.colour_offer = None;
                    t.term.grid_mut().damage_all();
                }
            }
        }
        self.save_prefs();
        self.play_event("toggle");
        self.dirty = true;
    }

    /// The offer chip was clicked: apply this pane's colours to the look.
    pub(crate) fn colour_offer_click(&mut self, x: f32, y: f32) -> bool {
        let Some(tab) = self.tabs.get(self.active) else { return false };
        let mut found: Option<(Option<nus_vt::palette::Rgb>, Option<nus_vt::palette::Rgb>)> = None;
        for p in std::iter::once(&tab.left).chain(tab.right.as_ref()) {
            if let Pane::Term(t) = p {
                if t.colour_offer.is_some() && t.colour_offer_hit.is_some_and(|r| r.contains(x, y)) {
                    found = Some((t.term.palette.override_of(nus_vt::palette::FG), t.term.palette.override_of(nus_vt::palette::BG)));
                }
            }
        }
        match found {
            Some((fg, bg)) => {
                self.apply_shell_colours(fg, bg);
                true
            }
            None => false,
        }
    }

    /// CEF's message-pump schedule does not drive our external BeginFrames.
    /// A visible browser needs a frame deadline even when its last paint was
    /// unchanged; otherwise requestAnimationFrame would be capped by idle polling.
    pub fn browser_frame_wait(&self) -> Option<std::time::Duration> {
        let main=(!self.hatch_state.main_hidden).then_some(self.active);
        let hatch=self.hatch.as_ref().filter(|h|h.visible&&!self.hatch_state.overview).and_then(|_|self.hatch_tab());
        let visible=main.into_iter().chain(hatch).any(|i| self.tabs.get(i).is_some_and(|tab| {
            std::iter::once(&tab.left).chain(tab.right.as_ref()).any(|pane|matches!(pane,Pane::Web(w) if w.asleep.is_none()))
        }));
        (visible || self.pip.is_some() || self.little.is_some() || self.docked.is_some()).then(|| {
            std::time::Duration::from_millis(16).saturating_sub(crate::clock::since(self.last_begin_frame))
        })
    }

    pub fn begin_frames(&mut self) {
        if crate::clock::since(self.last_begin_frame).as_millis() < 16 {
            return;
        }
        self.last_begin_frame = crate::clock::now();
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
        // The video pinned over a shell plays in a tab that isn't shown.
        if let Some(src) = self.docked.as_ref().filter(|d| !d.parked).map(|d| d.src_tab) {
            if let Some(tab) = self.tabs.iter().find(|t| t.id == src) {
                for pane in std::iter::once(&tab.left).chain(tab.right.as_ref()) {
                    if let Pane::Web(w) = pane {
                        w.tab.begin_frame();
                    }
                }
            }
        }
        let main=(!self.hatch_state.main_hidden).then_some(self.active);
        let hatch=self.hatch.as_ref().filter(|h|h.visible&&!self.hatch_state.overview).and_then(|_|self.hatch_tab());
        for index in main.into_iter().chain(hatch.filter(|h|Some(*h)!=main)) {
        if let Some(tab) = self.tabs.get(index) {
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
    }

    pub fn redraw(&mut self) {
        if self.hatch_state.main_hidden {return;}
        let changed = if self.arriving() { false } else { self.pump() };
        if !(changed || self.dirty || self.frames == 0) {
            return;
        }
        let _frame = crate::perf::scope("frame_build_submit");
        self.dirty = false;
        let arrival_frame = self.arriving();
        self.build();
        self.sync_traffic_lights();
        self.sync_webkit();
        self.scene.finish();
        for (x, y, w, h, data) in self.fonts.uploads.drain(..) {
            self.gpu.upload_glyph(x, y, w, h, &data);
        }
        self.window.pre_present_notify();
        // Outside a rounded shell: the opposite theme's paper until the window
        // itself is transparent (v1).
        let clear = if arrival_frame && self.target.translucent() {
            [0.0; 4]
        } else if self.surface.shell_radius > 0.0 && self.target.translucent() {
            [0.0; 4]
        } else if self.surface.shell_radius > 0.0 && !self.target.translucent() {
            if self.theme.mode == nus_render::Mode::Ink { Theme::paper().paper } else { Theme::ink().paper }
        } else {
            let p = self.paper();
            let a = if self.target.translucent() { self.surface.opacity } else { 1.0 };
            [p[0] * a, p[1] * a, p[2] * a, a]
        };
        let presented = self.gpu.render(&mut self.target, &self.scene, clear);
        if presented {
        crate::perf::first_frame();
        if let Some(tab) = self.tabs.get_mut(self.active) {
            for pane in std::iter::once(&mut tab.left).chain(tab.right.as_mut()) {
                if let Pane::Editor(e) = pane { if let Some(b) = e.buf_mut() { b.presented(); } }
            }
        }
        } else { self.dirty = true; }
        drop(_frame);
        self.shot_capture(clear);
        self.deferred_shots(clear);
        self.checkpoint_draw(clear);
        self.frames += 1;
        // NUS_FPS=1 logs the frame rate once a second.
        if std::env::var("NUS_FPS").is_ok() {
            thread_local! { static FPS: std::cell::Cell<(u64, Option<Instant>)> = const { std::cell::Cell::new((0, None)) }; }
            FPS.with(|f| {
                let (n, t) = f.get();
                let t = t.unwrap_or_else(Instant::now);
                if crate::clock::since(t).as_secs_f32() >= 1.0 {
                    eprintln!("FPS {}", n + 1);
                    f.set((0, Some(crate::clock::now())));
                } else {
                    f.set((n + 1, Some(t)));
                }
            });
        }
    }

    // --- drawing ---------------------------------------------------------

    pub(crate) fn label(&self) -> Style {
        Style {
            font: self.f.ui,
            px: self.px(m::LABEL_PX)*self.behavior.typography.ui_scale,
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
            px: self.px(m::UI_PX)*self.behavior.typography.ui_scale,
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

    /// The top strip: wordmark, crumb, window cluster. Compact narrows the
    /// sidebar only; the strip stays where it is.
    fn draw_strip(&mut self, scene: &mut Scene) {
        let t = self.theme.clone();
        let ink = t.ink;
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
        x += self.traffic_width();
        // The wordmark is the nus button: home, held down while it is up.
        self.crumb_hits.clear();
        self.tend_home_latch();
        {
            let ww = self.fonts.measure(wm, "nus");
            let cell = Rect::new(x - self.px(8.0), strip.y + self.px(4.0), ww + self.px(16.0), strip.h - self.px(8.0));
            let down = self.home_latch.is_some();
            let hot = cell.contains(self.mouse.0, self.mouse.1);
            if down {
                scene.rect(cell, ink);
            } else if hot {
                scene.rect(cell, fade(t.tint, 0.6));
            }
            let color = if down { self.on_fill(ink) } else { ink };
            self.fonts.draw(scene, Style { color, ..wm }, x, base, "nus");
            self.crumb_hits.push((cell, CrumbHit::Nus));
            x += ww + self.px(18.0);
        }
        let label = self.label();
        let dim = Style { color: t.dim, ..label };
        let lbase = strip.y + self.px(19.0);
        // Crumb: Space chip · tab · cwd/host — each a click target (self.crumb_hits).
        let (p4, p6, p12, p18, p8) = (self.px(4.0), self.px(6.0), self.px(12.0), self.px(18.0), self.px(8.0));
        let segment = move |hits: &mut Vec<(Rect, CrumbHit)>, x: &mut f32, w: f32, hit: CrumbHit| {
            let r = Rect::new(*x - p6, strip.y + p4, w + p12, strip.h - p8);
            hits.push((r, hit));
            *x += w + p18;
        };
        // The Space lives in the sidebar; the header keeps the wordmark once.
        let _ = CrumbHit::Space;
        if crate::private::enabled() {
            let private_label = "INCOGNITO";
            let width = self.fonts.measure(label, private_label) + p12;
            let badge = Rect::new(x-p6, strip.y+p4, width, strip.h-p8);
            scene.outline(badge, self.px(1.0), ink);
            self.fonts.draw(scene, label, x, lbase, private_label);
            x += width+p12;
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
            let reserve=self.px((if self.width_class()==Width::Narrow{172.0}else{300.0})+if cfg!(target_os="macos"){0.0}else{132.0});
            let maxw = (strip.w * 0.42).min(self.px(640.0)).min((strip.right()-reserve-x).max(0.0));
            let start = x;
            let _ = fav;
            {
                let tab = std::mem::take(&mut self.tabs);
                if let Some(t) = tab.get(self.active) {
                    let (main, _) = t.panes();
                    self.draw_pane_icon(scene, main, x, iy, ic, ink, None);
                }
                self.tabs = tab;
            }
            x += ic + self.px(8.0);
            let shown_title = if title.is_empty() { host.clone() } else { title.clone() };
            let host_text = if title.is_empty() || host.is_empty() { String::new() } else { format!(" · {host}") };
            let host_text=self.fit(dim_ui,&host_text,((maxw-(x-start))*0.45).max(0.0));
            let hw = self.fonts.measure(dim_ui, &host_text);
            let fade = Style { color: Theme::with_alpha(ink, self.crumb_anim.value()), ..ui };
            let tfit = self.fit(fade, &shown_title, maxw - (x - start) - hw);
            x += self.fonts.draw(scene, fade, x, lbase, &tfit);
            if !host_text.is_empty() {
                x += self.fonts.draw(scene, dim_ui, x, lbase, &host_text);
            }
            let field = Rect::new(start, strip.y, x - start + self.px(8.0), strip.h);
            if is_local(&url) {
                scene.hline(start, strip.bottom() - self.px(3.0), x - start, self.px(2.0), self.surface.signal);
            }
            self.crumb_hits.push((field, CrumbHit::Url));
        } else {
            let tab = &self.tabs[self.active];
            // Split tabs read as "left | right"; the pane strips name each side.
            let (icon, title) = match &tab.left {
                Pane::Term(p) if tab.right.is_none() => (nus_render::text::icons::TERMINAL, p.title.clone()),
                Pane::Term(_) => (nus_render::text::icons::TERMINAL, tab.title()),
                Pane::Web(_) => (nus_render::text::icons::GLOBE, tab.title()),
                Pane::Settings(_) => (nus_render::text::icons::SETTINGS, "settings".into()),
                Pane::Hints(_) => (nus_render::text::icons::HOME, "welcome".into()),
                Pane::Home(h) if h.library => (nus_render::text::icons::BOOK, "Reading list".into()),
                Pane::Home(_) => (nus_render::text::icons::HOME, "home".into()),
                Pane::Editor(e) => (nus_render::text::icons::CODE, e.title()),
                Pane::Ports(_) => (nus_render::text::icons::PORTS, "ports".into()),
            Pane::Downloads(_) => (nus_render::text::icons::DOWNLOAD, "downloads".into()),
            };
            let title = format!("{} {}", self.tab_label(self.active), title).caps();
            let reserve = self.px((if self.width_class() == Width::Narrow { 172.0 } else { 300.0 }) + if cfg!(target_os = "macos") { 0.0 } else { 132.0 });
            let title = self.fit(label, &title, (strip.right()-reserve-x-ic-p8).max(0.0));
            let tw = self.fonts.measure(label, &title);
            let fade = Style { color: Theme::with_alpha(ink, self.crumb_anim.value()), ..label };
            self.fonts.draw_icon(scene, icon, ic, x, iy, ink);
            self.fonts.draw(scene, fade, x + ic + self.px(8.0), lbase, &title);
            if let (Some((state, pct)), true) = (self.tabs.get(self.active).and_then(|t| t.progress()), self.behavior.progress_sidebar) {
                let (v, color) = self.progress_look(state, pct);
                scene.rect(Rect::new(x, strip.bottom() - self.px(2.0), (ic + p8 + tw) * v, self.px(2.0)), color);
            }
            segment(&mut self.crumb_hits, &mut x, ic + p8 + tw, CrumbHit::Tab);
            if let Some(host) = self.active_tunnel() {
                self.draw_tunnel_tag(scene, &host, x, lbase);
            }
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
                self.icon_button(scene, icon, ic, rx, iy, ink, hr, hover_key("winctl", hit as usize), IconMotion::Still);
                self.crumb_hits.push((hr, hit));
                rx -= gap;
            }
            rx -= self.px(6.0);
        }
        #[cfg(not(target_os="macos"))]
        {
            rx -= ic;
            let hr = Rect::new(rx-self.px(4.0),strip.y,ic+self.px(8.0),strip.h);
            self.icon_button(scene,nus_render::text::icons::MORE,ic,rx,iy,ink,hr,hover_key("application-menu",0),IconMotion::Still);
            self.crumb_hits.push((hr,CrumbHit::Menu)); rx-=gap;
        }
        rx -= ic;
        let hr = Rect::new(rx - self.px(4.0), strip.y, ic + self.px(8.0), strip.h);
        self.icon_button(scene, nus_render::text::icons::SIDEBAR, ic, rx, iy, if self.sidebar { ink } else { t.dim }, hr, hover_key("sidebar", 0), IconMotion::Pop);
        self.crumb_hits.push((hr, CrumbHit::Sidebar));
        rx -= gap + ic;
        let hr = Rect::new(rx - self.px(4.0), strip.y, ic + self.px(8.0), strip.h);
        self.icon_button(scene, nus_render::text::icons::SEARCH, ic, rx, iy, ink, hr, hover_key("search", 0), IconMotion::Pop);
        self.crumb_hits.push((hr, CrumbHit::Search));
        if !crate::private::enabled() {
            rx -= gap + ic;
            let hr=Rect::new(rx-self.px(4.0),strip.y,ic+self.px(8.0),strip.h);
            let status=crate::updates::status();let ready=status.available&&!status.busy;
            if ready{scene.push(nus_render::Instance::rounded(Rect::new(rx-self.px(5.0),iy-self.px(5.0),ic+self.px(10.0),ic+self.px(10.0)),self.px(4.0),fade(self.surface.signal,0.14)));}
            self.icon_button(scene,nus_render::text::icons::DOWNLOAD,ic,rx,iy,if ready{self.surface.signal}else{t.dim},hr,hover_key("updates",if status.busy{2}else if ready{1}else{0}),IconMotion::Still);
            self.crumb_hits.push((hr,CrumbHit::Updates));
        }
        rx -= gap + ic;
        let hr = Rect::new(rx - self.px(4.0), strip.y, ic + self.px(8.0), strip.h);
        let pc = if self.start.is_some() { self.surface.signal } else { ink };
        self.icon_button(scene, nus_render::text::icons::PLANET, ic, rx, iy, pc, hr, hover_key("atlas", 0), IconMotion::Spin(-25.0));
        self.crumb_hits.push((hr, CrumbHit::Start));
        rx -= gap;
        // Finish Work: a fixed place just before the cluster, on laptops
        // only, so nothing beside it moves when work comes and goes.
        if crate::finish_work::offered() {
            use crate::finish_work::Phase;
            let view = crate::finish_work::view();
            let (icon, color) = match view.phase {
                Phase::Holding(_) => (nus_render::text::icons::COFFEE_FILL, self.surface.signal),
                Phase::Ready(_) => (nus_render::text::icons::COFFEE, ink),
                Phase::Unavailable | Phase::SafetyReleased(_) => (nus_render::text::icons::COFFEE, t.dim),
            };
            rx -= ic;
            let hr = Rect::new(rx - self.px(4.0), strip.y, ic + self.px(8.0), strip.h);
            if let Phase::SafetyReleased(_) = view.phase {
                // Let go for safety: a quiet warning mark beside the cup.
                self.fonts.draw_icon(scene, nus_render::text::icons::WARNING, self.px(8.0), rx + ic - self.px(5.0), iy - self.px(3.0), self.surface.signal);
            }
            let key = hover_key("finish-work", 0);
            self.icon_button(scene, icon, ic, rx, iy, color, hr, key, IconMotion::Pop);
            self.offer_tip(key, hr, crate::finish_work::tooltip(view.phase, view.capability));
            self.crumb_hits.push((hr, CrumbHit::FinishWork));
            rx -= gap;
        }
        // status cluster: waiting · pip · assistant · ports (one icon when narrow)
        let waiting = self.tabs.iter().filter(|t| t.waiting()).count();
        let mut cluster: Vec<((&'static str, &'static str), String, CrumbHit, bool)> = Vec::new();
        if self.width_class() == Width::Narrow {
            let lit = waiting > 0 || self.pip.is_some();
            let count = if waiting > 0 { waiting.to_string() } else { String::new() };
            cluster.push((nus_render::text::icons::MORE, count, CrumbHit::Waiting, lit));
        } else {
        // Ports and the assistant always keep their place in the cluster,
        // dim until there is something to count; only pip and the bell come
        // and go, so the icons either side of them do not shift about.
        cluster.push((nus_render::text::icons::PORTS, if self.ports.is_empty() { String::new() } else { self.ports.len().to_string() }, CrumbHit::Ports, !self.ports.is_empty()));
        cluster.push((nus_render::text::icons::ASSISTANT, String::new(), CrumbHit::Assistant, !self.llm_tools.is_empty()));
        if self.pip.is_some() {
            cluster.push((nus_render::text::icons::PIP, String::new(), CrumbHit::Pip, true));
        }
        cluster.push((if waiting > 0 { nus_render::text::icons::BELL_BOLD } else { nus_render::text::icons::BELL }, if waiting > 0 { waiting.to_string() } else { String::new() }, CrumbHit::Waiting, waiting > 0));
        }
        // A new port: the ports icon glows signal for six seconds — no words;
        // the line itself lives in the board's foot and the hatch's.
        let toast_glow = self.board.toast.as_ref().map(|(at, _)| {
            let age = crate::clock::since(at).as_secs_f32();
            if age < 0.25 { age / 0.25 } else if age > 5.4 { ((6.0 - age) / 0.6).clamp(0.0, 1.0) } else { 1.0 }
        });
        for (icon, count, hit, lit) in cluster {
            let color = if lit { ink } else { t.dim };
            let color = match (hit, toast_glow) {
                (CrumbHit::Ports, Some(g)) => crate::surface::mix(color, self.surface.signal, g),
                _ => color,
            };
            if !count.is_empty() {
                let cw = self.fonts.measure(label, &count);
                rx -= cw;
                self.fonts.draw(scene, Style { color: if hit == CrumbHit::Waiting { self.surface.signal } else { color }, ..label }, rx, lbase, &count);
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
            self.icon_button(scene, icon, ic, rx, iy, if hit == CrumbHit::Waiting && lit { self.surface.signal } else { color }, hr, hover_key("cluster", hit as usize), motion);
            self.crumb_hits.push((hr, hit));
            rx -= gap;
        }
        // The Ledger's count: [1 WAITING] 2 WORKING, left of the cluster.
        if self.width_class() != Width::Narrow && self.behavior.ledger {
            if let Some((_, hit)) = self.draw_agent_counts(scene, rx, strip, lbase) {
                self.crumb_hits.push((hit, CrumbHit::Agents));
            }
        }
        let _ = dim;
    }

    fn build(&mut self) {
        self.intel.hits.clear();
        self.beat_want = None;
        if let Some(mut scene)=self.arrival_scene() {
            self.draw_splash(&mut scene);
            self.scene=scene;
            return;
        }
        // Offers are frame-local; lifetime and explicit dismissal are not.
        self.tip = None;
        self.tip_icon = None;
        self.compact_tip = None;
        self.download_ui.hits.clear();
        self.download_ui.anchor=None;
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
        scene.corner_radius = radius;
        // During first arrival the window clear is transparent. Supply the
        // normal paper inside the composition so it can be inked into view.
        if self.arriving() && radius == 0.0 {
            let mut paper = self.paper();
            paper[3] = if self.target.translucent() { self.surface.opacity } else { 1.0 };
            scene.rect(win, paper);
        }
        let sw = self.px(self.surface.shell_width);
        if radius > 0.0 {
            // The paper is a rounded card; the corners outside it show the
            // clear color. It is as translucent as the flat window's clear
            // would be, so a radius never makes the window opaque.
            let p = self.paper();
            let a = if self.target.translucent() { self.surface.opacity } else { 1.0 };
            scene.push(nus_render::Instance::rounded(win, radius, [p[0], p[1], p[2], a]));
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
                let tm = if self.surface.texture_motion { crate::clock::since(self.started).as_secs_f32() % 3600.0 } else { 0.0 };
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
        if !self.focus {
            self.draw_strip(&mut scene);
        }

        self.draw_resize_overlay(&mut scene);
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
        let narrow = self.width_class() == Width::Narrow || self.tabs[active].solo;
        let tiled = self.draw_tiling(&mut scene) || self.draw_peek(&mut scene);
        if has_right && !narrow && !tiled {
            let r = self.tabs[active].right.as_ref().map(|p| p.rect()).unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0));
            self.draw_split_rule(&mut scene, r);
        }
        let n = self.tab_label(active);
        let look = self.tabs[active].look.clone();
        self.drawing_tab = self.tabs[active].id;
        self.pane_hits.clear();
        self.gather_home_rows();
        let mut tabs = std::mem::take(&mut self.tabs);
        if !tiled {
            let tab = &mut tabs[active];
            let left_focused = !(focus_right && has_right);
            if !(narrow && has_right && !left_focused) {
                let split = tab.right.is_some();
                self.draw_pane(&mut scene, &mut tab.left, &n, left_focused, &look, split);
            }
            if let Some(r) = tab.right.as_mut() {
                if !(narrow && left_focused) {
                    self.draw_pane(&mut scene, r, &n, !left_focused, &look, true);
                }
            }
            // The pane controls, over each pane that's on screen.
            let solo = tab.solo;
            let narrow_now = narrow || solo;
            if tab.right.is_some() {
                let lr = tab.left.rect();
                let rr = tab.right.as_ref().map(|r| r.rect());
                if !(narrow_now && !left_focused) {
                    self.draw_pane_controls(&mut scene, lr, false, true);
                }
                if let Some(rr) = rr {
                    if !(narrow_now && left_focused) {
                        self.draw_pane_controls(&mut scene, rr, true, true);
                    }
                }
            }
        }
        self.tabs = tabs;

        // localhost chip, anchored under the detected line.
        if let Some((row, col, url)) = self.detected.clone().filter(|_| !focus_right) {
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
                let col = t.line_col.unwrap_or_else(|| c.col.saturating_sub(t.line.chars().count()));
                let x = t.origin.0 + col as f32 * cw;
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
            let inv_l = Style { color: t.paper, ..self.label() };
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
        self.sidebar_over = None;
        if slide > 0.001 && !self.sidebar_pinned() {
            let sb = self.sidebar_rect();
            // Whole device pixels: glyphs snap to the pixel grid, so a
            // fractional offset makes the text shimmer against the rules.
            let off = ((1.0 - slide) * (sb.w + self.px(12.0))).round();
            let sb = if self.sidebar_right() { Rect::new(sb.x + off, sb.y, sb.w, sb.h) } else { Rect::new(sb.x - off, sb.y, sb.w, sb.h) };
            let shadow = if self.sidebar_right() { -self.px(8.0) } else { self.px(8.0) };
            let edge = self.px(m::STRUCTURE).ceil();
            self.sidebar_over = Some(if self.sidebar_right() { Rect::new(sb.x + shadow - edge, sb.y, sb.w - shadow + edge, sb.h) } else { Rect::new(sb.x, sb.y, sb.w + shadow + edge, sb.h) });
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

        self.draw_compact_tip(&mut scene);
        self.draw_focus_hint(&mut scene);
        self.draw_docked(&mut scene);
        self.draw_pane_drag(&mut scene);
        self.draw_pane_mode(&mut scene);
        if let Some((i, _, _)) = self.drag {
            self.draw_drop(&mut scene, crate::pane_mode::Dragging::Tab(i));
        }
        if self.sidebar_visible() && (self.look_menu||self.look_anim.active()) {self.draw_look_menu(&mut scene,self.list_rect());}
        // A page's question: the strip turns and the sheet hangs from it.
        self.draw_page_dialog(&mut scene);
        // Palette.
        if let Some((mode, input)) = self.palette.clone() {
            // The palette is modal: nothing under it takes the atom's drags.
            self.intel.hits.clear();
            scene.layer(None);
            let rise = self.palette_anim.value();
            scene.rect(Rect::new(0.0, 0.0, w, h), Theme::with_alpha(t.scrim, t.scrim[3] * rise));
            let pw = self.px(m::PALETTE).min(w - 2.0 * self.px(16.0));
            let bx = ((w - pw) / 2.0).round();
            let by = self.px(220.0).min(h * 0.12) + (1.0 - rise) * self.px(10.0);
            let rows = self.palette_rows(mode, &input);
            let row_h = self.px(10.0) * 2.0 + self.px(m::UI_PX) + self.px(m::HAIRLINE);
            let head_h = self.px(14.0) * 2.0 + self.px(16.0) + self.px(2.0);
            let review = matches!(mode, PaletteMode::Assistant(_));
            let leading = self.ui().px * 1.35;
            let lines: Vec<Vec<String>> = rows.iter().map(|row| if review {
                crate::assistants::review_lines(&self.fonts, self.ui(), &row.text, pw-self.px(72.0))
            } else { vec![row.text.clone()] }).collect();
            let heights: Vec<f32> = lines.iter().zip(&rows).map(|(l,r)| if self.saved_detail(r).is_some() && self.behavior.prompt.saved_preview { self.px(56.0) } else { row_h + leading * l.len().saturating_sub(1) as f32 }).collect();
            let total = heights.iter().sum::<f32>().max(row_h);
            // Launching an assistant: the atom and its ring above the review.
            let band_h = if review { self.px(214.0) } else { 0.0 };
            let view_h = total.min((h-by-head_h-band_h-self.px(24.0)).max(row_h));
            self.palette_sel = self.palette_sel.min(rows.len().saturating_sub(1));
            self.palette_scroll_max = (total-view_h).max(0.0);
            if self.palette_reveal {
                let selected_top = heights.iter().take(self.palette_sel).sum::<f32>();
                let selected_end = selected_top + heights.get(self.palette_sel).copied().unwrap_or(row_h);
                if selected_top < self.palette_scroll { self.palette_scroll = selected_top; }
                else if selected_end > self.palette_scroll+view_h { self.palette_scroll=(selected_end-view_h).min(selected_top); }
                self.palette_reveal=false;
            }
            self.palette_scroll=self.palette_scroll.clamp(0.0,self.palette_scroll_max);
            let bh = head_h + band_h + view_h;
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
            let word = match mode { PaletteMode::Application => "menu", PaletteMode::Go => "go", PaletteMode::New => "new", PaletteMode::Url => "url", PaletteMode::Rename => "name", PaletteMode::SaveLayout => "layout", PaletteMode::SyncFolder | PaletteMode::SyncGit | PaletteMode::SyncJoin => "sync", PaletteMode::Place => "place", PaletteMode::Settings => "settings", PaletteMode::RenameTab(_) => "tab", PaletteMode::IconTab(_) => "icon", PaletteMode::Folder(_) => "folder", PaletteMode::Preference(_) => "choose", PaletteMode::Assistant(id) => crate::assistants::NAMES[id as usize], PaletteMode::PromptPin | PaletteMode::SavedEdit(_) => "save", PaletteMode::SavedName(_) => "name" };
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
            self.draw_line_caret(&mut scene, px + self.px(2.0), base, big.px, 1.0, self.last_key);
            self.fonts.draw(&mut scene, esc, r.right() - self.px(18.0) - ew, base - self.px(2.0), "ESC");
            scene.hline(r.x, r.y + head_h - self.px(2.0), r.w, self.px(2.0), ink);
            if review {
                self.draw_intel_band(&mut scene, Rect::new(r.x, r.y + head_h, r.w, band_h));
            }
            // rows
            let viewport=Rect::new(r.x,r.y+head_h+band_h,r.w,view_h);
            scene.layer(Some(viewport));
            let mut y = r.y + head_h + band_h - self.palette_scroll;
            self.palette_hits.clear();
            for (i, PaletteRow { num, text, .. }) in rows.iter().enumerate() {
                let row_h=heights[i];
                let hit_top=y.max(viewport.y);let hit_end=(y+row_h).min(viewport.bottom());
                self.palette_hits.push(Rect::new(r.x,hit_top,r.w,(hit_end-hit_top).max(0.0)));
                let sel = i == self.palette_sel;
                let (fg, bg) = if sel { (t.paper, Some(ink)) } else { (ink, None) };
                if let Some(bg) = bg {
                    scene.rect(Rect::new(r.x, y, r.w, row_h), bg);
                }
                if self.saved_detail(&rows[i]).is_some() {
                    self.draw_saved_row(&mut scene,Rect::new(r.x,y,r.w,row_h),&rows[i],sel,self.behavior.prompt.saved_preview);
                    y += row_h;
                    continue;
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
                if review {
                    for (line, text) in lines[i].iter().enumerate() {
                        self.fonts.draw(&mut scene, ui, px, base+leading*line as f32, text);
                    }
                } else {
                    let text = self.fit(ui, text, r.right() - self.px(18.0) - px);
                    self.fonts.draw(&mut scene, ui, px, base, &text);
                }
                if !sel {
                    scene.hline(r.x, y + row_h - self.px(m::HAIRLINE), r.w, self.px(m::HAIRLINE), t.tint);
                }
                y += row_h;
            }
            scene.layer(None);
            if self.palette_scroll_max > 0.0 {
                let thumb=(view_h*view_h/total).max(self.px(16.0)).min(view_h);
                let offset=(view_h-thumb)*self.palette_scroll/self.palette_scroll_max;
                scene.rect(Rect::new(r.right()-self.px(4.0),viewport.y+offset,self.px(3.0),thumb),t.dim);
            }
        }
        self.draw_layout_offer(&mut scene, w);
        self.draw_tidy(&mut scene, w, h);
        self.draw_board_overlay(&mut scene, w, h);
        self.draw_start(&mut scene);
        self.draw_me_card(&mut scene);
        self.draw_download_overlay(&mut scene);
        self.draw_tip(&mut scene, w, h);
        self.draw_toast(&mut scene);
        self.draw_page_menu(&mut scene);
        self.draw_splash(&mut scene);
        self.draw_mercury(&mut scene);
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
            .map(|(s, b)| self.fonts.measure(if *b { strong } else { label }, &s.caps()))
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
            px += self.fonts.draw(scene, if *b { strong } else { label }, px, by, &s.caps()) + gap;
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
        let logical_hit = hit;
        let hit = scene.clip().map(|clip| hit.intersect(&clip)).unwrap_or(hit);
        self.tip_icon = Some((key, logical_hit, hit));
        let (mx, my) = self.mouse;
        let hot = hit.w > 0.0 && hit.h > 0.0 && hit.contains(mx, my);
        let dur_in = self.motion.dur(80.0);
        let dur_out = self.motion.dur(140.0);
        let pulse_dur = self.motion.dur(260.0);
        let h = self.hovers.entry(key).or_insert_with(|| Hover { alpha: Anim::at(0.0), pulse: Anim::at(1.0), hot: false, since: crate::clock::now() });
        if hot != h.hot {
            h.hot = hot;
            h.alpha.go(if hot { 1.0 } else { 0.0 }, if hot { dur_in } else { dur_out });
            if hot {
                h.pulse.replay(0.0, 1.0, pulse_dur);
                h.since = crate::clock::now();
            }
        }
        if hot {
            if let Some(words) = tip_for(key) {
                self.tip = Some(Tip { key, anchor: hit, text: words.to_string() });
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

    /// The tooltip: a ruled caps chip under (or over) the icon, after the
    /// pointer has rested on it half a second. Cleared every frame; the
    /// icon that is hot sets it again.
    pub(crate) fn draw_tip(&mut self, scene: &mut Scene, w: f32, h: f32) {
        let tip = self.tip.take();
        let target = tip.as_ref().map(|tip| {
            // Labels can change without the control moving. They must not
            // inherit a previous label's dwell or dismissal state.
            let key = hover_key(&format!("{}:{}", tip.key, tip.text), 0);
            tooltip_state::Target { key, bounds: [tip.anchor.x, tip.anchor.y, tip.anchor.w, tip.anchor.h] }
        });
        let blocked = self.tooltip_blocked();
        let frame = self.tooltips.frame(target, self.mouse, crate::clock::now(), blocked, self.motion.reduced());
        self.dirty |= frame.needs_frame;
        let Some(tip) = tip.filter(|_| frame.alpha > 0.0) else { return; };
        let margin = self.px(6.0);
        let pad = self.px(8.0);
        let label = self.label();
        let leading = (label.px * 1.4).max(self.px(16.0));
        let max_width = self.px(440.0).min(w - 2.0 * margin);
        if max_width <= 2.0 * pad || h <= 2.0 * margin + leading { return; }
        let limit = (((h - 2.0 * margin - self.px(8.0)) / leading).floor() as usize).clamp(1, 8);
        let text = tip.text.caps();
        let wrapped = crate::reader::wrap(&self.fonts, label, &text, max_width - 2.0 * pad);
        let mut lines: Vec<String> = wrapped.iter().take(limit).map(|s| self.fit(label, s, max_width - 2.0 * pad)).collect();
        if lines.is_empty() { return; }
        if wrapped.len() > limit {
            if let Some(last) = lines.last_mut() { *last = self.fit(label, &format!("{last}…"), max_width - 2.0 * pad); }
        }
        let cw = (lines.iter().map(|s| self.fonts.measure(label, s)).fold(0.0, f32::max) + 2.0 * pad).min(max_width);
        let ch = (leading * lines.len() as f32 + self.px(8.0)).min(h - 2.0 * margin);
        let x = (tip.anchor.x + tip.anchor.w * 0.5 - cw * 0.5).round().clamp(margin, w - cw - margin);
        let below = tip.anchor.bottom() + margin;
        let y = if below + ch <= h - margin { below } else { tip.anchor.y - margin - ch };
        let y = y.clamp(margin, h - ch - margin);
        let r = Rect::new(x, y, cw, ch);
        let t = self.theme.clone(); let a = frame.alpha;
        scene.layer(None);
        scene.rect(Rect::new(r.x + self.px(3.0), r.y + self.px(3.0), r.w, r.h), fade(t.ink, a));
        scene.rect(r, fade(t.paper, a));
        scene.outline(r, self.px(m::HAIRLINE), fade(t.ink, a));
        for (index, text) in lines.iter().enumerate() {
            self.fonts.draw(scene, Style { color: fade(t.ink, a), ..label }, r.x + pad,
                r.y + self.px(4.0) + label.px + leading * index as f32, text);
        }
    }

    pub(crate) fn tooltip_blocked(&self) -> bool {
        !self.window_focused || self.pointer_hidden || self.palette.is_some()
            || self.board.open || self.start.is_some() || self.me_card.open
            || self.splash.is_some() || self.timeline.is_some() || self.page_menu.is_some()
            || self.dl_menu || self.look_menu || self.win_menu || self.kinds_menu
            || self.tab_menu.is_some() || self.sidebar_resize.is_some() || self.settings_drag.is_some()
            || self.pane_drag.is_some() || self.tile_drag.is_some() || self.split_drag
            || self.drag.is_some() || self.pins.drag.is_some()
    }

    pub(crate) fn dismiss_tip(&mut self) {
        self.tooltips.dismiss();
        self.tip = None;
        self.tip_icon = None;
        self.compact_tip = None;
        self.dirty = true; // Erase already-presented pixels on the next frame.
    }

    pub(crate) fn offer_tip(&mut self, key: u64, anchor: Rect, text: String) {
        if !text.trim().is_empty() && anchor.w > 0.0 && anchor.h > 0.0
            && anchor.contains(self.mouse.0, self.mouse.1) {
            self.tip = Some(Tip { key, anchor, text });
        }
    }

    /// A layout file in this folder: a chip under the strip — a stack icon
    /// and the file's name — for twelve seconds. Click opens it.
    fn draw_layout_offer(&mut self, scene: &mut Scene, w: f32) {
        let Some((path, at)) = self.layout_offer.clone() else { return };
        let age = crate::clock::since(at).as_secs_f32();
        let a = if age < 0.25 { age / 0.25 } else if age > 11.4 { ((12.0 - age) / 0.6).clamp(0.0, 1.0) } else { 1.0 };
        let t = self.theme.clone();
        let label = self.label();
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let text = format!("{} · OPEN THIS LAYOUT", name.trim_end_matches(".nus.luau").to_uppercase());
        let isz = self.px(14.0);
        let tw = self.fonts.measure(label, &text);
        let ch = self.px(26.0);
        let cw = tw + isz + self.px(30.0);
        let c = self.content_rect();
        let r = Rect::new((c.x + c.w - cw) / 2.0 + c.x / 2.0, c.y + self.px(10.0), cw, ch);
        scene.layer(None);
        scene.rect(Rect::new(r.x + self.px(3.0), r.y + self.px(3.0), r.w, r.h), fade(t.ink, a));
        scene.rect(r, fade(t.paper, a));
        scene.outline(r, self.px(m::HAIRLINE), fade(t.ink, a));
        self.fonts.draw_icon(scene, nus_render::text::icons::STACK, isz, r.x + self.px(10.0), r.y + (ch - isz) / 2.0, fade(self.surface.signal, a));
        self.fonts.draw(scene, Style { color: fade(t.ink, a), ..label }, r.x + isz + self.px(20.0), r.y + ch / 2.0 + self.px(m::LABEL_PX) / 2.0 - self.px(2.0), &text);
        self.layout_offer_hit = Some(r);
        let _ = w;
    }

    /// A progress state as (fraction, colour): error red, warning gold,
    /// indeterminate a marquee that keeps the frame alive.
    pub(crate) fn progress_look(&mut self, state: u8, pct: u8) -> (f32, nus_render::Color) {
        let t = &self.theme;
        let v = if state == 3 { (crate::clock::since(self.started).as_secs_f32() * 0.5) % 1.0 } else { pct as f32 / 100.0 };
        let color = match state {
            2 => self.surface.signal,
            4 => crate::theme_edit::from_rgb(t.ansi[3]),
            _ => t.ink,
        };
        if state == 3 {
            self.dirty = true;
        }
        (v, color)
    }

    /// The taskbar button follows the active tab's progress (Windows).
    pub(crate) fn sync_taskbar_progress(&mut self) {
        let cur = if self.behavior.progress_taskbar { self.tabs.get(self.active).and_then(|t| t.progress()) } else { None };
        if cur == self.taskbar_shown {
            return;
        }
        self.taskbar_shown = cur;
        crate::taskbar::set_progress(&self.window, cur);
    }

    /// The rail's width right now (0 when off or hidden).
    pub(crate) fn rail_w(&self) -> f32 {
        if self.header.style != crate::settings::HeaderStyle::Rail || self.sidebar_icons() {
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
        if self.sidebar_icons() {
            return self.px(crate::compact::COMPACT_HEAD);
        }
        let row = self.header_h();
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
        if crate::private::enabled() { return "Incognito".into(); }
        if let Some(n) = &self.window_named {
            if !n.trim().is_empty() {
                return n.clone();
            }
        }
        // The automatic name is cached: it's derived from the filesystem and
        // the tabs, and it must not change under the pointer between frames.
        self.auto_name.borrow().clone().unwrap_or_else(|| "nus".into())
    }

    /// Recompute the automatic window name when what it's made of changed:
    /// the focused cwd, or the set of tabs. Ties between hosts go to the
    /// earliest tab, so the answer is stable.
    pub(crate) fn refresh_auto_name(&mut self) {
        let cwd = self.focused_cwd();
        let hosts: Vec<String> = self.tabs.iter().filter(|t| t.peek.is_none() && !t.hatch).map(|t| t.row_text().1).collect();
        let key = (cwd.clone(), hosts.clone());
        if self.auto_name_key.as_ref() == Some(&key) {
            return;
        }
        self.auto_name_key = Some(key);
        let base = match self.workspace.as_ref().and_then(|w| w.file_name().map(|n| n.to_string_lossy().to_string())).or_else(|| git_root_name(cwd.map(std::path::PathBuf::from))) {
            Some(root) => root,
            None => {
                // The most common host; a tie goes to the one seen first.
                let mut counts: Vec<(String, usize)> = Vec::new();
                for h in hosts.into_iter().filter(|h| !h.is_empty()) {
                    match counts.iter_mut().find(|(k, _)| *k == h) {
                        Some((_, n)) => *n += 1,
                        None => counts.push((h, 1)),
                    }
                }
                counts.into_iter().max_by(|a, b| a.1.cmp(&b.1)).map(|(h, _)| h).unwrap_or_else(|| "nus".into())
            }
        };
        let name = self.unique_name(base);
        if self.auto_name.borrow().as_deref() != Some(name.as_str()) {
            *self.auto_name.borrow_mut() = Some(name);
            self.dirty = true;
        }
    }

    /// Another, earlier window already called that? Number this one.
    fn unique_name(&self, base: String) -> String {
        let me = u64::from(self.window.id());
        let taken = self.windows.iter().filter(|e| e.id != me && e.ordinal < self.ordinal && (e.name == base || e.name.starts_with(&format!("{base} ")))).count();
        if taken == 0 { base } else { format!("{base} {}", taken + 1) }
    }

    /// Where this window is, for the dateline.
    pub(crate) fn dateline(&self) -> String {
        let cwd = self.focused_cwd().map(std::path::PathBuf::from).unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
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
        self.registered_tabs = self.tabs.len();
        let name = self.window_name();
        let base = if name == "nus" { "nus".to_string() } else { format!("{name} · nus") };
        let title = if self.container == crate::containers::PERSONAL { base } else { format!("{base} · {}", self.container.to_lowercase()) };
        self.window.set_title(&title);
    }

    pub(crate) fn sidebar_geometry(&self) -> SidebarGeom {
        let sb = self.list_rect();
        let space_row = self.side_header_h();
        let pinned: Vec<usize> = (0..self.tabs.len()).filter(|&i| self.tabs[i].pinned && !self.tabs[i].hatch && !self.pins.owns(self.tabs[i].id)).collect();
        let pinned_h = if pinned.is_empty() { 0.0 } else if self.sidebar_icons() {self.px(32.0)*pinned.len() as f32} else {self.header_h()};
        let row = self.px(if self.sidebar_icons() && self.sidebar_rules.small_tabs==crate::sidebar::SmallTabs::Preview {48.0}else{m::ROW_H});
        let top = sb.y + space_row + self.pins_height() + pinned_h;
        let foot_y = sb.bottom() - self.sidebar_footer_h();
        let mut y = top - self.sidebar_scroll;
        let mut rows = Vec::new();
        for i in 0..self.tabs.len() {
            if self.tabs[i].pinned || self.tabs[i].peek.is_some() || self.tabs[i].hatch {
                continue;
            }
            let hidden = !self.row_visible(i);
            let want = if hidden { 0.0 } else { row + self.ledger_h(&self.tabs[i]) };
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
        // What the list would take unscrolled: the rows, NEW TAB, the folders.
        let mut reach = y + self.sidebar_scroll - top;
        if self.header.next_row && !self.sidebar_icons() {
            reach += row;
        }
        if !self.sidebar_icons() {
            reach += self.folders_reach();
        }
        SidebarGeom { pinned, pinned_h, rows, foot_y, next_y: y, top, reach }
    }

    /// Keep the list's scroll within what it has to show — a closed tab or
    /// a folded folder may have shortened it — and say how far it can go.
    pub(crate) fn settle_sidebar_scroll(&mut self) -> f32 {
        let max = self.sidebar_geometry().scroll_max();
        if self.sidebar_scroll > max {
            self.sidebar_scroll = max;
        }
        if self.sidebar_scroll < 0.0 {
            self.sidebar_scroll = 0.0;
        }
        max
    }

    /// The wheel over the sidebar: the list scrolls when it overflows, and
    /// the page under it never does. True when the wheel was the sidebar's.
    pub(crate) fn sidebar_wheel(&mut self, x: f32, y: f32, dy_px: f32) -> bool {
        if !self.sidebar_visible() || !self.sidebar_rect().contains(x, y) || self.dl_menu {
            return false;
        }
        if self.side_page!=crate::files::SidePage::Files && self.pins_wheel(x,y,dy_px) {return true;}
        let max = self.settle_sidebar_scroll();
        if max > 0.0 {
            let at = self.sidebar_scroll;
            self.sidebar_scroll = self.glide(crate::scrolling::Glider::Sidebar, at, -dy_px, max);
            self.hover_row = None;
            self.dirty = true;
        }
        true
    }

    fn draw_sidebar(&mut self, scene: &mut Scene) {
        self.adopt_pins();
        if self.sidebar_icons() {
            return self.draw_sidebar_compact(scene);
        }
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

        self.settle_sidebar_scroll();
        let g = self.sidebar_geometry();
        if self.side_page == crate::files::SidePage::Files {
            // FILES: the tree takes the list, from under the header to the footer.
            self.draw_tree(scene, sb, sb.y + row_h, g.foot_y);
            self.draw_sidebar_footer(scene, sb, g.foot_y);
            let _ = (label, ui, dim, strong, ui_strong);
            self.draw_sidebar_menus(scene, sb);
            return;
        }
        self.draw_pins(scene,sb);
        let tiled_ids: Vec<u64> = self.tiling.as_ref().map(|t| t.ids()).unwrap_or_default();
        let tabs = std::mem::take(&mut self.tabs);

        // Legacy shell/file pins retain their session behavior.
        if !g.pinned.is_empty() {
            let py = sb.y + row_h + self.pins_height();
            let cell_w = (sb.w / g.pinned.len() as f32).floor();
            for (k, &i) in g.pinned.iter().enumerate() {
                let cx = sb.x + k as f32 * cell_w;
                let cell = Rect::new(cx, py, cell_w, g.pinned_h - self.px(m::STRUCTURE));
                let active = i == self.active;
                let selected = self.selected.contains(&i);
                if active {
                    scene.rect(cell, ink);
                } else if selected {
                    // Held with Ctrl: the tint and a signal bar, as a row.
                    scene.rect(cell, fade(t.tint, 0.75));
                    scene.rect(Rect::new(cx, py, self.px(2.0), cell.h), tabs[i].look.signal.unwrap_or(self.surface.signal));
                }
                let st = Style { color: if active { self.on_fill(ink) } else { ink }, ..ui_strong };
                let base = py + self.px(8.0) + self.px(m::UI_PX) - self.px(3.0);
                let mut x = cx + self.px(10.0);
                let isz = self.px(12.0);
                self.fonts.draw_icon(scene, nus_render::text::icons::PIN, isz, x, base - isz + self.px(2.0), st.color);
                x += isz + self.px(8.0);
                let _ = k;
                let title = self.fit(st, &tabs[i].title(), cell_w - (x - cx) - self.px(10.0));
                self.fonts.draw(scene, Style { font: self.f.ui, ..st }, x, base, &title);
                if k + 1 < g.pinned.len() {
                    scene.vline(cx + cell_w, py, g.pinned_h, self.px(m::HAIRLINE), ink);
                }
            }
            scene.hline(sb.x, py + g.pinned_h - self.px(m::STRUCTURE), sb.w, self.px(m::STRUCTURE), ink);
        }

        // Tab rows: icon · title · (hover ×). Waiting shows a signal dot.
        let pad_x = self.px(m::ROW_PAD_X);
        let labels: Vec<String> = g.rows.iter().map(|&(i, _, _)| self.tab_label_of(&tabs, i)).collect();
        let depths: Vec<usize> = g.rows.iter().map(|&(i, _, _)| depth_of(&tabs, i)).collect();
        let row_h = self.px(m::ROW_H);
        for (k, &(i, y, h)) in g.rows.iter().enumerate() {
            // A row scrolled under the header or the footer is not drawn;
            // one half out is cut to the list's window.
            let Some(row_clip) = g.clip(sb, y, h) else { continue };
            scene.layer(Some(g.list(sb)));
            let tab = &tabs[i];
            let child = tab.parent.is_some();
            let depth = depths[k];
            let stack: Vec<usize> = (0..tabs.len()).filter(|&j| tabs[j].parent == Some(tab.id)).collect();
            let folded = self.collapsed.contains(&tab.id);
            let open = !stack.is_empty() && !folded;
            let active = i == self.active;
            let hovered = self.hover_row == Some(i);
            let selected = self.selected.contains(&i);
            if active {
                let ty = if self.tint_anim.active() { self.tint_anim.value() } else { y };
                scene.rect(Rect::new(sb.x, ty, sb.w, h), t.tint);
                scene.rect(Rect::new(sb.x, ty, self.px(2.0), h), tab.look.signal.unwrap_or(self.surface.signal));
            } else if selected {
                // Held with Ctrl: the tint and the bar, as the active row wears
                // them, so a selection reads as rows the active one is among.
                scene.rect(Rect::new(sb.x, y, sb.w, h), fade(t.tint, if hovered { 1.0 } else { 0.75 }));
                scene.rect(Rect::new(sb.x, y, self.px(2.0), h), tab.look.signal.unwrap_or(self.surface.signal));
            } else if hovered {
                scene.rect(Rect::new(sb.x, y, sb.w, h), fade(t.tint, 0.5));
            }
            scene.layer(Some(row_clip));
            let base = y + (row_h + self.px(m::UI_PX)) / 2.0 - self.px(2.0);
            let mut x = sb.x + pad_x;
            // A dim numeral for the first nine trees; a rule per level for the rest.
            let numw = self.px(18.0);
            if child {
                for d in 0..depth {
                    scene.vline(x + self.px(5.0) + d as f32 * self.px(12.0), y, row_h, self.px(m::HAIRLINE), t.dim);
                }
                x += numw + (depth.saturating_sub(1)) as f32 * self.px(12.0);
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
                Pane::Home(h) if h.library => nus_render::text::icons::BOOK,
                Pane::Home(_) => nus_render::text::icons::HOME,
                Pane::Editor(_) => nus_render::text::icons::CODE,
                Pane::Ports(_) => nus_render::text::icons::PORTS,
            Pane::Downloads(_) => nus_render::text::icons::DOWNLOAD,
            };
            let row_bg = if active { crate::surface::mix(self.paper(), ink, t.tint[3]) } else if selected { crate::surface::mix(self.paper(), ink, t.tint[3] * if hovered { 1.0 } else { 0.75 }) } else if hovered { crate::surface::mix(self.paper(), ink, t.tint[3] * 0.5) } else { self.paper() };
            let isz = self.px(15.0);
            let iy = y + ((row_h - isz) / 2.0).round();
            let _ = icon;
            match &tab.emoji {
                Some(e) => {
                    let st = Style { font: self.f.ui, px: self.px(14.0), color: if active { ink } else { t.dim }, tracking: 0.0 };
                    let ew = self.fonts.measure(st, e);
                    self.fonts.draw(scene, st, x + ((isz - ew) / 2.0).max(0.0), base, e);
                }
                None => {
                    let (main, other) = tab.panes();
                    self.draw_pane_icon(scene, main, x, iy, isz, if active { ink } else { t.dim }, Some(row_clip));
                    if let Some(o) = other {
                        // The split's other pane, as a badge at the corner.
                        let bsz = self.px(9.0);
                        let br = Rect::new(x + isz - bsz + self.px(2.0), iy + isz - bsz + self.px(2.0), bsz, bsz);
                        scene.rect(Rect::new(br.x - self.px(1.5), br.y - self.px(1.5), bsz + self.px(3.0), bsz + self.px(3.0)), row_bg);
                        self.draw_pane_icon(scene, o, br.x, br.y, bsz, if active { ink } else { t.dim }, Some(row_clip));
                    }
                }
            }
            x += isz + self.px(10.0);
            // Right side: × on hover, else a signal dot when waiting, else a
            // collapsed stack's count.
            let mut right = sb.right() - pad_x;
            if tiled_ids.contains(&tab.id) {
                let tsz = self.px(11.0);
                self.fonts.draw_icon(scene, nus_render::text::icons::TILES, tsz, right - tsz, y + (row_h - tsz) / 2.0, if active { ink } else { t.dim });
                right -= tsz + self.px(8.0);
            }
            if hovered {
                let cx = right - isz;
                self.fonts.draw_icon(scene, nus_render::text::icons::CLOSE, isz, cx, iy, ink);
                self.side_hits.push((Rect::new(cx - self.px(6.0), y, isz + self.px(12.0), row_h), SideHit::Close(i)));
                right = cx - self.px(8.0);
            } else if let Some((name, is_waiting)) = tab.attention().filter(|_| !self.ledger_shows(tab)) {
                // A signal square when waiting for you; an assistant at work
                // breathes in ink. The words are in the tooltip. (With the
                // Ledger's lines under the title, they say it instead.)
                let d = self.px(7.0);
                let dot = Rect::new(right - d, y + (row_h - d) / 2.0, d, d);
                if is_waiting {
                    scene.rect(dot, self.surface.signal);
                } else {
                    let breath = self.breath();
                    scene.rect(dot, fade(ink, breath));
                }
                if dot.contains(self.mouse.0, self.mouse.1) {
                    let words = if is_waiting { format!("{name} · waiting for you") } else { format!("{name} · working") };
                    self.tip_words(Rect::new(dot.x - self.px(4.0), y, d + self.px(8.0), row_h), &words);
                }
                right -= d + self.px(8.0);
            } else if !stack.is_empty() {
                // A node: its count and a caret; click the caret to fold or unfold.
                let all = subtree_of(&tabs, i).len();
                let tag = format!("{all}");
                let tw = self.fonts.measure(label, &tag);
                let csz = self.px(11.0);
                let icon = if open { nus_render::text::icons::CARET_DOWN } else { nus_render::text::icons::CARET_RIGHT };
                self.fonts.draw(scene, dim, right - tw, base - self.px(1.0), &tag);
                let cx = right - tw - csz - self.px(2.0);
                self.fonts.draw_icon(scene, icon, csz, cx, y + (row_h - csz) / 2.0, t.dim);
                self.side_hits.push((Rect::new(cx - self.px(8.0), y, tw + csz + self.px(16.0), row_h), SideHit::Fold(i)));
                right -= tw + csz + self.px(10.0);
            }
            // A page in a container: its colour as a small square before the title.
            if let Pane::Web(w) = &tab.left {
                if w.container != crate::containers::PERSONAL {
                    let c = self.colour_of(&w.container);
                    let d = self.px(6.0);
                    scene.rect(Rect::new(x, y + ((row_h - d) / 2.0).round(), d, d), c);
                    x += d + self.px(6.0);
                }
            }
            let (title, _) = tab.row_text();
            // A shell that only calls itself by its own name ("zsh") is
            // called by the assistant at work in it, when the Ledger knows one.
            let title = match (tab.agent(), &tab.left) {
                (Some(a), Pane::Term(t)) if tab.name.is_none() && self.behavior.ledger && title.eq_ignore_ascii_case(&t.profile_name) => a.name.clone(),
                _ => title,
            };
            let st = if active { ui_strong } else { ui };
            let st = Style { color: if active { ink } else { Theme::with_alpha(ink, 0.82) }, ..st };
            let tab_id = tab.id;
            self.marquee(scene, st, x, base, right - x, &title, active || hovered, row_bg, hover_key("row", tab_id as usize));
            // The Ledger: the assistant's lines, under the title.
            if h > row_h + 1.0 {
                self.draw_ledger(scene, tab, i, x, y + row_h - self.px(4.0), sb.right() - pad_x - x);
            }
            // The shell's progress (OSC 9;4): a line under the title.
            if let (Some((state, pct)), true) = (tab.progress(), self.behavior.progress_sidebar) {
                let (v, color) = self.progress_look(state, pct);
                let py = y + row_h - self.px(3.0);
                scene.rect(Rect::new(x, py, (right - x) * v, self.px(2.0)), color);
            }
            scene.layer(None);
        }
        self.tabs = tabs;

        // A row being dragged: the others part, the ghost follows the pointer.
        if let Some((di, off, dy)) = self.drag {
            if let Some(&(_, _, _)) = g.rows.iter().find(|&&(t, _, _)| t == di) {
                let (mx, my) = self.mouse;
                let _ = mx;
                // Drop marker: a signal rule between rows, or a tint on the row it nests under.
                if let Some(&(j, ry, rh)) = g.rows.iter().find(|&&(_, ry, rh)| my >= ry && my < ry + rh) {
                    if j != di {
                        let frac = (my - ry) / rh;
                        if (0.3..0.7).contains(&frac) {
                            scene.outline(Rect::new(sb.x + self.px(6.0), ry + self.px(2.0), sb.w - self.px(12.0), rh - self.px(4.0)), self.px(m::STRUCTURE), self.surface.signal);
                        } else {
                            let ly = if frac < 0.3 { ry } else { ry + rh };
                            scene.rect(Rect::new(sb.x + self.px(12.0), ly - self.px(1.0), sb.w - self.px(24.0), self.px(2.0)), self.surface.signal);
                        }
                    }
                }
                let ghost = Rect::new(sb.x + self.px(4.0), dy - off, sb.w - self.px(8.0), row_h);
                scene.rect(Rect::new(ghost.x + self.px(3.0), ghost.y + self.px(3.0), ghost.w, ghost.h), fade(ink, 0.5));
                scene.rect(ghost, self.paper());
                scene.outline(ghost, self.px(m::STRUCTURE), ink);
                let title = self.tabs[di].title();
                let st = ui_strong;
                let text = self.fit(st, &title, ghost.w - self.px(24.0));
                self.fonts.draw(scene, st, ghost.x + self.px(12.0), ghost.y + (row_h + self.px(m::UI_PX)) / 2.0 - self.px(2.0), &text);
            }
        }
        // The next ruled row is NEW TAB: a ghost plus where the tab will appear.
        if let Some(next_clip) = if self.header.next_row { g.clip(sb, g.next_y, row_h) } else { None } {
            scene.layer(Some(next_clip));
            let r = Rect::new(sb.x, g.next_y, sb.w, row_h);
            let hit = next_clip;
            let (mx, my) = self.mouse;
            let hot = hit.contains(mx, my) && self.sidebar_visible() && !self.win_menu && !self.kinds_menu;
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
            self.side_hits.push((hit, SideHit::NewShell));
            scene.layer(None);
        }
        // Folders, under the tabs, down to the footer.
        let folders_top = g.next_y + if self.header.next_row { row_h } else { 0.0 };
        let folders_from = folders_top.max(g.top);
        scene.layer(Some(Rect::new(sb.x, folders_from, sb.w, (g.foot_y - folders_from).max(0.0))));
        self.draw_folders(scene, sb, folders_top, g.foot_y - self.px(4.0));
        scene.layer(None);
        self.draw_sidebar_scrollbar(scene, sb, &g);

        self.draw_sidebar_footer(scene, sb, g.foot_y);
        let _ = (label, ui, dim, pad_x);
        self.draw_sidebar_menus(scene, sb);
    }

    /// The footer: one row of verbs. Avatar · new tab · the look · files ·
    /// recently closed · downloads · settings.
    fn draw_sidebar_footer(&mut self, scene:&mut Scene, sb:Rect, foot_y:f32) {self.draw_responsive_footer(scene,sb,foot_y);}

    /// A list taller than its window says so: a thin thumb at the list's
    /// outer edge, dim, there while the pointer is over the sidebar.
    pub(crate) fn draw_sidebar_scrollbar(&mut self, scene: &mut Scene, sb: Rect, g: &SidebarGeom) {
        let max = g.scroll_max();
        let window = g.foot_y - g.top;
        if max <= 0.0 || window <= 0.0 || !sb.contains(self.mouse.0, self.mouse.1) {
            return;
        }
        let w = self.px(2.0);
        let x = if self.sidebar_right() { sb.x + self.px(1.0) } else { sb.right() - w - self.px(1.0) };
        let track = Rect::new(x, g.top + self.px(2.0), w, window - self.px(4.0));
        let len = (track.h * window / g.reach).max(self.px(16.0)).min(track.h);
        let y = track.y + (track.h - len) * (self.sidebar_scroll / max).clamp(0.0, 1.0);
        scene.rect(Rect::new(track.x, y, track.w, len), fade(self.theme.dim, 0.6));
    }

    /// A tooltip for a footer verb.
    pub(crate) fn foot_tip(&mut self, key: u64, hit: Rect, words: String) {
        self.offer_tip(key, hit, words);
    }

    /// The look chip: paper, ink and signal as a hand of three cards —
    /// the studio's tiles at footer size. Ink outline at the structure
    /// weight, a hard offset shadow, corners from the carapace radius,
    /// colours from the theme. Hover fans them out and steps them up, the
    /// signal card on top; click opens the studio.
    pub(crate) fn draw_look_chip(&mut self, scene: &mut Scene, x: f32, fy: f32, fh: f32, available:f32) {
        let sw = self.px(13.0).min(available/3.4);
        let hit = Rect::new(x - self.px(6.0), fy, sw * 2.4 + self.px(12.0), fh);
        let (mx, my) = self.mouse;
        let hot = hit.contains(mx, my);
        let key = hover_key("look", 0);
        let dur_in = self.motion.dur(160.0);
        let dur_out = self.motion.dur(220.0);
        let h = self.hovers.entry(key).or_insert_with(|| Hover { alpha: Anim::at(0.0), pulse: Anim::at(1.0), hot: false, since: crate::clock::now() });
        if hot != h.hot {
            h.hot = hot;
            h.alpha.go(if hot { 1.0 } else { 0.0 }, if hot { dur_in } else { dur_out });
        }
        let a = if self.motion.reduced() { if hot { 1.0 } else { 0.0 } } else { h.alpha.value() };
        if h.alpha.active() {
            self.dirty = true;
        }
        let ink = self.theme.ink;
        let paper = self.paper();
        let signal = self.surface.signal;
        // The carapace's radius, scaled to a 13px card: square when it's square.
        let radius = (self.px(self.surface.shell_radius) * 0.18).min(self.px(4.0));
        let outline = self.px(m::HAIRLINE) * 1.5;
        let shadow = self.px(2.0) + self.px(1.0) * a;
        let cy = fy + fh / 2.0;
        // At rest the cards overlap by half; hovered they spread to touching and
        // step up, each a little higher than the last.
        let step = sw * 0.5 + sw * 0.62 * a;
        // One hard shadow under the whole hand, then the cards. The ink card
        // is outlined in paper so it reads as a card and not as the outline.
        let hand = Rect::new(x, cy - sw / 2.0 - self.px(3.0) * a, 2.0 * step + sw, sw + self.px(3.0) * a);
        scene.push(nus_render::Instance::rounded(Rect::new(hand.x + shadow, hand.y + shadow, hand.w, hand.h), radius, fade(ink, 0.9)));
        let mut i = 0.0;
        for c in [paper, ink, signal] {
            let rise = self.px(1.0) * a * (i + 1.0);
            let r = Rect::new(x + i * step, cy - sw / 2.0 - rise, sw, sw);
            let edge = if c == ink { paper } else { ink };
            scene.push(nus_render::Instance::rounded(r, radius, edge));
            let inner = Rect::new(r.x + outline, r.y + outline, r.w - 2.0 * outline, r.h - 2.0 * outline);
            scene.push(nus_render::Instance::rounded(inner, (radius - outline).max(0.0), c));
            i += 1.0;
        }
        self.side_hits.push((hit, SideHit::Look));
    }

    /// What a sidebar target does. From the mouse, NEW TAB waits for the
    /// release (a hold fans out); from the accessibility tree it acts at once.
    pub(crate) fn side_action(&mut self, hit: SideHit, from_mouse: bool) {
        match hit {
            SideHit::Pinned(act) => self.pin_action(act),
            SideHit::MenuDrawer => self.toggle_menu_drawer(self.menu_footer_anchor()),
            SideHit::Answer(i, answer) => {
                self.answer_agent(i, answer);
            }
            SideHit::Close(i) => {
                // The × does what Ctrl+W does: a row in the selection takes
                // the whole selection with it; a row outside it goes alone.
                if !self.selected.contains(&i) {
                    self.selected.clear();
                }
                self.activate(i);
                self.close_tabs(false);
            }
            SideHit::Profile => {
                if self.me_card.open {
                    self.close_me_card();
                } else {
                    self.open_me_card();
                }
            }
            SideHit::NewTab => self.open_start_page(false),
            SideHit::NewShell if !from_mouse => self.open_start_page(false),
            SideHit::NewShell => {
                self.press = Some((crate::clock::now(), SideHit::NewShell));
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
                    self.front_request = Some(e.id);
                }
            }
            SideHit::Rail(k) => {
                if let Some(e) = self.windows.get(k).cloned() {
                    if e.id != u64::from(self.window.id()) {
                        self.front_request = Some(e.id);
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
            SideHit::LookStudio => {
                self.close_menus();
                if let Some(row)=self.search_settings("show in footer").into_iter().find(|r|matches!(r.action,Action::SettingsRow(..))) {self.run(row.action);}
                else {self.open_settings_at(0,Some(0));}
            }
            SideHit::Look => {
                self.close_menus();
                self.open_settings();
                if let Some(Pane::Settings(s)) = self.tabs.get_mut(self.active).map(|t| &mut t.left) {
                    s.section = crate::settings::SEC_LOOK;
                }
                self.look_tab = crate::settings::LOOK_PRESETS;
            }
            SideHit::LookPreset(k) => {
                // The callout lists originals only; map to the full list.
                let all = crate::themes::all();
                if let Some(t) = all.get(k).cloned() {
                    self.apply_theme(&t);
                }
                self.play_event("toggle");
            }
            SideHit::LookQuick(q) => self.quick_look(q),

            SideHit::Files => self.toggle_files(),
            SideHit::FilesPin => {
                if self.workspace.is_some() {
                    self.bind_workspace(None);
                    self.notice(nus_render::text::icons::FOLDER, "Tree Follows The Shell", "");
                } else if let Some(r) = self.tree.root.clone() {
                    self.bind_workspace(Some(r.clone()));
                    self.notice(nus_render::text::icons::FOLDER, "Window Bound", r.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default());
                }
            }
            SideHit::FilesUp => {
                if let Some(up) = self.tree.root.as_ref().and_then(|r| r.parent().map(|p| p.to_path_buf())) {
                    if self.workspace.is_some() {
                        self.bind_workspace(Some(up));
                    } else {
                        self.tree.set_root(Some(up));
                    }
                }
            }
            SideHit::FileRow(k) => self.tree_click(k),
            SideHit::Closed => {
                self.open_palette(PaletteMode::Go);
                if let Some((_, input)) = self.palette.as_mut() {
                    input.push_str("reopen");
                }
            }
            SideHit::Downloads => {
                if self.dl_menu {
                    self.close_menus();
                } else {
                    self.close_menus();
                    self.dl_menu = true;
                    self.download_ui.scroll=0.0;self.download_ui.focus=None;
                    self.dl_anim.replay(0.0, 1.0, self.motion.dur(160.0));
                }
            }
            SideHit::Fold(i) => self.toggle_fold(i),
            SideHit::TabRename(i) => {
                self.close_menus();
                self.open_palette(PaletteMode::RenameTab(i));
            }
            SideHit::TabIcon(i) => {
                self.close_menus();
                self.open_palette(PaletteMode::IconTab(i));
            }
            SideHit::TabColour(i, k) => {
                let c = if k == 0 { None } else { crate::surface::SWATCHES.get(k - 1).map(|s| s.1) };
                self.run(Action::ColourTab(i, c));
            }
            SideHit::TabFolder(i) => {
                self.close_menus();
                self.open_palette(PaletteMode::Folder(i));
            }
            SideHit::Folder(fi) => self.toggle_folder(fi),
            SideHit::FolderItem(fi, k) => self.open_item(fi, k),
            SideHit::FolderDrop(fi, k) => self.remove_from_folder(fi, k),
            SideHit::TabTile(i) => {
                self.close_menus();
                if self.selected.iter().any(|&k| k != i) {
                    self.selected.insert(i);
                    self.activate(i);
                    self.tile_selected();
                } else {
                    self.untile();
                }
            }
            SideHit::TabPin(i) => { self.close_menus(); self.pin_tab(i); }
            SideHit::TabClose(i) => {
                self.close_menus();
                self.selected.clear();
                self.activate(i);
                self.close_tabs(false);
            }
            SideHit::Settings => self.open_settings(),
        }
        self.dirty = true;
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
        let me = u64::from(self.window.id());
        let entries = if self.windows.is_empty() { Arc::new(vec![crate::windows::Entry { id: me, name: self.window_name(), tabs: self.tabs.len(), ordinal: self.ordinal, colour: self.container_colour() }]) } else { self.windows.clone() };
        let (mx, my) = self.mouse;
        for (k, e) in entries.iter().enumerate() {
            let cy = r.y + k as f32 * row;
            let cell = Rect::new(x, cy, rw, row);
            let on = e.id == me;
            let hot = cell.contains(mx, my) && self.sidebar_visible();
            if on {
                scene.rect(cell, t.tint);
                let bar_x = if self.sidebar_right() { x } else { x + rw - self.px(2.0) };
                scene.rect(Rect::new(bar_x, cy, self.px(2.0), row), self.surface.signal);
            } else if hot {
                scene.rect(cell, fade(t.tint, 0.5));
            }
            let color = if on { e.colour } else { Theme::with_alpha(e.colour, 0.55) };
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
        let row = self.header_h();
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
                let bg = if hot || self.win_menu { crate::surface::mix(self.paper(), ink, t.tint[3] * 0.6) } else { self.paper() };
                self.marquee(scene, wm, sb.x + self.px(12.0), y + self.px(31.0), sb.w - self.px(52.0), &name, hot, bg, hover_key("mast", 0));
            } else {
                scene.rect(Rect::new(sb.x + self.px(12.0), y + self.px(10.0), self.px(10.0), self.px(10.0)), self.container_colour());
                let bg = if hot || self.win_menu { crate::surface::mix(self.paper(), ink, t.tint[3] * 0.6) } else { self.paper() };
                self.marquee(scene, strong, sb.x + self.px(30.0), y + self.px(19.0), sb.w - self.px(60.0), &name.caps(), hot, bg, hover_key("mast", 1));
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
            let text = self.dateline().caps();
            let dy = if !bar || masthead { y - self.px(6.0) } else { y };
            let dl_hot = Rect::new(sb.x, dy, sb.w, self.px(16.0)).contains(mx, my) && vis;
            let bg = self.paper();
            self.marquee(scene, dl, sb.x + self.px(12.0), dy + self.px(11.0), sb.w - self.px(24.0), &text, dl_hot, bg, hover_key("dateline", 0));
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
                    let bg = if hot || self.win_menu { crate::surface::mix(self.paper(), ink, t.tint[3] * 0.6) } else { self.paper() };
                    self.marquee(scene, label, x + self.px(30.0), y + self.px(19.0), cw - self.px(48.0), &name.caps(), hot, bg, hover_key("mast", 2));
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
                let c = fill.map(|f| self.on_fill(f)).unwrap_or(ink);
                let isz = self.px(12.0);
                let ix = x + self.px(if bar && !masthead { 10.0 } else { 12.0 });
                self.fonts.draw_icon(scene, nus_render::text::icons::PLUS, isz, ix, y + self.px(19.0) - isz + self.px(2.0), c);
                self.fonts.draw(scene, Style { color: c, ..strong }, ix + isz + self.px(6.0), y + self.px(19.0), "NEW TAB");
                if !bar || masthead {
                    let k = key("T", false);
                    let kw = self.fonts.measure(label, &k);
                    self.fonts.draw(scene, Style { color: Theme::with_alpha(c, 0.6), ..label }, cell.right() - self.px(12.0) - kw, y + self.px(19.0), &k);
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
                    self.fonts.draw_icon(scene, nus_render::text::icons::CARET_DOWN, csz, cc.x + ((cc.w - csz) / 2.0).round(), y + ((rh - csz) / 2.0).round(), if chot || self.kinds_menu { self.on_fill(ink) } else { t.dim });
                    self.side_hits.push((cc, SideHit::Kinds));
                }
            }
            y += rh;
            scene.hline(sb.x, y, sb.w, self.px(m::STRUCTURE), ink);
        }
    }

    /// The window list and the kinds fan-out, drawn last so they sit over the rows.
    pub(crate) fn draw_sidebar_menus(&mut self, scene: &mut Scene, sb: Rect) {
        let t = self.theme.clone();
        let ink = t.ink;
        let label = self.label();
        let strong = self.label_strong();
        let row = self.px(30.0);
        let (mx, my) = self.mouse;
        let top = sb.y + self.side_header_h();
        if self.win_menu || self.win_anim.active() {
            let k = self.win_anim.value();
            let me = u64::from(self.window.id());
            let n = self.windows.len().max(1) + 2;
            let h = n as f32 * row * k;
            let r = Rect::new(sb.x, top, sb.w, h);
            scene.layer(Some(r));
            scene.rect(r, t.paper);
            let mut y = top;
            let entries = if self.windows.is_empty() { Arc::new(vec![crate::windows::Entry { id: me, name: self.window_name(), tabs: self.tabs.len(), ordinal: self.ordinal, colour: self.container_colour() }]) } else { self.windows.clone() };
            for (i, e) in entries.iter().enumerate() {
                let cell = Rect::new(sb.x, y, sb.w, row);
                let hot = cell.contains(mx, my);
                if hot {
                    scene.rect(cell, t.tint);
                }
                let sq = self.px(10.0);
                scene.rect(Rect::new(sb.x + self.px(12.0), y + ((row - sq) / 2.0).round(), sq, sq), if e.id == me { e.colour } else { Theme::with_alpha(e.colour, 0.55) });
                let base = y + self.px(19.0);
                let st = if e.id == me { strong } else { label };
                let tabs = format!("{} TAB{}", e.tabs, if e.tabs == 1 { "" } else { "S" });
                let tw = self.fonts.measure(label, &tabs);
                let mut right = sb.right() - self.px(12.0);
                if e.id == me {
                    let csz = self.px(12.0);
                    self.fonts.draw_icon(scene, nus_render::text::icons::CHECK, csz, right - csz, y + ((row - csz) / 2.0).round(), ink);
                    right -= csz + self.px(8.0);
                }
                self.fonts.draw(scene, Style { color: t.dim, ..label }, right - tw, base, &tabs);
                let text = self.fit(st, &e.name.caps(), right - tw - self.px(8.0) - (sb.x + self.px(30.0)));
                self.fonts.draw(scene, st, sb.x + self.px(30.0), base, &text);
                scene.hline(sb.x, y + row - self.px(m::HAIRLINE), sb.w, self.px(m::HAIRLINE), Theme::with_alpha(ink, 0.18));
                if self.win_menu {
                    self.side_hits.push((cell, if e.id == me { SideHit::Window } else { SideHit::WinFront(i) }));
                }
                y += row;
            }
            for (icon, text, key, hit) in [
                (nus_render::text::icons::PENCIL, "RENAME", "SHIFT+F2", SideHit::Rename),
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
        if self.dl_menu || self.dl_anim.active() {
            // Downloads uses the shared modal, drawn above all panes.
        }
        if self.tab_menu.is_some() || self.tab_menu_anim.active() {
            self.draw_tab_menu(scene, sb);
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
            let page_first = self.behavior.lead == crate::settings::Lead::Browser;
            if page_first {
                let cell = Rect::new(sb.x, y, sb.w, row);
                if cell.contains(mx, my) {
                    scene.rect(cell, t.tint);
                }
                let isz = self.px(12.0);
                self.fonts.draw_icon(scene, nus_render::text::icons::GLOBE, isz, sb.x + self.px(11.0), y + ((row - isz) / 2.0).round(), ink);
                self.fonts.draw(scene, strong, sb.x + self.px(30.0), y + self.px(19.0), "PAGE");
                let k2 = "CTRL L";
                let kw = self.fonts.measure(label, k2);
                self.fonts.draw(scene, Style { color: t.dim, ..label }, sb.right() - self.px(12.0) - kw, y + self.px(19.0), k2);
                scene.hline(sb.x, y + row - self.px(m::HAIRLINE), sb.w, self.px(m::HAIRLINE), Theme::with_alpha(ink, 0.18));
                if self.kinds_menu {
                    self.side_hits.push((cell, SideHit::KindPage));
                }
                y += row;
            }
            for (i, p) in profiles.iter().enumerate() {
                let cell = Rect::new(sb.x, y, sb.w, row);
                let hot = cell.contains(mx, my);
                if hot {
                    scene.rect(cell, t.tint);
                }
                let sq = self.px(10.0);
                let ctx = TabCtx { kind: "terminal", index: self.tabs.len(), profile: &p.name, space: &self.space_name, space_signal: self.surface.signal, theme: if self.theme.mode == nus_render::Mode::Ink { "ink" } else { "paper" }, host: "", parent: None, tab_colours: &self.tab_colours, shell_color: self.shell_color(Some(crate::shell_colors::slot_for_folder(&p.name))) };
                let color = self.rules.new_tab(&ctx).signal.unwrap_or(self.surface.signal);
                scene.rect(Rect::new(sb.x + self.px(12.0), y + ((row - sq) / 2.0).round(), sq, sq), color);
                let st = if i == self.behavior.default_profile && !page_first { strong } else { label };
                self.fonts.draw(scene, st, sb.x + self.px(30.0), y + self.px(19.0), &p.name.caps());
                if i == self.behavior.default_profile && !page_first {
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
            if !page_first {
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
            }
            scene.hline(sb.x, top + h - self.px(m::STRUCTURE), sb.w, self.px(m::STRUCTURE), ink);
            scene.layer(None);
        }
    }

    /// A chosen tab colour's paper: mostly the theme's black or white.
    pub(crate) fn tab_tint(mode: nus_render::Mode, c: nus_render::Color) -> nus_render::Color {
        let dark = mode == nus_render::Mode::Ink;
        crate::surface::mix(c, if dark { [0.0, 0.0, 0.0, 1.0] } else { [1.0, 1.0, 1.0, 1.0] }, 0.88)
    }

    /// The tab's menu: name, icon, a row of colours, pin, close.
    fn draw_tab_menu(&mut self, scene: &mut Scene, sb: Rect) {
        let t = self.theme.clone();
        let ink = t.ink;
        let paper = self.paper();
        let label = self.label();
        let k = self.tab_menu_anim.value();
        let Some((i, top)) = self.tab_menu.or(self.tab_menu_last) else { return };
        if i >= self.tabs.len() {
            return;
        }
        let live = self.tab_menu.is_some();
        let (mx, my) = self.mouse;
        let row = self.px(30.0);
        // A tile row when there's a selection to tile with, or a tiling to leave.
        let others: Vec<usize> = self.selected.iter().copied().filter(|&k| k != i && k < self.tabs.len()).collect();
        let tile_row: Option<String> = if !others.is_empty() {
            Some(format!("TILE {}", (others.len() + 1).min(4)))
        } else if self.is_tiled(i) {
            Some("UNTILE".into())
        } else {
            None
        };
        let page = matches!(self.tabs[i].left, Pane::Web(_));
        let h_full = row * (4.0 + if tile_row.is_some() { 1.0 } else { 0.0 } + if page { 1.0 } else { 0.0 }) + self.px(40.0);
        let h = h_full * k;
        // Rises from under the row; flips up when there's no room below.
        let y0 = if top + h_full > sb.bottom() - self.px(m::FOOT_H) { top - row - h_full } else { top };
        let r = Rect::new(sb.x + self.px(6.0), y0, sb.w - self.px(12.0), h);
        scene.layer(Some(r));
        scene.rect(Rect::new(r.x + self.px(3.0), r.y + self.px(3.0), r.w, r.h), fade(ink, 0.6));
        scene.rect(r, paper);
        scene.outline(r, self.px(m::STRUCTURE), ink);
        let mut y = y0 + self.px(2.0);
        let mut item = |me: &mut Self, scene: &mut Scene, icon: (&'static str, &'static str), text: &str, key: &str, hit: SideHit, y: f32| {
            let cell = Rect::new(r.x, y, r.w, row);
            let hot = cell.contains(mx, my) && live;
            if hot {
                scene.rect(cell, t.tint);
            }
            let isz = me.px(12.0);
            me.fonts.draw_icon(scene, icon, isz, r.x + me.px(12.0), y + ((row - isz) / 2.0).round(), ink);
            me.fonts.draw(scene, label, r.x + me.px(32.0), y + me.px(19.0), text);
            if !key.is_empty() {
                let kw = me.fonts.measure(label, key);
                me.fonts.draw(scene, Style { color: t.dim, ..label }, r.right() - me.px(12.0) - kw, y + me.px(19.0), key);
            }
            if live {
                me.side_hits.push((cell, hit));
            }
        };
        item(self, scene, nus_render::text::icons::TAG, "RENAME", "F2", SideHit::TabRename(i), y);
        y += row;
        item(self, scene, nus_render::text::icons::SMILEY, "ICON", "", SideHit::TabIcon(i), y);
        y += row;
        // Colours: none, then the swatches.
        let sq = self.px(14.0);
        let mut cx = r.x + self.px(12.0);
        let cy = y + ((self.px(40.0) - sq) / 2.0).round();
        let cur = self.tabs[i].tint;
        let none = Rect::new(cx, cy, sq, sq);
        scene.outline(none, self.px(1.0), ink);
        self.fonts.draw_icon(scene, nus_render::text::icons::CLOSE, self.px(9.0), none.x + self.px(2.5), none.y + self.px(2.5), ink);
        if live {
            self.side_hits.push((Rect::new(none.x - 2.0, y, sq + 6.0, self.px(40.0)), SideHit::TabColour(i, 0)));
        }
        cx += sq + self.px(8.0);
        for (k, &(_, c)) in crate::surface::SWATCHES.iter().enumerate() {
            let sw = Rect::new(cx, cy, sq, sq);
            scene.rect(sw, c);
            if cur == Some(c) {
                scene.outline(Rect::new(sw.x - 2.0, sw.y - 2.0, sq + 4.0, sq + 4.0), self.px(1.5), ink);
            }
            if live {
                self.side_hits.push((Rect::new(sw.x - 2.0, y, sq + 6.0, self.px(40.0)), SideHit::TabColour(i, k + 1)));
            }
            cx += sq + self.px(8.0);
        }
        y += self.px(40.0);
        if let Some(text) = &tile_row {
            item(self, scene, nus_render::text::icons::TILES, text, if text == "UNTILE" { "" } else { "CTRL+SHIFT+D" }, SideHit::TabTile(i), y);
            y += row;
        }
        if page {
            item(self, scene, nus_render::text::icons::FOLDER_SIMPLE, "SAVE TO FOLDER", "", SideHit::TabFolder(i), y);
            y += row;
        }
        let pinned = self.tabs[i].pinned;
        item(self, scene, nus_render::text::icons::PIN, if pinned { "UNPIN" } else { "PIN" }, "", SideHit::TabPin(i), y);
        y += row;
        item(self, scene, nus_render::text::icons::CLOSE, "CLOSE", "CTRL+SHIFT+W", SideHit::TabClose(i), y);
        scene.layer(None);
        if live {
            self.tab_menu_last = Some((i, top));
        }
    }

    pub(crate) fn open_win_menu(&mut self) {
        self.kinds_menu = false;
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

    pub(crate) fn open_look_menu(&mut self) {
        self.win_menu = false;
        self.kinds_menu = false;
        self.look_menu = true;
        self.look_anim.replay(0.0, 1.0, self.motion.dur(160.0));
        self.dirty = true;
    }

    /// A quick change to the look, saved like any setting.
    pub(crate) fn quick_look(&mut self, q: Quick) {
        use crate::settings::Hit;
        match q {
            Quick::Shuffle => {
                let cur = self.surface.signal;
                let list = crate::surface::SWATCHES;
                let i = list.iter().position(|&(_, c)| c == cur).map(|i| (i + 1) % list.len()).unwrap_or(0);
                let c = list[i].1;
                self.apply_setting(Hit::Signal(c), 0.0);
            }
            Quick::Rotate => {
                let ink = self.theme.ink;
                let turn = |c: nus_render::Color| {
                    let (h, s, l) = crate::surface::to_hsl(c);
                    crate::surface::from_hsl((h + 1.0 / 12.0) % 1.0, s, l, 1.0)
                };
                let stops: Vec<nus_render::Color> = self.surface.ramp(ink).iter().map(|&c| turn(c)).collect();
                self.surface.signal = turn(self.surface.signal);
                self.surface.stops = stops;
                self.refresh_icon();
                self.rebuild_theme();
            }
            Quick::Flip => {
                let ink = self.theme.mode == nus_render::Mode::Ink;
                self.apply_setting(Hit::Theme(Some(!ink)), 0.0);
            }
            Quick::Texture => {
                let all = crate::surface::TextureKind::ALL;
                let i = all.iter().position(|&k| k == self.surface.texture_kind).map(|i| (i + 1) % all.len()).unwrap_or(0);
                self.apply_setting(Hit::TexKind(all[i]), 0.0);
                if self.surface.texture <= 0.0 {
                    self.surface.texture = 0.12;
                }
            }
            Quick::Reset => {
                let all = crate::themes::all();
                if let Some(t) = all.iter().find(|t| t.name == self.preset_name).or(all.first()).cloned() {
                    self.apply_theme(&t);
                }
            }
        }
        self.play_event("toggle");
        self.save_prefs();
        self.dirty = true;
    }

    pub(crate) fn close_menus(&mut self) {
        if self.tab_menu.is_some() {
            self.tab_menu = None;
            self.tab_menu_anim.go(0.0, self.motion.dur(100.0));
        }
        if self.dl_menu {
            self.dl_menu = false;
            self.dl_anim.go(0.0, self.motion.dur(100.0));
        }
        if self.look_menu {
            self.look_menu = false;
            self.look_anim.go(0.0, self.motion.dur(100.0));
        }
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
    /// Every settings row as a palette label, with where it lives.
    pub(crate) fn settings_rows_for_palette(&self) -> Vec<(String, Action)> {
        use crate::settings::{LOOK_TABS, SECTIONS};
        let mut out = Vec::new();
        for (sec, (name, _)) in SECTIONS.iter().enumerate() {
            out.push((format!("settings · {name}"), Action::SettingsAt(sec, None)));
            if sec == crate::settings::SEC_LOOK {
                for (k, tab) in LOOK_TABS.iter().enumerate() {
                    out.push((format!("settings · {name} · {tab}"), Action::SettingsAt(sec, Some(k))));
                }
                continue;
            }
            for (label, _) in self.settings_labels(sec) {
                if !label.is_empty() {
                    out.push((format!("settings · {name} · {label}"), Action::SettingsAt(sec, None)));
                }
            }
        }
        out
    }

    /// Open settings on a section, and on a LOOK tab when given.
    pub(crate) fn open_settings_at(&mut self, sec: usize, tab: Option<usize>) {
        if crate::private::enabled() { self.notice(nus_render::text::icons::EYE_SLASH, "Not In Incognito", "open Settings in a regular nus window"); return; }
        self.open_settings();
        if sec == crate::settings::SEC_ASSISTANTS { self.assistants.refresh(self.behavior.assistants.clone()); }
        if let Some(t) = tab {
            self.look_tab = t;
        }
        if let Some(Pane::Settings(p)) = self.tabs.get_mut(self.active).map(|t| &mut t.left) {
            p.section = sec;
            p.scroll = 0.0;
            p.drill = true;
        }
        self.dirty = true;
    }

    /// Run a chain: each step through the palette, in order. Pages and
    /// shells the chain opens are remembered so "tile" can tile them and
    /// "run" reaches the newest shell.
    pub(crate) fn run_chain(&mut self, name: &str) {
        let Some((_, steps)) = self.rules.chains().into_iter().find(|(n, _)| n == name) else { return };
        let mut opened: Vec<u64> = Vec::new();
        for step in steps {
            let step = step.trim();
            let lower = step.to_lowercase();
            if let Some(url) = lower.strip_prefix("open ") {
                let url = step[step.len() - url.len()..].trim();
                let (url, _) = self.url_or_search(url);
                self.open_url(&url, true);
                opened.push(self.tabs[self.active].id);
            } else if let Some(cmd) = lower.strip_prefix("run ") {
                let cmd = step[step.len() - cmd.len()..].trim().to_string();
                // The newest shell the chain opened, else the focused one.
                if let Some(i) = opened.iter().rev().filter_map(|id| self.tabs.iter().position(|t| t.id == *id)).find(|&i| matches!(self.tabs[i].left, Pane::Term(_))) {
                    self.activate(i);
                }
                self.run_in_shell(&cmd);
            } else if lower == "tile" {
                let idx: Vec<usize> = opened.iter().filter_map(|id| self.tabs.iter().position(|t| t.id == *id)).collect();
                if idx.len() >= 2 {
                    self.selected = idx.iter().copied().collect();
                    self.activate(idx[0]);
                    self.tile_selected();
                }
            } else if lower == "new terminal" || lower == "new shell" {
                self.new_tab(self.behavior.default_profile);
                opened.push(self.tabs[self.active].id);
            } else {
                let rows = self.palette_rows(PaletteMode::Go, step);
                if let Some(r) = rows.first() {
                    let a = r.action.clone();
                    if matches!(a, Action::Chain(_)) {
                        continue; // no chains from chains
                    }
                    self.run(a);
                }
            }
        }
        self.dirty = true;
    }

    /// The focused folder's journal as a Broadsheet page beside the shell.
    pub(crate) fn open_journal_page(&mut self) {
        let Some(cwd) = self.focused_cwd() else { return };
        let entries = crate::journal::entries(&cwd, 500);
        let html = crate::journal::page_html(&cwd, &entries, &self.theme, self.surface.signal);
        match crate::journal::write_page(&html) {
            Some(p) => {
                let url = format!("file:///{}", p.display().to_string().replace('\\', "/"));
                self.open_url(&url, false);
            }
            None => self.notice_problem("Could Not Write Log Page", ""),
        }
    }

    pub(crate) fn open_settings(&mut self) {
        if crate::private::enabled() { self.notice(nus_render::text::icons::EYE_SLASH, "Not In Incognito", "open Settings in a regular nus window"); return; }
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

    /// An existing tour marker means the introduction has been shown.
    /// Unfinished optional exercises must not override the saved start page.
    fn onboarded() -> bool {
        if crate::private::enabled() { return true; }
        if std::env::var_os("NUS_ONBOARD").is_some() {
            return false;
        }
        if std::path::Path::new("profile/onboarding-pending").exists() {
            // A new copy of a version already welcomed: no tour this time.
            // Its previous-install offer stays, in Welcome from Settings.
            if !crate::install::version_welcomed() { return false; }
            let _ = std::fs::remove_file("profile/onboarding-pending");
            if !App::onboarded_marker().is_file() {
                let _ = std::fs::write(App::onboarded_marker(), b"skip");
            }
            return true;
        }
        let s = std::fs::read_to_string(App::onboarded_marker()).unwrap_or_default();
        let marker = s.trim();
        marker == "skip" || (marker.len() == 5 && marker.bytes().all(|b| b == b'0' || b == b'1'))
    }

    fn load_hints() -> [bool; 5] {
        let s = std::fs::read_to_string(App::onboarded_marker()).unwrap_or_default();
        let mut h = [false; 5];
        for (k, c) in s.trim().chars().take(5).enumerate() {
            h[k] = c == '1';
        }
        h
    }

    pub(crate) fn save_hints(&self) {
        let s: String = self.hints.iter().map(|&h| if h { '1' } else { '0' }).collect();
        let _ = std::fs::create_dir_all(App::onboarded_marker().parent().unwrap());
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
        crate::install::complete();
        self.previous_install = None;
        if !self.hints.iter().all(|&h| h) {
            let _ = std::fs::write(App::onboarded_marker(), b"skip");
        }
        for t in self.tabs.iter_mut() {
            if matches!(t.right, Some(Pane::Hints(_))) {
                t.right = None;
                t.focus_right = false;
            }
        }
        // As a tab: close it, back to the last tab.
        if let Some(i) = self.tabs.iter().position(|t| matches!(t.left, Pane::Hints(_))) {
            if self.tabs.len() > 1 {
                self.tabs.remove(i);
                self.tab_removed(i);
                let next = self.mru.first().copied().unwrap_or(0).min(self.tabs.len() - 1);
                self.activate(next);
            } else {
                let id=self.tabs[i].id;self.pins.live.retain(|_,live|*live!=id);
                self.tabs[i].pinned=false;
                self.tabs[i].left = Pane::Home(crate::home::HomePane::new());
                self.active = i;
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

    pub(crate) fn draw_pane(&mut self, scene: &mut Scene, pane: &mut Pane, n: &str, focused: bool, look: &Overrides, split: bool) {
        // Native page zoom changes content, not window controls or browser DPI.
        let scale=self.scale;
        if matches!(pane,Pane::Home(_)|Pane::Settings(_)|Pane::Hints(_)|Pane::Downloads(_)|Pane::Ports(_)) {self.scale*=self.ui_zoom as f32/100.0;}
        self.draw_pane_content(scene,pane,n,focused,look,split);
        self.scale=scale;
    }

    fn draw_pane_content(&mut self, scene: &mut Scene, pane: &mut Pane, n: &str, focused: bool, look: &Overrides, split: bool) {
        let t = self.theme.clone();
        let ink = t.ink;
        let label = self.label();
        let strong = self.label_strong();
        match pane {
            Pane::Settings(p) => {
                self.draw_settings(scene, p);
            }
            Pane::Hints(p) => {
                let r = p.rect;
                let scroll = p.scroll;
                let reach = self.draw_welcome(scene, r, scroll);
                self.welcome_reach = reach;
                let moving = self.gliding(crate::scrolling::Glider::Welcome(self.drawing_tab));
                self.draw_thumb(scene, r, scroll, reach, moving);
            }
            Pane::Home(h) => self.draw_home(scene, h, focused),
            Pane::Editor(p) => {
                let r = p.rect;
                self.draw_editor(scene, p, r, focused);
            }
            Pane::Downloads(p) => {
                self.download_ui.reach = self.draw_downloads(scene, p.rect, p.scroll, false);
                p.scroll = p.scroll.min(self.download_ui.reach);
                let (r, scroll, reach) = (p.rect, p.scroll, self.download_ui.reach);
                let moving = self.gliding(crate::scrolling::Glider::Downloads(self.drawing_tab));
                self.draw_thumb(scene, r, scroll, reach + r.h, moving);
            }
            Pane::Ports(p) => {
                let r = p.rect;
                let t = self.theme.clone();
                scene.rect(r, t.paper);
                self.board.rect = r;
                self.draw_board(scene, r, false);
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
                    x += self.fonts.draw(scene, strong, x, base, &p.title.caps()) + self.px(14.0);
                    if let Some(host) = p.tunnel() {
                        x += self.draw_tunnel_tag(scene, &host, x, base) + self.px(14.0);
                    }
                    let dims = format!("{}×{}", p.term.cols(), p.term.rows());
                    let dw = self.fonts.measure(label, &dims);
                    let dx = r.right() - self.px(m::HEADER_PAD_X) - dw;
                    self.fonts.draw(scene, label, dx, base, &dims);
                    self.fonts.draw_icon(scene, nus_render::text::icons::EXPAND, isz, dx - isz - self.px(6.0), base - isz + self.px(2.0), t.dim);
                    let _ = x;
                    scene.hline(r.x, r.y + hh - self.px(m::HAIRLINE), r.w, self.px(m::HAIRLINE), ink);
                }
                if p.replay && self.timeline.as_ref().is_some_and(|tl|!tl.snapshot){self.draw_timeline_ruler(scene,p,r);return;}
                if let Some(proc_name) = p.confirm_close.clone() {
                    let drop = self.band_anim.value();
                    let bh = self.header_h();
                    let cr = Rect::new(r.x, r.y + hh - (1.0 - drop) * bh, r.w, bh);
                    scene.layer(Some(Rect::new(r.x, r.y + hh, r.w, bh)));
                    scene.rect(cr, ink);
                    let inv = Style { color: t.paper, ..strong };
                    let inv_l = Style { color: t.paper, ..self.label() };
                    let by = cr.y + self.px(m::HEADER_PAD_Y) + self.px(m::UI_PX) - self.px(3.0);
                    let mut x = cr.x + self.px(m::HEADER_PAD_X);
                    x += self.fonts.draw(scene, inv, x, by, &format!("{} IS RUNNING", proc_name.caps())) + self.px(14.0);
                    x += self.fonts.draw(scene, inv_l, x, by, "CLOSE ANYWAY?") + self.px(14.0);
                    x += self.fonts.draw(scene, inv, x, by, "ENTER") + self.px(14.0);
                    if p.pty.held_id().is_some() {
                        x += self.fonts.draw(scene, inv_l, x, by, "· ") ;
                        x += self.fonts.draw(scene, inv, x, by, "D") + self.px(6.0);
                        x += self.fonts.draw(scene, inv_l, x, by, "DETACHES · IT KEEPS RUNNING") + self.px(14.0);
                    }
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
                // In a tunnel the page takes the tunnel's hue, faintly; the
                // rails and the tag below say where it goes.
                let tunnel = p.tunnel().map(|h| { let c = self.tunnel_color(&h); (h, c) });
                if let Some((_, c)) = &tunnel {
                    scene.rect(clip, fade(*c, 0.08));
                }
                let pane_paper = look.bg.unwrap_or(self.paper());
                let look = self.cursor_look(p, focused, look.signal);
                let gliding = !matches!(self.cursor.motion, crate::settings::CursorMotion::Jump) && (p.cur_x.active() || p.cur_y.active() || p.smear.animating);
                let mut lk = look;
                if gliding {
                    lk.visible = false;
                }
                // The timeline: the scratch term stands in for the live one while drawing.
                let swapped = p.replay && self.timeline.is_some();
                if swapped {
                    if let Some(tl) = self.timeline.as_mut() {
                        std::mem::swap(&mut p.term, &mut tl.term);
                        p.view_key = None;
                    }
                }
                let view = p.view().to_vec();
                p.tend_program();
                p.grid.policy = self.pane_policy(&p.program, &p.cwd);
                p.grid.draw_view(scene, &mut self.fonts, &p.term, p.origin, focused, lk, &view);
                if look.visible && focused && !matches!(self.cursor.motion, crate::settings::CursorMotion::Jump) {
                    self.draw_moving_cursor(scene, p, look);
                }
                self.draw_term_images(scene, p);
                self.draw_block_layer(scene, p, r);
                self.draw_cutoff(scene, p, r);
                self.draw_prompt_line(scene, p, pane_paper);
                self.draw_blocks(scene, p, r, hh);
                self.draw_term_overlays(scene, p, r, hh, focused, split);
                self.draw_link_band(scene, p, r, hh);
                if let Some((_, c)) = &tunnel {
                    // Rails: a heavy rule down each side and a hairline
                    // across the top, like a page set in a box.
                    let (rail, hair) = (self.px(3.0), self.px(m::HAIRLINE));
                    scene.rect(Rect::new(clip.x, clip.y, rail, clip.h), *c);
                    scene.rect(Rect::new(clip.right() - rail, clip.y, rail, clip.h), *c);
                    scene.rect(Rect::new(clip.x, clip.y, clip.w, hair), *c);
                }
                let _ = p.term.grid_mut().take_damage();
                if swapped {
                    if let Some(tl) = self.timeline.as_mut() {
                        std::mem::swap(&mut p.term, &mut tl.term);
                        p.view_key = None;
                    }
                    self.draw_timeline_ruler(scene, p, r);
                }
                scene.layer(None);
                if p.ask.is_some() {
                    self.draw_ask(scene, p, Rect::new(r.x, r.y + hh, r.w, r.h - hh), focused);
                }
            }
            Pane::Web(p) => {
                let r = p.rect;
                let s = p.tab.shared.borrow();
                let (url, bind, loading) = (s.url.clone(), p.still.clone().or_else(|| s.bind.clone()), s.loading);
                let media_n = s.media.iter().filter(|mm| !mm.blob).count();
                let media_any = !s.media.is_empty();
                drop(s);
                let local = is_local(&url);
                if !p.bare {
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
                let dw = isz * 3.0 + self.px(28.0) + if media_any { isz + self.px(14.0) } else { 0.0 };
                let bug_x = r.right() - self.px(14.0) - isz;
                let book_x = bug_x - self.px(14.0) - isz;
                let gear_x = book_x - self.px(14.0) - isz;
                // Media on the page: the download icon, lit when there is a file to save.
                if media_any {
                    let dl_x = gear_x - self.px(14.0) - isz;
                    let hr = Rect::new(dl_x - self.px(6.0), r.y + self.px(6.0), isz + self.px(12.0), self.px(22.0));
                    let c = if media_n > 0 { ink } else { t.dim };
                    self.icon_button(scene, nus_render::text::icons::DOWNLOAD, isz, dl_x, iy, c, hr, hover_key("media", if media_n > 0 { 0 } else { 1 }), IconMotion::Still);
                }
                {
                    let host = crate::sites::host_of(&url);
                    let tuned = !crate::sites::prefs(&host).is_default();
                    if p.site_panel {
                        scene.rect(Rect::new(gear_x - self.px(6.0), r.y + self.px(6.0), isz + self.px(12.0), self.px(22.0)), ink);
                        self.fonts.draw_icon(scene, nus_render::text::icons::SETTINGS, isz, gear_x, iy, t.paper);
                    } else {
                        self.fonts.draw_icon(scene, nus_render::text::icons::SETTINGS, isz, gear_x, iy, if tuned { self.surface.signal } else { ink });
                    }
                }
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
                if local {
                    crate::page_signal::plot(scene, field, self.scale, ink, t.paper, true);
                } else {
                    scene.outline(field, self.px(m::HAIRLINE), ink);
                }
                let shown = self.fit(ui, url.trim_start_matches("https://").trim_start_matches("http://").trim_end_matches('/'), field.w - self.px(16.0));
                let small = Style { px: self.px(12.0), ..ui };
                self.fonts.draw(scene, small, field.x + self.px(8.0), base, &shown);
                scene.hline(r.x, p.page.y - self.px(m::HAIRLINE), r.w, self.px(m::HAIRLINE), ink);
                }
                // Page — or the reader set over it.
                scene.rect(p.page, t.page);
                if let Some(reader) = p.reader.as_mut() {
                    let rf = self.reader_fonts();
                    let paper = self.paper();
                    reader.draw(scene, &mut self.fonts, &rf, p.page, self.scale, ink, t.dim, paper, self.surface.signal);
                } else if let Some(bind) = bind {
                    // Stretched past its end, the page moves and the pane shows behind it.
                    let drawn = Rect::new(p.page.x, p.page.y - p.bounce_y, p.page.w, p.page.h);
                    scene.texture(drawn, bind, Some(p.page));
                    scene.layer(None);
                    if p.still.is_some() {
                        // Then, not now: a wash of paper over the still.
                        scene.rect(p.page, fade(self.paper(), 0.16));
                    }
                }
                if local {
                    crate::page_signal::plot(scene, p.page, self.scale, ink, t.paper, false);
                }
                let _ = loading;
                self.draw_load_bar(scene, p.page, p, look.signal);
                self.draw_web_overlays(scene, p);
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
                if !p.bare {
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
                let asleep = p.asleep.is_some();
                let status = self.behavior.status;
                use crate::settings::Status;
                if let Some(rw) = reading.as_deref() {
                    // Reader: the book and the count, whatever the status style.
                    let lw = self.fonts.measure(label, rw);
                    rx -= lw;
                    self.fonts.draw(scene, label, rx, base, rw);
                    rx -= isz + self.px(6.0);
                    self.fonts.draw_icon(scene, nus_render::text::icons::BOOK_TEXT, isz, rx, base - isz + self.px(2.0), ink);
                } else if status != Status::None {
                    let word = if asleep { "ASLEEP" } else if loading { "LOADING" } else if local { "LOCAL" } else { "LIVE" };
                    if matches!(status, Status::Word | Status::Both) {
                        let lw = self.fonts.measure(label, word);
                        rx -= lw;
                        self.fonts.draw(scene, Style { color: if asleep { t.dim } else { ink }, ..label }, rx, base, word);
                        rx -= self.px(8.0);
                    }
                    if matches!(status, Status::Lamp | Status::Both) {
                        // The lamp: a 7px dot. Loading breathes in the signal; live is
                        // ink; local has the four Plot corners; asleep is hollow.
                        let d = self.px(7.0);
                        rx -= d;
                        let lr = Rect::new(rx, base - d + self.px(1.0), d, d);
                        if asleep {
                            scene.push(nus_render::Instance::stroke(lr, d / 2.0, self.px(1.0), t.dim, None, 0.0));
                        } else if loading {
                            let k = 0.55 + 0.45 * (crate::clock::since(self.started).as_secs_f32() * 4.0).sin().abs();
                            scene.push(nus_render::Instance::rounded(lr, d / 2.0, fade(self.surface.signal, k)));
                            if let Some((mode,input))=&self.palette {
                self.palette_sel=self.palette_sel.min(self.palette_rows(*mode,input).len().saturating_sub(1));
            }
            self.dirty = true;
                        } else if local {
                            crate::page_signal::plot(scene, lr, self.scale, ink, t.paper, true);
                        } else {
                            scene.push(nus_render::Instance::rounded(lr, d / 2.0, ink));
                        }
                    }
                }
                }
            }
        }
    }

    /// Text that fits draws as is; text that doesn't marquees when `live`
    /// (the row is active or hovered) and otherwise clips under a fade to
    /// the background — never an ellipsis in the sidebar. `key` keeps the
    /// marquee's phase across frames.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn marquee(&mut self, scene: &mut Scene, style: Style, x: f32, base: f32, max_w: f32, text: &str, live: bool, bg: nus_render::Color, key: u64) {
        let w = self.fonts.measure(style, text);
        if w <= max_w {
            self.fonts.draw(scene, style, x, base, text);
            return;
        }
        let over = w - max_w;
        let clip = Rect::new(x, base - style.px * 1.4, max_w, style.px * 2.0);
        scene.layer(Some(clip));
        let off = if live && !self.motion.reduced() {
            // Crawl: pause, slide left over the overflow, pause, slide back.
            let speed = 28.0 * self.motion.register.max(0.2); // px/s, slower when cinematic
            let slide = over / speed;
            let period = slide * 2.0 + 2.4;
            let t = (crate::clock::since(self.started).as_secs_f32() + (key % 7) as f32 * 0.37) % period;
            self.dirty = true;
            if t < 1.2 { 0.0 } else if t < 1.2 + slide { (t - 1.2) / slide * over } else if t < 2.4 + slide { over } else { over - (t - 2.4 - slide) / slide * over }
        } else {
            0.0
        };
        self.fonts.draw(scene, style, x - off, base, text);
        // The fade: background over the tail (and the head while scrolled).
        let fw = self.px(18.0).min(max_w / 3.0);
        let clear = [bg[0], bg[1], bg[2], 0.0];
        if off < over - 0.5 {
            scene.push(nus_render::Instance::rounded_stops(Rect::new(x + max_w - fw, clip.y, fw, clip.h), 0.0, &[clear, bg], 0.0, 0.0, false));
        }
        if off > 0.5 {
            scene.push(nus_render::Instance::rounded_stops(Rect::new(x, clip.y, fw, clip.h), 0.0, &[bg, clear], 0.0, 0.0, false));
        }
        scene.layer(None);
    }

    pub(crate) fn fit(&self, style: Style, text: &str, max_w: f32) -> String {
        self.fit_by(|t| self.fonts.measure(style, t), text, max_w)
    }

    /// `fit` for text drawn with `draw_as_is`, keeping its own case.
    pub(crate) fn fit_as_is(&self, style: Style, text: &str, max_w: f32) -> String {
        self.fit_by(|t| self.fonts.measure_as_is(style, t), text, max_w)
    }

    fn fit_by(&self, measure: impl Fn(&str) -> f32, text: &str, max_w: f32) -> String {
        // Padding arithmetic can lose a fraction of a pixel. An exact-fit
        // label should not lose its last letters to an ellipsis.
        if measure(text) <= max_w + 0.01 {
            return text.to_string();
        }
        // The longest prefix that fits with an ellipsis: width grows with
        // the prefix, so bisect on the character count.
        let chars: Vec<char> = text.chars().collect();
        let (mut lo, mut hi) = (0usize, chars.len());
        while lo < hi {
            let mid = (lo + hi).div_ceil(2);
            let s: String = chars[..mid].iter().collect();
            if measure(&format!("{s}…")) <= max_w {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        let s: String = chars[..lo].iter().collect();
        format!("{s}…")
    }

    /// What typed words become: the address they are, or a search for
    /// them at the prompt's engine (PROMPT · SEARCH ENGINE).
    pub(crate) fn url_or_search(&self, input: &str) -> (String, String) {
        let q = input.trim();
        if q.contains("://") {
            (q.to_string(), format!("open {q}"))
        } else if q.contains('.') && !q.contains(' ') || q.starts_with("localhost") {
            (format!("http://{q}"), format!("open {q}"))
        } else {
            (
                self.behavior.prompt.search_url(q),
                format!("search \u{201c}{q}\u{201d}"),
            )
        }
    }

    pub(crate) fn palette_rows_raw(&self, mode: PaletteMode, input: &str) -> Vec<PaletteRow> {
        if crate::private::enabled() && mode != PaletteMode::Application { return self.private_rows(input); }
        let q = input.trim().to_lowercase();
        if mode==PaletteMode::Go {
            if let Some(query)=q.strip_prefix("reading:") {
                return self.reading_actions().into_iter().filter(|(label,_)|label.to_lowercase().contains(query.trim())).map(|(text,hit)|PaletteRow{num:String::new(),text,action:Action::ReadingControl(hit)}).collect();
            }
        }
        let hit = |s: &str| q.is_empty() || s.to_lowercase().contains(&q);
        let mut rows = Vec::new();
        let row = |num: &str, text: String, action: Action| PaletteRow { num: num.into(), text, action };
        match mode {
            PaletteMode::Application => {
                for item in crate::application_menu::ITEMS {
                    let text = format!("{} · {}", item.group, item.label);
                    if hit(&text) && self.command_enabled(item.command) { rows.push(row("", text, Action::Application(item.command))); }
                }
            }
            PaletteMode::Go => {
                if hit("reading list reading list saved articles") {rows.push(row("", "Reading list".into(), Action::Library));}
                if hit("save to reading list offline article") {rows.push(row("", "Save to reading list".into(), Action::SaveReading));}
                if hit("refresh saved reading copy from the open original") {rows.push(row("", "Refresh saved reading copy from the open original".into(), Action::RefreshReading));}
                if hit("incognito private new window") {
                    rows.push(row("◌", "New incognito window".into(), Action::NewPrivateWindow));
                }
                if hit("help report bug feedback") {
                    rows.push(row("?", "Report a bug · GitHub draft".into(), Action::Report(crate::support::Kind::Bug)));
                }
                if hit("help request feature feedback") {
                    rows.push(row("+", "Request a feature · GitHub draft".into(), Action::Report(crate::support::Kind::Feature)));
                }
                if hit("menu") { rows.push(row("", "application menu · File, Edit, View, Window, Help (F10)".into(), Action::OpenPalette(PaletteMode::Application))); }
                if hit("fold") || hit("collapse") || hit("stacks") {
                    rows.push(row("▾", "fold every stack · again unfolds (ctrl+shift+-)".into(), Action::FoldAll));
                }
                if hit("welcome") || hit("help") || hit("tour") {
                    rows.push(row("?", "welcome · the tour of nus (F1)".into(), Action::Welcome));
                }
                if hit("home") || hit("prompt") {
                    rows.push(row("»", "home · the prompt, a terminal with no shell behind it".into(), Action::Home));
                }
                if hit("copy") || hit("url") || hit("address") {
                    rows.push(row("⧉", "copy this page's url (ctrl+shift+c on a page)".into(), Action::CopyUrl));
                }
                if let Some(u) = q.strip_prefix("home ").map(str::trim).filter(|u| !u.is_empty()) {
                    rows.push(row("⌂", format!("home page · {u} · opens at launch"), Action::SetHome(u.to_string())));
                }
                // First, so Enter on `pin …` pins; the address keeps its case.
                let typed = input.trim();
                if let Some(u) = typed.get(4..).filter(|_| q.starts_with("pin ")).map(str::trim).filter(|u| crate::prompt::pinnable(u)) {
                    rows.insert(0, row("⌖", format!("pin to sidebar · {u}"), Action::PinUrl(u.to_string())));
                } else if q == "pin" {
                    rows.insert(0, row("⌖", "pin this tab · or type an address, or another tab's name, after pin".into(), Action::PinActive));
                }
                // Any open tab, not just this one: `pin` lists them, `pin mail` narrows.
                if let Some(rest) = q.strip_prefix("pin").filter(|r| r.is_empty() || r.starts_with(' ')).map(str::trim) {
                    let pinned: Vec<u64> = self.pins.live.values().copied().collect();
                    let open = self.tabs.iter().enumerate().filter(|(i, t)| *i != self.active && !t.pinned && !pinned.contains(&t.id)).filter_map(|(_, t)| {
                        let title = t.title();
                        let url = match &t.left { Pane::Web(w) => w.asleep.clone().unwrap_or_else(|| w.tab.shared.borrow().url.clone()), _ => String::new() };
                        let hay = format!("{} {}", title.to_lowercase(), url.to_lowercase());
                        (rest.is_empty() || hay.contains(rest)).then(|| row("⌖", format!("pin tab · {title}"), Action::PinTab(t.id)))
                    }).take(8);
                    let at = rows.iter().take_while(|r| matches!(r.action, Action::PinUrl(_) | Action::PinActive)).count();
                    for (k, r) in open.enumerate() { rows.insert(at + k, r); }
                }
                if q.starts_with("pane") || q.starts_with("layout") {
                    for (text, action) in self.pane_rows(&hit) {
                        rows.push(row("▦", text, action));
                    }
                }
                if q.starts_with("pane") || q.starts_with("move") || q.starts_with("window") {
                    for (text, action) in self.send_rows(&hit) {
                        rows.push(row("⇱", text, action));
                    }
                }
                if hit("launch tabs") || hit("startup") {
                    rows.push(row("⇥", "set this window as the launch tabs".into(), Action::SetLaunchTabs));
                }
                if hit("timeline") || hit("replay") || hit("then") {
                    rows.push(row("↺", "session history · browse the command map (ctrl+shift+h)".into(), Action::Timeline));
                }
                if hit("share") || hit("replay") {
                    rows.push(row("↗", "share this tab as a replay · one html file, the real cells".into(), Action::ShareReplay));
                }
                // The journal: `log` lists what ran in this folder, across restarts.
                if let Some(cwd) = self.focused_cwd() {
                    let rest = q.strip_prefix("log").map(|r| r.trim().to_string());
                    if let Some(rest) = rest {
                        let n = crate::journal::count_since(&cwd, crate::journal::now().saturating_sub(7 * 86_400));
                        rows.push(row("≡", format!("log · {n} commands here this week · open the page"), Action::JournalPage));
                        for e in crate::journal::entries(&cwd, 200).into_iter().filter(|e| rest.is_empty() || e.cmd.to_lowercase().contains(&rest)).take(8) {
                            let end = match e.exit { Some(0) => "ok".to_string(), Some(c) => format!("exit {c}"), None => String::new() };
                            rows.push(row("↻", format!("{} · {} · {} · {end}", e.cmd.trim(), crate::journal::when(e.start), crate::journal::took(e.ms)), Action::RunInShell(e.cmd.trim().to_string())));
                        }
                    }
                }
                // Recent commands from the focused shell: run again.
                if let Some(tab) = self.tabs.get(self.active) {
                    if let Pane::Term(t) = &tab.left {
                        let mut seen = std::collections::HashSet::new();
                        let cmds: Vec<String> = t.term.marks.iter().rev().filter(|m| m.kind == nus_vt::MarkKind::CommandStart).map(|m| t.term.command_text(m)).filter(|c| !c.is_empty() && seen.insert(c.clone())).take(5).collect();
                        for c in cmds {
                            if hit(&c) {
                                rows.push(row("↻", format!("{c} · run again"), Action::RunInShell(c.clone())));
                            }
                        }
                    }
                }
                for (i, t) in self.tabs.iter().enumerate() {
                    if hit(&t.title()) {
                        rows.push(row(&self.tab_label(i), format!("{} · switch to tab", t.title()), Action::SwitchTab(i)));
                    }
                }
                if q.is_empty() || hit("ports") || hit("board") {
                    rows.push(row("::", format!("ports · the board · {}", key("P", true)), Action::Board));
                }
                if let Some(pg) = self.focused_block_page() {
                    if q.is_empty() || hit("block") || hit("markdown") || hit("gist") {
                        rows.push(row("</>", "this block · copy as markdown".into(), Action::BlockMarkdown(pg.clone())));
                        rows.push(row("</>", "this block · gist through gh".into(), Action::BlockGist(pg)));
                    }
                }
                if hit("sync") {
                    rows.push(row("::", "sync now".into(), Action::SyncNow));
                    rows.push(row("::", "sync · show this device's key".into(), Action::SyncKey));
                    rows.push(row("::", "sync · join with a key from another device".into(), Action::OpenPalette(PaletteMode::SyncJoin)));
                    rows.push(row("::", "sync · the folder".into(), Action::OpenPalette(PaletteMode::SyncFolder)));
                    rows.push(row("::", "sync · the git remote".into(), Action::OpenPalette(PaletteMode::SyncGit)));
                }
                if hit("tidy") || hit("group") || hit("clean") {
                    rows.push(row("::", "tidy · suggest groups for these tabs".into(), Action::Tidy));
                }
                if q.is_empty() || hit("layout") || hit("workspace") || hit("session") {
                    rows.push(row("::", "save this window as a layout".into(), Action::OpenPalette(PaletteMode::SaveLayout)));
                    for (name, path) in crate::layout_file::saved() {
                        if q.is_empty() || hit(&name) || hit("layout") {
                            rows.push(row("::", format!("layout · {name}"), Action::OpenLayout(path.display().to_string())));
                        }
                    }
                }
                if hit("hatch") || hit("quick") {
                    rows.push(row("::", format!("hatch · the quick terminal · {}", self.behavior.hatch_hotkey.label().to_lowercase()), Action::Hatch));
                    rows.push(row("::", format!("hoist this tab into the hatch · {}", if cfg!(target_os = "macos") { "⌘⌥↑" } else { "CTRL+SHIFT+ALT+↑" }), Action::Hoist));
                }
                for p in &self.ports {
                    let label = format!("port {} · {}", p.port, if p.process.is_empty() { "?" } else { &p.process });
                    if hit(&label) || q == "local" {
                        rows.push(row("::", format!("{label} → open localhost:{} in the split", p.port), Action::OpenInPane(format!("http://localhost:{}/", p.port))));
                    }
                }
                let actions: [(String, Action); 22] = [
                    (format!("new terminal tab · {}", key("T", true)), Action::NewTerminal(self.behavior.default_profile)),
                    (format!("new browser tab · {} then a URL", key("T", true)), Action::NewBrowser(String::new())),
                    (format!("split with a browser · {}", key("D", true)), Action::ToggleSplit),
                    (format!("tile the selected tabs · {} with rows selected", key("D", true)), Action::Tile),
                    ("untile · one tab in the content again".to_string(), Action::Untile),
                    ("keep the peek · CTRL+ENTER · into the stack".to_string(), Action::KeepPeek),
                    (format!("compact sidebar · {} · icons only", key("B", true)), Action::Compact),
                    (format!("{} · CTRL+SHIFT+F11 · the page alone in the window", if self.focus { "leave focus" } else { "focus" }), Action::Focus),
                    (format!("ask · {} · a question beside this shell, commands back", key("?", true)), Action::Ask),
                    ("swap tiles · CTRL+ALT+SHIFT+→".to_string(), Action::TileSwap),
                    (format!("close tab · {}", key("W", true)), Action::CloseTab),
                    (format!("sidebar · {}", key("S", true)), Action::ToggleSidebar),
                    ("downloads · progress and saved files".into(), Action::Downloads),
                    (format!("files · the tree of this window's folder · {}", key("E", true)), Action::ToggleFiles),
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
                    (format!("pin video to this shell · {}", if cfg!(target_os = "macos") { "⌘⌥K plays or pauses" } else { "Ctrl+Alt+K plays or pauses" }), Action::DockVideo),
                ];
                for (label, a) in actions {
                    if hit(&label) {
                        rows.push(row("·", label, a));
                    }
                }
                // Containers: switch this window's, reopen the page in one, make one.
                if !q.is_empty() && (q.starts_with("container") || "containers".contains(q.as_str()) || q == "jar") {
                    let page = self.tabs.get(self.active).is_some_and(|t| matches!(t.left, Pane::Web(_)));
                    for c in self.containers.clone() {
                        let on = c.name == self.container;
                        rows.push(row(if on { "●" } else { "○" }, format!("container {} · {}", c.name, if on { "this window's · new pages open here" } else { "make it this window's" }), Action::Container(c.name.clone())));
                        if page && !on {
                            rows.push(row("↻", format!("reopen this page in {}", c.name), Action::ReopenIn(c.name.clone())));
                        }
                    }
                    let name = q.trim_start_matches("container").trim();
                    if !name.is_empty() && !self.containers.iter().any(|c| c.name.eq_ignore_ascii_case(name)) {
                        rows.push(row("+", format!("new container “{}” · its own cookies and sign-ins", name.caps()), Action::NewContainer(name.to_string())));
                    }
                }
                // Folder items, by title or folder name.
                if !q.is_empty() {
                    for (fi, f) in self.folders.iter().enumerate() {
                        for (k, it) in f.items.iter().enumerate() {
                            if hit(&it.title) || hit(&f.name) || hit(&it.detail) {
                                rows.push(row("▸", format!("{} · {} · {}", f.name, it.title, it.detail), Action::OpenItem(fi, k)));
                            }
                        }
                    }
                }
                // Chains from rules.luau: "chain <name>" or the name.
                for (name, steps) in self.rules.chains() {
                    let label = format!("chain {name} · {}", steps.join(" → "));
                    if hit(&label) || q == "chains" {
                        rows.push(row("»", label, Action::Chain(name.clone())));
                    }
                }
                // Settings rows: every section and its rows, by name.
                if !q.is_empty() {
                    for (r, a) in self.settings_rows_for_palette() {
                        if hit(&r) {
                            rows.push(row("⚙", r, a));
                        }
                    }
                }
                if !q.is_empty() {
                    self.query_rows(input, &mut rows, false);
                }
            }
            PaletteMode::Preference(field) => return self.preference_rows(field,input),
            PaletteMode::Assistant(id) => return self.assistant_prompt_rows(id,input),
            PaletteMode::PromptPin => return vec![PaletteRow {num:"+".into(),text:format!("Save shortcut · {input}"),action:Action::PromptPin(input.trim().into())}],
            PaletteMode::SavedEdit(i) => return vec![row("+", format!("Save command · {input}"), Action::SavedEdit(i,input.into()))],
            PaletteMode::SavedName(i) => return vec![row("+", format!("Name · {input}"), Action::SavedName(i,input.into()))],
            PaletteMode::New => {
                let browser_first = self.behavior.lead == crate::settings::Lead::Browser;
                // Browser first: the address leads, and an empty Enter is the atlas.
                if browser_first {
                    if q.is_empty() {
                        rows.push(row("→", "new page · the atlas: recent pages and shells, or type an address".into(), Action::Start));
                    } else {
                        self.query_rows(input, &mut rows, true);
                        rows.extend(self.history_rows(input, true, 5));
                    }
                }
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
                if !browser_first {
                    if !q.is_empty() {
                        rows.extend(self.history_rows(input, true, 5));
                    }
                    if q.is_empty() {
                        rows.push(row("→", "browser · type a URL or search terms".into(), Action::NewBrowser(String::new())));
                    } else {
                        self.query_rows(input, &mut rows, true);
                    }
                }
            }
            PaletteMode::Url => {
                if !q.is_empty() {
                    self.query_rows(input, &mut rows, false);
                    rows.extend(self.history_rows(input, false, 6));
                } else {
                    rows.extend(self.history_rows("", false, 8));
                }
            }
            PaletteMode::Rename => {
                if q.is_empty() {
                    rows.push(row("·", format!("name this window · now “{}” · empty = automatic", self.window_name()), Action::RenameWindow(String::new())));
                } else {
                    rows.push(row("→", format!("call this window “{q}”"), Action::RenameWindow(q.to_string())));
                }
            }
            PaletteMode::SyncFolder => {
                if q.is_empty() {
                    rows.push(row("·", "a folder your OS already syncs (iCloud Drive, OneDrive, Dropbox, a stick)".into(), Action::Noop));
                    if !self.behavior.sync_folder.is_empty() {
                        rows.push(row("×", format!("stop carrying through {}", self.behavior.sync_folder), Action::SyncFolder(String::new())));
                    }
                } else {
                    rows.push(row("→", format!("carry the profile through {q}"), Action::SyncFolder(q.to_string())));
                }
            }
            PaletteMode::SyncGit => {
                if q.is_empty() {
                    rows.push(row("·", "a private git remote (git@github.com:you/nus-profile.git)".into(), Action::Noop));
                    if !self.behavior.sync_git.is_empty() {
                        rows.push(row("×", format!("stop carrying through {}", self.behavior.sync_git), Action::SyncGit(String::new())));
                    }
                } else {
                    rows.push(row("→", format!("carry the profile through {q}"), Action::SyncGit(q.to_string())));
                }
            }
            PaletteMode::SyncJoin => {
                if q.is_empty() {
                    rows.push(row("·", "paste the key from the other device (nus5-…)".into(), Action::Noop));
                } else if nus_sync::decode_key(&q).is_some() {
                    rows.push(row("→", "join with this key".into(), Action::SyncJoin(q.to_string())));
                } else {
                    rows.push(row("·", "that isn't a nus key".into(), Action::Noop));
                }
            }
            PaletteMode::SaveLayout => {
                let n = self.tabs.iter().filter(|t| t.peek.is_none() && !t.hatch).count();
                if q.is_empty() {
                    rows.push(row("·", format!("save this window's {n} tabs as a layout · type a name"), Action::SaveLayout(self.space_name.clone())));
                } else {
                    rows.push(row("→", format!("save as profile/layouts/{q}.nus.luau"), Action::SaveLayout(q.to_string())));
                }
            }
            PaletteMode::RenameTab(i) => {
                let now = self.tabs.get(i).map(|t| t.title()).unwrap_or_default();
                if q.is_empty() {
                    rows.push(row("·", format!("name this tab · now “{now}” · empty = the page's own"), Action::NameTab(i, String::new())));
                } else {
                    rows.push(row("→", format!("call this tab “{q}”"), Action::NameTab(i, q.to_string())));
                }
            }
            PaletteMode::Settings => return self.search_settings(input),
            PaletteMode::Place => {
                let parsed = crate::app::parse_place(&q);
                if q.trim().is_empty() {
                    if self.behavior.place.is_some() {
                        rows.push(row("×", "Clear location · use illustrated skies".into(), Action::SetPlace(None)));
                    }
                    rows.push(row("·", "Enter latitude, longitude — for example 35.2, -106.6".into(), Action::Noop));
                } else if let Some([lat, lon]) = parsed {
                    rows.push(row("→", format!("{:.1}° {} · {:.1}° {}", lat.abs(), if lat >= 0.0 { "N" } else { "S" }, lon.abs(), if lon >= 0.0 { "E" } else { "W" }), Action::SetPlace(Some([lat, lon]))));
                } else {
                    rows.push(row("·", "two numbers: lat, lon".into(), Action::Noop));
                }
            }
            PaletteMode::Folder(i) => {
                for (fi, f) in self.folders.iter().enumerate() {
                    if f.kind == crate::folders::Kind::Plain && hit(&f.name) {
                        rows.push(row("▸", format!("{} · {} saved", f.name, f.items.len()), Action::SaveToFolder(i, fi)));
                    }
                }
                if !q.is_empty() {
                    rows.push(row("+", format!("new folder “{}”", q.caps()), Action::SaveToNewFolder(i, q.to_string())));
                } else if !self.folders.iter().any(|f| f.kind == crate::folders::Kind::Plain) {
                    rows.push(row("+", "new folder · type a name".into(), Action::SaveToNewFolder(i, "SAVED".into())));
                }
            }
            PaletteMode::IconTab(i) => {
                if !q.is_empty() {
                    rows.push(row("→", format!("{q}  as the icon"), Action::IconTab(i, q.chars().take(2).collect())));
                }
                rows.push(row("·", "none · back to the favicon".into(), Action::IconTab(i, String::new())));
                for (e, n) in [("📌", "pin"), ("🔥", "fire"), ("📚", "books"), ("🧪", "lab"), ("🐛", "bug"), ("🎨", "art"), ("🧭", "compass"), ("💬", "chat"), ("✅", "done"), ("⭐", "star"), ("🧰", "tools"), ("🌿", "green"), ("🚀", "ship"), ("🏗", "build"), ("🔒", "private"), ("🎧", "music")] {
                    if q.is_empty() || n.contains(&q.to_lowercase()) {
                        rows.push(row(e, n.to_string(), Action::IconTab(i, e.to_string())));
                    }
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
        let enc = crate::prompt::encode_query;
        let is_url = strict_url(q).is_some() || q.contains("://");
        let (url, text) = self.url_or_search(q);
        if let Some(p) = local_file(q) {
            rows.push(row("</>", format!("{} · open in the editor", p.display()), Action::OpenFile(p.display().to_string())));
        }
        if is_url {
            rows.push(row("→", text, open(url)));
            rows.push(row("?", format!("search “{q}”"), open(self.behavior.prompt.search_url(q))));
        } else {
            rows.push(row("?", text, open(url)));
        }
        // A skill by name: `ask explain`, `ask port 5173`.
        if let Some(rest) = q.strip_prefix("ask ").map(str::trim) {
            let (name, subject) = rest.split_once(' ').map(|(a, b)| (a, b.trim())).unwrap_or((rest, ""));
            for (i, sk) in self.skills.iter().enumerate() {
                if sk.name.starts_with(name) {
                    rows.push(row("*", format!("ask · {} · {}", sk.name, crate::app::fit_cmd(&sk.prompt, 50)), Action::Skill(i, subject.to_string())));
                }
            }
        }
        rows.push(row("*", format!("ask chatgpt “{q}”"), open(format!("https://chatgpt.com/?q={}", enc(q)))));
        rows.push(row("*", format!("ask claude “{q}”"), open(format!("https://claude.ai/new?q={}", enc(q)))));
        for (id,name) in crate::assistants::NAMES.iter().enumerate() {
            rows.push(row("*",format!("Review prompt for {name} · {q}"),Action::AssistantDraft(id as u8,q.into())));
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
            t.line_col = None;
            t.line_ok = true;
            t.armed = Some((crate::clock::now(), crate::finish_work::Origin::NusAction));
        }
        tab.focus_right = false;
    }

    pub(crate) fn open_palette(&mut self, mode: PaletteMode) {
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
        self.palette_scroll = 0.0;
        self.palette_reveal = true;
        self.palette_anim.replay(0.0, 1.0, self.motion.dur(base::PALETTE));
        self.play_event("palette.open");
        self.dirty = true;
    }

    pub(crate) fn run(&mut self, action: Action) {
        if crate::private::enabled() && !crate::private::allows(&action) {
            self.notice(nus_render::text::icons::EYE_SLASH, "Not In Incognito", "this works in a regular nus window");
            return;
        }
        match action {
            Action::SwitchTab(i) => self.activate(i),
            Action::NewTerminal(p) => self.new_tab(p),
            Action::RenameWindow(n) => {
                self.window_named = if n.trim().is_empty() { None } else { Some(n.trim().to_string()) };
                self.register_window();
                self.save_prefs();
                self.dirty = true;
            }
            Action::NewWindow => self.new_window_request = true,
            Action::NewPrivateWindow => {
                if crate::private::enabled() { self.new_window_request = true; }
                else if let Err(e) = crate::private::launch() { self.notice_problem("Could Not Open Incognito", e.to_string()); }
            }
            Action::Report(kind) => self.open_url(&crate::support::issue_url(kind), true),
            Action::Welcome => self.open_welcome(),
            Action::FoldAll => self.fold_all(),
            Action::KeepPeek => self.keep_peek(),
            Action::Compact => self.toggle_compact(),
            Action::Focus => self.toggle_focus(),
            Action::Ask => self.toggle_ask(),
            Action::OpenLayout(p) => {
                if self.behavior.then_layout.is_empty() {
                    if let Some(n) = std::path::Path::new(&p).file_name().and_then(|n| n.to_str()) {
                        self.behavior.then_layout = n.trim_end_matches(".nus.luau").to_string();
                        self.save_prefs();
                    }
                }
                self.open_layout(std::path::Path::new(&p));
            }
            Action::SaveLayout(n) => self.save_layout(&n),
            Action::OpenPalette(m) => self.open_palette(m),
            Action::Tidy => self.open_tidy(),
            Action::Application(command) => self.application_command(command),
            Action::Noop => {}
            Action::SyncNow => self.sync_now(),
            Action::SyncKey => {
                let word = crate::syncui::make_key();
                if let Ok(mut cb) = arboard::Clipboard::new() {
                    let _ = cb.set_text(word.clone());
                }
                self.notice(nus_render::text::icons::COPY, "Copied", format!("sync key · {word}"));
            }
            Action::SyncJoin(word) => {
                crate::syncui::write_key(&word);
                self.notice(nus_render::text::icons::CHECK, "Joined Sync", "this device has the key");
                self.sync_now();
            }
            Action::SyncFolder(p) => {
                self.behavior.sync_folder = p;
                self.save_prefs();
                self.sync_now();
            }
            Action::SyncGit(g) => {
                self.behavior.sync_git = g;
                self.save_prefs();
                self.sync_now();
            }
            Action::DedupeSwitch(here, there) => {
                if there < self.tabs.len() && here < self.tabs.len() {
                    self.selected.clear();
                    self.activate(here);
                    self.close_tabs(true);
                    if let Some(j) = (there < self.tabs.len()).then_some(there) {
                        self.activate(if j > here { j - 1 } else { j });
                    }
                }
            }
            Action::DedupeKeep(id) => {
                self.dedupe_kept.insert(id);
                self.dirty = true;
            }
            Action::Skill(i, subject) => {
                if let Some(t) = self.ask_term() {
                    if t.ask.is_none() {
                        t.ask = Some(crate::ask::Ask::new());
                    }
                    if let Some(a) = t.ask.as_mut() {
                        a.input = subject;
                    }
                }
                self.layout();
                self.ask_send_with(Some(i));
            }
            Action::Container(n) => self.set_container(&n),
            Action::NewContainer(n) => self.new_container(&n),
            Action::ReopenIn(n) => self.reopen_in(&n),
            Action::Chain(name) => self.run_chain(&name),
            Action::JournalPage => self.open_journal_page(),
            Action::Timeline => self.toggle_timeline(),
            Action::Home => self.open_home(),
            Action::Library => self.open_library(),
            Action::SaveReading => self.save_reading(),
            Action::RefreshReading => self.refresh_reading(),
            Action::ReadingControl(hit) => self.library_action(hit),
            Action::PromptShell(cmd) => self.open_prompt_shell(&cmd),
            Action::SavedUse(i, run) => self.saved_use(i, run),
            Action::SavedEdit(i, value) => self.saved_edit(i, false, value),
            Action::SavedName(i, value) => self.saved_edit(i, true, value),
            Action::PromptPin(value) => {if !value.is_empty() && !self.behavior.prompt.saved.contains(&value) {self.behavior.prompt.saved.push(value);self.save_prefs();}},
            Action::Preference(field,value) => self.set_preference(field,&value),
            Action::AssistantDraft(id,prompt) => self.draft_assistant(id,&prompt),
            Action::AssistantStart(id,prompt) => self.start_assistant(id,&prompt),
            Action::SetPlace(p) => {
                self.behavior.place = p;
                self.save_prefs();
                self.art = None;
            }
            Action::CopyUrl => {
                if !self.copy_page_url() {
                    self.toast(nus_render::text::icons::GLOBE, "No Page Focused", "", None);
                }
            }
            Action::ShellAt(cwd) => {
                let profile = self.behavior.default_profile;
                if let Ok(t) = self.new_term_pane_at(false, profile, Some(cwd)) {
                    let tab = self.make_tab(Pane::Term(t), None);
                    self.tabs.push(tab);
                    self.activate(self.tabs.len() - 1);
                }
            }
            Action::AttachHeld(id) => {
                if let Some(info) = self.held_loose().into_iter().find(|i| i.id == id) {
                    self.attach_held(info);
                }
            }
            Action::PinUrl(url) => self.pin_url(&url),
            Action::PinActive => self.pin_tab(self.active),
            Action::PaneUndo => self.pane_undo(),
            Action::PaneRedo => self.pane_redo(),
            Action::Pane(op) => { self.direct(op); }
            Action::SendTo(dest) => self.send_focused(dest),
            Action::PinTab(id) => if let Some(i) = self.tabs.iter().position(|t| t.id == id) { self.pin_tab(i); },
            Action::SetHome(url) => {
                self.behavior.home_url = crate::links::normalize(&url);
                self.behavior.then = crate::settings::Then::HomePage;
                self.save_prefs();
                self.notice(nus_render::text::icons::HOME, "Home Page Set", format!("{} · opens at launch and in new tabs", crate::links::host(&self.behavior.home_url)));
            }
            Action::SetSearch(template) => {
                let template = if template.contains("://") { template } else { format!("https://{template}") };
                self.behavior.prompt.search_url = template;
                self.behavior.prompt.engine = crate::prompt::SearchEngine::Custom;
                self.save_prefs();
                self.notice(nus_render::text::icons::SEARCH, "Search Engine Set", format!("{} · ? and the search row go there", crate::links::host(&self.behavior.prompt.search_url)));
            }
            Action::SetLaunchTabs => {
                self.save_layout("launch");
                self.behavior.then = crate::settings::Then::Layout;
                self.behavior.then_layout = "launch".into();
                self.save_prefs();
                self.notice(nus_render::text::icons::CHECK, "Layout Saved", "Start/New Tab opens it at launch and in new tabs");
            }
            Action::ShareReplay => match self.share_replay(self.active) {
                Ok(p) => {
                    let url = format!("file:///{}", p.display().to_string().replace('\\', "/"));
                    self.open_url(&url, true);
                }
                Err(e) => self.notice_problem("Could Not Share", e.to_string()),
            },
            Action::SaveToFolder(i, fi) => self.save_to_folder(i, fi),
            Action::OpenItem(fi, k) => self.open_item(fi, k),
            Action::SaveToNewFolder(i, name) => {
                let fi = self.new_folder(&name);
                self.save_to_folder(i, fi);
            }
            Action::SettingsAt(sec, tab) => self.open_settings_at(sec, tab),
            Action::SettingsRow(sec, tab, row) => {
                self.open_settings_at(sec,Some(tab));
                self.settings_target=Some((sec,tab,row));
            }
            Action::Tile => self.tile_selected(),
            Action::Untile => self.untile(),
            Action::TileSwap => self.tile_swap(1),
            Action::NameTab(i, n) => {
                if let Some(t) = self.tabs.get_mut(i) {
                    t.name = if n.trim().is_empty() { None } else { Some(n.trim().to_string()) };
                }
                self.save_session();
                self.dirty = true;
            }
            Action::IconTab(i, e) => {
                if let Some(t) = self.tabs.get_mut(i) {
                    t.emoji = if e.trim().is_empty() { None } else { Some(e.trim().to_string()) };
                }
                self.save_session();
                self.dirty = true;
            }
            Action::ColourTab(i, c) => {
                let mode = self.theme.mode;
                if let Some(t) = self.tabs.get_mut(i) {
                    t.tint = c;
                    t.look.signal = c;
                    t.look.bg = c.map(|c| Self::tab_tint(mode, c));
                }
                self.save_session();
                self.dirty = true;
            }
            Action::NewBrowser(url) if url.is_empty() => self.open_palette(PaletteMode::New),
            Action::NewBrowser(url) => {
                self.tick_hint(1);
                self.open_url(&url, true)
            }
            Action::OpenInPane(url) => self.open_url(&url, false),
            Action::OpenFile(p) => self.open_file(std::path::Path::new(&p), false),
            Action::Board => self.open_board(),
            Action::Hatch => self.toggle_hatch(),
            Action::Hoist => self.hoist(),
            Action::BlockMarkdown(u) => {
                if let Some(p) = self.block_pages.get(&u) {
                    if let Ok(mut cb) = arboard::Clipboard::new() {
                        let _ = cb.set_text(p.markdown());
                    }
                    self.notice(nus_render::text::icons::COPY, "Copied", "as Markdown");
                }
            }
            Action::BlockGist(u) => {
                if let Some(p) = self.block_pages.get(&u).cloned() {
                    match crate::blockpage::gist(&p.markdown()) {
                        Ok(url) => {
                            if let Ok(mut cb) = arboard::Clipboard::new() {
                                let _ = cb.set_text(url.clone());
                            }
                            self.notice(nus_render::text::icons::COPY, "Copied", format!("gist · {url}"));
                        }
                        Err(e) => self.notice_problem("Could Not Create Gist", e.to_string()),
                    }
                }
            }
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
            Action::DockVideo => self.dock_any(),
            Action::ShellRadius(d) => {
                self.surface.shell_radius = (self.surface.shell_radius + d).clamp(0.0, 24.0);
                self.layout();
            }
            Action::Downloads => self.open_downloads(),
            Action::ToggleSidebar => self.toggle_sidebar(),
            Action::ToggleFiles => self.toggle_files(),
            Action::OpenFolder(f) => self.open_folder(&f),
            Action::Workspace(f) => {
                let f = f.map(std::path::PathBuf::from);
                let name = f.as_ref().and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()));
                self.bind_workspace(f);
                match name {
                    Some(n) => self.notice(nus_render::text::icons::FOLDER, "Window Bound", n),
                    None => self.notice(nus_render::text::icons::FOLDER, "Window Follows The Shell", ""),
                }
            }
        }
        self.dirty = true;
    }

    // --- input -----------------------------------------------------------

    /// A key as every path in nus reads it. A real key from winit becomes
    /// one of these, and so does a recorder's scripted `key cmd+s`, so a
    /// shot runs the same code a finger does. (winit's own `KeyEvent`
    /// can't be built outside winit, which is why this exists.)
    pub fn key(&mut self, ev: &WKeyEvent) {
        let k = KeyIn {
            physical_key: ev.physical_key,
            logical_key: ev.logical_key.clone(),
            text: ev.text.clone(),
            state: ev.state,
            repeat: ev.repeat,
        };
        self.key_in(&k);
    }

    pub fn key_in(&mut self, ev: &KeyIn) {
        let _key = crate::perf::scope("input_handler");
        let pressed = ev.state == ElementState::Pressed;
        let tip_was_visible = self.tooltips.visible();
        if pressed { self.dismiss_tip(); }
        if self.page_menu_key(ev) { return; }
        // Esc leaves a page's fullscreen, as in any browser; the page is told.
        if pressed && self.mods.is_empty() && matches!(ev.logical_key, WKey::Named(NamedKey::Escape)) {
            if let Some((id, ..)) = self.page_fullscreen {
                if let Some(t) = self.tabs.iter().find(|t| t.id == id) {
                    for p in std::iter::once(&t.left).chain(t.right.as_ref()) {
                        if let Pane::Web(w) = p { w.tab.exit_fullscreen(); }
                    }
                }
                return;
            }
        }
        if pressed && tip_was_visible && matches!(ev.logical_key, WKey::Named(NamedKey::Escape)) {
            self.page_menu_keys.insert(ev.physical_key);
            return;
        }
        if self.library_context_key(ev) { return; }
        // An overlay takes its keys; a dialog's also takes Shift (typing)
        // and ⌘↵ / ⌘V, and lets every other chord through.
        if self.palette.is_none() && self.overlay_key(ev) { return; }
        if self.dock_key(ev) { return; }
        let ctrl = self.mods.control_key();
        let shift = self.mods.shift_key();
        let alt = self.mods.alt_key();
        let sup = self.mods.super_key();
        if self.hotkey_recording {
            if pressed { self.record_hotkey(ev); }
            return;
        }
        if pressed && ev.physical_key == PhysicalKey::Code(KeyCode::KeyN) && (if cfg!(target_os="macos") {sup} else {ctrl}) && shift {
            return self.run(Action::NewPrivateWindow);
        }
        // Private windows contain browser tabs only, so standard browser
        // chords cannot collide with a terminal's input.
        if crate::private::enabled() && pressed && !alt && (if cfg!(target_os="macos") {sup} else {ctrl}) {
            match ev.physical_key {
                PhysicalKey::Code(KeyCode::KeyW) if shift => {
                    let _ = self.proxy.send_event(crate::UserEvent::WindowControl(self.window.id(), 0));
                    return;
                }
                PhysicalKey::Code(KeyCode::KeyW) => return self.close_tabs(false),
                PhysicalKey::Code(KeyCode::KeyT) if !shift => return self.open_start_page(false),
                PhysicalKey::Code(KeyCode::KeyT) => return self.reopen_closed(),
                PhysicalKey::Code(KeyCode::KeyL) => return self.open_palette(PaletteMode::Url),
                _ => {}
            }
        }
        // App chords: ⌘ on macOS, Ctrl+Shift elsewhere — never reaches the shell.
        let app = if cfg!(target_os = "macos") { sup } else { ctrl && shift };
        if !crate::private::enabled() && pressed && self.behavior.hatch_hotkey.matches(ev, self.mods) && self.hotkey.as_ref().is_none_or(|k| !k.status.is_empty()) {
            return self.toggle_hatch();
        }


        if let Some(step) = crate::zoom::shortcut(ev, self.mods) {
            if pressed {
                if self.palette.is_none() && self.start.is_none() && !self.me_card.open
                    && self.splash.is_none() && self.timeline.is_none() && !self.dl_menu
                    && self.library_zoom_key(ev) { return; }
                self.zoom_focused(step);
            }
            return;
        }
        if pressed && ev.physical_key == PhysicalKey::Code(KeyCode::F10) { self.open_palette(PaletteMode::Application); return; }

        if self.timeline.is_some() && self.palette.is_none() {
            if pressed {
                if ctrl && shift && ev.physical_key==PhysicalKey::Code(KeyCode::KeyH){self.close_timeline();}
                else {self.timeline_key(&ev.logical_key);}
            }
            return;
        }

        if self.splash.is_some() {
            if ev.state==winit::event::ElementState::Pressed && matches!(ev.logical_key,winit::keyboard::Key::Named(winit::keyboard::NamedKey::Escape|winit::keyboard::NamedKey::Enter|winit::keyboard::NamedKey::Space)) {
                self.finish_arrival();self.splash=None;self.dirty=true;
            }
            return;
        }
        // The profile card, then the atlas, own the keyboard while open.
        if self.me_key(ev) {
            return;
        }
        if self.start_key(ev) {
            return;
        }
        let downloads_page=self.tabs.get(self.active).is_some_and(|t| matches!(if t.focus_right{t.right.as_ref().unwrap_or(&t.left)}else{&t.left},Pane::Downloads(_)));
        if pressed && (self.dl_menu || downloads_page && self.palette.is_none()) && self.download_key(ev) {return;}
        if pressed && (if cfg!(target_os="macos") {sup} else {ctrl}) && matches!(&ev.logical_key,WKey::Character(c) if c.eq_ignore_ascii_case("f")) && self.palette.is_none() && self.tabs.get(self.active).is_some_and(|t|matches!(t.left,Pane::Settings(_))) {
            self.open_palette(PaletteMode::Settings);return;
        }
        if self.palette.is_none() && !self.dl_menu && self.library_key(ev) { return; }
        if self.palette.is_none() && !app && self.settings_key(ev) { return; }
        // Find and hints take the keys while they're up.
        if self.web_mode_key(ev) {
            return;
        }
        if self.term_mode_key(ev) {
            return;
        }
        if self.prompt_lsp_key(ev) {
            return;
        }
        if self.palette.is_none() && self.blocks_key(ev) {
            return;
        }
        if self.ask_key(ev) {
            return;
        }
        if self.tidy_key(ev) {
            return;
        }
        if self.palette.is_none() && !app && self.board_key(ev) {
            return;
        }
        // Standard editing shortcuts belong to a focused editor even when
        // Command is also the application's shortcut modifier on macOS.
        let editor_chord = (if cfg!(target_os="macos") { sup } else { ctrl }) && !alt
            && matches!(ev.logical_key.to_text().map(str::to_ascii_lowercase).as_deref(), Some("a" | "c" | "x" | "v" | "z" | "y" | "s" | "f"));
        if self.palette.is_none() && (!app || editor_chord) && self.editor_key(ev) {
            return;
        }
        // Launching an assistant: ⌥← and ⌥→ turn the intelligence ring.
        if pressed && alt && matches!(self.palette, Some((PaletteMode::Assistant(_), _))) {
            if let WKey::Named(k @ (NamedKey::ArrowLeft | NamedKey::ArrowRight)) = &ev.logical_key {
                let l = self.intelligence();
                self.set_intelligence(if *k == NamedKey::ArrowLeft { l.saturating_sub(1) } else { l + 1 });
                return;
            }
        }
        if let Some((_, input)) = self.palette.as_mut() {
            if !pressed {
                return;
            }
            self.palette_reveal = true;
            // The palette's own chord again closes it.
            let again = app && matches!(&ev.logical_key, WKey::Character(c) if c.eq_ignore_ascii_case("k") || c.eq_ignore_ascii_case("t") || c.eq_ignore_ascii_case("l"));
            let took = if again { crate::field::Took::No } else { crate::field::edit(input, ev, self.mods, 2000) };
            if took.changed() {
                self.palette_sel = 0;
            }
            if !took.taken() {
                match &ev.logical_key {
                    WKey::Named(NamedKey::Escape) => self.palette = None,
                    WKey::Named(NamedKey::Enter) => self.palette_commit(),
                    // Tab walks the rows like the arrows; Shift+Tab back.
                    WKey::Named(NamedKey::ArrowDown) | WKey::Named(NamedKey::Tab) if !(shift && matches!(ev.logical_key, WKey::Named(NamedKey::Tab))) => {
                        self.palette_sel += 1;
                        self.play_event("palette.move");
                    }
                    WKey::Named(NamedKey::ArrowUp) | WKey::Named(NamedKey::Tab) => {
                        self.palette_sel = self.palette_sel.saturating_sub(1);
                        self.play_event("palette.move");
                    }
                    WKey::Character(_) if again => self.palette = None,
                    _ => {}
                }
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
                    match &ev.logical_key {
                        WKey::Named(NamedKey::Enter) => {
                            t.confirm_close = None;
                            self.close_tabs(true);
                        }
                        WKey::Named(NamedKey::Escape) => t.confirm_close = None,
                        WKey::Character(c) if c.eq_ignore_ascii_case("d") && t.pty.held_id().is_some() => {
                            t.confirm_close = None;
                            self.detach_active();
                        }
                        _ => {}
                    }
                    self.dirty = true;
                    return;
                }
            }
        }

        if pressed && crate::field::command(self.mods) && matches!(ev.logical_key,WKey::Named(NamedKey::Enter)) && self.library_form_key(ev) {return;}

        // Chords match the physical key: with Ctrl held, Windows reports no
        // character for many keys, so the logical key is unreliable here.
        let code = match ev.physical_key {
            PhysicalKey::Code(c) => Some(c),
            _ => None,
        };
        // Pane mode: Ctrl+Alt+P in and out; while it is on, every key is its.
        if pressed && ctrl && alt && code == Some(KeyCode::KeyP) {
            return self.toggle_pane_mode();
        }
        if self.pane_mode {
            if pressed {
                self.pane_mode_key(code, shift);
            }
            return;
        }
        if pressed && code==Some(KeyCode::KeyJ) && (if cfg!(target_os="macos"){sup}else{ctrl}) && !shift {return self.open_downloads();}
        if pressed && code == Some(KeyCode::F11) && !ctrl && !shift {
            return self.toggle_fullscreen();
        }
        if pressed && code == Some(KeyCode::F11) && ctrl && shift {
            return self.toggle_focus();
        }
        if pressed && code == Some(KeyCode::F1) && !ctrl && !shift {
            return self.open_welcome();
        }
        // F2 names the tab; Shift+F2 the window.
        if pressed && code == Some(KeyCode::F2) && !ctrl && self.palette.is_none() {
            return if shift { self.open_palette(PaletteMode::Rename) } else { self.open_palette(PaletteMode::RenameTab(self.active)) };
        }
        if pressed && code == Some(KeyCode::KeyN) && (if cfg!(target_os = "macos") { sup } else { ctrl }) && !shift && self.palette.is_none() {
            return self.run(Action::NewWindow);
        }
        // A peek: Esc closes it, Ctrl+Enter keeps it.
        if pressed && self.peeking().is_some() && self.palette.is_none() {
            if code == Some(KeyCode::Escape) && !ctrl && !shift {
                return self.close_peek();
            }
            if code == Some(KeyCode::Enter) && ctrl && !shift {
                return self.keep_peek();
            }
        }
        // The layout's history: Ctrl+Alt+Z takes the last change back,
        // with Shift it comes again.
        if pressed && ctrl && alt && code == Some(KeyCode::KeyZ) {
            return if shift { self.pane_redo() } else { self.pane_undo() };
        }
        // Tiles: Ctrl+Alt+arrows walk them, with Shift they swap.
        if pressed && ctrl && alt && self.tiling_shown() {
            let dir = match code {
                Some(KeyCode::ArrowLeft) | Some(KeyCode::ArrowUp) => Some(-1),
                Some(KeyCode::ArrowRight) | Some(KeyCode::ArrowDown) => Some(1),
                _ => None,
            };
            if let Some(d) = dir {
                return if shift { self.tile_swap(d) } else { self.tile_focus(d) };
            }
        }
        // The reader: Ctrl+Alt+R (⌘⌥R on macOS), as in Firefox. R with the
        // app chord — ⌘R, Ctrl+Shift+R — is a reload, as everywhere.
        if pressed && code == Some(KeyCode::KeyR) && alt && !shift && (if cfg!(target_os = "macos") { sup } else { ctrl }) {
            return self.toggle_reader();
        }
        // Palette-only mode explicitly makes Ctrl+T and Ctrl+K aliases on
        // Windows/Linux as well as the existing app chords.
        if !cfg!(target_os = "macos") && pressed && ctrl && !alt && !sup
            && self.behavior.then == crate::settings::Then::Palette
            && matches!(code, Some(KeyCode::KeyT) | Some(KeyCode::KeyK)) {
            return self.open_palette(PaletteMode::Go);
        }
        if pressed && app {
            // The shell's copy and paste chords, on the prompt: its line.
            // Select all, too, once there is something on it; on an empty
            // line the chord keeps opening Ask.
            let home_line = self.tabs.get(self.active).and_then(|t| if let Pane::Home(h) = t.focused_ref() { Some(!h.library && !h.input.is_empty()) } else { None });
            if (matches!(code, Some(KeyCode::KeyC) | Some(KeyCode::KeyV) | Some(KeyCode::KeyX)) && home_line.is_some() || code == Some(KeyCode::KeyA) && !shift && home_line == Some(true)) && self.home_key(ev) {
                return;
            }
            match code {
                Some(KeyCode::Minus) if shift => return self.fold_all(),
                // HOIST shares ↑ with walking prompts (⌘↑, the terminal
                // convention), so it takes Alt as well: ⌘⌥↑ / Ctrl+Shift+Alt+↑.
                Some(KeyCode::ArrowUp) if alt => return self.hoist(),
                Some(KeyCode::ArrowUp) => return self.jump_prompt(-1),
                Some(KeyCode::ArrowDown) => return self.jump_prompt(1),
                Some(KeyCode::KeyF) => return self.search_open(),
                Some(KeyCode::KeyO) => {
                    // While a new-port line shows, O opens that port; else hints.
                    if let Some((_, k)) = self.board.toast.clone() {
                        self.board.toast = None;
                        if matches!(self.toast.as_ref().and_then(|t| t.act.as_ref()), Some(crate::toast::Act::OpenPort(_))) {
                            self.toast = None;
                        }
                        self.ports_act(&k, crate::ports::Act::Open);
                        return;
                    }
                    return self.term_hints_open();
                }
                Some(KeyCode::KeyC) => return self.copy_selection_or_output(),
                Some(KeyCode::KeyV) => return self.paste_into_shell(),
                Some(KeyCode::KeyT) => return self.open_start_page(false),
                Some(KeyCode::KeyK) => return self.open_palette(PaletteMode::Go),
                Some(KeyCode::KeyP) => {
                    if self.board.open {
                        self.close_board();
                    } else {
                        self.open_board();
                    }
                    return;
                }

                Some(KeyCode::KeyL) => return self.open_palette(PaletteMode::Url),
                Some(KeyCode::KeyW) => return self.close_tabs(false),
                Some(KeyCode::KeyZ) => return self.reopen_closed(),
                Some(KeyCode::KeyD) => return self.divide(),
                Some(KeyCode::KeyB) => return self.toggle_compact(),
                Some(KeyCode::KeyH) => return self.toggle_timeline(),
                Some(KeyCode::KeyA) | Some(KeyCode::Slash) => return self.toggle_ask(),
                // ⌘R reloads the focused page; with Shift (and Ctrl+Shift+R
                // everywhere) past the cache. A shell keeps its own Ctrl+R.
                Some(KeyCode::KeyR) => return self.reload_page(shift || !cfg!(target_os = "macos")),
                Some(KeyCode::KeyS) => return self.toggle_sidebar(),
                Some(KeyCode::KeyE) => return self.toggle_files(),
                _ => {}
            }
            if let WKey::Named(NamedKey::Enter) = ev.logical_key {
                if self.tabs.get(self.active).is_some_and(|t| matches!(t.focused_ref(), Pane::Term(_))) {
                    if let Some((_, _, url)) = self.detected.clone() {
                        // On macOS this branch handles both advertised chords;
                        // on Windows/Linux only Ctrl+Shift+Enter reaches it.
                        self.open_url(&url, shift || !cfg!(target_os = "macos"));
                    }
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
                Some(KeyCode::Backquote) if self.hotkey.as_ref().is_none_or(|k| !k.status.is_empty()) && self.behavior.hatch_hotkey == crate::hotkey::Chord::CtrlGrave => {
                    // No OS hotkey here: the chord summons the hatch from inside nus.
                    return self.toggle_hatch();
                }
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
                WKey::Named(NamedKey::Home) | WKey::Named(NamedKey::End) if self.focused_term().is_some() => {
                    let up = matches!(ev.logical_key, WKey::Named(NamedKey::Home));
                    let easing = self.behavior.scroll_easing;
                    let motion = self.motion.clone();
                    if let Some(t) = self.focused_term() {
                        let far = t.term.grid().scrollback_len() as f32 + 1.0;
                        crate::scrolling::scroll_shell(t, if up { far } else { -far }, easing, &motion);
                    }
                    self.dirty = true;
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
            self.last_key = crate::clock::now();
            if self.cursor.hide_while_typing && !self.pointer_hidden && matches!(self.tabs.get_mut(self.active).map(|t| t.focused()), Some(Pane::Term(_))) {
                self.window.set_cursor_visible(false);
                self.pointer_hidden = true;
            }
        }
        // Right / End at the end of the line takes the prediction.
        if pressed && !ctrl && !alt && !sup && !shift {
            if let Some(k) = vt_key(&ev.logical_key) {
                if self.accept_prediction(k) {
                    return;
                }
            }
        }
        let easing = self.behavior.scroll_easing;
        let motion = self.motion.clone();
        // The timeline is up: its keys, unless a chord is being pressed.
        if ev.state == ElementState::Pressed && self.timeline.is_some() && !self.mods.control_key() && self.timeline_key(&ev.logical_key) {
            self.dirty = true;
            return;
        }
        // The prompt takes plain keys when it is the focused pane.
        if self.home_key(ev) {
            return;
        }
        // A link waiting on the focused shell: enter opens, esc cancels, d opens and stops asking.
        if ev.state == ElementState::Pressed && self.link_band_key(&ev.logical_key) {
            self.dirty = true;
            return;
        }
        // A hand waiting on the focused page: y/enter allows, n/esc denies, h allows the host; anything else takes over.
        if ev.state == ElementState::Pressed && self.hands_key(&ev.logical_key) {
            self.dirty = true;
            return;
        }
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        let w_right = tab.focus_right && tab.right.is_some();
        match tab.focused() {
            Pane::Settings(_) | Pane::Hints(_) | Pane::Editor(_) | Pane::Ports(_) | Pane::Downloads(_) | Pane::Home(_) => {}
            Pane::Term(t) => {
                // Shift+PgUp / PgDn: a page of scrollback, on the curve
                // (unless the program asked for the keys, as in an alternate screen).
                if pressed && shift && !ctrl && !alt && matches!(ev.logical_key, WKey::Named(NamedKey::PageUp) | WKey::Named(NamedKey::PageDown)) && !t.term.modes().contains(nus_vt::Modes::ALT_SCREEN) {
                    let page = (t.term.rows() as f32 - 1.0).max(1.0);
                    let up = matches!(ev.logical_key, WKey::Named(NamedKey::PageUp));
                    crate::scrolling::scroll_shell(t, if up { page } else { -page }, easing, &motion);
                    self.dirty = true;
                    return;
                }
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

                // A selection in the command being typed edits like one:
                // Backspace or Delete takes it out, a character replaces it.
                // The shell only has a caret, so it is walked to the
                // selection's end and backspaced through it, wherever it was.
                // Once: the selection goes with the key.
                if action == KeyAction::Press && !ctrl && !alt && !sup && matches!(key, Key::Backspace | Key::Delete | Key::Char(_)) {
                    let span = t.sel.as_ref().and_then(|sel| {
                        let (a, b) = sel.bounds(&t.term);
                        t.term.command_selection(a, b)
                    });
                    if let Some((from, to, caret)) = span {
                        let (modes, kb) = (t.term.modes(), t.term.keyboard_mode());
                        let press = |k: Key, m: Mods| input::encode(k, m, KeyAction::Press, modes, kb);
                        let (walk, steps) = if to >= caret { (Key::Right, to - caret) } else { (Key::Left, caret - to) };
                        let mut out = press(walk, Mods::empty()).repeat(steps);
                        out.extend(press(Key::Backspace, Mods::empty()).repeat(to - from));
                        if let Key::Char(_) = key {
                            out.extend(press(key, mods));
                        }
                        if t.term.grid().display_offset != 0 {
                            let off = t.term.grid().display_offset as isize;
                            t.term.grid_mut().scroll_display(-off);
                        }
                        let _ = t.pty.write(&out);
                        t.sel = None;
                        // The typed-line model no longer matches what the shell holds.
                        t.line_ok = false;
                        self.dirty = true;
                        return;
                    }
                }
                if pressed && t.sel.is_some() {
                    t.sel = None;
                }
                // A paste is waiting: Enter sends it, Esc drops it.
                if t.confirm_paste.is_some() && pressed {
                    match key {
                        Key::Enter => {
                            if let Some(text) = t.confirm_paste.take() {
                                t.write_paste(&text);
                            }
                            return;
                        }
                        Key::Escape => {
                            t.confirm_paste = None;
                            return;
                        }
                        _ => {}
                    }
                }
                // The URL-at-a-prompt rule: a whole-line URL at a fresh prompt
                // opens in the split instead of running. Ctrl+Enter runs it.
                if pressed {
                    match (key, ctrl || alt || sup) {
                        (Key::Enter, false) => {
                            // `nus <file>` / `edit <file>` at a prompt: the editor, not the shell.
                            if t.line_ok {
                                if let Some(path) = file_at_prompt(&t.line, t.term.cwd.as_deref()) {
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
                                    t.line_col = None;
                                    t.line_ok = true;
                                    let split = self.behavior.prompt_url != crate::settings::PromptUrl::NewTab;
                                    self.open_file(&path, split);
                                    return;
                                }
                            }
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
                            t.line_col = None;
                                    t.line_ok = true;
                                    let new_tab = self.behavior.prompt_url == crate::settings::PromptUrl::NewTab;
                                    self.open_url(&url, new_tab);
                                    return;
                                }
                            }
                            t.line.clear();
                            t.line_col = None;
                            t.line_ok = true;
                            t.armed = Some((crate::clock::now(), crate::finish_work::Origin::ShellInput));
                        }
                        (Key::Enter, true) => {
                            t.line.clear();
                            t.line_col = None;
                            t.line_ok = true;
                        }
                        (Key::Char(c), false) => {
                            if t.line.is_empty() {
                                t.line_col = Some(t.term.cursor().col);
                            }
                            t.line.push(c);
                        }
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
                    let off = t.term.grid().display_offset as isize;
                    t.term.grid_mut().scroll_display(-off);
                }
                crate::agent::typed(&mut t.agent, &bytes, crate::clock::now());
                t.notice = None;
                let _ = t.pty.write(&bytes);
            }
            Pane::Web(w) if w.focus_devtools && w.devtools.is_some() => {
                if pressed && matches!(ev.logical_key, WKey::Named(NamedKey::F12)) {
                    return self.toggle_devtools();
                }
                let d = w.devtools.as_ref().unwrap();
                for e in crate::webkeys::events(ev, self.mods) {
                    d.key(&e);
                }
            }
            Pane::Web(w) => {
                // Chrome-compatible keys while a browser pane is focused.
                // Chords go by the physical key: with Ctrl held, Windows
                // reports no character for many of them.
                if pressed {
                    match (&ev.logical_key, ctrl, alt) {
                        _ if code == Some(KeyCode::KeyL) && ctrl && !alt => {
                            return self.open_palette(PaletteMode::Url);
                        }
                        _ if code == Some(KeyCode::KeyR) && ctrl && !alt => return if shift { w.tab.reload_ignore_cache() } else { w.tab.reload() },
                        (WKey::Named(NamedKey::F5), _, _) => return if ctrl || shift { w.tab.reload_ignore_cache() } else { w.tab.reload() },
                        _ if code == Some(KeyCode::ArrowLeft) && alt && !ctrl => return self.navigate(w_right, true),
                        _ if code == Some(KeyCode::ArrowRight) && alt && !ctrl => return self.navigate(w_right, false),
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
        let shell_slot = self.new_shell_slot(&left);
        let look = self.look_with(&left, None, shell_slot);
        let mut tab = Tab { id, parent: None, left, right, focus_right: false, pinned: false, last_active: crate::clock::now(), name: None, emoji: None, tint: None, peek: None, hatch: false, split_w: None, solo: false, look, shell_slot };
        Self::fit_palette(&self.theme, &mut tab);
        tab
    }

    /// Ask the rules what a new tab looks like.
    pub(crate) fn look_for(&self, left: &Pane, parent: Option<&Overrides>) -> Overrides {
        self.look_with(left, parent, None)
    }

    /// The same, with a shell stack's family slot offered to the rules as
    /// `ctx.shell_color`.
    pub(crate) fn look_with(&self, left: &Pane, parent: Option<&Overrides>, slot: Option<u8>) -> Overrides {
        let (kind, profile) = match left {
            Pane::Term(t) => ("terminal", self.profiles.get(t.profile).map(|p| p.name.as_str()).unwrap_or("")),
            Pane::Web(_) => ("page", ""),
            Pane::Settings(_) => ("settings", ""),
            Pane::Hints(_) => ("welcome", ""),
            Pane::Home(_) => ("home", ""),
            Pane::Editor(_) => ("editor", ""),
            Pane::Ports(_) => ("ports", ""),
            Pane::Downloads(_) => ("downloads", ""),
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
            tab_colours: &self.tab_colours,
            shell_color: self.shell_color(slot),
        })
    }

    // ── Stacks ─────────────────────────────────────────────────────────
    // A stack is a top-level tab plus the tabs pages opened from it (popups,
    // target=_blank). Children sit right after their parent in `tabs`, show
    // in the sidebar only while the stack is active, and share the parent's
    // number: Ctrl+N goes to whichever member was used last.

    /// Index of the stack's top-level tab.
    /// The top of the tree `i` is in.
    fn stack_root(&self, i: usize) -> usize {
        let mut cur = i;
        for _ in 0..64 {
            match self.tabs[cur].parent {
                Some(pid) => match self.tabs.iter().position(|t| t.id == pid) {
                    Some(p) if p != cur => cur = p,
                    _ => return cur,
                },
                None => return cur,
            }
        }
        cur
    }

    /// How deep `i` sits: 0 at the top.
    pub(crate) fn depth(&self, i: usize) -> usize {
        let mut d = 0;
        let mut cur = i;
        while let Some(pid) = self.tabs[cur].parent {
            match self.tabs.iter().position(|t| t.id == pid) {
                Some(p) if p != cur && d < 64 => {
                    cur = p;
                    d += 1;
                }
                _ => break,
            }
        }
        d
    }

    /// Every descendant of `i`, in tab order.
    pub(crate) fn subtree(&self, i: usize) -> Vec<usize> {
        let mut out = Vec::new();
        let mut frontier = vec![self.tabs[i].id];
        while let Some(id) = frontier.pop() {
            for j in 0..self.tabs.len() {
                if self.tabs[j].parent == Some(id) && !out.contains(&j) {
                    out.push(j);
                    frontier.push(self.tabs[j].id);
                }
            }
        }
        out.sort_unstable();
        out
    }

    /// A row shows unless an ancestor is folded, or its tree isn't the
    /// active one (stacks unfold only while one of their members is active).
    pub(crate) fn row_visible(&self, i: usize) -> bool {
        let mut cur = i;
        while let Some(pid) = self.tabs[cur].parent {
            let Some(p) = self.tabs.iter().position(|t| t.id == pid) else { break };
            if self.collapsed.contains(&pid) {
                return false;
            }
            if p == cur {
                break;
            }
            cur = p;
        }
        self.tabs[i].parent.is_none() || self.stack_open(self.stack_root(i))
    }

    /// Where a dragged row lands: onto the middle of a row nests under it,
    /// the edges reorder at that row's level.
    pub(crate) fn drop_row(&mut self, i: usize, y: f32) {
        let g = self.sidebar_geometry();
        let Some(&(j, ry, rh)) = g.rows.iter().find(|&&(_, ry, rh)| y >= ry && y < ry + rh) else {
            // Below everything: to the top level, at the end.
            if y > g.rows.last().map(|r| r.1 + r.2).unwrap_or(0.0) {
                self.reparent(i, None, None);
            }
            return;
        };
        if j == i {
            return;
        }
        let frac = (y - ry) / rh;
        if (0.3..0.7).contains(&frac) {
            self.reparent(i, Some(j), None);
        } else if frac < 0.3 {
            let parent = self.tabs[j].parent.and_then(|pid| self.tabs.iter().position(|t| t.id == pid));
            self.reparent(i, parent, Some(j));
        } else {
            // After j: before j's next sibling if any, else the parent's end.
            let parent = self.tabs[j].parent.and_then(|pid| self.tabs.iter().position(|t| t.id == pid));
            let sub = self.subtree(j);
            let last = sub.last().copied().unwrap_or(j);
            let next = (last + 1..self.tabs.len()).find(|&k| self.tabs[k].parent == self.tabs[j].parent);
            self.reparent(i, parent, next);
        }
        self.play_event("toggle");
    }

    /// Fold or unfold a node's subtree.
    pub(crate) fn toggle_fold(&mut self, i: usize) {
        let id = self.tabs[i].id;
        if !self.collapsed.remove(&id) {
            self.collapsed.insert(id);
            // Folding away the active tab lands on the fold.
            if self.subtree(i).contains(&self.active) {
                self.activate(i);
            }
        }
        self.layout();
        self.dirty = true;
    }

    /// Fold every tree; a second call unfolds them all.
    pub(crate) fn fold_all(&mut self) {
        let parents: Vec<u64> = (0..self.tabs.len()).filter(|&i| !self.children(i).is_empty()).map(|i| self.tabs[i].id).collect();
        if parents.iter().all(|id| self.collapsed.contains(id)) {
            self.collapsed.clear();
        } else {
            for id in parents {
                self.collapsed.insert(id);
            }
            let root = self.stack_root(self.active);
            self.activate(root);
        }
        self.layout();
        self.dirty = true;
    }

    /// Move tab `i` under `new_parent` (None = top level), placed after
    /// its new siblings; children come along.
    pub(crate) fn reparent(&mut self, i: usize, new_parent: Option<usize>, before: Option<usize>) {
        if let Some(p) = new_parent {
            if p == i || self.subtree(i).contains(&p) {
                return;
            }
        }
        let moving_ids: Vec<u64> = std::iter::once(i).chain(self.subtree(i)).map(|k| self.tabs[k].id).collect();
        let parent_id = new_parent.map(|p| self.tabs[p].id);
        let before_id = before.map(|b| self.tabs[b].id);
        let active_id = self.tabs[self.active].id;
        // Lift the moving subtree out.
        let mut moving: Vec<Tab> = Vec::new();
        let mut rest: Vec<Tab> = Vec::new();
        for t in self.tabs.drain(..) {
            if moving_ids.contains(&t.id) {
                moving.push(t);
            } else {
                rest.push(t);
            }
        }
        if let Some(first) = moving.first_mut() {
            first.parent = parent_id;
        }
        // Where it goes: before `before`, else after the last descendant of the parent, else the end.
        let at = if let Some(bid) = before_id {
            rest.iter().position(|t| t.id == bid).unwrap_or(rest.len())
        } else if let Some(pid) = parent_id {
            // After the parent and everything under it.
            let mut ids = vec![pid];
            let mut last = rest.iter().position(|t| t.id == pid).map(|p| p + 1).unwrap_or(rest.len());
            loop {
                let mut grew = false;
                for (k, t) in rest.iter().enumerate() {
                    if let Some(par) = t.parent {
                        if ids.contains(&par) && !ids.contains(&t.id) {
                            ids.push(t.id);
                            last = last.max(k + 1);
                            grew = true;
                        }
                    }
                }
                if !grew {
                    break;
                }
            }
            last
        } else {
            rest.len()
        };
        let mut tabs = rest;
        for (k, t) in moving.into_iter().enumerate() {
            tabs.insert(at + k, t);
        }
        self.tabs = tabs;
        // Indices moved; rebuild what points at them.
        self.active = self.tabs.iter().position(|t| t.id == active_id).unwrap_or(0);
        self.mru = vec![self.active];
        self.selected.clear();
        self.pip = None;
        self.layout();
        self.dirty = true;
    }

    fn children(&self, root: usize) -> Vec<usize> {
        let id = self.tabs[root].id;
        (0..self.tabs.len()).filter(|&j| self.tabs[j].parent == Some(id)).collect()
    }

    /// Stacks unfold only while one of their members is active.
    fn stack_open(&self, root: usize) -> bool {
        self.stack_root(self.active) == root && !self.collapsed.contains(&self.tabs[root].id)
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
        let mut root = i;
        for _ in 0..64 {
            match tabs[root].parent {
                Some(pid) => match tabs.iter().position(|t| t.id == pid) {
                    Some(p) if p != root => root = p,
                    _ => break,
                },
                None => break,
            }
        }
        let n = (0..tabs.len()).filter(|&j| tabs[j].parent.is_none()).position(|j| j == root).map(|p| p + 1).unwrap_or(0);
        if root == i {
            format!("{n:02}")
        } else {
            // Letters down the branch: 01·b·a.
            let mut chain: Vec<char> = Vec::new();
            let mut cur = i;
            for _ in 0..8 {
                let Some(pid) = tabs[cur].parent else { break };
                let k = (0..tabs.len()).filter(|&j| tabs[j].parent == Some(pid)).position(|j| j == cur).unwrap_or(0);
                chain.push((b'a' + (k % 26) as u8) as char);
                match tabs.iter().position(|t| t.id == pid) {
                    Some(p) if p != cur => cur = p,
                    _ => break,
                }
            }
            chain.reverse();
            let mut s = format!("{n:02}");
            for c in chain {
                s.push('·');
                s.push(c);
            }
            s
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
        let Some(w) = self.new_web_pane(url) else { return };
        self.place_in_stack(source, w);
    }

    /// A page opened from a page nests under it: the tree grows with depth.
    pub(crate) fn place_in_stack(&mut self, source: usize, w: WebPane) {
        let root = source;
        let mut tab = self.make_tab(Pane::Web(w), None);
        tab.parent = Some(self.tabs[root].id);
        let parent_look = self.tabs[root].look.clone();
        tab.look = self.look_for(&tab.left, Some(&parent_look));
        self.collapsed.remove(&self.tabs[root].id);
        let at = self.subtree(root).last().copied().unwrap_or(root) + 1;
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

    /// Windows pages opened (`window.open`, `target=_blank`) that Chromium
    /// has made: each becomes a pane where a link from that page would
    /// go, still joined to its opener.
    fn adopt_popups(&mut self) {
        let mut ready: Vec<(usize, crate::browser::SharedRef, String)> = Vec::new();
        for (i, tab) in self.tabs.iter().enumerate() {
            for p in std::iter::once(&tab.left).chain(tab.right.as_ref()) {
                if let Pane::Web(w) = p {
                    let container = w.container.clone();
                    let mut s = w.tab.shared.borrow_mut();
                    let (now, later): (Vec<_>, Vec<_>) = std::mem::take(&mut s.opened).into_iter().partition(|c| c.borrow().adopted.is_some());
                    s.opened = later;
                    ready.extend(now.into_iter().map(|c| (i, c, container.clone())));
                }
            }
        }
        for (i, shared, container) in ready {
            let viewer = {
                let mut c = shared.borrow_mut();
                c.scale = self.scale;
                c.viewer.clone()
            };
            if let Ok(mut config) = viewer.write() { *config = self.viewer_config(); }
            let Some(tab) = crate::browser::BrowserTab::adopt(shared) else { continue };
            let w = self.pane_for(tab, &container);
            match self.behavior.links {
                crate::settings::Links::Split if self.tabs.get(i).is_some_and(|t| t.right.is_none()) => {
                    self.tabs[i].right = Some(Pane::Web(w));
                    self.tabs[i].focus_right = true;
                    self.activate(i);
                }
                crate::settings::Links::NewTab => {
                    let tab = self.make_tab(Pane::Web(w), None);
                    self.tabs.push(tab);
                    self.activate(self.tabs.len() - 1);
                }
                _ => self.place_in_stack(i, w),
            }
            self.layout();
            self.dirty = true;
        }
    }

    /// Pages Chromium closed on their own account — a popup that closed
    /// itself, a page you chose to leave, a new tab that turned out to be
    /// a download — go, and their pane with them.
    fn take_away_gone_pages(&mut self) {
        let mut gone: Vec<(u64, bool)> = Vec::new();
        for tab in &self.tabs {
            for (right, p) in std::iter::once((false, &tab.left)).chain(tab.right.as_ref().map(|p| (true, p))) {
                if let Pane::Web(w) = p {
                    let mut s = w.tab.shared.borrow_mut();
                    if s.gone || s.download_only {
                        s.download_only = false;
                        gone.push((tab.id, right));
                    }
                }
            }
        }
        for (id, right) in gone {
            let Some(i) = self.tabs.iter().position(|t| t.id == id) else { continue };
            let tab = &mut self.tabs[i];
            if right {
                tab.right = None;
                tab.focus_right = false;
            } else if let Some(r) = tab.right.take() {
                tab.left = r;
                tab.focus_right = false;
            } else if self.tabs.len() > 1 {
                self.selected.clear();
                self.active = i;
                self.close_tabs(true);
                continue;
            } else {
                // The last tab: the prompt takes its place.
                self.tabs[i].left = Pane::Home(crate::home::HomePane::new());
            }
            self.layout();
            self.dirty = true;
        }
    }

    /// Pages that asked for a new window since the last frame.
    pub(crate) fn drain_popups(&mut self) {
        self.adopt_popups();
        self.take_away_gone_pages();
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
        if i != self.active { self.dismiss_tip(); self.close_page_menu(); }
        // The hatch's tab is never the sidebar's active one: a cycle that
        // lands on it steps past.
        if self.tabs[i].hatch {
            let n = self.tabs.len();
            let dir = if i >= self.active { 1 } else { n - 1 };
            let mut j = (i + dir) % n;
            for _ in 0..n {
                if !self.tabs[j].hatch {
                    return self.activate(j);
                }
                j = (j + dir) % n;
            }
            return;
        }
        if self.timeline.as_ref().is_some_and(|t|self.tabs[i].id!=t.tab_id){self.close_timeline();}
        self.wake_tab(i);
        let prev = self.active;
        if let Some(p) = &self.pip {
            if p.tab == i && self.behavior.pip_policy.focus_tab {
                self.pip = None;
            }
        }
        let together = self.is_tiled(prev) && self.is_tiled(i);
        // A video pinned to a shell is already watched; leaving its tab
        // doesn't need a floating window as well.
        let docked_src = self.docked.as_ref().is_some_and(|d| self.tabs.get(prev).is_some_and(|t| t.id == d.src_tab));
        // WebKit's pages (webkit.rs) play in the system's picture in picture.
        if prev != i && !together && self.behavior.pip_policy.leave_tab {
            self.webkit_pip(prev, true);
        }
        if prev != i && self.behavior.pip_policy.focus_tab {
            self.webkit_pip(i, false);
        }
        if prev != i && self.pip.is_none() && !together && !docked_src && self.behavior.pip_policy.leave_tab {
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

    fn trim_memory(&mut self) {
        if crate::clock::since(self.memory_tended).as_secs() < 10 { return; }
        self.memory_tended = crate::clock::now();
        self.trim_language_servers();
        self.hovers.retain(|_, h| h.hot || h.alpha.active() || h.pulse.active() || crate::clock::since(h.since).as_secs() < 30);
        if self.closed.len() > crate::storage::CLOSED_TABS { self.closed.drain(..self.closed.len() - crate::storage::CLOSED_TABS); }
        let urls: std::collections::HashSet<_> = self.tabs.iter().flat_map(|t| std::iter::once(&t.left).chain(t.right.as_ref())).filter_map(|p| match p { Pane::Web(w) => Some(w.tab.shared.borrow().url.clone()), _ => None }).collect();
        self.block_pages.retain(|url, _| urls.contains(url));
        let streams: std::collections::HashSet<_> = self.tabs.iter().flat_map(|t| [crate::replay::stream_id(t.id, false), crate::replay::stream_id(t.id, true)]).collect();
        if let Some(rec) = &mut self.recorder { rec.retain(&streams); }
    }

    /// Keep `mru`/`selected` valid after `tabs[i]` was removed.
    pub(crate) fn tab_removed(&mut self, i: usize) {
        if let Some(p) = self.pip.as_mut() {
            if p.tab == i && self.behavior.pip_policy.focus_tab {
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
    pub(crate) fn toggle_devtools(&mut self) {
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
            if w.devtools.is_some() || w.tab.has_devtools() {
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
            tracing::info!("native devtools requested");
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
        if matches!(mode, PaletteMode::Assistant(_)) && action == Action::Noop {
            self.palette = Some((mode, input));
            self.dirty = true;
            return;
        }
        self.run(action);
    }

    pub(crate) fn open_url(&mut self, url: &str, new_tab: bool) {
        // Chromium's own settings pages do not exist in nus's pages (they
        // load forever); protected content and the rest live in nus's.
        if crate::widevine::is_chrome_settings(url) {
            self.open_settings_at(crate::settings::SEC_BROWSER, None);
            self.notice(nus_render::text::icons::GLOBE, "Browser Settings", "chrome://settings lives here in nus · protected content is under PROTECTED CONTENT");
            return;
        }
        if new_tab {
            if let Some(w) = self.new_web_pane(url) {
                let tab = self.make_tab(Pane::Web(w), None);
                self.tabs.push(tab);
                self.activate(self.tabs.len() - 1);
            }
        } else {
            // The focused page changes its address. A shell gets the page
            // beside it, or a new one on its right.
            let tab = &mut self.tabs[self.active];
            let (left_web, right_web) = (matches!(tab.left, Pane::Web(_)), matches!(tab.right, Some(Pane::Web(_))));
            let to_right = right_web && (tab.focus_right || !left_web);
            if to_right {
                if let Some(Pane::Web(w)) = &tab.right {
                    w.tab.load(url);
                }
                tab.focus_right = true;
            } else if left_web {
                if let Pane::Web(w) = &tab.left {
                    w.tab.load(url);
                }
                tab.focus_right = false;
            } else if let Some(w) = self.new_web_pane(url) {
                let tab = &mut self.tabs[self.active];
                tab.right = Some(Pane::Web(w));
                tab.focus_right = true;
            }
        }
        self.layout();
    }

    /// At quit: every local shell in this window, and everything it
    /// started, stopped over one reading of the process table — so the
    /// app leaves nothing of its own behind. Held shells are not ours to
    /// end; `release_idle_held` decides those.
    pub(crate) fn reap_local_shells(&mut self) {
        let tree = nus_pty::ports::process_tree();
        let all: Vec<usize> = (0..self.tabs.len()).collect();
        // Held shells are not ours to end here; `release_idle_held` has
        // already let go of the idle ones and kept the working ones.
        Self::reap_panes(&mut self.tabs, &all, &tree, false);
    }

    /// Stop the shells in `which` and everything under them: the trees
    /// are signalled together, over one reading of `tree`, and only then
    /// is each shell itself killed. A held shell has no tree of ours —
    /// the holder reaps it on its side — so only the word goes out, and
    /// only when `held` says this is an ending rather than a letting go.
    fn reap_panes(tabs: &mut [Tab], which: &[usize], tree: &std::collections::HashMap<u32, (u32, String)>, held: bool) {
        let mut pids: Vec<u32> = Vec::new();
        for &i in which {
            let Some(tab) = tabs.get(i) else { continue };
            for p in std::iter::once(&tab.left).chain(tab.right.as_ref()) {
                if let Pane::Term(t) = p {
                    pids.extend(t.pty.local_pid());
                }
            }
        }
        nus_pty::ports::reap_trees_in(tree, &pids);
        for &i in which {
            let Some(tab) = tabs.get_mut(i) else { continue };
            for p in std::iter::once(&mut tab.left).chain(tab.right.as_mut()) {
                if let Pane::Term(t) = p {
                    if held || t.pty.held_id().is_none() {
                        t.pty.kill_reaped();
                    }
                }
            }
        }
    }

    /// At quit: kill held shells sitting at a prompt; leave the ones with a
    /// foreground process for the next launch to attach to.
    pub(crate) fn release_idle_held(&mut self) {
        for tab in self.tabs.iter_mut() {
            for p in std::iter::once(&mut tab.left).chain(tab.right.as_mut()) {
                if let Pane::Term(t) = p {
                    if t.pty.held_id().is_some() && t.term.at_prompt() && t.pty.foreground_process().is_none() {
                        t.pty.kill();
                    }
                }
            }
        }
    }

    /// Let the active tab's held shell go on without us: the tab closes,
    /// the holder keeps the process, and reopen-closed (or the atlas) brings
    /// it back.
    pub(crate) fn detach_active(&mut self) {
        let i = self.active;
        let Some(tab) = self.tabs.get(i) else { return };
        let Pane::Term(t) = &tab.left else { return };
        let Some(id) = t.pty.held_id().map(String::from) else { return };
        let Some(info) = nus_pty::hold::Info::read(&Self::hold_dir(), &id) else { return };
        let tab = self.tabs.remove(i);
        self.tile_forget(tab.id);
        self.closed.push(Closed::Held(info));
        self.tab_removed(i);
        self.play_event("tab.close");
        self.notice(nus_render::text::icons::TERMINAL, "Detached", "the shell keeps running; reopen-closed or the atlas brings it back");
        self.dirty = true;
    }

    /// A held shell, back as a tab.
    pub(crate) fn attach_held(&mut self, info: nus_pty::hold::Info) {
        match self.new_term_pane_attached(false, info) {
            Ok(t) => {
                let tab = self.make_tab(Pane::Term(t), None);
                self.tabs.push(tab);
                self.activate(self.tabs.len() - 1);
            }
            Err(e) => self.notice_problem("Could Not Attach", e.to_string()),
        }
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
        self.close_timeline();
        if self.tabs.len()==1 && self.pins.owns(self.tabs[0].id) && !crate::private::enabled() {
            // Closing the last live pinned page leaves its pin and an empty prompt.
            let tab=self.make_tab(Pane::Home(crate::home::HomePane::new()),None);self.tabs.push(tab);
        }
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
        // A parent takes its whole subtree along.
        for i in targets.clone() {
            targets.extend(self.subtree(i));
        }
        targets.sort_unstable();
        targets.dedup();
        if !force {
            if let Some(&root) = targets.iter().find(|&&i| !self.children(i).is_empty()) {
                self.confirm_stack = Some(root);
                self.active = root;
                self.band_anim.replay(0.0, 1.0, self.motion.dur(base::BAND));
                self.dirty = true;
                return;
            }
        }
        if targets.len() >= self.tabs.len() {
            if crate::private::enabled() {
                let _ = self.proxy.send_event(crate::UserEvent::WindowControl(self.window.id(), 0));
                return;
            }
            // Never close the last tab; keep one.
            targets.retain(|&t| t != self.active);
            if targets.is_empty() {
                return;
            }
        }
        // A page with unsaved work gets its say (`beforeunload`): its tab
        // stays, with the leave-page question up, until you answer. Pages
        // with nothing to say start closing here.
        let mut held = None;
        targets.retain(|&i| {
            let Some(tab) = self.tabs.get(i) else { return true };
            let mut ok = true;
            for p in std::iter::once(&tab.left).chain(tab.right.as_ref()) {
                if let Pane::Web(w) = p {
                    if !w.tab.shared.borrow().gone && !w.tab.ask_to_close() {
                        ok = false;
                    }
                }
            }
            if !ok { held = Some(i); }
            ok
        });
        if let Some(i) = held {
            self.active = i;
            self.selected.clear();
            self.dirty = true;
            if targets.is_empty() {
                return;
            }
        }
        // One reading of the process table answers both questions: is
        // there work to warn about, and what has to be stopped.
        let tree = nus_pty::ports::process_tree();
        if !force {
            // Real work, not a bare prompt. A shell at its prompt with
            // nothing under it closes without a word; anything else is
            // named and asked about, in either half of a split.
            let integration = self.behavior.shell_integration;
            let mut ask: Option<(usize, bool, String)> = None;
            'scan: for &i in &targets {
                let Some(tab) = self.tabs.get_mut(i) else { continue };
                for (right, pane) in [(false, Some(&mut tab.left)), (true, tab.right.as_mut())] {
                    let Some(Pane::Term(t)) = pane else { continue };
                    let Some(pid) = t.pty.pid() else { continue };
                    let running = nus_pty::ports::child_name_in(&tree, pid);
                    if integration && t.term.at_prompt() && running.is_none() {
                        continue;
                    }
                    if let Some(name) = running {
                        ask = Some((i, right, name));
                        break 'scan;
                    }
                }
            }
            if let Some((i, right, name)) = ask {
                self.active = i;
                if let Some(tab) = self.tabs.get_mut(i) {
                    tab.focus_right = right && tab.right.is_some();
                    let pane = if tab.focus_right { tab.right.as_mut() } else { Some(&mut tab.left) };
                    if let Some(Pane::Term(t)) = pane {
                        t.confirm_close = Some(name);
                    }
                }
                self.band_anim.replay(0.0, 1.0, self.motion.dur(base::BAND));
                self.dirty = true;
                return;
            }
        }
        self.play_event("tab.close");
        // Closing is terminating: every shell in the targets goes, and so
        // does everything it started. The whole stack is reaped in one
        // pass, so a tab with work in it costs the grace period once.
        // A held shell is the holder's: closing the tab means ending it
        // on purpose, so it goes too.
        Self::reap_panes(&mut self.tabs, &targets, &tree, true);
        for &i in targets.iter().rev() {
            let tab = self.tabs.remove(i);
            self.tile_forget(tab.id);
            self.closed.push(match &tab.left {
                Pane::Term(t) => Closed::Term(t.profile),
                Pane::Web(w) => Closed::Web(w.tab.shared.borrow().url.clone()),
                Pane::Settings(_) | Pane::Hints(_) | Pane::Editor(_) | Pane::Ports(_) | Pane::Downloads(_) | Pane::Home(_) => {
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
            Some(Closed::Held(info)) => self.attach_held(info),
            Some(Closed::Web(url)) => self.open_url(&url, true),
            None => {}
        }
    }

    pub(crate) fn toggle_split(&mut self) {
        let Some(t) = self.tabs.get(self.active) else { return };
        let op = if t.right.is_some() { crate::director::Op::Kill { tab: t.id, right: true } } else { crate::director::Op::Split { tab: t.id } };
        self.direct(op);
    }

    pub fn focus_changed(&mut self, focused: bool) {
        if !focused {
            self.dismiss_tip();
            self.close_page_menu();
            self.page_menu_buttons.clear();
            self.page_menu_keys.clear();
        }
        if !focused {if let Some(tl)=&mut self.timeline{tl.map_drag=None;}}
        self.window_focused = focused;
        self.sidebar_leave=None;
        if focused {self.refresh_pointer(true);} else {self.hover_row=None;}
        self.news_focus(focused);
        if !focused {
            self.pip_away_pending = self.behavior.pip_policy.leave_app.then(crate::clock::now);
        } else {
            self.pip_away_pending = None;
            // A queued automatic request must not arrive after focus returned.
            self.pip_request = None;
            if self.behavior.pip_policy.focus_app { self.close_pip(); self.webkit_pip(self.active, false); }
        }
        self.dirty = true;
    }

    pub fn modifiers(&mut self, m: ModifiersState) {
        self.mods = m;
    }

    /// The held modifiers as the VT crate spells them.
    pub(crate) fn vt_mods(&self) -> Mods {
        let mut mods = Mods::empty();
        mods.set(Mods::SHIFT, self.mods.shift_key());
        mods.set(Mods::CTRL, self.mods.control_key());
        mods.set(Mods::ALT, self.mods.alt_key());
        mods.set(Mods::SUPER, self.mods.super_key());
        mods
    }

    pub(crate) fn refresh_sidebar_hover(&mut self,x:f32,y:f32) {
        if !self.sidebar_pinned() && self.sidebar_hoverable() {
            let c = self.content_rect();
            let sb = self.sidebar_rect();
            let inside = sb.contains(x, y);
            let edge = self.px(6.0);
            let at_edge = if self.sidebar_right() { x <= self.target.size.0 as f32 && x > self.target.size.0 as f32 - edge } else { x >= 0.0 && x < edge };
            // "Inside window" means the pointer must have travelled from inside
            // to the edge; arriving straight from the desktop doesn't count.
            let allowed = match self.sidebar_rules.hover_from {
                HoverFrom::ScreenEdge => true,
                HoverFrom::InsideWindow => self.pointer_inside,
            };
            if !self.sidebar_hover && at_edge && y >= c.y && y < c.bottom() && allowed {
                self.sidebar_hover = true;
                self.play_event("sidebar.reveal");
                self.tick_hint(3);
                self.sidebar_leave = None;
                self.dirty = true;
            } else if self.sidebar_hover {
                if inside {
                    self.sidebar_leave = None;
                } else if self.sidebar_leave.is_none() {
                    self.sidebar_leave = Some(crate::clock::now() + std::time::Duration::from_millis(self.sidebar_rules.grace_ms));
                }
            }
            if !at_edge {
                self.pointer_inside = true;
            }
        }
    }

    pub fn cursor_entered(&mut self) {
        self.refresh_pointer(false);
    }
    fn refresh_pointer(&mut self,focused:bool) {
        if let Some((x,y))=crate::pointer::in_window(&self.window) {
            if x>=0.0 && y>=0.0 && x<self.target.size.0 as f32 && y<self.target.size.1 as f32 {
                self.mouse=(x,y);
                // Focus is an explicit return to this window, including at its edge.
                if focused {self.pointer_inside=true;}
                self.refresh_sidebar_hover(x,y);
            } else {self.cursor_left();}
        } else if self.mouse.0>=0.0 && self.mouse.1>=0.0 {
            if focused {self.pointer_inside=true;}
            self.refresh_sidebar_hover(self.mouse.0,self.mouse.1);
        }
        self.dirty=true;
    }

    pub fn mouse_moved(&mut self, x: f32, y: f32) {
        let was = self.mouse;
        self.mouse = (x, y);
        self.tooltips.motion((x, y));
        // Coalesced by the existing redraw loop. Native controls outside the
        // sidebar also need hover invalidation; no polling continues at rest.
        if was != self.mouse { self.dirty = true; }
        if self.pointer_hidden {
            self.window.set_cursor_visible(true);
            self.pointer_hidden = false;
        }
        self.refresh_sidebar_hover(x,y);
        if self.page_menu_motion(x, y) { return; }
        self.pin_drag_move(x, y);
        if self.timeline_pointer(x,y){self.dirty=true;return;}
        if self.palette.is_none() && !self.me_card.open && self.start.is_none() && self.library_pointer(x,y) { return; }
        if self.sidebar_resize.is_some(){self.sidebar_resize_to(x,y);return;}
        if self.intel_move(x, y) {
            return;
        }
        if let Some(hit) = self.settings_drag {
            self.apply_setting(hit, x);
            return;
        }
        if self.compact() {
            let edge = self.strip_rect().bottom() + self.px(6.0);
            if (was.1 <= edge) != (y <= edge) {
                self.dirty = true;
            }
        }
        if (was.0 - x).abs() + (was.1 - y).abs() > 0.0 {
            let strip = self.strip_rect();
            if strip.contains(x, y) || strip.contains(was.0, was.1) || (self.sidebar_visible() && (self.sidebar_rect().contains(x, y) || self.sidebar_rect().contains(was.0, was.1))) {
                self.dirty = true;
            }
        }
        self.term_drag(x, y);
        self.editor_motion(x, y);
        self.home_drag(x);
        self.dock_moved();
        if let Some((i, off, y0)) = self.drag_armed {
            let x0 = self.row_host.map(|h| h.1).unwrap_or(x);
            if (y - y0).abs() > self.px(4.0) || (x - x0).abs() > self.px(12.0) {
                self.drag_armed = None;
                self.drag = Some((i, off, y));
                // The page shows the tab you were on, to drop this one beside.
                if let Some((h, _)) = self.row_host.filter(|&(h, _)| h != i && h < self.tabs.len()) {
                    self.activate(h);
                }
            }
        }
        if let Some(d) = self.drag.as_mut() {
            d.2 = y;
            self.dirty = true;
        }
        if self.tile_drag.is_some() {
            self.tile_drag_to(x, y);
        }
        if self.split_drag {
            self.split_drag_to(x);
        }
        if self.pane_drag.is_some() || self.near_pane_controls(x, y) || self.near_pane_controls(was.0, was.1) {
            self.dirty = true;
        }
        // A resize arrow over a divider (or while dragging one).
        let want = self.sidebar_resize_at(x,y).map(|r|if r==crate::sidebar::Resize::Width {crate::tiles::Divider::X}else{crate::tiles::Divider::Y}).or(self.tile_drag.as_ref().map(|g| if g.axis == crate::tiles::Axis::Row { crate::tiles::Divider::X } else { crate::tiles::Divider::Y })).or_else(|| self.divider_at(x, y)).or_else(|| if self.split_drag || self.split_divider_at(x, y) { Some(crate::tiles::Divider::X) } else { None });
        if want != self.resize_cursor {
            self.resize_cursor = want;
            match want {
                Some(crate::tiles::Divider::X) => self.window.set_cursor(winit::window::CursorIcon::ColResize),
                Some(crate::tiles::Divider::Y) => self.window.set_cursor(winit::window::CursorIcon::RowResize),
                None => self.pointer_reset = true,
            }
        }
        if self.sidebar_visible() {
            let g = self.sidebar_geometry();
            let row = g.rows.iter().find(|&&(_, ry, rh)| self.sidebar_rect().contains(x, y) && y >= ry.max(g.top) && y < (ry + rh).min(g.foot_y)).map(|&(i, _, _)| i);
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
                    // A page with a question up doesn't see the pointer either.
                    if (w.page.contains(x, y) || self.mouse_down_in_web) && w.tab.shared.borrow().dialog.is_none() {
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

    /// This window's shell work for Finish Work: every command a shell says
    /// is running, with how the user started it (None if they didn't).
    pub(crate) fn finish_work_items(&self) -> Vec<crate::finish_work::Work> {
        let mut out = Vec::new();
        for tab in &self.tabs {
            for p in std::iter::once(&tab.left).chain(tab.right.as_ref()) {
                if let Pane::Term(t) = p {
                    if let (Some(id), Some(_)) = (t.work_id, t.running_since) {
                        out.push(crate::finish_work::Work { id: crate::finish_work::WorkId::Shell(id), origin: t.work_origin });
                    }
                }
            }
        }
        out
    }

    /// What a header icon does; shared by the mouse and AccessKit.
    pub(crate) fn crumb_action(&mut self, hit: CrumbHit) {
        match hit {
            CrumbHit::Close => {let _=self.proxy.send_event(UserEvent::WindowControl(self.window.id(),0));},
            CrumbHit::Maximize => if cfg!(target_os="macos") {self.toggle_fullscreen();} else {self.window.set_maximized(!self.window.is_maximized());},
            CrumbHit::Minimize => self.window.set_minimized(true),
            CrumbHit::Menu => self.open_palette(PaletteMode::Application),
            CrumbHit::Space | CrumbHit::Tab | CrumbHit::Search => self.open_palette(PaletteMode::Go),
            CrumbHit::Url => self.open_palette(PaletteMode::Url),
            CrumbHit::Start => self.open_start(),
            CrumbHit::Nus => self.toggle_home_latch(),
            CrumbHit::Agents => self.goto_agent(),
            CrumbHit::Updates => {
                self.open_settings_at(14,None);
                if crate::updates::status().available {crate::updates::confirm(true);}
                self.dirty=true;
            },
            CrumbHit::Sidebar => self.toggle_sidebar(),
            CrumbHit::Ports => self.open_board(),
            CrumbHit::Assistant => self.open_settings_at(crate::settings::SEC_ASSISTANTS, None),
            CrumbHit::Pip => self.return_from_pip(),
            CrumbHit::Waiting => {
                if let Some(i) = self.tabs.iter().position(|t| t.waiting()) {
                    self.activate(i);
                }
            }
            // The host owns the lease (finish_work.rs); it answers next turn.
            CrumbHit::FinishWork => {
                crate::finish_work::request_toggle();
                self.dirty = true;
            }
        }
    }

    pub fn mouse_button(&mut self, button: MouseButton, state: ElementState) {
        let (x, y) = self.mouse;
        let pressed = state == ElementState::Pressed;
        if pressed { self.dismiss_tip(); }
        if self.splash.as_ref().is_some_and(|s|s.arrival) {
            if pressed && button == MouseButton::Left {self.finish_arrival();self.splash=None;self.dirty=true;}
            return;
        }
        if pressed && self.behavior.pip_policy.click_app {self.close_pip();}
        // A page's question stands: its sheet answers, its page takes nothing.
        if self.palette.is_none() && self.page_dialog_mouse(pressed && button == MouseButton::Left, x, y) { self.dirty = true; return; }
        if self.page_menu_mouse(button, state) { return; }
        if self.timeline_mouse(button,state,x,y){return;}
        // The mouse's own back and forward buttons, on the page under them.
        if pressed && matches!(button, MouseButton::Back | MouseButton::Forward) {
            if button == MouseButton::Back && self.home_latch_back() {
                return;
            }
            let under = self.tabs.get(self.active).and_then(|t| {
                [(false, Some(&t.left)), (true, t.right.as_ref())].into_iter().find_map(|(right, p)| match p {
                    Some(Pane::Web(w)) if w.rect.contains(x, y) => Some(right),
                    _ => None,
                })
            });
            if let Some(right) = under {
                self.navigate(right, button == MouseButton::Back);
            }
            return;
        }
        if !pressed && button == MouseButton::Left && self.settings_drag.take().is_some() {
            self.save_prefs();
            return;
        }
        if button == MouseButton::Left && self.intel_mouse(pressed, x, y) {
            return;
        }
        let strip = self.strip_rect();
        if !pressed && button==MouseButton::Left && self.sidebar_resize.take().is_some(){self.save_prefs();self.apply_term_resizes(true);return;}
        if pressed && button==MouseButton::Left {if let Some(kind)=self.sidebar_resize_at(x,y){self.close_menus();self.sidebar_resize=Some(kind);return;}}

        if self.me_mouse(button, state, x, y) {
            return;
        }
        if self.start_mouse(button, state, x, y) {
            return;
        }
        if pressed && button==MouseButton::Left && (self.dl_menu || !(self.sidebar_visible()&&self.sidebar_rect().contains(x,y))) && self.download_click(x,y) {return;}
        if pressed && button == MouseButton::Left && self.toast_click(x, y) {
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
        if pressed && button == MouseButton::Left && strip.contains(x, y) && self.strip_shown() {
            let hit = self.crumb_hits.iter().find(|(r, _)| r.contains(x, y)).map(|(_, h)| *h);
            match hit {
                Some(h) => self.crumb_action(h),
                None => {
                    let _ = self.window.drag_window();
                }
            }
            return;
        }

        if pressed && button==MouseButton::Left && self.look_menu {
            if let Some((_,hit))=self.side_hits.iter().rev().find(|(r,h)|r.contains(x,y)&&matches!(h,SideHit::LookPreset(_)|SideHit::LookQuick(_)|SideHit::LookStudio)).copied(){self.side_action(hit,true);return;}
        }
        // A menu is up: a click elsewhere closes it.
        if pressed && (self.win_menu || self.kinds_menu || self.dl_menu || self.tab_menu.is_some()) {
            let on_menu = self.side_hits.iter().any(|(r, h)| r.contains(x, y) && matches!(h, SideHit::WinFront(_) | SideHit::Rename | SideHit::NewWindow | SideHit::Kind(_) | SideHit::KindPage | SideHit::Window | SideHit::Kinds | SideHit::Downloads | SideHit::TabRename(_) | SideHit::TabIcon(_) | SideHit::TabColour(..) | SideHit::TabPin(_) | SideHit::TabClose(_) | SideHit::TabTile(_) | SideHit::TabFolder(_)));
            if !on_menu {
                self.close_menus();
            }
        }
        // Right-click on NEW TAB fans out the kinds; on a row, the tab's menu.
        if pressed && button == MouseButton::Right && self.sidebar_visible() && self.sidebar_rect().contains(x, y) {
            if self.pins.rect.contains(x,y) {
                let live=self.side_hits.iter().find_map(|(r,h)|match h {
                    SideHit::Pinned(crate::pins::Act::Open(k)) if r.contains(x,y)=>self.pins.items.get(*k).and_then(|p|self.pins.live.get(&p.id)).and_then(|id|self.tabs.iter().position(|t|&t.id==id)).map(|i|(i,r.bottom())),_=>None,
                });
                if let Some((i,top))=live {self.close_menus();self.tab_menu=Some((i,top));self.tab_menu_anim.replay(0.0,1.0,self.motion.dur(140.0));self.dirty=true;}
                else if !self.pins.editing {self.pin_action(crate::pins::Act::Edit);}
                return;
            }
            if self.side_hits.iter().any(|(r, h)| *h == SideHit::NewShell && r.contains(x, y)) {
                self.open_kinds_menu();
                return;
            }
            let g = self.sidebar_geometry();
            if let Some(&(i, ry, rh)) = g.rows.iter().find(|&&(_, ry, rh)| y >= ry && y < ry + rh) {
                self.close_menus();
                self.tab_menu = Some((i, ry + rh));
                self.tab_menu_anim.replay(0.0, 1.0, self.motion.dur(140.0));
                self.dirty = true;
            }
            return;
        }
        // NEW TAB acts on release: a hold fans the kinds out instead.
        if !pressed && button==MouseButton::Left && self.pin_drag_release(y) {return;}
        if !pressed && button == MouseButton::Left {
            if let Some((at, SideHit::NewShell)) = self.press.take() {
                let still = self.side_hits.iter().any(|(r, h)| *h == SideHit::NewShell && r.contains(x, y));
                if still && crate::clock::since(at).as_millis() < 240 && !self.kinds_menu {
                    if self.header.flash {
                        self.flash_anim.replay(0.0, 1.0, self.motion.dur(120.0));
                        self.flash_anim.go(0.0, self.motion.dur(120.0));
                    }
                    self.open_start_page(false);
                    self.dirty = true;
                }
                return;
            }
        }
        // Sidebar: pinned cells, tab rows, footer. Ctrl-click selects, Shift-click ranges.
        if pressed && button == MouseButton::Left && self.sidebar_visible() && self.sidebar_rect().contains(x, y) {
            let sb = self.sidebar_rect();
            let g = self.sidebar_geometry();
            let pad = self.touch_pad();
            if let Some(&(_, hit)) = self.side_hits.iter().rev().find(|(r, _)| crate::touch::grown(*r, pad).contains(x, y)) {
                if let SideHit::Pinned(crate::pins::Act::Open(k))=hit {self.pins.drag=Some((k,(x,y),false));}
                else {self.side_action(hit, true);}
                self.dirty = true;
                return;
            }
            if y > g.foot_y {
                return;
            }
            let space_row = self.side_header_h();
            let pinned_y = sb.y + space_row + self.pins_height();
            let hit = if !g.pinned.is_empty() && y >= pinned_y && y < pinned_y + g.pinned_h {
                let k = if self.sidebar_icons(){((y-pinned_y)/self.px(32.0)) as usize}else{((x - sb.x) / (sb.w / g.pinned.len() as f32).floor()) as usize};
                g.pinned.get(k).copied()
            } else {
                g.rows.iter().find(|&&(_, ry, rh)| y >= ry.max(g.top) && y < (ry + rh).min(g.foot_y)).map(|&(i, _, _)| i)
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
                    self.row_host = Some((self.active, x));
                    self.activate(i);
                    // Armed: a few px of travel starts a drag.
                    if let Some(&(_, ry, _)) = g.rows.iter().find(|&&(t, _, _)| t == i) {
                        self.drag_armed = Some((i, y - ry, y));
                    }
                }
                self.dirty = true;
            }
            return;
        }

        // The welcome page's controls.
        if pressed && button == MouseButton::Left && self.hints_open() && self.welcome_click(x, y) {
            return;
        }

        // Settings: chips, sliders, swatches, buttons.
        if pressed && button == MouseButton::Left && self.settings_click(x, y) {
            return;
        }

        // Peek: Alt+click on a link floats it; a click on the scrim closes.
        if pressed && button == MouseButton::Left {
            if let Some((_, _)) = self.peeking() {
                if !self.peek_rect().contains(x, y) && self.content_rect().contains(x, y) {
                    self.close_peek();
                    return;
                }
            } else if self.mods.alt_key() && !self.mods.shift_key() {
                let active = self.active;
                let link = self.tabs.get(active).and_then(|t| {
                    let p = if t.focus_right && t.right.is_some() { t.right.as_ref().unwrap() } else { &t.left };
                    match p {
                        Pane::Web(w) if w.page.contains(x, y) => {
                            let u = w.tab.shared.borrow().hover_url.clone();
                            if u.starts_with("http") { Some(u) } else { None }
                        }
                        _ => None,
                    }
                });
                if let Some(u) = link {
                    self.open_peek(active, &u);
                    return;
                }
            }
        }
        // Split panes: the corner cluster, or the rule between them.
        if pressed && button == MouseButton::Left && self.pane_click(x, y) {
            return;
        }
        if pressed && button == MouseButton::Left && self.split_divider_at(x, y) {
            self.split_drag = true;
            self.drag_from = self.tabs.get(self.active).map(|t| crate::director::Op::SplitWidth { tab: t.id, w: t.split_w });
            return;
        }
        // Tiles: grab a divider, or focus the tile under the pointer.
        if pressed && button == MouseButton::Left {
            if let Some(g) = self.tile_rule_at(x, y) {
                self.tile_drag = Some(g);
                self.drag_from = Some(crate::director::Op::Tiling(self.tiling.clone()));
                return;
            }
            if let Some(i) = self.tile_at(x, y) {
                if i != self.active {
                    self.activate(i);
                }
            }
        }
        if pressed && button == MouseButton::Right {
            if let Some(i) = self.tile_at(x, y) {
                if i != self.active { self.activate(i); }
            }
        }
        if self.library_mouse(button, state, x, y) { return; }
        // Panes: focus, and forward to the browser.
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        let mut hit_right = None;
        let in_left = match &tab.left {
            Pane::Term(t) => t.rect.contains(x, y),
            Pane::Web(w) => w.rect.contains(x, y),
            Pane::Settings(s) => s.rect.contains(x, y),
            Pane::Hints(s) => s.rect.contains(x, y),
            Pane::Home(s) => s.rect.contains(x, y),
            Pane::Editor(e) => e.rect.contains(x, y),
            Pane::Ports(p) => p.rect.contains(x, y),
            Pane::Downloads(p) => p.rect.contains(x, y),
        };
        if let Some(r) = &tab.right {
            let hit = match r {
                Pane::Term(t) => t.rect.contains(x, y),
                Pane::Web(w) => w.rect.contains(x, y),
                Pane::Settings(s) => s.rect.contains(x, y),
                Pane::Hints(s) => s.rect.contains(x, y),
                Pane::Home(s) => s.rect.contains(x, y),
                Pane::Editor(e) => e.rect.contains(x, y),
                Pane::Ports(p) => p.rect.contains(x, y),
            Pane::Downloads(p) => p.rect.contains(x, y),
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
        if !pressed && button == MouseButton::Left {
            self.term_release_scroll();
            self.drag_armed = None;
            if self.tile_drag.take().is_some() {
                self.apply_term_resizes(true);
                self.divider_dragged();
                self.save_session();
                return;
            }
            if self.split_drag {
                self.split_drag = false;
                self.apply_term_resizes(true);
                self.divider_dragged();
                self.save_session();
                return;
            }
            if self.pane_drag.is_some() {
                self.pane_drop(x, y);
                return;
            }
            if let Some((i, _, _)) = self.drag.take() {
                self.row_host = None;
                // Out of the window: onto another nus window, or a new one there.
                if let Some(dest) = self.dropped_outside() {
                    self.send_tab(i, dest);
                    return;
                }
                let what = crate::pane_mode::Dragging::Tab(i);
                if let Some(d) = self.drop_at(x, y, what) {
                    self.apply_drop(what, d);
                    return;
                }
                // A reorder in the sidebar; the dragged tab is in front again.
                let id = self.tabs.get(i).map(|t| t.id);
                self.drop_row(i, y);
                if let Some(k) = id.and_then(|id| self.tabs.iter().position(|t| t.id == id)) {
                    self.activate(k);
                }
                return;
            }
        }
        if pressed && button == MouseButton::Left && self.ask_click(x, y) {
            return;
        }
        if self.tidy_mouse(button, state, x, y) {
            return;
        }
        if self.board_mouse(button, state, x, y) {
            return;
        }
        if pressed && button == MouseButton::Left {
            if let (Some(r), Some((p, _))) = (self.layout_offer_hit, self.layout_offer.clone()) {
                if r.contains(x, y) {
                    self.layout_offer = None;
                    self.layout_offer_hit = None;
                    self.open_layout(&p);
                    return;
                }
            }
        }
        if !pressed && button == MouseButton::Left {
            self.home_release();
            self.dock_release();
        }
        if pressed && button == MouseButton::Left && self.palette.is_none() && self.dock_press(x, y) {
            return;
        }
        if self.editor_mouse(button, state, x, y) {
            return;
        }
        if pressed && button == MouseButton::Left && (self.hunk_click(x, y) || self.lamp_click(x, y) || self.cutoff_click(x, y)) {
            return;
        }
        if pressed && button == MouseButton::Left && self.colour_offer_click(x, y) {
            return;
        }
        if self.term_mouse(button, state, x, y) {
            return;
        }
        if pressed && button == MouseButton::Left && self.dedupe_click(x, y) {
            return;
        }
        if pressed && button == MouseButton::Left && self.home_click(x, y) {
            return;
        }
        if pressed && button == MouseButton::Left && self.palette.is_none() && self.overlay_click(x, y) {
            return;
        }
        if pressed && button == MouseButton::Left && self.hands_click(x, y) {
            return;
        }
        if pressed && button == MouseButton::Left && self.web_band_click(x, y) {
            return;
        }
        if pressed && button == MouseButton::Left && self.site_click(x, y) {
            return;
        }
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        let scale = self.scale;
        let mods = cef_mods(self.mods);
        let mut down_in_web = self.mouse_down_in_web;
        let mut open_url_palette = false;
        let mut toggle_devtools = false;
        let mut toggle_reader = false;
        let mut toggle_site: Option<bool> = None;
        let mut switch_panel: Option<(bool, usize)> = None;
        let mut focus_dt: Option<(bool, bool)> = None;
        let mut loop_click: Option<(bool, f32, f32)> = None;
        let mut media_click: Option<(u64, bool)> = None;
        let mut nav_click: Option<(bool, bool)> = None;
        for (is_right, p) in std::iter::once((false, &tab.left)).chain(tab.right.as_ref().map(|r| (true, r))) {
            match p {
                Pane::Web(w) => {
                    let url_row = Rect::new(w.rect.x, w.rect.y, w.rect.w, w.page.y - w.rect.y);
                    if pressed && button == MouseButton::Left && url_row.contains(x, y) {
                        let slot = m::NAV_SLOT * scale;
                        let nav_x = x - (w.rect.x + 14.0 * scale);
                        if nav_x < 3.0 * slot {
                            match (nav_x / slot) as usize {
                                0 => nav_click = Some((is_right, true)),
                                1 => nav_click = Some((is_right, false)),
                                _ => w.tab.reload(),
                            }
                        } else if x > w.rect.right() - 44.0 * scale {
                            toggle_devtools = true;
                        } else if x > w.rect.right() - 74.0 * scale {
                            toggle_reader = true;
                        } else if x > w.rect.right() - 104.0 * scale {
                            toggle_site = Some(is_right);
                        } else if x > w.rect.right() - 134.0 * scale && !w.tab.shared.borrow().media.is_empty() {
                            media_click = Some((tab.id, is_right));
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
                    if inside && pressed && button == MouseButton::Left && self.mods.alt_key() && self.mods.shift_key() {
                        loop_click = Some((is_right, (x - w.page.x) / scale, (y - w.page.y) / scale));
                        continue;
                    }
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
                Pane::Term(_) | Pane::Settings(_) | Pane::Hints(_) | Pane::Editor(_) | Pane::Ports(_) | Pane::Downloads(_) | Pane::Home(_) => {}
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
        if let Some((id, right)) = media_click {
            self.media_menu(id, right, (x, y));
        }
        if let Some(right) = toggle_site {
            if let Some(tab) = self.tabs.get_mut(self.active) {
                let pane = if right { tab.right.as_mut() } else { Some(&mut tab.left) };
                if let Some(Pane::Web(w)) = pane {
                    w.site_panel = !w.site_panel;
                }
            }
            self.play_event("toggle");
            self.dirty = true;
        }
        if let Some((right, back)) = nav_click {
            self.navigate(right, back);
            return;
        }
        if let Some((right, lx, ly)) = loop_click {
            let i = self.active;
            self.loop_click(i, right, lx, ly);
            return;
        }
        if open_url_palette {
            self.open_palette(PaletteMode::Url);
        }
    }

    pub fn wheel(&mut self, delta: MouseScrollDelta) {
        self.dismiss_tip();
        if self.page_menu_wheel(delta) { return; }
        let (x, y) = self.mouse;
        if let Some(i) = self.tile_at(x, y) {
            if i != self.active {
                self.activate(i);
            }
        }
        // Physical px the wheel asks for; a notch is WHEEL SPEED.
        let dy_px = match delta {
            MouseScrollDelta::LineDelta(_, y) => y * self.wheel_step(),
            MouseScrollDelta::PixelDelta(p) => p.y as f32,
        };
        if self.timeline_wheel(x,y,dy_px){return;}
        if self.page_dialog_blocks(x, y) { return; }
        if self.me_wheel(x, y, dy_px) { return; }
        if self.palette.is_some() {
            let (at, max) = (self.palette_scroll, self.palette_scroll_max);
            self.palette_scroll = self.glide(crate::scrolling::Glider::Palette, at, -dy_px, max);
            self.palette_reveal=false;self.dirty=true;return;
        }
        if self.dl_menu {
            let (at, max) = (self.download_ui.scroll, self.download_ui.reach);
            self.download_ui.scroll = self.glide(crate::scrolling::Glider::DownloadsMenu, at, -dy_px, max);
            self.dirty=true;return;
        }
        if self.look_menu && self.look_rect.is_some_and(|r|r.contains(x,y)) {
            let (at, max) = (self.look_scroll, self.look_scroll_max);
            self.look_scroll = self.glide(crate::scrolling::Glider::Look, at, -dy_px, max);
            self.dirty = true;
            return;
        }
        if self.ask_wheel(x, y, dy_px) {
            return;
        }
        if self.tree_wheel(x, y, dy_px) {
            return;
        }
        // Over the sidebar the wheel is the sidebar's: its list scrolls when
        // it overflows, and nothing under it moves.
        if self.sidebar_wheel(x, y, dy_px) {
            return;
        }
        if self.board_wheel(x, y, dy_px) {
            return;
        }
        let dx_px = match delta {
            MouseScrollDelta::LineDelta(x, _) => x * self.wheel_step(),
            MouseScrollDelta::PixelDelta(p) => p.x as f32,
        };
        if self.library_wheel(x, y, dx_px, dy_px) { return; }
        let wheel_lines = self.behavior.wheel_lines as f32;
        let easing = self.behavior.scroll_easing;
        let shift = self.mods.shift_key();
        let mods = self.vt_mods();
        let motion = self.motion.clone();
        let scale = self.scale;
        let cef_flags = cef_mods(self.mods);
        let (settings_reach, welcome_reach, dl_reach) = (self.settings_reach, self.welcome_reach, self.download_ui.reach);
        let Some(tab) = self.tabs.get(self.active) else { return };
        let tab_id = tab.id;
        // Which pane the wheel is over, and what to do there. The glides
        // need `self`, so the pane is found first and moved after.
        enum Do {
            Downloads,
            Settings,
            Welcome,
            Reader(bool),
            Page(bool),
            DevTools(bool),
            Term(bool),
            Editor(bool),
        }
        let mut what = None;
        for (right, p) in std::iter::once((false, &tab.left)).chain(tab.right.as_ref().map(|r| (true, r))) {
            what = match p {
                Pane::Downloads(p) if p.rect.contains(x, y) => Some(Do::Downloads),
                Pane::Term(t) if t.rect.contains(x, y) => Some(Do::Term(right)),
                Pane::Web(w) if w.devtools.is_some() && w.dt_rect.contains(x, y) => Some(Do::DevTools(right)),
                Pane::Settings(s) if s.rect.contains(x, y) => Some(Do::Settings),
                Pane::Editor(e) if e.rect.contains(x, y) => Some(Do::Editor(right)),
                Pane::Hints(h) if h.rect.contains(x, y) => Some(Do::Welcome),
                Pane::Web(w) if w.page.contains(x, y) && w.reader.is_some() => Some(Do::Reader(right)),
                Pane::Web(w) if w.page.contains(x, y) => Some(Do::Page(right)),
                _ => None,
            };
            if what.is_some() {
                break;
            }
        }
        let Some(what) = what else { return };
        match what {
            Do::Downloads => {
                let Some(Pane::Downloads(p)) = self.tabs.get(self.active).map(|t| &t.left) else { return };
                let at = p.scroll;
                let v = self.glide(crate::scrolling::Glider::Downloads(tab_id), at, -dy_px, dl_reach);
                if let Some(Pane::Downloads(p)) = self.tabs.get_mut(self.active).map(|t| &mut t.left) {
                    p.scroll = v;
                }
                self.dirty = true;
            }
            Do::Settings => {
                let Some(Pane::Settings(s)) = self.tabs.get(self.active).map(|t| &t.left) else { return };
                let (at, max) = (s.scroll, (settings_reach - s.rect.h + scale * 48.0).max(0.0));
                let v = self.glide(crate::scrolling::Glider::Settings(tab_id), at, -dy_px, max);
                if let Some(Pane::Settings(s)) = self.tabs.get_mut(self.active).map(|t| &mut t.left) {
                    s.scroll = v;
                }
                self.dirty = true;
            }
            Do::Welcome => {
                let Some(Pane::Hints(h)) = self.tabs.get(self.active).map(|t| &t.left) else { return };
                let (at, max) = (h.scroll, (welcome_reach - h.rect.h).max(0.0));
                let v = self.glide(crate::scrolling::Glider::Welcome(tab_id), at, -dy_px, max);
                if let Some(Pane::Hints(h)) = self.tabs.get_mut(self.active).map(|t| &mut t.left) {
                    h.scroll = v;
                }
                self.dirty = true;
            }
            Do::Reader(right) => {
                fn pane_of(t: &Tab, right: bool) -> Option<&Pane> { if right { t.right.as_ref() } else { Some(&t.left) } }
                let Some(Pane::Web(w)) = self.tabs.get(self.active).and_then(|t| pane_of(t, right)) else { return };
                let Some(rd) = w.reader.as_ref() else { return };
                let (at, max) = (rd.scroll, (rd.height - w.page.h).max(0.0));
                let v = self.glide(crate::scrolling::Glider::Reader(tab_id, right), at, -dy_px, max);
                let tab = &mut self.tabs[self.active];
                let pane = if right { tab.right.as_mut() } else { Some(&mut tab.left) };
                if let Some(Pane::Web(w)) = pane {
                    if let Some(rd) = w.reader.as_mut() {
                        rd.scroll = v;
                    }
                }
                self.dirty = true;
            }
            Do::Term(right) => {
                let tab = &mut self.tabs[self.active];
                let pane = if right { tab.right.as_mut() } else { Some(&mut tab.left) };
                let Some(Pane::Term(t)) = pane else { return };
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y * wheel_lines,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / t.grid.cell_size().1,
                };
                if t.wants_mouse() && !shift {
                    use nus_vt::input::{MouseAction, MouseButton};
                    let b = if lines > 0.0 { MouseButton::WheelUp } else { MouseButton::WheelDown };
                    let n = (lines.abs().round() as usize).clamp(1, 10);
                    for _ in 0..n {
                        t.report_mouse(b, MouseAction::Press, mods, x, y);
                    }
                    return;
                }
                if t.term.modes().contains(nus_vt::Modes::ALT_SCREEN) && t.term.modes().contains(nus_vt::Modes::ALTERNATE_SCROLL) {
                    // Alternate scroll (1007): the wheel is arrow keys.
                    let key: &[u8] = if lines > 0.0 { b"\x1b[A" } else { b"\x1b[B" };
                    let n = (lines.abs().round() as usize).clamp(1, 10);
                    let _ = t.pty.write(&key.repeat(n));
                    return;
                }
                crate::scrolling::scroll_shell(t, lines, easing, &motion);
                self.dirty = true;
            }
            Do::Editor(right) => {
                let tab = &mut self.tabs[self.active];
                let pane = if right { tab.right.as_mut() } else { Some(&mut tab.left) };
                let Some(Pane::Editor(e)) = pane else { return };
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => (y * wheel_lines).round() as i64,
                    MouseScrollDelta::PixelDelta(p) => (p.y as f32 / e.cell.1).round() as i64,
                };
                e.scroll_by(lines);
                e.hover = None;
                self.dirty = true;
            }
            Do::DevTools(right) => {
                let mut carry = (0.0, 0.0);
                let (dx, dy) = self.page_wheel_units(delta, &mut carry);
                let tab = &mut self.tabs[self.active];
                let pane = if right { tab.right.as_mut() } else { Some(&mut tab.left) };
                let Some(Pane::Web(w)) = pane else { return };
                let (lx, ly) = ((x - w.dt_rect.x) / scale, (y - w.dt_rect.y) / scale);
                w.devtools.as_ref().unwrap().wheel(lx as i32, ly as i32, cef_flags, dx, dy);
            }
            Do::Page(right) => {
                let mut carry = {
                    let tab = &self.tabs[self.active];
                    let pane = if right { tab.right.as_ref() } else { Some(&tab.left) };
                    match pane {
                        Some(Pane::Web(w)) => w.wheel_carry,
                        _ => (0.0, 0.0),
                    }
                };
                let (dx, dy) = self.page_wheel_units(delta, &mut carry);
                // A sideways swipe, mostly sideways: back or forward once it
                // has gone far enough (swipe.rs draws the arrow meanwhile).
                let (sx, sy) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (x * 60.0, y * 60.0),
                    MouseScrollDelta::PixelDelta(p) => (p.x as f32 / scale, p.y as f32 / scale),
                };
                let sideways = sx.abs() > 2.0 * sy.abs() && sx.abs() > 0.5;
                let tab = &mut self.tabs[self.active];
                let pane = if right { tab.right.as_mut() } else { Some(&mut tab.left) };
                let Some(Pane::Web(w)) = pane else { return };
                w.wheel_carry = carry;
                w.wheel_precise = matches!(delta, MouseScrollDelta::PixelDelta(_));
                let (lx, ly) = ((x - w.page.x) / scale, (y - w.page.y) / scale);
                w.tab.wheel(lx as i32, ly as i32, cef_flags, dx, dy);
                if sideways {
                    self.swipe_step(right, sx);
                }
            }
        }
    }

    /// The window's outer rect, remembered for "last size & place".
    pub(crate) fn remember_window(&mut self) {
        // No `is_maximized()` here: on macOS winit answers it by toggling the
        // window's style mask, which moves the window, which lands back here —
        // a loop that ate the main thread. It's asked once, when the debounced
        // save comes due.
        if self.fullscreen {
            return;
        }
        if let (Ok(p), s) = (self.window.outer_position(), self.window.inner_size()) {
            let r = (p.x, p.y, s.width, s.height);
            if self.window_rect != Some(r) && s.width > 200 && s.height > 200 {
                self.window_rect = Some(r);
                self.window_rect_dirty = Some(crate::clock::now());
            }
        }
    }

    pub fn window_moved(&mut self, x: i32, y: i32) {
        self.dismiss_tip();
        self.close_page_menu();
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

pub(crate) const SYSTEM_PROCS: &[&str] = &["nus-hold", "system", "svchost", "lsass", "wininit", "services", "spoolsv", "dns", "rpcbind", "systemd", "cupsd", "launchd", "rapportd", "controlce", "sharingd"];

/// Local/private destinations get the Plot boundary.
pub(crate) fn is_local_url(url: &str) -> bool {
    is_local(url)
}

fn is_local(url: &str) -> bool {
    crate::page_signal::is_local(url)
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
/// A command trimmed for a one-line label.
pub(crate) fn fit_cmd(cmd: &str, max: usize) -> String {
    let one: String = cmd.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() <= max {
        one
    } else {
        format!("{}…", one.chars().take(max.saturating_sub(1)).collect::<String>())
    }
}

/// `nus <file>` or `edit <file>` typed at a prompt, resolved against the
/// shell's cwd; only an existing file counts.
pub(crate) fn file_at_prompt(line: &str, cwd: Option<&str>) -> Option<std::path::PathBuf> {
    let s = line.trim();
    let rest = s.strip_prefix("nus ").or_else(|| s.strip_prefix("edit "))?.trim();
    if rest.is_empty() || rest.starts_with('-') {
        return None;
    }
    let rest = rest.trim_matches(['"', '\'']);
    let p = std::path::Path::new(rest);
    let p = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::path::Path::new(cwd?).join(p)
    };
    p.is_file().then_some(p)
}

/// An absolute path (or ~/) to an existing file.
pub(crate) fn local_file(q: &str) -> Option<std::path::PathBuf> {
    let q = q.trim().trim_matches('"');
    let p = if let Some(rest) = q.strip_prefix("~/").or_else(|| q.strip_prefix("~\\")) {
        std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(|h| std::path::PathBuf::from(h).join(rest))?
    } else {
        let p = std::path::PathBuf::from(q);
        if !p.is_absolute() {
            return None;
        }
        p
    };
    p.is_file().then_some(p)
}

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
/// The parts of a key press the app acts on: what winit gives, minus the
/// platform's private half, so one can be made from a script.
#[derive(Clone, Debug)]
pub struct KeyIn {
    pub physical_key: PhysicalKey,
    pub logical_key: WKey,
    pub text: Option<winit::keyboard::SmolStr>,
    pub state: ElementState,
    pub repeat: bool,
}

impl From<&WKeyEvent> for KeyIn {
    fn from(ev: &WKeyEvent) -> KeyIn {
        KeyIn { physical_key: ev.physical_key, logical_key: ev.logical_key.clone(), text: ev.text.clone(), state: ev.state, repeat: ev.repeat }
    }
}

/// A winit key to a browser, as the platform's own client would send it
/// (webkeys.rs): raw down, chars, or up.
pub(crate) fn forward_key(tab: &BrowserTab, ev: &KeyIn, mods: ModifiersState) {
    for e in crate::webkeys::events(ev, mods) {
        tab.key(&e);
    }
}

/// The name of the git repository at `from` (or where the app was launched), if any.
fn git_root_name(from: Option<std::path::PathBuf>) -> Option<String> {
    let mut d = match from {
        Some(p) => p,
        None => std::env::current_dir().ok()?,
    };
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
/// "35.2, -106.6" (or with ° N/S E/W) → [lat, lon].
pub(crate) fn parse_place(s: &str) -> Option<[f32; 2]> {
    let clean: String = s.chars().map(|c| if c == '°' || c == ';' { ' ' } else { c }).collect();
    let mut nums: Vec<f32> = Vec::new();
    let mut signs: Vec<f32> = Vec::new();
    for tok in clean.split(|c: char| c == ',' || c.is_whitespace()).filter(|t| !t.is_empty()) {
        let up = tok.to_uppercase();
        match up.as_str() {
            "N" | "E" => { if let Some(last) = signs.last_mut() { *last = 1.0; } }
            "S" | "W" => { if let Some(last) = signs.last_mut() { *last = -1.0; } }
            _ => {
                if let Ok(v) = tok.trim_end_matches(|c: char| c.is_alphabetic()).parse::<f32>() {
                    nums.push(v);
                    signs.push(1.0);
                    if up.ends_with('S') || up.ends_with('W') { *signs.last_mut().unwrap() = -1.0; }
                }
            }
        }
    }
    if nums.len() != 2 { return None; }
    let (lat, lon) = (nums[0] * signs[0], nums[1] * signs[1]);
    if lat.abs() > 90.0 || lon.abs() > 180.0 { return None; }
    Some([lat, lon])
}

/// Hand a file to the OS: its own program opens it.
pub(crate) fn open_with_os(path: &std::path::Path) {
    let p = path.to_string_lossy().to_string();
    let _ = if cfg!(target_os = "windows") {
        std::process::Command::new("cmd").args(["/c", "start", "", &p]).spawn()
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(&p).spawn()
    } else {
        std::process::Command::new("xdg-open").arg(&p).spawn()
    };
}

pub(crate) fn fade(c: nus_render::Color, k: f32) -> nus_render::Color {
    [c[0], c[1], c[2], c[3] * k]
}

impl TermPane {
    /// Whether the application asked for the mouse (any of 1000/1002/1003).
    pub fn wants_mouse(&self) -> bool {
        self.term.modes().intersects(nus_vt::Modes::ANY_MOUSE)
    }

    /// Report a mouse event to the application, xterm-style, when it asked
    /// for one. `x, y` are window pixels. Returns true when a report went.
    pub fn report_mouse(&mut self, button: nus_vt::input::MouseButton, action: nus_vt::input::MouseAction, mods: Mods, x: f32, y: f32) -> bool {
        use nus_vt::input::{MouseAction, MouseButton};
        if !self.wants_mouse() {
            return false;
        }
        let (cw, ch) = self.grid.cell_size();
        let grid = self.term.grid();
        let col = ((x - self.origin.0) / cw).floor().clamp(0.0, grid.cols() as f32 - 1.0) as usize;
        let row = ((y - self.origin.1) / ch).floor().clamp(0.0, grid.rows() as f32 - 1.0) as usize;
        let px = ((x - self.origin.0).max(0.0) as u32, (y - self.origin.1).max(0.0) as u32);
        let button = match (action, button) {
            (MouseAction::Motion, MouseButton::None) => self.mouse_held.unwrap_or(MouseButton::None),
            _ => button,
        };
        if action == MouseAction::Motion {
            if self.mouse_last == Some((col, row)) {
                return true;
            }
        }
        let bytes = input::encode_mouse(button, action, mods, (col, row), px, self.term.modes());
        match action {
            MouseAction::Press if !matches!(button, MouseButton::WheelUp | MouseButton::WheelDown | MouseButton::WheelLeft | MouseButton::WheelRight) => {
                self.mouse_held = Some(button);
            }
            MouseAction::Release => self.mouse_held = None,
            _ => {}
        }
        self.mouse_last = Some((col, row));
        if bytes.is_empty() {
            return false;
        }
        let _ = self.pty.write(&bytes);
        true
    }

    /// Write pasted text, bracketed when the program asked for it.
    pub fn write_paste(&mut self, text: &str) {
        let bracketed = self.term.modes().contains(nus_vt::Modes::BRACKETED_PASTE);
        let mut out = Vec::new();
        if bracketed {
            out.extend_from_slice(b"\x1b[200~");
        }
        out.extend_from_slice(text.replace("\r\n", "\r").replace('\n', "\r").as_bytes());
        if bracketed {
            out.extend_from_slice(b"\x1b[201~");
        }
        let _ = self.pty.write(&out);
    }
}

/// Today, for a saved theme's story (no chrono: from the epoch by hand).
fn chrono_date() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let days = secs / 86400;
    // Civil from days (Howard Hinnant's algorithm).
    let z = days as i64 + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

/// Depth of tab `i` in a tab list (0 at the top).
pub(crate) fn depth_of(tabs: &[Tab], i: usize) -> usize {
    let mut d = 0;
    let mut cur = i;
    while let Some(pid) = tabs[cur].parent {
        match tabs.iter().position(|t| t.id == pid) {
            Some(p) if p != cur && d < 64 => {
                cur = p;
                d += 1;
            }
            _ => break,
        }
    }
    d
}

/// Every descendant of tab `i` in a tab list.
pub(crate) fn subtree_of(tabs: &[Tab], i: usize) -> Vec<usize> {
    let mut out = Vec::new();
    let mut frontier = vec![tabs[i].id];
    while let Some(id) = frontier.pop() {
        for j in 0..tabs.len() {
            if tabs[j].parent == Some(id) && !out.contains(&j) {
                out.push(j);
                frontier.push(tabs[j].id);
            }
        }
    }
    out
}

/// Give a pane its rectangle. A split (or tiled) terminal shows its header
/// strip; a page lays out its URL row, page, DevTools and tools row.
pub(crate) fn place_pane(pane: &mut Pane, r: Rect, header: f32, pad_x: f32, pad_y: f32, scale: f32, split: bool) {
    place_pane_bare(pane, r, header, pad_x, pad_y, scale, split, false)
}

/// `place_pane`, with the page alone (no rows) when `bare`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn place_pane_bare(pane: &mut Pane, r: Rect, header: f32, pad_x: f32, pad_y: f32, scale: f32, split: bool, bare: bool) {
    let r=Rect::new(r.x.round(),r.y.round(),(r.right().round()-r.x.round()).max(0.0),(r.bottom().round()-r.y.round()).max(0.0));
    match pane {
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
        Pane::Home(h) => h.rect = r,
        Pane::Editor(e) => e.rect = r,
        Pane::Ports(p) => p.rect = r,
            Pane::Downloads(p) => p.rect = r,
        Pane::Web(w) => {
            w.rect = r;
            w.bare = bare;
            let url_row=if bare{0.0}else{header};
            let tools_row=if bare{0.0}else{(m::PANE_FOOTER*scale).round()};
            let hair=(m::HAIRLINE*scale).round().max(1.0);
            let avail=(r.h-url_row-tools_row).max(1.0);
            let dt_h=if w.devtools.is_some(){(avail*0.42).round()}else{0.0};
            w.page=Rect::new(r.x,r.y+url_row,r.w,avail-dt_h);
            w.dt_rect=Rect::new(r.x,w.page.bottom()+if dt_h>0.0{hair}else{0.0},r.w,(dt_h-hair).max(0.0));
            {
                let mut s = w.tab.shared.borrow_mut();
                s.origin = (w.page.x, w.page.y);
            }
            w.tab.set_scale(scale);
            w.tab.resized((w.page.w / scale).floor(), (w.page.h / scale).floor());
            if let Some(d) = &w.devtools {
                {
                    let mut s = d.shared.borrow_mut();
                    s.origin = (w.dt_rect.x, w.dt_rect.y);
                }
                d.set_scale(scale);
                d.resized((w.dt_rect.w / scale).floor(), (w.dt_rect.h.max(1.0) / scale).floor());
            }
        }
    }
}

impl App {
    /// A pane's icon at `size`: a page's favicon, else a letter tile for
    /// its host (the first letter on a square in a colour the host picks
    /// from the signal's family), else the kind's icon. `clip` restores a
    /// layer after a texture draw.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn draw_pane_icon(&mut self, scene: &mut Scene, pane: &Pane, x: f32, y: f32, size: f32, color: nus_render::Color, clip: Option<Rect>) {
        match pane {
            Pane::Web(w) => {
                if let Some((_, b)) = w.favicon.as_ref() {
                    scene.texture(Rect::new(x, y, size, size), b.clone(), None);
                    scene.layer(clip);
                    return;
                }
                let host = crate::sites::host_of(&w.tab.shared.borrow().url);
                let first = host.chars().find(|c| c.is_alphanumeric());
                match first {
                    Some(c) => {
                        // The colour: one of the signal's family, by the host.
                        let k = host.bytes().fold(0usize, |a, b| a.wrapping_mul(31).wrapping_add(b as usize)) % 5;
                        let fill = crate::surface::family(self.surface.signal)[k];
                        let fill = [fill[0], fill[1], fill[2], 1.0];
                        // The letter in whichever of paper and ink reads on the fill.
                        let lum = 0.2126 * fill[0] + 0.7152 * fill[1] + 0.0722 * fill[2];
                        let (paper, ink) = (self.theme.paper, self.theme.ink);
                        let plum = 0.2126 * paper[0] + 0.7152 * paper[1] + 0.0722 * paper[2];
                        let letter = if (lum - plum).abs() > 0.35 { paper } else { ink };
                        scene.push(nus_render::Instance::rounded(Rect::new(x, y, size, size), (size * 0.2).round().max(2.0), fill));
                        let st = Style { font: self.f.strong, px: (size * 0.72).round(), color: letter, tracking: 0.0 };
                        let ch = c.to_uppercase().to_string();
                        let cw = self.fonts.measure(st, &ch);
                        self.fonts.draw(scene, st, x + ((size - cw) / 2.0).round(), y + (size * 0.78).round(), &ch);
                    }
                    None => {
                        self.fonts.draw_icon(scene, nus_render::text::icons::GLOBE, size, x, y, color);
                    }
                }
            }
            Pane::Term(_) => {
                self.fonts.draw_icon(scene, nus_render::text::icons::TERMINAL, size, x, y, color);
            }
            Pane::Settings(_) => {
                self.fonts.draw_icon(scene, nus_render::text::icons::SETTINGS, size, x, y, color);
            }
            Pane::Hints(_) => {
                self.fonts.draw_icon(scene, nus_render::text::icons::ROCKET, size, x, y, color);
            }
            Pane::Home(h) => {
                self.fonts.draw_icon(scene, if h.library { nus_render::text::icons::BOOK } else { nus_render::text::icons::HOME }, size, x, y, color);
            }
            Pane::Editor(_) => {
                self.fonts.draw_icon(scene, nus_render::text::icons::CODE, size, x, y, color);
            }
            Pane::Downloads(_) => {self.fonts.draw_icon(scene,nus_render::text::icons::DOWNLOAD,size,x,y,color);}
            Pane::Ports(_) => {
                self.fonts.draw_icon(scene, nus_render::text::icons::PORTS, size, x, y, color);
            }
        }
    }

    /// Shells that have exited: the pane goes. A split's other pane takes
    /// the tab; a lone shell's tab closes (a new shell if it was the last).
    pub(crate) fn tend_shells(&mut self) {
        let mut gone: Vec<(usize, bool)> = Vec::new();
        for (i, tab) in self.tabs.iter_mut().enumerate() {
            for (right, p) in std::iter::once((false, &mut tab.left)).chain(tab.right.as_mut().map(|r| (true, r))) {
                if let Pane::Term(t) = p {
                    if t.pty.exit_code().is_some() {
                        gone.push((i, right));
                    }
                }
            }
        }
        if gone.is_empty() {
            return;
        }
        for &(i, right) in gone.iter().rev() {
            let Some(tab) = self.tabs.get_mut(i) else { continue };
            if right {
                tab.right = None;
                tab.focus_right = false;
            } else if let Some(r) = tab.right.take() {
                tab.left = r;
                tab.focus_right = false;
            } else {
                // A lone shell: the tab closes.
                let was_active = i == self.active;
                let tab = self.tabs.remove(i);
                self.tile_forget(tab.id);
                self.tab_removed(i);
                if self.tabs.is_empty() {
                    let p = self.behavior.default_profile;
                    self.new_tab(p);
                } else if was_active {
                    let next = self.mru.first().copied().unwrap_or(0).min(self.tabs.len() - 1);
                    self.activate(next);
                } else if self.active > i {
                    self.active -= 1;
                }
                continue;
            }
        }
        self.layout();
        self.save_session();
        self.dirty = true;
    }
}

/// Standard base64, for OSC 52 replies.
fn base64_encode(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().fold(0u32, |a, &b| (a << 8) | b as u32) << (8 * (3 - chunk.len()));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(T[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Names and titles read in Caps: the first letter of each word up, the
/// rest as they came ("wgpu - Rust" → "Wgpu - Rust"; an ALLCAPS word is
/// brought down to Caps).
pub trait Caps {
    fn caps(&self) -> String;
}

impl Caps for str {
    fn caps(&self) -> String {
        let mut out = String::with_capacity(self.len());
        for word in self.split_inclusive(char::is_whitespace) {
            let letters: Vec<char> = word.chars().filter(|c| c.is_alphabetic()).collect();
            let all_caps = !letters.is_empty() && letters.iter().all(|c| c.is_uppercase());
            let mut first = true;
            for ch in word.chars() {
                if ch.is_alphabetic() {
                    if first {
                        out.extend(ch.to_uppercase());
                        first = false;
                    } else if all_caps {
                        out.extend(ch.to_lowercase());
                    } else {
                        out.push(ch);
                    }
                } else {
                    out.push(ch);
                }
            }
        }
        out
    }
}

impl Caps for String {
    fn caps(&self) -> String {
        self.as_str().caps()
    }
}
