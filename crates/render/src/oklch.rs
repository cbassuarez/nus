//! OKLCH: Oklab (Björn Ottosson) in polar form, lightness · chroma · hue.
//! Lightness here is what the eye calls lighter, so moving a colour along
//! it keeps its hue and its character, which an sRGB mix toward white or
//! black does not. Contrast is still judged by WCAG 2 (`policy::contrast`);
//! OKLCH only decides how a colour moves to reach it.

use crate::policy::contrast;
use crate::scene::Color;

/// Lightness 0..1, chroma 0..~0.37, hue in degrees.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lch {
    pub l: f32,
    pub c: f32,
    pub h: f32,
}

fn lin(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn gamma(v: f32) -> f32 {
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// sRGB (alpha ignored) → OKLCH.
pub fn from_srgb(c: Color) -> Lch {
    let (r, g, b) = (lin(c[0]), lin(c[1]), lin(c[2]));
    let l = (0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    let ok_l = 0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s;
    let ok_a = 1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s;
    let ok_b = 0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s;
    Lch {
        l: ok_l,
        c: (ok_a * ok_a + ok_b * ok_b).sqrt(),
        h: ok_b.atan2(ok_a).to_degrees().rem_euclid(360.0),
    }
}

/// Linear sRGB for an OKLCH colour, unclamped (may be out of gamut).
fn raw(p: Lch) -> [f32; 3] {
    let (a, b) = (p.c * p.h.to_radians().cos(), p.c * p.h.to_radians().sin());
    let l = (p.l + 0.396_337_78 * a + 0.215_803_76 * b).powi(3);
    let m = (p.l - 0.105_561_346 * a - 0.063_854_17 * b).powi(3);
    let s = (p.l - 0.089_484_18 * a - 1.291_485_5 * b).powi(3);
    [
        4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s,
        -1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s,
        -0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s,
    ]
}

fn in_gamut(v: [f32; 3]) -> bool {
    v.iter().all(|&x| (-1e-4..=1.0 + 1e-4).contains(&x))
}

/// OKLCH → sRGB with `alpha`. Out of gamut, chroma gives way first
/// (lightness and hue are what the colour is), as CSS Color 4 maps it.
pub fn to_srgb(p: Lch, alpha: f32) -> Color {
    let p = Lch {
        l: p.l.clamp(0.0, 1.0),
        c: p.c.max(0.0),
        h: p.h,
    };
    let mut v = raw(p);
    if !in_gamut(v) {
        let (mut lo, mut hi) = (0.0f32, p.c);
        for _ in 0..20 {
            let mid = (lo + hi) * 0.5;
            if in_gamut(raw(Lch { c: mid, ..p })) {
                lo = mid
            } else {
                hi = mid
            }
        }
        v = raw(Lch { c: lo, ..p });
    }
    [
        gamma(v[0].clamp(0.0, 1.0)),
        gamma(v[1].clamp(0.0, 1.0)),
        gamma(v[2].clamp(0.0, 1.0)),
        alpha,
    ]
}

/// `oklch(l c h)` as a colour, for writing tokens the way CSS does.
pub fn oklch(l: f32, c: f32, h: f32) -> Color {
    to_srgb(Lch { l, c, h }, 1.0)
}

/// `fg` made to read on `bg` at `min` (WCAG 2): its OKLCH lightness moves
/// the shorter way that gets there, hue and chroma kept. A dark ink on a
/// page that went black comes out light, not grey. When neither way
/// reaches `min`, the end that reads best.
pub fn readable(fg: Color, bg: Color, min: f32) -> Color {
    if min <= 1.0 || contrast(fg, bg) >= min {
        return fg;
    }
    let p = from_srgb(fg);
    let at = |l: f32| to_srgb(Lch { l, ..p }, fg[3]);
    // March each way in small steps, then refine the crossing; contrast is
    // not monotone in L when the walk passes the background's lightness.
    let walk = |end: f32| -> Option<(f32, Color)> {
        let steps = 64;
        let mut prev = p.l;
        for i in 1..=steps {
            let l = p.l + (end - p.l) * i as f32 / steps as f32;
            if contrast(at(l), bg) >= min {
                let (mut lo, mut hi) = (prev, l);
                for _ in 0..12 {
                    let mid = (lo + hi) * 0.5;
                    if contrast(at(mid), bg) >= min {
                        hi = mid
                    } else {
                        lo = mid
                    }
                }
                return Some(((hi - p.l).abs(), at(hi)));
            }
            prev = l;
        }
        None
    };
    match (walk(1.0), walk(0.0)) {
        (Some(a), Some(b)) => {
            if a.0 <= b.0 {
                a.1
            } else {
                b.1
            }
        }
        (Some(a), None) => a.1,
        (None, Some(b)) => b.1,
        (None, None) => {
            let (hi, lo) = (at(1.0), at(0.0));
            if contrast(hi, bg) >= contrast(lo, bg) {
                hi
            } else {
                lo
            }
        }
    }
}

/// What to write on a `fill`: whichever of `a` and `b` reads better on it,
/// made to reach `min`. Buttons that fill on hover ask this, so the label
/// flips with the fill, whatever colour the fill turns out to be.
pub fn on(fill: Color, a: Color, b: Color, min: f32) -> Color {
    let pick = if contrast(a, fill) >= contrast(b, fill) {
        a
    } else {
        b
    };
    readable(pick, fill, min)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::hex;

    fn close(a: Color, b: Color) -> bool {
        (0..3).all(|i| (a[i] - b[i]).abs() < 2.0 / 255.0)
    }

    #[test]
    fn round_trips_srgb() {
        for c in [
            hex(0x000000),
            hex(0xffffff),
            hex(0xc8102e),
            hex(0x1f5fbf),
            hex(0xd9a400),
            hex(0x777777),
        ] {
            assert!(close(to_srgb(from_srgb(c), 1.0), c), "{c:?}");
        }
        let white = from_srgb(hex(0xffffff));
        assert!((white.l - 1.0).abs() < 1e-3 && white.c < 1e-3);
    }

    #[test]
    fn black_ink_on_a_black_page_turns_white() {
        let ink = hex(0x111111);
        let page = hex(0x000000);
        // Asked for the contrast it had on its own paper, it comes out white.
        let out = readable(ink, page, 17.0);
        assert!(contrast(out, page) >= 17.0);
        assert!(from_srgb(out).l > 0.9);
        // Asked only for AA, the least move that reads.
        assert!(contrast(readable(ink, page, 4.5), page) >= 4.5);
    }

    #[test]
    fn readable_keeps_hue_and_leaves_good_pairs_alone() {
        let red = hex(0xc8102e);
        assert_eq!(readable(red, hex(0xffffff), 4.5), red);
        let out = readable(red, hex(0x101010), 4.5);
        assert!(contrast(out, hex(0x101010)) >= 4.5);
        let (a, b) = (from_srgb(red), from_srgb(out));
        assert!((a.h - b.h).abs() < 3.0, "hue {} → {}", a.h, b.h);
    }

    #[test]
    fn labels_flip_with_the_fill() {
        let (paper, ink) = (hex(0xf4f1ea), hex(0x111111));
        assert!(contrast(on(ink, paper, ink, 4.5), ink) >= 4.5);
        assert!(contrast(on(paper, paper, ink, 4.5), paper) >= 4.5);
        let gold = hex(0xd9a400);
        assert!(contrast(on(gold, paper, ink, 4.5), gold) >= 4.5);
    }
}
