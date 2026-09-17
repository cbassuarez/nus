//! Stock themes. A theme is the whole look, audited across everything
//! the app draws from prefs:
//!
//!   · the surface — signal, ramp stops, base tint, texture (kind, scale,
//!     strength, where, motion), carapace (shell, width, radius), angle,
//!     drift, breath, opacity;
//!   · two faces — paper and ink — each with paper / ink / page tokens and
//!     a contrast-graded ANSI 16, so "follow OS" switches inside the theme;
//!   · the cursor's colour rule (ink, signal, the tab's own);
//!   · the loading bar's style and colour;
//!   · a sound signature: cues for the events that carry character
//!     (launch, ready, tab switch, bell), the rest left to the user;
//!   · a tab-colour rule the default rules.luau reads (`ctx.tab_colours`):
//!     "family" (tints and shades of the signal), "wheel" (round the hue
//!     wheel from the signal), "same" (every tab the signal).
//!
//! Not in a theme, by design: fonts, header structure, motion register,
//! keys — those are yours, not the look's. Saved themes use the same
//! shape, in profile/themes/<name>.json; ports arrive through the theme
//! import path and are listed here with a nus surface each.

use nus_render::theme::hex;
use nus_render::Color;

use crate::anim::{BarColor, BarStyle};
use crate::settings::CursorColor;
use crate::surface::{Shell, Surface, TextureKind, TextureOn};

/// One face of a theme: the mode's tokens and its sixteen.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Face {
    pub paper: Color,
    pub ink: Color,
    pub page: Color,
    /// None = Broadsheet's sixteen for that mode.
    pub ansi: Option<[Color; 16]>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct StockTheme {
    pub name: String,
    /// One line: what it is, where it's from.
    pub story: String,
    /// A port of someone else's palette (credited in `story`).
    #[serde(default)]
    pub port: bool,
    pub paper: Face,
    pub ink: Face,
    pub surface: Surface,
    pub cursor: CursorColor,
    pub bar: BarStyle,
    pub bar_color: BarColor,
    /// event → cue, for the events the theme has a voice on.
    #[serde(default)]
    pub sounds: Vec<(String, String)>,
    /// "family" | "wheel" | "same"
    #[serde(default = "default_tab_colours")]
    pub tab_colours: String,
    /// Which face the theme was drawn for; the other is its companion.
    #[serde(default)]
    pub prefers_ink: bool,
}

fn default_tab_colours() -> String {
    "family".into()
}

fn ansi(v: [u32; 16]) -> Option<[Color; 16]> {
    Some(v.map(hex))
}

fn face(paper: u32, ink: u32, page: u32, a: Option<[Color; 16]>) -> Face {
    Face { paper: hex(paper), ink: hex(ink), page: hex(page), ansi: a }
}

fn surface(signal: u32, stops: &[u32], shell: Shell, width: f32, radius: f32, tex: TextureKind, strength: f32, scale: f32) -> Surface {
    Surface {
        signal: hex(signal),
        stops: stops.iter().map(|&s| hex(s)).collect(),
        shell,
        shell_width: width,
        shell_radius: radius,
        texture: strength,
        texture_kind: tex,
        texture_scale: scale,
        texture_on: TextureOn::Carapace,
        ..Surface::default()
    }
}

fn theme(name: &str, story: &str, paper: Face, ink: Face, surface: Surface, cursor: CursorColor, bar: BarStyle, bar_color: BarColor, sounds: &[(&str, &str)], tab_colours: &str, prefers_ink: bool) -> StockTheme {
    StockTheme {
        name: name.into(),
        story: story.into(),
        port: false,
        paper,
        ink,
        surface,
        cursor,
        bar,
        bar_color,
        sounds: sounds.iter().map(|(e, c)| (e.to_string(), c.to_string())).collect(),
        tab_colours: tab_colours.into(),
        prefers_ink,
    }
}

// ── Sixteens for the originals that need their own ────────────────────

const PHOSPHOR: [u32; 16] = [0x0a0f0a, 0xd08c3a, 0x33ff66, 0xb8e05a, 0x4fc98a, 0x9fd68a, 0x5affc8, 0x9fd6a5, 0x2f4a33, 0xffb060, 0x66ff99, 0xd7ff7a, 0x7fe0a8, 0xc7f0b0, 0x8cffe0, 0xd9ffe4];
const AMBER: [u32; 16] = [0x120b02, 0xff6a3d, 0xffb000, 0xffd166, 0xe0a050, 0xf2b880, 0xffcc70, 0xd8b070, 0x4a3410, 0xff8a5c, 0xffc233, 0xffe08a, 0xf0be6a, 0xffd0a0, 0xffe0a0, 0xfff1cc];
const BLUEPRINT_INK: [u32; 16] = [0x0b2a4a, 0xf07a8a, 0x8fd3a8, 0xf2d27a, 0x7fc4ff, 0xc8a8ff, 0x6fe3f2, 0xd8e8f8, 0x3d5f85, 0xff9aa8, 0xa8f0c0, 0xffe39a, 0xa8d8ff, 0xdcc4ff, 0x9df0fb, 0xffffff];
const BLUEPRINT_PAPER: [u32; 16] = [0x0b2a4a, 0xb32a3f, 0x1f7a4a, 0x8a6a00, 0x1b58a8, 0x6f3fb8, 0x137a8a, 0x5a7390, 0x3d5f85, 0xd63a4f, 0x2a9a5a, 0xb08a00, 0x2b6fd0, 0x8a5ad8, 0x1f9aaa, 0x0b2a4a];
const ONYX: [u32; 16] = [0x000000, 0xe6e6e6, 0xbfbfbf, 0xdedede, 0xa6a6a6, 0xcccccc, 0xb3b3b3, 0xf2f2f2, 0x5c5c5c, 0xffffff, 0xd9d9d9, 0xf5f5f5, 0xc4c4c4, 0xe8e8e8, 0xd0d0d0, 0xffffff];
const ONYX_PAPER: [u32; 16] = [0xffffff, 0x1a1a1a, 0x404040, 0x262626, 0x595959, 0x333333, 0x4d4d4d, 0x0d0d0d, 0xa6a6a6, 0x000000, 0x2b2b2b, 0x121212, 0x3d3d3d, 0x1f1f1f, 0x353535, 0x000000];
const KYOTO_PAPER: [u32; 16] = [0x1c1a17, 0xc0392b, 0x4f7f3a, 0xb8860b, 0x2b5b8c, 0x8b3a62, 0x2a7a7a, 0x8a8478, 0x5a554c, 0xe8503f, 0x6a9a50, 0xd4a017, 0x3f77b0, 0xb05082, 0x3a9a9a, 0xf2eee6];
const KYOTO_INK: [u32; 16] = [0x1c1a17, 0xff6b57, 0x8fc17a, 0xe6c05a, 0x7aa6d8, 0xd08ab0, 0x78c8c8, 0xcfc8ba, 0x6b655a, 0xff8a78, 0xaad898, 0xf2d47a, 0x9cc0ea, 0xe4a8cc, 0x9ee0e0, 0xf2eee6];
const MANUSCRIPT_PAPER: [u32; 16] = [0x3b2f2f, 0x9b2c2c, 0x4a6b2f, 0x8a6a1a, 0x3b5a8a, 0x7a3f6a, 0x2f6a6a, 0x8f8478, 0x6b5f5f, 0xb84040, 0x5f8a3f, 0xa8862a, 0x4f72a8, 0x955287, 0x3f8888, 0xf6efe3];
const NEWSPRINT_PAPER: [u32; 16] = [0x1f1f1f, 0xa83232, 0x3b6e3b, 0x8a6a1e, 0x2f5f9a, 0x7a3a7a, 0x2a6a72, 0x7a7770, 0x5a5a5a, 0xc44444, 0x4f8a4f, 0xa8872a, 0x3f78b8, 0x965296, 0x3a8a94, 0xe9e6df];
const BAUHAUS_PAPER: [u32; 16] = [0x111111, 0xd7263d, 0x1b7f3b, 0x9a7a00, 0x1f4ea1, 0x8b2f97, 0x1f8a9a, 0x7a7a7a, 0x4d4d4d, 0xe84a5e, 0x2aa04c, 0xb08d00, 0x2f6fd0, 0xaa49b8, 0x2aa8bb, 0xf4f1ea];
const CONTRAST_PAPER: [u32; 16] = [0x000000, 0xb30000, 0x006400, 0x7a5c00, 0x0033b3, 0x800080, 0x006b6b, 0x333333, 0x555555, 0xd40000, 0x008000, 0x996f00, 0x0044e6, 0xa000a0, 0x008a8a, 0x000000];
const CONTRAST_INK: [u32; 16] = [0xffffff, 0xff6b6b, 0x7bff7b, 0xffe066, 0x7fb2ff, 0xff8cff, 0x7fffff, 0xdddddd, 0xbbbbbb, 0xff8f8f, 0xa0ffa0, 0xfff099, 0xa6c8ff, 0xffb0ff, 0xb0ffff, 0xffffff];

// ── Ports ─────────────────────────────────────────────────────────────

const SOLARIZED_DARK: [u32; 16] = [0x073642, 0xdc322f, 0x859900, 0xb58900, 0x268bd2, 0xd33682, 0x2aa198, 0xeee8d5, 0x002b36, 0xcb4b16, 0x586e75, 0x657b83, 0x839496, 0x6c71c4, 0x93a1a1, 0xfdf6e3];
const SOLARIZED_LIGHT: [u32; 16] = [0xeee8d5, 0xdc322f, 0x859900, 0xb58900, 0x268bd2, 0xd33682, 0x2aa198, 0x073642, 0xfdf6e3, 0xcb4b16, 0x93a1a1, 0x839496, 0x657b83, 0x6c71c4, 0x586e75, 0x002b36];
const GRUVBOX_DARK: [u32; 16] = [0x282828, 0xcc241d, 0x98971a, 0xd79921, 0x458588, 0xb16286, 0x689d6a, 0xa89984, 0x928374, 0xfb4934, 0xb8bb26, 0xfabd2f, 0x83a598, 0xd3869b, 0x8ec07c, 0xebdbb2];
const GRUVBOX_LIGHT: [u32; 16] = [0xfbf1c7, 0xcc241d, 0x98971a, 0xd79921, 0x458588, 0xb16286, 0x689d6a, 0x7c6f64, 0x928374, 0x9d0006, 0x79740e, 0xb57614, 0x076678, 0x8f3f71, 0x427b58, 0x3c3836];
const NORD_DARK: [u32; 16] = [0x3b4252, 0xbf616a, 0xa3be8c, 0xebcb8b, 0x81a1c1, 0xb48ead, 0x88c0d0, 0xe5e9f0, 0x4c566a, 0xbf616a, 0xa3be8c, 0xebcb8b, 0x81a1c1, 0xb48ead, 0x8fbcbb, 0xeceff4];
const NORD_LIGHT: [u32; 16] = [0xe5e9f0, 0xbf616a, 0x7d9a5f, 0xb08f3a, 0x5e81ac, 0x9a6f9a, 0x4f93a3, 0x4c566a, 0xd8dee9, 0xa8505a, 0x6f8c52, 0x9a7a2a, 0x4f7098, 0x8a5f8a, 0x3f8290, 0x2e3440];
const MOCHA: [u32; 16] = [0x45475a, 0xf38ba8, 0xa6e3a1, 0xf9e2af, 0x89b4fa, 0xf5c2e7, 0x94e2d5, 0xbac2de, 0x585b70, 0xf38ba8, 0xa6e3a1, 0xf9e2af, 0x89b4fa, 0xf5c2e7, 0x94e2d5, 0xa6adc8];
const LATTE: [u32; 16] = [0x5c5f77, 0xd20f39, 0x40a02b, 0xdf8e1d, 0x1e66f5, 0xea76cb, 0x179299, 0xacb0be, 0x6c6f85, 0xd20f39, 0x40a02b, 0xdf8e1d, 0x1e66f5, 0xea76cb, 0x179299, 0xbcc0cc];
const TOKYO_NIGHT: [u32; 16] = [0x15161e, 0xf7768e, 0x9ece6a, 0xe0af68, 0x7aa2f7, 0xbb9af7, 0x7dcfff, 0xa9b1d6, 0x414868, 0xf7768e, 0x9ece6a, 0xe0af68, 0x7aa2f7, 0xbb9af7, 0x7dcfff, 0xc0caf5];
const TOKYO_DAY: [u32; 16] = [0xe9e9ed, 0xf52a65, 0x587539, 0x8c6c3e, 0x2e7de9, 0x9854f1, 0x007197, 0x6172b0, 0xa1a6c5, 0xf52a65, 0x587539, 0x8c6c3e, 0x2e7de9, 0x9854f1, 0x007197, 0x3760bf];
const ROSE_MAIN: [u32; 16] = [0x26233a, 0xeb6f92, 0x31748f, 0xf6c177, 0x9ccfd8, 0xc4a7e7, 0xebbcba, 0xe0def4, 0x6e6a86, 0xeb6f92, 0x31748f, 0xf6c177, 0x9ccfd8, 0xc4a7e7, 0xebbcba, 0xe0def4];
const ROSE_DAWN: [u32; 16] = [0xf2e9e1, 0xb4637a, 0x286983, 0xea9d34, 0x56949f, 0x907aa9, 0xd7827e, 0x575279, 0x9893a5, 0xb4637a, 0x286983, 0xea9d34, 0x56949f, 0x907aa9, 0xd7827e, 0x575279];
const DRACULA: [u32; 16] = [0x21222c, 0xff5555, 0x50fa7b, 0xf1fa8c, 0xbd93f9, 0xff79c6, 0x8be9fd, 0xf8f8f2, 0x6272a4, 0xff6e6e, 0x69ff94, 0xffffa5, 0xd6acff, 0xff92df, 0xa4ffff, 0xffffff];
const ALUCARD: [u32; 16] = [0x3c3c3c, 0xcb3a2a, 0x14710a, 0x846e15, 0x644ac9, 0xa3144d, 0x036a96, 0x1f1f1f, 0x6c664b, 0xcb3a2a, 0x14710a, 0x846e15, 0x644ac9, 0xa3144d, 0x036a96, 0x000000];

/// The stock set: thirteen originals, then seven ports.
pub fn stock() -> Vec<StockTheme> {
    use CursorColor as C;
    let mut v = vec![
        theme(
            "broadsheet",
            "The newspaper: warm paper, ink, one red. The default, and the measure of the rest.",
            face(0xf4f1ea, 0x141414, 0xffffff, None),
            face(0x141414, 0xece7da, 0xffffff, None),
            Surface::default(),
            C::Ink, BarStyle::Comet, BarColor::Signal,
            &[], "family", false,
        ),
        theme(
            "newsprint",
            "The Sunday edition: grey stock, soft black, brick red, heavy grain on the band.",
            face(0xe9e6df, 0x232323, 0xf7f5f0, ansi(NEWSPRINT_PAPER)),
            face(0x1a1a1a, 0xd9d6cf, 0xf7f5f0, None),
            surface(0xa83232, &[0xa83232, 0x5a5a5a], Shell::Band, 6.0, 0.0, TextureKind::Grain, 0.22, 2.0),
            C::Ink, BarStyle::Rule, BarColor::Ink,
            &[("launch", "arrival"), ("page.ready", "tick")], "same", false,
        ),
        theme(
            "manuscript",
            "The archive: cream, sepia ink, oxblood; linen on a frame.",
            face(0xf6efe3, 0x3b2f2f, 0xfbf7ef, ansi(MANUSCRIPT_PAPER)),
            face(0x241d1a, 0xe4d8c4, 0xfbf7ef, None),
            surface(0x7a1f1f, &[0x7a1f1f, 0xb08a4a], Shell::Stroke, 4.0, 6.0, TextureKind::Linen, 0.14, 4.0),
            C::Signal, BarStyle::Rule, BarColor::Signal,
            &[("launch", "whisper"), ("tab.switch", "page")], "family", false,
        ),
        theme(
            "kyoto",
            "Woodblock: rice paper, sumi, shu vermilion; stitching on the band, the ramp runs to gold.",
            face(0xf2eee6, 0x1c1a17, 0xfaf8f3, ansi(KYOTO_PAPER)),
            face(0x1c1a17, 0xe8e2d4, 0xfaf8f3, ansi(KYOTO_INK)),
            surface(0xe34234, &[0xe34234, 0x1c1a17, 0xd4a017], Shell::Band, 8.0, 0.0, TextureKind::Stitch, 0.16, 6.0),
            C::Signal, BarStyle::Comet, BarColor::Signal,
            &[("launch", "chime"), ("bell", "pulse")], "family", false,
        ),
        theme(
            "blueprint",
            "The drawing room: prussian blue, chalk lines, cyan; linen, and the aurora drifts slowly.",
            face(0xe6eef7, 0x0b2a4a, 0xffffff, ansi(BLUEPRINT_PAPER)),
            face(0x0b2a4a, 0xdbe7f3, 0xffffff, ansi(BLUEPRINT_INK)),
            Surface { drift: 0.05, breath: 0.2, angle: 30.0, ..surface(0x2fb8d8, &[0x2fb8d8, 0x0b2a4a, 0x7fc4ff], Shell::Aurora, 4.0, 10.0, TextureKind::Linen, 0.1, 5.0) },
            C::Signal, BarStyle::Comet, BarColor::Signal,
            &[("launch", "scan"), ("page.ready", "ready")], "wheel", true,
        ),
        theme(
            "darkroom",
            "The print lab: black, dim white, a safelight red that never lights the content.",
            face(0xefefef, 0x1a1a1a, 0xffffff, None),
            face(0x0a0a0a, 0xc8c8c8, 0xffffff, None),
            surface(0xd7263d, &[0xd7263d, 0x5a0f18], Shell::Stroke, 3.0, 0.0, TextureKind::None, 0.0, 3.0),
            C::Signal, BarStyle::Rule, BarColor::Signal,
            &[("launch", "droplet")], "same", true,
        ),
        theme(
            "phosphor",
            "The VT220: black glass, P1 green, amber warnings, halftone scanlines on the band.",
            face(0xe8f2e8, 0x0f2a14, 0xf4faf4, None),
            face(0x0a0f0a, 0x33ff66, 0xf4faf4, ansi(PHOSPHOR)),
            Surface { texture_motion: true, ..surface(0x33ff66, &[0x33ff66, 0x0a3d1a], Shell::Band, 6.0, 0.0, TextureKind::Halftone, 0.2, 3.0) },
            C::Ink, BarStyle::Rule, BarColor::Ink,
            &[("launch", "scan"), ("bell", "error"), ("command.done", "tick")], "same", true,
        ),
        theme(
            "amber",
            "The Wyse: P3 amber on black, green for what went right, halftone on the band.",
            face(0xf8f0dc, 0x3a2a08, 0xfffaf0, None),
            face(0x120b02, 0xffb000, 0xfffaf0, ansi(AMBER)),
            Surface { texture_motion: true, ..surface(0xffb000, &[0xffb000, 0x4a3410], Shell::Band, 6.0, 0.0, TextureKind::Halftone, 0.18, 3.0) },
            C::Ink, BarStyle::Rule, BarColor::Ink,
            &[("launch", "scan"), ("bell", "error")], "same", true,
        ),
        theme(
            "midnight",
            "Ink and paper under an aurora: blue to violet to teal, breathing.",
            face(0xf4f1ea, 0x141414, 0xffffff, None),
            face(0x121420, 0xe6e4ee, 0xffffff, None),
            Surface { base: Some(hex(0x1f5fbf)), tint: 0.22, breath: 0.35, ..surface(0x1f5fbf, &[0x1f5fbf, 0x6b3fa0, 0x1a7f8a], Shell::Aurora, 4.0, 12.0, TextureKind::Linen, 0.05, 5.0) },
            C::Signal, BarStyle::Comet, BarColor::Tab,
            &[("launch", "bloom")], "wheel", true,
        ),
        theme(
            "ledger",
            "Green paper, a stitched stroke, the ramp from green to gold.",
            face(0xeef2e6, 0x1a1f16, 0xffffff, None),
            face(0x141814, 0xe0e6d6, 0xffffff, None),
            surface(0x2e7d32, &[0x2e7d32, 0xc48a00], Shell::Stroke, 3.0, 4.0, TextureKind::Stitch, 0.12, 6.0),
            C::Ink, BarStyle::Rule, BarColor::Signal,
            &[("tab.switch", "tick")], "family", false,
        ),
        theme(
            "bauhaus",
            "The poster: paper, black, and the primaries as stops on a thick frame.",
            face(0xf4f1ea, 0x111111, 0xffffff, ansi(BAUHAUS_PAPER)),
            face(0x111111, 0xf4f1ea, 0xffffff, None),
            surface(0xd7263d, &[0xd7263d, 0x1f4ea1, 0xf2c200], Shell::Gradient, 8.0, 0.0, TextureKind::None, 0.0, 3.0),
            C::Ink, BarStyle::Carapace, BarColor::Signal,
            &[("launch", "press"), ("tab.switch", "press")], "wheel", false,
        ),
        theme(
            "onyx",
            "Monochrome: pure black, white, no colour at all — the signal is white.",
            face(0xffffff, 0x000000, 0xffffff, ansi(ONYX_PAPER)),
            face(0x000000, 0xffffff, 0xffffff, ansi(ONYX)),
            surface(0xffffff, &[0xffffff, 0x555555], Shell::Stroke, 1.0, 0.0, TextureKind::None, 0.0, 3.0),
            C::Ink, BarStyle::Rule, BarColor::Ink,
            &[], "same", true,
        ),
        theme(
            "contrast",
            "Accessibility: black on white at AAA, thick strokes, nothing subtle.",
            face(0xffffff, 0x000000, 0xffffff, ansi(CONTRAST_PAPER)),
            face(0x000000, 0xffffff, 0xffffff, ansi(CONTRAST_INK)),
            surface(0xb30000, &[0xb30000, 0x000000], Shell::Stroke, 6.0, 0.0, TextureKind::None, 0.0, 3.0),
            C::Ink, BarStyle::Rule, BarColor::Signal,
            &[], "same", false,
        ),
    ];
    // Ports: their tokens and sixteens, a nus surface each.
    let mut port = |name: &str, story: &str, paper: Face, ink: Face, s: Surface, prefers_ink: bool| {
        let mut t = theme(name, story, paper, ink, s, C::Signal, BarStyle::Comet, BarColor::Signal, &[], "wheel", prefers_ink);
        t.port = true;
        v.push(t);
    };
    port("solarized", "Ethan Schoonover's Solarized, both faces; the stroke in blue.", face(0xfdf6e3, 0x586e75, 0xfdf6e3, ansi(SOLARIZED_LIGHT)), face(0x002b36, 0x93a1a1, 0xfdf6e3, ansi(SOLARIZED_DARK)), surface(0x268bd2, &[0x268bd2, 0x2aa198], Shell::Stroke, 3.0, 4.0, TextureKind::None, 0.0, 3.0), true);
    port("gruvbox", "Pavel Pertsev's Gruvbox, dark and light; a stitched band in orange.", face(0xfbf1c7, 0x3c3836, 0xfbf1c7, ansi(GRUVBOX_LIGHT)), face(0x282828, 0xebdbb2, 0xfbf1c7, ansi(GRUVBOX_DARK)), surface(0xd65d0e, &[0xd65d0e, 0xfabd2f], Shell::Band, 6.0, 0.0, TextureKind::Stitch, 0.12, 6.0), true);
    port("nord", "Arctic Ice Studio's Nord; frost blue on a hairline frame, linen.", face(0xeceff4, 0x2e3440, 0xeceff4, ansi(NORD_LIGHT)), face(0x2e3440, 0xd8dee9, 0xeceff4, ansi(NORD_DARK)), surface(0x88c0d0, &[0x88c0d0, 0x5e81ac, 0xb48ead], Shell::Gradient, 3.0, 8.0, TextureKind::Linen, 0.08, 5.0), true);
    port("catppuccin", "Catppuccin Mocha and Latte; the aurora runs mauve to blue to teal.", face(0xeff1f5, 0x4c4f69, 0xeff1f5, ansi(LATTE)), face(0x1e1e2e, 0xcdd6f4, 0xeff1f5, ansi(MOCHA)), Surface { drift: 0.06, breath: 0.25, ..surface(0xcba6f7, &[0xcba6f7, 0x89b4fa, 0x94e2d5], Shell::Aurora, 4.0, 14.0, TextureKind::None, 0.0, 3.0) }, true);
    port("tokyo night", "Enkia's Tokyo Night and Day; a blue band, grain.", face(0xe1e2e7, 0x3760bf, 0xe1e2e7, ansi(TOKYO_DAY)), face(0x1a1b26, 0xc0caf5, 0xe1e2e7, ansi(TOKYO_NIGHT)), surface(0x7aa2f7, &[0x7aa2f7, 0xbb9af7], Shell::Band, 6.0, 0.0, TextureKind::Grain, 0.1, 2.0), true);
    port("rosé pine", "Rosé Pine main and dawn; rose and gold on a soft frame.", face(0xfaf4ed, 0x575279, 0xfaf4ed, ansi(ROSE_DAWN)), face(0x191724, 0xe0def4, 0xfaf4ed, ansi(ROSE_MAIN)), surface(0xebbcba, &[0xebbcba, 0xf6c177, 0xc4a7e7], Shell::Gradient, 3.0, 10.0, TextureKind::Linen, 0.06, 5.0), true);
    port("dracula", "Zeno Rocha's Dracula, with Alucard for daylight; purple band.", face(0xfffbeb, 0x1f1f1f, 0xfffbeb, ansi(ALUCARD)), face(0x282a36, 0xf8f8f2, 0xfffbeb, ansi(DRACULA)), surface(0xbd93f9, &[0xbd93f9, 0xff79c6], Shell::Band, 6.0, 0.0, TextureKind::None, 0.0, 3.0), true);
    v
}

fn themes_dir() -> std::path::PathBuf {
    std::env::current_dir().unwrap_or_default().join("profile").join("themes")
}

/// Stock, then the user's saved themes (files win on a name clash).
pub fn all() -> Vec<StockTheme> {
    let mut v = stock();
    if let Ok(rd) = std::fs::read_dir(themes_dir()) {
        let mut mine: Vec<StockTheme> = rd
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
            .filter_map(|e| serde_json::from_str::<StockTheme>(&std::fs::read_to_string(e.path()).ok()?).ok())
            .collect();
        mine.sort_by(|a, b| a.name.cmp(&b.name));
        for m in mine {
            v.retain(|t| t.name != m.name);
            v.push(m);
        }
    }
    v
}

pub fn save(t: &StockTheme) -> std::io::Result<()> {
    std::fs::create_dir_all(themes_dir())?;
    let safe: String = t.name.chars().map(|c| if c.is_alphanumeric() || c == '-' { c } else { '-' }).collect();
    std::fs::write(themes_dir().join(format!("{safe}.json")), serde_json::to_string_pretty(t).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme_edit::contrast;

    #[test]
    fn every_face_reads() {
        for t in stock() {
            for (name, f) in [("paper", &t.paper), ("ink", &t.ink)] {
                let c = contrast(f.ink, f.paper);
                assert!(c >= 4.5, "{} {name}: ink on paper {c:.1}:1", t.name);
                if let Some(a) = f.ansi {
                    // Every colour but the background pair and the paper-coloured
                    // bright white must be visible on the paper.
                    for (i, col) in a.iter().enumerate() {
                        if i == 0 || i == 8 || i == 15 {
                            continue;
                        }
                        let cc = contrast(*col, f.paper);
                        assert!(cc >= 1.8, "{} {name}: ansi {i} {cc:.1}:1 on paper", t.name);
                    }
                }
            }
        }
    }

    #[test]
    fn names_unique_and_count() {
        let s = stock();
        let mut names: Vec<&str> = s.iter().map(|t| t.name.as_str()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), s.len());
        assert_eq!(s.len(), 20);
        assert_eq!(s.iter().filter(|t| t.port).count(), 7);
    }
}
