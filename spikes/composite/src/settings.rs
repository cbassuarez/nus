//! The settings tab (Ctrl+,): a ruled nav on the left, one section at a
//! time on the right. Every control is a click target recorded in
//! `App::settings_hits` — choice chips, sliders, swatches, buttons — so the
//! page is native chrome like everything else. The Luau file is the other
//! way in; the RULES section shows it and reloads it.

#[path = "workspace_settings.rs"]
pub(crate) mod workspace;
use crate::anim::Anim;
use crate::app::{Caps, fade, hover_key, Hover};
use crate::app::{App, Pane, SettingsPane};
use crate::anim::{BarColor, BarStyle};
use crate::surface::{self, Fullscreen, HoverFrom, OpacityOn, Shell, Side, TextureKind, TextureOn, SWATCHES};
use nus_render::text::icons;
use nus_render::Style;
use nus_render::theme::metric as m;
use nus_render::{Color, Rect, Scene};

/// Where links a page opens go (target=_blank, window.open).
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Links {
    Stack,
    Split,
    NewTab,
}

/// The hatch's look: a sheet from the top edge, or a framed card, centred.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub enum HatchLook {
    #[default]
    Sheet,
    Card,
}

/// Which monitor the hatch lands on.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub enum HatchMonitor {
    #[default]
    Pointer,
    Foreground,
    Primary,
}

/// One hatch per Space (follows the Space you're in), or one for all.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub enum HatchSpaces {
    Follow,
    #[default]
    One,
}

fn default_hatch_size() -> u8 {
    40
}

/// How the ports board groups its rows.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub enum PortsGrouping {
    /// MINE · OTHERS · SYSTEM · CONNECTIONS · DOCKER.
    #[default]
    Origin,
    Port,
    Process,
}

impl PortsGrouping {
    pub fn name(self) -> &'static str {
        match self {
            PortsGrouping::Origin => "by origin",
            PortsGrouping::Port => "by port",
            PortsGrouping::Process => "by process",
        }
    }
    pub fn next(self) -> PortsGrouping {
        match self {
            PortsGrouping::Origin => PortsGrouping::Port,
            PortsGrouping::Port => PortsGrouping::Process,
            PortsGrouping::Process => PortsGrouping::Origin,
        }
    }
}

/// Where OPEN on a port row goes.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub enum PortsOpen {
    Tab,
    #[default]
    Split,
    Peek,
}

/// When KILL asks first.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub enum KillConfirm {
    /// Only for processes nus didn't start.
    #[default]
    System,
    Always,
    Never,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub enum Tunnel {
    #[default]
    Cloudflared,
    Ngrok,
}

fn default_ports_poll() -> u8 {
    1
}

/// What a shell's OSC 10/11 (set foreground/background) does to the look.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub enum ShellColours {
    /// The pane changes; a chip offers to apply it to the whole look.
    #[default]
    Chip,
    /// The look follows at once.
    Always,
    /// The pane only, as any terminal.
    PaneOnly,
}

/// The contrast every program's text must reach against its background
/// (WCAG 2); what doesn't is walked toward ink until it does.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub enum Grade {
    Off,
    /// 3:1 — large text.
    Large,
    /// 4.5:1 — AA for text. VS Code's terminal default; ours.
    #[default]
    Aa,
    /// 7:1 — AAA.
    Aaa,
}

impl Grade {
    pub fn ratio(self) -> f32 {
        match self {
            Grade::Off => 0.0,
            Grade::Large => 3.0,
            Grade::Aa => 4.5,
            Grade::Aaa => 7.0,
        }
    }
}

/// What a program's truecolour and 256-colour text does.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub enum Truecolour {
    /// As sent, graded.
    #[default]
    AsSent,
    /// Snapped to the nearest of the theme's sixteen: the program wears the theme.
    Snapped,
}

/// How often TIDY suggests groups on its own.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub enum TidyEvery {
    #[default]
    Off,
    Hourly,
    Daily,
}

fn default_sync_every() -> u16 {
    10
}

fn default_ask_ctx() -> Vec<String> {
    vec!["shell".into(), "block".into(), "page".into()]
}

pub fn default_hidden_processes() -> Vec<String> {
    crate::app::SYSTEM_PROCS.iter().map(|s| s.to_string()).collect()
}

/// How loud the prompt line's language server is.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub enum PromptLsp {
    Off,
    /// A dotted underline for a diagnostic, hover for the message;
    /// completions ride the ghost and Tab accepts.
    #[default]
    Quiet,
    /// A small completion list under the caret.
    Menu,
}

/// Where a URL typed at a prompt goes.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum PromptUrl {
    Split,
    NewTab,
}

/// The sidebar header: what it does is what it's called.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum HeaderStyle {
    /// One ruled row: the window's name and NEW TAB.
    Bar,
    /// A rail of windows along the sidebar's edge; the name above the tabs.
    Rail,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct HeaderPrefs {
    pub style: HeaderStyle,
    /// The name set large in Newsreader (masthead) instead of caps.
    pub masthead: bool,
    /// A dateline under the name: where · tabs · ports.
    pub dateline: bool,
    /// NEW TAB in the header row.
    pub header_button: bool,
    /// The ruled line after the last tab is also NEW TAB.
    pub next_row: bool,
    /// The window's name in the bar's cell (else just its square).
    pub show_name: bool,
    /// A split caret on NEW TAB that fans the kinds out.
    pub kinds_caret: bool,
    /// The rail only while the pointer is over the sidebar.
    pub rail_hover: bool,
    /// Press feedback on NEW TAB: a flash through the signal.
    pub flash: bool,
}

impl Default for HeaderPrefs {
    fn default() -> Self {
        HeaderPrefs { style: HeaderStyle::Bar, masthead: false, dateline: false, header_button: true, next_row: true, show_name: true, kinds_caret: true, rail_hover: false, flash: true }
    }
}

/// The terminal cursor, the app's way.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum CursorShapePref {
    Shell,
    Block,
    Beam,
    Underline,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Blink {
    Never,
    AfterIdle,
    Always,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum CursorColor {
    /// The theme's caret token (the ink unless the theme says).
    #[serde(alias = "Ink")]
    Theme,
    Signal,
    Tab,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum CursorMotion {
    Jump,
    Glide,
    Comet,
    /// Neovide's smear: the body stretches from where it was to where it
    /// goes, the tail catching up a beat behind the head.
    Smear,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CursorPrefs {
    pub shape: CursorShapePref,
    pub blink: Blink,
    /// Blink period, ms.
    pub period: u32,
    pub color: CursorColor,
    pub motion: CursorMotion,
    /// Beam / underline weight, logical px.
    pub weight: f32,
    pub hollow_unfocused: bool,
    pub hide_while_typing: bool,
    /// Neovide's trail_size: how far the smear's tail lags its head, 0..1.
    #[serde(default = "default_smear")]
    pub smear: f32,
}

fn default_smear() -> f32 {
    1.0
}

impl Default for CursorPrefs {
    fn default() -> Self {
        CursorPrefs { shape: CursorShapePref::Shell, blink: Blink::Never, period: 530, color: CursorColor::Theme, motion: CursorMotion::Jump, weight: 2.0, hollow_unfocused: true, hide_while_typing: true, smear: 1.0 }
    }
}

/// How the window comes up.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum WindowStart {
    Last,
    Maximized,
    Fullscreen,
    Centered,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum SplashMode {
    Draw,
    Still,
    None,
}

/// Which comes first: a terminal that also browses, or a browser that
/// also has shells. It sets what NEW TAB opens with nothing typed, what
/// leads the palette, and (once, when picked) LINKS FROM OTHER APPS.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub enum Lead {
    #[default]
    Terminal,
    Browser,
}

/// What happens once the splash has gone.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Then {
    Restore,
    Shell,
    LastPage,
    /// The layout named in `then_layout` (a name under profile/layouts).
    Layout,
    /// The prompt: a terminal with no PTY behind it (home.rs).
    Prompt,
    /// One page, `home_url`, as the whole window.
    HomePage,
    /// The command palette, without creating a tab.
    Palette,
}

/// What a new window comes up as. A window is a surface of its own — its
/// tabs, its name, and the folder it works in — never a copy of the one
/// that asked. The prompt offers folders in its rows; a shell is born in
/// the asking window's folder; launch does what the first window does.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub enum NewWindow {
    #[default]
    Prompt,
    Shell,
    Launch,
}

/// A tab opened by something other than your own hand (`nus open`, an
/// assistant, a rule, a link from outside): behind with a toast, or in front.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub enum OpenedBy {
    #[default]
    Behind,
    Front,
}

/// What the prompt looks like: the line alone, or the line under the
/// plate — the icon at a plate's size, your last places as stops on its
/// band (plate.rs). The same typing, rows and enter either way.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub enum HomeLook {
    #[default]
    Line,
    Plate,
    /// One of the arts (art.rs): `home_art` names it.
    Art,
}

/// What a sideways swipe on a page draws while it goes (swipe.rs): the
/// arrow alone, the arrow with the words for what it will do, a band down
/// the page's edge, or nothing.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub enum SwipeLook {
    #[default]
    Arrow,
    Card,
    Edge,
    Off,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum AtlasMode {
    Planet,
    AtLaunch,
    Persistent,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Outside {
    Little,
    NewTab,
}

/// Tab and terminal behaviour the settings page edits.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Behavior {
    #[serde(default)] pub app_icon: crate::app_icon::Choice,
    #[serde(default = "default_true")]
    pub update_checks: bool,
    pub links: Links,
    pub prompt_url: PromptUrl,
    /// Closing a tab with a foreground process asks first.
    pub close_asks: bool,
    pub default_profile: usize,
    pub follow_os_theme: bool,
    /// The Start modal at launch, and its chime.
    pub start_on_launch: bool,
    pub startup_sound: bool,
    #[serde(default = "default_window_start")]
    pub window_start: WindowStart,
    #[serde(default = "default_splash")]
    pub splash: SplashMode,
    /// Seconds the splash holds at least.
    #[serde(default = "default_splash_hold")]
    pub splash_hold: f32,
    #[serde(default = "default_then")]
    pub then: Then,
    #[serde(default = "default_atlas")]
    pub atlas: AtlasMode,
    #[serde(default = "default_outside")]
    pub outside: Outside,
    /// Terminal first or browser first.
    #[serde(default)]
    pub lead: Lead,
    /// Shells get prompt marks, cwd and exit codes injected at spawn.
    #[serde(default = "default_true")]
    pub shell_integration: bool,
    /// Colour the command line's tokens as you type.
    #[serde(default = "default_true")]
    pub highlight: bool,
    /// The editor formats through the language server on Ctrl+S.
    #[serde(default = "default_true")]
    pub format_on_save: bool,
    /// The prompt line's language server: quiet, a menu, or off.
    #[serde(default)]
    pub prompt_lsp: PromptLsp,
    // Sync: the carriers, what travels, how often.
    #[serde(default)]
    pub sync_folder: String,
    #[serde(default)]
    pub sync_git: String,
    #[serde(default)]
    pub sync_session: bool,
    /// Minutes between exchanges; 0 = only on demand and at quit.
    #[serde(default = "default_sync_every")]
    pub sync_every_min: u16,
    #[serde(default = "default_true")]
    pub sync_at_quit: bool,
    /// OSC 10/11 from a shell: a chip offers the look, or it applies, or the pane only.
    #[serde(default)]
    pub shell_colours: ShellColours,
    /// SHELL COLORS: how a new shell stack gets its color (shell_colors.rs).
    #[serde(default)]
    pub shell_tint: crate::shell_colors::ShellTint,
    /// Program colours: the contrast they must reach, and whether their
    /// truecolour wears the theme.
    #[serde(default)]
    pub grade: Grade,
    #[serde(default)]
    pub truecolour: Truecolour,
    /// Tidy: how often to suggest groups; dedupe bands on/off.
    #[serde(default)]
    pub tidy_every: TidyEvery,
    #[serde(default = "default_true")]
    pub dedupe: bool,
    /// ssh profiles bring the shell integration to the remote.
    #[serde(default = "default_true")]
    pub ssh_integration: bool,
    /// THEN · LAYOUT: which saved layout opens at launch.
    #[serde(default)]
    pub then_layout: String,
    /// The assistant's default context, as chip keys (shell, block, page, tabs, editor, memory).
    #[serde(default = "default_ask_ctx")]
    pub ask_ctx: Vec<String>,
    /// OSC 9;4 progress: in the sidebar row and strip crumb, on the taskbar button.
    #[serde(default = "default_true")]
    pub progress_sidebar: bool,
    /// SIDEBAR · ASSISTANTS IN TAB ROWS: the Ledger's lines (ledger.rs).
    #[serde(default = "default_true")]
    pub ledger: bool,
    #[serde(default = "default_true")]
    pub progress_taskbar: bool,
    /// Blocks: lamps in the gutter, and output longer than this folds itself (0 = never).
    #[serde(default = "default_true")]
    pub blocks: bool,
    #[serde(default)]
    pub fold_over: u32,
    /// The journal: one line per finished block, per folder, kept this many days.
    #[serde(default = "default_true")]
    pub journal: bool,
    #[serde(default = "default_journal_keep")]
    pub journal_keep: u32,
    /// Cut off: what a restart-killed command's chip does. Chip · run again · nothing.
    #[serde(default)]
    pub cutoff: CutOff,
    /// Ports that remember: departed dev servers with a known command come back
    /// on the board with a start-again action.
    #[serde(default = "default_true")]
    pub ports_remember: bool,
    /// Held: shells run in a holder process that outlives the app. At quit
    /// an idle prompt is let go; a running command is kept.
    #[serde(default)]
    pub keep_alive: KeepAlive,
    /// The page THEN · HOME PAGE opens.
    #[serde(default = "default_home_url")]
    pub home_url: String,
    /// HOME: the prompt as the line alone, or under the plate.
    #[serde(default)]
    pub home_look: HomeLook,
    /// BACK & FORWARD · SWIPE OVERLAY and how far a swipe goes (swipe.rs).
    #[serde(default)]
    pub swipe_look: SwipeLook,
    #[serde(default = "default_swipe_reach")]
    pub swipe_reach: u16,
    /// TABS · OPENED BY OTHERS.
    #[serde(default)]
    pub opened_by_others: OpenedBy,
    /// STARTUP · A NEW WINDOW.
    #[serde(default)]
    pub new_window: NewWindow,
    /// ASSISTANTS · the backend the ask panel uses ("" = the best on the machine).
    #[serde(default)]
    pub ask_backend: String,
    #[serde(default)]
    pub prompt: crate::prompt::Config,
    #[serde(default)]
    pub assistants: crate::assistants::Config,
    #[serde(default)]
    pub typography: crate::fonts::Typography,
    /// SYNC · THE PHONE: this window served as a page on the LAN.
    #[serde(default)]
    pub phone: bool,
    /// HOME · ART: which art plays behind the line (a built-in's name or a file's stem).
    #[serde(default = "default_home_art")]
    pub home_art: String,
    /// Where this machine is, for the sky: [lat, lon]; none = not configured; never inferred.
    #[serde(default)]
    pub place: Option<[f32; 2]>,
    /// None uses the curated nine; a list is the user's footer collection.
    #[serde(default)]
    pub footer_themes: Option<Vec<String>>,
    #[serde(default)] pub download_rename: crate::downloads::Rename,
    #[serde(default)] pub ui_font: crate::fonts::Family,
    #[serde(default)] pub ui_weight: crate::fonts::Weight,
    #[serde(default)] pub term_font: crate::fonts::Family,
    #[serde(default)] pub term_weight: crate::fonts::Weight,
    /// Remember tabs and windows between launches (session.json); off, nothing is written.
    #[serde(default = "default_true")]
    pub remember: bool,
    /// A plain click on a URL in the shell: ask first, open, or leave it to hints mode.
    #[serde(default)]
    pub link_click: LinkClick,
    /// The loop: Alt+Shift+click on a localhost page opens the editor at the element's source.
    #[serde(default = "default_true")]
    pub click_to_source: bool,
    /// PICTURE IN PICTURE · THE BAND: the signal stripe along the top of
    /// the floating window — what says it is ours and where you take
    /// hold of it. On.
    #[serde(default = "default_true")]
    pub pip_band: bool,
    #[serde(default)]
    pub pip_policy: crate::pip_policy::Policy,
    #[serde(default)]
    pub viewers: crate::file_viewer::Preferences,
    /// PICTURE IN PICTURE · PROGRESS RULE: a hairline of played time
    /// along the foot, there whether or not the controls are. Off: the
    /// controls carry the scrubber, and a resting window stays a picture.
    #[serde(default)]
    pub pip_progress: bool,
    #[serde(default = "default_pip_skip")]
    pub pip_skip_seconds: u16,
    /// Replay: casts and checkpoints under profile/replay, kept this long.
    #[serde(default)]
    pub replay: ReplayKeep,
    /// Hands: what an assistant may do on the page beside its shell.
    #[serde(default)]
    pub hands: HandsMode,
    /// Hosts where hands need no asking (ALLOW ON THIS HOST).
    #[serde(default)]
    pub hands_hosts: Vec<String>,
    /// Even on an allowed host, a submit (Enter, a submit button, a navigation) asks.
    #[serde(default = "default_true")]
    pub hands_confirm_submit: bool,
    // The ports board.
    #[serde(default)]
    pub ports_grouping: PortsGrouping,
    #[serde(default)]
    pub ports_open: PortsOpen,
    /// Seconds between polls while the board is open (10 s when closed).
    #[serde(default = "default_ports_poll")]
    pub ports_poll: u8,
    #[serde(default = "default_true")]
    pub ports_toast: bool,
    #[serde(default)]
    pub ports_show_system: bool,
    #[serde(default)]
    pub ports_show_udp: bool,
    #[serde(default = "default_true")]
    pub ports_show_connections: bool,
    #[serde(default = "default_true")]
    pub ports_show_docker: bool,
    #[serde(default)]
    pub ports_kill_confirm: KillConfirm,
    #[serde(default = "default_true")]
    pub ports_probe: bool,
    #[serde(default)]
    pub ports_tunnel: Tunnel,
    /// Process names the board hides (the system set to start).
    #[serde(default = "default_hidden_processes")]
    pub ports_hidden: Vec<String>,
    // The hatch.
    #[serde(default)]
    pub hatch_look: HatchLook,
    #[serde(default)]
    pub hatch_hotkey: crate::hotkey::Chord,
    /// The sheet's height as a percentage of the monitor.
    #[serde(default = "default_hatch_size")]
    pub hatch_size: u8,
    #[serde(default)]
    pub hatch_monitor: HatchMonitor,
    #[serde(default = "default_true")]
    pub hatch_autohide: bool,
    #[serde(default)]
    pub hatch_spaces: HatchSpaces,
    #[serde(default = "default_true")]
    pub hatch_status: bool,
    #[serde(default = "default_true")]
    pub hatch_background: bool,
    #[serde(default)]
    pub hatch_dim: bool,
    #[serde(default)]
    pub hatch_notify: bool,
    #[serde(default)]
    pub menu_drawer: crate::menu_drawer::Config,
    /// Ghost the history entry that continues what's typed; Right/End accepts.
    #[serde(default = "default_true")]
    pub predict: bool,
    /// Refuse requests to known ad and tracker hosts.
    #[serde(default = "default_true")]
    pub block_content: bool,
    /// Blank an idle page after this many minutes (0 = never).
    #[serde(default = "default_sleep")]
    pub sleep_after_min: u32,
    /// Archive an idle page tab into recently closed after this many hours (0 = never).
    #[serde(default = "default_archive")]
    pub archive_after_h: u32,
    /// The page's status at the end of its tools row: a lamp, the word, or nothing.
    #[serde(default)]
    pub status: Status,
    /// A selection in the shell copies itself as it ends.
    #[serde(default)]
    pub copy_on_select: bool,
    /// The middle button pastes into the shell.
    #[serde(default)]
    pub middle_paste: bool,
    /// What programs in the shell may do with the clipboard through OSC 52.
    #[serde(default)]
    pub osc52: Osc52,
    /// The cluster at a split pane's corner: on hover, always, never.
    #[serde(default)]
    pub pane_controls: crate::panes::Controls,
    /// The rule between a split's panes drags to resize.
    #[serde(default = "default_true")]
    pub pane_divider: bool,
    /// How the shell's view moves: neoscroll's curves, or at once.
    #[serde(default)]
    pub scroll_easing: crate::scrolling::Easing,
    /// Lines per wheel tick in the shell.
    #[serde(default = "default_wheel_lines")]
    pub wheel_lines: u32,
    /// Chromium's smooth scrolling in pages (takes a restart).
    #[serde(default = "default_true")]
    pub page_smooth_scroll: bool,
    /// Logical px one wheel notch moves a page or one of nus's own lists.
    #[serde(default = "default_wheel_px")]
    pub wheel_px: u16,
    /// Pages' scrollbars: Chromium's overlay ones, the classic ones, or none (hidden applies live; the others when Chromium starts).
    #[serde(default)]
    pub scrollbars: Scrollbars,
    /// Where downloads go; empty is ~/Downloads.
    #[serde(default)]
    pub download_dir: String,
    /// Ask where to save each download, with the system's dialog.
    #[serde(default)]
    pub download_ask: bool,
    /// What happens when a download finishes.
    #[serde(default)]
    pub download_done: DownloadDone,
    /// Pages' zoom when a site has none of its own, in percent.
    #[serde(default = "default_zoom")]
    pub page_zoom: u16,
    /// Send Global Privacy Control and DNT with every request.
    #[serde(default)]
    pub privacy_signal: bool,
    /// Lines of scrollback a new shell keeps.
    #[serde(default = "default_scrollback")]
    pub scrollback: u32,
}

fn default_wheel_lines() -> u32 {
    3
}
fn default_wheel_px() -> u16 {
    100
}
fn default_zoom() -> u16 {
    100
}
fn default_scrollback() -> u32 {
    10_000
}

/// Pages' scrollbars, as Chromium draws them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Scrollbars {
    /// Thin, over the content, gone when still (the system's on macOS).
    #[default]
    Overlay,
    /// The classic track beside the content.
    Classic,
    /// None drawn; the wheel and the keys still scroll.
    Hidden,
}

impl Scrollbars {
    pub const ALL: [Scrollbars; 3] = [Scrollbars::Overlay, Scrollbars::Classic, Scrollbars::Hidden];
    pub fn name(self) -> &'static str {
        match self {
            Scrollbars::Overlay => "overlay",
            Scrollbars::Classic => "classic",
            Scrollbars::Hidden => "hidden",
        }
    }
}

/// What a finished download does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DownloadDone {
    /// A notice in the footer.
    #[default]
    Notice,
    /// The file, shown in its folder.
    Reveal,
    /// The file, opened with its program.
    Open,
    /// Nothing said.
    Quiet,
}

impl DownloadDone {
    pub const ALL: [DownloadDone; 4] = [DownloadDone::Notice, DownloadDone::Reveal, DownloadDone::Open, DownloadDone::Quiet];
    pub fn name(self) -> &'static str {
        match self {
            DownloadDone::Notice => "notice",
            DownloadDone::Reveal => "reveal",
            DownloadDone::Open => "open",
            DownloadDone::Quiet => "quiet",
        }
    }
}

/// OSC 52: tmux, neovim and friends setting (and reading) the clipboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Osc52 {
    Off,
    /// Programs may set the clipboard; reading it back is refused.
    #[default]
    Write,
    /// Programs may set it and read it.
    ReadWrite,
}

/// How a page says it's live, loading, local or asleep.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Status {
    /// A small lamp: signal while loading, ink when live, Plot corners when local, hollow asleep.
    #[default]
    Lamp,
    /// The lamp and the word.
    Both,
    /// The word alone (LIVE · LOCAL · ASLEEP).
    Word,
    None,
}

fn default_sleep() -> u32 {
    30
}
fn default_archive() -> u32 {
    12
}

fn default_home_url() -> String {
    "https://cbassuarez.com/nus.dev".into()
}

/// What a click on a link in the shell does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum LinkClick {
    #[default]
    Ask,
    Open,
    HintsOnly,
}

/// How long replay sessions are kept.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ReplayKeep {
    #[default]
    Days7,
    Day1,
    Off,
}

impl ReplayKeep {
    pub fn days(self) -> Option<u32> {
        match self {
            ReplayKeep::Days7 => Some(7),
            ReplayKeep::Day1 => Some(1),
            ReplayKeep::Off => None,
        }
    }
}

/// What an assistant's hands may do on a page.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum HandsMode {
    #[default]
    Ask,
    Always,
    Never,
}

/// Whether shells are held (see `nus_pty::hold`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum KeepAlive {
    #[default]
    On,
    Off,
}

fn default_journal_keep() -> u32 {
    30
}

/// What a command the restart killed gets on the next launch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CutOff {
    #[default]
    Chip,
    RunAgain,
    Off,
}

fn default_true() -> bool {
    true
}

fn default_window_start() -> WindowStart {
    WindowStart::Last
}
fn default_splash() -> SplashMode {
    SplashMode::Draw
}
fn default_splash_hold() -> f32 {
    1.0
}
fn default_home_art() -> String {
    "pond".into()
}
fn default_then() -> Then {
    Then::Prompt
}
fn default_atlas() -> AtlasMode {
    AtlasMode::Planet
}
fn default_outside() -> Outside {
    Outside::Little
}

impl Default for Behavior {
    fn default() -> Self {
        Behavior {
            app_icon: Default::default(),
            update_checks: true,
            links: Links::Stack,
            prompt_url: PromptUrl::Split,
            close_asks: true,
            default_profile: 0,
            follow_os_theme: true,
            start_on_launch: false,
            startup_sound: false,
            window_start: WindowStart::Last,
            splash: SplashMode::Draw,
            splash_hold: 1.0,
            then: Then::Prompt,
            atlas: AtlasMode::Planet,
            outside: Outside::Little,
            lead: Lead::Terminal,
            shell_integration: true,
            highlight: true,
            format_on_save: true,
            prompt_lsp: PromptLsp::Quiet,
            blocks: true,
            fold_over: 0,
            journal: true,
            journal_keep: 30,
            cutoff: CutOff::Chip,
            ports_remember: true,
            keep_alive: KeepAlive::On,
            hands: HandsMode::Ask,
            replay: ReplayKeep::Days7,
            click_to_source: true,
            pip_band: true,
            pip_policy: Default::default(),
            viewers: Default::default(),
            pip_progress: false,
            pip_skip_seconds: default_pip_skip(),
            link_click: LinkClick::Ask,
            home_url: default_home_url(),
            home_look: HomeLook::Line,
            swipe_look: SwipeLook::Arrow,
            swipe_reach: default_swipe_reach(),
            opened_by_others: OpenedBy::Behind,
            new_window: NewWindow::Prompt,
            ask_backend: String::new(),
            prompt: Default::default(),
            assistants: Default::default(),
            typography: Default::default(),
            phone: false,
            home_art: default_home_art(),
            place: None,
            footer_themes: None,
            download_rename: Default::default(),
            ui_font: Default::default(), ui_weight: Default::default(), term_font: Default::default(), term_weight: Default::default(),
            remember: true,
            hands_hosts: Vec::new(),
            hands_confirm_submit: true,
            progress_sidebar: true,
            ledger: true,
            progress_taskbar: true,
            ask_ctx: default_ask_ctx(),
            then_layout: String::new(),
            ssh_integration: true,
            shell_colours: ShellColours::Chip,
            shell_tint: crate::shell_colors::ShellTint::Random,
            grade: Grade::Aa,
            truecolour: Truecolour::AsSent,
            sync_folder: String::new(),
            sync_git: String::new(),
            sync_session: false,
            sync_every_min: 10,
            sync_at_quit: true,
            tidy_every: TidyEvery::Off,
            dedupe: true,
            ports_grouping: PortsGrouping::Origin,
            ports_open: PortsOpen::Split,
            ports_poll: 1,
            ports_toast: true,
            ports_show_system: false,
            ports_show_udp: false,
            ports_show_connections: true,
            ports_show_docker: true,
            ports_kill_confirm: KillConfirm::System,
            ports_probe: true,
            ports_tunnel: Tunnel::Cloudflared,
            ports_hidden: default_hidden_processes(),
            hatch_look: HatchLook::Sheet,
            hatch_hotkey: crate::hotkey::Chord::platform_default(),
            hatch_size: 40,
            hatch_monitor: HatchMonitor::Pointer,
            hatch_autohide: true,
            hatch_spaces: HatchSpaces::One,
            hatch_status: true,
            hatch_background: true,
            hatch_dim: false,
            hatch_notify: false,
            menu_drawer: crate::menu_drawer::Config::default(),
            predict: true,
            block_content: true,
            sleep_after_min: 30,
            archive_after_h: 12,
            status: Status::Lamp,
            copy_on_select: false,
            middle_paste: false,
            osc52: Osc52::Write,
            pane_controls: crate::panes::Controls::Near,
            pane_divider: true,
            scroll_easing: crate::scrolling::Easing::Cubic,
            wheel_lines: 3,
            page_smooth_scroll: true,
            wheel_px: default_wheel_px(),
            scrollbars: Scrollbars::Overlay,
            download_dir: String::new(),
            download_ask: false,
            download_done: DownloadDone::Notice,
            page_zoom: default_zoom(),
            privacy_signal: false,
            scrollback: default_scrollback(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Slider {
    PipSkip,
    Tint,
    Texture,
    Opacity,
    ShellWidth,
    Radius,
    Grace,
    SidebarWidth,
    FooterSize,
    Motion,
    BarThickness,
    BarChase,
    TexScale,
    Angle,
    Drift,
    Breath,
    Volume,
    SplashHold,
    Saturation,
    BlinkPeriod,
    CurWeight,
    Smear,
    Hue,
    Sat,
    Light,
}

/// The prompt line's language servers: bundle id, the shells it reads.
pub(crate) const LSP_TOOLS: [(&str, &str); 2] = [("bash-language-server", "BASH & ZSH"), ("powershell-editor-services", "POWERSHELL")];

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Hit {
    AppIcon(crate::app_icon::Choice),
    Mercury,
    CopySupportDetails,
    RecoverPrevious,
    UpdateCheck,
    UpdateInstall,
    UpdateConfirm,
    UpdateCancel,
    UpdateChecks(bool),
    /// UPDATES · PROFILE: this copy keeps its own (`true`) or shares (install.rs).
    ProfileSeparate(bool),
    /// UPDATES · PROFILE: show the channel's profiles in the file manager.
    ProfileFolder,
    Report(crate::support::Kind),
    Workspace(workspace::Hit),
    Section(usize),
    Theme(Option<bool>),
    Signal(Color),
    Base(Option<Color>),
    Shell(Shell),
    /// A slider bar: kind, bar x, bar width.
    Slider(Slider, f32, f32),
    Side(Side),
    HoverFrom(HoverFrom),
    Fullscreen(Fullscreen),
    Pin(bool),
    Links(Links),
    PromptUrl(PromptUrl),
    CloseAsks(bool),
    DefaultProfile(usize),
    ReloadRules,
    OpenRules,
    ResetRules,
    Reduce(Option<bool>),
    BarStyle(BarStyle),
    BarColor(BarColor),
    /// Tile grid → section, and back.
    Tile(usize),
    Back,
    MakeDefault,
    Unregister,
    Widevine,
    /// PROMPT LSP: GET or REMOVE a prompt language server (index into LSP_TOOLS).
    LspTool(usize),
    StartOnLaunch(bool),
    StartupSound(bool),
    ReloadAvatar,
    /// Browse for a picture for the face, on any platform.
    PickAvatar,
    OpenProfileDir,
    Preset(usize),
    SavePreset,
    OpenPresets,
    StopSel(usize),
    StopAdd,
    StopRemove,
    StopColor(Color),
    OpacityOn(OpacityOn),
    TexKind(TextureKind),
    TexOn(TextureOn),
    TexMotion(bool),
    TokPaper(Color),
    TokInk(Color),
    TokPage(Color),
    TokCaret(Option<Color>),
    TokSelection(Option<Color>),
    TokReset,
    AnsiSel(usize),
    AnsiSet(Color),
    Family(crate::theme_edit::Family),
    Import(usize),
    OpenThemes,
    Starter(usize),
    LookTab(usize),
    TokSel(TokSel),
    /// Set the selected token to this colour.
    TokSet(Color),
    ShellInt(bool),
    Welcome,
    PinDisplay(crate::pins::Display),
    Block(bool),
    StatusStyle(Status),
    SleepAfter(u32),
    ArchiveAfter(u32),
    Highlight(bool),
    CopyOnSelect(bool),
    PaneControls(crate::panes::Controls),
    ScrollEasing(crate::scrolling::Easing),
    WheelLines(u32),
    PageSmooth(bool),
    PaneDivider(bool),
    MiddlePaste(bool),
    Osc52(Osc52),
    Predict(bool),
    PromptLsp(PromptLsp),
    FormatOnSave(bool),
    Blocks(bool),
    Journal(bool),
    JournalKeep(u32),
    CutOffMode(CutOff),
    PortsRemember(bool),
    KeepAlive(KeepAlive),
    Hands(HandsMode),
    Replay(ReplayKeep),
    ClickToSource(bool),
    PipBand(bool),
    PipPolicy(crate::pip_policy::Event, bool),
    Viewer(crate::file_viewer::Setting),
    PipProgress(bool),
    LinkClick(LinkClick),
    Remember(bool),
    SetLaunchTabs,
    ClearLaunchTabs,
    HandsSubmit(bool),
    HandsForget,
    FoldOver(u32),
    ProgressSidebar(bool),
    Ledger(bool),
    ProgressTaskbar(bool),
    AskCtx(crate::askctx::Ctx),
    ForgetMemory,
    SshIntegration(bool),
    TidyEvery(TidyEvery),
    Dedupe(bool),
    ShellColours(ShellColours),
    ShellTint(crate::shell_colors::ShellTint),
    Lead(Lead),
    Grade(Grade),
    Truecolour(Truecolour),
    /// The profile card at one of its steps (name, face, device).
    MeEdit(u8),
    MeCard,
    MeFolder,
    MeForget,
    SyncSession(bool),
    SyncEvery(u16),
    SyncAtQuit(bool),
    SyncForget,
    SyncNow,
    SyncKey,
    SyncEdit(u8),
    /// The card's walk, at how the profile lives / the forge.
    MeWalk(u8),
    ForgeForget,
    PortsGrouping(PortsGrouping),
    PortsOpen(PortsOpen),
    PortsPoll(u8),
    PortsToast(bool),
    PortsShow(u8, bool),
    PortsKill(KillConfirm),
    PortsProbe(bool),
    PortsTunnel(Tunnel),
    PortsHidden,
    HatchLook(HatchLook),
    HatchHotkey(crate::hotkey::Chord),
    /// Record a hotkey of your own: the next chord pressed.
    HatchRecord,
    HatchSize(u8),
    HatchMonitor(HatchMonitor),
    HatchAutohide(bool),
    HatchSpaces(HatchSpaces),
    HatchStatus(bool),
    HatchBackground(bool),
    HatchDim(bool),
    HatchNotify(bool),
    MenuEnabled(bool),
    MenuSignal(crate::menu_drawer::SignalStyle),
    MenuDensity(crate::menu_drawer::Module,crate::menu_drawer::Density),
    MenuMove(crate::menu_drawer::Module,bool),
    MenuNames(bool),
    MenuRecent(bool),
    MenuPreview,
    HdrStyle(HeaderStyle),
    HdrMasthead(bool),
    HdrDateline(bool),
    HdrButton(bool),
    HdrNextRow(bool),
    HdrName(bool),
    HdrCaret(bool),
    HdrRailHover(bool),
    HdrFlash(bool),
    Compact(bool),
    SmallTabs(crate::sidebar::SmallTabs),
    DownloadRename(crate::downloads::Rename),
    /// 0 choose a folder · 1 back to the default · 2 open it.
    DownloadDir(u8),
    DownloadAsk(bool),
    DownloadDone(DownloadDone),
    WheelPx(u16),
    Scrollbars(Scrollbars),
    PageZoom(u16),
    PrivacySignal(bool),
    Scrollback(u32),
    /// 0 cookies · 1 the cache.
    ClearBrowsing(u8),
    Downloads,
    CurShape(CursorShapePref),
    CurBlink(Blink),
    CurColor(CursorColor),
    CurMotion(CursorMotion),
    CurHollow(bool),
    CurHide(bool),
    WindowStart(WindowStart),
    Splash(SplashMode),
    HomeLook(HomeLook),
    SwipeLook(SwipeLook),
    SwipeReach(u16),
    OpenedBy(OpenedBy),
    NewWindow(NewWindow),
    /// ASSISTANTS · ASK WITH: an index into the backends on the machine.
    AskBackend(usize),
    /// SYNC · THE PHONE, on or off.
    Phone(bool),
    CopyPhoneUrl,
    /// An art from the picker, by its place in art::list().
    HomeArt(usize),
    AddArt,
    AskArt,
    OpenArtFolder,
    PlaceEdit,
    FooterTheme(usize),
    FooterDefaults,
    Search,
    UiFont(crate::fonts::Family), UiWeight(crate::fonts::Weight),
    TermFont(crate::fonts::Family), TermWeight(crate::fonts::Weight),
    Then(Then),
    StartupLayout(usize),
    EditHomeUrl,
    Atlas(AtlasMode),
    Outside(Outside),
    LoginItem(bool),
    SoundOn(bool),
    /// Play a cue by index into sound::NAMES.
    Play(usize),
    /// Event (index into sound::EVENTS) → cue index, or usize::MAX for quiet.
    EventCue(usize, usize),
    EventNext(usize),
}

/// The sections, grouped by what they're about: how nus looks, how it
/// feels, what you work in, and the machine.
pub const GROUPS: [(&str, std::ops::Range<usize>); 8] = [("LOOK", 0..1), ("FEEL", 1..5), ("WORK", 5..9), ("COMMANDS",19..20), ("SYSTEM", 9..12), ("DESKTOP", 17..19), ("YOU", 12..15), ("PERSONALIZE", 15..17)];

/// The look studio's tabs.
pub const LOOK_TABS: [&str; 6] = ["PRESETS", "SURFACE", "TOKENS", "TYPE & MOTION", "CURSOR", "APP ICON"];
pub const LOOK_PRESETS: usize = 0;
pub const LOOK_SURFACE: usize = 1;
pub const LOOK_TOKENS: usize = 2;
pub const LOOK_TYPE: usize = 3;
pub const LOOK_CURSOR: usize = 4;
pub const LOOK_APP_ICON: usize = 5;

/// Which token the picker is editing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TokSel {
    Signal,
    Stop(usize),
    Paper,
    Ink,
    Page,
    Caret,
    Selection,
    Ansi(usize),
}

pub const SECTIONS: [(&str, (&str, &str)); 20] = [
    ("LOOK", icons::PALETTE),
    ("SOUND", icons::SPEAKER),
    ("START/NEW TAB", icons::ROCKET),
    ("SIDEBAR", icons::SIDEBAR),
    ("TABS", icons::SQUARES),
    ("TERMINAL", icons::TERMINAL),
    ("BROWSER", icons::GLOBE),
    ("PORTS", icons::PORTS),
    ("HATCH", icons::TERMINAL),
    ("ASSISTANTS", icons::ASSISTANT),
    ("RULES", icons::CODE),
    ("KEYS", icons::KEYBOARD),
    ("SYNC", icons::BROADCAST),
    ("PROFILE", icons::USER),
    ("UPDATES", icons::DOWNLOAD),
    ("FONTS", icons::CODE),
    ("PROMPT", icons::SEARCH),
    ("MENU & TRAY", icons::SQUARES),
    ("FILE VIEWERS", icons::BOOK),
    ("SAVED COMMANDS", icons::COMMAND),
];
pub const SEC_SAVED: usize = 19;

pub const SEC_VIEWERS: usize = 18;
pub const SEC_MENU: usize = 17;
pub const SEC_FONTS: usize = 15;
pub const SEC_PROMPT: usize = 16;
pub const SEC_ASSISTANTS: usize = 9;
pub const SEC_LOOK: usize = 0;
pub const SEC_SOUND: usize = 1;
pub const SEC_STARTUP: usize = 2;
pub const SEC_TERMINAL: usize = 5;
pub const SEC_BROWSER: usize = 6;
pub const SEC_PORTS: usize = 7;
pub const SEC_HATCH: usize = 8;
pub const SEC_SYNC: usize = 12;
pub const SEC_PROFILE: usize = 13;
pub const RULES: usize = 10;

/// How many lines a run of widths takes at `gap` apart within `width`.
fn wrap_count(widths: &[f32], gap: f32, width: f32) -> usize {
    let mut lines = 1;
    let mut x = 0.0;
    for &w in widths {
        if x > 0.0 && x + w > width {
            lines += 1;
            x = 0.0;
        }
        x += w + gap;
    }
    lines
}

pub(crate) fn key(k: &str, shift: bool) -> String {
    if cfg!(target_os = "macos") {
        format!("⌘{}{}", if shift { "⇧" } else { "" }, k)
    } else {
        format!("CTRL+{}{}", if shift { "SHIFT+" } else { "" }, k)
    }
}

/// One row's control.
enum Control {
    AppIcons,
    /// The intelligence ring and what a launch sends.
    Intelligence,
    SavedCommand(usize),
    Mercury,
    FontProof,
    PromptProof,
    /// The live proof of the current look: a miniature window.
    Studio,
    /// The studio's tab strip.
    Strip(Vec<(String, Hit, bool)>),
    /// Preset cards: name, ramp, signal, hit, current.
    Cards(Vec<(String, Vec<Color>, Color, f32, Hit, bool, Option<(Color, Color, Color, Color)>)>),
    /// Token tiles: name, colour (None = dashed "none/add"), caption, hit, selected, big.
    Tokens(Vec<(String, Option<Color>, String, Hit, bool)>, bool),
    Info(String),
    Choice(Vec<(String, Hit, bool)>),
    Slider(Slider, f32, String),
    Swatches(Vec<(Option<Color>, Hit, bool)>),
    Buttons(Vec<(String, (&'static str, &'static str), Hit)>),
    /// Coloured runs of text, as proof.
    Proof(Vec<(Color, String)>),
    /// A mini sidebar: (bg, signal, title, child) rows the rules produced.
    Tabs(Vec<(Option<Color>, Option<Color>, String, bool)>),
    /// The art picker: (key, name, says, hit, current, built-in), each card alive.
    Art(Vec<(String, String, String, Hit, bool, bool)>),
    /// A sound event: (event index, its cue's index or None for quiet, a note).
    /// One speaker toggle, the cue's name (click to hear it), a next.
    Cue(usize, Option<usize>, String),
    /// A chord as keycaps, then what it does.
    Keys(Vec<String>, String),
    /// A group heading within a page: the row's label, small and dim.
    Caption,
    /// A part of a page: a rule and a heading, above its captions.
    Section,
    /// The prompt's sources as a table: (source, before typing, while
    /// typing, how many), in display order.
    Sources(Vec<(crate::prompt::Source, bool, bool, u8)>),
    /// A number with − and +: (shown value, minus, plus).
    Stepper(String, Hit, Hit),
    /// What the setting above does as it is set now: quieter than a note,
    /// and joined to its setting with no rule between.
    Help(String),
    /// Picture cards: (name, caption, picture, hit, current). Each option
    /// drawn as what it does, so it's clear what you're clicking on.
    Pics(Vec<(String, String, Pic, Hit, bool)>),
    /// Settings · Menu: the real drawer and tray icon, live.
    DrawerPreview,
    /// Buttons that say what they do: (label, caption, icon, hit).
    Actions(Vec<(String, String, (&'static str, &'static str), Hit)>),
}

/// What a picture card shows: a small drawing of the option itself.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Pic {
    Setting(Hit),
    LeadTerminal,
    LeadBrowser,
    WinLast,
    WinMax,
    WinFull,
    WinCentered,
    SplashDraw,
    SplashStill,
    SplashNone,
    StartPrompt,
    StartHome,
    StartLayout,
    StartLast,
    LookLine,
    LookPlate,
    NewPrompt,
    NewShell,
    NewLaunch,
}

/// Group headings for a page's rows: each caption goes in before the row
/// whose label it names, so the pages read in parts.
fn captioned(mut rows: Vec<(String, Control)>, caps: &[(&str, &str)]) -> Vec<(String, Control)> {
    for (before, cap) in caps {
        if let Some(i) = rows.iter().position(|(k, _)| k == before) {
            rows.insert(i, (cap.to_string(), Control::Caption));
        }
    }
    rows
}

/// Section headings: stronger than a caption, for a page whose parts
/// must not run together (Start/New Tab: launch vs. every new tab).
fn sectioned(mut rows: Vec<(String, Control)>, heads: &[(&str, &str)]) -> Vec<(String, Control)> {
    for (before, head) in heads {
        if let Some(i) = rows.iter().position(|(k, _)| k == before) {
            rows.insert(i, (head.to_string(), Control::Section));
        }
    }
    rows
}

impl App {
    pub(crate) fn slider_value(&self, s: Slider) -> f32 {
        match s {
            Slider::Tint => self.surface.tint,
            Slider::Texture => self.surface.texture / 0.3,
            Slider::Opacity => (self.surface.opacity - 0.5) / 0.5,
            Slider::ShellWidth => (self.surface.shell_width - 1.0) / 11.0,
            Slider::Radius => self.surface.shell_radius / 24.0,
            Slider::Grace => self.sidebar_rules.grace_ms as f32 / 1000.0,
            Slider::SidebarWidth => (self.sidebar_rules.width-48.0)/432.0,
            Slider::FooterSize => (self.sidebar_rules.footer_row-28.0)/36.0,
            Slider::Smear => self.cursor.smear,
            Slider::Motion => self.motion.register,
            Slider::BarThickness => (self.load_bar.thickness - 1.0) / 5.0,
            Slider::BarChase => (self.load_bar.chase - 2.0) / 14.0,
            Slider::TexScale => (self.surface.texture_scale - 1.0) / 9.0,
            Slider::Angle => self.surface.angle / 360.0,
            Slider::Drift => self.surface.drift / 0.5,
            Slider::Breath => self.surface.breath,
            Slider::PipSkip => (self.behavior.pip_skip_seconds.clamp(1,120)-1) as f32 / 119.0,
            Slider::Volume => self.sound.prefs.volume,
            Slider::SplashHold => (self.behavior.splash_hold - 0.4) / 2.2,
            Slider::Saturation => (self.theme_edit.saturation - 0.5) / 1.0,
            Slider::Hue => surface::to_hsl(self.tok_color()).0,
            Slider::Sat => surface::to_hsl(self.tok_color()).1,
            Slider::Light => surface::to_hsl(self.tok_color()).2,
            Slider::BlinkPeriod => (self.cursor.period as f32 - 200.0) / 1000.0,
            Slider::CurWeight => (self.cursor.weight - 1.0) / 5.0,
        }
    }

    pub(crate) fn set_slider(&mut self, s: Slider, v: f32) {
        let v = v.clamp(0.0, 1.0);
        match s {
            Slider::Tint => self.surface.tint = v,
            Slider::Texture => self.surface.texture = v * 0.3,
            Slider::Opacity => self.surface.opacity = 0.5 + v * 0.5,
            Slider::ShellWidth => self.surface.shell_width = (1.0 + v * 11.0).round(),
            Slider::Radius => self.surface.shell_radius = (v * 24.0).round(),
            Slider::Grace => self.sidebar_rules.grace_ms = (v * 1000.0).round() as u64,
            Slider::SidebarWidth => {self.sidebar_rules.compact=false;self.sidebar_rules.width=(48.0+v*432.0).round();},
            Slider::FooterSize => self.sidebar_rules.footer_row=(28.0+v*36.0).round(),
            Slider::Smear => self.cursor.smear = v.clamp(0.1, 0.9),
            Slider::Motion => self.motion.register = v,
            Slider::BarThickness => self.load_bar.thickness = (1.0 + v * 5.0).round(),
            Slider::BarChase => self.load_bar.chase = (2.0 + v * 14.0).round(),
            Slider::TexScale => self.surface.texture_scale = ((1.0 + v * 9.0) * 2.0).round() / 2.0,
            Slider::Angle => self.surface.angle = (v * 360.0 / 15.0).round() * 15.0 % 360.0,
            Slider::Drift => self.surface.drift = (v * 0.5 * 100.0).round() / 100.0,
            Slider::Breath => self.surface.breath = (v * 20.0).round() / 20.0,
            Slider::PipSkip => self.behavior.pip_skip_seconds = (1.0 + v * 119.0).round() as u16,
            Slider::Volume => {
                self.sound.prefs.volume = (v * 20.0).round() / 20.0;
                self.sound.cue("tick");
            }
            Slider::SplashHold => self.behavior.splash_hold = ((0.4 + v * 2.2) * 10.0).round() / 10.0,
            Slider::Saturation => {
                self.theme_edit.saturation = ((0.5 + v) * 20.0).round() / 20.0;
                self.rebuild_theme();
            }
            Slider::Hue | Slider::Sat | Slider::Light => {
                let (h, sa, l) = surface::to_hsl(self.tok_color());
                let c = match s {
                    Slider::Hue => surface::from_hsl(v, sa.max(0.02), l, 1.0),
                    Slider::Sat => surface::from_hsl(h, v, l, 1.0),
                    _ => surface::from_hsl(h, sa, v, 1.0),
                };
                self.set_tok(c);
            }
            Slider::BlinkPeriod => self.cursor.period = ((200.0 + v * 1000.0) / 10.0).round() as u32 * 10,
            Slider::CurWeight => self.cursor.weight = ((1.0 + v * 5.0) * 2.0).round() / 2.0,
        }
        self.layout();
    }

    /// Registration is a `reg query` away; ask once per visit, not per frame.
    pub(crate) fn refresh_register_note(&mut self) {
        self.register_note = if crate::little::registered() {
            "registered as a browser · links from other apps open little".into()
        } else if cfg!(target_os = "windows") {
            "not registered · links from other apps would open elsewhere".into()
        } else {
            "registration needs an app bundle (macOS) or .desktop file (Linux) · v1".into()
        };
    }

    /// A click inside the settings pane. Returns true when it was handled.
    pub(crate) fn settings_key(&mut self, ev:&crate::app::KeyIn)->bool {
        use winit::keyboard::{Key,NamedKey};
        if ev.state!=winit::event::ElementState::Pressed || !self.tabs.get(self.active).is_some_and(|t|matches!(t.left,Pane::Settings(_))) {return false;}
        if self.settings_hits.is_empty(){return false;}
        match &ev.logical_key {
            Key::Named(NamedKey::ArrowLeft)|Key::Named(NamedKey::ArrowRight) if self.settings_focus.is_some()=>{
                let Some((_,Hit::Slider(kind,_,_)))=self.settings_hits.get(self.settings_focus.unwrap()).copied() else{return false;};
                let step=if kind==Slider::PipSkip {1.0/119.0}else{0.01};
                let delta=if ev.logical_key==Key::Named(NamedKey::ArrowLeft){-step}else{step};
                self.set_slider(kind,self.slider_value(kind)+delta);self.save_prefs();
            },
            Key::Named(NamedKey::Tab)=>{let n=self.settings_hits.len();self.settings_focus=Some(match self.settings_focus{Some(i) if self.mods.shift_key()=>(i+n-1)%n,Some(i)=>(i+1)%n,None=>0});},
            Key::Named(NamedKey::Enter)|Key::Named(NamedKey::Space) if self.settings_focus.is_some()=>{let i=self.settings_focus.take().unwrap();if let Some((r,h))=self.settings_hits.get(i).copied(){self.apply_setting(h,r.x+r.w*0.5);self.save_prefs();}},
            Key::Named(NamedKey::PageDown)|Key::Named(NamedKey::PageUp)=>{let down=matches!(ev.logical_key,Key::Named(NamedKey::PageDown));if let Some(Pane::Settings(p))=self.tabs.get_mut(self.active).map(|t|&mut t.left){p.scroll=(p.scroll+if down{p.rect.h*0.7}else{-p.rect.h*0.7}).clamp(0.0,(self.settings_reach-p.rect.h+self.scale*80.0).max(0.0));}self.settings_focus=None;},
            Key::Named(NamedKey::Escape) if self.settings_focus.is_some()=>self.settings_focus=None,
            _=>return false,
        }
        self.dirty=true;true
    }

    pub(crate) fn settings_click(&mut self, x: f32, y: f32) -> bool {
        let Some(tab) = self.tabs.get(self.active) else { return false };
        let Pane::Settings(s) = &tab.left else { return false };
        if !s.rect.contains(x, y) {
            return false;
        }
        let pad = self.touch_pad();
        self.settings_focus=None;
        let Some(&(_, hit)) = self.settings_hits.iter().find(|(r, _)| crate::touch::grown(*r, pad).contains(x, y)) else { return true };
        match hit {
            // A bundle's GET plays its own press.
            Hit::Play(_) | Hit::EventCue(..) | Hit::EventNext(_) | Hit::SoundOn(_) | Hit::Slider(..) | Hit::LspTool(_) => {}
            Hit::ReloadRules | Hit::OpenRules | Hit::ResetRules | Hit::MakeDefault | Hit::Unregister | Hit::Widevine | Hit::ReloadAvatar | Hit::PickAvatar | Hit::OpenProfileDir | Hit::SavePreset | Hit::OpenPresets | Hit::StopAdd | Hit::StopRemove => {
                self.play_event("control.press")
            }
            _ => self.play_event("toggle"),
        }
        if matches!(hit, Hit::Slider(..)) { self.settings_drag = Some(hit); }
        self.apply_setting(hit, x);
        self.save_prefs();
        self.dirty = true;
        true
    }

    /// A spoken label for a control (AccessKit).
    pub(crate) fn setting_is_action(hit: Hit) -> bool { catalog::is_action(hit) }

    pub(crate) fn setting_states(&self, section: usize) -> Vec<(Hit, bool)> {
        self.rows_for(section).into_iter().flat_map(|(_,control)| match control {
            Control::Pics(cards) => cards.into_iter().map(|(_,_,_,hit,on)|(hit,on)).collect(),
            Control::Art(cards) => cards.into_iter().map(|(_,_,_,hit,on,_)|(hit,on)).collect(),
            Control::Choice(cards) | Control::Strip(cards) => cards.into_iter().map(|(_,hit,on)|(hit,on)).collect(),
            _ => Vec::new(),
        }).collect()
    }

    pub(crate) fn startup_choice_selected(&self, hit: Hit) -> Option<bool> {
        let b = &self.behavior;
        Some(match hit {
            Hit::Lead(v) => b.lead == v,
            Hit::WindowStart(v) => b.window_start == v,
            Hit::Splash(v) => b.splash == v,
            Hit::Then(v) => b.then == v,
            Hit::HomeLook(v) => b.home_look == v,
            Hit::SwipeLook(v) => b.swipe_look == v,
            Hit::SwipeReach(v) => b.swipe_reach == v,
            Hit::HomeArt(i) => b.home_look == HomeLook::Art && crate::art::list().get(i).is_some_and(|a| a.key == b.home_art),
            Hit::NewWindow(v) => b.new_window == v,
            Hit::StartupLayout(i) => b.then == Then::Layout && crate::layout_file::saved().get(i).is_some_and(|(name, _)| *name == b.then_layout || (b.then_layout.is_empty() && i == 0)),
            _ => return None,
        })
    }

    /// PROMPT LSP's servers for the shells this profile has (both when
    /// none of them has one), each as a button that GETs or REMOVEs it.
    fn lsp_tool_buttons(&self) -> Vec<(String, (&'static str, &'static str), Hit)> {
        let kinds: Vec<_> = self.profiles.iter().map(|p| crate::shell::kind_of(&p.program)).collect();
        let wanted = |i: usize| match i {
            0 => kinds.iter().any(|k| matches!(k, crate::shell::Kind::Bash | crate::shell::Kind::Zsh)),
            _ => kinds.contains(&crate::shell::Kind::PowerShell),
        };
        let mut picked: Vec<usize> = (0..LSP_TOOLS.len()).filter(|&i| wanted(i)).collect();
        if picked.is_empty() {
            picked = (0..LSP_TOOLS.len()).collect();
        }
        picked
            .into_iter()
            .map(|i| {
                let (label, _, icon) = self.lsp_tool_state(i);
                (label, icon, Hit::LspTool(i))
            })
            .collect()
    }

    /// A prompt server's button words and the line under them.
    fn lsp_tool_words(&self, i: usize) -> (String, String) {
        let (label, line, _) = self.lsp_tool_state(i);
        (label, line)
    }

    fn lsp_tool_state(&self, i: usize) -> (String, String, (&'static str, &'static str)) {
        use crate::bundles::State;
        let Some(&(id, shells)) = LSP_TOOLS.get(i) else { return (String::new(), String::new(), icons::WARNING) };
        let Some(b) = crate::bundles::list().into_iter().find(|b| b.id == id) else {
            return (format!("{} · UNAVAILABLE", shells), format!("{id} is not in this build's tool list"), icons::WARNING);
        };
        match self.bundle_state(&b) {
            State::Absent => (format!("GET · {shells}"), format!("{} · about {} MB into profile/tools/{id}", b.name, b.size_mb), icons::DOWNLOAD),
            State::Fetching => (format!("FETCHING · {shells}"), format!("{} · installing, you can keep working", b.name), icons::DOWNLOAD),
            State::Installed => (format!("REMOVE · {shells}"), format!("{} · installed in this profile", b.name), icons::CHECK),
            State::External => (format!("READY · {shells}"), format!("{} · on this machine, managed outside nus", b.name), icons::CHECK),
            State::Failed(e) => (format!("TRY AGAIN · {shells}"), format!("{} · failed: {e}", b.name), icons::WARNING),
            State::Soon | State::NoPlatform => (format!("UNAVAILABLE · {shells}"), format!("{} · not offered for this platform yet", b.name), icons::WARNING),
        }
    }

    pub(crate) fn setting_label(&self, hit: Hit) -> String {
        match hit {
            Hit::Workspace(h) => workspace::label(h),
            Hit::Section(k) | Hit::Tile(k) => SECTIONS[k].0.to_lowercase(),
            Hit::Back => "back to settings".into(),
            Hit::Theme(None) => "theme follows the OS".into(),
            Hit::Theme(Some(true)) => "ink theme".into(),
            Hit::Theme(Some(false)) => "paper theme".into(),
            Hit::Signal(c) => format!("signal {}", surface::hex(c)),
            Hit::Base(None) => "no base".into(),
            Hit::Base(Some(c)) => format!("base {}", surface::hex(c)),
            Hit::Shell(s) => format!("carapace {}", s.name()),
            Hit::Slider(k, _, _) => format!("{:?}", k).to_lowercase(),
            Hit::Side(s) => format!("sidebar {:?}", s).to_lowercase(),
            Hit::SmallTabs(mode)=>format!("small sidebar {mode:?}"),
            Hit::Downloads => "open downloads".into(),
            Hit::DownloadRename(mode) => format!("download naming {mode:?}"),
            Hit::DownloadDir(0) => "choose the downloads folder".into(),
            Hit::DownloadDir(1) => "downloads to the system's folder".into(),
            Hit::DownloadDir(_) => "open the downloads folder".into(),
            Hit::DownloadAsk(on) => (if on { "ask where to save each download" } else { "save downloads without asking" }).into(),
            Hit::DownloadDone(what) => format!("when a download finishes · {}", what.name()),
            Hit::WheelPx(px) => format!("wheel speed {px} px"),
            Hit::Scrollbars(s) => format!("scrollbars {}", s.name()),
            Hit::PageZoom(pct) => format!("default zoom {pct}%"),
            Hit::PrivacySignal(on) => (if on { "send the privacy signal" } else { "no privacy signal" }).into(),
            Hit::Scrollback(n) => format!("scrollback {n} lines"),
            Hit::ClearBrowsing(0) => "clear cookies".into(),
            Hit::ClearBrowsing(_) => "clear the cache".into(),
            Hit::Compact(c) => (if c { "compact sidebar" } else { "full sidebar" }).into(),
            Hit::HoverFrom(h) => format!("reveal from {:?}", h).to_lowercase(),
            Hit::Fullscreen(f) => format!("fullscreen {:?}", f).to_lowercase(),
            Hit::Pin(p) => if p { "pin sidebar".into() } else { "sidebar on hover".into() },
            Hit::Links(l) => format!("links {:?}", l).to_lowercase(),
            Hit::PromptUrl(p) => format!("url at prompt {:?}", p).to_lowercase(),
            Hit::CloseAsks(a) => if a { "ask before closing a busy tab".into() } else { "never ask".into() },
            Hit::DefaultProfile(i) => format!("default shell {}", self.profiles.get(i).map(|p| p.name.as_str()).unwrap_or("")),
            Hit::ReloadRules => "reload rules".into(),
            Hit::OpenRules => "open rules in editor".into(),
            Hit::ResetRules => "reset rules to default".into(),
            Hit::Reduce(None) => "reduce motion follows the OS".into(),
            Hit::Reduce(Some(r)) => format!("reduce motion {}", if r { "on" } else { "off" }),
            Hit::Preset(k) => format!("theme {}", crate::themes::all().get(k).map(|p| p.name.clone()).unwrap_or_default()),
            Hit::SavePreset => "save the look as a theme".into(),
            Hit::OpenPresets => "open the presets folder".into(),
            Hit::StopSel(i) => format!("stop {}", i + 1),
            Hit::StopAdd => "add a stop".into(),
            Hit::StopRemove => "remove a stop".into(),
            Hit::StopColor(c) => format!("stop color {}", surface::hex(c)),
            Hit::TokPaper(c) => format!("paper {}", surface::hex(c)),
            Hit::TokInk(c) => format!("ink {}", surface::hex(c)),
            Hit::TokPage(c) => format!("page {}", surface::hex(c)),
            Hit::TokCaret(c) => c.map(|c| format!("caret {}", surface::hex(c))).unwrap_or_else(|| "caret follows the ink".into()),
            Hit::TokSelection(c) => c.map(|c| format!("selection {}", surface::hex(c))).unwrap_or_else(|| "selection follows the ink".into()),
            Hit::TokReset => "reset this mode's tokens".into(),
            Hit::AnsiSel(i) => format!("ansi {i}"),
            Hit::AnsiSet(c) => format!("set to {}", surface::hex(c)),
            Hit::Family(f) => format!("family {}", f.name()),
            Hit::Import(k) => format!("import {}", crate::theme_edit::imports().get(k).map(|t| t.name.clone()).unwrap_or_default()),
            Hit::OpenThemes => "open the themes folder".into(),
            Hit::Starter(k) => format!("start from {}", surface::STARTERS.get(k).map(|s| s.0).unwrap_or("")),
            Hit::LookTab(k) => LOOK_TABS.get(k).map(|t| t.to_lowercase()).unwrap_or_default(),
            Hit::TokSel(t) => format!("edit {:?}", t).to_lowercase(),
            Hit::TokSet(c) => format!("set to {}", surface::hex(c)),
            Hit::ShellInt(b) => if b { "shell integration auto".into() } else { "shell integration off".into() },
            Hit::Welcome => "open the welcome page".into(),
            Hit::PinDisplay(mode) => format!("pinned tiles {mode:?}"),
            Hit::Report(crate::support::Kind::Bug) => "Report a bug · opens a GitHub draft with version and OS".into(),
            Hit::AppIcon(choice)=>format!("Use {} app icon",choice.name()),
            Hit::Mercury => if crate::mercury::earned() { "Replay Mercury" } else { "Claim Mercury" }.into(),
            Hit::CopySupportDetails=>"Copy the support details shown below".into(),
            Hit::RecoverPrevious=>"Review recovery to the previous version".into(),
            Hit::UpdateCheck=>"Check GitHub Releases for an update".into(),
            Hit::UpdateInstall=>"Review update and restart warning".into(),
            Hit::UpdateConfirm=>"Download, verify, install and restart nus".into(),
            Hit::UpdateCancel=>"Cancel update".into(),
            Hit::UpdateChecks(on)=>format!("Automatic update checks {}",if on{"on"}else{"off"}),
            Hit::ProfileSeparate(own)=>if own{"this copy keeps its own profile".into()}else{"this copy shares the profile".into()},
            Hit::ProfileFolder=>"show profiles".into(),
            Hit::Report(crate::support::Kind::Feature) => "Request a feature · opens a GitHub draft with version and OS".into(),
            Hit::Block(b) => if b { "content blocking on".into() } else { "content blocking off".into() },
            Hit::StatusStyle(s) => format!("page status {:?}", s).to_lowercase(),
            Hit::SleepAfter(n) => if n == 0 { "never sleep tabs".into() } else { format!("sleep after {n} minutes") },
            Hit::ArchiveAfter(n) => if n == 0 { "never archive".into() } else { format!("archive after {n} hours") },
            Hit::Highlight(b) => if b { "highlight the command line".into() } else { "plain command line".into() },
            Hit::CopyOnSelect(b) => if b { "copy on select on".into() } else { "copy on select off".into() },
            Hit::PaneControls(c) => format!("pane controls {:?}", c).to_lowercase(),
            Hit::ScrollEasing(e) => format!("scroll {}", e.name()),
            Hit::WheelLines(n) => format!("{n} lines per wheel tick"),
            Hit::PageSmooth(b) => if b { "smooth page scrolling".into() } else { "instant page scrolling".into() },
            Hit::PaneDivider(b) => if b { "pane divider drags".into() } else { "pane divider fixed".into() },
            Hit::MiddlePaste(b) => if b { "middle click pastes".into() } else { "middle click does nothing".into() },
            Hit::Osc52(o) => format!("osc 52 {:?}", o).to_lowercase(),
            Hit::Predict(b) => if b { "predictions on".into() } else { "predictions off".into() },
            Hit::PromptLsp(m) => match m { PromptLsp::Quiet => "prompt lsp quiet".into(), PromptLsp::Menu => "prompt lsp menu".into(), PromptLsp::Off => "prompt lsp off".into() },
            Hit::FormatOnSave(b) => if b { "format on save".into() } else { "save as is".into() },
            Hit::Blocks(b) => if b { "block lamps on".into() } else { "block lamps off".into() },
            Hit::Journal(b) => if b { "remember commands".into() } else { "don't remember commands".into() },
            Hit::JournalKeep(n) => format!("keep command history {n} days"),
            Hit::CutOffMode(m) => match m { CutOff::Chip => "cut off: a chip".into(), CutOff::RunAgain => "cut off: run again".into(), CutOff::Off => "cut off: nothing".into() },
            Hit::PortsRemember(b) => if b { "ports remember".into() } else { "ports forget".into() },
            Hit::KeepAlive(k) => if k == KeepAlive::On { "shells are held".into() } else { "shells die with the app".into() },
            Hit::ClickToSource(b) => if b { "click to source on".into() } else { "click to source off".into() },
            Hit::Viewer(v) => v.label(),
            Hit::PipPolicy(e,b) => format!("picture in picture {} {}",e.label(),if b {"on"} else {"off"}),
            Hit::PipBand(b) => if b { "picture in picture band on".into() } else { "picture in picture band off".into() },
            Hit::PipProgress(b) => if b { "picture in picture progress rule on".into() } else { "picture in picture progress rule off".into() },
            Hit::Remember(b) => if b { "tabs and windows remembered".into() } else { "nothing remembered between launches".into() },
            Hit::SetLaunchTabs => "this window is the launch tabs".into(),
            Hit::ClearLaunchTabs => "launch tabs cleared".into(),
            Hit::LinkClick(l) => match l { LinkClick::Ask => "links ask before opening".into(), LinkClick::Open => "links open on click".into(), LinkClick::HintsOnly => "links open from hints mode only".into() },
            Hit::Replay(k) => match k { ReplayKeep::Days7 => "replay keeps a week".into(), ReplayKeep::Day1 => "replay keeps a day".into(), ReplayKeep::Off => "replay off".into() },
            Hit::Hands(h) => match h { HandsMode::Ask => "hands ask first".into(), HandsMode::Always => "hands never ask".into(), HandsMode::Never => "hands off".into() },
            Hit::HandsSubmit(b) => if b { "a submit always asks".into() } else { "a submit does not ask on allowed hosts".into() },
            Hit::HandsForget => "allowed hosts forgotten".into(),
            Hit::FoldOver(n) => if n == 0 { "never fold on its own".into() } else { format!("fold output over {n} lines") },
            Hit::ProgressSidebar(b) => if b { "progress in the sidebar".into() } else { "progress in the pane only".into() },
            Hit::Ledger(b) => if b { "assistants in tab rows".into() } else { "assistants as a dot".into() },
            Hit::ProgressTaskbar(b) => if b { "progress on the taskbar".into() } else { "taskbar left alone".into() },
            Hit::AskCtx(c) => format!("ask context · {}", c.key()),
            Hit::ForgetMemory => "memory cleared".into(),
            Hit::SshIntegration(b) => if b { "ssh brings the integration".into() } else { "ssh as is".into() },
            Hit::TidyEvery(e) => format!("tidy {:?}", e).to_lowercase(),
            Hit::Dedupe(b) => if b { "dedupe bands on".into() } else { "dedupe bands off".into() },
            Hit::ShellColours(c) => format!("shell colors: {:?}", c).to_lowercase(),
            Hit::ShellTint(t) => format!("new shell colors: {:?}", t).to_lowercase(),
            Hit::Grade(g) => match g { Grade::Off => "program colors as they come".into(), g => format!("program colors graded to {}:1", g.ratio()) },
            Hit::Truecolour(t) => match t { Truecolour::AsSent => "truecolor as sent".into(), Truecolour::Snapped => "truecolor wears the theme".into() },
            Hit::SyncSession(b) => if b { "the session syncs".into() } else { "the session stays here".into() },
            Hit::SyncEvery(n) => if n == 0 { "sync on demand".into() } else { format!("sync every {n} min") },
            Hit::SyncAtQuit(b) => if b { "sync at quit".into() } else { "no sync at quit".into() },
            Hit::SyncForget => "key forgotten".into(),
            Hit::SyncNow => "syncing".into(),
            Hit::SyncKey => "key copied".into(),
            Hit::SyncEdit(_) => "sync".into(),
            Hit::MeWalk(0) => "how the profile lives".into(),
            Hit::MeWalk(2) => "import from another application".into(),
            Hit::MeWalk(_) => "a forge".into(),
            Hit::ForgeForget => "forget the forge".into(),
            Hit::PortsGrouping(g) => g.name().into(),
            Hit::PortsOpen(o) => format!("open in {}", match o { PortsOpen::Tab => "a tab", PortsOpen::Split => "the split", PortsOpen::Peek => "a peek" }),
            Hit::PortsPoll(n) => format!("poll every {n}s"),
            Hit::PortsToast(b) => if b { "new-port toast on".into() } else { "new-port toast off".into() },
            Hit::PortsShow(k, b) => format!("{} {}", ["system", "udp", "connections", "docker"][(k as usize).min(3)], if b { "shown" } else { "hidden" }),
            Hit::PortsKill(k) => format!("ask before kill: {:?}", k).to_lowercase(),
            Hit::PortsProbe(b) => if b { "probe on".into() } else { "probe off".into() },
            Hit::PortsTunnel(t) => format!("tunnel: {:?}", t).to_lowercase(),
            Hit::PortsHidden => "hidden processes reset".into(),
            Hit::HatchLook(l) => format!("the {:?}", l).to_lowercase(),
            Hit::MenuEnabled(b)=>if b{"menu bar / tray icon on"}else{"menu bar / tray icon off"}.into(),
            Hit::MenuSignal(style)=>format!("signal icon {style:?}"),
            Hit::MenuDensity(module,density)=>format!("{} section {density:?}",module.label()),
            Hit::MenuMove(module,down)=>format!("move {} {}",module.label(),if down{"down"}else{"up"}),
            Hit::MenuNames(b)=>if b{"show task and file names"}else{"hide task and file names"}.into(),
            Hit::MenuRecent(b)=>if b{"include finished work and downloads"}else{"only active work and downloads"}.into(),
            Hit::MenuPreview=>"open menu drawer".into(),
            Hit::HatchHotkey(c) => c.label().to_lowercase(),
            Hit::HatchRecord => "record a hatch hotkey".into(),
            Hit::HatchSize(n) => format!("{n}% tall"),
            Hit::HatchMonitor(m) => format!("on the {:?} monitor", m).to_lowercase(),
            Hit::HatchStatus(b) => if b { "compact work status on".into() } else { "compact work status off".into() },
            Hit::HatchBackground(b) => if b { "keep nus available in the background".into() } else { "closing windows ends their sessions".into() },
            Hit::HatchNotify(b) => if b { "brief completion notices on".into() } else { "completion notices off".into() },
            Hit::HatchDim(b) => if b { "dim behind the modal".into() } else { "leave the desktop visible".into() },
            Hit::HatchAutohide(b) => if b { "hides when you look away".into() } else { "stays up".into() },
            Hit::HatchSpaces(s) => match s { HatchSpaces::Follow => "one hatch per space".into(), HatchSpaces::One => "one hatch for all".into() },
            Hit::HdrStyle(s) => format!("header {:?}", s).to_lowercase(),
            Hit::HdrMasthead(b) => if b { "masthead title".into() } else { "caps title".into() },
            Hit::HdrDateline(b) => if b { "dateline on".into() } else { "dateline off".into() },
            Hit::HdrButton(b) => if b { "new tab in the header".into() } else { "no new tab in the header".into() },
            Hit::HdrNextRow(b) => if b { "next row is new tab".into() } else { "next row off".into() },
            Hit::HdrName(b) => if b { "window name shown".into() } else { "window square only".into() },
            Hit::HdrCaret(b) => if b { "kinds caret on".into() } else { "kinds caret off".into() },
            Hit::HdrRailHover(b) => if b { "rail on hover".into() } else { "rail always".into() },
            Hit::HdrFlash(b) => if b { "press flash".into() } else { "no press flash".into() },
            Hit::CurShape(s) => format!("cursor {:?}", s).to_lowercase(),
            Hit::CurBlink(b) => format!("blink {:?}", b).to_lowercase(),
            Hit::CurColor(c) => format!("cursor color {:?}", c).to_lowercase(),
            Hit::CurMotion(m) => format!("cursor motion {:?}", m).to_lowercase(),
            Hit::CurHollow(h) => if h { "hollow when unfocused".into() } else { "hidden when unfocused".into() },
            Hit::CurHide(h) => if h { "hide the pointer while typing".into() } else { "keep the pointer while typing".into() },
            Hit::WindowStart(w) => format!("window {:?}", w).to_lowercase(),
            Hit::Splash(m) => format!("splash {:?}", m).to_lowercase(),
            Hit::HomeLook(l) => format!("home {:?}", l).to_lowercase(),
            Hit::OpenedBy(o) => format!("opened by others {:?}", o).to_lowercase(),
            Hit::SwipeLook(l) => format!("swipe overlay {:?}", l).to_lowercase(),
            Hit::SwipeReach(r) => format!("swipe distance {r} px"),
            Hit::NewWindow(w) => format!("a new window {:?}", w).to_lowercase(),
            Hit::AskBackend(i) => format!("ask with {}", crate::ask::backends().get(i).map(|b| b.name.clone()).unwrap_or_default()),
            Hit::Phone(on) => if on { "serve this window to the phone".into() } else { "stop serving the phone".into() },
            Hit::CopyPhoneUrl => "copy the phone's address".into(),
            Hit::HomeArt(i) => format!("art · {}", crate::art::list().get(i).map(|a| a.name.clone()).unwrap_or_default()),
            Hit::AddArt => "a new art of your own".into(),
            Hit::AskArt => "asking for an art".into(),
            Hit::OpenArtFolder => "the art folder".into(),
            Hit::PlaceEdit => "location".into(),
            Hit::FooterTheme(i) => format!("footer theme {}", crate::themes::all().get(i).map(|t|t.name.as_str()).unwrap_or("")),
            Hit::FooterDefaults => "reset footer themes".into(),
            Hit::Search => "Search settings".into(),
            Hit::UiFont(f)|Hit::TermFont(f) => f.name().into(),
            Hit::UiWeight(w)|Hit::TermWeight(w) => w.name().into(),
            Hit::Then(t) => format!("start page: {}", match t {
                Then::Palette => "command palette", Then::Prompt => "home prompt", Then::HomePage => "home page", Then::Layout => "custom layout",
                Then::LastPage => "the last page", Then::Restore => "saved session", Then::Shell => "shell",
            }),
            Hit::StartupLayout(i) => format!("startup layout {}", crate::layout_file::saved().get(i).map(|(n, _)| n.as_str()).unwrap_or("missing")),
            Hit::EditHomeUrl => "edit home page address".into(),
            Hit::Atlas(a) => format!("atlas {:?}", a).to_lowercase(),
            Hit::Outside(o) => format!("links from outside {:?}", o).to_lowercase(),
            Hit::Lead(l) => match l { Lead::Terminal => "terminal first".into(), Lead::Browser => "browser first".into() },
            Hit::MeEdit(k) => match k { 0 => "your name".into(), 1 => "your face".into(), _ => "this device's name".into() },
            Hit::MeCard => "the profile card".into(),
            Hit::MeFolder => "open the profile folder".into(),
            Hit::MeForget => "start the profile over".into(),
            Hit::LoginItem(on) => if on { "start with the system".into() } else { "do not start with the system".into() },
            Hit::SoundOn(b) => if b { "sound on".into() } else { "sound off".into() },
            Hit::Play(i) => format!("play {}", crate::sound::NAMES.get(i).copied().unwrap_or("")),
            Hit::EventCue(e, c) => format!("{} → {}", crate::sound::EVENTS[e].0, if c == usize::MAX { "quiet" } else { crate::sound::NAMES[c] }),
            Hit::EventNext(e) => format!("{}: next cue", crate::sound::EVENTS[e].0),
            Hit::OpacityOn(o) => format!("opacity on {:?}", o).to_lowercase(),
            Hit::TexKind(k) => format!("texture {}", k.name()),
            Hit::TexOn(o) => format!("texture on {:?}", o).to_lowercase(),
            Hit::TexMotion(b) => if b { "texture animated".into() } else { "texture still".into() },
            Hit::ReloadAvatar => "reload avatar".into(),
            Hit::PickAvatar => "choose a picture for your profile".into(),
            Hit::OpenProfileDir => "open the profile folder".into(),
            Hit::StartOnLaunch(b) => if b { "atlas also at launch".into() } else { "atlas from the planet".into() },
            Hit::StartupSound(b) => if b { "startup sound on".into() } else { "startup sound off".into() },
            Hit::MakeDefault => "make nus the default browser".into(),
            Hit::Unregister => "unregister nus as a browser".into(),
            Hit::Widevine => "fetch the Widevine module now".into(),
            Hit::LspTool(i) => self.lsp_tool_words(i).1,
            Hit::BarStyle(b) => format!("loading bar {}", b.name()),
            Hit::BarColor(c) => format!("bar color {:?}", c).to_lowercase(),
        }
    }

    pub(crate) fn apply_setting(&mut self, hit: Hit, x: f32) {
        match hit {
            Hit::Workspace(h) => self.apply_workspace_setting(h),
            Hit::Section(k) => {
                if k == SEC_ASSISTANTS { self.assistants.refresh(self.behavior.assistants.clone()); }
                if let Some(Pane::Settings(s)) = self.tabs.get_mut(self.active).map(|t| &mut t.left) {
                    s.section = k;
                    s.scroll = 0.0;
                }
                if k == SEC_BROWSER {
                    self.refresh_register_note();
                }
            }
            Hit::Theme(None) => {
                self.behavior.follow_os_theme = true;
                if let Some(theme) = self.window.theme() {
                    self.set_mode(if theme == winit::window::Theme::Dark { nus_render::Mode::Ink } else { nus_render::Mode::Paper });
                }
            },
            Hit::Theme(Some(ink)) => {
                self.behavior.follow_os_theme = false;
                self.set_mode(if ink { nus_render::Mode::Ink } else { nus_render::Mode::Paper });
                self.refresh_icon();
            }
            Hit::Signal(c) => {
                self.surface.signal = c;
                if self.theme_edit.family == crate::theme_edit::Family::FromSignal {
                    self.rebuild_theme();
                }
                self.refresh_icon();
            }
            Hit::Base(b) => {
                self.surface.base = b;
                if b.is_some() && self.surface.tint == 0.0 {
                    self.surface.tint = 0.35;
                }
                self.refresh_icon();
            }
            Hit::Shell(sh) => {
                self.surface.shell = sh;
                self.layout();
            }
            Hit::Slider(kind, x0, w) => self.set_slider(kind, (x - x0) / w),
            Hit::Side(side) => {
                self.sidebar_rules.side = side;
                self.sidebar_hover = false;
                self.layout();
            }
            Hit::HoverFrom(h) => self.sidebar_rules.hover_from = h,
            Hit::SmallTabs(mode)=>{self.sidebar_rules.small_tabs=mode;self.layout();},
            Hit::Downloads => self.open_downloads(),
            Hit::DownloadRename(mode) => { self.behavior.download_rename=mode;crate::downloads::set_rename(mode); },
            Hit::DownloadDir(0) => self.pick_download_dir(),
            Hit::DownloadDir(1) => { self.behavior.download_dir.clear(); let b = self.behavior.clone(); self.apply_behavior_statics(&b); self.notice(nus_render::text::icons::FOLDER, "Downloads Folder Reset", "back to the system's folder"); }
            Hit::DownloadDir(_) => self.download_action(crate::downloads::Hit::Folder),
            Hit::DownloadAsk(on) => { self.behavior.download_ask = on; let b = self.behavior.clone(); self.apply_behavior_statics(&b); }
            Hit::DownloadDone(what) => self.behavior.download_done = what,
            Hit::WheelPx(px) => self.behavior.wheel_px = px,
            Hit::Scrollbars(s) => {
                self.behavior.scrollbars = s;
                // Hidden applies to open pages now; overlay and classic
                // are Chromium features, read when it starts.
                crate::browser::SCROLLBARS.store(s as u8, std::sync::atomic::Ordering::Relaxed);
                for tab in &self.tabs {
                    for pane in std::iter::once(&tab.left).chain(tab.right.as_ref()) {
                        if let Pane::Web(w) = pane {
                            w.tab.hide_scrollbars(s == Scrollbars::Hidden);
                        }
                    }
                }
            }
            Hit::PageZoom(pct) => { self.behavior.page_zoom = pct; let b = self.behavior.clone(); self.apply_behavior_statics(&b); self.rezoom_pages(); }
            Hit::PrivacySignal(on) => { self.behavior.privacy_signal = on; let b = self.behavior.clone(); self.apply_behavior_statics(&b); }
            Hit::Scrollback(n) => self.behavior.scrollback = n,
            Hit::ClearBrowsing(what) => self.clear_browsing(what),
            Hit::Compact(c) => {
                if self.sidebar_rules.compact != c {
                    self.toggle_compact();
                }
            }
            Hit::Fullscreen(f) => {
                self.sidebar_rules.fullscreen = f;
                self.layout();
            }
            Hit::Pin(p) => {
                self.sidebar = p;
                self.sidebar_hover = false;
                self.layout();
            }
            Hit::Links(l) => self.behavior.links = l,
            Hit::PromptUrl(p) => self.behavior.prompt_url = p,
            Hit::CloseAsks(a) => self.behavior.close_asks = a,
            Hit::DefaultProfile(i) => self.behavior.default_profile = i,
            Hit::ReloadRules => {
                self.rules.reload();
                self.refresh_rules_folders();
            }
            Hit::OpenRules => {
                let path = self.rules.path.to_string_lossy().to_string();
                let cmd = if cfg!(target_os = "windows") {
                    format!("start \"\" \"{path}\"")
                } else if cfg!(target_os = "macos") {
                    format!("open \"{path}\"")
                } else {
                    format!("xdg-open \"{path}\"")
                };
                self.run_in_shell(&cmd);
            }
            Hit::ResetRules => {
                let _ = std::fs::write(&self.rules.path, surface::DEFAULT_RULES);
                self.rules.reload();
                self.refresh_rules_folders();
            }
            Hit::Tile(k) => {
                if let Some(Pane::Settings(s)) = self.tabs.get_mut(self.active).map(|t| &mut t.left) {
                    s.section = k;
                    s.scroll = 0.0;
                    s.drill = true;
                }
                if k == SEC_BROWSER {
                    self.refresh_register_note();
                }
            }
            Hit::Back => {
                if let Some(Pane::Settings(s)) = self.tabs.get_mut(self.active).map(|t| &mut t.left) {
                    s.drill = false;
                    s.scroll = 0.0;
                }
            }
            Hit::LspTool(i) => {
                if let Some((id, _)) = LSP_TOOLS.get(i) {
                    self.bundle_toggle(id);
                }
            }
            Hit::Widevine => {
                crate::browser_runtime::ensure();
                crate::widevine::fetch();
                self.notice(icons::DOWNLOAD, "Widevine", "asked Chromium for the protected-content module");
            }
            Hit::MakeDefault => match crate::little::register() {
                Ok(()) => self.register_note = "registered · pick nus in Windows Settings".into(),
                Err(e) => {
                    tracing::warn!("register: {e}");
                    self.register_note = e;
                }
            },
            Hit::Unregister => {
                let _ = crate::little::unregister();
                self.register_note = "unregistered".into();
            }
            Hit::Preset(k) => {
                if let Some(t) = crate::themes::all().get(k).cloned() {
                    self.apply_theme(&t);
                }
            }
            Hit::SavePreset => {
                let n = crate::themes::all().len() + 1;
                let name = format!("mine-{n}");
                let t = self.current_theme(&name);
                if crate::themes::save(&t).is_ok() {
                    self.preset_name = name;
                }
            }
            Hit::OpenPresets => {
                let dir = std::env::current_dir().unwrap_or_default().join("profile").join("surfaces");
                let _ = std::fs::create_dir_all(&dir);
                let cmd = if cfg!(target_os = "windows") {
                    format!("start \"\" \"{}\"", dir.display())
                } else if cfg!(target_os = "macos") {
                    format!("open \"{}\"", dir.display())
                } else {
                    format!("xdg-open \"{}\"", dir.display())
                };
                self.run_in_shell(&cmd);
            }
            Hit::StopSel(i) => {
                self.stop_sel = i;
                self.tok_sel = TokSel::Stop(i);
            }
            Hit::StopAdd => {
                if self.surface.stops.len() < 4 {
                    let ink = self.theme.ink;
                    let mut stops = self.surface.ramp(ink);
                    let last = *stops.last().unwrap();
                    stops.push(surface::rotate_hue(last, 0.12));
                    self.surface.stops = stops;
                    self.stop_sel = self.surface.stops.len() - 1;
                }
            }
            Hit::StopRemove => {
                if self.surface.stops.len() > 2 {
                    self.surface.stops.pop();
                    self.stop_sel = self.stop_sel.min(self.surface.stops.len() - 1);
                } else {
                    self.surface.stops.clear();
                    self.stop_sel = 0;
                }
            }
            Hit::StopColor(c) => {
                let ink = self.theme.ink;
                if self.surface.stops.len() < 2 {
                    self.surface.stops = self.surface.ramp(ink);
                }
                let i = self.stop_sel.min(self.surface.stops.len() - 1);
                self.surface.stops[i] = c;
            }
            Hit::TokPaper(c) => {
                let mode = self.theme.mode;
                self.theme_edit.edit_mut(mode).paper = Some(c);
                self.rebuild_theme();
            }
            Hit::TokInk(c) => {
                let mode = self.theme.mode;
                self.theme_edit.edit_mut(mode).ink = Some(c);
                self.rebuild_theme();
            }
            Hit::TokPage(c) => {
                let mode = self.theme.mode;
                self.theme_edit.edit_mut(mode).page = Some(c);
                self.rebuild_theme();
            }
            Hit::TokCaret(c) => {
                let mode = self.theme.mode;
                self.theme_edit.edit_mut(mode).caret = c;
                self.rebuild_theme();
            }
            Hit::TokSelection(c) => {
                let mode = self.theme.mode;
                self.theme_edit.edit_mut(mode).selection = c;
                self.rebuild_theme();
            }
            Hit::TokReset => {
                let mode = self.theme.mode;
                *self.theme_edit.edit_mut(mode) = Default::default();
                self.theme_edit.family = crate::theme_edit::Family::Broadsheet;
                self.theme_edit.saturation = 1.0;
                self.rebuild_theme();
            }
            Hit::AnsiSel(i) => self.ansi_sel = i.min(15),
            Hit::AnsiSet(c) => {
                let mode = self.theme.mode;
                let current: [Color; 16] = std::array::from_fn(|i| crate::theme_edit::from_rgb(self.theme.ansi[i]));
                let e = self.theme_edit.edit_mut(mode);
                let mut a = e.ansi.unwrap_or(current);
                a[self.ansi_sel.min(15)] = c;
                e.ansi = Some(a);
                self.theme_edit.family = crate::theme_edit::Family::Imported;
                self.rebuild_theme();
            }
            Hit::Family(f) => {
                self.theme_edit.family = f;
                if f == crate::theme_edit::Family::Broadsheet {
                    let mode = self.theme.mode;
                    self.theme_edit.edit_mut(mode).ansi = None;
                }
                self.rebuild_theme();
            }
            Hit::Import(k) => {
                if let Some(t) = crate::theme_edit::imports().get(k) {
                    let mode = self.theme.mode;
                    let e = self.theme_edit.edit_mut(mode);
                    e.ansi = t.ansi;
                    if let Some(p) = t.paper {
                        e.paper = Some(p);
                    }
                    if let Some(i) = t.ink {
                        e.ink = Some(i);
                    }
                    self.theme_edit.family = crate::theme_edit::Family::Imported;
                    self.rebuild_theme();
                }
            }
            Hit::Starter(k) => self.rules.write_starter(k),
            Hit::LookTab(k) => {
                self.look_tab = k;
                if let Some(Pane::Settings(s)) = self.tabs.get_mut(self.active).map(|t| &mut t.left) {
                    s.scroll = 0.0;
                }
            }
            Hit::TokSel(t) => {
                self.tok_sel = t;
                match t {
                    TokSel::Stop(i) => self.stop_sel = i,
                    TokSel::Ansi(i) => self.ansi_sel = i,
                    _ => {}
                }
            }
            Hit::TokSet(c) => self.set_tok(c),
            Hit::ShellInt(b) => self.behavior.shell_integration = b,
            Hit::Welcome => self.open_welcome(),
            Hit::PinDisplay(mode) => self.sidebar_rules.pin_display = mode,
            Hit::Report(kind) => self.run(crate::app::Action::Report(kind)),
            Hit::AppIcon(choice)=>{if choice==crate::app_icon::Choice::Mercury && !crate::mercury::earned(){self.claim_mercury();}
                if choice!=crate::app_icon::Choice::Mercury || crate::mercury::earned(){self.behavior.app_icon=choice;crate::app_icon::select(choice);self.refresh_icon();self.toast(nus_render::text::icons::APP_WINDOW,"App Icon Changed",choice.name(),None);}},
            Hit::Mercury => self.claim_mercury(),
            Hit::CopySupportDetails => {
                match arboard::Clipboard::new().and_then(|mut cb| cb.set_text(crate::support::details())) {
                    Ok(()) => self.notice(nus_render::text::icons::COPY, "Copied", "support details · review them before sharing"),
                    Err(_) => self.notice_problem("Could Not Use Clipboard", ""),
                }
            }
            Hit::RecoverPrevious => self.recover_previous_version(),
            Hit::UpdateCheck=>crate::updates::check(),
            Hit::UpdateInstall=>crate::updates::confirm(true),
            Hit::UpdateCancel=>crate::updates::confirm(false),
            Hit::UpdateChecks(on)=>self.behavior.update_checks=on,
            Hit::ProfileSeparate(own)=>{
                match crate::install::set_separate(own) {
                    Ok(()) => self.notice(icons::RELOAD, "Restart To Switch", if own { "this copy keeps a profile of its own from its next launch" } else { "this copy shares the channel's profile from its next launch" }),
                    Err(e) => self.notice_problem("Could Not Change Profile", e.to_string()),
                }
            },
            Hit::ProfileFolder=>if let Some(p)=crate::install::placement(){crate::downloads::reveal(&p.channel_root,false);},
            Hit::UpdateConfirm=>self.install_update(),
            Hit::Block(b) => {
                self.behavior.block_content = b;
                crate::browser::BLOCKING.store(b, std::sync::atomic::Ordering::Relaxed);
            }
            Hit::StatusStyle(s) => self.behavior.status = s,
            Hit::SleepAfter(n) => self.behavior.sleep_after_min = n,
            Hit::ArchiveAfter(n) => self.behavior.archive_after_h = n,
            Hit::Highlight(b) => self.behavior.highlight = b,
            Hit::CopyOnSelect(b) => self.behavior.copy_on_select = b,
            Hit::PaneControls(c) => self.behavior.pane_controls = c,
            Hit::ScrollEasing(e) => self.behavior.scroll_easing = e,
            Hit::WheelLines(n) => self.behavior.wheel_lines = n,
            Hit::PageSmooth(b) => {
                self.behavior.page_smooth_scroll = b;
                self.notice(nus_render::text::icons::RELOAD, "Restart To Apply", "page scrolling changes after you restart nus");
            }
            Hit::PaneDivider(b) => self.behavior.pane_divider = b,
            Hit::MiddlePaste(b) => self.behavior.middle_paste = b,
            Hit::Osc52(o) => self.behavior.osc52 = o,
            Hit::Predict(b) => self.behavior.predict = b,
            Hit::PromptLsp(m) => self.behavior.prompt_lsp = m,
            Hit::FormatOnSave(b) => self.behavior.format_on_save = b,
            Hit::Blocks(b) => self.behavior.blocks = b,
            Hit::Journal(b) => self.behavior.journal = b,
            Hit::JournalKeep(n) => self.behavior.journal_keep = n,
            Hit::CutOffMode(m) => self.behavior.cutoff = m,
            Hit::PortsRemember(b) => self.behavior.ports_remember = b,
            Hit::KeepAlive(k) => self.behavior.keep_alive = k,
            Hit::Hands(h) => self.behavior.hands = h,
            Hit::Replay(k) => {
                if self.behavior.replay != k {
                    self.recorder = k.days().and_then(crate::replay::Recorder::new);
                    self.behavior.replay = if k.days().is_some() && self.recorder.is_none() { ReplayKeep::Off } else { k };
                    if k.days().is_some() && self.recorder.is_none() { self.notice_problem("Could Not Start Recording", "check that the profile folder is writable"); }
                }
            },
            Hit::ClickToSource(b) => self.behavior.click_to_source = b,
            Hit::PipBand(b) => self.behavior.pip_band = b,
            Hit::PipPolicy(e,b) => self.behavior.pip_policy.set(e,b),
            Hit::Viewer(v) => {self.behavior.viewers.set(v);self.sync_viewer_preferences(true);},
            Hit::PipProgress(b) => self.behavior.pip_progress = b,
            Hit::LinkClick(l) => self.behavior.link_click = l,
            Hit::Remember(b) => {
                self.behavior.remember = b;
                if !b {
                    let _ = std::fs::remove_file(std::env::current_dir().unwrap_or_default().join("profile").join("session.json"));
                    self.last_session = None;
                }
            }
            Hit::SetLaunchTabs => {
                self.save_layout("launch");
                self.behavior.then = Then::Layout;
                self.behavior.then_layout = "launch".into();
            }
            Hit::ClearLaunchTabs => {
                let _ = std::fs::remove_file(std::env::current_dir().unwrap_or_default().join("profile").join("layouts").join("launch.nus.luau"));
                if self.behavior.then == Then::Layout && self.behavior.then_layout == "launch" {
                    self.behavior.then = Then::Prompt;
                    self.behavior.then_layout.clear();
                }
            }
            Hit::HandsSubmit(b) => self.behavior.hands_confirm_submit = b,
            Hit::HandsForget => self.behavior.hands_hosts.clear(),
            Hit::FoldOver(n) => self.behavior.fold_over = n,
            Hit::ProgressSidebar(b) => self.behavior.progress_sidebar = b,
            Hit::Ledger(b) => self.behavior.ledger = b,
            Hit::ProgressTaskbar(b) => self.behavior.progress_taskbar = b,
            Hit::AskCtx(c) => {
                let k = c.key().to_string();
                if let Some(i) = self.behavior.ask_ctx.iter().position(|x| *x == k) {
                    self.behavior.ask_ctx.remove(i);
                } else {
                    self.behavior.ask_ctx.push(k);
                }
            }
            Hit::SshIntegration(b) => self.behavior.ssh_integration = b,
            Hit::TidyEvery(e) => self.behavior.tidy_every = e,
            Hit::Dedupe(b) => self.behavior.dedupe = b,
            Hit::ShellColours(c) => self.behavior.shell_colours = c,
            Hit::ShellTint(t) => {
                self.behavior.shell_tint = t;
                self.recolor_shells();
            }
            Hit::Grade(g) => self.behavior.grade = g,
            Hit::Truecolour(t) => self.behavior.truecolour = t,
            Hit::MeEdit(k) => self.open_me_card_at(match k { 0 => crate::me::Step::Name, 1 => crate::me::Step::Face, _ => crate::me::Step::Device }),
            Hit::MeCard => self.open_me_card(),
            Hit::MeFolder => {
                let dir = std::env::current_dir().unwrap_or_default().join("profile");
                let cmd = if cfg!(target_os = "windows") {
                    format!("start \"\" \"{}\"", dir.display())
                } else if cfg!(target_os = "macos") {
                    format!("open \"{}\"", dir.display())
                } else {
                    format!("xdg-open \"{}\"", dir.display())
                };
                self.run_in_shell(&cmd);
            }
            Hit::MeForget => {
                crate::me::Me::forget();
                self.me = None;
                self.user_name = self.me_name();
                self.open_me_card();
            }
            Hit::SyncSession(b) => self.behavior.sync_session = b,
            Hit::SyncEvery(n) => self.behavior.sync_every_min = n,
            Hit::SyncAtQuit(b) => self.behavior.sync_at_quit = b,
            Hit::SyncForget => crate::syncui::forget_key(),
            Hit::SyncNow => self.sync_now(),
            Hit::SyncKey => self.run(crate::app::Action::SyncKey),
            Hit::SyncEdit(k) => self.open_palette(match k { 0 => crate::app::PaletteMode::SyncFolder, 1 => crate::app::PaletteMode::SyncGit, _ => crate::app::PaletteMode::SyncJoin }),
            Hit::MeWalk(k) => self.open_me_card_at(match k {0=>crate::me::Step::Sync,2=>crate::me::Step::Import,_=>crate::me::Step::Forge}),
            Hit::ForgeForget => {
                crate::forge::forget();
                self.behavior.sync_git.clear();
                self.notice(nus_render::text::icons::GITHUB, "Forge Forgotten", "the repo is still yours to delete");
            }
            Hit::ForgetMemory => {
                let _ = crate::protected_state::write(&std::env::current_dir().unwrap_or_default().join("profile").join("memory.md"), b"");
            }
            Hit::PortsGrouping(g) => self.behavior.ports_grouping = g,
            Hit::PortsOpen(o) => self.behavior.ports_open = o,
            Hit::PortsPoll(n) => self.behavior.ports_poll = n,
            Hit::PortsToast(b) => self.behavior.ports_toast = b,
            Hit::PortsShow(k, b) => match k {
                0 => self.behavior.ports_show_system = b,
                1 => self.behavior.ports_show_udp = b,
                2 => self.behavior.ports_show_connections = b,
                _ => self.behavior.ports_show_docker = b,
            },
            Hit::PortsKill(k) => self.behavior.ports_kill_confirm = k,
            Hit::PortsProbe(b) => self.behavior.ports_probe = b,
            Hit::PortsTunnel(t) => self.behavior.ports_tunnel = t,
            Hit::PortsHidden => self.behavior.ports_hidden = default_hidden_processes(),
            Hit::HatchLook(l) => {
                self.behavior.hatch_look = l;
                self.hatch_settings_changed();
            }
            Hit::MenuEnabled(b)=>self.behavior.menu_drawer.enabled=b,
            Hit::MenuSignal(style)=>self.behavior.menu_drawer.signal=style,
            Hit::MenuDensity(module,density)=>self.behavior.menu_drawer.set_density(module,density),
            Hit::MenuMove(module,down)=>self.behavior.menu_drawer.move_section(module,down),
            Hit::MenuNames(b)=>self.behavior.menu_drawer.names=b,
            Hit::MenuRecent(b)=>self.behavior.menu_drawer.recent=b,
            Hit::MenuPreview=>self.toggle_menu_drawer(None),
            Hit::HatchHotkey(c) => {
                self.hotkey_recording = false;
                self.behavior.hatch_hotkey = c;
                self.hatch_settings_changed();
            }
            Hit::HatchRecord => {
                self.hotkey_recording = !self.hotkey_recording;
            }
            Hit::HatchSize(n) => {
                self.behavior.hatch_size = n;
                self.hatch_settings_changed();
            }
            Hit::HatchMonitor(m) => { self.behavior.hatch_monitor = m; self.hatch_settings_changed(); },
            Hit::HatchAutohide(b) => self.behavior.hatch_autohide = b,
            Hit::HatchStatus(b) => self.behavior.hatch_status = b,
            Hit::HatchNotify(b) => { self.behavior.hatch_notify = b; if !b { self.hatch_state.completion = None; } },
            Hit::HatchBackground(b) => self.behavior.hatch_background = b,
            Hit::HatchDim(b) => { self.behavior.hatch_dim = b; self.hatch_settings_changed(); },
            Hit::HatchSpaces(s) => self.behavior.hatch_spaces = s,
            Hit::HdrStyle(s) => {
                self.header.style = s;
            }
            Hit::HdrMasthead(b) => self.header.masthead = b,
            Hit::HdrDateline(b) => self.header.dateline = b,
            Hit::HdrButton(b) => self.header.header_button = b,
            Hit::HdrNextRow(b) => self.header.next_row = b,
            Hit::HdrName(b) => self.header.show_name = b,
            Hit::HdrCaret(b) => self.header.kinds_caret = b,
            Hit::HdrRailHover(b) => self.header.rail_hover = b,
            Hit::HdrFlash(b) => self.header.flash = b,
            Hit::CurShape(s) => self.cursor.shape = s,
            Hit::CurBlink(b) => self.cursor.blink = b,
            Hit::CurColor(c) => self.cursor.color = c,
            Hit::CurMotion(m) => self.cursor.motion = m,
            Hit::CurHollow(h) => self.cursor.hollow_unfocused = h,
            Hit::CurHide(h) => self.cursor.hide_while_typing = h,
            Hit::OpenThemes => {
                let dir = crate::theme_edit::themes_dir();
                let _ = std::fs::create_dir_all(&dir);
                let cmd = if cfg!(target_os = "windows") {
                    format!("start \"\" \"{}\"", dir.display())
                } else if cfg!(target_os = "macos") {
                    format!("open \"{}\"", dir.display())
                } else {
                    format!("xdg-open \"{}\"", dir.display())
                };
                self.run_in_shell(&cmd);
            }
            Hit::WindowStart(w) => self.behavior.window_start = w,
            Hit::Splash(m) => self.behavior.splash = m,
            Hit::HomeLook(l) => self.behavior.home_look = l,
            Hit::OpenedBy(o) => self.behavior.opened_by_others = o,
            Hit::SwipeLook(l) => self.behavior.swipe_look = l,
            Hit::SwipeReach(r) => self.behavior.swipe_reach = r,
            Hit::NewWindow(w) => self.behavior.new_window = w,
            Hit::AskBackend(i) => {
                if let Some(b) = crate::ask::backends().get(i) {
                    self.behavior.ask_backend = b.name.clone();
                }
            }
            Hit::Phone(on) => {
                self.behavior.phone = on;
                if on {
                    self.phone_on();
                    self.behavior.phone = crate::phone::current().is_some();
                } else {
                    crate::phone::stop();
                    self.notice(nus_render::text::icons::BROADCAST, "Phone Access Stopped", "the old address no longer works");
                }
            }
            Hit::CopyPhoneUrl => {
                if let Some(p) = crate::phone::current() {
                    if let Ok(mut cb) = arboard::Clipboard::new() {
                        let _ = cb.set_text(p.url());
                    }
                    self.toast(icons::COPY, "Copied", p.url(), None);
                }
            }
            Hit::HomeArt(i) => {
                if let Some(a) = crate::art::list().get(i) {
                    self.behavior.home_look = HomeLook::Art;
                    self.behavior.home_art = a.key.clone();
                }
            }
            Hit::AddArt => self.add_art(),
            Hit::AskArt => self.ask_for_art(),
            Hit::OpenArtFolder => crate::art::open_dir(),
            Hit::PlaceEdit => self.open_palette(crate::app::PaletteMode::Place),
            Hit::FooterTheme(i) => {
                if let Some(theme) = crate::themes::all().get(i) {
                    let mut names = self.footer_theme_names();
                    if let Some(at) = names.iter().position(|name| name == &theme.name) { names.remove(at); }
                    else { names.push(theme.name.clone()); }
                    self.behavior.footer_themes = Some(names);
                    self.look_scroll = 0.0;
                }
            }
            Hit::Search => self.open_palette(crate::app::PaletteMode::Settings),
            Hit::FooterDefaults => { self.behavior.footer_themes = None; self.look_scroll = 0.0; }
            Hit::UiFont(f) => {self.behavior.typography.system[0].clear();self.behavior.ui_font=f;self.apply_fonts();}
            Hit::UiWeight(w) => {self.behavior.ui_weight=w;self.apply_fonts();}
            Hit::TermFont(f) => {self.behavior.typography.system[1].clear();self.behavior.term_font=f;self.apply_fonts();}
            Hit::TermWeight(w) => {self.behavior.term_weight=w;self.apply_fonts();}

            Hit::Then(t) => self.behavior.then = t,
            Hit::StartupLayout(i) => {
                if let Some((name, _)) = crate::layout_file::saved().get(i) {
                    self.behavior.then_layout = name.clone();
                    self.behavior.then = Then::Layout;
                }
            }
            Hit::EditHomeUrl => {
                self.open_palette(crate::app::PaletteMode::Go);
                if let Some((_, input)) = self.palette.as_mut() {
                    *input = format!("home {}", self.behavior.home_url);
                }
            }
            Hit::Atlas(a) => {
                self.behavior.atlas = a;
                self.behavior.start_on_launch = a != AtlasMode::Planet;
            }
            Hit::Outside(o) => self.behavior.outside = o,
            Hit::Lead(l) => {
                self.behavior.lead = l;
                // Picked, not merely loaded: the two settings that follow from it.
                self.behavior.outside = match l {
                    Lead::Terminal => Outside::Little,
                    Lead::Browser => Outside::NewTab,
                };
            }
            Hit::LoginItem(on) => {
                self.login_note = match crate::little::login_item(on) {
                    Ok(()) => if on { "registered · nus starts with the system".into() } else { "removed".into() },
                    Err(e) => e,
                };
            }
            Hit::SoundOn(b) => {
                self.sound.prefs.enabled = b;
                if b {
                    self.sound.cue("chime");
                }
            }
            Hit::Play(i) => {
                if let Some(n) = crate::sound::NAMES.get(i) {
                    self.sound.cue(n);
                }
            }
            Hit::EventCue(e, c) => {
                let ev = crate::sound::EVENTS[e].0.to_string();
                let cue = if c == usize::MAX { String::new() } else { crate::sound::NAMES[c].to_string() };
                if !cue.is_empty() {
                    self.sound.cue(&cue);
                }
                if ev == "launch" {
                    self.behavior.startup_sound = !cue.is_empty();
                }
                self.sound.prefs.map.insert(ev, cue);
            }
            Hit::EventNext(e) => {
                let ev = crate::sound::EVENTS[e].0;
                let cur = self.sound.prefs.cue_for(ev);
                let idx = cur.as_deref().and_then(|c| crate::sound::NAMES.iter().position(|n| *n == c));
                let next = match idx {
                    Some(i) if i + 1 < crate::sound::NAMES.len() => Some(i + 1),
                    Some(_) => None,
                    None => Some(0),
                };
                let cue = next.map(|i| crate::sound::NAMES[i].to_string()).unwrap_or_default();
                if !cue.is_empty() {
                    self.sound.cue(&cue);
                }
                self.sound.prefs.map.insert(ev.to_string(), cue);
            }
            Hit::OpacityOn(o) => self.surface.opacity_on = o,
            Hit::TexKind(k) => {
                self.surface.texture_kind = k;
                if k != TextureKind::None && self.surface.texture == 0.0 {
                    self.surface.texture = 0.08;
                }
            }
            Hit::TexOn(o) => self.surface.texture_on = o,
            Hit::TexMotion(b) => self.surface.texture_motion = b,
            Hit::ReloadAvatar => self.load_avatar(),
            Hit::PickAvatar => self.pick_avatar(),
            Hit::OpenProfileDir => {
                let dir = std::env::current_dir().unwrap_or_default().join("profile");
                let cmd = if cfg!(target_os = "windows") {
                    format!("start \"\" \"{}\"", dir.display())
                } else if cfg!(target_os = "macos") {
                    format!("open \"{}\"", dir.display())
                } else {
                    format!("xdg-open \"{}\"", dir.display())
                };
                self.run_in_shell(&cmd);
            }
            Hit::StartOnLaunch(b) => self.behavior.start_on_launch = b,
            Hit::StartupSound(b) => {
                self.behavior.startup_sound = b;
                if b {
                    self.play_event("launch");
                }
            }
            Hit::Reduce(r) => self.motion.reduce = r,
            Hit::BarStyle(b) => self.load_bar.style = b,
            Hit::BarColor(c) => self.load_bar.color = c,
        }
        // A preference can change row heights, hit targets and pane geometry.
        // Recompute them for every input path, including accessibility.
        self.layout();
        self.dirty = true;
    }

    /// The colour of the token the picker is on.
    pub(crate) fn tok_color(&self) -> Color {
        let ink = self.theme.ink;
        // The tokens as the user set them, before they were made legible on
        // the tinted paper; editing the drawn ones would tint twice.
        let own = self.theme_edit.build(self.theme.mode, self.surface.signal);
        match self.tok_sel {
            TokSel::Signal => self.surface.signal,
            TokSel::Stop(i) => self.surface.ramp(ink).get(i).copied().unwrap_or(self.surface.signal),
            TokSel::Paper => own.paper,
            TokSel::Ink => own.ink,
            TokSel::Page => own.page,
            TokSel::Caret => own.caret,
            TokSel::Selection => nus_render::Theme::with_alpha(own.selection, 1.0),
            TokSel::Ansi(i) => crate::theme_edit::from_rgb(own.ansi[i.min(15)]),
        }
    }

    /// Set the token the picker is on.
    pub(crate) fn set_tok(&mut self, c: Color) {
        let hit = match self.tok_sel {
            TokSel::Signal => Hit::Signal(c),
            TokSel::Stop(i) => {
                self.stop_sel = i;
                Hit::StopColor(c)
            }
            TokSel::Paper => Hit::TokPaper(c),
            TokSel::Ink => Hit::TokInk(c),
            TokSel::Page => Hit::TokPage(c),
            TokSel::Caret => Hit::TokCaret(Some(c)),
            TokSel::Selection => Hit::TokSelection(Some(c)),
            TokSel::Ansi(i) => {
                self.ansi_sel = i;
                Hit::AnsiSet(c)
            }
        };
        self.apply_setting(hit, 0.0);
    }

    /// A neobrutal tile: hard offset shadow, 2px ink outline, the colour
    /// inside; lifts on hover; a double ring when selected. None = a
    /// dashed "nothing here" tile.
    fn draw_tile(&mut self, scene: &mut Scene, r: Rect, color: Option<Color>, on: bool, key: u64) {
        let t = self.theme.clone();
        let ink = t.ink;
        let (mx, my) = self.mouse;
        let hot = r.contains(mx, my) && scene.clip().is_none_or(|clip| clip.contains(mx, my));
        let dur = self.motion.dur(120.0);
        let h = self.hovers.entry(key).or_insert_with(|| Hover { alpha: Anim::at(0.0), pulse: Anim::at(1.0), hot: false, since: crate::clock::now() });
        if hot != h.hot {
            h.hot = hot;
            h.alpha.go(if hot { 1.0 } else { 0.0 }, dur);
        }
        let a = h.alpha.value();
        if h.alpha.active() {
            self.dirty = true;
        }
        let lift = self.px(2.0) * a;
        let off = self.px(4.0) + lift;
        let tile = Rect::new(r.x - lift, r.y - lift, r.w, r.h);
        let sw = self.px(m::STRUCTURE);
        scene.rect(Rect::new(tile.x + off, tile.y + off, tile.w, tile.h), ink);
        match color {
            Some(c) => scene.rect(tile, c),
            None => {
                scene.rect(tile, t.paper);
                scene.push(nus_render::Instance::hazard(tile, self.px(1.0), fade(ink, 0.35), [0.0, 0.0, 0.0, 0.0], self.px(8.0)));
            }
        }
        scene.outline(tile, sw, ink);
        if on {
            // Double ring: paper inside the ink.
            let inner = Rect::new(tile.x + sw, tile.y + sw, tile.w - 2.0 * sw, tile.h - 2.0 * sw);
            scene.outline(inner, sw, t.paper);
            let inner2 = Rect::new(inner.x + sw, inner.y + sw, inner.w - 2.0 * sw, inner.h - 2.0 * sw);
            scene.outline(inner2, sw, ink);
        }
    }

    /// A preset card: the ramp as its face, the signal as a chip, the name
    /// set in Newsreader; hard shadow, ink outline.
    #[allow(clippy::too_many_arguments)]
    /// An art's card: the art itself, small and alive, a paper strip with its name, its line beneath.
    /// A picture card: a small drawing of what the option does, its name,
    /// and a plain line saying what you get. The same neobrutal card as
    /// the theme chips — outline, hard shadow, the signal under the one
    /// that's on.
    fn draw_pic_card(&mut self, scene: &mut Scene, r: Rect, name: &str, caption: &str, pic: Pic, on: bool, hit: Hit) {
        let t = self.theme.clone();
        let ink = t.ink;
        let hk = hover_key(&format!("piccard:{hit:?}"), 0);
        let (mx, my) = self.mouse;
        let hot = Rect::new(r.x, r.y, r.w + self.px(6.0), r.h + self.px(50.0)).contains(mx, my) && scene.clip().is_none_or(|clip| clip.contains(mx, my));
        let dur = self.motion.dur(140.0);
        let h = self.hovers.entry(hk).or_insert_with(|| Hover { alpha: Anim::at(0.0), pulse: Anim::at(1.0), hot: false, since: crate::clock::now() });
        if hot != h.hot {
            h.hot = hot;
            h.alpha.go(if hot { 1.0 } else { 0.0 }, dur);
        }
        let a = h.alpha.value();
        if h.alpha.active() {
            self.dirty = true;
        }
        let lift = self.px(3.0) * a;
        let off = self.px(5.0) + lift;
        let card = Rect::new(r.x - lift, r.y - lift, r.w, r.h);
        scene.rect(Rect::new(card.x + off, card.y + off, card.w, card.h), if on { self.surface.signal } else { ink });
        scene.rect(card, t.paper);
        let pad = self.px(12.0);
        self.draw_pic(scene, Rect::new(card.x + pad, card.y + pad, card.w - pad * 2.0, card.h - pad * 2.0), pic);
        scene.outline(card, self.px(m::STRUCTURE), ink);
        if on {
            let badge = Rect::new(card.right() - self.px(22.0), card.y, self.px(22.0), self.px(22.0));
            scene.rect(badge, ink);
            self.fonts.draw_icon(scene, icons::CHECK, self.px(14.0), badge.x + self.px(4.0), badge.y + self.px(4.0), t.paper);
        }
        let label = self.label();
        let dim = Style { color: t.dim, ..label };
        let nm = self.fit(label, name, r.w + self.px(6.0));
        self.fonts.draw(scene, Style { color: ink, ..label }, r.x, r.y + r.h + self.px(18.0), &nm);
        let lines = crate::reader::wrap(&self.fonts, dim, caption, r.w + self.px(6.0));
        let mut cy = r.y + r.h + self.px(31.0);
        for l in lines.into_iter().take(2) {
            self.fonts.draw(scene, dim, r.x, cy, &l);
            cy += self.px(12.0);
        }
    }

    /// The drawing inside a picture card: windows, pages, shells and the
    /// icon, at a size where the shape is the whole message.
    pub(crate) fn draw_pic(&mut self, scene: &mut Scene, r: Rect, pic: Pic) {
        let t = self.theme.clone();
        let ink = t.ink;
        let sig = self.surface.signal;
        let hair = self.px(m::HAIRLINE).max(1.0);
        let edge = self.px(1.5).max(1.0);
        let faint = fade(ink, 0.28);
        let mid = fade(ink, 0.5);
        // A window: its frame, and the rule under its tab bar.
        let frame = |scene: &mut Scene, w: Rect, bar: bool, c: Color| {
            scene.outline(w, edge, c);
            if bar {
                scene.hline(w.x, w.y + (w.h * 0.22).round(), w.w, hair, c);
            }
        };
        // Lines of text: n of them, from a fraction of the way down.
        let text = |scene: &mut Scene, w: Rect, from: f32, n: usize, c: Color| {
            let step = self.px(7.0);
            let widths = [0.62f32, 0.84, 0.46, 0.72];
            for i in 0..n {
                let y = w.y + w.h * from + i as f32 * step;
                scene.hline(w.x + self.px(7.0), y, (w.w - self.px(14.0)) * widths[i % 4], hair * 2.0, c);
            }
        };
        // A shell's body: ink, with paper lines and a caret waiting.
        let shell = |scene: &mut Scene, b: Rect| {
            scene.rect(b, ink);
            let step = self.px(7.0);
            for (i, f) in [0.6f32, 0.4, 0.75].iter().enumerate() {
                let y = b.y + self.px(9.0) + i as f32 * step;
                scene.hline(b.x + self.px(7.0), y, (b.w - self.px(14.0)) * f, hair * 2.0, fade(t.paper, 0.75));
            }
            scene.rect(Rect::new(b.x + self.px(7.0), b.y + self.px(9.0) + step * 3.0 - self.px(4.0), self.px(5.0), self.px(6.0)), sig);
        };
        // The prompt: one line across the middle, with the caret at its head.
        let prompt = |app: &Self, scene: &mut Scene, b: Rect, rows: bool| {
            let y = (b.y + b.h * 0.46).round();
            let x = b.x + b.w * 0.16;
            let w = b.w * 0.68;
            scene.hline(x, y, w, hair * 2.0, mid);
            scene.rect(Rect::new(x, y - app.px(7.0), app.px(4.0), app.px(8.0)), sig);
            if rows {
                for i in 0..2 {
                    scene.hline(x, y + app.px(10.0) + i as f32 * app.px(7.0), w * if i == 0 { 0.8 } else { 0.55 }, hair, faint);
                }
            }
        };
        // A page: its masthead, a picture, and prose.
        let page = |scene: &mut Scene, b: Rect| {
            scene.rect(Rect::new(b.x + self.px(7.0), b.y + self.px(7.0), (b.w - self.px(14.0)) * 0.5, self.px(6.0)), ink);
            scene.rect(Rect::new(b.x + self.px(7.0), b.y + self.px(18.0), (b.w - self.px(14.0)) * 0.38, b.h - self.px(25.0)), faint);
            let x = b.x + self.px(7.0) + (b.w - self.px(14.0)) * 0.44;
            let w = (b.w - self.px(14.0)) * 0.56;
            for i in 0..4 {
                scene.hline(x, b.y + self.px(22.0) + i as f32 * self.px(7.0), w * [0.94f32, 0.8, 0.9, 0.5][i], hair * 2.0, mid);
            }
        };
        // The app icon at a moment of its draw-in.
        let icon = |app: &mut Self, scene: &mut Scene, b: Rect, progress: f32, alpha: f32| {
            let size = (b.w.min(b.h).round() as u32).clamp(32, 512);
            let bind = app.pic_icon(size, progress);
            let d = b.w.min(b.h);
            let rect = Rect::new((b.x + (b.w - d) / 2.0).round(), (b.y + (b.h - d) / 2.0).round(), d, d);
            let outer = scene.clip();
            let clip = outer.map_or(rect, |o| rect.intersect(&o));
            if alpha >= 0.999 {
                scene.texture(rect, bind, Some(clip));
            } else {
                scene.texture_uv_alpha(rect, [0.0, 0.0, 1.0, 1.0], bind, Some(clip), alpha);
            }
            scene.layer(outer);
        };
        // The screen a window comes up on.
        let screen = |scene: &mut Scene, r: Rect| {
            scene.outline(r, hair, faint);
        };
        let body = |w: Rect| Rect::new(w.x, w.y + (w.h * 0.22).round(), w.w, w.h - (w.h * 0.22).round());
        match pic {
            Pic::Setting(hit) => self.draw_setting_picture(scene, r, hit),
            Pic::LeadTerminal => {
                frame(scene, r, true, ink);
                // The live tab, filled; a page waiting beside it.
                scene.rect(Rect::new(r.x + hair, r.y + hair, r.w * 0.42, (r.h * 0.22).round() - hair), ink);
                scene.hline(r.x + r.w * 0.5, r.y + (r.h * 0.11).round(), r.w * 0.3, hair * 2.0, faint);
                shell(scene, body(r).inset(hair));
            }
            Pic::LeadBrowser => {
                frame(scene, r, true, ink);
                // The address, across the bar.
                let bar = Rect::new(r.x + self.px(6.0), r.y + self.px(4.0), r.w - self.px(12.0), (r.h * 0.22).round() - self.px(8.0));
                scene.outline(bar, hair, mid);
                scene.hline(bar.x + self.px(4.0), bar.y + bar.h / 2.0, bar.w * 0.5, hair * 2.0, mid);
                page(scene, body(r));
            }
            Pic::WinLast => {
                screen(scene, r);
                // Where it was, and where it comes back.
                let ghost = Rect::new(r.x + self.px(6.0), r.y + self.px(5.0), r.w * 0.5, r.h * 0.5);
                scene.outline(ghost, hair, faint);
                let w = Rect::new(r.x + r.w * 0.34, r.y + r.h * 0.34, r.w * 0.58, r.h * 0.56);
                scene.rect(w, t.paper);
                frame(scene, w, true, ink);
            }
            Pic::WinMax => {
                screen(scene, r);
                // The system bar stays; the window takes the rest.
                scene.rect(Rect::new(r.x + hair, r.y + hair, r.w - hair * 2.0, self.px(6.0)), faint);
                let w = Rect::new(r.x + self.px(3.0), r.y + self.px(9.0), r.w - self.px(6.0), r.h - self.px(12.0));
                scene.rect(w, t.paper);
                frame(scene, w, true, ink);
            }
            Pic::WinFull => {
                let w = r;
                scene.rect(w, t.paper);
                frame(scene, w, false, ink);
                shell(scene, w.inset(self.px(6.0)));
            }
            Pic::WinCentered => {
                screen(scene, r);
                let w = Rect::new((r.x + r.w * 0.22).round(), (r.y + r.h * 0.24).round(), r.w * 0.56, r.h * 0.52);
                scene.rect(w, t.paper);
                frame(scene, w, true, ink);
            }
            Pic::SplashDraw => {
                icon(self, scene, Rect::new(r.x, r.y, r.w, r.h - self.px(6.0)), 0.62, 1.0);
            }
            Pic::SplashStill => {
                icon(self, scene, Rect::new(r.x, r.y, r.w, r.h - self.px(6.0)), 1.0, 1.0);
            }
            Pic::SplashNone => {
                // No icon on the way in: the start page, straight away.
                icon(self, scene, Rect::new(r.x, r.y, r.w, r.h - self.px(6.0)), 1.0, 0.12);
                frame(scene, r, true, ink);
                prompt(self, scene, body(r), false);
            }
            Pic::StartPrompt => {
                frame(scene, r, true, ink);
                prompt(self, scene, body(r), true);
            }
            Pic::StartHome => {
                frame(scene, r, true, ink);
                let bar = Rect::new(r.x + self.px(6.0), r.y + self.px(4.0), r.w * 0.5, (r.h * 0.22).round() - self.px(8.0));
                scene.outline(bar, hair, mid);
                page(scene, body(r));
            }
            Pic::StartLayout => {
                frame(scene, r, true, ink);
                // Tabs across the top, two panes below.
                let bh = (r.h * 0.22).round();
                scene.rect(Rect::new(r.x + hair, r.y + hair, r.w * 0.3, bh - hair), ink);
                for i in 1..3 {
                    scene.vline(r.x + r.w * (0.3 + 0.24 * i as f32), r.y + hair, bh - hair, hair, faint);
                }
                let b = body(r);
                let split = (b.x + b.w * 0.5).round();
                shell(scene, Rect::new(b.x + hair, b.y + hair, split - b.x - hair, b.h - hair * 2.0));
                scene.vline(split, b.y, b.h, hair, ink);
                text(scene, Rect::new(split, b.y, b.w - (split - b.x), b.h), 0.18, 4, mid);
            }
            Pic::StartLast => {
                frame(scene, r, true, ink);
                page(scene, body(r));
                // The one you had open: its tab, still lit.
                scene.rect(Rect::new(r.x + hair, r.y + hair, r.w * 0.4, (r.h * 0.22).round() - hair), ink);
                let isz = self.px(11.0);
                self.fonts.draw_icon(scene, icons::BACK, isz, r.x + self.px(6.0), r.y + (r.h * 0.11).round() - isz / 2.0, t.paper);
            }
            Pic::LookLine => {
                prompt(self, scene, r, true);
            }
            Pic::LookPlate => {
                let d = (r.h * 0.62).min(r.w * 0.62);
                icon(self, scene, Rect::new((r.x + (r.w - d) / 2.0).round(), r.y, d, d), 1.0, 1.0);
                let y = (r.y + d + self.px(10.0)).round();
                scene.hline(r.x + r.w * 0.14, y, r.w * 0.72, hair * 2.0, mid);
                scene.rect(Rect::new(r.x + r.w * 0.14, y - self.px(7.0), self.px(4.0), self.px(8.0)), sig);
            }
            Pic::NewPrompt | Pic::NewShell | Pic::NewLaunch => {
                // This window, and the one the keystroke makes.
                let here = Rect::new(r.x, r.y, r.w * 0.6, r.h * 0.62);
                scene.rect(here, t.paper);
                frame(scene, here, true, faint);
                let w = Rect::new((r.x + r.w * 0.3).round(), (r.y + r.h * 0.3).round(), r.w * 0.7, r.h * 0.7);
                scene.rect(w, t.paper);
                frame(scene, w, true, ink);
                let b = body(w);
                match pic {
                    Pic::NewShell => shell(scene, b.inset(hair)),
                    Pic::NewLaunch => icon(self, scene, Rect::new(b.x, b.y, b.w, b.h - self.px(2.0)), 0.62, 1.0),
                    _ => prompt(self, scene, b, true),
                }
            }
        }
    }

    fn draw_art_card(&mut self, scene: &mut Scene, r: Rect, key: &str, name: &str, says: &str, on: bool, builtin: bool, hit: Hit) {
        let t = self.theme.clone();
        let ink = t.ink;
        let hk = hover_key(&format!("artcard:{hit:?}"), 0);
        let (mx, my) = self.mouse;
        let hot = Rect::new(r.x, r.y, r.w + self.px(6.0), r.h + self.px(50.0)).contains(mx, my) && scene.clip().is_none_or(|clip| clip.contains(mx, my));
        let dur = self.motion.dur(140.0);
        let h = self.hovers.entry(hk).or_insert_with(|| Hover { alpha: Anim::at(0.0), pulse: Anim::at(1.0), hot: false, since: crate::clock::now() });
        if hot != h.hot {
            h.hot = hot;
            h.alpha.go(if hot { 1.0 } else { 0.0 }, dur);
        }
        let a = h.alpha.value();
        let lift = self.px(3.0) * a;
        let off = self.px(5.0) + lift;
        let card = Rect::new(r.x - lift, r.y - lift, r.w, r.h);
        scene.rect(Rect::new(card.x + off, card.y + off, card.w, card.h), if on { self.surface.signal } else { ink });
        scene.rect(card, t.paper);
        // The art, alive, run at a pane's size and shrunk into the card;
        // its line a small box a third of the way down.
        // Stars need a closer view to remain distinct in a small preview.
        let sc = card.w / self.px(if key == "space" { 480.0 } else { 1280.0 });
        let cmds = {
            let (w, h) = (card.w / sc, card.h / sc);
            let env = crate::art::Env {
                w,
                h,
                line: [w * 0.2, h * 0.34, w * 0.6, self.px(40.0)],
                rows: 0.0,
                pointer: None,
                typed: String::new(),
                taps: Vec::new(),
                face: if self.theme.mode == nus_render::Mode::Ink { "ink".into() } else { "paper".into() },
                paper: t.paper,
                ink: t.ink,
                signal: self.surface.signal,
                dim: t.dim,
                tint: t.tint,
                place: self.place(),
                pieces: Vec::new(),
                procs: Some(self.procs_shared()),
                scale: self.scale,
            };
            let art = self.art_previews.entry(key.to_string()).or_insert_with(|| crate::art::Art::open(key));
            art.tend();
            if self.motion.reduced() { art.frame_at(env, 8.0) } else { art.frame(env) }
        };
        self.draw_art_cmds_scaled(scene, card, cmds, sc);
        // The line, in miniature.
        let lx = card.x + card.w * 0.2;
        let ly = card.y + card.h * 0.34 + self.px(10.0);
        let line_ink = self.art_previews.get(key).map(|a| a.backdrop).unwrap_or_default().foreground(t.mode, ink, t.paper);
        scene.hline(lx, ly, card.w * 0.6, self.px(m::HAIRLINE), fade(line_ink, 0.5));
        scene.rect(Rect::new(lx, ly - self.px(6.0), self.px(3.0), self.px(5.0)), self.surface.signal);
        scene.outline(card, self.px(m::STRUCTURE), ink);
        if on {
            let badge = Rect::new(card.right() - self.px(22.0), card.y, self.px(22.0), self.px(22.0));
            scene.rect(badge, ink);
            self.fonts.draw_icon(scene, icons::CHECK, self.px(14.0), badge.x + self.px(4.0), badge.y + self.px(4.0), t.paper);
        }
        let label = self.label();
        let dim = Style { color: t.dim, ..label };
        let nm = self.fit(label, &name.to_uppercase(), r.w);
        self.fonts.draw(scene, Style { color: ink, ..label }, r.x, r.y + r.h + self.px(18.0), &nm);
        let sub = if says.is_empty() { if builtin { "ships with nus".to_string() } else { "yours".to_string() } } else { says.to_string() };
        for (i, line) in crate::reader::wrap(&self.fonts, dim, &sub, r.w).into_iter().take(2).enumerate() {
            self.fonts.draw(scene, dim, r.x, r.y + r.h + self.px(31.0 + i as f32 * 12.0), &line);
        }
        if !self.motion.reduced() && self.art_wants_frame() {
            self.dirty = true;
        }
    }

    pub(crate) fn draw_card(&mut self, scene: &mut Scene, r: Rect, name: &str, ramp: &[Color], signal: Color, angle: f32, on: bool, faces: Option<(Color, Color, Color, Color)>, key: u64) {
        let t = self.theme.clone();
        let ink = t.ink;
        let (mx, my) = self.mouse;
        let hot = Rect::new(r.x, r.y, r.w + self.px(6.0), r.h + self.px(6.0)).contains(mx, my) && scene.clip().is_none_or(|clip| clip.contains(mx, my));
        let dur = self.motion.dur(140.0);
        let h = self.hovers.entry(key).or_insert_with(|| Hover { alpha: Anim::at(0.0), pulse: Anim::at(1.0), hot: false, since: crate::clock::now() });
        if hot != h.hot {
            h.hot = hot;
            h.alpha.go(if hot { 1.0 } else { 0.0 }, dur);
        }
        let a = h.alpha.value();
        if h.alpha.active() {
            self.dirty = true;
        }
        let lift = self.px(3.0) * a;
        let off = self.px(5.0) + lift;
        let card = Rect::new(r.x - lift, r.y - lift, r.w, r.h);
        let sw = self.px(m::STRUCTURE);
        scene.rect(Rect::new(card.x + off, card.y + off, card.w, card.h), if on { signal } else { ink });
        if ramp.is_empty() {
            // Save-as: paper face, dashed feel via a hazard, a plus.
            scene.rect(card, t.paper);
            scene.push(nus_render::Instance::hazard(card, self.px(1.0), fade(ink, 0.25), [0.0, 0.0, 0.0, 0.0], self.px(10.0)));
            let isz = self.px(22.0);
            self.fonts.draw_icon(scene, icons::PLUS, isz, card.x + (card.w - isz) / 2.0, card.y + (card.h - isz) / 2.0 - self.px(8.0), ink);
        } else {
            scene.push(nus_render::Instance::rounded_stops(card, 0.0, ramp, angle, 0.0, false));
            // A paper strip along the bottom for the name, like a label.
            let strip_h = self.px(30.0);
            scene.rect(Rect::new(card.x, card.bottom() - strip_h, card.w, strip_h), t.paper);
            scene.hline(card.x, card.bottom() - strip_h, card.w, sw, ink);
            let chip = self.px(12.0);
            scene.rect(Rect::new(card.x + self.px(12.0), card.bottom() - strip_h + (strip_h - chip) / 2.0, chip, chip), signal);
            scene.outline(Rect::new(card.x + self.px(12.0), card.bottom() - strip_h + (strip_h - chip) / 2.0, chip, chip), self.px(1.0), ink);
            // The two faces as tiny pages: paper with an ink line, ink with a paper line.
            if let Some((pp, pi, ip, ii)) = faces {
                let pw = self.px(22.0);
                let ph = self.px(28.0);
                let mut fx = card.right() - self.px(12.0) - pw;
                for (bg, fg) in [(ip, ii), (pp, pi)] {
                    let pr = Rect::new(fx, card.y + self.px(10.0), pw, ph);
                    scene.rect(pr, bg);
                    scene.outline(pr, self.px(1.0), ink);
                    for k in 0..3 {
                        scene.rect(Rect::new(pr.x + self.px(4.0), pr.y + self.px(6.0) + k as f32 * self.px(6.0), pw - self.px(8.0) - if k == 2 { self.px(6.0) } else { 0.0 }, self.px(2.0)), fg);
                    }
                    fx -= pw + self.px(6.0);
                }
            }
        }
        let wm = Style { font: self.f.wordmark, px: self.px(19.0), color: ink, tracking: 0.0 };
        let ny = if ramp.is_empty() { card.bottom() - self.px(14.0) } else { card.bottom() - self.px(10.0) };
        let nx = if ramp.is_empty() { card.x + self.px(12.0) } else { card.x + self.px(32.0) };
        let nm = self.fit(wm, name, card.w - (nx - card.x) - self.px(10.0));
        self.fonts.draw(scene, wm, nx, ny, &nm);
        scene.outline(card, sw, ink);
        if on {
            let inner = Rect::new(card.x + sw, card.y + sw, card.w - 2.0 * sw, card.h - 2.0 * sw);
            scene.outline(inner, sw, t.paper);
        }
    }

    /// The live proof: a miniature nus window in the current look —
    /// carapace with its ramp and texture, chrome, sidebar, a shell with
    /// coloured runs and the cursor, a page.
    fn draw_studio(&mut self, scene: &mut Scene, r: Rect) {
        // The page's own clip: nested clips sit inside it and hand it back.
        let outer = scene.clip();
        let within = |r: Rect| outer.map_or(r, |o| r.intersect(&o));
        let t = self.theme.clone();
        let ink = t.ink;
        let paper = self.paper();
        let ramp = self.surface.ramp(ink);
        let sw = (self.px(self.surface.shell_width) * 0.6).max(self.px(3.0));
        let radius = self.px(self.surface.shell_radius) * 0.5;
        let win = r;
        // Hard shadow under the whole proof: it's a card too.
        scene.rect(Rect::new(win.x + self.px(6.0), win.y + self.px(6.0), win.w, win.h), ink);
        scene.push(nus_render::Instance::rounded(win, radius, paper));
        match self.surface.shell {
            Shell::Band => {
                scene.layer(Some(within(Rect::new(win.x, win.y, win.w, sw))));
                scene.push(nus_render::Instance::rounded(win, radius, self.surface.signal));
                scene.layer(outer);
            }
            Shell::Stroke => scene.push(nus_render::Instance::stroke(win, radius, sw, self.surface.signal, None, 0.0)),
            Shell::Gradient => scene.push(nus_render::Instance::stroke_stops(win, radius, sw, &ramp, self.surface.angle, 0.0, false)),
            Shell::Aurora => scene.push(nus_render::Instance::stroke_stops(win, radius, sw, &ramp, self.surface.angle, self.shell_phase, true)),
        }
        if let Some(kind) = self.surface.texture_kind.shader_kind() {
            if self.surface.texture > 0.0 && self.surface.texture_on == TextureOn::Carapace {
                let gc = [1.0, 1.0, 1.0, (self.surface.texture * 3.0).min(1.0)];
                let tm = if self.surface.texture_motion { crate::clock::since(self.started).as_secs_f32() % 3600.0 } else { 0.0 };
                let pitch = self.px(self.surface.texture_scale);
                if self.surface.shell == Shell::Band {
                    scene.layer(Some(within(Rect::new(win.x, win.y, win.w, sw))));
                    scene.push(nus_render::Instance::texture_stroke(win, kind, gc, pitch, tm, radius, sw));
                    scene.layer(outer);
                } else {
                    scene.push(nus_render::Instance::texture_stroke(win, kind, gc, pitch, tm, radius, sw));
                }
            }
        }
        // Inside the carapace.
        let inner = Rect::new(win.x + sw, win.y + sw, win.w - 2.0 * sw, win.h - 2.0 * sw);
        scene.layer(Some(within(inner)));
        let strip_h = self.px(20.0);
        let wm = Style { font: self.f.wordmark, px: self.px(13.0), color: ink, tracking: 0.0 };
        self.fonts.draw(scene, wm, inner.x + self.px(10.0), inner.y + self.px(14.0), "nus");
        let tiny = Style { font: self.f.ui, px: self.px(8.0), color: t.dim, tracking: 0.08 };
        self.fonts.draw(scene, tiny, inner.x + self.px(40.0), inner.y + self.px(13.5), "01 SHELL · ~/NUS");
        scene.hline(inner.x, inner.y + strip_h, inner.w, self.px(1.0), ink);
        // Sidebar.
        let sb_w = self.px(74.0);
        let sb = Rect::new(inner.x, inner.y + strip_h + 1.0, sb_w, inner.h - strip_h - 1.0);
        scene.vline(sb.right(), sb.y, sb.h, self.px(1.0), ink);
        let row_h = self.px(16.0);
        for (i, name) in ["shell", "docs", "cargo"].iter().enumerate() {
            let ry = sb.y + self.px(6.0) + i as f32 * row_h;
            if i == 0 {
                scene.rect(Rect::new(sb.x, ry, sb.w, row_h), t.tint);
                scene.rect(Rect::new(sb.x, ry, self.px(1.5), row_h), self.surface.signal);
            }
            let st = Style { font: if i == 0 { self.f.strong } else { self.f.ui }, px: self.px(8.0), color: if i == 0 { ink } else { t.dim }, tracking: 0.0 };
            self.fonts.draw(scene, st, sb.x + self.px(10.0), ry + self.px(11.0), name);
        }
        // Shell pane.
        let pane = Rect::new(sb.right() + 1.0, sb.y, (inner.w - sb_w) * 0.58, sb.h);
        let ansi: Vec<Color> = (0..16).map(|i| crate::theme_edit::from_rgb(t.ansi[i])).collect();
        let mono = |c: Color, me: &Self| Style { font: me.f.ui, px: me.px(9.0), color: c, tracking: 0.0 };
        let lines: Vec<Vec<(Color, &str)>> = vec![
            vec![(ansi[2], "seb@nus"), (ink, ":"), (ansi[4], "~/nus"), (ink, "$ cargo test")],
            vec![(ansi[3], "warning"), (ink, ": unused"), (t.dim, " · 18 passed "), (ansi[1], "0 failed")],
            vec![(ansi[5], "> "), (ansi[6], "git"), (ink, " log "), (t.dim, "9058fca")],
        ];
        let mut ly = pane.y + self.px(16.0);
        for line in lines {
            let mut lx = pane.x + self.px(10.0);
            for (c, txt) in line {
                lx += self.fonts.draw(scene, mono(c, self), lx, ly, txt);
            }
            ly += self.px(14.0);
        }
        // Prompt with the cursor as configured.
        let mut lx = pane.x + self.px(10.0);
        lx += self.fonts.draw(scene, mono(ansi[2], self), lx, ly, "$ ");
        let cur_c = match self.cursor.color { crate::settings::CursorColor::Theme => self.theme.caret, _ => self.surface.signal };
        let cw = self.px(5.5);
        let chh = self.px(11.0);
        match self.cursor.shape {
            crate::settings::CursorShapePref::Beam => scene.rect(Rect::new(lx, ly - self.px(9.0), self.px(1.5), chh), cur_c),
            crate::settings::CursorShapePref::Underline => scene.rect(Rect::new(lx, ly + self.px(1.0), cw, self.px(1.5)), cur_c),
            _ => scene.rect(Rect::new(lx, ly - self.px(9.0), cw, chh), cur_c),
        }
        // Page pane.
        let page = Rect::new(pane.right(), pane.y, inner.right() - pane.right(), pane.h);
        scene.vline(page.x, page.y, page.h, self.px(1.0), ink);
        scene.rect(Rect::new(page.x + 1.0, page.y, page.w, page.h), t.page);
        let page_ink = if crate::theme_edit::contrast(ink, t.page) >= crate::theme_edit::contrast(t.paper, t.page) { ink } else { t.paper };
        let serif = Style { font: self.f.serif, px: self.px(12.0), color: page_ink, tracking: 0.0 };
        self.fonts.draw(scene, serif, page.x + self.px(12.0), page.y + self.px(24.0), "Monterey Bay");
        for k in 0..4 {
            let w = page.w - self.px(24.0) - if k == 3 { page.w * 0.3 } else { 0.0 };
            scene.rect(Rect::new(page.x + self.px(12.0), page.y + self.px(34.0) + k as f32 * self.px(9.0), w.max(0.0), self.px(3.0)), fade(page_ink, 0.25));
        }
        // ANSI strip along the bottom of the shell pane.
        let strip_y = pane.bottom() - self.px(12.0);
        let cell = (pane.w - self.px(20.0)) / 16.0;
        for i in 0..16 {
            scene.rect(Rect::new(pane.x + self.px(10.0) + i as f32 * cell, strip_y, cell - 1.0, self.px(6.0)), ansi[i]);
        }
        scene.layer(outer);
        scene.outline(win, self.px(m::STRUCTURE), ink);
    }

    /// The row labels of a section, for the palette.
    pub(crate) fn settings_labels(&self, section: usize) -> Vec<(String, ())> {
        self.rows_for(section).into_iter().map(|(l, _)| (l, ())).collect()
    }

    fn rows_for(&self, section: usize) -> Vec<(String, Control)> {
        self.rows_for_at(section, self.look_tab)
    }

    fn rows_for_at(&self, section: usize, look_tab: usize) -> Vec<(String,Control)> {
        let mut rows = self.visual_settings(section, self.rows_for_raw(section, look_tab));
        rows.retain(|(k, c)| !(k.is_empty() && matches!(c, Control::Info(s) if s.is_empty())));
        match section {
            2 => sectioned(rows, &[("WINDOW AT LAUNCH", "WHEN NUS LAUNCHES"), ("TERMINAL OR BROWSER", "START PAGE · AT LAUNCH AND EVERY NEW TAB"), (key("N", false).as_str(), "NEW WINDOWS"), ("LINKS FROM OTHER APPS", "FROM OTHER APPS")]),
            5 => rows,
            6 => captioned(rows, &[("LOADING BAR", "LOADING"), ("DEFAULT BROWSER", "THE SYSTEM"), ("SEARCH", "AS SHIPPED")]),
            _ => rows,
        }
    }

    fn rows_for_raw(&self, section: usize, look_tab: usize) -> Vec<(String, Control)> {
        use Control::*;
        let hex = surface::hex;
        let ink = self.theme.mode == nus_render::Mode::Ink;
        match section {
            SEC_VIEWERS => self.viewer_settings(),
            SEC_MENU=>{
                use crate::menu_drawer::{SignalStyle,Density};
                let c=&self.behavior.menu_drawer;
                let mut rows=vec![
                    ("".into(),Info("Choose your Signal icon, then arrange its drawer. Mix compact work lists with expanded download cards and quick actions. Each section has its own look and position.".into())),
                    ("MENU BAR / TRAY ICON".into(),Choice(vec![("ON".into(),Hit::MenuEnabled(true),c.enabled),("OFF".into(),Hit::MenuEnabled(false),!c.enabled)])),
                    ("SIGNAL ICON".into(),Choice(vec![("DOT".into(),Hit::MenuSignal(SignalStyle::Dot),c.signal==SignalStyle::Dot),("COUNT".into(),Hit::MenuSignal(SignalStyle::Count),c.signal==SignalStyle::Count),("STATUS TEXT".into(),Hit::MenuSignal(SignalStyle::Text),c.signal==SignalStyle::Text)])),
                    ("".into(),Info("macOS can show a count or status beside the icon. Windows and Linux use an icon badge and tooltip. The orbit follows your theme.".into())),
                    ("LIVE PREVIEW".into(),DrawerPreview),
                    ("".into(),Info("This is the drawer itself, drawn here as it opens from the menu bar, with your work and downloads in it. Every change below shows at once.".into())),
                    ("".into(),Buttons(vec![("OPEN YOUR DRAWER".into(),icons::SQUARES,Hit::MenuPreview)])),
                ];
                for (i,s) in c.sections().iter().enumerate(){
                    rows.push((format!("{}. {}",i+1,s.module.label().to_uppercase()),Choice(vec![("HIDDEN".into(),Hit::MenuDensity(s.module,Density::Hidden),s.density==Density::Hidden),("COMPACT".into(),Hit::MenuDensity(s.module,Density::Compact),s.density==Density::Compact),("EXPANDED".into(),Hit::MenuDensity(s.module,Density::Expanded),s.density==Density::Expanded)])));
                    let mut buttons=Vec::new();if i>0{buttons.push(("MOVE UP".into(),icons::SQUARES,Hit::MenuMove(s.module,false)));}if i<2{buttons.push(("MOVE DOWN".into(),icons::SQUARES,Hit::MenuMove(s.module,true)));}rows.push(("ORDER".into(),Buttons(buttons)));
                }
                rows.extend([
                    ("SHOW TASK & FILE NAMES".into(),Choice(vec![("ON".into(),Hit::MenuNames(true),c.names),("OFF".into(),Hit::MenuNames(false),!c.names)])),
                    ("INCLUDE FINISHED ITEMS".into(),Choice(vec![("ON".into(),Hit::MenuRecent(true),c.recent),("OFF".into(),Hit::MenuRecent(false),!c.recent)])),
                    ("".into(),Info("The drawer is also in the nus footer, including desktops without a tray. Escape or clicking outside closes it. Opening the drawer never starts a terminal.".into())),
                ]);rows
            }
            0 => {
                let tab = look_tab.min(LOOK_TABS.len() - 1);
                let strip: Vec<(String, Hit, bool)> = LOOK_TABS.iter().enumerate().map(|(k, n)| (n.to_string(), Hit::LookTab(k), k == tab)).collect();
                let mut v: Vec<(String, Control)> = if tab == LOOK_APP_ICON {
                    vec![("".into(), Strip(strip))]
                } else {
                    vec![("".into(), Studio), ("".into(), Strip(strip))]
                };
                let rest: Vec<(String, Control)> = match tab {
                    LOOK_PRESETS => {
                        let themes = crate::themes::all();
                        let card = |k: usize, t: &crate::themes::StockTheme| -> (String, Vec<Color>, Color, f32, Hit, bool, Option<(Color, Color, Color, Color)>) {
                            let ramp = t.surface.ramp(t.ink.ink);
                            (t.name.clone(), ramp, t.surface.signal, t.surface.angle, Hit::Preset(k), t.name == self.preset_name, Some((t.paper.paper, t.paper.ink, t.ink.paper, t.ink.ink)))
                        };
                        let mut originals: Vec<_> = themes.iter().enumerate().filter(|(_, t)| !t.port).map(|(k, t)| card(k, t)).collect();
                        originals.push(("save as…".into(), Vec::new(), self.surface.signal, 0.0, Hit::SavePreset, false, None));
                        let ports: Vec<_> = themes.iter().enumerate().filter(|(_, t)| t.port).map(|(k, t)| card(k, t)).collect();
                        let current = themes.iter().find(|t| t.name == self.preset_name).map(|t| t.story.clone()).unwrap_or_else(|| "edited from a theme · SAVE AS keeps it".into());
                        vec![
                            ("THEMES".into(), Cards(originals)),
                            ("".into(), Info(current)),
                            ("PORTS".into(), Cards(ports)),
                            ("FOOTER THEME SLOTS".into(), Info("Nine visible slots in a 3 × 3 grid. Select themes below to add or remove them; scroll the footer picker to reach additional rows.".into())),
                            ("SHOW IN FOOTER".into(), Cards(themes.iter().enumerate().map(|(i,t)| {
                                let mut c = card(i,t); c.4 = Hit::FooterTheme(i); c.5 = self.footer_theme_names().contains(&t.name); c
                            }).collect())),
                            ("".into(), Buttons(vec![("RESTORE DEFAULT NINE".into(),icons::UNDO,Hit::FooterDefaults)])),
                            ("".into(), Info("a theme is the whole look: both faces' tokens and sixteens, the carapace, the cursor, the bar, a few sounds · saved ones live in profile/themes".into())),
                            ("".into(), Buttons(vec![("OPEN THEMES FOLDER".into(), icons::FOLDER, Hit::OpenThemes)])),
                        ]
                    }
                    LOOK_SURFACE => {
                let ink = self.theme.ink;
                let presets = surface::presets();
                let mut preset_chips: Vec<(String, Hit, bool)> =
                    presets.iter().enumerate().map(|(k, p)| (p.name.caps(), Hit::Preset(k), p.name == self.preset_name)).collect();
                preset_chips.push(("+ SAVE AS…".into(), Hit::SavePreset, false));
                let sig: Vec<(Option<Color>, Hit, bool)> =
                    SWATCHES[..6].iter().map(|&(_, c)| (Some(c), Hit::Signal(c), c == self.surface.signal)).collect();
                let fam: Vec<(Option<Color>, Hit, bool)> = surface::family(self.surface.signal).iter().map(|&c| (Some(c), Hit::Signal(c), false)).collect();
                let ramp = self.surface.ramp(ink);
                let stop_sel = self.stop_sel.min(ramp.len() - 1);
                let mut stops: Vec<(Option<Color>, Hit, bool)> = ramp.iter().enumerate().map(|(i, &c)| (Some(c), Hit::StopSel(i), i == stop_sel)).collect();
                stops.push((None, Hit::StopAdd, false));
                let mut stop_colors: Vec<(Option<Color>, Hit, bool)> =
                    SWATCHES.iter().map(|&(_, c)| (Some(c), Hit::StopColor(c), ramp.get(stop_sel) == Some(&c))).collect();
                stop_colors.extend(surface::family(self.surface.signal).iter().map(|&c| (Some(c), Hit::StopColor(c), false)));
                let mut base: Vec<(Option<Color>, Hit, bool)> = vec![(None, Hit::Base(None), self.surface.base.is_none())];
                base.extend(SWATCHES.iter().map(|&(_, c)| (Some(c), Hit::Base(Some(c)), self.surface.base == Some(c))));
                let translucent = self.target.translucent();
                let _ = (&preset_chips, &sig, &fam, &stops, &stop_colors);
                // The carapace's tokens as tiles: signal, then the ramp's stops.
                let mut tiles: Vec<(String, Option<Color>, String, Hit, bool)> = vec![("SIGNAL".into(), Some(self.surface.signal), hex(self.surface.signal), Hit::TokSel(TokSel::Signal), self.tok_sel == TokSel::Signal)];
                for (i, &c) in ramp.iter().enumerate() {
                    tiles.push((format!("STOP {}", i + 1), Some(c), hex(c), Hit::TokSel(TokSel::Stop(i)), self.tok_sel == TokSel::Stop(i)));
                }
                if ramp.len() < 4 {
                    tiles.push(("ADD".into(), None, "a stop".into(), Hit::StopAdd, false));
                }
                if self.surface.stops.len() > 2 {
                    tiles.push(("REMOVE".into(), None, "last stop".into(), Hit::StopRemove, false));
                }
                let editing = matches!(self.tok_sel, TokSel::Signal | TokSel::Stop(_));
                let mut tray: Vec<(String, Option<Color>, String, Hit, bool)> = Vec::new();
                if editing {
                    let cur = self.tok_color();
                    for &c in surface::family(self.surface.signal).iter() {
                        tray.push((String::new(), Some(c), hex(c), Hit::TokSet(c), c == cur));
                    }
                    for &(_, c) in SWATCHES.iter() {
                        tray.push((String::new(), Some(c), hex(c), Hit::TokSet(c), c == cur));
                    }
                }
                // White leads the bases — the paper's own — then ink, then the signals.
                let mut base_tiles: Vec<(String, Option<Color>, String, Hit, bool)> = vec![("NONE".into(), None, "the theme's paper as is".into(), Hit::Base(None), self.surface.base.is_none())];
                base_tiles.extend(SWATCHES[6..].iter().chain(SWATCHES[..6].iter()).map(|&(_, c)| (String::new(), Some(c), hex(c), Hit::Base(Some(c)), self.surface.base == Some(c))));
                let _ = base;
                let mut v = vec![
                    ("CARAPACE TOKENS".into(), Tokens(tiles, true)),
                ];
                if editing {
                    let what = match self.tok_sel { TokSel::Signal => "signal".to_string(), TokSel::Stop(i) => format!("stop {}", i + 1), _ => String::new() };
                    v.push((format!("{} HUE", what.caps()), Slider(self::Slider::Hue, self.slider_value(self::Slider::Hue), format!("{}°", (surface::to_hsl(self.tok_color()).0 * 360.0).round()))));
                    v.push(("SATURATION".into(), Slider(self::Slider::Sat, self.slider_value(self::Slider::Sat), format!("{}%", (surface::to_hsl(self.tok_color()).1 * 100.0).round()))));
                    v.push(("LIGHTNESS".into(), Slider(self::Slider::Light, self.slider_value(self::Slider::Light), format!("{}% · {}", (surface::to_hsl(self.tok_color()).2 * 100.0).round(), hex(self.tok_color())))));
                    v.push(("TRAY".into(), Tokens(tray, false)));
                }
                v.push(("".into(), Info("signal: the carapace, the window square, ticks, progress · stops: the gradient and aurora ramps".into())));
                v.push(("BASE".into(), Tokens(base_tiles, false)));
                v.extend(vec![
                    (
                        "TINT".into(),
                        Slider(
                            self::Slider::Tint,
                            self.slider_value(self::Slider::Tint),
                            match self.surface.base {
                                Some(_) => format!("{}% toward {}", (self.surface.tint * 100.0).round(), hex(self.paper())),
                                None => "pick a base first".into(),
                            },
                        ),
                    ),
                    (
                        "OPACITY".into(),
                        Slider(
                            self::Slider::Opacity,
                            self.slider_value(self::Slider::Opacity),
                            if translucent {
                                format!("{}%", (self.surface.opacity * 100.0).round())
                            } else {
                                "opaque swapchain on this compositor · v1".into()
                            },
                        ),
                    ),
                    (
                        "".into(),
                        Choice(vec![
                            ("PANES".into(), Hit::OpacityOn(OpacityOn::Panes), self.surface.opacity_on == OpacityOn::Panes),
                            ("CHROME TOO".into(), Hit::OpacityOn(OpacityOn::Chrome), self.surface.opacity_on == OpacityOn::Chrome),
                            ("WHOLE WINDOW".into(), Hit::OpacityOn(OpacityOn::Window), self.surface.opacity_on == OpacityOn::Window),
                        ]),
                    ),
                    (
                        "TEXTURE".into(),
                        Choice(TextureKind::ALL.iter().map(|&k| (k.name().caps(), Hit::TexKind(k), k == self.surface.texture_kind)).collect()),
                    ),
                    (
                        "STRENGTH".into(),
                        Slider(self::Slider::Texture, self.slider_value(self::Slider::Texture), format!("{}%", (self.surface.texture * 100.0).round())),
                    ),
                    (
                        "SCALE".into(),
                        Slider(self::Slider::TexScale, self.slider_value(self::Slider::TexScale), format!("{}px pitch", self.surface.texture_scale)),
                    ),
                    (
                        "ON".into(),
                        Choice(vec![
                            ("CARAPACE".into(), Hit::TexOn(TextureOn::Carapace), self.surface.texture_on == TextureOn::Carapace),
                            ("CHROME".into(), Hit::TexOn(TextureOn::Chrome), self.surface.texture_on == TextureOn::Chrome),
                            ("PANES".into(), Hit::TexOn(TextureOn::Panes), self.surface.texture_on == TextureOn::Panes),
                        ]),
                    ),
                    (
                        "MOTION".into(),
                        Choice(vec![
                            ("STILL".into(), Hit::TexMotion(false), !self.surface.texture_motion),
                            ("ANIMATED · GRAIN FLICKERS, PATTERNS DRIFT".into(), Hit::TexMotion(true), self.surface.texture_motion),
                        ]),
                    ),
                    (
                        "CARAPACE".into(),
                        Choice(Shell::ALL.iter().map(|&s| (s.name().caps(), Hit::Shell(s), s == self.surface.shell)).collect()),
                    ),
                    (
                        "WIDTH".into(),
                        Slider(self::Slider::ShellWidth, self.slider_value(self::Slider::ShellWidth), format!("{}px", self.surface.shell_width)),
                    ),
                    (
                        "RADIUS".into(),
                        Slider(self::Slider::Radius, self.slider_value(self::Slider::Radius), format!("{}px corners", self.surface.shell_radius)),
                    ),
                    (
                        "ANGLE".into(),
                        Slider(self::Slider::Angle, self.slider_value(self::Slider::Angle), format!("{}° · gradient and aurora", self.surface.angle)),
                    ),
                    (
                        "DRIFT".into(),
                        Slider(self::Slider::Drift, self.slider_value(self::Slider::Drift), format!("{} turns/s · aurora", self.surface.drift)),
                    ),
                    (
                        "BREATH".into(),
                        Slider(self::Slider::Breath, self.slider_value(self::Slider::Breath), format!("{}% · the aurora stroke swells", (self.surface.breath * 100.0).round())),
                    ),
                ]);
                v
            }
                    LOOK_TOKENS => {
                use crate::theme_edit::{contrast, grade, Family};
                let t = self.theme.clone();
                let ink_mode = t.mode == nus_render::Mode::Ink;
                let papers: &[u32] = if ink_mode { &[0x141414, 0x0f0f0f, 0x1b1a1a, 0x1c1b19, 0x1e2126, 0x16253a, 0x201c1c, 0x0d1117] } else { &[0xffffff, 0xfcfcfa, 0xf4f1ea, 0xfffdf7, 0xf7f3e8, 0xece7da, 0xfbf1c7, 0xfdf6e3] };
                let inks: &[u32] = if ink_mode { &[0xece7da, 0xf4f1ea, 0xffffff, 0xd8d2c4, 0xe6e1d3, 0xcdd6f4, 0xa89984, 0x93a1a1] } else { &[0x141414, 0x000000, 0x2b2a27, 0x3c3836, 0x073642, 0x1c1b19, 0x3b4252, 0x4a4740] };
                let pages: &[u32] = &[0xffffff, 0xf4f1ea, 0xfdf6e3, 0x141414, 0x1b1a1a, 0x0f0f0f];
                let sw = |list: &[u32], cur: Color, mk: fn(Color) -> Hit| -> Vec<(Option<Color>, Hit, bool)> {
                    list.iter().map(|&v| { let c = nus_render::theme::hex(v); (Some(c), mk(c), (c[0] - cur[0]).abs() < 0.004 && (c[1] - cur[1]).abs() < 0.004 && (c[2] - cur[2]).abs() < 0.004) }).collect()
                };
                let ansi: Vec<Color> = (0..16).map(|i| crate::theme_edit::from_rgb(t.ansi[i])).collect();
                let edit = if ink_mode { self.theme_edit.ink.clone() } else { self.theme_edit.paper.clone() };
                // What a caret or a selection might be: the ink, the signal, the brights.
                let marks: Vec<Color> = std::iter::once(t.ink).chain(std::iter::once(self.surface.signal)).chain(surface::family(self.surface.signal)).chain((9..16).map(|i| ansi[i])).collect();
                let sel = self.ansi_sel.min(15);
                let row = |from: usize| -> Vec<(Option<Color>, Hit, bool)> { (from..from + 8).map(|i| (Some(ansi[i]), Hit::AnsiSel(i), i == sel)).collect() };
                let mut cands: Vec<(Option<Color>, Hit, bool)> = Vec::new();
                for c in nus_render::theme::signal::ALL {
                    for f in surface::family(c) {
                        cands.push((Some(f), Hit::AnsiSet(f), false));
                    }
                }
                cands.truncate(24);
                let c_ink = contrast(t.ink, t.paper);
                let c_dim = contrast(t.dim, t.paper);
                let c_sig = contrast(self.surface.signal, t.paper);
                let imports = crate::theme_edit::imports();
                let mut import_chips: Vec<(String, Hit, bool)> = imports.iter().enumerate().map(|(k, i)| (format!("{} · {}", i.name, i.format).caps(), Hit::Import(k), false)).collect();
                if import_chips.is_empty() {
                    import_chips.push(("DROP GHOSTTY · WINDOWS TERMINAL · VS CODE · BASE16 FILES INTO PROFILE/THEMES".into(), Hit::OpenThemes, false));
                }
                let proof: Vec<(Color, String)> = vec![
                    (ansi[2], "seb@nus".into()), (t.ink, ":".into()), (ansi[4], "~/nus".into()), (t.ink, "$ cargo test  ".into()),
                    (ansi[3], "warning".into()), (t.ink, ": unused  ".into()), (ansi[2], "ok".into()), (t.ink, " 18 passed ".into()),
                    (ansi[1], "0 failed  ".into()), (ansi[5], "> ".into()), (ansi[6], "git".into()), (t.ink, " log  ".into()), (t.dim, "9058fca".into()),
                ];
                let brights: Vec<(Color, String)> = (8..16).map(|i| (ansi[i], format!("{i} "))).collect();
                let _ = (&sw, &row, &cands);
                let hx = surface::hex;
                let tiles: Vec<(String, Option<Color>, String, Hit, bool)> = vec![
                    ("PAPER".into(), Some(t.paper), hx(t.paper), Hit::TokSel(TokSel::Paper), self.tok_sel == TokSel::Paper),
                    ("INK".into(), Some(t.ink), hx(t.ink), Hit::TokSel(TokSel::Ink), self.tok_sel == TokSel::Ink),
                    ("PAGE".into(), Some(t.page), hx(t.page), Hit::TokSel(TokSel::Page), self.tok_sel == TokSel::Page),
                    ("DIM".into(), Some(t.dim), format!("{} · derived", hx(t.dim)), Hit::TokSel(self.tok_sel), false),
                    ("CARET".into(), Some(t.caret), if edit.caret.is_some() { hx(t.caret) } else { format!("{} · the ink", hx(t.caret)) }, Hit::TokSel(TokSel::Caret), self.tok_sel == TokSel::Caret),
                    ("SELECTION".into(), Some(nus_render::Theme::with_alpha(t.selection, 1.0)), if edit.selection.is_some() { format!("{} · at 22%", hx(t.selection)) } else { format!("{} · the ink at 22%", hx(t.selection)) }, Hit::TokSel(TokSel::Selection), self.tok_sel == TokSel::Selection),
                ];
                let ansi_tiles: Vec<(String, Option<Color>, String, Hit, bool)> = (0..16).map(|i| (format!("{i}"), Some(ansi[i]), hx(ansi[i]), Hit::TokSel(TokSel::Ansi(i)), self.tok_sel == TokSel::Ansi(i))).collect();
                let editing_tok = matches!(self.tok_sel, TokSel::Paper | TokSel::Ink | TokSel::Page | TokSel::Caret | TokSel::Selection);
                let editing_ansi = matches!(self.tok_sel, TokSel::Ansi(_));
                let cur = self.tok_color();
                let tray: Vec<(String, Option<Color>, String, Hit, bool)> = match self.tok_sel {
                    TokSel::Paper => papers.iter().map(|&v| { let c = nus_render::theme::hex(v); (String::new(), Some(c), hx(c), Hit::TokSet(c), c == cur) }).collect(),
                    TokSel::Ink => inks.iter().map(|&v| { let c = nus_render::theme::hex(v); (String::new(), Some(c), hx(c), Hit::TokSet(c), c == cur) }).collect(),
                    TokSel::Page => pages.iter().map(|&v| { let c = nus_render::theme::hex(v); (String::new(), Some(c), hx(c), Hit::TokSet(c), c == cur) }).collect(),
                    TokSel::Caret | TokSel::Selection => marks.iter().map(|&c| (String::new(), Some(c), hx(c), Hit::TokSet(c), c == cur)).collect(),
                    TokSel::Ansi(_) => cands.iter().map(|(c, _, _)| { let c = c.unwrap(); (String::new(), Some(c), hx(c), Hit::TokSet(c), c == cur) }).collect(),
                    _ => Vec::new(),
                };
                let mut v: Vec<(String, Control)> = vec![
                    (format!("{} TOKENS", if ink_mode { "INK" } else { "PAPER" }), Tokens(tiles, true)),
                ];
                let picker = |v: &mut Vec<(String, Control)>, what: String, me: &Self| {
                    let (h, sa, l) = surface::to_hsl(me.tok_color());
                    v.push((format!("{} HUE", what.caps()), Slider(self::Slider::Hue, me.slider_value(self::Slider::Hue), format!("{}°", (h * 360.0).round()))));
                    v.push(("SATURATION".into(), Slider(self::Slider::Sat, me.slider_value(self::Slider::Sat), format!("{}%", (sa * 100.0).round()))));
                    v.push(("LIGHTNESS".into(), Slider(self::Slider::Light, me.slider_value(self::Slider::Light), format!("{}% · {}", (l * 100.0).round(), hx(me.tok_color())))));
                };
                if editing_tok {
                    picker(&mut v, format!("{:?}", self.tok_sel), self);
                    v.push(("TRAY".into(), Tokens(tray.clone(), false)));
                    match self.tok_sel {
                        TokSel::Caret if edit.caret.is_some() => v.push(("".into(), Choice(vec![("FOLLOW THE INK".into(), Hit::TokCaret(None), false)]))),
                        TokSel::Selection if edit.selection.is_some() => v.push(("".into(), Choice(vec![("FOLLOW THE INK".into(), Hit::TokSelection(None), false)]))),
                        _ => {}
                    }
                }
                v.push(("CONTRAST".into(), Info(format!("ink on paper {:.1}:1 {} · dim {:.1}:1 {} · signal {:.1}:1 {}", c_ink, grade(c_ink), c_dim, grade(c_dim), c_sig, grade(c_sig)))));
                v.push(("".into(), Info(format!("dim, tint and hot follow paper and ink · editing the {} theme; TYPE & MOTION switches", if ink_mode { "ink" } else { "paper" }))));
                v.push(("".into(), Buttons(vec![("RESET TOKENS".into(), icons::WARNING, Hit::TokReset)])));
                v.push(("ANSI".into(), Tokens(ansi_tiles, false)));
                if editing_ansi {
                    picker(&mut v, format!("ansi {sel}"), self);
                    v.push(("TRAY".into(), Tokens(tray, false)));
                }
                v.extend(vec![
                    ("FAMILY".into(), Choice(Family::ALL.iter().map(|&f| (f.name().caps(), Hit::Family(f), f == self.theme_edit.family)).chain(std::iter::once(("IMPORTED".to_string(), Hit::Family(Family::Imported), self.theme_edit.family == Family::Imported))).collect())),
                    ("SATURATION".into(), Slider(self::Slider::Saturation, self.slider_value(self::Slider::Saturation), format!("{}%", (self.theme_edit.saturation * 100.0).round()))),
                    ("PROOF".into(), Proof(proof)),
                    ("BRIGHTS".into(), Proof(brights)),
                    ("IMPORT".into(), Choice(import_chips)),
                    ("".into(), Buttons(vec![("OPEN THEMES FOLDER".into(), icons::FOLDER, Hit::OpenThemes)])),
                ]);
                v
            }
                    LOOK_APP_ICON => vec![
                        ("APP ICON".into(), Info(if crate::mercury::can_claim() {
                            "Choose your running app’s icon. Mercury, a silver n for the first edition, is yours for the claiming until 2027: click its tile. Choosing another icon later keeps it. The installed package icon remains unchanged."
                        } else {
                            "Choose your running app’s icon. Automatic uses Mercury when you have it; choosing another icon keeps it. The installed package icon remains unchanged."
                        }.into())),
                        ("".into(),AppIcons),
                    ].into_iter().chain(crate::mercury::earned().then(|| ("MERCURY".into(), Mercury))).collect(),
                    LOOK_TYPE => vec![
                (
                    "THEME".into(),
                    Choice(vec![
                        ("FOLLOW OS".into(), Hit::Theme(None), self.behavior.follow_os_theme),
                        ("PAPER".into(), Hit::Theme(Some(false)), !self.behavior.follow_os_theme && !ink),
                        ("INK".into(), Hit::Theme(Some(true)), !self.behavior.follow_os_theme && ink),
                    ]),
                ),
                (
                    "MOTION".into(),
                    Slider(
                        self::Slider::Motion,
                        self.slider_value(self::Slider::Motion),
                        format!("{} · snappy ← → cinematic · sidebar {}ms", self.motion.name(), (self.motion.dur(crate::anim::base::SIDEBAR) * 1000.0).round()),
                    ),
                ),
                (
                    "REDUCE MOTION".into(),
                    Choice(vec![
                        (format!("FOLLOW OS · {}", if crate::anim::os_reduce_motion() { "ON" } else { "OFF" }), Hit::Reduce(None), self.motion.reduce.is_none()),
                        ("OFF".into(), Hit::Reduce(Some(false)), self.motion.reduce == Some(false)),
                        ("ON".into(), Hit::Reduce(Some(true)), self.motion.reduce == Some(true)),
                    ]),
                ),
                ("INTERFACE FONT".into(), Choice(crate::fonts::Family::ALL.iter().map(|&f|(f.name().into(),Hit::UiFont(f),self.behavior.ui_font==f)).collect())),
                ("INTERFACE WEIGHT".into(), Choice(crate::fonts::Weight::ALL.iter().map(|&w|(w.name().into(),Hit::UiWeight(w),self.behavior.ui_weight==w)).collect())),
                ("TERMINAL FONT".into(), Choice(crate::fonts::Family::MONO.iter().map(|&f|(f.name().into(),Hit::TermFont(f),self.behavior.term_font==f)).collect())),
                ("TERMINAL WEIGHT".into(), Choice(crate::fonts::Weight::ALL.iter().map(|&w|(w.name().into(),Hit::TermWeight(w),self.behavior.term_weight==w)).collect())),
                ("".into(), Info("Bundled fonts work without installation. Interface and terminal weights change independently. Terminal choices use fixed-width families so columns stay aligned.".into())),
                ("WORDMARK".into(), Info("Newsreader Italic".into())),
            ],
                    _ => {
                let c = &self.cursor;
                vec![
                    (
                        "SHAPE".into(),
                        Choice(vec![
                            ("THE SHELL'S".into(), Hit::CurShape(CursorShapePref::Shell), c.shape == CursorShapePref::Shell),
                            ("BLOCK".into(), Hit::CurShape(CursorShapePref::Block), c.shape == CursorShapePref::Block),
                            ("BEAM".into(), Hit::CurShape(CursorShapePref::Beam), c.shape == CursorShapePref::Beam),
                            ("UNDERLINE".into(), Hit::CurShape(CursorShapePref::Underline), c.shape == CursorShapePref::Underline),
                        ]),
                    ),
                    (
                        "UNFOCUSED".into(),
                        Choice(vec![("HOLLOW".into(), Hit::CurHollow(true), c.hollow_unfocused), ("HIDDEN".into(), Hit::CurHollow(false), !c.hollow_unfocused)]),
                    ),
                    (
                        "BLINK".into(),
                        Choice(vec![
                            ("NEVER".into(), Hit::CurBlink(Blink::Never), c.blink == Blink::Never),
                            ("AFTER 2S IDLE".into(), Hit::CurBlink(Blink::AfterIdle), c.blink == Blink::AfterIdle),
                            ("ALWAYS".into(), Hit::CurBlink(Blink::Always), c.blink == Blink::Always),
                        ]),
                    ),
                    ("PERIOD".into(), Slider(self::Slider::BlinkPeriod, self.slider_value(self::Slider::BlinkPeriod), format!("{}ms", c.period))),
                    (
                        "COLOR".into(),
                        Choice(vec![
                            ("THE THEME'S CARET".into(), Hit::CurColor(CursorColor::Theme), c.color == CursorColor::Theme),
                            ("SIGNAL".into(), Hit::CurColor(CursorColor::Signal), c.color == CursorColor::Signal),
                            ("THE TAB'S OWN".into(), Hit::CurColor(CursorColor::Tab), c.color == CursorColor::Tab),
                        ]),
                    ),
                    ("".into(), Info("the theme's caret is a token — LOOK · TOKENS sets it, the ink unless a theme says; the selection wash is a token there too · text under a block cursor inverts".into())),
                    (
                        "MOTION".into(),
                        Choice(vec![
                            ("JUMP".into(), Hit::CurMotion(CursorMotion::Jump), c.motion == CursorMotion::Jump),
                            ("GLIDE".into(), Hit::CurMotion(CursorMotion::Glide), c.motion == CursorMotion::Glide),
                            ("COMET".into(), Hit::CurMotion(CursorMotion::Comet), c.motion == CursorMotion::Comet),
                            ("SMEAR".into(), Hit::CurMotion(CursorMotion::Smear), c.motion == CursorMotion::Smear),
                        ]),
                    ),
                    ("".into(), Info("glide eases between cells on the motion register; comet leaves a short ink trail; smear stretches the body the way neovide does".into())),
                    ("TRAIL".into(), Slider(self::Slider::Smear, self.slider_value(self::Slider::Smear), format!("{:.0}% · neovide's trail_size: how far the tail lags the head (smear only)", c.smear * 100.0))),
                    ("WEIGHT".into(), Slider(self::Slider::CurWeight, self.slider_value(self::Slider::CurWeight), format!("{}px · beam and underline", c.weight))),
                    (
                        "WHILE TYPING".into(),
                        Choice(vec![("HIDE THE POINTER".into(), Hit::CurHide(true), c.hide_while_typing), ("KEEP IT".into(), Hit::CurHide(false), !c.hide_while_typing)]),
                    ),
                ]
            }
                };
                v.extend(rest);
                v
            }
            1 => {
                let on = self.sound.prefs.enabled;
                let mut rows: Vec<(String, Control)> = vec![
                    (
                        "APP SOUNDS".into(),
                        Choice(vec![("ON".into(), Hit::SoundOn(true), on), ("OFF".into(), Hit::SoundOn(false), !on)]),
                    ),
                    (
                        "VOLUME".into(),
                        Slider(self::Slider::Volume, self.slider_value(self::Slider::Volume), format!("{}%", (self.sound.prefs.volume * 100.0).round())),
                    ),
                    (
                        "".into(),
                        Info(if self.sound.player.is_some() { "Seventeen short sounds, made on this device. Click any name below to hear it.".into() } else { "No audio output device found, so nothing will play.".into() }),
                    ),
                ];
                // The palette, in rows of six.
                for chunk in (0..crate::sound::NAMES.len()).collect::<Vec<_>>().chunks(6) {
                    rows.push((
                        if chunk[0] == 0 { "HEAR THEM".into() } else { "".into() },
                        Choice(chunk.iter().map(|&i| (crate::sound::NAMES[i].caps(), Hit::Play(i), false)).collect()),
                    ));
                }
                rows.push(("WHAT PLAYS WHEN".into(), Section));
                rows.push(("".into(), Info("For each moment: the speaker button silences it, the sound's name plays it, and ▸ tries the next sound.".into())));
                for (e, (ev, _, note)) in crate::sound::EVENTS.iter().enumerate() {
                    let cur = self.sound.prefs.cue_for(ev);
                    let ci = cur.as_deref().and_then(|c| crate::sound::NAMES.iter().position(|n| *n == c));
                    rows.push((ev.replace('.', " · ").caps(), Cue(e, ci, note.to_string())));
                }
                rows.push(("".into(), Info("Rules can change any of these with on_event. Sounds by Daniel Belyi (cuelume, MIT).".into())));
                rows
            }
            2 => {
                let b = &self.behavior;
                let launch_cue = self.sound.prefs.cue_for("launch");
                let sound_chips: Vec<(String, Hit, bool)> = {
                    let ev = crate::sound::EVENTS.iter().position(|(e, _, _)| *e == "launch").unwrap_or(0);
                    let mut v = vec![("OFF".into(), Hit::EventCue(ev, usize::MAX), launch_cue.is_none() || !b.startup_sound)];
                    for name in ["arrival", "chime", "bloom", "ready"] {
                        let ci = crate::sound::NAMES.iter().position(|n| *n == name).unwrap_or(0);
                        v.push((name.caps(), Hit::EventCue(ev, ci), b.startup_sound && launch_cue.as_deref() == Some(name)));
                    }
                    v
                };
                let layouts: Vec<String> = crate::layout_file::saved().into_iter().map(|(n, _)| n).collect();
                let new_tab = key("T", !cfg!(target_os = "macos"));
                let home_host = crate::links::host(&b.home_url);
                vec![
                    (
                        "WINDOW AT LAUNCH".into(),
                        Pics(vec![
                            ("LAST SIZE & PLACE".into(), "where you left it".into(), Pic::WinLast, Hit::WindowStart(WindowStart::Last), b.window_start == WindowStart::Last),
                            ("MAXIMIZED".into(), "fills the screen; the system bar stays".into(), Pic::WinMax, Hit::WindowStart(WindowStart::Maximized), b.window_start == WindowStart::Maximized),
                            ("FULLSCREEN".into(), "the whole screen, nothing else".into(), Pic::WinFull, Hit::WindowStart(WindowStart::Fullscreen), b.window_start == WindowStart::Fullscreen),
                            ("CENTERED".into(), "1440 × 900, in the middle".into(), Pic::WinCentered, Hit::WindowStart(WindowStart::Centered), b.window_start == WindowStart::Centered),
                        ]),
                    ),
                    (
                        "LAUNCH ANIMATION".into(),
                        Pics(vec![
                            ("DRAWS IN".into(), "the nus logo animates before opening".into(), Pic::SplashDraw, Hit::Splash(SplashMode::Draw), b.splash == SplashMode::Draw),
                            ("STILL".into(), "the icon, no motion".into(), Pic::SplashStill, Hit::Splash(SplashMode::Still), b.splash == SplashMode::Still),
                            ("NONE".into(), "straight to your start page".into(), Pic::SplashNone, Hit::Splash(SplashMode::None), b.splash == SplashMode::None),
                        ]),
                    ),
                    (
                        "ANIMATION DURATION".into(),
                        Slider(self::Slider::SplashHold, self.slider_value(self::Slider::SplashHold), format!("{:.1} seconds", b.splash_hold)),
                    ),
                    ("LAUNCH SOUND".into(), Choice(sound_chips)),
                    (
                        "ON QUIT, KEEP".into(),
                        Choice(vec![
                            ("TABS AND WINDOWS".into(), Hit::Remember(true), b.remember),
                            ("NOTHING".into(), Hit::Remember(false), !b.remember),
                        ]),
                    ),
                    ("".into(), Info("Kept tabs and windows come back at launch, and the start page below opens over them. Saved sessions are also in the atlas (the planet button).".into())),
                    (
                        "SESSION PICKER".into(),
                        Choice(vec![
                            ("WHEN I OPEN IT".into(), Hit::Atlas(AtlasMode::Planet), b.atlas == AtlasMode::Planet),
                            ("SHOW AT LAUNCH".into(), Hit::Atlas(AtlasMode::AtLaunch), b.atlas == AtlasMode::AtLaunch),
                            ("KEEP OPEN UNTIL I PICK".into(), Hit::Atlas(AtlasMode::Persistent), b.atlas == AtlasMode::Persistent),
                        ]),
                    ),
                    (
                        "OPEN AT LOGIN".into(),
                        Choice(vec![
                            ("YES".into(), Hit::LoginItem(true), crate::little::login_item_registered()),
                            ("NO".into(), Hit::LoginItem(false), !crate::little::login_item_registered()),
                        ]),
                    ),
                    ("".into(), Info(if self.login_note.is_empty() { "nus starts when you log in · change it any time".into() } else { self.login_note.clone() })),
                    (
                        "TERMINAL OR BROWSER".into(),
                        Pics(vec![
                            ("TERMINAL".into(), "shells first in the prompt palette".into(), Pic::LeadTerminal, Hit::Lead(Lead::Terminal), b.lead == Lead::Terminal),
                            ("BROWSER".into(), "pages first in the prompt palette".into(), Pic::LeadBrowser, Hit::Lead(Lead::Browser), b.lead == Lead::Browser),
                        ]),
                    ),
                    ("".into(), Info("Sets the order of palette suggestions and what an empty prompt opens. Also sets where links from other apps open; you can change that below.".into())),
                    (
                        "START PAGE".into(),
                        Pics(vec![
                            ("PALETTE ONLY".into(), "open the command palette · no extra tab".into(), Pic::StartPrompt, Hit::Then(Then::Palette), b.then == Then::Palette),
                            ("HOME · PROMPT".into(), "one line: type a URL, a command or a folder".into(), Pic::StartPrompt, Hit::Then(Then::Prompt), b.then == Then::Prompt),
                            ("WEBSITE".into(), format!("open {home_host}"), Pic::StartHome, Hit::Then(Then::HomePage), b.then == Then::HomePage),
                            (
                                "CUSTOM LAYOUT".into(),
                                if layouts.is_empty() { "your saved tabs and panes · none saved yet".into() } else if b.then_layout.is_empty() { format!("your saved tabs and panes · {}", layouts[0]) } else { format!("your saved tabs and panes · {}", b.then_layout) },
                                Pic::StartLayout,
                                Hit::Then(Then::Layout),
                                b.then == Then::Layout,
                            ),
                            ("THE LAST PAGE".into(), "reopen your most recent web page".into(), Pic::StartLast, Hit::Then(Then::LastPage), b.then == Then::LastPage),
                        ]),
                    ),
                    ("".into(), Info(format!("Opens at launch and with {new_tab} or New tab. A custom layout adds its saved tabs and panes. With no saved page or usable layout, the prompt palette opens."))),
                    ("HOME ADDRESS".into(), Buttons(vec![("EDIT ADDRESS".into(), icons::PENCIL, Hit::EditHomeUrl)])),
                    ("".into(), Info(b.home_url.clone())),
                    (
                        "SAVED LAYOUT".into(),
                        if layouts.is_empty() {
                            Info("No layouts saved yet. Use Save current layout below to keep these tabs and panes.".into())
                        } else {
                            Choice(layouts.iter().enumerate().map(|(i, name)| (name.caps(), Hit::StartupLayout(i), b.then == Then::Layout && (*name == b.then_layout || (b.then_layout.is_empty() && i == 0)))).collect())
                        },
                    ),
                    (
                        "SAVE CURRENT LAYOUT".into(),
                        Choice(vec![
                            ("SET FROM THIS WINDOW".into(), Hit::SetLaunchTabs, false),
                            (if layouts.iter().any(|n| n == "launch") { "CLEAR".into() } else { "NONE SET".into() }, Hit::ClearLaunchTabs, false),
                        ]),
                    ),
                    ("".into(), Info("Saves these tabs and panes as the layout “launch” and selects Custom layout above.".into())),
                    ("HOME BACKGROUND".into(), Caption),
                    ("".into(), Info("Applies to every Home prompt, including new tabs and startup when Home · Prompt is selected.".into())),
                    (
                        "MINIMAL".into(),
                        Pics(vec![
                            ("PROMPT ONLY".into(), "a clean background for typing".into(), Pic::LookLine, Hit::HomeLook(HomeLook::Line), b.home_look == HomeLook::Line),
                            ("LOGO & PROMPT".into(), "the nus logo and your recent places".into(), Pic::LookPlate, Hit::HomeLook(HomeLook::Plate), b.home_look == HomeLook::Plate),
                        ]),
                    ),
                    (
                        "ART".into(),
                        Art(crate::art::list().into_iter().enumerate().map(|(i, a)| (a.key.clone(), a.name, if a.path.is_none() { match a.key.as_str() {
                            "pond" => "koi swimming behind the prompt".into(),
                            "memphis" => "colorful shapes in motion".into(),
                            "space" => "constellations · location optional".into(),
                            "sky" => "sun and clouds · location optional".into(),
                            "brain" => "a live view of running processes".into(),
                            _ => a.says,
                        } } else { a.says }, Hit::HomeArt(i), b.home_look == HomeLook::Art && b.home_art == a.key, a.path.is_none())).collect()),
                    ),
                    (
                        "".into(),
                        Actions(vec![
                            ("ADD YOUR OWN".into(), "create artwork in the editor".into(), icons::PLUS, Hit::AddArt),
                            ("ASK FOR ONE".into(), "ask your assistant to create artwork".into(), icons::ASSISTANT, Hit::AskArt),
                            ("OPEN FOLDER".into(), "browse your saved artwork files".into(), icons::FOLDER, Hit::OpenArtFolder),
                        ]),
                    ),
                    ("".into(), Info("Built-in artwork includes editable examples. Your saved changes appear in the preview automatically.".into())),
                    (
                        "YOUR LOCATION".into(),
                        Choice(vec![(
                            match b.place {
                                Some([lat, lon]) => format!("{:.1}° {} · {:.1}° {}", lat.abs(), if lat >= 0.0 { "N" } else { "S" }, lon.abs(), if lon >= 0.0 { "E" } else { "W" }),
                                None => "NOT SET · CHOOSE LOCATION".into()
                            },
                            Hit::PlaceEdit,
                            b.place.is_some(),
                        )]),
                    ),
                    ("".into(), Info("Sky and Space show illustrated skies by default. Add latitude and longitude to show your local sky. Location stays on this machine and is never inferred. Clear it any time.".into())),
                    (
                        key("N", false),
                        Pics(vec![
                            ("PROMPT PALETTE".into(), "start at the prompt and choose a folder".into(), Pic::NewPrompt, Hit::NewWindow(NewWindow::Prompt), b.new_window == NewWindow::Prompt),
                            ("SHELL".into(), "a terminal in the current window’s folder".into(), Pic::NewShell, Hit::NewWindow(NewWindow::Shell), b.new_window == NewWindow::Shell),
                            ("SAME AS LAUNCH".into(), "use the animation and start page above".into(), Pic::NewLaunch, Hit::NewWindow(NewWindow::Launch), b.new_window == NewWindow::Launch),
                        ]),
                    ),
                    ("".into(), Info("Each new window has its own tabs and working folder.".into())),
                    (
                        "LINKS FROM OTHER APPS".into(),
                        Choice(vec![
                            ("LITTLE WINDOW".into(), Hit::Outside(Outside::Little), b.outside == Outside::Little),
                            ("NEW TAB HERE".into(), Hit::Outside(Outside::NewTab), b.outside == Outside::NewTab),
                        ]),
                    ),
                ]
            }
            3 => vec![
                ("SIZE & PLACE".into(), Section),
                ("WIDTH".into(), Slider(self::Slider::SidebarWidth, self.slider_value(self::Slider::SidebarWidth), format!("{} px · you can also drag the sidebar's edge", self.sidebar_rules.width as u32))),
                ("SIDE OF THE WINDOW".into(), Choice(vec![
                    ("LEFT".into(), Hit::Side(Side::Left), self.sidebar_rules.side == Side::Left),
                    ("RIGHT".into(), Hit::Side(Side::Right), self.sidebar_rules.side == Side::Right),
                ])),
                ("SIZE".into(), Choice(vec![
                    ("FULL · ICONS AND TITLES".into(), Hit::Compact(false), !self.sidebar_rules.compact),
                    ("COMPACT · ICONS ONLY".into(), Hit::Compact(true), self.sidebar_rules.compact),
                ])),
                ("ASSISTANTS IN TAB ROWS".into(), Choice(vec![
                    ("STATUS, QUESTION & ANSWERS".into(), Hit::Ledger(true), self.behavior.ledger),
                    ("A DOT".into(), Hit::Ledger(false), !self.behavior.ledger),
                ])),
                ("".into(), Info("Claude, Codex and other assistants show what they are doing under their tab: working, waiting for you with the question, or done. Answer a permission here or in the terminal; both stay in step. Settings · Assistants connects their hooks for the full picture.".into())),
                ("WHEN IT'S NARROW, SHOW".into(), Choice(vec![
                    ("TAB-TYPE ICONS".into(), Hit::SmallTabs(crate::sidebar::SmallTabs::Icons), self.sidebar_rules.small_tabs == crate::sidebar::SmallTabs::Icons),
                    ("SITE ICONS".into(), Hit::SmallTabs(crate::sidebar::SmallTabs::Favicons), self.sidebar_rules.small_tabs == crate::sidebar::SmallTabs::Favicons),
                    ("PAGE THUMBNAILS".into(), Hit::SmallTabs(crate::sidebar::SmallTabs::Preview), self.sidebar_rules.small_tabs == crate::sidebar::SmallTabs::Preview),
                ])),
                ("".into(), Info("Below about 105 px wide, and in compact size, titles give way to these. Hover a tab to read its title.".into())),
                ("TOP OF THE SIDEBAR".into(), Section),
                ("STYLE".into(), Choice(vec![
                    ("TITLE BAR".into(), Hit::HdrStyle(HeaderStyle::Bar), self.header.style == HeaderStyle::Bar),
                    ("WINDOW RAIL".into(), Hit::HdrStyle(HeaderStyle::Rail), self.header.style == HeaderStyle::Rail),
                ])),
                ("WINDOW NAME".into(), Choice(vec![
                    ("SMALL CAPITALS".into(), Hit::HdrMasthead(false), !self.header.masthead),
                    ("LARGE SERIF".into(), Hit::HdrMasthead(true), self.header.masthead),
                ])),
                ("NAME BESIDE THE WINDOW SQUARE".into(), Choice(vec![("SHOW".into(), Hit::HdrName(true), self.header.show_name), ("HIDE".into(), Hit::HdrName(false), !self.header.show_name)])),
                ("FOLDER, TAB & PORT COUNTS".into(), Choice(vec![("SHOW".into(), Hit::HdrDateline(true), self.header.dateline), ("HIDE".into(), Hit::HdrDateline(false), !self.header.dateline)])),
                ("WINDOW RAIL".into(), Choice(vec![("ALWAYS VISIBLE".into(), Hit::HdrRailHover(false), !self.header.rail_hover), ("ONLY ON HOVER".into(), Hit::HdrRailHover(true), self.header.rail_hover)])),
                ("NEW TAB BUTTON".into(), Section),
                ("+ IN THE TOP ROW".into(), Choice(vec![("SHOW".into(), Hit::HdrButton(true), self.header.header_button), ("HIDE".into(), Hit::HdrButton(false), !self.header.header_button)])),
                ("“NEW TAB” AFTER THE LAST TAB".into(), Choice(vec![("SHOW".into(), Hit::HdrNextRow(true), self.header.next_row), ("HIDE".into(), Hit::HdrNextRow(false), !self.header.next_row)])),
                ("ARROW FOR OTHER TAB TYPES".into(), Choice(vec![("SHOW".into(), Hit::HdrCaret(true), self.header.kinds_caret), ("HIDE".into(), Hit::HdrCaret(false), !self.header.kinds_caret)])),
                ("FLASH WHEN PRESSED".into(), Choice(vec![("ON".into(), Hit::HdrFlash(true), self.header.flash), ("OFF".into(), Hit::HdrFlash(false), !self.header.flash)])),
                ("".into(), Info("Either button opens the start page chosen in Start/New Tab. Hold it, or right-click, to pick a different kind of tab.".into())),
                ("PINNED TILES".into(), Section),
                ("TILES SHOW".into(), Choice(vec![
                    ("AN ICON".into(), Hit::PinDisplay(crate::pins::Display::Icon), self.sidebar_rules.pin_display == crate::pins::Display::Icon),
                    ("THE LIVE PAGE".into(), Hit::PinDisplay(crate::pins::Display::Preview), self.sidebar_rules.pin_display == crate::pins::Display::Preview),
                ])),
                ("SHOWING & HIDING".into(), Section),
                (format!("RIGHT NOW · {}", key("S", true)), Choice(vec![
                    ("ALWAYS VISIBLE".into(), Hit::Pin(true), self.sidebar),
                    ("SLIDES IN ON HOVER".into(), Hit::Pin(false), !self.sidebar),
                ])),
                ("HOVER STARTS AT".into(), Choice(vec![
                    ("THE SCREEN EDGE".into(), Hit::HoverFrom(HoverFrom::ScreenEdge), self.sidebar_rules.hover_from == HoverFrom::ScreenEdge),
                    ("THIS WINDOW'S EDGE".into(), Hit::HoverFrom(HoverFrom::InsideWindow), self.sidebar_rules.hover_from == HoverFrom::InsideWindow),
                ])),
                ("WAIT BEFORE HIDING".into(), Slider(self::Slider::Grace, self.slider_value(self::Slider::Grace), format!("{} ms after the pointer leaves", self.sidebar_rules.grace_ms))),
                ("IN FULLSCREEN".into(), Choice(vec![
                    ("SLIDES IN ON HOVER".into(), Hit::Fullscreen(Fullscreen::Hover), self.sidebar_rules.fullscreen == Fullscreen::Hover),
                    ("HIDDEN".into(), Hit::Fullscreen(Fullscreen::Hidden), self.sidebar_rules.fullscreen == Fullscreen::Hidden),
                    ("ALWAYS VISIBLE".into(), Hit::Fullscreen(Fullscreen::Pinned), self.sidebar_rules.fullscreen == Fullscreen::Pinned),
                ])),
                ("FOOTER".into(), Section),
                ("BUTTON ROW HEIGHT".into(), Slider(self::Slider::FooterSize, self.slider_value(self::Slider::FooterSize), format!("{} px · you can also drag the footer's top edge", self.sidebar_rules.footer_row as u32))),
            ],
            4 => vec![
                ("WHERE NEW PAGES OPEN".into(), Section),
                ("A LINK CLICKED ON A PAGE".into(), Choice(vec![
                    ("UNDER IT, AS A STACK".into(), Hit::Links(Links::Stack), self.behavior.links == Links::Stack),
                    ("BESIDE IT".into(), Hit::Links(Links::Split), self.behavior.links == Links::Split),
                    ("IN A NEW TAB".into(), Hit::Links(Links::NewTab), self.behavior.links == Links::NewTab),
                ])),
                ("AN ADDRESS TYPED IN A SHELL".into(), Choice(vec![
                    ("BESIDE THE SHELL".into(), Hit::PromptUrl(PromptUrl::Split), self.behavior.prompt_url == PromptUrl::Split),
                    ("IN A NEW TAB".into(), Hit::PromptUrl(PromptUrl::NewTab), self.behavior.prompt_url == PromptUrl::NewTab),
                ])),
                ("A TAB FROM ANOTHER APP".into(), Choice(vec![
                    ("BEHIND, WITH A NOTICE".into(), Hit::OpenedBy(OpenedBy::Behind), self.behavior.opened_by_others == OpenedBy::Behind),
                    ("IN FRONT".into(), Hit::OpenedBy(OpenedBy::Front), self.behavior.opened_by_others == OpenedBy::Front),
                ])),
                ("".into(), Info("Covers tabs opened by other apps, nus open, assistants and rules. Tabs you open yourself always come to the front.".into())),
                ("A PAGE THAT'S ALREADY OPEN".into(), Choice(vec![
                    ("OFFER TO SWITCH".into(), Hit::Dedupe(true), self.behavior.dedupe),
                    ("OPEN ANOTHER COPY".into(), Hit::Dedupe(false), !self.behavior.dedupe),
                ])),
                ("BACK & FORWARD".into(), Section),
                ("SWIPE OVERLAY".into(), Choice(vec![
                    ("ARROW".into(), Hit::SwipeLook(SwipeLook::Arrow), self.behavior.swipe_look == SwipeLook::Arrow),
                    ("ARROW & WORDS".into(), Hit::SwipeLook(SwipeLook::Card), self.behavior.swipe_look == SwipeLook::Card),
                    ("EDGE BAND".into(), Hit::SwipeLook(SwipeLook::Edge), self.behavior.swipe_look == SwipeLook::Edge),
                    ("NONE".into(), Hit::SwipeLook(SwipeLook::Off), self.behavior.swipe_look == SwipeLook::Off),
                ])),
                ("SWIPE DISTANCE".into(), Choice(vec![
                    ("SHORT".into(), Hit::SwipeReach(120), self.behavior.swipe_reach == 120),
                    ("MEDIUM".into(), Hit::SwipeReach(180), self.behavior.swipe_reach == 180),
                    ("LONG".into(), Hit::SwipeReach(260), self.behavior.swipe_reach == 260),
                ])),
                ("".into(), Info("Two fingers sideways on a page go back or forward. The overlay fills as the swipe nears the distance, then goes signal-colored once it fires, and shows when there's nowhere to go. Back on a tab's first page closes the tab.".into())),
                ("SPLIT PANES".into(), Section),
                ("CORNER BUTTONS".into(), Choice(vec![
                    ("SHOW NEAR THE CORNER".into(), Hit::PaneControls(crate::panes::Controls::Near), self.behavior.pane_controls == crate::panes::Controls::Near),
                    ("NEVER".into(), Hit::PaneControls(crate::panes::Controls::Never), self.behavior.pane_controls == crate::panes::Controls::Never),
                ])),
                ("".into(), Info("Move a pane to its own tab, swap the two sides, show one alone, or close it. They appear when the pointer nears a pane's top corner.".into())),
                ("DIVIDER BETWEEN PANES".into(), Choice(vec![("DRAG TO RESIZE".into(), Hit::PaneDivider(true), self.behavior.pane_divider), ("FIXED".into(), Hit::PaneDivider(false), !self.behavior.pane_divider)])),
                ("IDLE PAGES".into(), Section),
                ("PAUSE A PAGE AFTER".into(), Choice(vec![("NEVER".into(), Hit::SleepAfter(0), self.behavior.sleep_after_min == 0), ("10 MIN".into(), Hit::SleepAfter(10), self.behavior.sleep_after_min == 10), ("30 MIN".into(), Hit::SleepAfter(30), self.behavior.sleep_after_min == 30), ("2 HOURS".into(), Hit::SleepAfter(120), self.behavior.sleep_after_min == 120)])),
                ("CLOSE A PAGE AFTER".into(), Choice(vec![("NEVER".into(), Hit::ArchiveAfter(0), self.behavior.archive_after_h == 0), ("12 HOURS".into(), Hit::ArchiveAfter(12), self.behavior.archive_after_h == 12), ("A DAY".into(), Hit::ArchiveAfter(24), self.behavior.archive_after_h == 24), ("A WEEK".into(), Hit::ArchiveAfter(168), self.behavior.archive_after_h == 168)])),
                ("".into(), Info("A paused page wakes when you select it. A closed page goes to Recently closed. Pinned tabs and terminals are never paused or closed.".into())),
                ("CLOSING & TIDYING".into(), Section),
                ("CLOSING A BUSY SHELL".into(), Choice(vec![
                    ("ASK FIRST".into(), Hit::CloseAsks(true), self.behavior.close_asks),
                    ("CLOSE AT ONCE".into(), Hit::CloseAsks(false), !self.behavior.close_asks),
                ])),
                ("".into(), Info("Closing a tab stops what it was running and everything it started: a dev server takes its node with it. Ask first names the command before it goes. A shell sitting at its prompt closes without asking either way.".into())),
                ("SUGGEST TAB GROUPS".into(), Choice(vec![
                    ("ONLY WHEN I ASK".into(), Hit::TidyEvery(TidyEvery::Off), self.behavior.tidy_every == TidyEvery::Off),
                    ("EVERY HOUR".into(), Hit::TidyEvery(TidyEvery::Hourly), self.behavior.tidy_every == TidyEvery::Hourly),
                    ("ONCE A DAY".into(), Hit::TidyEvery(TidyEvery::Daily), self.behavior.tidy_every == TidyEvery::Daily),
                ])),
                ("".into(), Info("Tidy finds tabs that belong together (same site, same project folder) and offers to stack or close them. Nothing moves without you. Also in the palette: tidy.".into())),
                ("GOOD TO KNOW".into(), Section),
                ("STACKS".into(), Info(format!("Related tabs can nest one level deep. {} jumps to a stack at its last-used tab. Closing a stack asks before closing what's inside.", key("1–9", false)))),
                ("SHELL COLORS".into(), Choice(vec![
                    ("RANDOM".into(), Hit::ShellTint(crate::shell_colors::ShellTint::Random), self.behavior.shell_tint == crate::shell_colors::ShellTint::Random),
                    ("BY FOLDER".into(), Hit::ShellTint(crate::shell_colors::ShellTint::Folder), self.behavior.shell_tint == crate::shell_colors::ShellTint::Folder),
                    ("NONE".into(), Hit::ShellTint(crate::shell_colors::ShellTint::None), self.behavior.shell_tint == crate::shell_colors::ShellTint::None),
                ])),
                ("".into(), Info("Each new shell, and every page opened from it, wears a color from the theme's signal family: rich on paper, tinted on ink. Random picks one no open shell is wearing; by folder gives the same project the same color every time. Change the theme and every shell follows.".into())),
                ("TAB COLORS".into(), Info("Pick a tab's color from its right-click menu, or set them by rule in Rules.".into())),
            ],
            5 => {
                let mut v: Vec<(String, Control)> = vec![(
                    "DEFAULT SHELL".into(),
                    Choice(
                        self.profiles
                            .iter()
                            .enumerate()
                            .map(|(i, p)| (p.name.caps(), Hit::DefaultProfile(i), i == self.behavior.default_profile))
                            .collect(),
                    ),
                )];
                // Shell integration: what each shell gets.
                v.insert(0, ("".into(), Info("prompt marks · cwd · exit codes · new tabs open where you are · jump to prompts · copy a command's output".into())));
                v.insert(1, (
                    "COMMAND LINE".into(),
                    Choice(vec![("HIGHLIGHT".into(), Hit::Highlight(!self.behavior.highlight), self.behavior.highlight), ("PREDICT".into(), Hit::Predict(!self.behavior.predict), self.behavior.predict)]),
                ));
                v.insert(2, ("".into(), Info("terminal-side, nothing to install: tokens colored as you type; the history entry that continues your line ghosts after the caret, Right or End accepts".into())));
                let pl = self.behavior.prompt_lsp;
                v.insert(3, (
                    "PROMPT LSP".into(),
                    Choice(vec![
                        ("QUIET".into(), Hit::PromptLsp(PromptLsp::Quiet), pl == PromptLsp::Quiet),
                        ("MENU".into(), Hit::PromptLsp(PromptLsp::Menu), pl == PromptLsp::Menu),
                        ("OFF".into(), Hit::PromptLsp(PromptLsp::Off), pl == PromptLsp::Off),
                    ]),
                ));
                v.insert(4, ("LANGUAGE SERVERS".into(), Buttons(self.lsp_tool_buttons())));
                v.insert(5, (
                    "EDITOR".into(),
                    Choice(vec![("FORMAT ON SAVE".into(), Hit::FormatOnSave(!self.behavior.format_on_save), self.behavior.format_on_save)]),
                ));
                let fo = self.behavior.fold_over;
                v.insert(6, (
                    "BLOCKS".into(),
                    Choice(vec![
                        ("LAMPS".into(), Hit::Blocks(!self.behavior.blocks), self.behavior.blocks),
                        ("FOLD · NEVER".into(), Hit::FoldOver(0), fo == 0),
                        ("OVER 50".into(), Hit::FoldOver(50), fo == 50),
                        ("OVER 200".into(), Hit::FoldOver(200), fo == 200),
                    ]),
                ));
                let lc = self.behavior.link_click;
                v.insert(7, (
                    "CLICK LINKS".into(),
                    Choice(vec![
                        ("ASK".into(), Hit::LinkClick(LinkClick::Ask), lc == LinkClick::Ask),
                        ("OPEN".into(), Hit::LinkClick(LinkClick::Open), lc == LinkClick::Open),
                        ("HINTS ONLY".into(), Hit::LinkClick(LinkClick::HintsOnly), lc == LinkClick::HintsOnly),
                    ]),
                ));
                v.insert(8, ("".into(), Info("a URL in the shell underlines under the pointer and a click opens it where LINKS says pages go; ASK puts a band on the pane first (D on it stops the asking) · hints mode (ctrl+shift+o) is always there".into())));
                let (jn, jk, co) = (self.behavior.journal, self.behavior.journal_keep, self.behavior.cutoff);
                v.insert(9, (
                    "JOURNAL".into(),
                    Choice(vec![
                        ("REMEMBER".into(), Hit::Journal(true), jn),
                        ("DON'T REMEMBER".into(), Hit::Journal(false), !jn),
                        ("7 DAYS".into(), Hit::JournalKeep(7), jk == 7),
                        ("30 DAYS".into(), Hit::JournalKeep(30), jk == 30),
                        ("90 DAYS".into(), Hit::JournalKeep(90), jk == 90),
                    ]),
                ));
                v.insert(10, ("".into(), Info("one line per finished command, per folder, in profile/journal · nus log, the palette's log rows, and a page per folder read it back".into())));
                v.insert(11, (
                    "CUT OFF".into(),
                    Choice(vec![
                        ("CHIP".into(), Hit::CutOffMode(CutOff::Chip), co == CutOff::Chip),
                        ("RUN AGAIN".into(), Hit::CutOffMode(CutOff::RunAgain), co == CutOff::RunAgain),
                        ("OFF".into(), Hit::CutOffMode(CutOff::Off), co == CutOff::Off),
                    ]),
                ));
                v.insert(12, ("".into(), Info("a command a restart killed comes back as a chip on the restored shell: resume for claude and codex, run again for a server, reconnect for ssh".into())));
                let rk = self.behavior.replay;
                v.insert(13, (
                    "REPLAY".into(),
                    Choice(vec![
                        ("KEEP 7 DAYS".into(), Hit::Replay(ReplayKeep::Days7), rk == ReplayKeep::Days7),
                        ("1 DAY".into(), Hit::Replay(ReplayKeep::Day1), rk == ReplayKeep::Day1),
                        ("OFF".into(), Hit::Replay(ReplayKeep::Off), rk == ReplayKeep::Off),
                    ]),
                ));
                v.insert(14, ("".into(), Info("Recent command history with optional playback and page snapshots. Up to 8 MiB per pane, 32 MiB per window, and 128 MiB of closed sessions. Oldest history rolls off at the selected age or size limit. Shared exports are kept. Ctrl+Shift+H opens history.".into())));
                let ka = self.behavior.keep_alive;
                v.insert(15, (
                    "KEEP ALIVE".into(),
                    Choice(vec![
                        ("ON".into(), Hit::KeepAlive(KeepAlive::On), ka == KeepAlive::On),
                        ("OFF".into(), Hit::KeepAlive(KeepAlive::Off), ka == KeepAlive::Off),
                    ]),
                ));
                v.insert(16, ("".into(), Info(if nus_pty::hold::holder_exe().is_some() { "each shell runs in nus-hold, a small process that outlives the app: quit, crash or update, and a running command is still there when you come back; an idle prompt is let go".into() } else { "nus-hold was not found beside the app, so shells are not held".into() })));
                let sc = self.behavior.shell_colours;
                v.insert(17, (
                    "SHELL COLORS".into(),
                    Choice(vec![
                        ("OFFER".into(), Hit::ShellColours(ShellColours::Chip), sc == ShellColours::Chip),
                        ("ALWAYS".into(), Hit::ShellColours(ShellColours::Always), sc == ShellColours::Always),
                        ("PANE ONLY".into(), Hit::ShellColours(ShellColours::PaneOnly), sc == ShellColours::PaneOnly),
                    ]),
                ));
                v.insert(8, ("".into(), Info("a script that sets the terminal's colors (OSC 10/11, like kitty's set-colors) changes the pane; OFFER puts a chip on it to apply them to the whole look — ink or paper by the background, the accent from the foreground — ALWAYS does it at once · nus theme <name> / nus look from the shell also work".into())));
                let (g, tc) = (self.behavior.grade, self.behavior.truecolour);
                v.insert(9, (
                    "PROGRAM COLORS".into(),
                    Choice(vec![
                        ("AS THEY COME".into(), Hit::Grade(Grade::Off), g == Grade::Off),
                        ("3:1".into(), Hit::Grade(Grade::Large), g == Grade::Large),
                        ("4.5:1 · AA".into(), Hit::Grade(Grade::Aa), g == Grade::Aa),
                        ("7:1 · AAA".into(), Hit::Grade(Grade::Aaa), g == Grade::Aaa),
                    ]),
                ));
                v.insert(10, (
                    "TRUECOLOR".into(),
                    Choice(vec![
                        ("AS SENT".into(), Hit::Truecolour(Truecolour::AsSent), tc == Truecolour::AsSent),
                        ("THE THEME'S SIXTEEN".into(), Hit::Truecolour(Truecolour::Snapped), tc == Truecolour::Snapped),
                    ]),
                ));
                v.insert(11, ("".into(), Info("claude, codex and every TUI bring colors picked against someone else's background; the grade walks any text that can't be read against its paper toward ink until it reads (WCAG), and THE THEME'S SIXTEEN snaps their truecolor to the nearest of ours so they wear the theme · program(p) in rules.luau gives one program its own sixteen, remaps a color it hardcodes, or sets these per program".into())));
                v.insert(9, (
                    "SSH".into(),
                    Choice(vec![("BRING THE INTEGRATION".into(), Hit::SshIntegration(!self.behavior.ssh_integration), self.behavior.ssh_integration)]),
                ));
                v.insert(10, ("".into(), Info("an ssh profile (from ~/.ssh/config, or nus ssh <host>) writes nus's bash and zsh scripts to ~/.cache/nus on the remote over the same connection and execs your shell with them: marks, cwd with the host, exit codes, progress · nothing to install there".into())));
                v.insert(11, (
                    "PROGRESS".into(),
                    Choice(vec![
                        ("SIDEBAR · CRUMB".into(), Hit::ProgressSidebar(!self.behavior.progress_sidebar), self.behavior.progress_sidebar),
                        ("TASKBAR".into(), Hit::ProgressTaskbar(!self.behavior.progress_taskbar), self.behavior.progress_taskbar),
                    ]),
                ));
                v.insert(12, ("REMOTE CONTROL".into(), Info(format!("the nus command drives this window: nus ls · open · edit · launch · send-text · focus · theme · look · ports · hatch · block · ask · the port and token are in profile/instance · rules can call nus.run(\"split\")"))));
                v.insert(13, ("".into(), Info(format!("every command is a block: a lamp on its prompt (click to fold), {} walks them, {} folds and unfolds, {} twice selects one, {} filters by command; hover a block for share · run again · copy", key("↑↓", false), key("←→", true), key("A", false), key("/", true)))));
                v.insert(3, (
                    "CLIPBOARD".into(),
                    Choice(vec![
                        ("COPY ON SELECT".into(), Hit::CopyOnSelect(!self.behavior.copy_on_select), self.behavior.copy_on_select),
                        ("MIDDLE CLICK PASTES".into(), Hit::MiddlePaste(!self.behavior.middle_paste), self.behavior.middle_paste),
                    ]),
                ));
                v.insert(4, (
                    "OSC 52".into(),
                    Choice(vec![
                        ("OFF".into(), Hit::Osc52(Osc52::Off), self.behavior.osc52 == Osc52::Off),
                        ("PROGRAMS MAY SET IT".into(), Hit::Osc52(Osc52::Write), self.behavior.osc52 == Osc52::Write),
                        ("SET AND READ".into(), Hit::Osc52(Osc52::ReadWrite), self.behavior.osc52 == Osc52::ReadWrite),
                    ]),
                ));
                v.insert(5, ("".into(), Info("tmux, neovim and ssh sessions put text on your clipboard through OSC 52; reading it back is off unless you say so · copy keeps the selection, paste is bracketed and asks when it's many lines".into())));
                v.insert(6, (
                    "SCROLL".into(),
                    Choice(crate::scrolling::Easing::ALL.iter().map(|&e| (e.name().caps(), Hit::ScrollEasing(e), e == self.behavior.scroll_easing)).collect()),
                ));
                v.insert(7, (
                    "WHEEL".into(),
                    Choice(vec![("1 LINE".into(), Hit::WheelLines(1), self.behavior.wheel_lines == 1), ("3 LINES".into(), Hit::WheelLines(3), self.behavior.wheel_lines == 3), ("5 LINES".into(), Hit::WheelLines(5), self.behavior.wheel_lines == 5), ("8 LINES".into(), Hit::WheelLines(8), self.behavior.wheel_lines == 8)]),
                ));
                v.insert(8, ("".into(), Info("neoscroll's curves: the view moves a line at a time on an eased clock, and more ticks extend the trip; Shift+PgUp/PgDn and Ctrl+Shift+Home/End ride the same curve".into())));
                v.insert(0, (
                    "SHELL INTEGRATION".into(),
                    Choice(vec![("AUTO".into(), Hit::ShellInt(true), self.behavior.shell_integration), ("OFF".into(), Hit::ShellInt(false), !self.behavior.shell_integration)]),
                ));
                for (n, p) in self.profiles.iter().enumerate().take(6) {
                    let kind = crate::shell::kind_of(&p.program);
                    v.insert(2 + n, (p.name.caps(), Info(crate::shell::describe(kind).into())));
                }
                for p in &self.profiles {
                    v.push((format!("PROFILE · {}", p.name.caps()), Info(format!("{} {}", p.program, p.args.join(" ")))));
                }
                v.push((
                    "AVATAR".into(),
                    Buttons(vec![("CHOOSE A PICTURE".into(), icons::IMAGE, Hit::PickAvatar), ("RELOAD".into(), icons::RELOAD, Hit::ReloadAvatar), ("OPEN PROFILE FOLDER".into(), icons::FOLDER, Hit::OpenProfileDir)]),
                ));
                v.push(("".into(), Info(if self.avatar.is_some() { "profile/avatar.png · shown in the sidebar".into() } else { "choose a picture from anywhere on this machine · it is squared off and kept as profile/avatar.png".into() })));
                v.push(("SCROLLBACK".into(), Choice([2_000u32, 10_000, 50_000, 200_000].iter().map(|&n| (if n >= 1000 { format!("{}K LINES", n / 1000) } else { format!("{n} LINES") }, Hit::Scrollback(n), n == self.behavior.scrollback)).collect())));
                v.push(("ATTENTION".into(), Info("BEL and OSC 133 mark a tab WAITING while it is not active".into())));
                v.push(("ENV".into(), Info("TERM=xterm-256color · COLORTERM=truecolor · TERM_PROGRAM=nus".into())));
                v
            }
            6 => vec![
                (
                    "CLICK TO SOURCE".into(),
                    Choice(vec![("ON".into(), Hit::ClickToSource(true), self.behavior.click_to_source), ("OFF".into(), Hit::ClickToSource(false), !self.behavior.click_to_source)]),
                ),
                ("".into(), Info("alt+shift+click an element on a localhost page and the editor opens at its source: a framework's debug marker (react, svelte, vue), else the served file under the folder the server runs from".into())),
                (
                    "CONTENT BLOCKING".into(),
                    Choice(vec![("ON".into(), Hit::Block(true), self.behavior.block_content), ("OFF".into(), Hit::Block(false), !self.behavior.block_content)]),
                ),
                ("".into(), Info(format!("{} hosts refused · ads, trackers, analytics · add yours to profile/blocklist.txt", crate::browser::blocklist_len()))),
                ("PICTURE IN PICTURE".into(), Control::Caption),
                (crate::pip_policy::Event::LeaveApp.label().to_uppercase(), Choice(vec![("ON".into(), Hit::PipPolicy(crate::pip_policy::Event::LeaveApp,true), self.behavior.pip_policy.get(crate::pip_policy::Event::LeaveApp)), ("OFF".into(), Hit::PipPolicy(crate::pip_policy::Event::LeaveApp,false), !self.behavior.pip_policy.get(crate::pip_policy::Event::LeaveApp))])),
                (crate::pip_policy::Event::LeaveTab.label().to_uppercase(), Choice(vec![("ON".into(), Hit::PipPolicy(crate::pip_policy::Event::LeaveTab,true), self.behavior.pip_policy.get(crate::pip_policy::Event::LeaveTab)), ("OFF".into(), Hit::PipPolicy(crate::pip_policy::Event::LeaveTab,false), !self.behavior.pip_policy.get(crate::pip_policy::Event::LeaveTab))])),
                (crate::pip_policy::Event::FocusApp.label().to_uppercase(), Choice(vec![("ON".into(), Hit::PipPolicy(crate::pip_policy::Event::FocusApp,true), self.behavior.pip_policy.get(crate::pip_policy::Event::FocusApp)), ("OFF".into(), Hit::PipPolicy(crate::pip_policy::Event::FocusApp,false), !self.behavior.pip_policy.get(crate::pip_policy::Event::FocusApp))])),
                (crate::pip_policy::Event::ClickApp.label().to_uppercase(), Choice(vec![("ON".into(), Hit::PipPolicy(crate::pip_policy::Event::ClickApp,true), self.behavior.pip_policy.get(crate::pip_policy::Event::ClickApp)), ("OFF".into(), Hit::PipPolicy(crate::pip_policy::Event::ClickApp,false), !self.behavior.pip_policy.get(crate::pip_policy::Event::ClickApp))])),
                (crate::pip_policy::Event::RestoreWindow.label().to_uppercase(), Choice(vec![("ON".into(), Hit::PipPolicy(crate::pip_policy::Event::RestoreWindow,true), self.behavior.pip_policy.get(crate::pip_policy::Event::RestoreWindow)), ("OFF".into(), Hit::PipPolicy(crate::pip_policy::Event::RestoreWindow,false), !self.behavior.pip_policy.get(crate::pip_policy::Event::RestoreWindow))])),
                (crate::pip_policy::Event::FocusTab.label().to_uppercase(), Choice(vec![("ON".into(), Hit::PipPolicy(crate::pip_policy::Event::FocusTab,true), self.behavior.pip_policy.get(crate::pip_policy::Event::FocusTab)), ("OFF".into(), Hit::PipPolicy(crate::pip_policy::Event::FocusTab,false), !self.behavior.pip_policy.get(crate::pip_policy::Event::FocusTab))])),
                ("".into(), Info("Return controls close PiP only when enabled. Alt-Tab and clicking another app both count as leaving the app; focusing PiP itself keeps it open.".into())),
                ("SKIP INTERVAL".into(), Slider(self::Slider::PipSkip, self.slider_value(self::Slider::PipSkip), format!("{} seconds", self.behavior.pip_skip_seconds.clamp(1,120)))),
                ("".into(), Info("Left / Right and the skip buttons use this interval, including when the page hides its own controls.".into())),
                ("".into(), Info("a video floats out into a small window of its own when you leave its tab; any other pane can be sent out by hand. Hover it for the controls; they fade when the pointer leaves.".into())),
                (
                    "THE BAND".into(),
                    Choice(vec![("ON".into(), Hit::PipBand(true), self.behavior.pip_band), ("OFF".into(), Hit::PipBand(false), !self.behavior.pip_band)]),
                ),
                ("".into(), Info("the signal stripe along the top of the floating window: it says the window is ours, and it is where you take hold to move it".into())),
                (
                    "PROGRESS RULE".into(),
                    Choice(vec![("ON".into(), Hit::PipProgress(true), self.behavior.pip_progress), ("OFF".into(), Hit::PipProgress(false), !self.behavior.pip_progress)]),
                ),
                ("".into(), Info("a hairline of played time along the foot, there whether or not the controls are · off by default: the controls carry the scrubber, and a window at rest stays a picture".into())),
                (
                    "SCROLL".into(),
                    Choice(vec![("SMOOTH".into(), Hit::PageSmooth(true), self.behavior.page_smooth_scroll), ("INSTANT".into(), Hit::PageSmooth(false), !self.behavior.page_smooth_scroll)]),
                ),
                ("".into(), Info("Chromium's own smooth scrolling for wheels and keys; trackpads are pixel-precise either way · takes effect at the next start".into())),
                ("WHEEL SPEED".into(), Choice([50u16, 75, 100, 150, 200].iter().map(|&px| (format!("{px} PX"), Hit::WheelPx(px), px == self.behavior.wheel_px)).collect())),
                ("".into(), Info("how far one notch of the wheel moves a page, and nus's own lists — settings, the reader, the sidebar — which ride TERMINAL · SCROLL's curve · trackpads move as far as your fingers do".into())),
                ("SCROLLBARS".into(), Choice(Scrollbars::ALL.iter().map(|&s| (s.name().caps(), Hit::Scrollbars(s), s == self.behavior.scrollbars)).collect())),
                ("".into(), Info(if cfg!(target_os = "macos") {
                    "for pages and nus's own lists · overlay: thin, gone when still · classic: always up, on a track · hidden: none drawn, applies at once · pages follow System Settings › Appearance › Show scroll bars for overlay or classic".into()
                } else {
                    "for pages and nus's own lists · overlay: thin, gone when still · classic: always up, on a track · hidden: none drawn · hidden applies at once; pages take overlay or classic at the next start".into()
                })),
                (
                    "LOADING BAR".into(),
                    Choice(BarStyle::ALL.iter().map(|&b| (b.name().caps(), Hit::BarStyle(b), b == self.load_bar.style)).collect()),
                ),
                (
                    "STATUS".into(),
                    Choice(vec![
                        ("LAMP".into(), Hit::StatusStyle(Status::Lamp), self.behavior.status == Status::Lamp),
                        ("LAMP + WORD".into(), Hit::StatusStyle(Status::Both), self.behavior.status == Status::Both),
                        ("WORD".into(), Hit::StatusStyle(Status::Word), self.behavior.status == Status::Word),
                        ("NONE".into(), Hit::StatusStyle(Status::None), self.behavior.status == Status::None),
                    ]),
                ),
                ("".into(), Info("the lamp at the end of the tools row: signal while loading, ink when live, Plot corners for local addresses, hollow while asleep".into())),
                (
                    "BAR COLOR".into(),
                    Choice(vec![
                        ("SIGNAL".into(), Hit::BarColor(BarColor::Signal), self.load_bar.color == BarColor::Signal),
                        ("TAB".into(), Hit::BarColor(BarColor::Tab), self.load_bar.color == BarColor::Tab),
                        ("INK".into(), Hit::BarColor(BarColor::Ink), self.load_bar.color == BarColor::Ink),
                    ]),
                ),
                (
                    "BAR WEIGHT".into(),
                    Slider(self::Slider::BarThickness, self.slider_value(self::Slider::BarThickness), format!("{}px", self.load_bar.thickness)),
                ),
                (
                    "BAR CHASE".into(),
                    Slider(
                        self::Slider::BarChase,
                        self.slider_value(self::Slider::BarChase),
                        format!("{} · how eagerly it follows real progress", self.load_bar.chase),
                    ),
                ),
                ("PROTECTED CONTENT".into(), Info(crate::widevine::status())),
                ("".into(), Buttons(vec![("FETCH NOW".into(), icons::DOWNLOAD, Hit::Widevine)])),
                ("".into(), Info("Widevine plays DRM video (Netflix, Prime Video, Disney+). Chromium downloads it into your profile once, then keeps it current; FETCH NOW asks for it straight away.".into())),
                (
                    "DEFAULT BROWSER".into(),
                    Buttons(vec![("MAKE DEFAULT".into(), icons::GLOBE, Hit::MakeDefault), ("UNREGISTER".into(), icons::CLOSE, Hit::Unregister)]),
                ),
                (
                    "".into(),
                    Info(self.register_note.clone()),
                ),
                ("SEARCH".into(), Info(format!("Words that are not an address search {} · PROMPT · SEARCH ENGINE chooses.", self.behavior.prompt.engine.name()))),
                ("NEW TAB".into(), Info("Uses the start page selected in Start/New Tab: prompt palette, home page, saved layout or last page.".into())),
                ("COOKIES".into(), Info("Website storage is managed by Chromium. Use the site settings beside the address for site-specific controls.".into())),
                ("DOWNLOADS".into(), Buttons(vec![("OPEN DOWNLOADS".into(),icons::DOWNLOAD,Hit::Downloads)])),
                ("FILE NAMING".into(), Choice(vec![
                    ("OFF · ORIGINAL NAMES".into(),Hit::DownloadRename(crate::downloads::Rename::Off),self.behavior.download_rename==crate::downloads::Rename::Off),
                    ("ALL DOWNLOADS".into(),Hit::DownloadRename(crate::downloads::Rename::All),self.behavior.download_rename==crate::downloads::Rename::All),
                    ("SELECTIVE · BETA".into(),Hit::DownloadRename(crate::downloads::Rename::Selective),self.behavior.download_rename==crate::downloads::Rename::Selective),
                ])),
                ("".into(),Info("Off keeps the site's filename. All downloads uses a readable page title and keeps the extension. Selective (beta) renames documents, images and media, preserving technical, versioned and unknown filenames. Existing files are never replaced; duplicates receive a number.".into())),
                ("DOWNLOADS · LOCATION".into(), Info(if self.behavior.download_dir.trim().is_empty() { format!("{} · the system's", crate::browser::default_downloads_dir().display()) } else { crate::browser::downloads_dir().display().to_string() })),
                ("".into(), Buttons(vec![("CHOOSE FOLDER".into(), icons::FOLDER, Hit::DownloadDir(0)), ("OPEN".into(), icons::OPEN_EXTERNAL, Hit::DownloadDir(2)), ("RESET".into(), icons::UNDO, Hit::DownloadDir(1))])),
                ("ASK WHERE TO SAVE".into(), Choice(vec![("EACH TIME".into(), Hit::DownloadAsk(true), self.behavior.download_ask), ("NEVER · STRAIGHT TO THE FOLDER".into(), Hit::DownloadAsk(false), !self.behavior.download_ask)])),
                ("".into(), Info("each time: the system's save dialog, starting in the folder above with the file's name; the download waits until you choose, and cancelling drops it".into())),
                ("WHEN A DOWNLOAD FINISHES".into(), Choice(DownloadDone::ALL.iter().map(|&d| (d.name().caps(), Hit::DownloadDone(d), d == self.behavior.download_done)).collect())),
                ("".into(), Info("notice: a slip at the foot, click to show the file · reveal: its folder opens with it selected · open: the file opens in its program · quiet: nothing, the list keeps it".into())),
                ("DEFAULT ZOOM".into(), Choice([80u16, 90, 100, 110, 125, 150].iter().map(|&z| (format!("{z}%"), Hit::PageZoom(z), z == self.behavior.page_zoom)).collect())),
                ("".into(), Info("what a site gets before you zoom it yourself · the gear beside the address remembers a zoom per site, and its reset goes back to this".into())),
                ("PRIVACY SIGNAL".into(), Choice(vec![("SEND".into(), Hit::PrivacySignal(true), self.behavior.privacy_signal), ("OFF".into(), Hit::PrivacySignal(false), !self.behavior.privacy_signal)])),
                ("".into(), Info("Global Privacy Control (Sec-GPC: 1) and Do Not Track on every request · sites that honour it stop selling what they see; the rest ignore it".into())),
                ("CLEAR".into(), Buttons(vec![("COOKIES · ALL SITES".into(), icons::WARNING, Hit::ClearBrowsing(0)), ("THE CACHE".into(), icons::RELOAD, Hit::ClearBrowsing(1))])),
                ("".into(), Info("cookies: every site signs you out, the containers included · the cache: pages fetch fresh; nothing of yours is touched".into())),
                ("PASSWORDS".into(), Info("No built-in password manager. Password integration is not available in this build.".into())),
                ("ENGINE".into(), Info(format!("Chromium {}", crate::chromium_version()))),
            ],
            7 => {
                let b = &self.behavior;
                let g = b.ports_grouping;
                vec![
                    ("".into(), Info(format!("{} · the board: what's listening, who owns it, what to do about it", key("P", true)))),
                    ("GROUPING".into(), Choice(vec![
                        ("ORIGIN".into(), Hit::PortsGrouping(PortsGrouping::Origin), g == PortsGrouping::Origin),
                        ("PORT".into(), Hit::PortsGrouping(PortsGrouping::Port), g == PortsGrouping::Port),
                        ("PROCESS".into(), Hit::PortsGrouping(PortsGrouping::Process), g == PortsGrouping::Process),
                    ])),
                    ("".into(), Info("origin: MINE (ports your shells started) · OTHERS · SYSTEM · CONNECTIONS · DOCKER".into())),
                    ("OPEN IN".into(), Choice(vec![
                        ("TAB".into(), Hit::PortsOpen(PortsOpen::Tab), b.ports_open == PortsOpen::Tab),
                        ("SPLIT".into(), Hit::PortsOpen(PortsOpen::Split), b.ports_open == PortsOpen::Split),
                        ("PEEK".into(), Hit::PortsOpen(PortsOpen::Peek), b.ports_open == PortsOpen::Peek),
                    ])),
                    ("POLL WHILE OPEN".into(), Choice(vec![
                        ("1 S".into(), Hit::PortsPoll(1), b.ports_poll == 1),
                        ("5 S".into(), Hit::PortsPoll(5), b.ports_poll == 5),
                        ("10 S".into(), Hit::PortsPoll(10), b.ports_poll == 10),
                    ])),
                    ("NEW-PORT TOAST".into(), Choice(vec![
                        ("ON".into(), Hit::PortsToast(true), b.ports_toast),
                        ("OFF".into(), Hit::PortsToast(false), !b.ports_toast),
                    ])),
                    ("".into(), Info(format!("a line beside the ports icon for 6 s when something starts listening · {} opens it", key("O", true)))),
                    ("SHOW".into(), Choice(vec![
                        ("SYSTEM".into(), Hit::PortsShow(0, !b.ports_show_system), b.ports_show_system),
                        ("UDP".into(), Hit::PortsShow(1, !b.ports_show_udp), b.ports_show_udp),
                        ("CONNECTIONS".into(), Hit::PortsShow(2, !b.ports_show_connections), b.ports_show_connections),
                        ("DOCKER".into(), Hit::PortsShow(3, !b.ports_show_docker), b.ports_show_docker),
                    ])),
                    ("ASK BEFORE KILL".into(), Choice(vec![
                        ("SYSTEM".into(), Hit::PortsKill(KillConfirm::System), b.ports_kill_confirm == KillConfirm::System),
                        ("ALWAYS".into(), Hit::PortsKill(KillConfirm::Always), b.ports_kill_confirm == KillConfirm::Always),
                        ("NEVER".into(), Hit::PortsKill(KillConfirm::Never), b.ports_kill_confirm == KillConfirm::Never),
                    ])),
                    ("".into(), Info("kill is graceful first, force after 3 s; a DYING lamp in between".into())),
                    ("PROBE".into(), Choice(vec![
                        ("ON".into(), Hit::PortsProbe(true), b.ports_probe),
                        ("OFF".into(), Hit::PortsProbe(false), !b.ports_probe),
                    ])),
                    ("".into(), Info("one GET / to a new port for its status, title and framework · it is a request to your server".into())),
                    ("TUNNEL".into(), Choice(vec![
                        ("CLOUDFLARED".into(), Hit::PortsTunnel(Tunnel::Cloudflared), b.ports_tunnel == Tunnel::Cloudflared),
                        ("NGROK".into(), Hit::PortsTunnel(Tunnel::Ngrok), b.ports_tunnel == Tunnel::Ngrok),
                    ])),
                    ("HIDDEN PROCESSES".into(), Choice(vec![(format!("{} NAMES · RESET", b.ports_hidden.len()), Hit::PortsHidden, false)])),
                    ("".into(), Info("edit the list in prefs.json · rules.luau's `ports` names, tints, auto-opens, tunnels, hides and watches by port or process".into())),
                ]
            }
            8 => {
                let b = &self.behavior;
                let hk = self.hotkey.as_ref().map(|k| k.status.clone()).unwrap_or_else(|| "not registered".into());
                vec![
                    ("".into(), Info("A terminal that drops down over any app with one shortcut, and a list of the work running in your shells. Pick a job to jump to its terminal; open it in nus without restarting it.".into())),
                    ("OPENING IT".into(), Section),
                    ("SHORTCUT".into(), Choice({
                        let mut offered = crate::hotkey::Chord::offered().to_vec();
                        if !offered.contains(&b.hatch_hotkey) { offered.push(b.hatch_hotkey); }
                        offered.into_iter().map(|c| (c.label().into(), Hit::HatchHotkey(c), b.hatch_hotkey == c && !self.hotkey_recording)).collect()
                    })),
                    ("".into(), Help(if hk.is_empty() { format!("{} opens it from any app.", b.hatch_hotkey.label()) } else { format!("{hk} · {} still works inside nus.", b.hatch_hotkey.label()) })),
                    ("OR YOUR OWN".into(), Choice(vec![(if self.hotkey_recording { "PRESS A SHORTCUT… · ESC CANCELS".into() } else { "RECORD A SHORTCUT".into() }, Hit::HatchRecord, self.hotkey_recording)])),
                    ("OPENS ON".into(), Choice(vec![
                        ("THE SCREEN WITH THE POINTER".into(), Hit::HatchMonitor(HatchMonitor::Pointer), b.hatch_monitor == HatchMonitor::Pointer),
                        ("NUS'S SCREEN".into(), Hit::HatchMonitor(HatchMonitor::Foreground), b.hatch_monitor == HatchMonitor::Foreground),
                        ("THE MAIN SCREEN".into(), Hit::HatchMonitor(HatchMonitor::Primary), b.hatch_monitor == HatchMonitor::Primary),
                    ])),
                    ("ON EVERY DESKTOP".into(), Choice(vec![
                        ("ONE HATCH, SHARED".into(), Hit::HatchSpaces(HatchSpaces::One), b.hatch_spaces == HatchSpaces::One),
                        ("ONE PER DESKTOP".into(), Hit::HatchSpaces(HatchSpaces::Follow), b.hatch_spaces == HatchSpaces::Follow),
                    ])),
                    ("HOW IT LOOKS".into(), Section),
                    ("SHAPE".into(), Choice(vec![
                        ("DROPS FROM THE TOP".into(), Hit::HatchLook(HatchLook::Sheet), b.hatch_look == HatchLook::Sheet),
                        ("FLOATING CARD".into(), Hit::HatchLook(HatchLook::Card), b.hatch_look == HatchLook::Card),
                    ])),
                    ("HEIGHT".into(), Choice(vec![
                        ("30%".into(), Hit::HatchSize(30), b.hatch_size == 30),
                        ("40%".into(), Hit::HatchSize(40), b.hatch_size == 40),
                        ("50%".into(), Hit::HatchSize(50), b.hatch_size == 50),
                        ("60%".into(), Hit::HatchSize(60), b.hatch_size == 60),
                    ])),
                    ("".into(), Info("You can also drag its lower edge. On a Mac with a camera notch, the top sheet opens just below it.".into())),
                    ("DIM THE SCREEN BEHIND THE HATCH".into(), Choice(vec![("YES".into(), Hit::HatchDim(true), b.hatch_dim), ("NO".into(), Hit::HatchDim(false), !b.hatch_dim)])),
                    ("BEHAVIOR".into(), Section),
                    ("HIDE WHEN I CLICK AWAY".into(), Choice(vec![("YES".into(), Hit::HatchAutohide(true), b.hatch_autohide), ("NO".into(), Hit::HatchAutohide(false), !b.hatch_autohide)])),
                    ("".into(), Info(format!("{} pins it open. Escape always goes to the terminal, never closes the hatch.", key("↑", true)))),
                    ("WORK STATUS BY THE NOTCH".into(), Choice(vec![("SHOW".into(), Hit::HatchStatus(true), b.hatch_status), ("HIDE".into(), Hit::HatchStatus(false), !b.hatch_status)])),
                    ("NOTICE WHEN WORK FINISHES".into(), Choice(vec![("YES".into(), Hit::HatchNotify(true), b.hatch_notify), ("NO".into(), Hit::HatchNotify(false), !b.hatch_notify)])),
                    ("".into(), Info("A short notice by the top edge that doesn't take focus. Click it to open that session.".into())),
                    ("KEEP RUNNING WHEN WINDOWS CLOSE".into(), Choice(vec![("YES".into(), Hit::HatchBackground(true), b.hatch_background), ("NO".into(), Hit::HatchBackground(false), !b.hatch_background)])),
                    ("".into(), Info("With this on, closing the last window keeps your shells and the hatch alive. Quit from the menu bar or tray to exit fully.".into())),
                    ("".into(), Info(format!("Shortcuts: {} moves the tab you're on into the hatch · {} moves the hatch's tab into this window.", key("↑", true), key("↓", true)))),
                ]
            }
            SEC_ASSISTANTS => self.assistants_settings(),
            SEC_FONTS => self.fonts_settings(),
            SEC_PROMPT => self.prompt_settings(),
            SEC_SAVED => self.saved_settings(),
            10 => {
                // What the rules do right now: three shells, a stack child, a page.
                let theme = if self.theme.mode == nus_render::Mode::Ink { "ink" } else { "paper" };
                let mk = |kind: &str, index: usize, host: &str, parent: Option<&surface::Overrides>| {
                    let shell_color = if kind == "terminal" && self.behavior.shell_tint != crate::shell_colors::ShellTint::None { self.shell_color(Some((index * 4 % crate::shell_colors::SLOTS) as u8)) } else { None };
                    self.rules.new_tab(&surface::TabCtx { kind, index, profile: "powershell", space: &self.space_name, space_signal: self.surface.signal, theme, host, parent, tab_colours: &self.tab_colours, shell_color })
                };
                let a = mk("terminal", 0, "", None);
                let b = mk("terminal", 1, "", None);
                let bc = mk("page", 1, "docs.rs", Some(&b));
                let c = mk("terminal", 2, "", None);
                let d = mk("page", 3, "github.com", None);
                let preview = vec![
                    (a.bg, a.signal, "1  powershell".to_string(), false),
                    (b.bg, b.signal, "2  powershell".to_string(), false),
                    (bc.bg, bc.signal, "docs.rs".to_string(), true),
                    (c.bg, c.signal, "3  wsl · ubuntu".to_string(), false),
                    (d.bg, d.signal, "4  github".to_string(), false),
                ];
                let try4 = mk("terminal", 3, "", None);
                vec![
                ("FILE".into(), Info(self.rules.path.to_string_lossy().to_string())),
                ("STATUS".into(), Info(self.rules.status.clone())),
                ("START FROM".into(), Choice(surface::STARTERS.iter().enumerate().map(|(k, (n, _))| (n.caps(), Hit::Starter(k), false)).collect())),
                ("".into(), Info("a starter replaces new_tab and new_space; on_page and on_event are kept".into())),
                ("NOW".into(), Tabs(preview)),
                ("TRY".into(), Info("new_tab { kind = \"terminal\", index = 4 } →".into())),
                ("EXAMPLE COLORS".into(), Tabs(vec![(try4.bg, try4.signal, "Next terminal tab".into(), false)])),
                ("HOOKS".into(), Info("new_tab · new_space · on_page · on_event   helpers: hue · mix · hsl · family".into())),
                (
                    "".into(),
                    Buttons(vec![
                        ("RELOAD".into(), icons::RELOAD, Hit::ReloadRules),
                        ("OPEN IN EDITOR".into(), icons::OPEN_EXTERNAL, Hit::OpenRules),
                        ("RESET TO DEFAULT".into(), icons::WARNING, Hit::ResetRules),
                    ]),
                ),
            ]
            }
            13 => {
                let device = crate::me::device();
                let (name, face, since, days) = match &self.me {
                    Some(me) => (
                        me.name.clone(),
                        match &me.face { crate::me::Face::Initial => "the initial".to_string(), crate::me::Face::Emoji(e) => e.clone(), crate::me::Face::Picture => "profile/avatar.png".to_string() },
                        me.created.clone(),
                        me.day_word(),
                    ),
                    None => (crate::me::os_user(), "the initial".to_string(), "not yet".to_string(), "not set up".to_string()),
                };
                let mut v: Vec<(String, Control)> = vec![
                    ("".into(), Info("Your name, picture and device name. Your profile is stored locally; no online account is required.".into())),
                ];
                if self.me.is_none() {
                    v.push(("".into(), Buttons(vec![("SET UP THE PROFILE".into(), icons::USER, Hit::MeCard)])));
                }
                v.extend(vec![
                    ("NAME".into(), Choice(vec![(name.caps(), Hit::MeEdit(0), true)])),
                    ("PROFILE PICTURE".into(), Buttons(vec![("CHOOSE A PICTURE".into(), icons::IMAGE, Hit::PickAvatar), ("INITIAL OR EMOJI".into(), icons::USER, Hit::MeEdit(1))])),
                    ("".into(), Info(format!("Current picture: {face}. Choose a picture from anywhere on this machine, or pick an initial or emoji in the profile editor."))),
                    ("".into(), Info("the face is the avatar in the footer; a picture is squared off from the middle and kept as profile/avatar.png, drawn at 22px".into())),
                    ("DEVICE".into(), Choice(vec![(device.caps(), Hit::MeEdit(2), true)])),
                    ("".into(), Info("The device name identifies changes made by this machine when you sync.".into())),
                    ("SINCE".into(), Info(format!("{since} · {days}"))),
                    ("IMPORT FROM".into(), Buttons(vec![("BROWSERS, TERMINALS & EDITORS".into(), icons::DOWNLOAD, Hit::MeWalk(2))])),
                    ("SYNC".into(), Info(self.sync_status())),
                    ("".into(), Buttons(vec![("HOW IT LIVES".into(), icons::BROADCAST, Hit::MeWalk(0)), ("SYNC SETTINGS".into(), icons::SLIDERS, Hit::Section(SEC_SYNC))])),
                    ("PRIVATE".into(), Info("Profile data is stored in a folder on this device. Sync is optional. Opening the folder lets you inspect or back up your files.".into())),
                    ("".into(), Buttons(vec![("OPEN THE PROFILE FOLDER".into(), icons::FOLDER, Hit::MeFolder), ("START OVER".into(), icons::WARNING, Hit::MeForget)])),
                ]);
                v
            }
            12 => {
                let b = &self.behavior;
                let has_key = crate::syncui::key().is_some();
                let ready = has_key && (!b.sync_folder.is_empty() || !b.sync_git.is_empty());
                vec![
                    ("".into(), Info("Keep your settings, rules and layouts the same on every device you use. Everything is encrypted on this device before it goes anywhere; without your key, the files are unreadable.".into())),
                    ("STATUS".into(), Info(if ready { self.sync_status() } else if !has_key { "Not set up · start with step 1".into() } else { "Almost there · choose where to sync in step 2".into() })),
                    ("STEP 1 · YOUR KEY".into(), Section),
                    ("ENCRYPTION KEY".into(), Buttons({
                        let mut v = vec![(if has_key { "SHOW & COPY MY KEY".into() } else { "CREATE A KEY".into() }, icons::SHIELD, Hit::SyncKey), ("I HAVE A KEY FROM ANOTHER DEVICE".into(), icons::ENTER, Hit::SyncEdit(2))];
                        if has_key { v.push(("FORGET THIS KEY".into(), icons::CLOSE, Hit::SyncForget)); }
                        v
                    })),
                    ("".into(), Info("First device: create a key. Every other device: paste that same key. Keep a copy somewhere safe; nus can't recover it for you.".into())),
                    ("STEP 2 · WHERE TO SYNC".into(), Section),
                    ("A SYNCED FOLDER".into(), Buttons(vec![(if b.sync_folder.is_empty() { "CHOOSE A FOLDER".into() } else { format!("FOLDER · {}", crate::app::fit_cmd(&b.sync_folder, 36)) }, icons::FOLDER, Hit::SyncEdit(0))])),
                    ("".into(), Info("A folder your system already syncs, such as iCloud Drive, Dropbox or Syncthing.".into())),
                    ("A PRIVATE GIT REPOSITORY".into(), Buttons({
                        let mut v = vec![(if b.sync_git.is_empty() { "SET A REPOSITORY".into() } else { format!("GIT · {}", crate::app::fit_cmd(&b.sync_git, 36)) }, icons::GITHUB, Hit::SyncEdit(1)), ("SIGN IN TO GITHUB, GITLAB…".into(), icons::USER, Hit::MeWalk(1))];
                        if crate::forge::load().is_some() { v.push(("SIGN OUT".into(), icons::CLOSE, Hit::ForgeForget)); }
                        v
                    })),
                    ("".into(), Info(match crate::forge::load() {
                        Some(f) => format!("Signed in to {}. The token stays on this device and is only sent to that service.", f.word()),
                        None => "GitHub, GitLab, Forgejo or Gitea. Use one or both destinations.".into(),
                    })),
                    ("STEP 3 · WHAT & WHEN".into(), Section),
                    ("ALSO SYNC OPEN TABS".into(), Choice(vec![("YES".into(), Hit::SyncSession(true), b.sync_session), ("NO".into(), Hit::SyncSession(false), !b.sync_session)])),
                    ("".into(), Info("Settings, rules, layouts, folders, port labels, assistant memory and site preferences always sync. Cookies, caches, downloads and shell history never do.".into())),
                    ("HOW OFTEN".into(), Choice(vec![
                        ("ONLY WHEN I ASK".into(), Hit::SyncEvery(0), b.sync_every_min == 0),
                        ("EVERY 5 MIN".into(), Hit::SyncEvery(5), b.sync_every_min == 5),
                        ("EVERY 10 MIN".into(), Hit::SyncEvery(10), b.sync_every_min == 10),
                        ("EVERY 30 MIN".into(), Hit::SyncEvery(30), b.sync_every_min == 30),
                    ])),
                    ("WHEN QUITTING".into(), Choice(vec![("SYNC ONE LAST TIME".into(), Hit::SyncAtQuit(true), b.sync_at_quit), ("DON'T".into(), Hit::SyncAtQuit(false), !b.sync_at_quit)])),
                    ("".into(), Buttons(vec![("SYNC NOW".into(), icons::RELOAD, Hit::SyncNow), ("HOW SYNC WORKS".into(), icons::BOOK, Hit::MeWalk(0))])),
                    ("YOUR PHONE".into(), Section),
                    ("PHONE PAGE".into(), Choice(vec![("ON".into(), Hit::Phone(true), b.phone), ("OFF".into(), Hit::Phone(false), !b.phone)])),
                    ("".into(), match crate::phone::current() {
                        Some(p) if b.phone => Buttons(vec![(format!("COPY LINK · {}", p.url()), icons::COPY, Hit::CopyPhoneUrl)]),
                        _ => Info("A private page for your phone, on this network only: what ran or failed while you were away, what's listening, and a line to ask. The link carries a secret token.".into()),
                    }),
                    ("".into(), match crate::phone::current() {
                        Some(p) if b.phone => Info(format!("Your phone will ask once whether to trust this page. Its fingerprint should be sha-256 {}.", p.fingerprint.to_lowercase())),
                        _ => Info("".into()),
                    }),
                ]
            }
            11 => {
                let mac = cfg!(target_os = "macos");
                let mod_ = |shift: bool| -> Vec<String> {
                    let mut v = vec![if mac { "⌘".to_string() } else { "Ctrl".to_string() }];
                    if shift {
                        v.push(if mac { "⇧".to_string() } else { "Shift".to_string() });
                    }
                    v
                };
                let chord = |k: &str, shift: bool| -> Vec<String> { let mut v = mod_(shift); v.push(k.to_string()); v };
                vec![
                    ("".into(), Info("These shortcuts control nus. Other keys are passed to the active page or terminal.".into())),
                    ("NEW TAB".into(), Keys(chord("T", !mac), "the start page selected in Start/New Tab".into())),
                    ("NEW WINDOW".into(), Keys(chord("N", false), "the new window behavior selected in Start/New Tab".into())),
                    ("GO".into(), Keys(chord("K", true), "the palette: commands, tabs, places".into())),
                    ("URL".into(), Keys(chord("L", true), "a page, by address".into())),
                    ("CLOSE".into(), Keys(chord("W", true), "the tab; the stack folds first".into())),
                    ("REOPEN CLOSED".into(), Keys(chord("Z", true), "the last one closed".into())),
                    ("SPLIT".into(), Keys(chord("D", true), "a second pane beside this one".into())),
                    ("SIDEBAR".into(), Keys(chord("S", true), "shown, hidden, pinned".into())),
                    ("DEVTOOLS".into(), Keys(chord("I", true), "the page's, in a split".into())),
                    ("COPY".into(), Keys(chord("C", true), "the selection, else the last output; on a page, its url".into())),
                    ("FOLD".into(), Keys(chord("-", true), "every stack; again unfolds".into())),
                    ("TIMELINE".into(), Keys(chord("H", true), "this tab at any checkpoint".into())),
                    ("TAB N".into(), Keys(chord("1–9", false), "the stack, at its last-used member".into())),
                    ("MRU".into(), Keys(chord("`", false), "the tab you were on".into())),
                    ("PREV / NEXT".into(), Keys(chord("PgUp / PgDn", false), "the tab beside this one".into())),
                    ("SETTINGS".into(), Keys(chord(",", false), "this".into())),
                    ("FULLSCREEN".into(), Keys(vec!["F11".into()], "the window edge to edge".into())),
                    ("WELCOME".into(), Buttons(vec![("THE TOUR · F1".into(), icons::BOOK, Hit::Welcome)])),
                ]
            }
            _ => {
                let status=crate::updates::status();
                let mut rows=vec![("UPDATES".into(),Info(status.message.clone()))];
                if status.available && !status.busy && !status.confirming {rows.push(("".into(),Buttons(vec![("UPDATE".into(),icons::DOWNLOAD,Hit::UpdateInstall)])));}
                if status.confirming {rows.push(("RESTART REQUIRED".into(),Info(crate::updates::INTERRUPTION_WARNING.into())));rows.push(("".into(),Buttons(vec![("DOWNLOAD & RESTART".into(),icons::RELOAD,Hit::UpdateConfirm),("CANCEL".into(),icons::CLOSE,Hit::UpdateCancel)])));}
                if let Some(recovery) = crate::update_install::recovery() {
                    rows.push(("RECOVERY".into(),Info(format!("Return to {} and its saved profile. This version's profile and application will be kept separately. Project files will not be reverted. Running processes may be interrupted.", recovery.previous_version))));
                    if !status.busy { rows.push(("".into(),Buttons(vec![("RETURN TO PREVIOUS VERSION…".into(),icons::RELOAD,Hit::RecoverPrevious)]))); }
                }
                if let Some(place)=crate::install::placement() {
                    let own=crate::install::wants_separate();
                    let others=crate::install::other_profiles().len();
                    rows.push(("PROFILE".into(),Section));
                    rows.push(("THIS COPY USES".into(),Choice(vec![("THE SHARED PROFILE".into(),Hit::ProfileSeparate(false),!own),("A PROFILE OF ITS OWN".into(),Hit::ProfileSeparate(true),own)])));
                    let mut says=if place.separate {"This copy keeps its own profile. ".to_string()} else {"Every copy of nus on this channel shares one profile: settings, sessions, sign-ins and pages. One copy uses it at a time; a newer version saves a recoverable copy before upgrading it. ".to_string()};
                    if own!=place.separate {says.push_str("Your change applies when this copy restarts. ");}
                    says.push_str(&format!("In {}", place.root.display()));
                    if others>0 {says.push_str(&format!(" · {others} other profile{} in this channel", if others==1 {""} else {"s"}));}
                    says.push('.');
                    rows.push(("".into(),Info(says)));
                    rows.push(("".into(),Buttons(vec![("SHOW PROFILES".into(),icons::FOLDER,Hit::ProfileFolder)])));
                }
                rows.extend(vec![
                    ("PROFILE COMPATIBILITY".into(),Info(crate::compatibility::summary())),
                    ("SUPPORT DETAILS".into(),Info(crate::support::details())),
                    ("".into(),Buttons(vec![("COPY SUPPORT DETAILS".into(),icons::COPY,Hit::CopySupportDetails)])),
                    ("CHECK AUTOMATICALLY".into(),Choice(vec![("ON".into(),Hit::UpdateChecks(true),self.behavior.update_checks),("OFF".into(),Hit::UpdateChecks(false),!self.behavior.update_checks)])),
                    ("".into(),Buttons(vec![("CHECK FOR UPDATES".into(),icons::RELOAD,Hit::UpdateCheck)])),
                    ("UPDATE PRIVACY".into(),Info("Checks contact GitHub Releases without a profile ID, account, file paths or usage events. GitHub receives normal connection metadata such as your IP address. Downloads and installation require your click.".into())),
                    ("LOCAL STATE".into(),Info(crate::protected_state::status())),
                    ("TELEMETRY".into(),Info("none".into())),
                    ("VERSION".into(),Info(crate::support::version())),
                    ("HELP".into(),Buttons(vec![("REPORT A BUG".into(),icons::BUG,Hit::Report(crate::support::Kind::Bug)),("REQUEST A FEATURE".into(),icons::CHAT,Hit::Report(crate::support::Kind::Feature))])),
                    ("REPORT PRIVACY".into(),Info("GitHub opens an editable draft with your app version and OS. URLs, file paths, logs and other personal details are not attached. Review anything you add before posting publicly.".into())),
                ]);rows
            },
        }
    }

    /// One-line hint under each tile.
    fn tile_hint(&self, k: usize) -> String {
        match k {
            0 => format!("{} · {} · {}", self.preset_name.to_lowercase(), if self.theme.mode == nus_render::Mode::Ink { "ink" } else { "paper" }, self.surface.shell.name()),
            1 => if self.sound.prefs.enabled { format!("on · {}%", (self.sound.prefs.volume * 100.0).round()) } else { "off".into() },
            2 => format!("start page: {}", match self.behavior.then {
                Then::Palette => "command palette", Then::Prompt => "home prompt", Then::HomePage => "home page", Then::Layout => "custom layout",
                Then::LastPage => "the last page", Then::Restore => "saved session", Then::Shell => "shell",
            }),
            3 => format!("{:?} · {:?}", self.sidebar_rules.side, self.sidebar_rules.fullscreen).to_lowercase(),
            4 => format!("links → {:?}", self.behavior.links).to_lowercase(),
            5 => self.profiles.get(self.behavior.default_profile).map(|p| p.name.clone()).unwrap_or_default(),
            6 => format!("{} bar · google", self.load_bar.style.name()),
            7 => format!("{} · {}", self.behavior.ports_grouping.name(), if self.behavior.ports_toast { "toast on" } else { "toast off" }),
            8 => format!("{:?} · {}", self.behavior.hatch_look, self.behavior.hatch_hotkey.label()).to_lowercase(),
            9 => "claude · codex · ollama".into(),
            10 => self.rules.status.clone(),
            11 => "chords · the shell keeps its own".into(),
            12 => if self.sync_ready() { "on".into() } else { "off · no key or carrier".into() },
            13 => match &self.me { Some(me) => format!("{} · {}", me.name.to_lowercase(), me.day_word()), None => "not set up · local, no account".into() },
            SEC_FONTS => "interface · terminal · editor".into(),
            SEC_PROMPT => "presets · sources · layout".into(),
            SEC_SAVED => format!("{} saved · visibility · behavior", self.behavior.prompt.saved.len()),
            _ => "github releases".into(),
        }
    }

    /// The tile grid: icon, name, a one-line state; 44px+ targets.
    fn draw_tiles(&mut self, scene: &mut Scene, r: Rect, scroll: f32) {
        let t = self.theme.clone();
        let ink = t.ink;
        let label = self.label();
        let strong = self.label_strong();
        let dim = Style { color: t.dim, ..label };
        let pad = self.px(18.0);
        scene.layer(Some(r));
        let mut y = r.y + self.px(28.0) - scroll;
        let wm = Style { font: self.f.wordmark, px: self.px(34.0), color: ink, tracking: 0.0 };
        self.fonts.draw(scene, wm, r.x + pad, y + self.px(30.0), "settings");
        y += self.px(58.0);
        let cols = if r.w / self.scale < 560.0 { 2 } else { 3 };
        let gap = self.px(12.0);
        let tw = ((r.w - 2.0 * pad - gap * (cols as f32 - 1.0)) / cols as f32).floor();
        let th = self.px(96.0);
        let isz = self.px(22.0);
        let mut slot = 0usize;
        let mut gy = y;
        for (gname, range) in GROUPS.iter() {
            // A group caption, then its tiles on a fresh row.
            if slot % cols != 0 {
                slot += cols - slot % cols;
            }
            let row0 = slot / cols;
            let cy = y + row0 as f32 * (th + gap) + gy - y;
            self.fonts.draw(scene, dim, r.x + pad, cy + self.px(10.0), gname);
            gy += self.px(22.0);
            for k in range.clone() {
                let (name, icon) = &SECTIONS[k];
                let col = slot % cols;
                let row = slot / cols;
                slot += 1;
                let tile = Rect::new(r.x + pad + col as f32 * (tw + gap), y + row as f32 * (th + gap) + (gy - y), tw, th);
                self.settings_reach=(tile.bottom()+scroll-r.y+self.px(24.0)).max(self.settings_reach);
                if tile.bottom() < r.y || tile.y > r.bottom() { continue; }
            scene.outline(tile, self.px(m::HAIRLINE), ink);
            self.fonts.draw_icon(scene, *icon, isz, tile.x + self.px(16.0), tile.y + self.px(16.0), ink);
            let base = tile.y + self.px(16.0) + isz + self.px(22.0);
            self.fonts.draw(scene, strong, tile.x + self.px(16.0), base, name);
            let hint = self.fit(dim, &self.tile_hint(k), tw - self.px(32.0));
            self.fonts.draw(scene, dim, tile.x + self.px(16.0), base + self.px(18.0), &hint);
            self.settings_hits.push((tile.intersect(&r), Hit::Tile(k)));
            }
        }
    }

    pub(crate) fn draw_settings(&mut self, scene: &mut Scene, p: &mut SettingsPane) {
        let t = self.theme.clone();
        let ink = t.ink;
        let label = self.label();
        let strong = self.label_strong();
        let ui = self.ui();
        let dim = Style { color: t.dim, ..label };
        let mut r = p.rect;
        self.settings_hits.clear();
        let search=Rect::new(r.x+self.px(12.0),r.y+self.px(8.0),r.w-self.px(24.0),self.px(32.0));
        scene.outline(search,self.px(1.0),t.dim);
        self.fonts.draw_icon(scene,icons::SEARCH,self.px(14.0),search.x+self.px(10.0),search.y+self.px(8.0),ink);
        let hint=self.fit(label,&format!("Search settings · {}",key("F",false)),search.w-self.px(50.0));
        self.fonts.draw(scene,label,search.x+self.px(32.0),search.y+self.px(21.0),&hint);
        self.settings_hits.push((search,Hit::Search));
        r.y+=self.px(48.0);r.h-=self.px(48.0);

        // Use a scrolling index when width or height cannot fit the complete
        // navigation. Even the last section must remain reachable.
        let nav_min = SECTIONS.len() as f32 * self.px(m::LABEL_PX + 10.0)
            + GROUPS.len() as f32 * self.px(24.0) + self.px(40.0);
        let tiles = r.w / self.scale < crate::app::NARROW || r.h < nav_min;
        if tiles && !p.drill {
            self.settings_reach=0.0;
            self.draw_tiles(scene, r, p.scroll);
            scene.layer(None);
            return;
        }

        // Nav (or, drilled in, a back crumb).
        let nav_w = if tiles { 0.0 } else { self.px(220.0) };
        let mut top = r.y;
        if tiles {
            let bh = self.px(12.0) * 2.0 + self.px(m::LABEL_PX) + self.px(m::HAIRLINE);
            let isz = self.px(14.0);
            let base = r.y + self.px(12.0) + self.px(m::LABEL_PX) - self.px(2.0);
            self.fonts.draw_icon(scene, icons::BACK, isz, r.x + self.px(18.0), base - isz + self.px(2.0), ink);
            self.fonts.draw(scene, label, r.x + self.px(18.0) + isz + self.px(10.0), base, "SETTINGS");
            scene.hline(r.x, r.y + bh - self.px(m::HAIRLINE), r.w, self.px(m::HAIRLINE), ink);
            self.settings_hits.push((Rect::new(r.x, r.y, r.w, bh), Hit::Back));
            top += bh;
        } else {
            scene.vline(r.x + nav_w, r.y, r.h, self.px(m::STRUCTURE), ink);
        }
        // The nav fits the window: rows tighten when it is short.
        let mut sh = self.px(12.0) * 2.0 + self.px(m::LABEL_PX) + self.px(m::HAIRLINE);
        let mut gh = self.px(24.0);
        if !tiles {
            let avail = r.h - self.px(40.0);
            let need = SECTIONS.len() as f32 * sh + GROUPS.len() as f32 * gh + (GROUPS.len() as f32 - 1.0) * self.px(6.0);
            if need > avail {
                gh = self.px(18.0);
                sh = ((avail - GROUPS.len() as f32 * gh - (GROUPS.len() as f32 - 1.0) * self.px(6.0)) / SECTIONS.len() as f32).max(self.px(m::LABEL_PX) + self.px(10.0));
            }
        }
        let isz = self.px(14.0);
        let (mx, my) = self.mouse;
        let mut y = r.y;
        for (gi, (gname, range)) in GROUPS.iter().enumerate().filter(|_| !tiles) {
            // Group caption: small, dim, no rule of its own.
            if gi > 0 {
                y += self.px(6.0);
            }
            let cap = Style { color: t.dim, px: self.px(10.0), ..label };
            self.fonts.draw(scene, cap, r.x + self.px(18.0), y + gh - self.px(8.0), gname);
            y += gh;
            for i in range.clone() {
            let (name, icon) = &SECTIONS[i];
            let sel = i == p.section;
            let row = Rect::new(r.x, y, nav_w, sh);
            if sel {
                scene.rect(Rect::new(r.x, y, nav_w, sh - self.px(m::HAIRLINE)), ink);
            } else if row.contains(mx, my) {
                scene.rect(Rect::new(r.x, y, nav_w, sh - self.px(m::HAIRLINE)), t.tint);
            }
            let col = if sel { self.on_fill(ink) } else { ink };
            let base = y + (sh - self.px(m::LABEL_PX)) / 2.0 + self.px(m::LABEL_PX) - self.px(2.0);
            self.fonts.draw_icon(scene, *icon, isz, r.x + self.px(18.0), base - isz + self.px(2.0), col);
            self.fonts.draw(scene, Style { color: col, ..label }, r.x + self.px(18.0) + isz + self.px(10.0), base, name);
            scene.hline(r.x, y + sh - self.px(m::HAIRLINE), nav_w, self.px(m::HAIRLINE), ink);
            self.settings_hits.push((row, Hit::Section(i)));
            y += sh;
            }
        }
        if !tiles {
            let cfg = "Changes save automatically";
            self.fonts.draw(scene, dim, r.x + self.px(18.0), r.bottom() - self.px(14.0), cfg);
        }

        // Content, scrolling within its column.
        let cx = r.x + nav_w + if tiles { self.px(18.0) } else { self.px(40.0) };
        let mut content = Rect::new(r.x + nav_w, top, r.w - nav_w, r.bottom() - top);
        // A live preview, drawn from the settings as they are this frame:
        // beside the rows when there is room, else a band above them. It
        // stays put while the rows scroll, so a change shows where it lands.
        let avail = r.w - nav_w - if tiles { self.px(36.0) } else { self.px(80.0) };
        // Too narrow to draw anything legible: the rows alone.
        let live = crate::live::has_live(p.section) && avail >= self.px(360.0);
        let live_gap = self.px(36.0);
        let side_w = if live && avail >= self.px(560.0) + live_gap + self.px(300.0) { (avail * 0.42).clamp(self.px(300.0), self.px(520.0)) } else { 0.0 };
        let maxw = if side_w > 0.0 { (avail - side_w - live_gap).min(self.px(700.0)) } else { avail.min(self.px(760.0)) };
        let live_rect = if !live {
            None
        } else if side_w > 0.0 {
            let h = self.live_height(p.section, side_w).min(content.h - self.px(56.0));
            Some(Rect::new(cx + maxw + live_gap, top + self.px(28.0), side_w, h))
        } else {
            let h = self.live_height(p.section, maxw).min(content.h * 0.34).min(maxw * 0.5);
            let band = Rect::new(cx, top + self.px(14.0), maxw, h);
            content = Rect::new(content.x, band.bottom() + self.px(10.0), content.w, (content.bottom() - band.bottom() - self.px(10.0)).max(0.0));
            scene.hline(content.x, content.y, content.w, self.px(m::HAIRLINE), t.tint);
            Some(band)
        };
        let top = content.y;
        let (mx, my) = self.mouse;
        let mut live_focus: Option<Hit> = None;
        let mut last_row_hit: Option<Hit> = None;
        let scroll = p.scroll.max(0.0);
        let content_hit_start = self.settings_hits.len();
        scene.layer(Some(content));
        let mut y = top + self.px(28.0) - scroll;
        let wm = Style { font: self.f.wordmark, px: self.px(34.0), color: ink, tracking: 0.0 };
        self.fonts.draw(scene, wm, cx, y + self.px(30.0), &SECTIONS[p.section].0.to_lowercase());
        y += self.px(58.0);
        let label_w = if tiles { self.px(140.0) } else { self.px(200.0) };
        let rows = self.rows_for(p.section);
        let helped: Vec<bool> = (0..rows.len()).map(|i| matches!(rows.get(i + 1), Some((_, Control::Help(_))))).collect();
        for (row_index, (k, control)) in rows.into_iter().enumerate() {
            let row_hits = self.settings_hits.len();
            // Full-width controls: caption above, the control across the column.
            let stacked = matches!(control, Control::Choice(_) | Control::Buttons(_) | Control::Slider(..) | Control::Keys(..) | Control::Stepper(..))
                && (tiles || self.fonts.measure(label, &k) > label_w - self.px(14.0));
            let full = stacked || matches!(control, Control::AppIcons | Control::Intelligence | Control::Mercury | Control::DrawerPreview | Control::FontProof | Control::PromptProof | Control::Studio | Control::Strip(_) | Control::Cards(_) | Control::Tokens(..) | Control::Art(_) | Control::Pics(_) | Control::Actions(_) | Control::Sources(_))
                || matches!(control, Control::Info(_) | Control::Help(_) | Control::SavedCommand(_));
            let cap_h = if full && !k.is_empty() { self.px(26.0) } else { 0.0 };
            let report_actions = matches!(&control, Control::Actions(items) if items.iter().any(|(_,_,_,h)| matches!(h, Hit::Report(_))));
            let card_w = self.px(if report_actions {220.0} else {168.0}).min((maxw - self.px(6.0)).max(self.px(60.0)));
            let card_h = self.px(104.0);
            let gap = self.px(14.0);
            let per_row = ((maxw + gap) / (card_w + gap)).floor().max(1.0) as usize;
            let text_w = if full { maxw } else { maxw - label_w };
            let rh = match &control {
                Control::SavedCommand(i) => self.saved_card_height(*i,maxw),
                Control::AppIcons=>self.icon_choices_height(maxw)+cap_h+self.px(18.0),
                Control::Mercury => self.mercury_settings_height(maxw) + cap_h + self.px(18.0),
                Control::DrawerPreview => self.drawer_preview_height() + cap_h,
                Control::Intelligence => cap_h + self.px(124.0),
                Control::FontProof | Control::PromptProof => self.px(226.0) + cap_h,
                Control::Pics(cards) => cap_h + cards.len().div_ceil(per_row) as f32 * (card_h + self.px(50.0) + gap) + self.px(10.0),
                Control::Actions(items) => cap_h + items.len().div_ceil(per_row) as f32 * (self.px(94.0) + gap) + self.px(10.0),
                Control::Studio => self.px(180.0) + self.px(18.0),
                Control::Strip(items) => cap_h + self.px(10.0) + wrap_count(&items.iter().map(|(text,_,_)|(self.fonts.measure(strong,text)+self.px(28.0)+1.0).ceil().min(maxw)).collect::<Vec<_>>(),self.px(10.0),maxw) as f32*self.px(44.0),
                Control::Cards(cards) => {
                    let rows = (cards.len() + per_row - 1) / per_row;
                    cap_h + rows as f32 * (card_h + self.px(8.0) + gap) + self.px(10.0)
                }
                Control::Art(cards) => {
                    let rows = (cards.len() + per_row - 1) / per_row;
                    cap_h + rows as f32 * (card_h + self.px(50.0) + gap) + self.px(10.0)
                }
                Control::Tokens(items, big) => {
                    let (tw, th) = if *big { (self.px(84.0), self.px(64.0) + self.px(34.0)) } else { (self.px(34.0), self.px(34.0)) };
                    let tg = if *big { self.px(16.0) } else { self.px(10.0) };
                    let per = ((maxw + tg) / (tw + tg)).floor().max(1.0) as usize;
                    let rows = (items.len().max(1) + per - 1) / per;
                    cap_h + rows as f32 * (th + tg) + self.px(10.0)
                }
                Control::Swatches(_) => self.px(12.0) * 2.0 + self.px(18.0) + self.px(m::HAIRLINE),
                // Prose wraps; a row grows with its lines.
                Control::Info(v) | Control::Help(v) => {
                    let lines = crate::reader::wrap(&self.fonts, ui, v, text_w).len().max(1);
                    self.px(10.0) * 2.0 + self.px(m::UI_PX) + self.px(m::HAIRLINE) + (lines as f32 - 1.0) * self.px(m::UI_PX * 1.5)
                }
                // Chips and buttons wrap when the column is narrow.
                Control::Choice(opts) => {
                    let widths: Vec<f32> = opts.iter().map(|(t, _, _)| self.fonts.measure(label, t) + self.px(20.0)).collect();
                    let lines = wrap_count(&widths, self.px(8.0), text_w);
                    self.px(10.0) * 2.0 + self.px(m::UI_PX) + self.px(m::HAIRLINE) + (lines as f32 - 1.0) * (self.px(m::LABEL_PX) + self.px(20.0))
                }
                Control::Buttons(items) => {
                    let widths: Vec<f32> = items.iter().map(|(t, _, _)| self.fonts.measure(strong, t) + self.px(24.0) + self.px(13.0) + self.px(8.0)).collect();
                    let lines = wrap_count(&widths, self.px(14.0), text_w);
                    self.px(10.0) * 2.0 + self.px(m::UI_PX) + self.px(m::HAIRLINE) + (lines as f32 - 1.0) * (self.px(m::LABEL_PX) + self.px(24.0))
                }
                Control::Tabs(rows) => self.px(8.0) + rows.len() as f32 * self.px(26.0) + self.px(12.0),
                Control::Caption => self.px(40.0),
                Control::Section => self.px(64.0),
                Control::Sources(items) => cap_h + self.px(30.0) + items.len() as f32 * self.px(32.0) + self.px(12.0),
                Control::Keys(keys, note) => {
                    let kw: f32 = (keys.iter().map(|k| self.fonts.measure(strong, k) + self.px(16.0) + self.px(10.0)).sum::<f32>() + self.px(8.0)).max(self.px(236.0));
                    let lines = crate::reader::wrap(&self.fonts, dim, note, if tiles {maxw} else {(maxw - label_w - kw).max(self.px(80.0))}).len().max(1);
                    self.px(10.0) * 2.0 + self.px(m::UI_PX) + self.px(m::HAIRLINE) + (lines as f32 - 1.0) * self.px(m::UI_PX * 1.5) + if tiles {self.px(34.0)} else {0.0}
                }
                _ => self.px(10.0) * 2.0 + self.px(m::UI_PX) + self.px(m::HAIRLINE),
            };
            let rh = rh + if stacked || matches!(control, Control::Info(_) | Control::Help(_)) { cap_h } else { 0.0 };
            // Help sits close under its setting.
            let rh = if matches!(control, Control::Help(_)) { rh - self.px(8.0) } else { rh };
            if self.settings_target == Some((p.section,self.look_tab,row_index)) {
                p.scroll=(y+scroll-top-self.px(16.0)).max(0.0);
                self.settings_target=None;
                self.settings_highlight=Some((p.section,self.look_tab,row_index,crate::clock::now()));
                self.dirty=true;
            }
            if self.settings_highlight.is_some_and(|(s,t,i,at)|s==p.section && t==self.look_tab && i==row_index && crate::clock::since(at).as_secs_f32()<3.0) {
                scene.rect(Rect::new(cx-self.px(8.0),y,maxw+self.px(16.0),rh),fade(self.surface.signal,0.1));
                scene.vline(cx-self.px(8.0),y,rh,self.px(3.0),self.surface.signal);
                self.dirty=true;
            }
            let control_kind = if matches!(control, Control::Studio | Control::Strip(_) | Control::Caption | Control::Section) { 0 } else { 1 };
            let base = y + self.px(10.0) + self.px(m::UI_PX) - self.px(3.0) + if stacked || matches!(control, Control::Info(_) | Control::Help(_)) { cap_h } else { 0.0 } - if matches!(control, Control::Help(_)) { self.px(8.0) } else { 0.0 };
            if matches!(control, Control::Section) {
                // A rule across the column, then the part's name in ink:
                // the parts of the page read as parts, not one long list.
                if y > top + self.px(8.0) { scene.rect(Rect::new(cx, y + self.px(18.0), maxw, self.px(m::HAIRLINE)), t.ink); }
                let head = Style { color: t.ink, px: self.px(12.0), tracking: self.px(1.4), ..strong };
                self.fonts.draw(scene, head, cx, y + self.px(50.0), &k);
                y += rh;
                continue;
            }
            if matches!(control, Control::Caption) {
                let cap = Style { color: t.dim, px: self.px(10.0), tracking: self.px(1.2), ..label };
                self.fonts.draw(scene, cap, cx, y + self.px(30.0), &k);
                y += rh;
                continue;
            }
            if full {
                if !k.is_empty() {
                    self.fonts.draw(scene, dim, cx, y + self.px(12.0), &k);
                }
            } else {
                self.fonts.draw(scene, dim, cx, base, &k);
            }
            // In the studio, unlabelled rows run the full column.
            let vx = if full || (p.section == SEC_LOOK && k.is_empty()) { cx } else { cx + label_w };
            match control {
                Control::SavedCommand(i) => {
                    self.draw_saved_card(scene,Rect::new(cx,y,maxw,rh),i);
                }
                Control::AppIcons=>self.draw_icon_choices(scene,Rect::new(cx,y+cap_h,maxw,self.icon_choices_height(maxw))),
                Control::Mercury => self.draw_mercury_settings(scene, Rect::new(cx, y+cap_h, maxw, self.mercury_settings_height(maxw))),
                Control::DrawerPreview => self.draw_drawer_preview(scene, Rect::new(cx, y+cap_h, maxw, self.drawer_preview_height())),
                Control::Intelligence => {
                    let w = maxw.min(self.px(520.0));
                    self.draw_intel_ring(scene, Rect::new(cx, y + cap_h, w, self.px(72.0)));
                    let line = self.fit(dim, &self.intel_sends(), maxw);
                    self.fonts.draw(scene, dim, cx, y + cap_h + self.px(98.0), &line);
                }
                Control::FontProof | Control::PromptProof => {self.draw_type_proof(scene,Rect::new(cx,y+cap_h,maxw,self.px(206.0)),matches!(control,Control::PromptProof));}
                Control::Caption | Control::Section => {}
                Control::Pics(cards) => {
                    for (i, (name, caption, pic, hit, on)) in cards.into_iter().enumerate() {
                        let card = Rect::new(cx + (i % per_row) as f32 * (card_w + gap), y + cap_h + (i / per_row) as f32 * (card_h + self.px(50.0) + gap), card_w, card_h);
                        let target = Rect::new(card.x, card.y, card.w + self.px(6.0), card.h + self.px(50.0));
                        if target.bottom() > content.y && target.y < content.bottom() {
                            self.draw_pic_card(scene, card, &name, &caption, pic, on, hit);
                            self.settings_hits.push((target, hit));
                        }
                    }
                }
                Control::Actions(items) => {
                    for (i, (name, caption, icon, hit)) in items.into_iter().enumerate() {
                        let x = cx + (i % per_row) as f32 * (card_w + gap);
                        let cy = y + cap_h + (i / per_row) as f32 * (self.px(94.0) + gap);
                        let rest = Rect::new(x, cy + self.px(6.0), card_w, self.px(40.0));
                        let hot = rest.contains(mx, my);
                        scene.rect(Rect::new(rest.x + self.px(3.0), rest.y + self.px(3.0), rest.w, rest.h), ink);
                        // Hover presses the card halfway into its shadow. The tint is
                        // translucent, so paper goes under it or the shadow shows through.
                        let push = if hot { self.px(1.5) } else { 0.0 };
                        let button = Rect::new(rest.x + push, rest.y + push, rest.w, rest.h);
                        let x = button.x;
                        scene.rect(button, t.paper);
                        if hot { scene.rect(button, t.tint); }
                        scene.outline(button, self.px(m::STRUCTURE), ink);
                        self.fonts.draw_icon(scene, icon, self.px(16.0), x + self.px(10.0), button.y + self.px(12.0), ink);
                        let name = self.fit(label, &name, card_w - self.px(42.0));
                        self.fonts.draw(scene, label, x + self.px(34.0), button.y + self.px(24.0), &name);
                        for (line, text) in crate::reader::wrap(&self.fonts, dim, &caption, card_w).into_iter().take(3).enumerate() {
                            self.fonts.draw(scene, dim, rest.x, cy + self.px(63.0 + line as f32 * 12.0), &text);
                        }
                        self.settings_hits.push((rest, hit));
                    }
                }
                Control::Studio => {
                    let r = Rect::new(cx, y, maxw, self.px(180.0));
                    self.draw_studio(scene, r);
                }
                Control::Sources(items) => {
                    use crate::settings::workspace::Hit as W;
                    let top_y = y + cap_h;
                    let head = Style { color: t.dim, px: self.px(9.5), tracking: self.px(1.0), ..label };
                    let cols = [maxw * 0.40, maxw * 0.16, maxw * 0.16, maxw * 0.15, maxw * 0.13];
                    let mut xs = [cx; 5];
                    for i in 1..5 { xs[i] = xs[i - 1] + cols[i - 1]; }
                    for (i, h) in ["SOURCE", "BEFORE TYPING", "WHILE TYPING", "HOW MANY", "ORDER"].iter().enumerate() {
                        self.fonts.draw(scene, head, xs[i], top_y + self.px(18.0), h);
                    }
                    scene.hline(cx, top_y + self.px(26.0), maxw, self.px(m::HAIRLINE), ink);
                    let n = items.len();
                    for (k, (src, home, search, count)) in items.into_iter().enumerate() {
                        let ry = top_y + self.px(30.0) + k as f32 * self.px(32.0);
                        let mid = ry + self.px(16.0);
                        let off = !home && !search;
                        self.fonts.draw(scene, Style { color: if off { t.dim } else { ink }, ..ui }, xs[0], mid + self.px(4.0), &self.fit(ui, src.name(), cols[0] - self.px(8.0)));
                        for (col, on, which) in [(1, home, 0u8), (2, search, 1u8)] {
                            let b = Rect::new(xs[col], mid - self.px(8.0), self.px(16.0), self.px(16.0));
                            if on { scene.rect(b, ink); self.fonts.draw_icon(scene, icons::CHECK, self.px(12.0), b.x + self.px(2.0), b.y + self.px(2.0), t.paper); } else { scene.outline(b, self.px(m::HAIRLINE), ink); }
                            self.settings_hits.push((Rect::new(b.x - self.px(6.0), ry, cols[col] - self.px(4.0), self.px(32.0)), Hit::Workspace(W::Source(src, which))));
                        }
                        // − n +
                        let bw = self.px(18.0);
                        let minus = Rect::new(xs[3], mid - self.px(9.0), bw, self.px(18.0));
                        let plus = Rect::new(xs[3] + bw + self.px(22.0), mid - self.px(9.0), bw, self.px(18.0));
                        for (r, word, hit) in [(minus, "−", W::Count(src, -1)), (plus, "+", W::Count(src, 1))] {
                            scene.outline(r, self.px(m::HAIRLINE), ink);
                            let w = self.fonts.measure(ui, word);
                            self.fonts.draw(scene, ui, r.x + (r.w - w) * 0.5, r.y + self.px(13.5), word);
                            self.settings_hits.push((r, Hit::Workspace(hit)));
                        }
                        let num = count.to_string();
                        let w = self.fonts.measure(strong, &num);
                        self.fonts.draw(scene, strong, minus.right() + (self.px(22.0) - w) * 0.5, mid + self.px(4.0), &num);
                        // ↑ ↓
                        for (j, (icon, d)) in [(icons::CARET_DOWN, -1i8), (icons::CARET_DOWN, 1i8)].iter().enumerate() {
                            let r = Rect::new(xs[4] + j as f32 * (bw + self.px(4.0)), mid - self.px(9.0), bw, self.px(18.0));
                            let usable = (*d < 0 && k > 0) || (*d > 0 && k + 1 < n);
                            scene.outline(r, self.px(m::HAIRLINE), if usable { ink } else { t.tint });
                            let word = if *d < 0 { "↑" } else { "↓" };
                            let _ = icon;
                            let w = self.fonts.measure(ui, word);
                            self.fonts.draw(scene, Style { color: if usable { ink } else { t.dim }, ..ui }, r.x + (r.w - w) * 0.5, r.y + self.px(13.5), word);
                            if usable { self.settings_hits.push((r, Hit::Workspace(W::Move(src, *d)))); }
                        }
                        scene.hline(cx, ry + self.px(32.0), maxw, self.px(m::HAIRLINE), t.tint);
                    }
                }
                Control::Stepper(value, minus, plus) => {
                    let bw = self.px(22.0);
                    let r1 = Rect::new(vx, base - self.px(15.0), bw, self.px(20.0));
                    let vw = self.fonts.measure(strong, &value).max(self.px(24.0)) + self.px(16.0);
                    let r2 = Rect::new(r1.right() + vw, r1.y, bw, r1.h);
                    for (r, word, hit) in [(r1, "−", minus), (r2, "+", plus)] {
                        scene.outline(r, self.px(m::HAIRLINE), ink);
                        let w = self.fonts.measure(ui, word);
                        self.fonts.draw(scene, ui, r.x + (r.w - w) * 0.5, r.y + self.px(14.5), word);
                        self.settings_hits.push((r, hit));
                    }
                    let w = self.fonts.measure(strong, &value);
                    self.fonts.draw(scene, strong, r1.right() + (vw - w) * 0.5, base, &value);
                }
                Control::Strip(items) => {
                    // Neobrutal tab strip: outlined chips, the current one filled with a hard shadow.
                    let mut x = cx;
                    let ch = self.px(34.0);
                    let mut sy = y + cap_h + self.px(5.0);
                    for (text, hit, on) in items {
                        let w = (self.fonts.measure(strong, &text) + self.px(28.0) + 1.0).ceil().min(maxw);
                        if x > cx && x+w > cx+maxw {x=cx;sy+=self.px(44.0);}
                        let text=self.fit(strong,&text,(w-self.px(28.0)).max(1.0));
                        let chip = Rect::new(x, sy, w, ch);
                        let hot = chip.contains(self.mouse.0, self.mouse.1);
                        let off = if on { self.px(4.0) } else if hot { self.px(3.0) } else { 0.0 };
                        if off > 0.0 {
                            scene.rect(Rect::new(chip.x + off, chip.y + off, chip.w, chip.h), if on { self.surface.signal } else { ink });
                        }
                        scene.rect(chip, if on { ink } else { t.paper });
                        scene.outline(chip, self.px(m::STRUCTURE), ink);
                        let st = Style { color: if on { self.on_fill(ink) } else { ink }, ..strong };
                        self.fonts.draw(scene, st, x + self.px(14.0), sy + ch / 2.0 + self.px(4.0), &text);
                        self.settings_hits.push((chip, hit));
                        x += w + self.px(10.0);
                    }
                }
                Control::Cards(cards) => {
                    let mut x = cx;
                    let mut cy = y + cap_h;
                    let mut n = 0;
                    for (name, ramp, signal, angle, hit, on, faces) in cards {
                        if n > 0 && n % per_row == 0 {
                            x = cx;
                            cy += card_h + self.px(8.0) + gap;
                        }
                        let card = Rect::new(x, cy, card_w, card_h);
                        self.draw_card(scene, card, &name, &ramp, signal, angle, on, faces, hover_key(&format!("card:{hit:?}"), row_index));
                        self.settings_hits.push((Rect::new(card.x, card.y, card.w + self.px(6.0), card.h + self.px(6.0)), hit));
                        x += card_w + gap;
                        n += 1;
                    }
                }
                Control::Art(cards) => {
                    let mut x = cx;
                    let mut cy = y + cap_h;
                    let mut n = 0;
                    for (key, name, says, hit, on, builtin) in cards {
                        if n > 0 && n % per_row == 0 {
                            x = cx;
                            cy += card_h + self.px(50.0) + gap;
                        }
                        let card = Rect::new(x, cy, card_w, card_h);
                        if card.bottom() + self.px(50.0) > content.y && card.y < content.bottom() {
                            self.draw_art_card(scene, card, &key, &name, &says, on, builtin, hit);
                            self.settings_hits.push((Rect::new(card.x, card.y, card.w + self.px(6.0), card.h + self.px(50.0)), hit));
                        }
                        x += card_w + gap;
                        n += 1;
                    }
                }
                Control::Tokens(items, big) => {
                    let (tw, th) = if big { (self.px(84.0), self.px(64.0)) } else { (self.px(34.0), self.px(34.0)) };
                    let tg = if big { self.px(16.0) } else { self.px(10.0) };
                    let per = ((maxw + tg) / (tw + tg)).floor().max(1.0) as usize;
                    let row_h = if big { th + self.px(34.0) + tg } else { th + tg };
                    let mut x = cx;
                    let mut ty = y + cap_h;
                    for (n, (name, color, caption, hit, on)) in items.into_iter().enumerate() {
                        if n > 0 && n % per == 0 {
                            x = cx;
                            ty += row_h;
                        }
                        let tile = Rect::new(x, ty, tw, th);
                        let hk = hover_key(&format!("tok:{hit:?}"), row_index * 256 + n);
                        self.draw_tile(scene, tile, color, on, hk);
                        if color.is_none() {
                            // An empty tile says what it does: add, remove, or none.
                            let icon = match name.as_str() { "ADD" => Some(icons::PLUS), "REMOVE" => Some(icons::MINIMIZE), "NONE" => Some(icons::CLOSE), _ => None };
                            if let Some(icon) = icon {
                                let isz = (th * 0.4).round();
                                self.fonts.draw_icon(scene, icon, isz, tile.x + ((tw - isz) / 2.0).round(), tile.y + ((th - isz) / 2.0).round(), ink);
                            }
                        }
                        if big {
                            let nm = self.fit(label, &name, tw + self.px(8.0));
                            self.fonts.draw(scene, Style { color: ink, ..label }, x, ty + th + self.px(18.0), &nm);
                            let cp = self.fit(dim, &caption, tw + self.px(8.0));
                            self.fonts.draw(scene, dim, x, ty + th + self.px(31.0), &cp);
                        }
                        self.settings_hits.push((Rect::new(tile.x - self.px(2.0), tile.y - self.px(2.0), tile.w + self.px(8.0), tile.h + self.px(8.0) + if big { self.px(30.0) } else { 0.0 }), hit));
                        x += tw + tg;
                    }
                }
                Control::Info(v) => {
                    let mut ly = base;
                    for line in crate::reader::wrap(&self.fonts, ui, &v, text_w) {
                        self.fonts.draw(scene, ui, vx, ly, &line);
                        ly += self.px(m::UI_PX * 1.5);
                    }
                }
                Control::Help(v) => {
                    let st = Style { color: t.dim, ..ui };
                    let mut ly = base;
                    for line in crate::reader::wrap(&self.fonts, st, &v, text_w) {
                        self.fonts.draw(scene, st, vx, ly, &line);
                        ly += self.px(m::UI_PX * 1.5);
                    }
                }
                Control::Keys(keys, note) => {
                    // Keycaps: outlined, a hard shadow, a + between; the note after.
                    let mut x = vx;
                    let ch = self.px(m::LABEL_PX) + self.px(12.0);
                    let cy = base - self.px(m::LABEL_PX) - self.px(6.0);
                    for (i, k) in keys.iter().enumerate() {
                        if i > 0 {
                            self.fonts.draw(scene, dim, x, base - self.px(1.0), "+");
                            x += self.px(10.0);
                        }
                        let w = self.fonts.measure(strong, k) + self.px(16.0);
                        let cap = Rect::new(x, cy, w, ch);
                        scene.rect(Rect::new(cap.x + self.px(2.0), cap.y + self.px(2.0), cap.w, cap.h), ink);
                        scene.rect(cap, t.paper);
                        scene.outline(cap, self.px(m::HAIRLINE), ink);
                        self.fonts.draw(scene, strong, x + self.px(8.0), base - self.px(1.0), k);
                        x += w + self.px(6.0);
                    }
                    // The notes line up in a column when the caps allow.
                    x = (x + self.px(8.0)).max(vx + self.px(236.0));
                    let mut ly = if tiles {base+self.px(34.0)} else {base};
                    if tiles {x=vx;}
                    for line in crate::reader::wrap(&self.fonts, dim, &note, (cx + maxw - x).max(self.px(80.0))) {
                        self.fonts.draw(scene, dim, x, ly, &line);
                        ly += self.px(m::UI_PX * 1.5);
                    }
                }
                Control::Cue(e, ci, note) => {
                    // The speaker: on, or slashed for quiet. Click toggles;
                    // quiet remembers nothing, so back on is the event's default cue.
                    let quiet = ci.is_none();
                    let isz = self.px(15.0);
                    let hit_r = Rect::new(vx - self.px(6.0), base - self.px(m::LABEL_PX) - self.px(8.0), isz + self.px(12.0), self.px(m::LABEL_PX) + self.px(16.0));
                    let default_ci = crate::sound::NAMES.iter().position(|n| *n == crate::sound::EVENTS[e].1).unwrap_or(0);
                    self.fonts.draw_icon(scene, if quiet { icons::SPEAKER_OFF } else { icons::SPEAKER }, isz, vx, base - isz + self.px(2.0), if quiet { t.dim } else { ink });
                    self.settings_hits.push((hit_r, if quiet { Hit::EventCue(e, default_ci) } else { Hit::EventCue(e, usize::MAX) }));
                    let mut x = vx + isz + self.px(16.0);
                    // The cue's name, a chip that plays it; a caret walks the palette.
                    let name = ci.map(|i| crate::sound::NAMES[i].caps()).unwrap_or_else(|| "QUIET".to_string());
                    let w = self.fonts.measure(label, &name) + self.px(20.0);
                    let chip = Rect::new(x, base - self.px(m::LABEL_PX) - self.px(6.0), w, self.px(m::LABEL_PX) + self.px(12.0));
                    if quiet {
                        scene.outline(chip, self.px(m::HAIRLINE), t.dim);
                        self.fonts.draw(scene, dim, x + self.px(10.0), base - self.px(1.0), &name);
                    } else {
                        scene.rect(chip, ink);
                        self.fonts.draw(scene, Style { color: self.on_fill(ink), ..label }, x + self.px(10.0), base - self.px(1.0), &name);
                        self.settings_hits.push((chip, Hit::Play(ci.unwrap_or(0))));
                    }
                    x += w + self.px(6.0);
                    let csz = self.px(12.0);
                    let next = Rect::new(x, chip.y, csz + self.px(12.0), chip.h);
                    self.fonts.draw_icon(scene, icons::CARET_RIGHT, csz, x + self.px(6.0), base - csz + self.px(1.0), if quiet { t.dim } else { ink });
                    self.settings_hits.push((next, Hit::EventNext(e)));
                    x += next.w + self.px(10.0);
                    if !note.is_empty() {
                        let ns = self.fit(dim, &note, (cx + maxw - x).max(self.px(40.0)));
                        self.fonts.draw(scene, dim, x, base, &ns);
                    }
                }
                Control::Proof(runs) => {
                    let mut x = vx;
                    for (c, text) in runs {
                        let st = Style { color: c, ..ui };
                        x += self.fonts.draw(scene, st, x, base, &text);
                    }
                }
                Control::Tabs(rows) => {
                    // A 248px sidebar in miniature, one row per result.
                    let sw = self.px(248.0);
                    let rh_row = self.px(26.0);
                    let x0 = vx;
                    let mut ry = y + self.px(4.0);
                    scene.outline(Rect::new(x0, ry, sw, rh_row * rows.len() as f32), self.px(m::HAIRLINE), t.tint);
                    for (bg, sig, title, child) in rows {
                        let rr = Rect::new(x0, ry, sw, rh_row);
                        if let Some(b) = bg {
                            scene.rect(rr, b);
                        }
                        scene.rect(Rect::new(x0, ry, self.px(2.0), rh_row), sig.unwrap_or(t.dim));
                        let tx = x0 + if child { self.px(26.0) } else { self.px(12.0) };
                        if child {
                            scene.vline(x0 + self.px(14.0), ry, rh_row, self.px(m::HAIRLINE), t.dim);
                        }
                        let st = Style { color: ink, ..label };
                        self.fonts.draw(scene, st, tx, ry + self.px(17.0), &title);
                        ry += rh_row;
                    }
                }
                Control::Choice(opts) => {
                    let mut x = vx;
                    let mut cb = base;
                    for (text, hit, on) in opts {
                        let w = self.fonts.measure(label, &text) + self.px(20.0);
                        if x > vx && x + w > cx + maxw {
                            x = vx;
                            cb += self.px(m::LABEL_PX) + self.px(20.0);
                        }
                        let base = cb;
                        let chip = Rect::new(x, base - self.px(m::LABEL_PX) - self.px(6.0), w, self.px(m::LABEL_PX) + self.px(12.0));
                        if on {
                            scene.rect(chip, ink);
                        } else {
                            scene.outline(chip, self.px(m::HAIRLINE), ink);
                        }
                        let st = Style { color: if on { self.on_fill(ink) } else { ink }, ..label };
                        self.fonts.draw(scene, st, x + self.px(10.0), base - self.px(1.0), &text);
                        self.settings_hits.push((chip, hit));
                        x += w + self.px(8.0);
                    }
                }
                Control::Slider(kind, v, text) => {
                    let bw = self.px(200.0).min((text_w - self.px(100.0)).max(self.px(40.0)));
                    let bar = Rect::new(vx, base - self.px(6.0), bw, self.px(2.0));
                    scene.rect(bar, t.tint);
                    scene.rect(Rect::new(vx, bar.y, bw * v, bar.h), self.surface.signal);
                    let knob = self.px(10.0);
                    scene.rect(Rect::new(vx + bw * v - knob / 2.0, bar.y - knob / 2.0 + bar.h / 2.0, knob, knob), ink);
                    self.settings_hits.push((Rect::new(vx - knob, bar.y - self.px(12.0), bw + 2.0 * knob, self.px(26.0)), Hit::Slider(kind, vx, bw)));
                    let ts = self.fit(dim, &text, text_w - bw - self.px(20.0));
                    self.fonts.draw(scene, dim, vx + bw + self.px(20.0), base, &ts);
                }
                Control::Swatches(items) => {
                    let sz = self.px(18.0);
                    let mut x = vx;
                    let sy = y + self.px(12.0);
                    for (c, hit, on) in items {
                        let sw = Rect::new(x, sy, sz, sz);
                        match c {
                            Some(c) => scene.rect(sw, c),
                            None => {
                                scene.outline(sw, self.px(m::HAIRLINE), ink);
                                // "none": a diagonal hairline.
                                scene.push(nus_render::Instance::hazard(sw, sz, t.tint, t.paper, sz * 2.0));
                            }
                        }
                        if on {
                            scene.outline(Rect::new(x - self.px(3.0), sy - self.px(3.0), sz + self.px(6.0), sz + self.px(6.0)), self.px(m::STRUCTURE), ink);
                        }
                        self.settings_hits.push((Rect::new(x - self.px(4.0), sy - self.px(4.0), sz + self.px(8.0), sz + self.px(8.0)), hit));
                        x += sz + self.px(12.0);
                    }
                }
                Control::Buttons(items) => {
                    let mut x = vx;
                    let mut cb = base;
                    for (text, icon, hit) in items {
                        let isz = self.px(13.0);
                        let w = self.fonts.measure(strong, &text) + self.px(24.0) + isz + self.px(8.0);
                        if x > vx && x + w > cx + maxw {
                            x = vx;
                            cb += self.px(m::LABEL_PX) + self.px(24.0);
                        }
                        let base = cb;
                        let b = Rect::new(x, base - self.px(m::LABEL_PX) - self.px(8.0), w, self.px(m::LABEL_PX) + self.px(16.0));
                        scene.rect(Rect::new(b.x + self.px(3.0), b.y + self.px(3.0), b.w, b.h), ink);
                        scene.rect(b, t.paper);
                        scene.outline(b, self.px(m::STRUCTURE), ink);
                        self.fonts.draw_icon(scene, icon, isz, x + self.px(12.0), base - isz + self.px(2.0), ink);
                        self.fonts.draw(scene, strong, x + self.px(12.0) + isz + self.px(8.0), base, &text);
                        self.settings_hits.push((b, hit));
                        x += w + self.px(14.0);
                    }
                }
            }
            if !matches!(control_kind, 0) && !helped[row_index] {
                scene.hline(cx, y + rh - self.px(m::HAIRLINE), maxw, self.px(m::HAIRLINE), t.tint);
            }
            // The row under the pointer tells the preview what to point at;
            // a note row belongs to the setting above it.
            if let Some(&(_, h)) = self.settings_hits.get(row_hits) { last_row_hit = Some(h); }
            if content.contains(mx, my) && Rect::new(cx - self.px(8.0), y, maxw + self.px(16.0), rh).contains(mx, my) {
                live_focus = self.settings_hits.get(row_hits).map(|&(_, h)| h).or(last_row_hit);
            }
            y += rh;
        }
        // Remember the reach so the wheel can clamp.
        self.settings_reach = (y + scroll - top + self.px(40.0)).max(0.0);
        {
            let moving = self.gliding(crate::scrolling::Glider::Settings(self.drawing_tab));
            let reach = (self.settings_reach - p.rect.h + self.scale * 48.0).max(0.0) + content.h;
            self.draw_thumb(scene, content, scroll, reach, moving);
        }
        scene.layer(None);
        // Hits above or below the column are unreachable.
        for (hr, _) in self.settings_hits.iter_mut().skip(content_hit_start) {
            *hr = hr.intersect(&content);
        }
        self.settings_hits.retain(|(hr, _)| hr.w > 0.0 && hr.h > 0.0);
        if let Some(lr) = live_rect {
            scene.layer(Some(lr));
            self.draw_live(scene, lr, p.section, live_focus);
            scene.layer(None);
        }
        if let Some((r,_))=self.settings_focus.and_then(|i|self.settings_hits.get(i)){scene.outline(*r,self.px(3.0),self.surface.signal);}

        // RULES: the file itself, as far as it fits.
        if p.section == RULES {
            y += self.px(16.0);
            let code = Style { font: self.f.ui, px: self.px(11.5), color: ink, tracking: 0.0 };
            let lh = self.px(11.5 * 1.55);
            let src = self.rules.source.clone();
            let clip = Rect::new(cx, y, maxw, (r.bottom() - y - self.px(20.0)).max(0.0)).intersect(&content);
            scene.layer(Some(clip));
            let mut ly = y + self.px(12.0);
            for (n, line) in src.lines().enumerate() {
                if ly > clip.bottom() {
                    break;
                }
                self.fonts.draw(scene, Style { color: t.dim, ..code }, cx, ly, &format!("{:>3}", n + 1));
                let mut x = cx + self.px(36.0);
                let limit = cx + maxw;
                for (kind, tok) in luau_tokens(line) {
                    let color = match kind {
                        Tok::Comment => t.dim,
                        Tok::Keyword => crate::theme_edit::from_rgb(t.ansi[12]),
                        Tok::Str => crate::theme_edit::from_rgb(t.ansi[10]),
                        Tok::Num => crate::theme_edit::from_rgb(t.ansi[13]),
                        Tok::Name => crate::theme_edit::from_rgb(t.ansi[11]),
                        Tok::Plain => ink,
                    };
                    if x >= limit {
                        break;
                    }
                    x += self.fonts.draw(scene, Style { color, ..code }, x, ly, &tok);
                }
                ly += lh;
            }
            scene.layer(None);
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tok {
    Comment,
    Keyword,
    Str,
    Num,
    Name,
    Plain,
}

/// A small Luau tokenizer for the RULES listing: comments, strings,
/// numbers, keywords, the name after `function`.
fn luau_tokens(line: &str) -> Vec<(Tok, String)> {
    const KW: [&str; 16] = ["function", "end", "if", "then", "else", "elseif", "return", "local", "and", "or", "not", "nil", "true", "false", "for", "in"];
    let mut out = Vec::new();
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    let mut after_function = false;
    while i < chars.len() {
        let c = chars[i];
        if c == '-' && chars.get(i + 1) == Some(&'-') {
            out.push((Tok::Comment, chars[i..].iter().collect()));
            break;
        }
        if c == '"' || c == '\'' {
            let q = c;
            let mut j = i + 1;
            while j < chars.len() && chars[j] != q {
                j += 1;
            }
            let end = (j + 1).min(chars.len());
            out.push((Tok::Str, chars[i..end].iter().collect()));
            i = end;
            continue;
        }
        if c.is_ascii_digit() {
            let mut j = i;
            while j < chars.len() && (chars[j].is_ascii_digit() || chars[j] == '.') {
                j += 1;
            }
            out.push((Tok::Num, chars[i..j].iter().collect()));
            i = j;
            continue;
        }
        if c.is_alphabetic() || c == '_' {
            let mut j = i;
            while j < chars.len() && (chars[j].is_alphanumeric() || chars[j] == '_') {
                j += 1;
            }
            let word: String = chars[i..j].iter().collect();
            let kind = if KW.contains(&word.as_str()) {
                Tok::Keyword
            } else if after_function {
                Tok::Name
            } else {
                Tok::Plain
            };
            after_function = word == "function";
            out.push((kind, word));
            i = j;
            continue;
        }
        let mut j = i;
        while j < chars.len() && !(chars[j].is_alphanumeric() || chars[j] == '_' || chars[j] == '"' || chars[j] == '\'' || (chars[j] == '-' && chars.get(j + 1) == Some(&'-'))) {
            j += 1;
        }
        if j == i {
            j = i + 1;
        }
        if !chars[i..j].iter().all(|c| c.is_whitespace()) {
            after_function = false;
        }
        out.push((Tok::Plain, chars[i..j].iter().collect()));
        i = j;
    }
    out
}

#[path = "settings_catalog.rs"]
mod catalog;

#[path = "settings_search.rs"]
mod search;

fn default_pip_skip() -> u16 { 10 }

#[cfg(test)]
mod pip_preferences_tests {
    use super::*;
    #[test]
    fn partial_behavior_keeps_explicit_values_and_initializes_missing_fields() {
        let value:Behavior=serde_json::from_value(serde_json::json!({"pip_skip_seconds":17,"hatch_background":false})).unwrap();
        assert_eq!(value.pip_skip_seconds,17);assert!(!value.hatch_background);assert_eq!(value.links,Behavior::default().links);
        let empty:Behavior=serde_json::from_str("{}").unwrap();assert_eq!(empty.pip_skip_seconds,10);
        assert!(serde_json::from_str::<Behavior>(r#"{"pip_skip_seconds":{}}"#).is_err());
        assert!(serde_json::from_str::<Behavior>(r#"{"pip_skip_seconds":[]}"#).is_err());
    }
}

impl App {
    fn viewer_settings(&self)->Vec<(String,Control)> {
        use crate::file_viewer::{Setting as S,Kind,Theme,Font};
        use Control::*;
        let p=&self.behavior.viewers;
        let toggle=|on:bool,yes:S,no:S|Choice(vec![("ON".into(),Hit::Viewer(yes),on),("OFF".into(),Hit::Viewer(no),!on)]);
        let mut rows=vec![("FILE VIEWERS".into(),toggle(p.enabled,S::Enabled(true),S::Enabled(false))),
            ("".into(),Info("Open supported local files as documents from Files or a file URL. Turn a format off to use its source. Right-click a document to Edit source. Changes apply to open viewers.".into()))];
        for k in Kind::ALL {let on=match k{Kind::Markdown=>p.markdown,Kind::Json=>p.json,Kind::Csv=>p.csv,Kind::Text=>p.text};rows.push((k.label().to_uppercase(),toggle(on,S::Format(k,true),S::Format(k,false))));}
        rows.extend([
            ("THEME".into(),Choice([(Theme::Follow,"FOLLOW APP"),(Theme::Paper,"PAPER"),(Theme::Ink,"INK")].into_iter().map(|(v,n)|(n.into(),Hit::Viewer(S::Theme(v)),p.theme==v)).collect())),
            ("BODY FONT".into(),Choice([(Font::Serif,"SERIF"),(Font::Sans,"SANS"),(Font::Mono,"MONO")].into_iter().map(|(v,n)|(n.into(),Hit::Viewer(S::Font(v)),p.font==v)).collect())),
            ("TEXT SIZE".into(),Choice([14,16,18,20,24,28].into_iter().map(|v|(format!("{v}"),Hit::Viewer(S::Size(v)),p.text_size==v)).collect())),
            ("PAGE WIDTH".into(),Choice([640,860,1100,1400].into_iter().map(|v|(format!("{v}"),Hit::Viewer(S::Width(v)),p.width==v)).collect())),
            ("LINE HEIGHT".into(),Choice([140,170,200].into_iter().map(|v|(format!("{v}%"),Hit::Viewer(S::LineHeight(v)),p.line_height==v)).collect())),
            ("WRAP TEXT & CODE".into(),toggle(p.wrap,S::Wrap(true),S::Wrap(false))),
            ("CONTENTS".into(),toggle(p.contents,S::Contents(true),S::Contents(false))),
            ("LOCAL IMAGES".into(),toggle(p.local_images,S::LocalImages(true),S::LocalImages(false))),
            ("REMOTE IMAGES".into(),toggle(p.remote_images,S::RemoteImages(true),S::RemoteImages(false))),
            ("".into(),Info("Relative images resolve beside the document, including ../ paths. Remote images contact the linked server when enabled. Embedded HTML is shown as text; document scripts are never run.".into())),
            ("CSV HEADER ROW".into(),toggle(p.csv_header,S::CsvHeader(true),S::CsvHeader(false))),
        ]);rows
    }
}

fn default_swipe_reach() -> u16 {
    180
}
