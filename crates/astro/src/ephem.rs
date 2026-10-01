//! Positions: time scales, the Sun, the Moon, the planets, and the turning
//! of the Earth that carries them across a local horizon.

use crate::vsop;
use crate::{wrap, wrap_pm, Observer, D2R, DAY_MS, TAU};

const EARTH_EQUATORIAL_RADIUS_KM: f64 = 6378.14;
const AU_KM: f64 = 149_597_870.7;
const SUN_RADIUS_KM: f64 = 695_700.0;
const MOON_RADIUS_KM: f64 = 1737.4;
const EARTH_FLATTENING: f64 = 1.0 / 298.257;

/// Julian day (UT) of a Unix time in milliseconds.
pub fn jd_from_unix_ms(ms: f64) -> f64 {
    ms / DAY_MS + 2_440_587.5
}

/// TT − UT in seconds. Observed to 2025, then a gentle extrapolation; the
/// Moon moves half an arcsecond a second, so a few seconds here are invisible.
pub(crate) fn delta_t(jd_ut: f64) -> f64 {
    let y = 2000.0 + (jd_ut - 2_451_544.5) / 365.2425;
    let poly = |t: f64, c: &[f64]| c.iter().rev().fold(0.0, |a, k| a * t + k);
    if y < 1900.0 {
        let u = (y - 1820.0) / 100.0;
        -20.0 + 32.0 * u * u
    } else if y < 1920.0 {
        poly(
            y - 1900.0,
            &[-2.79, 1.494119, -0.0598939, 0.0061966, -0.000197],
        )
    } else if y < 1941.0 {
        poly(y - 1920.0, &[21.20, 0.84493, -0.076100, 0.0020936])
    } else if y < 1961.0 {
        let t = y - 1950.0;
        29.07 + 0.407 * t - t * t / 233.0 + t * t * t / 2547.0
    } else if y < 1986.0 {
        let t = y - 1975.0;
        45.45 + 1.067 * t - t * t / 260.0 - t * t * t / 718.0
    } else if y < 2005.0 {
        poly(
            y - 2000.0,
            &[
                63.86,
                0.3345,
                -0.060374,
                0.0017275,
                0.000651814,
                0.00002373599,
            ],
        )
    } else if y < 2025.0 {
        // Observed (IERS): 2005 64.7 · 2010 66.1 · 2015 67.6 · 2020 69.4 · 2025 69.1.
        const OBSERVED: [(f64, f64); 5] = [
            (2005.0, 64.7),
            (2010.0, 66.1),
            (2015.0, 67.6),
            (2020.0, 69.4),
            (2025.0, 69.1),
        ];
        let i = (((y - 2005.0) / 5.0) as usize).min(3);
        let (a, b) = (OBSERVED[i], OBSERVED[i + 1]);
        a.1 + (b.1 - a.1) * (y - a.0) / (b.0 - a.0)
    } else if y < 2050.0 {
        69.1 + (93.0 - 69.1) * (y - 2025.0) / 25.0
    } else if y < 2150.0 {
        let u = (y - 1820.0) / 100.0;
        -20.0 + 32.0 * u * u - 0.5628 * (2150.0 - y)
    } else {
        let u = (y - 1820.0) / 100.0;
        -20.0 + 32.0 * u * u
    }
}

/// Dynamical-time Julian day of a Unix time.
pub(crate) fn jde_from_unix_ms(ms: f64) -> f64 {
    let jd = jd_from_unix_ms(ms);
    jd + delta_t(jd) / 86_400.0
}

fn centuries(jde: f64) -> f64 {
    (jde - 2_451_545.0) / 36_525.0
}

/// Mean obliquity of the ecliptic (Meeus 22.2), radians.
fn mean_obliquity(t: f64) -> f64 {
    (23.0 + 26.0 / 60.0 + 21.448 / 3600.0
        - (46.8150 * t + 0.00059 * t * t - 0.001813 * t * t * t) / 3600.0)
        * D2R
}

/// Nutation in longitude and obliquity (Meeus 22, the short series), radians.
fn nutation(t: f64) -> (f64, f64) {
    let omega = (125.04452 - 1934.136261 * t) * D2R;
    let l = (280.4665 + 36000.7698 * t) * D2R;
    let lm = (218.3165 + 481267.8813 * t) * D2R;
    let arc = D2R / 3600.0;
    let dpsi = -17.20 * omega.sin() - 1.32 * (2.0 * l).sin() - 0.23 * (2.0 * lm).sin()
        + 0.21 * (2.0 * omega).sin();
    let deps = 9.20 * omega.cos() + 0.57 * (2.0 * l).cos() + 0.10 * (2.0 * lm).cos()
        - 0.09 * (2.0 * omega).cos();
    (dpsi * arc, deps * arc)
}

fn ecliptic_to_equatorial(lon: f64, lat: f64, eps: f64) -> (f64, f64) {
    let ra = (lon.sin() * eps.cos() - lat.tan() * eps.sin()).atan2(lon.cos());
    let dec = (lat.sin() * eps.cos() + lat.cos() * eps.sin() * lon.sin())
        .clamp(-1.0, 1.0)
        .asin();
    (wrap(ra), dec)
}

/// A body as seen from the Earth's centre: right ascension and declination of
/// date (apparent), and distance in kilometres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Body {
    pub ra: f64,
    pub dec: f64,
    pub dist_km: f64,
    /// Apparent ecliptic longitude of date.
    pub lon: f64,
    pub lat: f64,
}

impl Body {
    /// Unit vector in the equatorial frame of date.
    pub fn unit(&self) -> [f64; 3] {
        [
            self.dec.cos() * self.ra.cos(),
            self.dec.cos() * self.ra.sin(),
            self.dec.sin(),
        ]
    }
}

/// The Sun's apparent geocentric place: the Earth's VSOP87D position turned
/// about, corrected to the FK5 frame, with nutation and aberration.
pub(crate) fn sun_geocentric(jde: f64) -> Body {
    let t = centuries(jde);
    let (l, b, r_au) = vsop::earth(t);
    let arc = D2R / 3600.0;
    let mut lon = wrap(l + std::f64::consts::PI);
    let mut lat = -b;
    let lp = lon - (1.397 * t + 0.00031 * t * t) * D2R;
    lon -= 0.09033 * arc;
    lat += 0.03916 * arc * (lp.cos() - lp.sin());
    let (dpsi, deps) = nutation(t);
    lon = wrap(lon + dpsi - 20.4898 * arc / r_au);
    let eps = mean_obliquity(t) + deps;
    let (ra, dec) = ecliptic_to_equatorial(lon, lat, eps);
    Body {
        ra,
        dec,
        dist_km: r_au * AU_KM,
        lon,
        lat,
    }
}

// Meeus table 47.A: multiples of D, M, M', F; longitude sum (1e-6°), distance sum (1e-3 km).
#[rustfmt::skip]
const MOON_LR: [(i8, i8, i8, i8, i32, i32); 60] = [
    (0,0,1,0, 6288774,-20905355),(2,0,-1,0, 1274027,-3699111),(2,0,0,0, 658314,-2955968),(0,0,2,0, 213618,-569925),
    (0,1,0,0, -185116,48888),(0,0,0,2, -114332,-3149),(2,0,-2,0, 58793,246158),(2,-1,-1,0, 57066,-152138),
    (2,0,1,0, 53322,-170733),(2,-1,0,0, 45758,-204586),(0,1,-1,0, -40923,-129620),(1,0,0,0, -34720,108743),
    (0,1,1,0, -30383,104755),(2,0,0,-2, 15327,10321),(0,0,1,2, -12528,0),(0,0,1,-2, 10980,79661),
    (4,0,-1,0, 10675,-34782),(0,0,3,0, 10034,-23210),(4,0,-2,0, 8548,-21636),(2,1,-1,0, -7888,24208),
    (2,1,0,0, -6766,30824),(1,0,-1,0, -5163,-8379),(1,1,0,0, 4987,-16675),(2,-1,1,0, 4036,-12831),
    (2,0,2,0, 3994,-10445),(4,0,0,0, 3861,-11650),(2,0,-3,0, 3665,14403),(0,1,-2,0, -2689,-7003),
    (2,0,-1,2, -2602,0),(2,-1,-2,0, 2390,10056),(1,0,1,0, -2348,6322),(2,-2,0,0, 2236,-9884),
    (0,1,2,0, -2120,5751),(0,2,0,0, -2069,0),(2,-2,-1,0, 2048,-4950),(2,0,1,-2, -1773,4130),
    (2,0,0,2, -1595,0),(4,-1,-1,0, 1215,-3958),(0,0,2,2, -1110,0),(3,0,-1,0, -892,3258),
    (2,1,1,0, -810,2616),(4,-1,-2,0, 759,-1897),(0,2,-1,0, -713,-2117),(2,2,-1,0, -700,2354),
    (2,1,-2,0, 691,0),(2,-1,0,-2, 596,0),(4,0,1,0, 549,-1423),(0,0,4,0, 537,-1117),
    (4,-1,0,0, 520,-1571),(1,0,-2,0, -487,-1739),(2,1,0,-2, -399,0),(0,0,2,-2, -381,-4421),
    (1,1,1,0, 351,0),(3,0,-2,0, -340,0),(4,0,-3,0, 330,0),(2,-1,2,0, 327,0),
    (0,2,1,0, -323,1165),(1,1,-1,0, 299,0),(2,0,3,0, 294,0),(2,0,-1,-2, 0,8752),
];
// Table 47.B: latitude sum (1e-6°).
#[rustfmt::skip]
const MOON_B: [(i8, i8, i8, i8, i32); 60] = [
    (0,0,0,1, 5128122),(0,0,1,1, 280602),(0,0,1,-1, 277693),(2,0,0,-1, 173237),
    (2,0,-1,1, 55413),(2,0,-1,-1, 46271),(2,0,0,1, 32573),(0,0,2,1, 17198),
    (2,0,1,-1, 9266),(0,0,2,-1, 8822),(2,-1,0,-1, 8216),(2,0,-2,-1, 4324),
    (2,0,1,1, 4200),(2,1,0,-1, -3359),(2,-1,-1,1, 2463),(2,-1,0,1, 2211),
    (2,-1,-1,-1, 2065),(0,1,-1,-1, -1870),(4,0,-1,-1, 1828),(0,1,0,1, -1794),
    (0,0,0,3, -1749),(0,1,-1,1, -1565),(1,0,0,1, -1491),(0,1,1,1, -1475),
    (0,1,1,-1, -1410),(0,1,0,-1, -1344),(1,0,0,-1, -1335),(0,0,3,1, 1107),
    (4,0,0,-1, 1021),(4,0,-1,1, 833),(0,0,1,-3, 777),(4,0,-2,1, 671),
    (2,0,0,-3, 607),(2,0,2,-1, 596),(2,-1,1,-1, 491),(2,0,-2,1, -451),
    (0,0,3,-1, 439),(2,0,2,1, 422),(2,0,-3,-1, 421),(2,1,-1,1, -366),
    (2,1,0,1, -351),(4,0,0,1, 331),(2,-1,1,1, 315),(2,-2,0,-1, 302),
    (0,0,1,3, -283),(2,1,1,-1, -229),(1,1,0,-1, 223),(1,1,0,1, 223),
    (0,1,-2,-1, -220),(2,1,-1,-1, -220),(1,0,1,1, -185),(2,-1,-2,-1, 181),
    (0,1,2,1, -177),(4,0,-2,-1, 176),(4,-1,-1,-1, 166),(1,0,1,-1, -164),
    (4,0,1,-1, 132),(1,0,-1,-1, -119),(4,-1,0,-1, 115),(2,-2,0,1, 107),
];

/// The Moon's apparent geocentric place (Meeus ch. 47, nutation applied).
pub(crate) fn moon_geocentric(jde: f64) -> Body {
    let t = centuries(jde);
    let poly = |c: [f64; 5]| (c[0] + t * (c[1] + t * (c[2] + t * (c[3] + t * c[4])))) * D2R;
    let lp = poly([
        218.3164477,
        481267.88123421,
        -0.0015786,
        1.0 / 538841.0,
        -1.0 / 65194000.0,
    ]);
    let d = poly([
        297.8501921,
        445267.1114034,
        -0.0018819,
        1.0 / 545868.0,
        -1.0 / 113065000.0,
    ]);
    let m = poly([
        357.5291092,
        35999.0502909,
        -0.0001536,
        1.0 / 24490000.0,
        0.0,
    ]);
    let mp = poly([
        134.9633964,
        477198.8675055,
        0.0087414,
        1.0 / 69699.0,
        -1.0 / 14712000.0,
    ]);
    let f = poly([
        93.2720950,
        483202.0175233,
        -0.0036539,
        -1.0 / 3526000.0,
        1.0 / 863310000.0,
    ]);
    let a1 = (119.75 + 131.849 * t) * D2R;
    let a2 = (53.09 + 479264.290 * t) * D2R;
    let a3 = (313.45 + 481266.484 * t) * D2R;
    let e = 1.0 - 0.002516 * t - 0.0000074 * t * t;
    let eterm = |m_mult: i8| match m_mult.abs() {
        1 => e,
        2 => e * e,
        _ => 1.0,
    };
    let (mut sl, mut sr, mut sb) = (0.0, 0.0, 0.0);
    for &(cd, cm, cmp, cf, l, r) in &MOON_LR {
        let arg = f64::from(cd) * d + f64::from(cm) * m + f64::from(cmp) * mp + f64::from(cf) * f;
        let k = eterm(cm);
        sl += f64::from(l) * k * arg.sin();
        sr += f64::from(r) * k * arg.cos();
    }
    for &(cd, cm, cmp, cf, b) in &MOON_B {
        let arg = f64::from(cd) * d + f64::from(cm) * m + f64::from(cmp) * mp + f64::from(cf) * f;
        sb += f64::from(b) * eterm(cm) * arg.sin();
    }
    sl += 3958.0 * a1.sin() + 1962.0 * (lp - f).sin() + 318.0 * a2.sin();
    sb += -2235.0 * lp.sin()
        + 382.0 * a3.sin()
        + 175.0 * (a1 - f).sin()
        + 175.0 * (a1 + f).sin()
        + 127.0 * (lp - mp).sin()
        - 115.0 * (lp + mp).sin();
    let lon = lp + sl * 1e-6 * D2R;
    let lat = sb * 1e-6 * D2R;
    let dist = 385_000.56 + sr * 1e-3;
    let (dpsi, deps) = nutation(t);
    let eps = mean_obliquity(t) + deps;
    let lon_app = wrap(lon + dpsi);
    let (ra, dec) = ecliptic_to_equatorial(lon_app, lat, eps);
    Body {
        ra,
        dec,
        dist_km: dist,
        lon: lon_app,
        lat,
    }
}

/// Mean sidereal time at Greenwich, radians, from a UT Julian day (Meeus 12.4).
fn gmst(jd_ut: f64) -> f64 {
    let t = (jd_ut - 2_451_545.0) / 36_525.0;
    wrap(
        (280.46061837 + 360.98564736629 * (jd_ut - 2_451_545.0) + 0.000387933 * t * t
            - t * t * t / 38_710_000.0)
            * D2R,
    )
}

/// Local apparent sidereal time in radians.
pub fn sidereal_time(ms: f64, lon_deg: f64) -> f64 {
    let jd = jd_from_unix_ms(ms);
    let t = centuries(jd + delta_t(jd) / 86_400.0);
    let (dpsi, deps) = nutation(t);
    wrap(gmst(jd) + dpsi * (mean_obliquity(t) + deps).cos() + lon_deg * D2R)
}

/// Where a body is for an observer: altitude, azimuth (north through east),
/// the East/Up/North unit vector, and the topocentric distance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TopoBody {
    pub alt: f64,
    pub az: f64,
    pub ra: f64,
    pub dec: f64,
    pub dist_km: f64,
    pub dir: [f64; 3],
}

fn observer_vector(o: &Observer, lst: f64) -> [f64; 3] {
    let phi = o.lat_deg * D2R;
    let u = ((1.0 - EARTH_FLATTENING) * phi.tan()).atan();
    let h = o.height_m / EARTH_EQUATORIAL_RADIUS_KM / 1000.0;
    let rho_sin = (1.0 - EARTH_FLATTENING) * u.sin() + h * phi.sin();
    let rho_cos = u.cos() + h * phi.cos();
    [
        EARTH_EQUATORIAL_RADIUS_KM * rho_cos * lst.cos(),
        EARTH_EQUATORIAL_RADIUS_KM * rho_cos * lst.sin(),
        EARTH_EQUATORIAL_RADIUS_KM * rho_sin,
    ]
}

/// East/Up/North components of an equatorial-of-date unit vector.
fn to_horizon(v: [f64; 3], lst: f64, lat: f64) -> [f64; 3] {
    let (sl, cl) = lst.sin_cos();
    let (xl, yl, zl) = (v[0] * cl + v[1] * sl, -v[0] * sl + v[1] * cl, v[2]);
    let (sp, cp) = lat.sin_cos();
    [yl, xl * cp + zl * sp, -xl * sp + zl * cp]
}

/// Topocentric place of a geocentric body: parallax is exact (vector
/// subtraction), refraction is not applied.
pub(crate) fn topocentric(body: &Body, o: &Observer, lst: f64) -> TopoBody {
    let g = body.unit().map(|c| c * body.dist_km);
    let ov = observer_vector(o, lst);
    let t = [g[0] - ov[0], g[1] - ov[1], g[2] - ov[2]];
    let dist = (t[0] * t[0] + t[1] * t[1] + t[2] * t[2]).sqrt();
    let u = t.map(|c| c / dist);
    let dir = to_horizon(u, lst, o.lat_deg * D2R);
    TopoBody {
        alt: dir[1].clamp(-1.0, 1.0).asin(),
        az: wrap(dir[0].atan2(dir[2])),
        ra: wrap(u[1].atan2(u[0])),
        dec: u[2].clamp(-1.0, 1.0).asin(),
        dist_km: dist,
        dir,
    }
}

/// East/Up/North of any equatorial-of-date direction (stars, planets).
pub fn horizontal(ra: f64, dec: f64, ms: f64, o: &Observer) -> [f64; 3] {
    let u = [dec.cos() * ra.cos(), dec.cos() * ra.sin(), dec.sin()];
    to_horizon(u, sidereal_time(ms, o.lon_deg), o.lat_deg * D2R)
}

// ── precession ──────────────────────────────────────────────────────────

type Mat = [[f64; 3]; 3];

fn mul(a: &Mat, b: &Mat) -> Mat {
    let mut r = [[0.0; 3]; 3];
    for (i, row) in r.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            *cell = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    r
}

fn rz(a: f64) -> Mat {
    let (s, c) = a.sin_cos();
    [[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]]
}

/// Mean precession from J2000 to the date of `t` centuries (IAU 1976).
fn precession(t: f64) -> Mat {
    let arc = D2R / 3600.0;
    let zeta = (2306.2181 * t + 0.30188 * t * t + 0.017998 * t * t * t) * arc;
    let z = (2306.2181 * t + 1.09468 * t * t + 0.018203 * t * t * t) * arc;
    let theta = (2004.3109 * t - 0.42665 * t * t - 0.041833 * t * t * t) * arc;
    let (st, ct) = theta.sin_cos();
    let tilt = [[ct, 0.0, -st], [0.0, 1.0, 0.0], [st, 0.0, ct]];
    mul(&rz(z), &mul(&tilt, &rz(zeta)))
}

/// One matrix taking a J2000 equatorial unit vector (a Hipparcos star) to
/// East/Up/North at this time and place: precession, then the turning Earth.
pub fn rotation_equatorial_to_horizon(ms: f64, o: &Observer) -> [[f64; 3]; 3] {
    let jde = jde_from_unix_ms(ms);
    let p = precession(centuries(jde));
    let lst = sidereal_time(ms, o.lon_deg);
    let (sl, cl) = lst.sin_cos();
    let (sp, cp) = (o.lat_deg * D2R).sin_cos();
    // Rows are East, Up, North in the equatorial frame of date.
    let to_h: Mat = [
        [-sl, cl, 0.0],
        [cl * cp, sl * cp, sp],
        [-cl * sp, -sl * sp, cp],
    ];
    mul(&to_h, &p)
}

// ── planets ─────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Planet {
    Mercury,
    Venus,
    Mars,
    Jupiter,
    Saturn,
    Uranus,
    Neptune,
}

impl Planet {
    pub const ALL: [Planet; 7] = [
        Planet::Mercury,
        Planet::Venus,
        Planet::Mars,
        Planet::Jupiter,
        Planet::Saturn,
        Planet::Uranus,
        Planet::Neptune,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Planet::Mercury => "Mercury",
            Planet::Venus => "Venus",
            Planet::Mars => "Mars",
            Planet::Jupiter => "Jupiter",
            Planet::Saturn => "Saturn",
            Planet::Uranus => "Uranus",
            Planet::Neptune => "Neptune",
        }
    }
    /// Within reach of the naked eye on a dark night.
    pub fn naked_eye(self) -> bool {
        !matches!(self, Planet::Uranus | Planet::Neptune)
    }
}

// a, e, I, L, long.peri, long.node at J2000 and per century (Standish, 1800–2050):
// [a, da, e, de, I, dI, L, dL, w, dw, node, dnode]
#[rustfmt::skip]
const ELEMENTS: [(Option<Planet>, [f64; 12]); 8] = [
    (Some(Planet::Mercury), [0.38709927, 0.00000037, 0.20563593, 0.00001906, 7.00497902, -0.00594749, 252.25032350, 149472.67411175, 77.45779628, 0.16047689, 48.33076593, -0.12534081]),
    (Some(Planet::Venus),   [0.72333566, 0.00000390, 0.00677672, -0.00004107, 3.39467605, -0.00078890, 181.97909950, 58517.81538729, 131.60246718, 0.00268329, 76.67984255, -0.27769418]),
    (None,                  [1.00000261, 0.00000562, 0.01671123, -0.00004392, -0.00001531, -0.01294668, 100.46457166, 35999.37244981, 102.93768193, 0.32327364, 0.0, 0.0]),
    (Some(Planet::Mars),    [1.52371034, 0.00001847, 0.09339410, 0.00007882, 1.84969142, -0.00813131, -4.55343205, 19140.30268499, -23.94362959, 0.44441088, 49.55953891, -0.29257343]),
    (Some(Planet::Jupiter), [5.20288700, -0.00011607, 0.04838624, -0.00013253, 1.30439695, -0.00183714, 34.39644051, 3034.74612775, 14.72847983, 0.21252668, 100.47390909, 0.20469106]),
    (Some(Planet::Saturn),  [9.53667594, -0.00125060, 0.05386179, -0.00050991, 2.48599187, 0.00193609, 49.95424423, 1222.49362201, 92.59887831, -0.41897216, 113.66242448, -0.28867794]),
    (Some(Planet::Uranus),  [19.18916464, -0.00196176, 0.04725744, -0.00004397, 0.77263783, -0.00242939, 313.23810451, 428.48202785, 170.95427630, 0.40805281, 74.01692503, 0.04240589]),
    (Some(Planet::Neptune), [30.06992276, 0.00026291, 0.00859048, 0.00005105, 1.77004347, 0.00035372, -55.12002969, 218.45945325, 44.96476227, -0.32241464, 131.78422574, -0.00508664]),
];

/// Heliocentric rectangular ecliptic coordinates (J2000), AU.
fn heliocentric(el: &[f64; 12], t: f64) -> [f64; 3] {
    let a = el[0] + el[1] * t;
    let e = el[2] + el[3] * t;
    let inc = (el[4] + el[5] * t) * D2R;
    let l = el[6] + el[7] * t;
    let w_bar = el[8] + el[9] * t;
    let node = (el[10] + el[11] * t) * D2R;
    let omega = w_bar * D2R - node;
    let m = wrap_pm((l - w_bar) * D2R);
    let mut ecc = m + e * m.sin();
    for _ in 0..12 {
        ecc -= (ecc - e * ecc.sin() - m) / (1.0 - e * ecc.cos());
    }
    let xv = a * (ecc.cos() - e);
    let yv = a * (1.0 - e * e).sqrt() * ecc.sin();
    let (so, co) = omega.sin_cos();
    let (sn, cn) = node.sin_cos();
    let (si, ci) = inc.sin_cos();
    [
        (co * cn - so * sn * ci) * xv + (-so * cn - co * sn * ci) * yv,
        (co * sn + so * cn * ci) * xv + (-so * sn + co * cn * ci) * yv,
        (so * si) * xv + (co * si) * yv,
    ]
}

/// A planet as seen from the Earth's centre, plus what the sky says about it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlanetPos {
    pub planet: Planet,
    /// Equatorial of date, geocentric, light-time corrected.
    pub ra: f64,
    pub dec: f64,
    pub dist_au: f64,
    pub sun_dist_au: f64,
    /// Angle from the Sun as seen from the Earth, radians.
    pub elongation: f64,
    /// Sun–planet–Earth angle, radians.
    pub phase_angle: f64,
    pub magnitude: f64,
}

/// A planet's place at a Unix time.
pub fn planet(p: Planet, ms: f64) -> PlanetPos {
    let jde = jde_from_unix_ms(ms);
    let t = centuries(jde);
    let pl = ELEMENTS
        .iter()
        .find(|(q, _)| *q == Some(p))
        .map(|(_, e)| e)
        .expect("elements");
    let earth_el = &ELEMENTS[2].1;
    let earth = heliocentric(earth_el, t);
    // Light time: where the planet was when the light left it.
    let mut tau = 0.0;
    let mut pos = heliocentric(pl, t);
    for _ in 0..3 {
        pos = heliocentric(pl, t - tau / 36_525.0);
        let d = [pos[0] - earth[0], pos[1] - earth[1], pos[2] - earth[2]];
        tau = 0.0057755183 * (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    }
    let g = [pos[0] - earth[0], pos[1] - earth[1], pos[2] - earth[2]];
    let delta = (g[0] * g[0] + g[1] * g[1] + g[2] * g[2]).sqrt();
    // Ecliptic J2000 → equatorial J2000 → equatorial of date.
    let eps0 = 23.43929111 * D2R;
    let eq = [
        g[0],
        g[1] * eps0.cos() - g[2] * eps0.sin(),
        g[1] * eps0.sin() + g[2] * eps0.cos(),
    ];
    let pm = precession(t);
    let v: [f64; 3] =
        std::array::from_fn(|i| pm[i][0] * eq[0] + pm[i][1] * eq[1] + pm[i][2] * eq[2]);
    let ra = wrap(v[1].atan2(v[0]));
    let dec = (v[2] / delta).clamp(-1.0, 1.0).asin();
    let r = (pos[0] * pos[0] + pos[1] * pos[1] + pos[2] * pos[2]).sqrt();
    let big_r = (earth[0] * earth[0] + earth[1] * earth[1] + earth[2] * earth[2]).sqrt();
    let cos_i = ((r * r + delta * delta - big_r * big_r) / (2.0 * r * delta)).clamp(-1.0, 1.0);
    let i_deg = cos_i.acos() / D2R;
    let cos_el = ((delta * delta + big_r * big_r - r * r) / (2.0 * delta * big_r)).clamp(-1.0, 1.0);
    let log = 5.0 * (r * delta).log10();
    let magnitude = match p {
        Planet::Mercury => {
            -0.42 + log + 0.0380 * i_deg - 0.000273 * i_deg * i_deg + 0.000002 * i_deg.powi(3)
        }
        Planet::Venus => {
            -4.40 + log + 0.0009 * i_deg + 0.000239 * i_deg * i_deg - 0.00000065 * i_deg.powi(3)
        }
        Planet::Mars => -1.52 + log + 0.016 * i_deg,
        Planet::Jupiter => -9.40 + log + 0.005 * i_deg,
        Planet::Saturn => {
            // The rings: how open they are to us (north pole of Saturn, J2000).
            let (pra, pdec) = (40.589 * D2R, 83.537 * D2R);
            let pole = [pdec.cos() * pra.cos(), pdec.cos() * pra.sin(), pdec.sin()];
            let sin_b = -(pole[0] * eq[0] + pole[1] * eq[1] + pole[2] * eq[2]) / delta;
            -8.88 + log - 2.60 * sin_b.abs() + 1.25 * sin_b * sin_b
        }
        Planet::Uranus => -7.19 + log,
        Planet::Neptune => -6.87 + log,
    };
    PlanetPos {
        planet: p,
        ra,
        dec,
        dist_au: delta,
        sun_dist_au: r,
        elongation: cos_el.acos(),
        phase_angle: i_deg * D2R,
        magnitude,
    }
}

// ── the whole sky at an instant ─────────────────────────────────────────

/// Sun and Moon at one instant for one observer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sky {
    pub ms: f64,
    pub jde: f64,
    pub sun_geo: Body,
    pub moon_geo: Body,
    pub sun: TopoBody,
    pub moon: TopoBody,
    /// Local apparent sidereal time, radians.
    pub lst: f64,
    /// Lit fraction of the Moon's disc, 0..1.
    pub illumination: f64,
    /// Between new and full.
    pub waxing: bool,
    /// Apparent angular radii, radians (the Moon's is topocentric).
    pub sun_radius: f64,
    pub moon_radius: f64,
}

/// Sun and Moon for an observer at a Unix time in milliseconds.
pub fn sky_at(ms: f64, o: &Observer) -> Sky {
    let jde = jde_from_unix_ms(ms);
    let lst = sidereal_time(ms, o.lon_deg);
    let sun_geo = sun_geocentric(jde);
    let moon_geo = moon_geocentric(jde);
    let sun = topocentric(&sun_geo, o, lst);
    let moon = topocentric(&moon_geo, o, lst);
    let (illumination, waxing) = phase_of(&sun_geo, &moon_geo);
    Sky {
        ms,
        jde,
        sun_geo,
        moon_geo,
        sun,
        moon,
        lst,
        illumination,
        waxing,
        sun_radius: (SUN_RADIUS_KM / sun.dist_km).asin(),
        moon_radius: (MOON_RADIUS_KM / moon.dist_km).asin(),
    }
}

fn phase_of(sun: &Body, moon: &Body) -> (f64, bool) {
    let cos_psi = (moon.dec.sin() * sun.dec.sin()
        + moon.dec.cos() * sun.dec.cos() * (moon.ra - sun.ra).cos())
    .clamp(-1.0, 1.0);
    let psi = cos_psi.acos();
    let i = (sun.dist_km * psi.sin()).atan2(moon.dist_km - sun.dist_km * cos_psi);
    (
        (1.0 + i.cos()) / 2.0,
        wrap(moon.lon - sun.lon) < std::f64::consts::PI,
    )
}

/// Moon phase without an observer: lit fraction, waxing, and age in days since
/// the last new Moon (from the elongation, so it follows the real, uneven month).
pub fn moon_phase(ms: f64) -> (f64, bool, f64) {
    let jde = jde_from_unix_ms(ms);
    let (s, m) = (sun_geocentric(jde), moon_geocentric(jde));
    let (k, waxing) = phase_of(&s, &m);
    let elong = wrap(m.lon - s.lon);
    (k, waxing, elong / TAU * 29.530588)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deg(r: f64) -> f64 {
        r / D2R
    }

    #[test]
    fn moon_matches_meeus_example_47a() {
        // 1992 April 12, 0h TD: λ = 133.162655°, β = −3.229126°, Δ = 368409.7 km.
        let m = moon_geocentric(2_448_724.5);
        let t = centuries(2_448_724.5);
        let (dpsi, _) = nutation(t);
        assert!(
            (deg(m.lon - dpsi) - 133.162655).abs() < 2e-4,
            "lon {}",
            deg(m.lon - dpsi)
        );
        assert!((deg(m.lat) + 3.229126).abs() < 2e-4, "lat {}", deg(m.lat));
        assert!((m.dist_km - 368409.7).abs() < 1.0, "dist {}", m.dist_km);
    }

    #[test]
    fn sun_matches_meeus_example_25a() {
        // 1992 October 13, 0h TD: apparent λ = 199.90895°, α = 198.38083°, δ = −7.78507°, R = 0.99766.
        let s = sun_geocentric(2_448_908.5);
        assert!((deg(s.lon) - 199.90895).abs() < 6e-3, "lon {}", deg(s.lon));
        assert!((deg(s.ra) - 198.38083).abs() < 6e-3, "ra {}", deg(s.ra));
        assert!((deg(s.dec) + 7.78507).abs() < 6e-3, "dec {}", deg(s.dec));
        assert!((s.dist_km / AU_KM - 0.99766).abs() < 2e-4);
    }

    #[test]
    fn mars_opposition_2025() {
        // Mars was at opposition 2025-01-16 (≈ 02:38 UT): ~180° from the Sun, closest to Earth.
        let ms = 1_736_996_280_000.0;
        let m = planet(Planet::Mars, ms);
        let s = sun_geocentric(jde_from_unix_ms(ms));
        let sep = deg(wrap_pm(m.ra - s.ra).abs());
        assert!(sep > 176.0, "separation {sep}");
        assert!(
            m.magnitude < -1.0 && m.magnitude > -1.8,
            "mag {}",
            m.magnitude
        );
        assert!((m.dist_au - 0.64).abs() < 0.02, "dist {}", m.dist_au);
    }

    #[test]
    fn jupiter_and_venus_are_where_the_sky_says() {
        // Jupiter on 2025-01-10 (a month past opposition, retrograde in Taurus): RA ≈ 4h 43m, Dec ≈ +22.3°, magnitude ≈ −2.7.
        let ms = 1_736_500_000_000.0; // 2025-01-10 ≈ 09:46 UT
        let j = planet(Planet::Jupiter, ms);
        assert!(
            (deg(j.ra) / 15.0 - 4.72).abs() < 0.08,
            "ra {}h",
            deg(j.ra) / 15.0
        );
        assert!((deg(j.dec) - 22.0).abs() < 0.8, "dec {}", deg(j.dec));
        assert!((j.magnitude + 2.7).abs() < 0.2, "mag {}", j.magnitude);
        // Venus peaked near −4.9 at inferior conjunction on 2025-03-22, an evening/morning object.
        let v = planet(Planet::Venus, 1_742_600_000_000.0);
        assert!(v.magnitude < -4.0, "mag {}", v.magnitude);
    }

    #[test]
    fn sidereal_time_meeus_example_12a() {
        // 1987 April 10, 0h UT: mean sidereal time at Greenwich 13h10m46.3668s.
        let ms = (2_446_895.5 - 2_440_587.5) * DAY_MS;
        let g = gmst(jd_from_unix_ms(ms));
        assert!((deg(g) / 15.0 - (13.0 + 10.0 / 60.0 + 46.3668 / 3600.0)).abs() < 1e-4);
    }

    #[test]
    fn horizon_vector_is_a_unit_vector_with_sane_axes() {
        let o = Observer::new(40.0, -75.0);
        let ms = 1_750_000_000_000.0;
        let s = sky_at(ms, &o);
        let n = s.sun.dir.iter().map(|c| c * c).sum::<f64>().sqrt();
        assert!((n - 1.0).abs() < 1e-9);
        // A star on the meridian at the pole's altitude: Polaris-ish direction rises to the latitude.
        let rot = rotation_equatorial_to_horizon(ms, &o);
        let pole = [0.0, 0.0, 1.0];
        let h: [f64; 3] = std::array::from_fn(|i| rot[i][2] * pole[2]);
        assert!(
            (h[1].asin() / D2R - 40.0).abs() < 0.5,
            "pole altitude {}",
            h[1].asin() / D2R
        );
        assert!(h[2] > 0.7); // north
    }
}
