//! Eclipses, by geometry. Nothing is looked up: a solar eclipse is the
//! topocentric Sun and Moon overlapping on the sky; a lunar eclipse is the
//! Moon crossing the Earth's shadow. The same functions that draw an eclipse
//! frame find the next one.

use crate::ephem::{jde_from_unix_ms, moon_geocentric, sky_at, sun_geocentric, Sky};
use crate::events::{next_phase, Phase};
use crate::{Observer, D2R, DAY_MS};

/// Sun above this is "up": the standard −50′ for refraction and the Sun's own radius.
const HORIZON: f64 = -0.8333 * D2R;
/// Moon latitude at new or full Moon beyond which no eclipse is possible.
const ECLIPSE_LIMIT: f64 = 1.65 * D2R;
const EARTH_RADIUS_KM: f64 = 6378.14;
const SUN_RADIUS_KM: f64 = 695_700.0;
/// The Earth's atmosphere fattens its shadow by about this much (Danjon).
const SHADOW_ENLARGEMENT: f64 = 1.02;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SolarKind {
    Partial,
    Annular,
    Total,
}

/// The Sun and Moon's overlap at one instant, for one observer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SolarState {
    /// Angular separation of the two centres.
    pub separation: f64,
    /// Fraction of the Sun's diameter covered; ≥ 1 for a total eclipse. 0 when none.
    pub magnitude: f64,
    /// Fraction of the Sun's disc covered, 0..1.
    pub obscuration: f64,
    pub sun_radius: f64,
    pub moon_radius: f64,
}

fn angle_between(a: [f64; 3], b: [f64; 3]) -> f64 {
    let d = a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let c = [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ];
    (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt().atan2(d)
}

/// Area of the lens where two discs overlap, as a fraction of the first.
fn overlap_fraction(r1: f64, r2: f64, d: f64) -> f64 {
    if d >= r1 + r2 {
        return 0.0;
    }
    if d <= (r1 - r2).abs() {
        return if r2 >= r1 { 1.0 } else { (r2 / r1).powi(2) };
    }
    let a1 = ((d * d + r1 * r1 - r2 * r2) / (2.0 * d * r1))
        .clamp(-1.0, 1.0)
        .acos();
    let a2 = ((d * d + r2 * r2 - r1 * r1) / (2.0 * d * r2))
        .clamp(-1.0, 1.0)
        .acos();
    let k = (-d + r1 + r2) * (d + r1 - r2) * (d - r1 + r2) * (d + r1 + r2);
    (r1 * r1 * a1 + r2 * r2 * a2 - 0.5 * k.max(0.0).sqrt()) / (std::f64::consts::PI * r1 * r1)
}

/// The solar eclipse state in a sky already computed for this observer.
pub fn solar_eclipse_at(sky: &Sky) -> SolarState {
    let d = angle_between(sky.sun.dir, sky.moon.dir);
    let (rs, rm) = (sky.sun_radius, sky.moon_radius);
    SolarState {
        separation: d,
        magnitude: ((rs + rm - d) / (2.0 * rs)).max(0.0),
        obscuration: overlap_fraction(rs, rm, d),
        sun_radius: rs,
        moon_radius: rm,
    }
}

/// An eclipse of the Sun, as one place sees it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SolarEclipse {
    pub kind: SolarKind,
    /// Greatest eclipse while the Sun is up.
    pub max_ms: f64,
    pub magnitude: f64,
    pub obscuration: f64,
    /// First and last contact above the horizon (the eclipse may already be
    /// under way at sunrise, or run on past sunset).
    pub start_ms: f64,
    pub end_ms: f64,
    /// Second and third contact: the Sun wholly covered (or the ring complete).
    pub central_start_ms: Option<f64>,
    pub central_end_ms: Option<f64>,
    pub sun_alt_at_max: f64,
}

impl SolarEclipse {
    pub fn central_seconds(&self) -> Option<f64> {
        Some((self.central_end_ms? - self.central_start_ms?) / 1000.0)
    }
}

fn solar_state_at(ms: f64, o: &Observer) -> (SolarState, f64) {
    let sky = sky_at(ms, o);
    (solar_eclipse_at(&sky), sky.sun.alt)
}

/// Bisect `f(ms) > 0` (inside) against `f(ms) <= 0` between two times.
fn bisect(mut inside: f64, mut outside: f64, f: impl Fn(f64) -> bool) -> f64 {
    for _ in 0..28 {
        let mid = 0.5 * (inside + outside);
        if f(mid) {
            inside = mid;
        } else {
            outside = mid;
        }
        if (inside - outside).abs() < 250.0 {
            break;
        }
    }
    0.5 * (inside + outside)
}

/// New Moons (or full, `Phase::Full`) from `from_ms` on, as an iterator of times.
fn lunations(from_ms: f64, phase: Phase) -> impl Iterator<Item = f64> {
    let mut t = from_ms;
    std::iter::from_fn(move || {
        let next = next_phase(t, phase);
        t = next + 10.0 * DAY_MS;
        Some(next)
    })
}

/// Moon latitude (radians) at an instant, geocentric.
fn moon_latitude(ms: f64) -> f64 {
    moon_geocentric(jde_from_unix_ms(ms)).lat
}

/// The first solar eclipse seen from `o` — in progress now or later — within
/// `within_days` of `from_ms`. "Seen" means the Sun is up for some of it.
pub fn next_solar_eclipse(from_ms: f64, o: &Observer, within_days: f64) -> Option<SolarEclipse> {
    // Begin a lunation early: an eclipse may already be under way.
    for new_moon in lunations(from_ms - DAY_MS, Phase::New) {
        if new_moon > from_ms + within_days * DAY_MS {
            return None;
        }
        if moon_latitude(new_moon).abs() > ECLIPSE_LIMIT {
            continue;
        }
        let Some(e) = solar_eclipse_near(new_moon, o) else {
            continue;
        };
        if e.end_ms >= from_ms {
            return Some(e);
        }
    }
    None
}

/// Scan ±6 h around a new Moon for an eclipse seen from `o` with the Sun up.
/// The next solar eclipse worth looking up for: not a sliver (a partial under a tenth of the Sun).
pub fn next_notable_solar_eclipse(
    from_ms: f64,
    o: &Observer,
    within_days: f64,
) -> Option<SolarEclipse> {
    let mut from = from_ms;
    for _ in 0..12 {
        let e = next_solar_eclipse(from, o, within_days - (from - from_ms) / DAY_MS)?;
        if e.kind != SolarKind::Partial || e.obscuration >= 0.10 {
            return Some(e);
        }
        from = e.end_ms + DAY_MS;
    }
    None
}

/// The next lunar eclipse a person can see: the Moon at least partly in the umbra, not just the penumbra.
pub fn next_notable_lunar_eclipse(
    from_ms: f64,
    o: &Observer,
    within_days: f64,
) -> Option<LunarEclipse> {
    let mut from = from_ms;
    for _ in 0..12 {
        let e = next_lunar_eclipse(from, o, within_days - (from - from_ms) / DAY_MS)?;
        if e.kind != LunarKind::Penumbral {
            return Some(e);
        }
        from = e.penumbral_end_ms + DAY_MS;
    }
    None
}

fn solar_eclipse_near(new_moon: f64, o: &Observer) -> Option<SolarEclipse> {
    let step = 120_000.0;
    let (lo, hi) = (new_moon - 6.0 * 3_600_000.0, new_moon + 6.0 * 3_600_000.0);
    let mut best: Option<(f64, f64, SolarState, f64)> = None; // (ms, magnitude, state, alt)
    let (mut first, mut last): (Option<f64>, Option<f64>) = (None, None);
    let mut ms = lo;
    let mut previous_inside = false;
    let mut prev_ms = lo;
    while ms <= hi {
        let (s, alt) = solar_state_at(ms, o);
        let up = alt > HORIZON;
        let inside = s.magnitude > 0.0 && up;
        if inside {
            if first.is_none() {
                // Refine the first contact: where it begins, either by the Moon
                // reaching the Sun or by the Sun rising into an eclipse.
                first = Some(if previous_inside {
                    ms
                } else {
                    refine_start(prev_ms, ms, o)
                });
            }
            last = Some(ms);
            if best.as_ref().is_none_or(|b| s.separation < b.2.separation) {
                best = Some((ms, s.magnitude, s, alt));
            }
        } else if previous_inside {
            last = Some(refine_end(prev_ms, ms, o));
        }
        previous_inside = inside;
        prev_ms = ms;
        ms += step;
    }
    let (best_ms, _, _, _) = best?;
    // Sharpen the greatest eclipse: the minimum separation within a step of the sample.
    let max_ms = refine_min(best_ms - step, best_ms + step, |t| {
        let (s, alt) = solar_state_at(t, o);
        if alt > HORIZON {
            s.separation
        } else {
            f64::MAX
        }
    });
    let (s, alt) = solar_state_at(max_ms, o);
    let (rs, rm) = (s.sun_radius, s.moon_radius);
    // Total when the Moon is the larger disc and covers the Sun; annular when it is
    // the smaller and sits wholly inside; otherwise the Sun is only part covered.
    let kind = if rm >= rs && s.separation <= rm - rs {
        SolarKind::Total
    } else if rs > rm && s.separation <= rs - rm {
        SolarKind::Annular
    } else {
        SolarKind::Partial
    };
    // The central phase (second to third contact): the separation inside |rs − rm|.
    let (central_start_ms, central_end_ms) = if kind == SolarKind::Partial {
        (None, None)
    } else {
        let central = |t: f64| {
            let (s, _) = solar_state_at(t, o);
            s.separation < (s.sun_radius - s.moon_radius).abs()
        };
        let minute = 60_000.0;
        let (mut a, mut b) = (max_ms, max_ms);
        while central(a - minute) && max_ms - a < 20.0 * minute {
            a -= minute;
        }
        while central(b + minute) && b - max_ms < 20.0 * minute {
            b += minute;
        }
        (
            Some(bisect(a, a - minute, central)),
            Some(bisect(b, b + minute, central)),
        )
    };
    Some(SolarEclipse {
        kind,
        max_ms,
        magnitude: s.magnitude,
        obscuration: s.obscuration,
        start_ms: first?,
        end_ms: last?,
        central_start_ms,
        central_end_ms,
        sun_alt_at_max: alt,
    })
}

fn refine_start(outside: f64, inside: f64, o: &Observer) -> f64 {
    bisect(inside, outside, |t| {
        let (s, alt) = solar_state_at(t, o);
        s.magnitude > 0.0 && alt > HORIZON
    })
}

fn refine_end(inside: f64, outside: f64, o: &Observer) -> f64 {
    bisect(inside, outside, |t| {
        let (s, alt) = solar_state_at(t, o);
        s.magnitude > 0.0 && alt > HORIZON
    })
}

/// Golden-section minimum of `f` between two times.
fn refine_min(mut a: f64, mut b: f64, f: impl Fn(f64) -> f64) -> f64 {
    let g = 0.618_033_988_75;
    let (mut c, mut d) = (b - g * (b - a), a + g * (b - a));
    let (mut fc, mut fd) = (f(c), f(d));
    for _ in 0..40 {
        if (b - a).abs() < 500.0 {
            break;
        }
        if fc < fd {
            b = d;
            d = c;
            fd = fc;
            c = b - g * (b - a);
            fc = f(c);
        } else {
            a = c;
            c = d;
            fc = fd;
            d = a + g * (b - a);
            fd = f(d);
        }
    }
    0.5 * (a + b)
}

// ── lunar ───────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LunarKind {
    Penumbral,
    Partial,
    Total,
}

/// The Moon against the Earth's shadow at one instant.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LunarState {
    /// How far the Moon has gone into the umbra, in Moon diameters (≥ 1 is total).
    pub umbral_magnitude: f64,
    pub penumbral_magnitude: f64,
    /// East/Up/North direction of the shadow's axis at the Moon's distance, as this observer sees it.
    pub shadow_dir: [f64; 3],
    /// Angular radii of the umbra and penumbra at the Moon, as this observer sees the Moon.
    pub umbra_radius: f64,
    pub penumbra_radius: f64,
    /// Centre of the Moon to centre of the umbra.
    pub separation: f64,
}

struct ShadowGeometry {
    separation: f64,
    umbra: f64,
    penumbra: f64,
    moon_radius: f64,
}

fn shadow_geometry(jde: f64) -> ShadowGeometry {
    let sun = sun_geocentric(jde);
    let moon = moon_geocentric(jde);
    let anti = sun.unit().map(|c| -c);
    let separation = angle_between(moon.unit(), anti);
    let pi_moon = (EARTH_RADIUS_KM / moon.dist_km).asin();
    let pi_sun = (EARTH_RADIUS_KM / sun.dist_km).asin();
    let s_sun = (SUN_RADIUS_KM / sun.dist_km).asin();
    ShadowGeometry {
        separation,
        umbra: SHADOW_ENLARGEMENT * (pi_moon + pi_sun - s_sun),
        penumbra: SHADOW_ENLARGEMENT * (pi_moon + pi_sun + s_sun),
        moon_radius: (1737.4 / moon.dist_km).asin(),
    }
}

/// The lunar eclipse state for the sky computed for this observer.
pub fn lunar_eclipse_at(sky: &Sky, o: &Observer) -> LunarState {
    let g = shadow_geometry(sky.jde);
    let anti = sky.sun_geo.unit().map(|c| -c);
    // The shadow's axis, at the Moon's distance, as seen from the observer's place.
    let p = anti.map(|c| c * sky.moon_geo.dist_km);
    let lst = sky.lst;
    let phi = o.lat_deg * D2R;
    let u = ((1.0 - 1.0 / 298.257) * phi.tan()).atan();
    let (rho_s, rho_c) = ((1.0 - 1.0 / 298.257) * u.sin(), u.cos());
    let ov = [
        EARTH_RADIUS_KM * rho_c * lst.cos(),
        EARTH_RADIUS_KM * rho_c * lst.sin(),
        EARTH_RADIUS_KM * rho_s,
    ];
    let t = [p[0] - ov[0], p[1] - ov[1], p[2] - ov[2]];
    let n = (t[0] * t[0] + t[1] * t[1] + t[2] * t[2]).sqrt();
    let t = t.map(|c| c / n);
    let (sl, cl) = lst.sin_cos();
    let (xl, yl, zl) = (t[0] * cl + t[1] * sl, -t[0] * sl + t[1] * cl, t[2]);
    let (sp, cp) = phi.sin_cos();
    let shadow_dir = [yl, xl * cp + zl * sp, -xl * sp + zl * cp];
    // Keep the shadow in proportion to the Moon as this observer sees it.
    let scale = sky.moon_radius / g.moon_radius;
    let sep = angle_between(sky.moon.dir, shadow_dir);
    LunarState {
        umbral_magnitude: ((g.umbra + g.moon_radius - g.separation) / (2.0 * g.moon_radius))
            .max(0.0),
        penumbral_magnitude: ((g.penumbra + g.moon_radius - g.separation) / (2.0 * g.moon_radius))
            .max(0.0),
        shadow_dir,
        umbra_radius: g.umbra * scale,
        penumbra_radius: g.penumbra * scale,
        separation: sep,
    }
}

/// A lunar eclipse, with the moments it can be watched.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LunarEclipse {
    pub kind: LunarKind,
    pub max_ms: f64,
    pub umbral_magnitude: f64,
    pub penumbral_magnitude: f64,
    /// Penumbral contacts (P1, P4).
    pub penumbral_start_ms: f64,
    pub penumbral_end_ms: f64,
    /// Partial phase (U1, U4) and totality (U2, U3).
    pub partial_start_ms: Option<f64>,
    pub partial_end_ms: Option<f64>,
    pub total_start_ms: Option<f64>,
    pub total_end_ms: Option<f64>,
    /// The Moon's altitude at greatest eclipse, from this place.
    pub moon_alt_at_max: f64,
    /// Whether the Moon is up for some of it.
    pub visible: bool,
}

fn lunar_mag_at(ms: f64) -> (f64, f64, f64) {
    let g = shadow_geometry(jde_from_unix_ms(ms));
    (
        (g.umbra + g.moon_radius - g.separation) / (2.0 * g.moon_radius),
        (g.penumbra + g.moon_radius - g.separation) / (2.0 * g.moon_radius),
        g.separation,
    )
}

/// The first lunar eclipse from `from_ms` on that has the Moon up from `o`
/// for some of it, within `within_days`.
pub fn next_lunar_eclipse(from_ms: f64, o: &Observer, within_days: f64) -> Option<LunarEclipse> {
    for full in lunations(from_ms - DAY_MS, Phase::Full) {
        if full > from_ms + within_days * DAY_MS {
            return None;
        }
        if moon_latitude(full).abs() > ECLIPSE_LIMIT {
            continue;
        }
        let Some(e) = lunar_eclipse_near(full, o) else {
            continue;
        };
        if e.penumbral_end_ms >= from_ms && e.visible {
            return Some(e);
        }
    }
    None
}

fn lunar_eclipse_near(full: f64, o: &Observer) -> Option<LunarEclipse> {
    let step = 120_000.0;
    let span = 5.0 * 3_600_000.0;
    let max_ms = refine_min(full - span, full + span, |t| lunar_mag_at(t).2);
    let (umbral, penumbral, _) = lunar_mag_at(max_ms);
    if penumbral <= 0.0 {
        return None;
    }
    let kind = if umbral >= 1.0 {
        LunarKind::Total
    } else if umbral > 0.0 {
        LunarKind::Partial
    } else {
        LunarKind::Penumbral
    };
    // A contact is where a magnitude crosses zero (outer edge) or one (inner edge).
    let find = |level: f64, which: fn((f64, f64, f64)) -> f64| -> Option<(f64, f64)> {
        let above = |t: f64| which(lunar_mag_at(t)) > level;
        if !above(max_ms) {
            return None;
        }
        let (mut a, mut b) = (max_ms, max_ms);
        let mut n = 0;
        while above(a - step) && n < 150 {
            a -= step;
            n += 1;
        }
        let mut m = 0;
        while above(b + step) && m < 150 {
            b += step;
            m += 1;
        }
        Some((bisect(a, a - step, above), bisect(b, b + step, above)))
    };
    let (p1, p4) = find(0.0, |m| m.1)?;
    let partial = find(0.0, |m| m.0);
    let total = find(1.0, |m| m.0);
    let sky = sky_at(max_ms, o);
    let up_at = |t: f64| sky_at(t, o).moon.alt > -0.5 * D2R;
    let mut visible = up_at(p1) || up_at(p4) || up_at(max_ms);
    if !visible {
        let mut t = p1;
        while t < p4 {
            if up_at(t) {
                visible = true;
                break;
            }
            t += 10.0 * 60_000.0;
        }
    }
    Some(LunarEclipse {
        kind,
        max_ms,
        umbral_magnitude: umbral,
        penumbral_magnitude: penumbral,
        penumbral_start_ms: p1,
        penumbral_end_ms: p4,
        partial_start_ms: partial.map(|p| p.0),
        partial_end_ms: partial.map(|p| p.1),
        total_start_ms: total.map(|p| p.0),
        total_end_ms: total.map(|p| p.1),
        moon_alt_at_max: sky.moon.alt,
        visible,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> f64 {
        // Days from civil (Howard Hinnant).
        let y = if mo <= 2 { y - 1 } else { y } as i64;
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let m = i64::from(mo);
        let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(d) - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        let days = era * 146_097 + doe - 719_468;
        (days as f64) * DAY_MS + f64::from(h) * 3_600_000.0 + f64::from(mi) * 60_000.0
    }

    #[test]
    fn total_solar_eclipse_2024_04_08_in_dallas() {
        // Dallas, 32.78 N 96.80 W: totality ~18:40–18:44 UT (1:40 pm CDT), lasting ~3m52s; greatest eclipse 18:17 UT globally.
        let dallas = Observer::new(32.7767, -96.7970);
        let e = next_solar_eclipse(utc(2024, 4, 1, 0, 0), &dallas, 30.0).expect("eclipse");
        assert_eq!(e.kind, SolarKind::Total, "{e:?}");
        let begins = e.central_start_ms.unwrap();
        // 1:40–1:42 pm CDT = 18:40–18:42 UT.
        assert!(
            (begins - utc(2024, 4, 8, 18, 41)).abs() < 150_000.0,
            "totality begins {} s off 18:41",
            (begins - utc(2024, 4, 8, 18, 41)) / 1000.0
        );
        let secs = e.central_seconds().unwrap();
        assert!((secs - 232.0).abs() < 25.0, "totality {secs}s");
        assert!(
            e.magnitude > 1.0 && e.magnitude < 1.07,
            "magnitude {}",
            e.magnitude
        );
    }

    #[test]
    fn total_solar_eclipse_2026_08_12_in_spain() {
        // Burgos-ish (42.34 N, 3.70 W): totality ≈ 18:27 UT, a little over a minute.
        let burgos = Observer::new(42.34, -3.70);
        let e = next_solar_eclipse(utc(2026, 8, 1, 0, 0), &burgos, 30.0).expect("eclipse");
        assert_eq!(e.kind, SolarKind::Total, "{e:?}");
        let mid = 0.5 * (e.central_start_ms.unwrap() + e.central_end_ms.unwrap());
        assert!(
            (mid - utc(2026, 8, 12, 18, 28)).abs() < 4.0 * 60_000.0,
            "totality mid {} min off 18:28",
            (mid - utc(2026, 8, 12, 18, 28)) / 60_000.0
        );
        assert!(e.sun_alt_at_max > 0.0 && e.sun_alt_at_max < 12.0 * D2R);
    }

    #[test]
    fn no_eclipse_where_there_is_none() {
        // The 2024-04-08 eclipse was not visible from Sydney.
        let sydney = Observer::new(-33.87, 151.21);
        let e = next_solar_eclipse(utc(2024, 4, 1, 0, 0), &sydney, 10.0);
        assert!(e.is_none(), "{e:?}");
    }

    #[test]
    fn lunar_eclipses_match_the_published_ones() {
        let london = Observer::new(51.5, -0.1);
        // 2025-03-14: total, greatest 06:58 UT, umbral magnitude 1.178.
        let e = next_lunar_eclipse(utc(2025, 3, 1, 0, 0), &london, 30.0).expect("eclipse");
        assert_eq!(e.kind, LunarKind::Total, "{e:?}");
        assert!(
            (e.max_ms - utc(2025, 3, 14, 6, 59)).abs() < 3.0 * 60_000.0,
            "{}",
            (e.max_ms - utc(2025, 3, 14, 6, 59)) / 60_000.0
        );
        assert!(
            (e.umbral_magnitude - 1.178).abs() < 0.02,
            "umbral {}",
            e.umbral_magnitude
        );
        // 2025-09-07: total, greatest 18:12 UT, umbral magnitude 1.362 (seen from Delhi, Moon up).
        let delhi = Observer::new(28.6, 77.2);
        let e = next_lunar_eclipse(utc(2025, 9, 1, 0, 0), &delhi, 30.0).expect("eclipse");
        assert!(
            (e.umbral_magnitude - 1.362).abs() < 0.02,
            "umbral {}",
            e.umbral_magnitude
        );
        assert!((e.max_ms - utc(2025, 9, 7, 18, 12)).abs() < 3.0 * 60_000.0);
        // 2026-08-28: partial, 0.930.
        let e = next_lunar_eclipse(utc(2026, 8, 20, 0, 0), &Observer::new(40.0, -100.0), 30.0)
            .expect("eclipse");
        assert_eq!(e.kind, LunarKind::Partial);
        assert!(
            (e.umbral_magnitude - 0.930).abs() < 0.02,
            "umbral {}",
            e.umbral_magnitude
        );
    }

    #[test]
    fn overlap_geometry() {
        assert_eq!(overlap_fraction(1.0, 1.0, 2.0), 0.0);
        assert!((overlap_fraction(1.0, 1.0, 0.0) - 1.0).abs() < 1e-12);
        assert!((overlap_fraction(1.0, 0.5, 0.0) - 0.25).abs() < 1e-12);
        let half = overlap_fraction(1.0, 1.0, 1.0);
        assert!(half > 0.38 && half < 0.40, "{half}");
    }
}
