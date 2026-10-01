//! Everything a renderer needs to paint the real sky at one instant, as plain
//! numbers: no types from here leak into the GPU code, and none from there
//! into the astronomy.

use crate::eclipse::{lunar_eclipse_at, solar_eclipse_at};
use crate::ephem::{planet, rotation_equatorial_to_horizon, sky_at, Planet};
use crate::{horizontal, Observer, D2R};

/// A planet as a point of light, already in East/Up/North.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlanetMark {
    pub planet: Planet,
    pub direction: [f32; 3],
    pub magnitude: f32,
    /// A B−V stand-in for the colour of its light.
    pub bv: f32,
    pub altitude: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SkyFrame {
    pub sun_direction: [f32; 3],
    pub moon_direction: [f32; 3],
    pub sun_altitude: f32,
    pub moon_altitude: f32,
    pub moon_illumination: f32,
    pub moon_waxing: bool,
    /// Apparent angular radii in radians.
    pub sun_radius: f32,
    pub moon_radius: f32,
    /// The Sun's disc still showing, 0..1 (1 = no eclipse).
    pub sun_visible: f32,
    /// How much of a total eclipse's corona to show, 0..1.
    pub corona: f32,
    /// Fraction of the Sun's diameter covered; 0 when no eclipse is under way.
    pub solar_magnitude: f32,
    /// J2000 equatorial → East/Up/North, row-major.
    pub rotation: [[f32; 3]; 3],
    pub ecliptic_north: [f32; 3],
    /// The Earth's shadow axis at the Moon's distance and its radii; all zero
    /// unless the Moon is in the penumbra.
    pub shadow_direction: [f32; 3],
    pub umbra_radius: f32,
    pub penumbra_radius: f32,
    pub umbral_magnitude: f32,
    /// How much moonlight washes out the faint stars, 0..1.
    pub moon_glow: f32,
    pub planets: Vec<PlanetMark>,
}

fn bv(p: Planet) -> f32 {
    match p {
        Planet::Mercury => 0.6,
        Planet::Venus => 0.15,
        Planet::Mars => 1.35,
        Planet::Jupiter => 0.75,
        Planet::Saturn => 1.05,
        Planet::Uranus => -0.05,
        Planet::Neptune => -0.25,
    }
}

fn f32v(v: [f64; 3]) -> [f32; 3] {
    [v[0] as f32, v[1] as f32, v[2] as f32]
}

/// The sky as `o` sees it at `ms`.
pub fn sky_frame(ms: f64, o: &Observer) -> SkyFrame {
    let sky = sky_at(ms, o);
    let solar = solar_eclipse_at(&sky);
    let sun_visible = (1.0 - solar.obscuration).clamp(0.0, 1.0);
    // Total only: a Moon larger than the Sun, all of it gone.
    let corona = if solar.moon_radius >= solar.sun_radius {
        1.0 - smooth(0.0, 0.012, sun_visible)
    } else {
        0.0
    };
    let mut shadow_direction = [0.0; 3];
    let (mut umbra, mut penumbra, mut umbral) = (0.0_f32, 0.0_f32, 0.0_f32);
    // The Moon is near the Earth's shadow only near full: skip the arithmetic otherwise.
    if sky.illumination > 0.97 {
        let l = lunar_eclipse_at(&sky, o);
        if l.penumbral_magnitude > 0.0 {
            shadow_direction = f32v(l.shadow_dir);
            umbra = l.umbra_radius as f32;
            penumbra = l.penumbra_radius as f32;
            umbral = l.umbral_magnitude as f32;
        }
    }
    let rot = rotation_equatorial_to_horizon(ms, o);
    let eps0 = 23.43929111 * D2R;
    let pole = [0.0, -eps0.sin(), eps0.cos()];
    let ecliptic_north: [f64; 3] =
        std::array::from_fn(|i| rot[i][0] * pole[0] + rot[i][1] * pole[1] + rot[i][2] * pole[2]);
    let planets = Planet::ALL
        .into_iter()
        .map(|p| {
            let q = planet(p, ms);
            let h = horizontal(q.ra, q.dec, ms, o);
            PlanetMark {
                planet: p,
                direction: f32v(h),
                magnitude: q.magnitude as f32,
                bv: bv(p),
                altitude: h[1].clamp(-1.0, 1.0).asin() as f32,
            }
        })
        .collect();
    let moon_glow = (sky.illumination * (1.0 - 0.92 * f64::from(umbral.clamp(0.0, 1.0)))) as f32;
    SkyFrame {
        sun_direction: f32v(sky.sun.dir),
        moon_direction: f32v(sky.moon.dir),
        sun_altitude: sky.sun.alt as f32,
        moon_altitude: sky.moon.alt as f32,
        moon_illumination: sky.illumination as f32,
        moon_waxing: sky.waxing,
        sun_radius: sky.sun_radius as f32,
        moon_radius: sky.moon_radius as f32,
        sun_visible: sun_visible as f32,
        corona: corona as f32,
        solar_magnitude: solar.magnitude as f32,
        rotation: std::array::from_fn(|i| std::array::from_fn(|j| rot[i][j] as f32)),
        ecliptic_north: f32v(ecliptic_north),
        shadow_direction,
        umbra_radius: umbra,
        penumbra_radius: penumbra,
        umbral_magnitude: umbral,
        moon_glow,
        planets,
    }
}

fn smooth(a: f64, b: f64, x: f64) -> f64 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn totality_has_a_corona_and_a_dark_sun() {
        // Burgos, mid-totality 2026-08-12 ≈ 18:29:07 UT.
        let o = Observer::new(42.34, -3.70);
        let f = sky_frame(1_786_559_347_000.0, &o);
        assert!(f.sun_visible < 0.01, "{}", f.sun_visible);
        assert!(f.corona > 0.9);
        assert!(f.solar_magnitude > 1.0);
        // An hour before, nothing.
        let f = sky_frame(1_786_559_347_000.0 - 3_600_000.0 * 3.0, &o);
        assert!(f.sun_visible > 0.999 && f.corona == 0.0);
    }

    #[test]
    fn lunar_totality_puts_the_moon_in_the_umbra() {
        // 2025-03-14 06:59 UT: total lunar eclipse.
        let f = sky_frame(1_741_935_540_000.0, &Observer::new(40.0, -100.0));
        assert!(f.umbral_magnitude > 1.1, "{}", f.umbral_magnitude);
        assert!(f.umbra_radius > f.penumbra_radius * 0.0 && f.penumbra_radius > f.umbra_radius);
        assert!(f.moon_glow < 0.2, "{}", f.moon_glow);
        assert_eq!(f.planets.len(), 7);
    }

    #[test]
    fn rotation_is_orthonormal() {
        let f = sky_frame(1_750_000_000_000.0, &Observer::new(-33.9, 151.2));
        for i in 0..3 {
            let n: f32 = f.rotation[i].iter().map(|v| v * v).sum();
            assert!((n - 1.0).abs() < 1e-5);
        }
        let n: f32 = f.ecliptic_north.iter().map(|v| v * v).sum();
        assert!((n - 1.0).abs() < 1e-4);
    }
}
