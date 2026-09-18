//! Theme editing: the two papers' tokens and the sixteen ANSI colours, as
//! overrides on Broadsheet's own; families (Broadsheet, Solarized, Gruvbox,
//! Nord, from the signal); import from Ghostty, Windows Terminal, base16
//! and VS Code theme files dropped in profile/themes; contrast readouts.

use nus_render::theme::{hex, Mode, Theme};
use nus_render::Color;
use serde::{Deserialize, Serialize};

use crate::surface::{family, mix, parse_hex, rotate_hue};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Family {
    Broadsheet,
    Solarized,
    Gruvbox,
    Nord,
    FromSignal,
    Imported,
}

impl Family {
    pub const ALL: [Family; 5] = [Family::Broadsheet, Family::Solarized, Family::Gruvbox, Family::Nord, Family::FromSignal];
    pub fn name(self) -> &'static str {
        match self {
            Family::Broadsheet => "broadsheet",
            Family::Solarized => "solarized",
            Family::Gruvbox => "gruvbox",
            Family::Nord => "nord",
            Family::FromSignal => "from signal",
            Family::Imported => "imported",
        }
    }
}

/// Per-mode overrides. None = Broadsheet's value.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ModeEdit {
    pub paper: Option<Color>,
    pub ink: Option<Color>,
    pub page: Option<Color>,
    pub ansi: Option<[Color; 16]>,
    /// The caret, when it isn't the ink; the selection's colour, when it
    /// isn't the ink (the wash is always 22%).
    #[serde(default)]
    pub caret: Option<Color>,
    #[serde(default)]
    pub selection: Option<Color>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ThemeEdit {
    pub ink: ModeEdit,
    pub paper: ModeEdit,
    #[serde(default = "default_family")]
    pub family: Family,
    /// Saturation multiplier for the sixteen, 0.5..1.5.
    #[serde(default = "default_sat")]
    pub saturation: f32,
}

fn default_family() -> Family {
    Family::Broadsheet
}
fn default_sat() -> f32 {
    1.0
}

impl Default for ThemeEdit {
    fn default() -> Self {
        ThemeEdit { ink: ModeEdit::default(), paper: ModeEdit::default(), family: Family::Broadsheet, saturation: 1.0 }
    }
}

fn to_rgb(c: Color) -> nus_vt::Rgb {
    nus_vt::Rgb { r: (c[0] * 255.0).round() as u8, g: (c[1] * 255.0).round() as u8, b: (c[2] * 255.0).round() as u8 }
}
pub fn from_rgb(c: nus_vt::Rgb) -> Color {
    [c.r as f32 / 255.0, c.g as f32 / 255.0, c.b as f32 / 255.0, 1.0]
}

/// The sixteen of a named family, for a mode.
pub fn family_ansi(f: Family, mode: Mode, signal: Color) -> Option<[Color; 16]> {
    let h = |v: u32| hex(v);
    Some(match (f, mode) {
        (Family::Broadsheet, _) | (Family::Imported, _) => return None,
        (Family::Solarized, Mode::Ink) => [
            h(0x073642), h(0xdc322f), h(0x859900), h(0xb58900), h(0x268bd2), h(0xd33682), h(0x2aa198), h(0xeee8d5),
            h(0x002b36), h(0xcb4b16), h(0x586e75), h(0x657b83), h(0x839496), h(0x6c71c4), h(0x93a1a1), h(0xfdf6e3),
        ],
        (Family::Solarized, Mode::Paper) => [
            h(0xeee8d5), h(0xdc322f), h(0x859900), h(0xb58900), h(0x268bd2), h(0xd33682), h(0x2aa198), h(0x073642),
            h(0xfdf6e3), h(0xcb4b16), h(0x93a1a1), h(0x839496), h(0x657b83), h(0x6c71c4), h(0x586e75), h(0x002b36),
        ],
        (Family::Gruvbox, Mode::Ink) => [
            h(0x282828), h(0xcc241d), h(0x98971a), h(0xd79921), h(0x458588), h(0xb16286), h(0x689d6a), h(0xa89984),
            h(0x928374), h(0xfb4934), h(0xb8bb26), h(0xfabd2f), h(0x83a598), h(0xd3869b), h(0x8ec07c), h(0xebdbb2),
        ],
        (Family::Gruvbox, Mode::Paper) => [
            h(0xfbf1c7), h(0xcc241d), h(0x98971a), h(0xd79921), h(0x458588), h(0xb16286), h(0x689d6a), h(0x7c6f64),
            h(0x928374), h(0x9d0006), h(0x79740e), h(0xb57614), h(0x076678), h(0x8f3f71), h(0x427b58), h(0x3c3836),
        ],
        (Family::Nord, _) => [
            h(0x3b4252), h(0xbf616a), h(0xa3be8c), h(0xebcb8b), h(0x81a1c1), h(0xb48ead), h(0x88c0d0), h(0xe5e9f0),
            h(0x4c566a), h(0xbf616a), h(0xa3be8c), h(0xebcb8b), h(0x81a1c1), h(0xb48ead), h(0x8fbcbb), h(0xeceff4),
        ],
        (Family::FromSignal, mode) => {
            // Six hues spun from the signal; black/white from the mode's papers.
            let base = if mode == Mode::Ink { Theme::ink() } else { Theme::paper() };
            let s = |turn: f32| rotate_hue(signal, turn);
            let hues = [s(0.0), s(0.33), s(0.16), s(0.6), s(0.83), s(0.5)]; // red green yellow blue magenta cyan
            let mut out = [base.paper; 16];
            out[0] = mix(base.paper, base.ink, 0.12);
            out[7] = mix(base.ink, base.paper, 0.25);
            out[8] = mix(base.paper, base.ink, 0.35);
            out[15] = base.ink;
            for (k, c) in hues.iter().enumerate() {
                out[1 + k] = *c;
                out[9 + k] = family(*c)[1];
            }
            out
        }
    })
}

fn saturate(c: Color, k: f32) -> Color {
    let l = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
    [
        (l + (c[0] - l) * k).clamp(0.0, 1.0),
        (l + (c[1] - l) * k).clamp(0.0, 1.0),
        (l + (c[2] - l) * k).clamp(0.0, 1.0),
        c[3],
    ]
}

/// WCAG contrast ratio.
pub fn contrast(a: Color, b: Color) -> f32 {
    let lum = |c: Color| {
        let f = |v: f32| if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
        0.2126 * f(c[0]) + 0.7152 * f(c[1]) + 0.0722 * f(c[2])
    };
    let (la, lb) = (lum(a) + 0.05, lum(b) + 0.05);
    if la > lb { la / lb } else { lb / la }
}

pub fn grade(ratio: f32) -> &'static str {
    if ratio >= 7.0 {
        "aaa"
    } else if ratio >= 4.5 {
        "aa"
    } else if ratio >= 3.0 {
        "large only"
    } else {
        "fails"
    }
}

impl ThemeEdit {
    fn edit(&self, mode: Mode) -> &ModeEdit {
        if mode == Mode::Ink { &self.ink } else { &self.paper }
    }
    pub fn edit_mut(&mut self, mode: Mode) -> &mut ModeEdit {
        if mode == Mode::Ink { &mut self.ink } else { &mut self.paper }
    }

    /// Broadsheet's theme for `mode`, with the edits applied and the
    /// derived tokens (dim, tint, hot, scrim) following.
    pub fn build(&self, mode: Mode, signal: Color) -> Theme {
        let mut t = if mode == Mode::Ink { Theme::ink() } else { Theme::paper() };
        let e = self.edit(mode);
        if let Some(p) = e.paper {
            t.paper = p;
        }
        if let Some(i) = e.ink {
            t.ink = i;
        }
        if let Some(p) = e.page {
            t.page = p;
        }
        t.caret = e.caret.unwrap_or(t.ink);
        t.selection = Theme::with_alpha(e.selection.unwrap_or(t.ink), 0.22);
        if e.paper.is_some() || e.ink.is_some() {
            t.dim = mix(t.ink, t.paper, 0.45);
            t.tint = Theme::with_alpha(t.ink, if mode == Mode::Ink { 0.07 } else { 0.06 });
            t.hot = Theme::with_alpha(t.ink, if mode == Mode::Ink { 0.14 } else { 0.12 });
            t.scrim = if mode == Mode::Ink { [0.0, 0.0, 0.0, 0.5] } else { Theme::with_alpha(t.paper, 0.55) };
        }
        let ansi: Option<[Color; 16]> = match self.family {
            Family::Imported | Family::Broadsheet => e.ansi,
            f => family_ansi(f, mode, signal),
        };
        if let Some(a) = ansi {
            for (i, c) in a.iter().enumerate() {
                t.ansi[i] = to_rgb(*c);
            }
        }
        if (self.saturation - 1.0).abs() > 0.01 {
            for i in 1..16 {
                if i != 7 && i != 8 && i != 15 {
                    t.ansi[i] = to_rgb(saturate(from_rgb(t.ansi[i]), self.saturation));
                }
            }
        }
        t
    }
}

// ── Import ───────────────────────────────────────────────────────────────

/// A theme read from a file: whatever it named.
#[derive(Clone, Debug, Default)]
pub struct Imported {
    pub name: String,
    pub paper: Option<Color>,
    pub ink: Option<Color>,
    pub ansi: Option<[Color; 16]>,
    pub format: &'static str,
}

pub fn themes_dir() -> std::path::PathBuf {
    std::env::current_dir().unwrap_or_default().join("profile").join("themes")
}

/// Theme files in profile/themes, parsed by shape, not extension.
pub fn imports() -> Vec<Imported> {
    let Ok(rd) = std::fs::read_dir(themes_dir()) else { return Vec::new() };
    let mut v: Vec<Imported> = rd
        .flatten()
        .filter_map(|e| {
            let text = std::fs::read_to_string(e.path()).ok()?;
            let name = e.path().file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            parse_theme(&name, &text)
        })
        .collect();
    v.sort_by(|a, b| a.name.cmp(&b.name));
    v
}

pub fn parse_theme(name: &str, text: &str) -> Option<Imported> {
    parse_windows_terminal(name, text)
        .or_else(|| parse_vscode(name, text))
        .or_else(|| parse_base16(name, text))
        .or_else(|| parse_ghostty(name, text))
}

fn all16(v: [Option<Color>; 16]) -> Option<[Color; 16]> {
    let mut out = [[0.0; 4]; 16];
    for (i, c) in v.iter().enumerate() {
        out[i] = (*c)?;
    }
    Some(out)
}

/// Ghostty: `palette = 3=#hex`, `background = #hex`, `foreground = #hex`.
fn parse_ghostty(name: &str, text: &str) -> Option<Imported> {
    let mut ansi = [None; 16];
    let mut out = Imported { name: name.into(), format: "ghostty", ..Default::default() };
    let mut any = false;
    for line in text.lines() {
        let line = line.trim();
        let Some((k, v)) = line.split_once('=') else { continue };
        let (k, v) = (k.trim(), v.trim());
        match k {
            "palette" => {
                if let Some((i, c)) = v.split_once('=') {
                    if let (Ok(i), Some(c)) = (i.trim().parse::<usize>(), parse_hex(c.trim())) {
                        if i < 16 {
                            ansi[i] = Some(c);
                            any = true;
                        }
                    }
                }
            }
            "background" => out.paper = parse_hex(v),
            "foreground" => out.ink = parse_hex(v),
            _ => {}
        }
    }
    if !any && out.paper.is_none() {
        return None;
    }
    out.ansi = all16(ansi);
    Some(out)
}

/// Windows Terminal scheme: {"name","black","red",…,"brightWhite","background","foreground"}.
fn parse_windows_terminal(name: &str, text: &str) -> Option<Imported> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    let obj = if v.get("black").is_some() { v.clone() } else { v.get("schemes")?.as_array()?.first()?.clone() };
    let g = |k: &str| obj.get(k).and_then(|x| x.as_str()).and_then(parse_hex);
    let keys = ["black", "red", "green", "yellow", "blue", "purple", "cyan", "white", "brightBlack", "brightRed", "brightGreen", "brightYellow", "brightBlue", "brightPurple", "brightCyan", "brightWhite"];
    let mut ansi = [None; 16];
    for (i, k) in keys.iter().enumerate() {
        ansi[i] = g(k);
    }
    let ansi = all16(ansi)?;
    Some(Imported { name: obj.get("name").and_then(|n| n.as_str()).unwrap_or(name).to_string(), paper: g("background"), ink: g("foreground"), ansi: Some(ansi), format: "windows terminal" })
}

/// VS Code theme: colors["terminal.ansiBlack"] … and editor.background/foreground.
fn parse_vscode(name: &str, text: &str) -> Option<Imported> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    let colors = v.get("colors")?;
    let g = |k: &str| colors.get(k).and_then(|x| x.as_str()).and_then(|s| parse_hex(&s[..s.len().min(7)]));
    let keys = ["Black", "Red", "Green", "Yellow", "Blue", "Magenta", "Cyan", "White", "BrightBlack", "BrightRed", "BrightGreen", "BrightYellow", "BrightBlue", "BrightMagenta", "BrightCyan", "BrightWhite"];
    let mut ansi = [None; 16];
    for (i, k) in keys.iter().enumerate() {
        ansi[i] = g(&format!("terminal.ansi{k}"));
    }
    let ansi = all16(ansi)?;
    Some(Imported { name: v.get("name").and_then(|n| n.as_str()).unwrap_or(name).to_string(), paper: g("editor.background"), ink: g("editor.foreground"), ansi: Some(ansi), format: "vs code" })
}

/// base16 YAML: base00: "hex" … base0F; the canonical ANSI mapping.
fn parse_base16(name: &str, text: &str) -> Option<Imported> {
    let mut b = [None; 16];
    let mut any = false;
    for line in text.lines() {
        let line = line.trim();
        let Some((k, v)) = line.split_once(':') else { continue };
        let k = k.trim();
        if let Some(idx) = k.strip_prefix("base") {
            if let Ok(i) = usize::from_str_radix(idx, 16) {
                let v = v.trim().trim_matches('"').trim_matches('\'').trim_start_matches('#');
                if i < 16 {
                    b[i] = parse_hex(&format!("#{v}"));
                    any = true;
                }
            }
        }
    }
    if !any {
        return None;
    }
    let b = all16(b)?;
    // base16 → ANSI as the styling guide maps it.
    let ansi = [b[0], b[8], b[11], b[10], b[13], b[14], b[12], b[5], b[3], b[8], b[11], b[10], b[13], b[14], b[12], b[7]];
    Some(Imported { name: name.into(), paper: Some(b[0]), ink: Some(b[5]), ansi: Some(ansi), format: "base16" })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contrast_grades() {
        assert!(contrast([0.0, 0.0, 0.0, 1.0], [1.0, 1.0, 1.0, 1.0]) > 20.0);
        assert_eq!(grade(21.0), "aaa");
        assert_eq!(grade(2.0), "fails");
    }

    #[test]
    fn parses_ghostty_and_wt() {
        let g = "background = #141414\nforeground = #ece7da\npalette = 0=#000000\npalette = 1=#ff0000\n";
        let t = parse_theme("x", g).unwrap();
        assert_eq!(t.format, "ghostty");
        assert!(t.ansi.is_none() && t.paper.is_some());
        let wt = r##"{"name":"Mine","black":"#000000","red":"#ff0000","green":"#00ff00","yellow":"#ffff00","blue":"#0000ff","purple":"#ff00ff","cyan":"#00ffff","white":"#ffffff","brightBlack":"#555555","brightRed":"#ff5555","brightGreen":"#55ff55","brightYellow":"#ffff55","brightBlue":"#5555ff","brightPurple":"#ff55ff","brightCyan":"#55ffff","brightWhite":"#ffffff","background":"#101010","foreground":"#f0f0f0"}"##;
        let t = parse_theme("y", wt).unwrap();
        assert_eq!(t.format, "windows terminal");
        assert_eq!(t.name, "Mine");
        assert!(t.ansi.is_some());
    }

    #[test]
    fn families_build() {
        for f in Family::ALL {
            let e = ThemeEdit { family: f, ..Default::default() };
            let t = e.build(Mode::Ink, nus_render::theme::signal::RED);
            assert_eq!(t.ansi.len(), 16);
        }
    }
}
