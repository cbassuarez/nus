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
}

impl Default for SidebarRules {
    fn default() -> Self {
        SidebarRules { side: Side::Left, hover_from: HoverFrom::ScreenEdge, fullscreen: Fullscreen::Hover, grace_ms: 300 }
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

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Surface {
    /// The signal colour: carapace, Space square, ticks, progress.
    pub signal: Color,
    /// Optional base the paper is tinted toward, and how far (0 = pure paper).
    pub base: Option<Color>,
    pub tint: f32,
    /// Grain strength on the carapace and stroke; 0 = none. Never on content.
    pub texture: f32,
    /// Window opacity 0.5..1. Below 1 the panes show the desktop through.
    pub opacity: f32,
    pub shell: Shell,
    pub shell_width: f32,
    pub shell_radius: f32,
}

impl Default for Surface {
    fn default() -> Self {
        Surface {
            signal: nus_render::theme::signal::RED,
            base: None,
            tint: 0.0,
            texture: 0.08,
            opacity: 1.0,
            shell: Shell::Band,
            shell_width: nus_render::theme::metric::BAND,
            shell_radius: 0.0,
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

fn to_hsl(c: Color) -> (f32, f32, f32) {
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

fn from_hsl(h: f32, s: f32, l: f32, a: f32) -> Color {
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
    pub parent: Option<&'a Overrides>,
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
-- Helpers: hue(hex, turns) rotates hue; mix(a, b, t) blends; hsl(h, s, l).

-- Example: every new terminal gets its own background hue, and pages opened
-- from it (its stack) stay in the same family, a touch lighter.
function new_tab(ctx)
  if ctx.parent then
    return { bg = ctx.parent.bg and mix(ctx.parent.bg, "#ffffff", 0.06) or nil,
             signal = ctx.parent.signal }
  end
  if ctx.kind == "terminal" then
    local turn = (ctx.index - 1) * 0.11
    local light = ctx.theme == "ink" and 0.11 or 0.93
    return { bg = hsl(turn, 0.18, light), signal = hue(ctx.signal, turn) }
  end
end

-- new_space(ctx) · ctx has index and name. Return { signal = "#rrggbb" }.
function new_space(ctx)
  return { signal = hue("#c8102e", (ctx.index - 1) * 0.17) }
end

-- on_page(ctx) runs whenever a page's address changes: ctx has url and
-- host. Return { css = "…", js = "…" } to boost the site (either key
-- optional), or nil to leave it alone. Boosts run inside the page.
function on_page(ctx)
  if ctx.host == "example.com" then
    return { css = "body { font-family: 'IBM Plex Mono', ui-monospace, monospace; }" }
  end
end
"##;

/// The Luau state: loaded from the rules file, re-read on demand.
pub struct Rules {
    lua: mlua::Lua,
    pub path: PathBuf,
    pub status: String,
    pub source: String,
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
        let mut r = Rules { lua: mlua::Lua::new(), path, status: String::new(), source: String::new() };
        r.reload();
        r
    }

    pub fn reload(&mut self) {
        let src = std::fs::read_to_string(&self.path).unwrap_or_default();
        self.apply(src);
    }

    pub fn from_source(source: &str) -> Rules {
        let mut r = Rules { lua: mlua::Lua::new(), path: PathBuf::new(), status: String::new(), source: String::new() };
        r.apply(source.to_string());
        r
    }

    fn apply(&mut self, source: String) {
        self.source = source;
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
            "hsl",
            lua.create_function(|_, (h, s, l): (f32, f32, f32)| Ok(hex(from_hsl(h.rem_euclid(1.0), s, l, 1.0)))).unwrap(),
        );
        self.status = match lua.load(&self.source).set_name("rules.luau").exec() {
            Ok(()) => {
                let has = |n: &str| g.get::<mlua::Function>(n).is_ok();
                let names: Vec<&str> = ["new_tab", "new_space", "on_page"].into_iter().filter(|n| has(n)).collect();
                format!("ok · {}", names.join(" "))
            }
            Err(e) => first_line(&e.to_string()),
        };
        self.lua = lua;
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

fn first_line(s: &str) -> String {
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
        let ctx = TabCtx { kind: "terminal", index: 1, profile: "pwsh", space: "nus", space_signal: nus_render::theme::signal::RED, theme: "ink", parent: None };
        let o = r.new_tab(&ctx);
        assert!(o.bg.is_some() && o.signal.is_some());
        let child = TabCtx { kind: "page", index: 1, profile: "", space: "nus", space_signal: nus_render::theme::signal::RED, theme: "ink", parent: Some(&o) };
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
        let ctx = TabCtx { kind: "terminal", index: 0, profile: "", space: "", space_signal: [0.0; 4], theme: "ink", parent: None };
        assert_eq!(r.new_tab(&ctx).bg, None);
    }
}
