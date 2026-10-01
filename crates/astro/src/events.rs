//! What happens in the sky, and when: phases, the turning of the seasons,
//! meteor showers, close pairs, eclipses — and the occasional moment that
//! belongs to the machine rather than the heavens.

use crate::eclipse::{
    next_notable_lunar_eclipse, next_notable_solar_eclipse, LunarEclipse, LunarKind, SolarEclipse,
    SolarKind,
};
use crate::ephem::{jde_from_unix_ms, moon_geocentric, planet, sun_geocentric, Planet};
use crate::{wrap, wrap_pm, Observer, D2R, DAY_MS};

// ── time, written out ───────────────────────────────────────────────────

/// (year, month 1–12, day 1–31) of a day number since 1970-01-01.
pub(crate) fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

pub(crate) fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = i64::from(if m > 2 { m - 3 } else { m + 9 });
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const WEEKDAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];

/// "14:22" in the zone `tz_min` minutes east of UTC.
pub fn clock(ms: f64, tz_min: i32) -> String {
    let local = ms + f64::from(tz_min) * 60_000.0;
    let secs = (local / 1000.0).floor() as i64;
    let sod = secs.rem_euclid(86_400);
    format!("{:02}:{:02}", sod / 3600, (sod % 3600) / 60)
}

/// "Wed 12 Aug".
pub fn date(ms: f64, tz_min: i32) -> String {
    let local = ms + f64::from(tz_min) * 60_000.0;
    let days = (local / DAY_MS).floor() as i64;
    let (_, m, d) = civil_from_days(days);
    format!(
        "{} {} {}",
        WEEKDAYS[days.rem_euclid(7) as usize],
        d,
        MONTHS[(m - 1) as usize]
    )
}

/// "12 Aug 2026".
pub fn date_year(ms: f64, tz_min: i32) -> String {
    let local = ms + f64::from(tz_min) * 60_000.0;
    let (y, m, d) = civil_from_days((local / DAY_MS).floor() as i64);
    format!("{} {} {}", d, MONTHS[(m - 1) as usize], y)
}

pub(crate) fn year_of(ms: f64) -> i32 {
    civil_from_days((ms / DAY_MS).floor() as i64).0 as i32
}

/// "4 min", "1 h 12 min", "3 days".
pub fn span(ms: f64) -> String {
    let s = (ms / 1000.0).abs();
    if s < 90.0 {
        format!("{} s", s.round() as i64)
    } else if s < 90.0 * 60.0 {
        format!("{} min", (s / 60.0).round() as i64)
    } else if s < 36.0 * 3600.0 {
        let m = (s / 60.0).round() as i64;
        format!("{} h {:02} min", m / 60, m % 60)
    } else {
        let d = (s / 86_400.0).round() as i64;
        format!("{d} day{}", if d == 1 { "" } else { "s" })
    }
}

// ── phases ──────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    New,
    FirstQuarter,
    Full,
    LastQuarter,
}

impl Phase {
    fn target(self) -> f64 {
        match self {
            Phase::New => 0.0,
            Phase::FirstQuarter => 90.0 * D2R,
            Phase::Full => 180.0 * D2R,
            Phase::LastQuarter => 270.0 * D2R,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Phase::New => "New Moon",
            Phase::FirstQuarter => "First quarter",
            Phase::Full => "Full Moon",
            Phase::LastQuarter => "Last quarter",
        }
    }
}

fn elongation(ms: f64) -> f64 {
    let jde = jde_from_unix_ms(ms);
    wrap(moon_geocentric(jde).lon - sun_geocentric(jde).lon)
}

/// The next time the Moon is this far round its orbit from the Sun.
pub fn next_phase(from_ms: f64, phase: Phase) -> f64 {
    let target = phase.target();
    let mut t = from_ms + wrap(target - elongation(from_ms)) / (12.19 * D2R) * DAY_MS;
    for _ in 0..12 {
        let diff = wrap_pm(target - elongation(t));
        let rate = wrap_pm(elongation(t + 600_000.0) - elongation(t)) / (600_000.0 / DAY_MS);
        t += diff / rate.max(0.15) * DAY_MS;
        if diff.abs() < 2e-7 {
            break;
        }
    }
    // Landing just before `from_ms` means this phase has only just passed.
    if t < from_ms - 3_600_000.0 {
        t += 29.530588 * DAY_MS;
        return next_phase(t - 3.0 * DAY_MS, phase);
    }
    t
}

// ── seasons ─────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Season {
    MarchEquinox,
    JuneSolstice,
    SeptemberEquinox,
    DecemberSolstice,
}

impl Season {
    pub const ALL: [Season; 4] = [
        Season::MarchEquinox,
        Season::JuneSolstice,
        Season::SeptemberEquinox,
        Season::DecemberSolstice,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Season::MarchEquinox => "March equinox",
            Season::JuneSolstice => "June solstice",
            Season::SeptemberEquinox => "September equinox",
            Season::DecemberSolstice => "December solstice",
        }
    }
    fn longitude(self) -> f64 {
        match self {
            Season::MarchEquinox => 0.0,
            Season::JuneSolstice => 90.0 * D2R,
            Season::SeptemberEquinox => 180.0 * D2R,
            Season::DecemberSolstice => 270.0 * D2R,
        }
    }
    fn guess_ordinal(self) -> f64 {
        // Days from 1 January (0-based) of the approximate instant.
        match self {
            Season::MarchEquinox => 78.6,
            Season::JuneSolstice => 171.4,
            Season::SeptemberEquinox => 264.7,
            Season::DecemberSolstice => 354.6,
        }
    }
}

/// Solve for the Unix time at which the Sun's apparent longitude (equinox of
/// date, `precess` degrees added for catalogues quoted at J2000) reaches `target`.
fn solar_longitude_time(year: i32, guess_ordinal: f64, target: f64) -> f64 {
    let mut t = (days_from_civil(i64::from(year), 1, 1) as f64 + guess_ordinal) * DAY_MS;
    for _ in 0..10 {
        let lon = sun_geocentric(jde_from_unix_ms(t)).lon;
        let diff = wrap_pm(target - lon);
        t += diff / (0.9856 * D2R) * DAY_MS;
        if diff.abs() < 1e-8 {
            break;
        }
    }
    t
}

/// The instant of an equinox or solstice, as a Unix time in milliseconds.
pub fn season_instant(year: i32, season: Season) -> f64 {
    solar_longitude_time(year, season.guess_ordinal(), season.longitude())
}

pub fn seasons_in_year(year: i32) -> [(Season, f64); 4] {
    Season::ALL.map(|s| (s, season_instant(year, s)))
}

// ── meteor showers ──────────────────────────────────────────────────────

/// A shower's peak, from the International Meteor Organization's calendar.
pub struct Shower {
    pub name: &'static str,
    /// Solar longitude of the peak, J2000.
    pub lon_j2000_deg: f64,
    /// Zenithal hourly rate at the peak, under a perfect sky.
    pub zhr: u32,
    pub ra_deg: f64,
    pub dec_deg: f64,
}

pub const SHOWERS: [Shower; 10] = [
    Shower {
        name: "Quadrantids",
        lon_j2000_deg: 283.15,
        zhr: 110,
        ra_deg: 230.0,
        dec_deg: 49.0,
    },
    Shower {
        name: "Lyrids",
        lon_j2000_deg: 32.32,
        zhr: 18,
        ra_deg: 271.0,
        dec_deg: 34.0,
    },
    Shower {
        name: "Eta Aquariids",
        lon_j2000_deg: 45.5,
        zhr: 50,
        ra_deg: 338.0,
        dec_deg: -1.0,
    },
    Shower {
        name: "Delta Aquariids",
        lon_j2000_deg: 125.0,
        zhr: 25,
        ra_deg: 340.0,
        dec_deg: -16.0,
    },
    Shower {
        name: "Perseids",
        lon_j2000_deg: 140.0,
        zhr: 100,
        ra_deg: 48.0,
        dec_deg: 58.0,
    },
    Shower {
        name: "Draconids",
        lon_j2000_deg: 195.4,
        zhr: 10,
        ra_deg: 262.0,
        dec_deg: 54.0,
    },
    Shower {
        name: "Orionids",
        lon_j2000_deg: 208.0,
        zhr: 20,
        ra_deg: 95.0,
        dec_deg: 16.0,
    },
    Shower {
        name: "Leonids",
        lon_j2000_deg: 235.27,
        zhr: 15,
        ra_deg: 152.0,
        dec_deg: 22.0,
    },
    Shower {
        name: "Geminids",
        lon_j2000_deg: 262.2,
        zhr: 150,
        ra_deg: 112.0,
        dec_deg: 33.0,
    },
    Shower {
        name: "Ursids",
        lon_j2000_deg: 270.7,
        zhr: 10,
        ra_deg: 217.0,
        dec_deg: 76.0,
    },
];

impl Shower {
    pub fn peak_ms(&self, year: i32) -> f64 {
        // Catalogue longitudes are J2000; the Sun's longitude of date has since
        // run ahead by the precession of the equinoxes (about 1.397° a century).
        let t = f64::from(year - 2000) / 100.0;
        let target = wrap((self.lon_j2000_deg + 1.396971 * t) * D2R);
        solar_longitude_time(
            year,
            self.lon_j2000_deg / 360.0 * 365.2422 + 78.6
                - if self.lon_j2000_deg > 281.0 {
                    365.2422
                } else {
                    0.0
                },
            target,
        )
    }
}

// ── a moment that belongs to the machine ────────────────────────────────

/// Round and rare numbers on the Unix clock, and what they are called.
pub const UNIX_MOMENTS: [(i64, &str); 6] = [
    (1_800_000_000, "1,800,000,000"),
    (1_900_000_000, "1,900,000,000"),
    (2_000_000_000, "2,000,000,000"),
    (2_100_000_000, "2,100,000,000"),
    (2_147_483_648, "2³¹ · the 32-bit horizon"),
    (2_200_000_000, "2,200,000,000"),
];

// ── events ──────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
pub enum EventKind {
    Solar(SolarEclipse),
    Lunar(LunarEclipse),
    Season(Season),
    Shower(usize),
    /// Two bodies close together: names, separation in degrees.
    Pair(&'static str, &'static str, f64),
    Unix(i64),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub kind: EventKind,
    pub start_ms: f64,
    pub peak_ms: f64,
    pub end_ms: f64,
}

impl Event {
    /// A name that does not change: "Total solar eclipse", "Perseids".
    pub fn title(&self) -> String {
        match &self.kind {
            EventKind::Solar(e) => format!(
                "{} solar eclipse",
                match e.kind {
                    SolarKind::Total => "Total",
                    SolarKind::Annular => "Annular",
                    SolarKind::Partial => "Partial",
                }
            ),
            EventKind::Lunar(e) => format!(
                "{} lunar eclipse",
                match e.kind {
                    LunarKind::Total => "Total",
                    LunarKind::Partial => "Partial",
                    LunarKind::Penumbral => "Penumbral",
                }
            ),
            EventKind::Season(s) => s.name().to_string(),
            EventKind::Shower(i) => SHOWERS[*i].name.to_string(),
            EventKind::Pair(a, b, sep) => format!("{a} and {b}, {sep:.1}° apart"),
            EventKind::Unix(n) => UNIX_MOMENTS
                .iter()
                .find(|(v, _)| v == n)
                .map_or_else(|| n.to_string(), |(_, s)| (*s).to_string()),
        }
    }

    /// One line for the prompt, in terms of `now`: counting down, or under way.
    pub fn note(&self, now_ms: f64, tz_min: i32) -> String {
        let until = self.peak_ms - now_ms;
        match &self.kind {
            EventKind::Solar(e) => {
                let central = matches!(e.kind, SolarKind::Total | SolarKind::Annular);
                let word = if e.kind == SolarKind::Total {
                    "totality"
                } else {
                    "the ring"
                };
                if now_ms >= e.start_ms && now_ms <= e.end_ms {
                    if let (Some(a), Some(b)) = (e.central_start_ms, e.central_end_ms) {
                        if now_ms < a {
                            return format!("{word} in {}", span(a - now_ms));
                        }
                        if now_ms <= b {
                            return format!("{word} · {} left", span(b - now_ms));
                        }
                        return "the Sun returns".to_string();
                    }
                    return format!(
                        "partial eclipse · {:.0}% covered at {}",
                        e.obscuration * 100.0,
                        clock(e.max_ms, tz_min)
                    );
                }
                if now_ms < e.start_ms && e.start_ms - now_ms < 6.0 * 3_600_000.0 {
                    return format!(
                        "{} solar eclipse begins in {}",
                        if central {
                            self.title().split(' ').next().unwrap_or("").to_lowercase()
                        } else {
                            "partial".into()
                        },
                        span(e.start_ms - now_ms)
                    );
                }
                format!(
                    "{} · {}",
                    self.title().to_lowercase(),
                    when(now_ms, e.max_ms, tz_min)
                )
            }
            EventKind::Lunar(e) => {
                if now_ms >= e.penumbral_start_ms && now_ms <= e.penumbral_end_ms {
                    if let (Some(a), Some(b)) = (e.total_start_ms, e.total_end_ms) {
                        if now_ms < a {
                            return format!("totality in {}", span(a - now_ms));
                        }
                        if now_ms <= b {
                            return format!("totality · {} left", span(b - now_ms));
                        }
                    }
                    return format!(
                        "the Moon in Earth's shadow · greatest at {}",
                        clock(e.max_ms, tz_min)
                    );
                }
                format!(
                    "{} · {}",
                    self.title().to_lowercase(),
                    when(now_ms, e.max_ms, tz_min)
                )
            }
            EventKind::Season(s) => {
                if until.abs() < 3_600_000.0 {
                    format!("{} · now", s.name())
                } else {
                    format!("{} · {}", s.name(), when(now_ms, self.peak_ms, tz_min))
                }
            }
            EventKind::Shower(i) => format!(
                "{} peak · {}",
                SHOWERS[*i].name,
                when(now_ms, self.peak_ms, tz_min)
            ),
            EventKind::Pair(..) => {
                format!("{} · {}", self.title(), when(now_ms, self.peak_ms, tz_min))
            }
            EventKind::Unix(n) => {
                let label = UNIX_MOMENTS
                    .iter()
                    .find(|(v, _)| v == n)
                    .map_or_else(|| n.to_string(), |(_, s)| (*s).to_string());
                if until.abs() < 90_000.0 {
                    format!("unix {label} · now")
                } else {
                    format!("unix {label} · {}", when(now_ms, self.peak_ms, tz_min))
                }
            }
        }
    }
}

/// "in 3 days", "tomorrow 14:22", "today 14:22".
fn when(now: f64, at: f64, tz_min: i32) -> String {
    let (a, b) = (
        now + f64::from(tz_min) * 60_000.0,
        at + f64::from(tz_min) * 60_000.0,
    );
    let days = (b / DAY_MS).floor() - (a / DAY_MS).floor();
    match days as i64 {
        0 => format!("today {}", clock(at, tz_min)),
        1 => format!("tomorrow {}", clock(at, tz_min)),
        -1 => format!("yesterday {}", clock(at, tz_min)),
        2..=6 => format!("{} {}", date(at, tz_min), clock(at, tz_min)),
        _ => date_year(at, tz_min),
    }
}

/// The bright things that can sit near each other: the Moon and the naked-eye planets.
fn separation(a: (f64, f64), b: (f64, f64)) -> f64 {
    let c = (a.1.sin() * b.1.sin() + a.1.cos() * b.1.cos() * (a.0 - b.0).cos()).clamp(-1.0, 1.0);
    c.acos()
}

fn body_place(name: &str, ms: f64) -> (f64, f64) {
    if name == "the Moon" {
        let m = moon_geocentric(jde_from_unix_ms(ms));
        (m.ra, m.dec)
    } else {
        let p = Planet::ALL
            .into_iter()
            .find(|p| p.name() == name)
            .expect("planet");
        let q = planet(p, ms);
        (q.ra, q.dec)
    }
}

/// Close pairs between from and to: planet with planet inside 1.5°, the Moon
/// with a bright planet inside 3° (geocentric — the Moon shifts up to 1° by place).
fn pairs(from_ms: f64, to_ms: f64) -> Vec<Event> {
    const NAMES: [&str; 5] = ["Venus", "Mars", "Jupiter", "Saturn", "Mercury"];
    let mut out = Vec::new();
    let mut scan = |a: &'static str, b: &'static str, step: f64, limit: f64| {
        let f = |t: f64| separation(body_place(a, t), body_place(b, t));
        let mut t = from_ms;
        let (mut p0, mut p1) = (f(t), f(t + step));
        t += step;
        while t < to_ms {
            let p2 = f(t + step);
            if p1 < p0 && p1 <= p2 {
                // A minimum near t: refine by parabolic steps of a shrinking window.
                let (mut lo, mut hi) = (t - step, t + step);
                for _ in 0..30 {
                    let (m1, m2) = (lo + (hi - lo) / 3.0, hi - (hi - lo) / 3.0);
                    if f(m1) < f(m2) {
                        hi = m2
                    } else {
                        lo = m1
                    }
                }
                let at = 0.5 * (lo + hi);
                let sep = f(at) / D2R;
                if sep < limit {
                    out.push(Event {
                        kind: EventKind::Pair(a, b, sep),
                        start_ms: at - 6.0 * 3_600_000.0,
                        peak_ms: at,
                        end_ms: at + 6.0 * 3_600_000.0,
                    });
                }
            }
            p0 = p1;
            p1 = p2;
            t += step;
        }
    };
    for (i, a) in NAMES.iter().enumerate() {
        for b in &NAMES[i + 1..] {
            // Planet pairs drift slowly; a day is a fine step, and closest approach is what is wanted.
            scan(a, b, DAY_MS, 1.5);
        }
        scan("the Moon", a, 2.0 * 3_600_000.0, 3.0);
    }
    out
}

/// Everything worth a line in `days` days after `from_ms`, in order. Eclipses
/// are those this observer can see; with none, only the heavens' own calendar.
pub fn upcoming(from_ms: f64, observer: Option<&Observer>, days: f64) -> Vec<Event> {
    let to_ms = from_ms + days * DAY_MS;
    let mut out = Vec::new();
    let y0 = year_of(from_ms) - 1;
    for year in y0..=year_of(to_ms) + 1 {
        for (s, t) in seasons_in_year(year) {
            out.push(Event {
                kind: EventKind::Season(s),
                start_ms: t,
                peak_ms: t,
                end_ms: t,
            });
        }
        for (i, sh) in SHOWERS.iter().enumerate() {
            let t = sh.peak_ms(year);
            out.push(Event {
                kind: EventKind::Shower(i),
                start_ms: t - 12.0 * 3_600_000.0,
                peak_ms: t,
                end_ms: t + 12.0 * 3_600_000.0,
            });
        }
    }
    for (n, _) in UNIX_MOMENTS {
        let t = n as f64 * 1000.0;
        out.push(Event {
            kind: EventKind::Unix(n),
            start_ms: t,
            peak_ms: t,
            end_ms: t,
        });
    }
    if let Some(o) = observer {
        let mut cursor = from_ms;
        while let Some(e) = next_notable_solar_eclipse(cursor, o, days) {
            if e.start_ms > to_ms {
                break;
            }
            cursor = e.end_ms + DAY_MS;
            out.push(Event {
                start_ms: e.start_ms,
                peak_ms: e.max_ms,
                end_ms: e.end_ms,
                kind: EventKind::Solar(e),
            });
        }
        let mut cursor = from_ms;
        while let Some(e) = next_notable_lunar_eclipse(cursor, o, days) {
            if e.penumbral_start_ms > to_ms {
                break;
            }
            cursor = e.penumbral_end_ms + DAY_MS;
            out.push(Event {
                start_ms: e.penumbral_start_ms,
                peak_ms: e.max_ms,
                end_ms: e.penumbral_end_ms,
                kind: EventKind::Lunar(e),
            });
        }
        out.extend(pairs(from_ms, to_ms.min(from_ms + 60.0 * DAY_MS)));
    }
    // Keep what has not finished by now (or finished within a day), inside the window.
    out.retain(|e| e.end_ms >= from_ms - DAY_MS && e.start_ms <= to_ms);
    out.sort_by(|a, b| a.peak_ms.total_cmp(&b.peak_ms));
    out
}

/// The line the prompt may carry: the one event worth saying out loud now.
/// `events` is `upcoming` from some time not long ago; this only chooses.
pub fn prompt_note(events: &[Event], now_ms: f64, tz_min: i32) -> Option<String> {
    let mut best: Option<(f64, &Event)> = None;
    for e in events {
        // How close this event is to deserving a word: lower is sooner.
        let (live, lead) = match &e.kind {
            EventKind::Solar(_) | EventKind::Lunar(_) => (e.start_ms, 3.0 * DAY_MS),
            EventKind::Season(_) => (e.peak_ms, 10.0 * 3_600_000.0),
            EventKind::Shower(_) => (e.peak_ms, 18.0 * 3_600_000.0),
            EventKind::Pair(..) => (e.peak_ms, 14.0 * 3_600_000.0),
            EventKind::Unix(_) => (e.peak_ms, 3_600_000.0),
        };
        let in_progress = now_ms >= e.start_ms && now_ms <= e.end_ms.max(e.start_ms + 60_000.0);
        let imminent = now_ms < live && live - now_ms <= lead;
        let just_passed = matches!(e.kind, EventKind::Season(_) | EventKind::Unix(_))
            && now_ms > e.peak_ms
            && now_ms - e.peak_ms < 90_000.0 * 2.0;
        if !(in_progress || imminent || just_passed) {
            continue;
        }
        // Eclipses outrank everything, then the rest by nearness.
        let rank = match &e.kind {
            EventKind::Solar(_) | EventKind::Lunar(_) => -1e15,
            _ => 0.0,
        } + (e.peak_ms - now_ms).abs();
        if best.is_none_or(|(r, _)| rank < r) {
            best = Some((rank, e));
        }
    }
    best.map(|(_, e)| e.note(now_ms, tz_min))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc(y: i64, mo: u32, d: u32, h: u32, mi: u32) -> f64 {
        days_from_civil(y, mo, d) as f64 * DAY_MS
            + f64::from(h) * 3_600_000.0
            + f64::from(mi) * 60_000.0
    }

    #[test]
    fn civil_dates_round_trip() {
        for days in [-1000_i64, 0, 11_016, 20_000, 40_000] {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, m, d), days);
        }
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(date(utc(2026, 8, 12, 12, 0), 0), "Wed 12 Aug");
        assert_eq!(clock(utc(2026, 8, 12, 17, 46), 120), "19:46");
    }

    #[test]
    fn new_and_full_moons_match_the_almanac() {
        // (UT, to within ~3 minutes)
        let new = [
            utc(2024, 4, 8, 18, 21),
            utc(2025, 1, 29, 12, 36),
            utc(2026, 2, 17, 12, 1),
            utc(2026, 8, 12, 17, 37),
        ];
        for n in new {
            let found = next_phase(n - 5.0 * DAY_MS, Phase::New);
            assert!(
                (found - n).abs() < 3.0 * 60_000.0,
                "new moon off by {} min",
                (found - n) / 60_000.0
            );
        }
        let full = [
            utc(2024, 3, 25, 7, 0),
            utc(2025, 3, 14, 6, 55),
            utc(2025, 9, 7, 18, 9),
            utc(2026, 3, 3, 11, 38),
        ];
        for f in full {
            let found = next_phase(f - 5.0 * DAY_MS, Phase::Full);
            assert!(
                (found - f).abs() < 3.0 * 60_000.0,
                "full moon off by {} min",
                (found - f) / 60_000.0
            );
        }
    }

    #[test]
    fn solstices_and_equinoxes_match_the_almanac() {
        let truth = [
            (2024, Season::MarchEquinox, utc(2024, 3, 20, 3, 6)),
            (2024, Season::JuneSolstice, utc(2024, 6, 20, 20, 51)),
            (2024, Season::SeptemberEquinox, utc(2024, 9, 22, 12, 44)),
            (2024, Season::DecemberSolstice, utc(2024, 12, 21, 9, 21)),
            (2025, Season::JuneSolstice, utc(2025, 6, 21, 2, 42)),
            (2025, Season::SeptemberEquinox, utc(2025, 9, 22, 18, 19)),
            (2025, Season::DecemberSolstice, utc(2025, 12, 21, 15, 3)),
            (2026, Season::MarchEquinox, utc(2026, 3, 20, 14, 46)),
            (2026, Season::JuneSolstice, utc(2026, 6, 21, 8, 24)),
        ];
        for (y, s, t) in truth {
            let got = season_instant(y, s);
            println!("{y} {}: {:+.2} min", s.name(), (got - t) / 60_000.0);
            assert!(
                (got - t).abs() < 1.5 * 60_000.0,
                "{y} {}: {} min off",
                s.name(),
                (got - t) / 60_000.0
            );
        }
    }

    #[test]
    fn showers_peak_on_their_nights() {
        let per = SHOWERS.iter().position(|s| s.name == "Perseids").unwrap();
        let t = SHOWERS[per].peak_ms(2025);
        assert!(
            (t - utc(2025, 8, 12, 17, 0)).abs() < 12.0 * 3_600_000.0,
            "{}",
            date(t, 0)
        );
        let gem = SHOWERS.iter().position(|s| s.name == "Geminids").unwrap();
        let t = SHOWERS[gem].peak_ms(2025);
        assert!(
            (t - utc(2025, 12, 14, 12, 0)).abs() < 24.0 * 3_600_000.0,
            "{}",
            date(t, 0)
        );
        let quad = SHOWERS
            .iter()
            .position(|s| s.name == "Quadrantids")
            .unwrap();
        let t = SHOWERS[quad].peak_ms(2026);
        assert!(
            (t - utc(2026, 1, 3, 12, 0)).abs() < 24.0 * 3_600_000.0,
            "{}",
            date(t, 0)
        );
    }

    #[test]
    fn the_sky_has_company() {
        let ev = upcoming(
            utc(2026, 7, 1, 0, 0),
            Some(&Observer::new(41.4, -2.2)),
            60.0,
        );
        assert!(
            ev.iter().any(|e| matches!(
                e.kind,
                EventKind::Solar(SolarEclipse {
                    kind: SolarKind::Total,
                    ..
                })
            )),
            "no eclipse in {ev:#?}"
        );
        assert!(ev.iter().any(|e| matches!(e.kind, EventKind::Shower(_))));
        let note = prompt_note(&ev, utc(2026, 8, 12, 18, 20), 120);
        assert!(
            note.as_deref()
                .is_some_and(|n| n.contains("totality") || n.contains("eclipse")),
            "{note:?}"
        );
    }
}
