//! The almanac in words: rising and setting, the Moon's phase, what is up
//! tonight. The same sentences serve the prompt's `sky`, `moon` and `tonight`
//! and the `nus` command.

use crate::ephem::{
    jde_from_unix_ms, moon_geocentric, planet, sidereal_time, sky_at, sun_geocentric, topocentric,
    Planet,
};
use crate::events::{clock, date, date_year, next_phase, span, upcoming, Phase};
use crate::{horizontal, Observer, D2R};

/// Rising and setting of a body within a window.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RiseSet {
    pub rise: Option<f64>,
    pub set: Option<f64>,
}

/// First rising and first setting of whatever `alt` tracks (radians above the
/// horizon at a Unix time), crossing `h0`, between two times.
pub fn rise_set(alt: impl Fn(f64) -> f64, from_ms: f64, to_ms: f64, h0: f64) -> RiseSet {
    let step = 10.0 * 60_000.0;
    let mut out = RiseSet::default();
    let mut t = from_ms;
    let mut prev = alt(t) - h0;
    while t < to_ms && (out.rise.is_none() || out.set.is_none()) {
        let next = t + step;
        let cur = alt(next) - h0;
        if (prev < 0.0) != (cur < 0.0) {
            let (mut lo, mut hi) = (t, next);
            for _ in 0..24 {
                let mid = 0.5 * (lo + hi);
                if (alt(mid) - h0 < 0.0) == (prev < 0.0) {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            let at = 0.5 * (lo + hi);
            if prev < 0.0 && out.rise.is_none() {
                out.rise = Some(at);
            } else if prev >= 0.0 && out.set.is_none() {
                out.set = Some(at);
            }
        }
        prev = cur;
        t = next;
    }
    out
}

fn sun_alt(ms: f64, o: &Observer) -> f64 {
    let jde = jde_from_unix_ms(ms);
    topocentric(&sun_geocentric(jde), o, sidereal_time(ms, o.lon_deg)).alt
}

fn moon_alt(ms: f64, o: &Observer) -> f64 {
    let jde = jde_from_unix_ms(ms);
    topocentric(&moon_geocentric(jde), o, sidereal_time(ms, o.lon_deg)).alt
}

fn planet_alt(p: Planet, ms: f64, o: &Observer) -> f64 {
    let q = planet(p, ms);
    horizontal(q.ra, q.dec, ms, o)[1].asin()
}

const SUN_H0: f64 = -0.8333 * D2R;
const MOON_H0: f64 = -0.8167 * D2R;

/// What the almanac has to say: a heading and its lines.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    pub title: String,
    pub lines: Vec<String>,
}

fn compass(az: f64) -> &'static str {
    const NAMES: [&str; 16] = [
        "north",
        "north-northeast",
        "northeast",
        "east-northeast",
        "east",
        "east-southeast",
        "southeast",
        "south-southeast",
        "south",
        "south-southwest",
        "southwest",
        "west-southwest",
        "west",
        "west-northwest",
        "northwest",
        "north-northwest",
    ];
    NAMES[(((az / (std::f64::consts::TAU / 16.0)).round() as i64).rem_euclid(16)) as usize]
}

fn mag(m: f64) -> String {
    format!("{}{:.1}", if m < 0.0 { "−" } else { "+" }, m.abs())
}

fn phase_name(elongation_deg: f64) -> &'static str {
    match elongation_deg {
        e if !(12.0..348.0).contains(&e) => "New Moon",
        e if e < 78.0 => "Waxing crescent",
        e if e < 102.0 => "First quarter",
        e if e < 168.0 => "Waxing gibbous",
        e if e < 192.0 => "Full Moon",
        e if e < 258.0 => "Waning gibbous",
        e if e < 282.0 => "Last quarter",
        _ => "Waning crescent",
    }
}

/// The Moon's phase in words, from how far round its orbit it is from the Sun.
pub fn moon_phase_name(ms: f64) -> &'static str {
    let jde = jde_from_unix_ms(ms);
    phase_name(
        (moon_geocentric(jde).lon - sun_geocentric(jde).lon).rem_euclid(std::f64::consts::TAU)
            / D2R,
    )
}

fn no_place() -> Report {
    Report {
        title: "No place set".into(),
        lines: vec![
            "Choose where you are (Settings · Start · Place) and the sky answers for there.".into(),
            "Nothing is looked up: it is computed here, from where you say you are.".into(),
        ],
    }
}

/// The Moon: phase and age, the next quarters, and — with a place — its rising and setting.
pub fn moon_report(ms: f64, observer: Option<&Observer>, tz_min: i32) -> Report {
    let jde = jde_from_unix_ms(ms);
    let (s, m) = (sun_geocentric(jde), moon_geocentric(jde));
    let elong = (m.lon - s.lon).rem_euclid(std::f64::consts::TAU) / D2R;
    let sky = sky_at(ms, &observer.copied().unwrap_or(Observer::new(0.0, 0.0)));
    let mut lines = vec![format!(
        "{} · {:.0}% lit · {:.1} days old",
        phase_name(elong),
        sky.illumination * 100.0,
        elong / 360.0 * 29.530588
    )];
    let mean = 384_400.0;
    let pct = (m.dist_km / mean - 1.0) * 100.0;
    lines.push(format!(
        "{:.0} km away · {:.1}% {} than its average",
        m.dist_km,
        pct.abs(),
        if pct < 0.0 { "closer" } else { "farther" }
    ));
    let mut coming: Vec<(f64, Phase)> = [
        Phase::New,
        Phase::FirstQuarter,
        Phase::Full,
        Phase::LastQuarter,
    ]
    .into_iter()
    .map(|p| (next_phase(ms, p), p))
    .collect();
    coming.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (t, p) in coming {
        lines.push(format!(
            "Next {} · {} {} · in {}",
            p.name().to_lowercase(),
            date(t, tz_min),
            clock(t, tz_min),
            span(t - ms)
        ));
    }
    if let Some(o) = observer {
        let rs = rise_set(|t| moon_alt(t, o), ms, ms + 30.0 * 3_600_000.0, MOON_H0);
        let up = sky.moon.alt > MOON_H0;
        let mut events: Vec<(f64, &str)> = Vec::new();
        events.extend(rs.rise.map(|t| (t, "rises")));
        events.extend(rs.set.map(|t| (t, "sets")));
        events.sort_by(|a, b| a.0.total_cmp(&b.0));
        let parts: Vec<String> = events
            .iter()
            .map(|(t, w)| format!("{w} {} {}", date(*t, tz_min), clock(*t, tz_min)))
            .collect();
        lines.push(if up {
            format!(
                "up now · {:.0}° above the {} · {}",
                sky.moon.alt / D2R,
                compass(sky.moon.az),
                parts.join(" · ")
            )
        } else {
            format!("below the horizon · {}", parts.join(" · "))
        });
    } else {
        lines.push("Set a place for its rising and setting.".into());
    }
    Report {
        title: "The Moon".into(),
        lines,
    }
}

/// The sky right now: the Sun, the Moon, the planets above the horizon.
pub fn sky_report(ms: f64, observer: Option<&Observer>, tz_min: i32) -> Report {
    let Some(o) = observer else { return no_place() };
    let sky = sky_at(ms, o);
    let mut lines = Vec::new();
    let sun_alt_deg = sky.sun.alt / D2R;
    let rs = rise_set(|t| sun_alt(t, o), ms, ms + 30.0 * 3_600_000.0, SUN_H0);
    lines.push(if sun_alt_deg > -0.83 {
        format!(
            "Sun · {:.0}° above the {}{}",
            sun_alt_deg,
            compass(sky.sun.az),
            rs.set
                .map_or(String::new(), |t| format!(" · sets {}", clock(t, tz_min)))
        )
    } else {
        format!(
            "Sun · {:.0}° below the horizon{}",
            -sun_alt_deg,
            rs.rise
                .map_or(String::new(), |t| format!(" · rises {}", clock(t, tz_min)))
        )
    });
    let jde = jde_from_unix_ms(ms);
    let elong = (moon_geocentric(jde).lon - sun_geocentric(jde).lon)
        .rem_euclid(std::f64::consts::TAU)
        / D2R;
    lines.push(if sky.moon.alt > 0.0 {
        format!(
            "Moon · {} · {:.0}% lit · {:.0}° above the {}",
            phase_name(elong).to_lowercase(),
            sky.illumination * 100.0,
            sky.moon.alt / D2R,
            compass(sky.moon.az)
        )
    } else {
        format!(
            "Moon · {} · {:.0}% lit · below the horizon",
            phase_name(elong).to_lowercase(),
            sky.illumination * 100.0
        )
    });
    for p in Planet::ALL.into_iter().filter(|p| p.naked_eye()) {
        let q = planet(p, ms);
        let h = horizontal(q.ra, q.dec, ms, o);
        let alt = h[1].asin();
        if alt > 5.0 * D2R && q.elongation > 15.0 * D2R {
            let az = h[0].atan2(h[2]).rem_euclid(std::f64::consts::TAU);
            let glare = if sun_alt_deg > -6.0 {
                " · in daylight"
            } else {
                ""
            };
            lines.push(format!(
                "{} · {} · {:.0}° above the {}{}",
                p.name(),
                mag(q.magnitude),
                alt / D2R,
                compass(az),
                glare
            ));
        }
    }
    if let Some(e) = upcoming(ms, Some(o), 120.0).into_iter().find(|e| {
        e.peak_ms > ms - 3_600_000.0 && !matches!(e.kind, crate::events::EventKind::Unix(_))
    }) {
        lines.push(format!("Next · {}", e.note(ms, tz_min)));
    }
    Report {
        title: format!("The sky · {} {}", date(ms, tz_min), clock(ms, tz_min)),
        lines,
    }
}

/// Tonight: dusk, dark, dawn, the Moon, and what is worth going out for.
pub fn tonight_report(ms: f64, observer: Option<&Observer>, tz_min: i32) -> Report {
    let Some(o) = observer else { return no_place() };
    let mut lines = Vec::new();
    // The night this belongs to: if the Sun is down, the one under way (last sunset to
    // next sunrise); if it is up, the coming one.
    let up = sun_alt(ms, o) > SUN_H0;
    let hour = 3_600_000.0;
    let (night_start, night_end) = if up {
        let set = rise_set(|t| sun_alt(t, o), ms, ms + 24.0 * hour, SUN_H0).set;
        let rise = set.and_then(|s| rise_set(|t| sun_alt(t, o), s, s + 24.0 * hour, SUN_H0).rise);
        (set, rise)
    } else {
        let set = rise_set(|t| sun_alt(t, o), ms - 20.0 * hour, ms, SUN_H0).set;
        let rise = rise_set(|t| sun_alt(t, o), ms, ms + 24.0 * hour, SUN_H0).rise;
        (set, rise)
    };
    let (Some(night_start), Some(night_end)) = (night_start, night_end) else {
        lines.push(if up {
            "The Sun does not set within a day of now.".into()
        } else {
            "The Sun does not rise within a day of now.".into()
        });
        return Report {
            title: "Tonight".into(),
            lines,
        };
    };
    lines.push(format!(
        "Sunset {} · sunrise {}",
        clock(night_start, tz_min),
        clock(night_end, tz_min)
    ));
    // Fully dark: the Sun more than 18° down.
    let dark = |t: f64| sun_alt(t, o) < -18.0 * D2R;
    let quarter_hour = 15.0 * 60_000.0;
    let dark_start = (0..80)
        .map(|i| night_start + f64::from(i) * quarter_hour)
        .take_while(|t| *t < night_end)
        .find(|t| dark(*t));
    let dark_end = dark_start.and_then(|s| {
        (0..80)
            .map(|i| s + f64::from(i) * quarter_hour)
            .take_while(|t| *t <= night_end)
            .filter(|t| dark(*t))
            .last()
    });
    match (dark_start, dark_end) {
        (Some(a), Some(b)) => lines.push(format!(
            "Fully dark {} – {}",
            clock(a, tz_min),
            clock(b, tz_min)
        )),
        _ => lines.push("It does not get fully dark tonight.".into()),
    }
    let sky = sky_at(night_start + (night_end - night_start) / 2.0, o);
    let jde = jde_from_unix_ms(ms);
    let elong = (moon_geocentric(jde).lon - sun_geocentric(jde).lon)
        .rem_euclid(std::f64::consts::TAU)
        / D2R;
    let moon = rise_set(|t| moon_alt(t, o), night_start, night_end, MOON_H0);
    let mut moon_line = format!(
        "Moon · {} · {:.0}% lit",
        phase_name(elong).to_lowercase(),
        sky.illumination * 100.0
    );
    if let Some(t) = moon.rise {
        moon_line += &format!(" · rises {}", clock(t, tz_min));
    }
    if let Some(t) = moon.set {
        moon_line += &format!(" · sets {}", clock(t, tz_min));
    }
    if moon.rise.is_none() && moon.set.is_none() {
        moon_line += if moon_alt(night_start, o) > 0.0 {
            " · up all night"
        } else {
            " · down all night"
        };
    }
    lines.push(moon_line);
    for p in Planet::ALL.into_iter().filter(|p| p.naked_eye()) {
        let q = planet(p, night_start);
        if q.elongation < 15.0 * D2R {
            continue;
        }
        let mut best = (-90.0_f64, 0.0);
        let (mut first, mut last) = (None, None);
        let mut t = night_start;
        while t <= night_end {
            let alt = planet_alt(p, t, o) / D2R;
            if alt > 8.0 {
                if first.is_none() {
                    first = Some(t);
                }
                last = Some(t);
                if alt > best.0 {
                    best = (alt, t);
                }
            }
            t += 30.0 * 60_000.0;
        }
        if let (Some(a), Some(b)) = (first, last) {
            let h = horizontal(planet(p, best.1).ra, planet(p, best.1).dec, best.1, o);
            let az = h[0].atan2(h[2]).rem_euclid(std::f64::consts::TAU);
            lines.push(format!(
                "{} · {} · up {} – {} · {:.0}° at {} in the {}",
                p.name(),
                mag(planet(p, best.1).magnitude),
                clock(a, tz_min),
                clock(b, tz_min),
                best.0,
                clock(best.1, tz_min),
                compass(az)
            ));
        }
    }
    for e in upcoming(night_start - 6.0 * 3_600_000.0, Some(o), 2.0) {
        if e.peak_ms >= night_start - 6.0 * 3_600_000.0
            && e.peak_ms <= night_end + 6.0 * 3_600_000.0
            && !matches!(
                e.kind,
                crate::events::EventKind::Unix(_) | crate::events::EventKind::Season(_)
            )
        {
            lines.push(e.note(ms, tz_min));
        }
    }
    Report {
        title: format!("Tonight · {}", date_year(night_start, tz_min)),
        lines,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DAY_MS;

    #[test]
    fn rise_and_set_in_london_at_the_june_solstice() {
        // 2025-06-21, London: sunrise ≈ 04:43 BST (03:43 UT), sunset ≈ 21:21 BST (20:21 UT).
        let o = Observer::new(51.5074, -0.1278);
        let from = (crate::events::days_from_civil(2025, 6, 21) as f64) * DAY_MS;
        let rs = rise_set(|t| sun_alt(t, &o), from, from + DAY_MS, SUN_H0);
        let rise = rs.rise.unwrap();
        let set = rs.set.unwrap();
        assert!(
            (rise - (from + (3.0 * 60.0 + 43.0) * 60_000.0)).abs() < 3.0 * 60_000.0,
            "rise {}",
            clock(rise, 0)
        );
        assert!(
            (set - (from + (20.0 * 60.0 + 21.0) * 60_000.0)).abs() < 3.0 * 60_000.0,
            "set {}",
            clock(set, 0)
        );
    }

    #[test]
    fn reports_have_something_to_say() {
        let o = Observer::new(40.7, -74.0);
        let ms = 1_790_000_000_000.0;
        assert!(moon_report(ms, Some(&o), -240).lines.len() >= 6);
        assert!(sky_report(ms, Some(&o), -240).lines.len() >= 2);
        assert!(tonight_report(ms, Some(&o), -240).lines.len() >= 3);
        assert_eq!(sky_report(ms, None, 0).title, "No place set");
    }

    #[test]
    fn phases_have_names() {
        assert_eq!(phase_name(2.0), "New Moon");
        assert_eq!(phase_name(180.0), "Full Moon");
        assert_eq!(phase_name(100.0), "First quarter");
    }
}
