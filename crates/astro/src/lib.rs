//! A small, honest almanac. Everything here is computed on the device from
//! published low-order series — no network, no data files:
//!
//! * the Moon from the truncated ELP-2000/82 series (J. Meeus, *Astronomical
//!   Algorithms*, ch. 47), good to about 10″ in longitude;
//! * the Sun from Meeus ch. 25 with its planetary terms, good to a few ″;
//! * the planets from the JPL "Keplerian Elements for Approximate Positions of
//!   the Major Planets" (E. M. Standish), good to a few arcminutes — a naked-eye
//!   sky, not a navigation table;
//! * eclipses found by geometry — the angular separation of the topocentric Sun
//!   and Moon against their radii — rather than from a catalogue.
//!
//! Angles are radians unless a name says `_deg`. Times are Unix milliseconds
//! in UTC, the clock nus already runs on; Δ T is applied inside.

mod almanac;
mod eclipse;
mod ephem;
mod events;
mod frame;
mod tz;
mod vsop;

pub use almanac::{
    moon_phase_name, moon_report, rise_set, sky_report, tonight_report, Report, RiseSet,
};
pub use eclipse::{
    lunar_eclipse_at, next_lunar_eclipse, next_notable_lunar_eclipse, next_notable_solar_eclipse,
    next_solar_eclipse, solar_eclipse_at, LunarEclipse, LunarKind, LunarState, SolarEclipse,
    SolarKind, SolarState,
};
pub use ephem::{
    horizontal, jd_from_unix_ms, moon_phase, planet, rotation_equatorial_to_horizon, sidereal_time,
    sky_at, Body, Planet, PlanetPos, Sky, TopoBody,
};
pub use events::{
    clock as format_clock, date as format_date, date_year as format_date_year, next_phase,
    prompt_note, season_instant, seasons_in_year, span as format_span, upcoming, Event, EventKind,
    Phase, Season, Shower, SHOWERS, UNIX_MOMENTS,
};
pub use frame::{sky_frame, PlanetMark, SkyFrame};
pub use tz::local_offset_minutes;

pub(crate) const D2R: f64 = std::f64::consts::PI / 180.0;
pub(crate) const TAU: f64 = std::f64::consts::TAU;
/// One day in milliseconds.
pub const DAY_MS: f64 = 86_400_000.0;

/// Where on Earth. Longitude is east-positive, in degrees.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Observer {
    pub lat_deg: f64,
    pub lon_deg: f64,
    pub height_m: f64,
}

impl Observer {
    pub fn new(lat_deg: f64, lon_deg: f64) -> Self {
        Self {
            lat_deg: lat_deg.clamp(-90.0, 90.0),
            lon_deg,
            height_m: 0.0,
        }
    }
}

/// East/Up/North unit vector for an altitude and an azimuth (north through east).
pub fn direction(alt: f64, az: f64) -> [f64; 3] {
    [alt.cos() * az.sin(), alt.sin(), alt.cos() * az.cos()]
}

/// A radian angle wrapped to 0..2π.
pub(crate) fn wrap(a: f64) -> f64 {
    a.rem_euclid(TAU)
}

/// A radian angle wrapped to −π..π.
pub(crate) fn wrap_pm(a: f64) -> f64 {
    let a = wrap(a);
    if a > std::f64::consts::PI {
        a - TAU
    } else {
        a
    }
}
