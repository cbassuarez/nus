//! The 256-color palette plus default fg/bg/cursor, with OSC 4/10/11/12
//! overrides layered on top.

use crate::cell::Color;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl From<vte::ansi::Rgb> for Rgb {
    fn from(c: vte::ansi::Rgb) -> Self {
        Rgb {
            r: c.r,
            g: c.g,
            b: c.b,
        }
    }
}

pub const FG: usize = 256;
pub const BG: usize = 257;
pub const CURSOR: usize = 258;
const LEN: usize = 259;

#[derive(Clone, Debug)]
pub struct Palette {
    base: [Rgb; LEN],
    overrides: [Option<Rgb>; LEN],
}

impl Palette {
    /// A dark default theme. Themes replace `base` wholesale via `set_base`.
    pub fn new() -> Palette {
        let mut base = [Rgb::default(); LEN];
        // ANSI 0–15 (a muted, readable default; themes override).
        let ansi: [u32; 16] = [
            0x1d1f21, 0xcc6666, 0xb5bd68, 0xf0c674, 0x81a2be, 0xb294bb, 0x8abeb7, 0xc5c8c6,
            0x666666, 0xd54e53, 0xb9ca4a, 0xe7c547, 0x7aa6da, 0xc397d8, 0x70c0b1, 0xeaeaea,
        ];
        for (i, &c) in ansi.iter().enumerate() {
            base[i] = hex(c);
        }
        // 16–231: 6×6×6 cube.
        for i in 0..216 {
            let (r, g, b) = (i / 36, (i / 6) % 6, i % 6);
            let f = |v: usize| if v == 0 { 0 } else { (55 + v * 40) as u8 };
            base[16 + i] = Rgb {
                r: f(r),
                g: f(g),
                b: f(b),
            };
        }
        // 232–255: grayscale ramp.
        for i in 0..24 {
            let v = (8 + i * 10) as u8;
            base[232 + i] = Rgb { r: v, g: v, b: v };
        }
        base[FG] = hex(0xc5c8c6);
        base[BG] = hex(0x1d1f21);
        base[CURSOR] = hex(0xc5c8c6);
        Palette {
            base,
            overrides: [None; LEN],
        }
    }

    pub fn set_base(&mut self, index: usize, rgb: Rgb) {
        if index < LEN {
            self.base[index] = rgb;
        }
    }

    pub fn set_override(&mut self, index: usize, rgb: Rgb) {
        if index < LEN {
            self.overrides[index] = Some(rgb);
        }
    }

    pub fn reset_override(&mut self, index: usize) {
        if index < LEN {
            self.overrides[index] = None;
        }
    }

    pub fn reset_all_overrides(&mut self) {
        self.overrides = [None; LEN];
    }

    pub fn get(&self, index: usize) -> Rgb {
        let index = index.min(LEN - 1);
        self.overrides[index].unwrap_or(self.base[index])
    }

    pub fn resolve(&self, color: Color, is_fg: bool) -> Rgb {
        match color {
            Color::Default => self.get(if is_fg { FG } else { BG }),
            Color::Indexed(i) => self.get(i as usize),
            Color::Rgb(r, g, b) => Rgb { r, g, b },
        }
    }
}

impl Default for Palette {
    fn default() -> Self {
        Palette::new()
    }
}

fn hex(v: u32) -> Rgb {
    Rgb {
        r: (v >> 16) as u8,
        g: (v >> 8) as u8,
        b: v as u8,
    }
}
