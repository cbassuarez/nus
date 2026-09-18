//! What happens to a program's colours before they reach the screen.
//!
//! A theme owns the sixteen; a program that sends truecolour or the 256
//! brings its own, chosen against some other background — Claude Code's
//! greys on a paper theme, a TUI's panels on ink — and they land wrong.
//! Three moves, each a setting and each a rule can set per program:
//!
//!   · **grade**: every foreground must reach a WCAG ratio against its
//!     background (4.5:1 is AA for text); one that doesn't is walked
//!     toward white or black, whichever way it already leans, until it
//!     does. VS Code's terminal does this by default; so do we.
//!   · **snap**: truecolour and the 256 become the nearest of the sixteen
//!     (in Oklab, so "nearest" is what the eye says), and the program
//!     wears the theme.
//!   · **its own sixteen and remaps**: a program gets a palette of its
//!     own, and exact colours it hardcodes can be pointed at ours.

use nus_vt::Rgb;

use crate::scene::Color;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Policy {
    /// The ratio every foreground reaches; 0 or 1 leaves colours alone.
    pub min_contrast: f32,
    /// Truecolour and the 256 snap to the nearest of the sixteen.
    pub snap: bool,
    /// This program's sixteen, over the theme's.
    pub ansi: Option<[Rgb; 16]>,
    /// Exact colours the program sends, and what to draw instead.
    pub remap: Vec<(Rgb, Rgb)>,
}

impl Policy {
    pub fn is_plain(&self) -> bool {
        self.min_contrast <= 1.0 && !self.snap && self.ansi.is_none() && self.remap.is_empty()
    }

    /// Something that changes when the policy does, for row caches.
    pub fn stamp(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::hash::DefaultHasher::new();
        self.min_contrast.to_bits().hash(&mut h);
        self.snap.hash(&mut h);
        if let Some(a) = &self.ansi {
            for c in a {
                (c.r, c.g, c.b).hash(&mut h);
            }
        }
        for (a, b) in &self.remap {
            (a.r, a.g, a.b, b.r, b.g, b.b).hash(&mut h);
        }
        h.finish()
    }

    /// One colour through the remaps — a remap is exact and final — and,
    /// when it isn't one of the sixteen, the snap.
    pub fn place(&self, c: Rgb, of_sixteen: bool, sixteen: &[Rgb; 16]) -> Rgb {
        if let Some((_, to)) = self.remap.iter().find(|(from, _)| *from == c) {
            return *to;
        }
        if self.snap && !of_sixteen {
            nearest(c, sixteen)
        } else {
            c
        }
    }
}

/// WCAG 2 relative luminance of an sRGB colour.
pub fn luminance(c: Color) -> f32 {
    let f = |v: f32| {
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * f(c[0]) + 0.7152 * f(c[1]) + 0.0722 * f(c[2])
}

/// WCAG 2 contrast ratio, 1..21.
pub fn contrast(a: Color, b: Color) -> f32 {
    let (la, lb) = (luminance(a) + 0.05, luminance(b) + 0.05);
    if la > lb {
        la / lb
    } else {
        lb / la
    }
}

/// `fg` walked toward white or black — the way it already leans against
/// `bg` — until it reads at `min`; the nearer end when neither reaches.
pub fn ensure_contrast(fg: Color, bg: Color, min: f32) -> Color {
    if min <= 1.0 || contrast(fg, bg) >= min {
        return fg;
    }
    let lighter = luminance(fg) >= luminance(bg);
    let toward = |end: f32| -> (Color, f32) {
        let end_c = [end, end, end, fg[3]];
        // The least of the move that reads: a binary search on the mix.
        let (mut lo, mut hi) = (0.0f32, 1.0f32);
        let mix = |t: f32| [
            fg[0] + (end - fg[0]) * t,
            fg[1] + (end - fg[1]) * t,
            fg[2] + (end - fg[2]) * t,
            fg[3],
        ];
        if contrast(end_c, bg) < min {
            return (end_c, contrast(end_c, bg));
        }
        for _ in 0..10 {
            let mid = (lo + hi) * 0.5;
            if contrast(mix(mid), bg) >= min {
                hi = mid;
            } else {
                lo = mid;
            }
        }
        (mix(hi), contrast(mix(hi), bg))
    };
    let (first, other) = if lighter { (1.0, 0.0) } else { (0.0, 1.0) };
    let (c, ratio) = toward(first);
    if ratio >= min {
        return c;
    }
    let (c2, ratio2) = toward(other);
    if ratio2 > ratio {
        c2
    } else {
        c
    }
}

/// sRGB → Oklab (Björn Ottosson), for distances the eye agrees with.
fn oklab(c: Rgb) -> [f32; 3] {
    let lin = |v: u8| {
        let v = v as f32 / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    let (r, g, b) = (lin(c.r), lin(c.g), lin(c.b));
    let l = (0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    [
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}

/// The one of the sixteen nearest to `c`.
pub fn nearest(c: Rgb, sixteen: &[Rgb; 16]) -> Rgb {
    let want = oklab(c);
    let mut best = (f32::MAX, sixteen[7]);
    for &s in sixteen {
        let have = oklab(s);
        let d = (want[0] - have[0]).powi(2)
            + (want[1] - have[1]).powi(2)
            + (want[2] - have[2]).powi(2);
        if d < best.0 {
            best = (d, s);
        }
    }
    best.1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb(v: u32) -> Rgb {
        Rgb {
            r: (v >> 16) as u8,
            g: (v >> 8) as u8,
            b: v as u8,
        }
    }

    #[test]
    fn grading_reaches_the_ratio_and_moves_the_least() {
        let bg = [0.08, 0.08, 0.08, 1.0];
        let grey = [0.3, 0.3, 0.3, 1.0]; // Claude Code's dim on ink: fails
        assert!(contrast(grey, bg) < 4.5);
        let fixed = ensure_contrast(grey, bg, 4.5);
        assert!(contrast(fixed, bg) >= 4.5);
        assert!(fixed[0] > grey[0] && fixed[0] < 0.9, "{fixed:?}");
        // Already readable: untouched.
        let ink = [0.92, 0.9, 0.85, 1.0];
        assert_eq!(ensure_contrast(ink, bg, 4.5), ink);
        // Off: untouched.
        assert_eq!(ensure_contrast(grey, bg, 0.0), grey);
    }

    #[test]
    fn snapping_finds_the_eye_nearest() {
        let sixteen = [
            rgb(0x141414),
            rgb(0xe0574c),
            rgb(0x7ac77f),
            rgb(0xe5b94a),
            rgb(0x7fa7f0),
            rgb(0xd88fd8),
            rgb(0x6fd3dc),
            rgb(0xd8d2c4),
            rgb(0x6b665c),
            rgb(0xff7a6e),
            rgb(0x93e29a),
            rgb(0xffd06a),
            rgb(0x8fbcff),
            rgb(0xe9a0e9),
            rgb(0x8be6ef),
            rgb(0xece7da),
        ];
        // Claude's orange lands on the red, not the yellow.
        assert_eq!(nearest(rgb(0xd97757), &sixteen), rgb(0xe0574c));
        // A mid grey lands on bright black.
        assert_eq!(nearest(rgb(0x808080), &sixteen), rgb(0x6b665c));
        // A remap wins over the snap.
        let p = Policy {
            snap: true,
            remap: vec![(rgb(0xd97757), rgb(0x123456))],
            ..Default::default()
        };
        assert_eq!(p.place(rgb(0xd97757), false, &sixteen), rgb(0x123456));
        assert_eq!(p.place(rgb(0x808080), false, &sixteen), rgb(0x6b665c));
        assert_eq!(p.place(rgb(0x808080), true, &sixteen), rgb(0x808080));
    }
}
