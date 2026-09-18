//! Surface: what the window is made of. The Broadsheet tokens are fixed; the
//! surface is the user's — a base colour, how much of it, texture, opacity,
//! the carapace — plus Luau rules that colour new tabs and Spaces.
//!
//! Rules live in `profile/rules.luau` (v1: `~/.config/nus/rules.luau`) and
//! run sandboxed: no io, no os, no require. A rule is a function that gets a
//! context table and returns a table of overrides, or nothing.

use nus_render::Color;
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Shell {
    Band,
    Stroke,
    Gradient,
    Aurora,
}

impl Shell {
    pub fn next(self) -> Shell {
        match self {
            Shell::Band => Shell::Stroke,
            Shell::Stroke => Shell::Gradient,
            Shell::Gradient => Shell::Aurora,
            Shell::Aurora => Shell::Band,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Shell::Band => "band",
            Shell::Stroke => "stroke",
            Shell::Gradient => "gradient",
            Shell::Aurora => "aurora",
        }
    }
    pub const ALL: [Shell; 4] = [Shell::Band, Shell::Stroke, Shell::Gradient, Shell::Aurora];
}

/// Where the sidebar lives and how it shows itself.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Side {
    Left,
    Right,
}

/// Which edge hovers reveal it: the screen edge counts (so a flick from the
/// desktop works), or only movement that starts inside the window.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum HoverFrom {
    ScreenEdge,
    InsideWindow,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Fullscreen {
    /// Hover reveals it, as windowed.
    Hover,
    /// Never shown; chords still work.
    Hidden,
    /// Stays open.
    Pinned,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SidebarRules {
    pub side: Side,
    pub hover_from: HoverFrom,
    pub fullscreen: Fullscreen,
    /// Milliseconds the sidebar stays after the pointer leaves it.
    pub grace_ms: u64,
    /// A 48px column of icons; the top strip hides until hovered.
    #[serde(default)]
    pub compact: bool,
}

impl Default for SidebarRules {
    fn default() -> Self {
        SidebarRules { side: Side::Left, hover_from: HoverFrom::ScreenEdge, fullscreen: Fullscreen::Hover, grace_ms: 300, compact: false }
    }
}

/// A named swatch the picker offers. The six signals plus paper and ink.
pub const SWATCHES: [(&str, Color); 8] = [
    ("red", nus_render::theme::signal::RED),
    ("blue", nus_render::theme::signal::BLUE),
    ("gold", nus_render::theme::signal::GOLD),
    ("green", nus_render::theme::signal::GREEN),
    ("violet", nus_render::theme::signal::VIOLET),
    ("teal", nus_render::theme::signal::TEAL),
    ("paper", [0.957, 0.945, 0.918, 1.0]),
    ("ink", [0.078, 0.078, 0.078, 1.0]),
];

/// Which surfaces the window opacity thins.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum OpacityOn {
    Panes,
    Chrome,
    Window,
}

/// The texture family. Grain is speckle; the rest are patterns.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum TextureKind {
    None,
    Grain,
    Stipple,
    Stitch,
    Linen,
    Halftone,
}

impl TextureKind {
    pub const ALL: [TextureKind; 6] = [TextureKind::None, TextureKind::Grain, TextureKind::Stipple, TextureKind::Stitch, TextureKind::Linen, TextureKind::Halftone];
    pub fn name(self) -> &'static str {
        match self {
            TextureKind::None => "none",
            TextureKind::Grain => "grain",
            TextureKind::Stipple => "stipple",
            TextureKind::Stitch => "stitch",
            TextureKind::Linen => "linen",
            TextureKind::Halftone => "halftone",
        }
    }
    /// The quad-shader kind, or None for no texture.
    pub fn shader_kind(self) -> Option<u32> {
        match self {
            TextureKind::None => None,
            TextureKind::Grain => Some(6),
            TextureKind::Stipple => Some(7),
            TextureKind::Stitch => Some(8),
            TextureKind::Linen => Some(9),
            TextureKind::Halftone => Some(10),
        }
    }
}

/// Where a texture is laid.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum TextureOn {
    Carapace,
    Chrome,
    Panes,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Surface {
    /// The signal colour: carapace, Space square, ticks, progress.
    pub signal: Color,
    /// Ordered stops the gradient and aurora carapaces run through. Empty
    /// means "signal → ink" as before.
    #[serde(default)]
    pub stops: Vec<Color>,
    /// Optional base the paper is tinted toward, and how far (0 = pure paper).
    pub base: Option<Color>,
    pub tint: f32,
    /// Texture strength 0..0.3; 0 = none.
    pub texture: f32,
    #[serde(default = "default_texture_kind")]
    pub texture_kind: TextureKind,
    /// Pattern pitch in logical px.
    #[serde(default = "default_texture_scale")]
    pub texture_scale: f32,
    #[serde(default = "default_texture_on")]
    pub texture_on: TextureOn,
    /// Animated: grain flickers like film, patterns drift.
    #[serde(default)]
    pub texture_motion: bool,
    /// Window opacity 0.5..1. Below 1 the panes show the desktop through.
    pub opacity: f32,
    #[serde(default = "default_opacity_on")]
    pub opacity_on: OpacityOn,
    pub shell: Shell,
    pub shell_width: f32,
    pub shell_radius: f32,
    /// Gradient direction in degrees (0 = left→right).
    #[serde(default = "default_angle")]
    pub angle: f32,
    /// Aurora: how fast the ramp drifts (turns per second) and how much the
    /// stroke breathes (0..1 of its width).
    #[serde(default = "default_drift")]
    pub drift: f32,
    #[serde(default)]
    pub breath: f32,
}

fn default_texture_kind() -> TextureKind {
    TextureKind::Grain
}
fn default_texture_scale() -> f32 {
    1.0
}
fn default_texture_on() -> TextureOn {
    TextureOn::Carapace
}
fn default_opacity_on() -> OpacityOn {
    OpacityOn::Panes
}
fn default_angle() -> f32 {
    30.0
}
fn default_drift() -> f32 {
    0.09
}

impl Default for Surface {
    fn default() -> Self {
        Surface {
            signal: nus_render::theme::signal::RED,
            stops: Vec::new(),
            base: None,
            tint: 0.0,
            texture: 0.08,
            texture_kind: TextureKind::Grain,
            texture_scale: 1.0,
            texture_on: TextureOn::Carapace,
            texture_motion: false,
            opacity: 1.0,
            opacity_on: OpacityOn::Panes,
            shell: Shell::Band,
            shell_width: nus_render::theme::metric::BAND,
            shell_radius: 0.0,
            angle: 30.0,
            drift: 0.09,
            breath: 0.0,
        }
    }
}

/// A saved surface: the card as a file under profile/surfaces.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Preset {
    pub name: String,
    pub surface: Surface,
}

fn presets_dir() -> std::path::PathBuf {
    std::env::current_dir().unwrap_or_default().join("profile").join("surfaces")
}

/// Built-in presets, then the saved ones (files win on a name clash).
pub fn presets() -> Vec<Preset> {
    use nus_render::theme::signal;
    let mut v = vec![
        Preset { name: "broadsheet".into(), surface: Surface::default() },
        Preset {
            name: "midnight".into(),
            surface: Surface {
                signal: signal::BLUE,
                stops: vec![signal::BLUE, signal::VIOLET, signal::TEAL],
                base: Some(signal::BLUE),
                tint: 0.22,
                shell: Shell::Aurora,
                shell_width: 4.0,
                shell_radius: 12.0,
                texture: 0.05,
                texture_kind: TextureKind::Linen,
                texture_on: TextureOn::Carapace,
                ..Surface::default()
            },
        },
        Preset {
            name: "ledger".into(),
            surface: Surface {
                signal: signal::GREEN,
                stops: vec![signal::GREEN, signal::GOLD],
                shell: Shell::Stroke,
                shell_width: 3.0,
                texture: 0.12,
                texture_kind: TextureKind::Stitch,
                texture_scale: 6.0,
                texture_on: TextureOn::Carapace,
                ..Surface::default()
            },
        },
        Preset {
            name: "darkroom".into(),
            surface: Surface {
                signal: signal::RED,
                stops: vec![signal::RED, [0.55, 0.05, 0.12, 1.0], signal::VIOLET],
                shell: Shell::Gradient,
                shell_width: 8.0,
                angle: 90.0,
                texture: 0.16,
                texture_kind: TextureKind::Halftone,
                texture_scale: 4.0,
                texture_on: TextureOn::Carapace,
                texture_motion: true,
                ..Surface::default()
            },
        },
    ];
    if let Ok(rd) = std::fs::read_dir(presets_dir()) {
        let mut files: Vec<_> = rd.flatten().collect();
        files.sort_by_key(|e| e.file_name());
        for e in files {
            if let Ok(text) = std::fs::read_to_string(e.path()) {
                if let Ok(p) = serde_json::from_str::<Preset>(&text) {
                    v.retain(|q| q.name != p.name);
                    v.push(p);
                }
            }
        }
    }
    v
}

pub fn save_preset(p: &Preset) -> std::io::Result<()> {
    std::fs::create_dir_all(presets_dir())?;
    let safe: String = p.name.chars().map(|c| if c.is_alphanumeric() || c == '-' { c } else { '-' }).collect();
    std::fs::write(presets_dir().join(format!("{safe}.json")), serde_json::to_string_pretty(p).unwrap_or_default())
}

/// Five steps of a colour: two tints, itself, two shades.
pub fn family(c: Color) -> [Color; 5] {
    let white = [1.0, 1.0, 1.0, 1.0];
    let black = [0.0, 0.0, 0.0, 1.0];
    [mix(c, white, 0.72), mix(c, white, 0.38), c, mix(c, black, 0.32), mix(c, black, 0.62)]
}

impl Surface {
    /// The stops the ramp carapaces use: the user's, or signal → ink.
    pub fn ramp(&self, ink: Color) -> Vec<Color> {
        if self.stops.len() >= 2 {
            self.stops.iter().take(4).copied().collect()
        } else {
            vec![self.signal, ink]
        }
    }
}

impl Surface {
    /// The paper the chrome draws on: theme paper pulled toward `base`.
    pub fn paper(&self, theme_paper: Color) -> Color {
        match self.base {
            Some(b) if self.tint > 0.0 => mix(theme_paper, b, self.tint),
            _ => theme_paper,
        }
    }
}

pub fn mix(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t, a[3] + (b[3] - a[3]) * t]
}

pub fn hex(c: Color) -> String {
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", q(c[0]), q(c[1]), q(c[2]))
}

pub fn parse_hex(s: &str) -> Option<Color> {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(s, 16).ok()?;
    Some([((v >> 16) & 255) as f32 / 255.0, ((v >> 8) & 255) as f32 / 255.0, (v & 255) as f32 / 255.0, 1.0])
}

/// Shift a colour's hue by `turns` (0..1) keeping saturation and lightness.
pub fn rotate_hue(c: Color, turns: f32) -> Color {
    let (h, s, l) = to_hsl(c);
    from_hsl((h + turns).rem_euclid(1.0), s, l, c[3])
}

pub fn to_hsl(c: Color) -> (f32, f32, f32) {
    let (r, g, b) = (c[0], c[1], c[2]);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    if (max - min).abs() < 1e-6 {
        return (0.0, 0.0, l);
    }
    let d = max - min;
    let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
    let h = if max == r {
        ((g - b) / d + if g < b { 6.0 } else { 0.0 }) / 6.0
    } else if max == g {
        ((b - r) / d + 2.0) / 6.0
    } else {
        ((r - g) / d + 4.0) / 6.0
    };
    (h, s, l)
}

pub fn from_hsl(h: f32, s: f32, l: f32, a: f32) -> Color {
    if s == 0.0 {
        return [l, l, l, a];
    }
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    let f = |mut t: f32| {
        if t < 0.0 {
            t += 1.0;
        }
        if t > 1.0 {
            t -= 1.0;
        }
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    [f(h + 1.0 / 3.0), f(h), f(h - 1.0 / 3.0), a]
}

// ── Rules ────────────────────────────────────────────────────────────────

/// What a rule can set on a new tab or Space.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Overrides {
    /// Pane background (terminal paper).
    pub bg: Option<Color>,
    /// Per-tab signal (row swatch, header chip).
    pub signal: Option<Color>,
}

/// A site boost: CSS and/or JS the rules hand a page.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Boost {
    pub css: Option<String>,
    pub js: Option<String>,
}

/// Context handed to `new_tab` / `new_space`.
pub struct TabCtx<'a> {
    pub kind: &'a str,
    pub index: usize,
    pub profile: &'a str,
    pub space: &'a str,
    pub space_signal: Color,
    /// "ink" or "paper", so rules can pick lightness.
    pub theme: &'a str,
    /// The page's host, "" for shells.
    pub host: &'a str,
    pub parent: Option<&'a Overrides>,
    /// The theme's rule: "family" | "wheel" | "same".
    pub tab_colours: &'a str,
}

pub const DEFAULT_RULES: &str = r##"-- nus rules · Luau, sandboxed. Edit, save, and the settings tab reloads it.
--
-- new_tab(ctx) runs for every new tab. ctx has:
--   kind      "terminal" | "page" | "settings"
--   index     1-based position among top-level tabs
--   profile   shell profile name (terminals)
--   space     the Space's name
--   signal    the Space's signal colour as "#rrggbb"
--   theme     "ink" (dark) or "paper" (light)
--   parent    { bg = "#..", signal = "#.." } when the tab joins a stack
-- Return { bg = "#rrggbb", signal = "#rrggbb" } (either key optional) or nil.
--
-- Helpers: hue(hex, turns) rotates hue; mix(a, b, t) blends; hsl(h, s, l);
-- family(hex, 1..5) picks a tint (1–2), the colour (3) or a shade (4–5).

-- Example: every new terminal gets its own background hue, and pages opened
-- from it (its stack) stay in the same family, a touch lighter.
-- ctx.tab_colours is the theme's wish: "family" (tints and shades of the
-- signal), "wheel" (round the hue wheel from the signal), "same" (every
-- tab the signal). Themes set it; you can ignore it.
function new_tab(ctx)
  if ctx.parent then
    return { bg = ctx.parent.bg and mix(ctx.parent.bg, "#ffffff", 0.06) or nil,
             signal = ctx.parent.signal }
  end
  if ctx.kind == "terminal" then
    if ctx.tab_colours == "same" then
      return { signal = ctx.signal }
    end
    local light = ctx.theme == "ink" and 0.11 or 0.93
    if ctx.tab_colours == "family" then
      local step = ((ctx.index - 1) % 5) + 1
      return { bg = mix(family(ctx.signal, step), ctx.theme == "ink" and "#000000" or "#ffffff", 0.86),
               signal = family(ctx.signal, step) }
    end
    local turn = (ctx.index - 1) * 0.11
    return { bg = hsl(turn, 0.18, light), signal = hue(ctx.signal, turn) }
  end
end

-- new_space(ctx) · ctx has index and name. Return { signal = "#rrggbb" }.
function new_space(ctx)
  return { signal = hue("#c8102e", (ctx.index - 1) * 0.17) }
end

-- on_event(ev) picks a sound: ev.name is one of launch, tab.switch,
-- tab.close, sidebar.reveal, palette.open, palette.move, control.press,
-- control.release, toggle, page.ready, copied, bell, onboarding.tick,
-- hover. Return a cue name ("droplet"), false for quiet, or nil to keep
-- the setting.
function on_event(ev)
  if ev.name == "hover" then return false end
end

-- on_page(ctx) runs whenever a page's address changes: ctx has url and
-- host. Return { css = "…", js = "…" } to boost the site (either key
-- optional), or nil to leave it alone. Boosts run inside the page.
function on_page(ctx)
  if ctx.host == "example.com" then
    return { css = "body { font-family: 'IBM Plex Mono', ui-monospace, monospace; }" }
  end
end

-- chains: named lists of palette commands, run in order from the palette
-- ("chain review"). A step is anything the palette accepts, plus:
--   "open <url>"      a page in a new tab
--   "run <command>"   typed into the newest shell the chain opened (or
--                     the focused one), with Enter
--   "tile"            tiles the tabs the chain opened (up to four)
--   "compact" / "sidebar" / "new terminal" / "welcome" …
chains = {
  review = { "new terminal", "run git status", "open https://github.com/pulls", "tile" },
  docs = { "open https://docs.rs", "open https://developer.mozilla.org", "tile" },
}

-- folders: live folders in the sidebar, under the tabs. Each is a list of
-- { title, url, detail } or a function returning one (polled each minute;
-- os_hour is there for time-of-day lists). GITHUB and PORTS are built in.
folders = {
  reading = {
    { title = "Rust std", url = "https://doc.rust-lang.org/std/", detail = "docs" },
    { title = "MDN", url = "https://developer.mozilla.org/", detail = "web" },
  },
}

-- group(tab): the group tidy proposes for a tab, or nil for the default
-- (a page's host, a shell's project folder). tab has kind, title, url,
-- host, cwd, stacked.
function group(tab)
  if tab.host == "github.com" then return "github" end
  if tab.cwd:find("nus") then return "nus" end
end

-- skills: saved prompts for the assistant, each a chip in the panel and a
-- palette row (ask <name>). context picks what goes along: shell, block,
-- page, tabs, editor, memory. Leave it out to use the chips as they are.
skills = {
  explain = { prompt = "Explain what went wrong in the command in focus and how to fix it.", context = { "shell", "block" } },
  summarize = { prompt = "Summarize the page beside the shell in five lines.", context = { "page" } },
  commit = { prompt = "Write a conventional commit message for the staged changes; run `git diff --staged` first if you need to.", context = { "shell", "block" } },
  compare = { prompt = "Compare the open tabs: what each is for and which to read first.", context = { "tabs" } },
  port = { prompt = "What is running on the port named in the question, and how do I stop or restart it?", context = { "shell", "block" } },
}

-- nus.run(cmd, args): anything the nus command can do, from a rule —
-- nus.run("open", { url = "http://localhost:5173/", split = true }),
-- nus.run("theme", { name = "darkroom" }), nus.run("hatch", { ["do"] = "show" }).

-- on_block(b): a command finished. b has cmd, exit, lines, cwd. Return
-- { fold = true } to fold its output, { notify = true } to be told when
-- you're elsewhere. Long test runs fold themselves; failures notify.
function on_block(b)
  if b.cmd:find("^cargo test") and b.lines > 40 then return { fold = true } end
  if b.exit ~= 0 and b.exit ~= -1 then return { notify = true } end
end

-- program(p): a program is running in a shell; p has name ("claude",
-- "nvim"), cmd, cwd, theme ("ink" | "paper"), ink, paper, signal (hex).
-- Return nothing to leave it to the settings, or a table: contrast (a
-- WCAG ratio, 0 for as-they-come), snap (true: truecolour wears the
-- theme), ansi (sixteen hex strings, this program's own), remap
-- ({ ["#d97757"] = "signal" } — a colour it hardcodes, and ours; hex or
-- "ink" | "paper" | "signal" | "dim").
function program(p)
  -- claude's orange as this Space's signal, its greys graded a notch harder:
  -- if p.name == "claude" then return { contrast = 7, remap = { ["#d77757"] = "signal" } } end
end

-- ports: the board asks this for every port it finds. p has port, pid,
-- process, command, cwd, exposed, mine, udp. Return nothing, or a table:
-- name, tint ("#rrggbb"), open ("split" | "tab" | "peek" — when it
-- appears), tunnel (true), hide (true), watch (true).
function ports(p)
  if p.port == 5173 then return { name = "vite", open = "split" } end
  if p.process == "node" and p.exposed then return { tint = "#d9a400" } end
end
"##;

/// Starter rule sets the RULES page can write (each replaces new_tab and
/// new_space; on_page and on_event are kept from the default).
pub const STARTERS: [(&str, &str); 5] = [
    ("hue per tab", r##"-- hue per tab: every terminal its own hue; stacks stay in the family.
function new_tab(ctx)
  if ctx.parent then
    return { bg = ctx.parent.bg and mix(ctx.parent.bg, "#ffffff", 0.06) or nil, signal = ctx.parent.signal }
  end
  if ctx.kind == "terminal" then
    local turn = (ctx.index - 1) * 0.11
    local light = ctx.theme == "ink" and 0.11 or 0.93
    return { bg = hsl(turn, 0.18, light), signal = hue(ctx.signal, turn) }
  end
end
"##),
    ("family per stack", r##"-- family per stack: each top-level tab takes a tint of the signal;
-- its children step down the same family.
function new_tab(ctx)
  if ctx.parent then
    return { bg = ctx.parent.bg, signal = family(ctx.parent.signal or ctx.signal, 4) }
  end
  local step = ((ctx.index - 1) % 5) + 1
  return { signal = family(ctx.signal, step) }
end
"##),
    ("by host", r##"-- by host: pages colour by where they are; shells stay plain.
function new_tab(ctx)
  if ctx.kind ~= "page" then return nil end
  local h = ctx.host or ""
  if h:find("github") then return { signal = "#6b3fa0" } end
  if h:find("localhost") or h:find("127.0.0.1") then return { signal = "#d9a400" } end
  if h:find("docs") then return { signal = "#1a7f8a" } end
  return { signal = hue(ctx.signal, (#h % 7) / 7) }
end
"##),
    ("monochrome", r##"-- monochrome: everything in the signal; only lightness moves.
function new_tab(ctx)
  local light = ctx.theme == "ink" and (0.10 + (ctx.index % 4) * 0.015) or (0.94 - (ctx.index % 4) * 0.015)
  return { bg = hsl(0, 0, light), signal = ctx.signal }
end
"##),
    ("time of day", r##"-- time of day: warm in the morning, cool at night (hour from the clock).
function new_tab(ctx)
  local h = tonumber(os_hour or 12)
  local turn = (h / 24) * 0.9
  return { signal = hue(ctx.signal, turn) }
end
"##),
];

/// The Luau state: loaded from the rules file, re-read on demand.
/// A Luau value as JSON, for `nus.run` args (tables → objects or arrays).
fn lua_to_json(lua: &mlua::Lua, v: mlua::Value) -> serde_json::Value {
    match v {
        mlua::Value::Nil => serde_json::Value::Null,
        mlua::Value::Boolean(b) => serde_json::Value::Bool(b),
        mlua::Value::Integer(i) => serde_json::Value::from(i),
        mlua::Value::Number(n) => serde_json::Value::from(n),
        mlua::Value::String(s) => serde_json::Value::String(s.to_str().map(|s| s.to_string()).unwrap_or_default()),
        mlua::Value::Table(t) => {
            let is_array = t.raw_len() > 0;
            if is_array {
                serde_json::Value::Array(t.sequence_values::<mlua::Value>().filter_map(|v| v.ok()).map(|v| lua_to_json(lua, v)).collect())
            } else {
                let mut m = serde_json::Map::new();
                for (k, v) in t.pairs::<String, mlua::Value>().filter_map(|p| p.ok()) {
                    m.insert(k, lua_to_json(lua, v));
                }
                serde_json::Value::Object(m)
            }
        }
        _ => serde_json::Value::Null,
    }
}

/// What `on_block` asked for.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BlockVerdict {
    pub fold: Option<bool>,
    pub notify: bool,
}

pub struct Rules {
    lua: mlua::Lua,
    pub path: PathBuf,
    pub status: String,
    pub source: String,
    /// Commands a rule asked for through `nus.run(cmd, args)`; the app
    /// answers them after the hook returns (rules run on the app's thread).
    pub queued: std::rc::Rc<std::cell::RefCell<Vec<(String, serde_json::Value)>>>,
    /// `program(p)` answers, by (name, face); cleared on reload and on a
    /// theme change.
    programs: std::cell::RefCell<std::collections::HashMap<(String, bool), Option<ProgramLook>>>,
}

/// What `program(p)` asked for one program. Colours stay as the rule
/// wrote them (hex or a token word) and resolve against the live theme.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProgramLook {
    pub contrast: Option<f32>,
    pub snap: Option<bool>,
    pub ansi: Option<[String; 16]>,
    pub remap: Vec<(String, String)>,
}

impl Rules {
    pub fn path() -> PathBuf {
        std::env::current_dir().unwrap_or_default().join("profile").join("rules.luau")
    }

    pub fn load() -> Rules {
        let path = Rules::path();
        if !path.exists() {
            let _ = std::fs::create_dir_all(path.parent().unwrap());
            let _ = std::fs::write(&path, DEFAULT_RULES);
        }
        // Older files get the chains and folders examples appended, once each.
        for (word, marker) in [("chains", "-- chains:"), ("folders", "-- folders:"), ("group", "-- group(tab):"), ("skills", "-- skills:"), ("on_block", "-- on_block(b):"), ("program", "-- program(p):"), ("ports", "-- ports:")] {
            let Ok(src) = std::fs::read_to_string(&path) else { break };
            if src.contains(word) {
                continue;
            }
            let Some(k) = DEFAULT_RULES.find(marker) else { continue };
            let block = &DEFAULT_RULES[k..];
            let block = match word {
                "chains" => block.split("-- folders:").next().unwrap_or(block),
                "folders" => block.split("-- group(tab):").next().unwrap_or(block),
                "group" => block.split("-- skills:").next().unwrap_or(block),
                "skills" => block.split("-- nus.run(cmd, args):").next().unwrap_or(block),
                "on_block" => block.split("-- program(p):").next().unwrap_or(block),
                "program" => block.split("-- ports:").next().unwrap_or(block),
                _ => block,
            };
            let _ = std::fs::write(&path, format!("{}\n{}", src.trim_end(), block.trim_end()));
        }
        let mut r = Rules { lua: mlua::Lua::new(), path, status: String::new(), source: String::new(), queued: Default::default(), programs: Default::default() };
        r.reload();
        r
    }

    /// Replace new_tab / new_space with a starter, keeping on_page and
    /// on_event from the default file.
    pub fn write_starter(&mut self, k: usize) {
        let Some((_, body)) = STARTERS.get(k) else { return };
        let keep = DEFAULT_RULES.split("-- new_space(ctx)").nth(1).map(|rest| format!("-- new_space(ctx){rest}")).unwrap_or_default();
        let src = format!("-- nus rules · Luau, sandboxed. Starter: {}.\n\n{}\n{}", STARTERS[k].0, body, keep);
        let _ = std::fs::write(&self.path, src);
        self.reload();
    }

    pub fn reload(&mut self) {
        let src = std::fs::read_to_string(&self.path).unwrap_or_default();
        self.apply(src);
    }

    pub fn from_source(source: &str) -> Rules {
        let mut r = Rules { lua: mlua::Lua::new(), path: PathBuf::new(), status: String::new(), source: String::new(), queued: Default::default(), programs: Default::default() };
        r.apply(source.to_string());
        r
    }

    fn apply(&mut self, source: String) {
        self.source = source;
        self.programs.borrow_mut().clear();
        let lua = mlua::Lua::new();
        lua.sandbox(true).ok();
        let g = lua.globals();
        // Colour helpers.
        let _ = g.set(
            "hue",
            lua.create_function(|_, (h, t): (String, f32)| Ok(parse_hex(&h).map(|c| hex(rotate_hue(c, t))))).unwrap(),
        );
        let _ = g.set(
            "mix",
            lua.create_function(|_, (a, b, t): (String, String, f32)| {
                Ok(match (parse_hex(&a), parse_hex(&b)) {
                    (Some(a), Some(b)) => Some(hex(mix(a, b, t))),
                    _ => None,
                })
            })
            .unwrap(),
        );
        let _ = g.set(
            "family",
            lua.create_function(|_, (h, i): (String, i64)| Ok(parse_hex(&h).map(|c| hex(family(c)[(i.clamp(1, 5) - 1) as usize])))).unwrap(),
        );
        // nus.run(cmd, args): the remote-control verbs, from a rule. Queued
        // and answered once the hook returns; no reply comes back to Luau.
        {
            let queued = self.queued.clone();
            let nus = lua.create_table().unwrap();
            let _ = nus.set(
                "run",
                lua.create_function(move |lua, (cmd, args): (String, Option<mlua::Table>)| {
                    let v: serde_json::Value = match args {
                        Some(t) => lua_to_json(lua, mlua::Value::Table(t)),
                        None => serde_json::Value::Null,
                    };
                    queued.borrow_mut().push((cmd, v));
                    Ok(())
                })
                .unwrap(),
            );
            let _ = g.set("nus", nus);
        }
        // The hour, for time-of-day rules (no os library in the sandbox).
        let hour = std::process::Command::new(if cfg!(target_os = "windows") { "cmd" } else { "date" })
            .args(if cfg!(target_os = "windows") { vec!["/c", "echo %TIME%"] } else { vec!["+%H"] })
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| s.trim().split(':').next().and_then(|h| h.trim().parse::<i64>().ok()))
            .unwrap_or(12);
        let _ = g.set("os_hour", hour);
        let _ = g.set(
            "hsl",
            lua.create_function(|_, (h, s, l): (f32, f32, f32)| Ok(hex(from_hsl(h.rem_euclid(1.0), s, l, 1.0)))).unwrap(),
        );
        self.status = match lua.load(&self.source).set_name("rules.luau").exec() {
            Ok(()) => {
                let has = |n: &str| g.get::<mlua::Function>(n).is_ok();
                let mut names: Vec<String> = ["new_tab", "new_space", "on_page", "on_event"].into_iter().filter(|n| has(n)).map(String::from).collect();
                let n = g.get::<mlua::Table>("chains").map(|t| t.pairs::<String, mlua::Value>().count()).unwrap_or(0);
                if n > 0 {
                    names.push(format!("{n} chain{}", if n == 1 { "" } else { "s" }));
                }
                let n = g.get::<mlua::Table>("folders").map(|t| t.pairs::<String, mlua::Value>().count()).unwrap_or(0);
                if n > 0 {
                    names.push(format!("{n} folder{}", if n == 1 { "" } else { "s" }));
                }
                format!("ok · {}", names.join(" "))
            }
            Err(e) => first_line(&e.to_string()),
        };
        self.lua = lua;
    }

    /// The `folders` table: name → items (a list, or a function returning
    /// one), sorted by name.
    pub fn folders(&self) -> Vec<(String, Vec<crate::folders::Item>)> {
        let Ok(t) = self.lua.globals().get::<mlua::Table>("folders") else { return Vec::new() };
        let item = |v: mlua::Table| -> Option<crate::folders::Item> {
            let url: String = v.get("url").ok()?;
            let title: String = v.get("title").unwrap_or_else(|_| url.clone());
            let detail: String = v.get("detail").unwrap_or_default();
            Some(crate::folders::Item { title, url, detail })
        };
        let mut out: Vec<(String, Vec<crate::folders::Item>)> = t
            .pairs::<String, mlua::Value>()
            .filter_map(|p| p.ok())
            .filter_map(|(name, v)| {
                let list: mlua::Table = match v {
                    mlua::Value::Table(t) => t,
                    mlua::Value::Function(f) => f.call::<mlua::Table>(()).ok()?,
                    _ => return None,
                };
                let items: Vec<crate::folders::Item> = list.sequence_values::<mlua::Table>().filter_map(|r| r.ok()).filter_map(item).collect();
                Some((name, items))
            })
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    /// The `chains` table: name → steps, sorted by name.
    pub fn chains(&self) -> Vec<(String, Vec<String>)> {
        let Ok(t) = self.lua.globals().get::<mlua::Table>("chains") else { return Vec::new() };
        let mut out: Vec<(String, Vec<String>)> = t
            .pairs::<String, mlua::Table>()
            .filter_map(|p| p.ok())
            .map(|(name, steps)| (name, steps.sequence_values::<String>().filter_map(|s| s.ok()).collect()))
            .filter(|(_, s): &(String, Vec<String>)| !s.is_empty())
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    pub fn new_tab(&self, ctx: &TabCtx) -> Overrides {
        let Ok(f) = self.lua.globals().get::<mlua::Function>("new_tab") else { return Overrides::default() };
        let t = self.lua.create_table().unwrap();
        let _ = t.set("kind", ctx.kind);
        let _ = t.set("index", ctx.index + 1);
        let _ = t.set("profile", ctx.profile);
        let _ = t.set("space", ctx.space);
        let _ = t.set("signal", hex(ctx.space_signal));
        let _ = t.set("theme", ctx.theme);
        let _ = t.set("host", ctx.host);
        let _ = t.set("tab_colours", ctx.tab_colours);
        if let Some(p) = ctx.parent {
            let pt = self.lua.create_table().unwrap();
            if let Some(bg) = p.bg {
                let _ = pt.set("bg", hex(bg));
            }
            if let Some(s) = p.signal {
                let _ = pt.set("signal", hex(s));
            }
            let _ = t.set("parent", pt);
        }
        match f.call::<Option<mlua::Table>>(t) {
            Ok(Some(o)) => Overrides {
                bg: o.get::<String>("bg").ok().and_then(|s| parse_hex(&s)),
                signal: o.get::<String>("signal").ok().and_then(|s| parse_hex(&s)),
            },
            Ok(None) => Overrides::default(),
            Err(e) => {
                tracing::warn!("rules new_tab: {e}");
                Overrides::default()
            }
        }
    }

    /// The `group` hook: a name for a tab, or nil for the default (its host
    /// or its project folder). `group({ kind, title, url, cwd, stacked })`.
    pub fn group(&self, kind: &str, title: &str, url: &str, cwd: &str, stacked: bool) -> Option<String> {
        let Ok(f) = self.lua.globals().get::<mlua::Function>("group") else { return None };
        let t = self.lua.create_table().unwrap();
        let _ = t.set("kind", kind);
        let _ = t.set("title", title);
        let _ = t.set("url", url);
        let _ = t.set("host", crate::tidy::host_of(url));
        let _ = t.set("cwd", cwd);
        let _ = t.set("stacked", stacked);
        match f.call::<Option<String>>(t) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("rules group: {e}");
                None
            }
        }
    }

    /// The `on_tidy` hook: the groups about to be suggested (names and counts).
    pub fn on_tidy(&self, groups: &[crate::tidy::Group]) {
        let Ok(f) = self.lua.globals().get::<mlua::Function>("on_tidy") else { return };
        let list = self.lua.create_table().unwrap();
        for (i, g) in groups.iter().enumerate() {
            let t = self.lua.create_table().unwrap();
            let _ = t.set("name", g.name.as_str());
            let _ = t.set("count", g.tabs.len());
            let _ = list.set(i + 1, t);
        }
        if let Err(e) = f.call::<()>(list) {
            tracing::warn!("rules on_tidy: {e}");
        }
    }

    /// The `on_open_layout` hook: the layout about to open, as a table the
    /// rule may edit (space, tabs); what it returns is what opens.
    pub fn on_open_layout(&self, l: crate::layout_file::Layout) -> crate::layout_file::Layout {
        let Ok(f) = self.lua.globals().get::<mlua::Function>("on_open_layout") else { return l };
        let t = self.lua.create_table().unwrap();
        let _ = t.set("space", l.space.clone());
        let tabs = self.lua.create_table().unwrap();
        for (i, tab) in l.tabs.iter().enumerate() {
            let tt = self.lua.create_table().unwrap();
            let _ = tt.set("shell", tab.shell.clone());
            let _ = tt.set("cwd", tab.cwd.clone());
            let _ = tt.set("run", tab.run.clone());
            let _ = tt.set("page", tab.page.clone());
            let _ = tt.set("edit", tab.edit.clone());
            let _ = tt.set("beside", tab.beside);
            let _ = tt.set("name", tab.name.clone());
            let _ = tt.set("pinned", tab.pinned);
            let _ = tabs.set(i + 1, tt);
        }
        let _ = t.set("tabs", tabs);
        match f.call::<Option<mlua::Table>>(t) {
            Ok(Some(o)) => {
                let tab_of = |v: mlua::Table| crate::layout_file::LayoutTab {
                    shell: v.get("shell").ok(),
                    cwd: v.get("cwd").ok(),
                    run: v.get("run").ok(),
                    page: v.get("page").ok(),
                    edit: v.get("edit").ok(),
                    beside: v.get::<usize>("beside").ok(),
                    name: v.get("name").ok(),
                    pinned: v.get("pinned").unwrap_or(false),
                };
                crate::layout_file::Layout {
                    space: o.get("space").ok().or(l.space),
                    tabs: o.get::<mlua::Table>("tabs").map(|list| list.sequence_values::<mlua::Table>().filter_map(|r| r.ok()).map(tab_of).collect()).unwrap_or(l.tabs),
                    hatch: l.hatch,
                }
            }
            Ok(None) => l,
            Err(e) => {
                tracing::warn!("rules on_open_layout: {e}");
                l
            }
        }
    }

    /// The `skills` table: name → { prompt, context = {"shell","block","page","tabs","editor","memory"} }.
    pub fn skills(&self) -> Vec<crate::askctx::Skill> {
        let Ok(t) = self.lua.globals().get::<mlua::Table>("skills") else { return Vec::new() };
        let mut out: Vec<crate::askctx::Skill> = t
            .pairs::<String, mlua::Table>()
            .filter_map(|p| p.ok())
            .filter_map(|(name, v)| {
                let prompt: String = v.get("prompt").ok()?;
                let context: Vec<crate::askctx::Ctx> = v
                    .get::<mlua::Table>("context")
                    .ok()
                    .map(|c| {
                        c.sequence_values::<String>()
                            .filter_map(|s| s.ok())
                            .filter_map(|s| crate::askctx::Ctx::ALL.iter().copied().find(|c| c.key() == s))
                            .collect()
                    })
                    .unwrap_or_default();
                Some(crate::askctx::Skill { name, prompt, context })
            })
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    /// The `on_progress` hook: a shell's progress finished or errored.
    /// `on_progress({ state = "done"|"error", tab = "<title>" })`.
    pub fn on_progress(&self, state: &str, tab: &str) {
        let Ok(f) = self.lua.globals().get::<mlua::Function>("on_progress") else { return };
        let t = self.lua.create_table().unwrap();
        let _ = t.set("state", state);
        let _ = t.set("tab", tab);
        if let Err(e) = f.call::<()>(t) {
            tracing::warn!("rules on_progress: {e}");
        }
    }

    /// The `on_block` hook: a command finished. `b` has cmd, exit, lines,
    /// cwd, seconds. Return nothing, or `{ fold = true|false, notify = true }`.
    pub fn on_block(&self, b: &crate::blocks::Block, cwd: &str) -> BlockVerdict {
        let Ok(f) = self.lua.globals().get::<mlua::Function>("on_block") else { return BlockVerdict::default() };
        let t = self.lua.create_table().unwrap();
        let _ = t.set("cmd", b.cmd.as_str());
        let _ = t.set("exit", b.exit.unwrap_or(-1));
        let _ = t.set("lines", b.lines());
        let _ = t.set("cwd", cwd);
        match f.call::<Option<mlua::Table>>(t) {
            Ok(Some(o)) => BlockVerdict { fold: o.get::<bool>("fold").ok(), notify: o.get::<bool>("notify").unwrap_or(false) },
            Ok(None) => BlockVerdict::default(),
            Err(e) => {
                tracing::warn!("rules on_block: {e}");
                BlockVerdict::default()
            }
        }
    }

    /// The theme changed: `program(p)` answers built on it are stale.
    pub fn forget_programs(&self) {
        self.programs.borrow_mut().clear();
    }

    /// The `program` hook: a program is running; what it wears. Cached by
    /// (name, face) until the rules reload or the theme changes.
    pub fn program(&self, name: &str, cmd: &str, cwd: &str, theme: &nus_render::Theme, signal: Color) -> Option<ProgramLook> {
        let ink_face = theme.mode == nus_render::Mode::Ink;
        if let Some(hit) = self.programs.borrow().get(&(name.to_string(), ink_face)) {
            return hit.clone();
        }
        let look = (|| {
            let f = self.lua.globals().get::<mlua::Function>("program").ok()?;
            let t = self.lua.create_table().ok()?;
            let _ = t.set("name", name);
            let _ = t.set("cmd", cmd);
            let _ = t.set("cwd", cwd);
            let _ = t.set("theme", if ink_face { "ink" } else { "paper" });
            let _ = t.set("ink", hex(theme.ink));
            let _ = t.set("paper", hex(theme.paper));
            let _ = t.set("signal", hex(signal));
            match f.call::<Option<mlua::Table>>(t) {
                Ok(Some(o)) => {
                    let ansi = o.get::<Vec<String>>("ansi").ok().filter(|v| v.len() == 16).map(|v| std::array::from_fn(|i| v[i].clone()));
                    let remap = o.get::<mlua::Table>("remap").ok().map(|r| r.pairs::<String, String>().flatten().collect()).unwrap_or_default();
                    Some(ProgramLook { contrast: o.get::<f32>("contrast").ok(), snap: o.get::<bool>("snap").ok(), ansi, remap })
                }
                Ok(None) => None,
                Err(e) => {
                    tracing::warn!("rules program: {e}");
                    None
                }
            }
        })();
        self.programs.borrow_mut().insert((name.to_string(), ink_face), look.clone());
        look
    }

    /// The `ports` hook: given a row's facts, what to call it and do with it.
    /// `ports = function(p) if p.port == 5173 then return { name = "vite", open = "split" } end end`
    pub fn ports(&self, row: &crate::ports::Row) -> crate::ports::Rule {
        let Ok(f) = self.lua.globals().get::<mlua::Function>("ports") else { return crate::ports::Rule::default() };
        let t = self.lua.create_table().unwrap();
        let _ = t.set("port", row.port);
        let _ = t.set("pid", row.pid);
        let _ = t.set("process", row.process.as_str());
        let _ = t.set("command", row.cmdline.as_str());
        let _ = t.set("exposed", row.exposed);
        let _ = t.set("mine", row.group == crate::ports::Group::Mine);
        let _ = t.set("udp", row.proto == nus_pty::ports::Proto::Udp);
        if let Some(c) = &row.cwd {
            let _ = t.set("cwd", c.as_str());
        }
        match f.call::<Option<mlua::Table>>(t) {
            Ok(Some(o)) => crate::ports::Rule {
                name: o.get::<String>("name").ok(),
                tint: o.get::<String>("tint").ok().and_then(|s| parse_hex(&s)),
                open: o.get::<String>("open").ok(),
                tunnel: o.get::<bool>("tunnel").unwrap_or(false),
                hide: o.get::<bool>("hide").unwrap_or(false),
                watch: o.get::<bool>("watch").unwrap_or(false),
            },
            Ok(None) => crate::ports::Rule::default(),
            Err(e) => {
                tracing::warn!("rules ports: {e}");
                crate::ports::Rule::default()
            }
        }
    }

    /// What the rules want done to a page at `url`.
    pub fn on_page(&self, url: &str) -> Boost {
        let Ok(f) = self.lua.globals().get::<mlua::Function>("on_page") else { return Boost::default() };
        let host = url.split("//").nth(1).unwrap_or("").split('/').next().unwrap_or("").trim_start_matches("www.");
        let t = self.lua.create_table().unwrap();
        let _ = t.set("url", url);
        let _ = t.set("host", host);
        match f.call::<Option<mlua::Table>>(t) {
            Ok(Some(o)) => Boost { css: o.get::<String>("css").ok(), js: o.get::<String>("js").ok() },
            Ok(None) => Boost::default(),
            Err(e) => {
                tracing::warn!("rules on_page: {e}");
                Boost::default()
            }
        }
    }

    /// What the rules say an event should sound like: Some(Some(cue)) to
    /// pick one, Some(None) for quiet (`return false`), None to leave it.
    pub fn on_event(&self, event: &str) -> Option<Option<String>> {
        let f = self.lua.globals().get::<mlua::Function>("on_event").ok()?;
        let t = self.lua.create_table().ok()?;
        let _ = t.set("name", event);
        match f.call::<mlua::Value>(t) {
            Ok(mlua::Value::String(s)) => Some(Some(s.to_str().map(|s| s.to_string()).unwrap_or_default())),
            Ok(mlua::Value::Boolean(false)) => Some(None),
            _ => None,
        }
    }

    pub fn new_space(&self, index: usize, name: &str) -> Overrides {
        let Ok(f) = self.lua.globals().get::<mlua::Function>("new_space") else { return Overrides::default() };
        let t = self.lua.create_table().unwrap();
        let _ = t.set("index", index + 1);
        let _ = t.set("name", name);
        match f.call::<Option<mlua::Table>>(t) {
            Ok(Some(o)) => Overrides { bg: None, signal: o.get::<String>("signal").ok().and_then(|s| parse_hex(&s)) },
            _ => Overrides::default(),
        }
    }
}

pub(crate) fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or("").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_roundtrip() {
        let c = parse_hex("#c8102e").unwrap();
        assert_eq!(hex(c), "#c8102e");
    }

    #[test]
    fn hue_rotates() {
        let c = parse_hex("#ff0000").unwrap();
        assert_eq!(hex(rotate_hue(c, 1.0 / 3.0)), "#00ff00");
    }

    #[test]
    fn default_rules_run() {
        let r = Rules::from_source(DEFAULT_RULES);
        assert!(r.status.starts_with("ok"), "{}", r.status);
        let ctx = TabCtx { kind: "terminal", index: 1, profile: "pwsh", space: "nus", space_signal: nus_render::theme::signal::RED, theme: "ink", host: "", parent: None, tab_colours: "family" };
        let o = r.new_tab(&ctx);
        assert!(o.bg.is_some() && o.signal.is_some());
        let child = TabCtx { kind: "page", index: 1, profile: "", space: "nus", space_signal: nus_render::theme::signal::RED, theme: "ink", host: "", parent: Some(&o), tab_colours: "family" };
        let c = r.new_tab(&child);
        assert_eq!(c.signal, o.signal);
        assert!(r.new_tab(&TabCtx { kind: "page", parent: None, ..ctx }).bg.is_none());
    }

    #[test]
    fn boosts_by_host() {
        let r = Rules::from_source(DEFAULT_RULES);
        assert!(r.on_page("https://www.example.com/x").css.is_some());
        assert_eq!(r.on_page("https://docs.rs/"), Boost::default());
    }

    #[test]
    fn sandbox_blocks_io() {
        let r = Rules::from_source("function new_tab(c) return { bg = tostring(io) } end");
        assert!(r.status.starts_with("ok"));
        let ctx = TabCtx { kind: "terminal", index: 0, profile: "", space: "", space_signal: [0.0; 4], theme: "ink", host: "", parent: None, tab_colours: "family" };
        assert_eq!(r.new_tab(&ctx).bg, None);
    }
}
