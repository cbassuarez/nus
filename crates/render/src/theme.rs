//! Broadsheet tokens. Source of truth: docs/DESIGN.md.

use crate::scene::Color;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Paper,
    Ink,
}

#[derive(Clone, Debug)]
pub struct Theme {
    pub mode: Mode,
    pub paper: Color,
    pub ink: Color,
    pub tint: Color,
    pub hot: Color,
    pub dim: Color,
    pub page: Color,
    pub scrim: Color,
    pub ansi: [nus_vt::Rgb; 16],
}

pub const fn hex(v: u32) -> Color {
    [
        ((v >> 16) & 0xff) as f32 / 255.0,
        ((v >> 8) & 0xff) as f32 / 255.0,
        (v & 0xff) as f32 / 255.0,
        1.0,
    ]
}

pub const fn hexa(v: u32, a: f32) -> Color {
    let c = hex(v);
    [c[0], c[1], c[2], a]
}

const fn rgb(v: u32) -> nus_vt::Rgb {
    nus_vt::Rgb {
        r: ((v >> 16) & 0xff) as u8,
        g: ((v >> 8) & 0xff) as u8,
        b: (v & 0xff) as u8,
    }
}

/// Space signal colors, same in both themes.
pub mod signal {
    use super::{hex, Color};
    pub const RED: Color = hex(0xc8102e);
    pub const BLUE: Color = hex(0x1f5fbf);
    pub const GOLD: Color = hex(0xd9a400);
    pub const GREEN: Color = hex(0x2e7d32);
    pub const VIOLET: Color = hex(0x6b3fa0);
    pub const TEAL: Color = hex(0x1a7f8a);
    pub const ALL: [Color; 6] = [RED, BLUE, GOLD, GREEN, VIOLET, TEAL];
}

/// Rule weights (logical px) and spacing.
pub mod metric {
    pub const HAIRLINE: f32 = 1.0;
    pub const STRUCTURE: f32 = 1.5;
    pub const FLOATING: f32 = 2.0;
    pub const BAND: f32 = 6.0;
    pub const TOP_STRIP: f32 = 30.0;
    pub const SIDEBAR: f32 = 272.0;
    pub const SPLIT: f32 = 520.0;
    pub const PALETTE: f32 = 600.0;
    pub const ROW_PAD_Y: f32 = 12.0;
    pub const ROW_PAD_X: f32 = 14.0;
    pub const HEADER_PAD_Y: f32 = 9.0;
    pub const HEADER_PAD_X: f32 = 18.0;
    pub const PREVIEW_H: f32 = 52.0;
    pub const UI_PX: f32 = 13.0;
    pub const LABEL_PX: f32 = 11.0;
    pub const LABEL_TRACKING: f32 = 0.08;
    pub const WORDMARK_PX: f32 = 20.0;
    pub const SHADOW: f32 = 8.0;
}

impl Theme {
    pub fn paper() -> Theme {
        Theme {
            mode: Mode::Paper,
            paper: hex(0xf4f1ea),
            ink: hex(0x141414),
            tint: hexa(0x141414, 0.06),
            hot: hexa(0x141414, 0.12),
            dim: hex(0x8a857a),
            page: hex(0xffffff),
            scrim: hexa(0xf4f1ea, 0.55),
            ansi: [
                rgb(0x141414),
                rgb(0xb3261e),
                rgb(0x2e7d32),
                rgb(0x9a6b00),
                rgb(0x1f5fbf),
                rgb(0x8e3b8e),
                rgb(0x1a7f8a),
                rgb(0x8a857a),
                rgb(0x4a4740),
                rgb(0xd63a2f),
                rgb(0x3f9a45),
                rgb(0xc48a00),
                rgb(0x3b7ee0),
                rgb(0xb04eb0),
                rgb(0x22a3b0),
                rgb(0xf4f1ea),
            ],
        }
    }

    pub fn ink() -> Theme {
        Theme {
            mode: Mode::Ink,
            paper: hex(0x141414),
            ink: hex(0xece7da),
            tint: hexa(0xece7da, 0.07),
            hot: hexa(0xece7da, 0.14),
            dim: hex(0x8a857a),
            page: hex(0xffffff),
            scrim: hexa(0x000000, 0.5),
            ansi: [
                rgb(0x141414),
                rgb(0xe0574c),
                rgb(0x7ac77f),
                rgb(0xe5b94a),
                rgb(0x6ea3ef),
                rgb(0xd086d0),
                rgb(0x6fd0da),
                rgb(0xbdb8ab),
                rgb(0x5a564e),
                rgb(0xff6f63),
                rgb(0x93e39a),
                rgb(0xffd06a),
                rgb(0x8fbcff),
                rgb(0xe9a0e9),
                rgb(0x8be6ef),
                rgb(0xece7da),
            ],
        }
    }

    /// Install this theme's ANSI colors and default fg/bg into a terminal.
    pub fn apply(&self, palette: &mut nus_vt::Palette) {
        for (i, c) in self.ansi.iter().enumerate() {
            palette.set_base(i, *c);
        }
        let to_rgb = |c: Color| nus_vt::Rgb {
            r: (c[0] * 255.0).round() as u8,
            g: (c[1] * 255.0).round() as u8,
            b: (c[2] * 255.0).round() as u8,
        };
        palette.set_base(nus_vt::palette::FG, to_rgb(self.ink));
        palette.set_base(nus_vt::palette::BG, to_rgb(self.paper));
        palette.set_base(nus_vt::palette::CURSOR, to_rgb(self.ink));
    }

    pub fn with_alpha(c: Color, a: f32) -> Color {
        [c[0], c[1], c[2], a]
    }
}
