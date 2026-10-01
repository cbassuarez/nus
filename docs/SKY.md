# The real sky

"The sky" (a Home art) is the sky over the Place you chose: the Hipparcos stars,
the planets, the Moon in its true phase, the Sun, and — when it happens — an
eclipse. Nothing is looked up. Every position is computed on the device from a
published series, and nothing at all runs without an explicit Place.

This page is for people working on it. The rest of nus is not told about any of
it; see "Quiet extras" below for what a person can find.

## Where it lives

| piece | where |
|---|---|
| ephemerides, eclipses, almanac | `crates/astro` (`nus-astro`) — pure Rust, no network, no data files |
| what the shader needs for one instant | `nus_astro::sky_frame(ms, &Observer) -> SkyFrame` |
| stars, planets, figures, eclipse, corona, lunar shadow | `crates/render/src/sky.rs`, `sky.wgsl` (`Celestial` in `SkyParams`) |
| catalogue, IAU names, figures | `crates/render/src/space.rs` (`catalogue`, `star_names`, `figures`), generated `space_names.rs` |
| the app side: picks, labels, clock, framing, chord | `spikes/composite/src/skyview.rs` |
| the art opts in | `assets/art/sky.luau` — `astro = place ~= nil` on `c:atmosphere` |

## Accuracy (and why it is enough)

* **Sun** — truncated VSOP87D Earth (Meeus app. III), FK5 correction, nutation,
  aberration. Equinox and solstice instants land within 0.6 minute of the
  almanac (`events::tests::solstices_and_equinoxes_match_the_almanac`).
* **Moon** — Meeus ch. 47 (truncated ELP-2000/82), about 10″ in longitude.
  Reproduces Meeus's worked example 47.a to 2e-4°. New and full moons are within
  3 minutes of the almanac.
* **Planets** — Standish's approximate Keplerian elements (JPL, 1800–2050),
  good to a few arcminutes, with light-time. A naked-eye sky, not a table.
* **Eclipses** — found by geometry, not looked up: the topocentric Sun and Moon
  against their radii, contacts by bisection. Dallas 2024-04-08 totality begins
  within 2 s of 18:40:44 UT and lasts 3 m 59 s (published ≈ 3 m 52 s); the
  2026-08-12 total eclipse and the 2025–26 lunar eclipses match published times
  and magnitudes (umbral magnitude within 0.02). The Earth's shadow is enlarged
  2 % for its atmosphere (Danjon). Path edges are good to a few kilometres, not
  to a metre: this is not eclipse-chasing software.
* ΔT is observed to 2025 and extrapolated; a few seconds are invisible (the Moon
  moves half an arcsecond a second).
* The Moon is rendered without libration or maria; its terminator, its dark disc
  in front of the Sun, and its copper in the umbra are correct.

`cargo test -p nus-astro` runs the validation. `cargo run -p nus-astro --example
almanac -- <lat> <lon> [unix_ms] [tz_minutes]` prints what `sky`, `moon` and
`tonight` say.

## Rendering

`Celestial` rides in `SkyParams`; absent, the sky is exactly as before (decorative
night). With it:

1. A star pass lays the catalogue (9,827 stars, point sprites sized by
   magnitude, extinction by airmass, limiting magnitude by Sun altitude and
   moonlight), the seven planets (in the catalogue's spare rows, rewritten per
   frame) and, on request, the constellation strokes into the target — opaque
   black, additive.
2. The presentation pass writes `(colour, 1 − how much of the star layer shows)`
   with the blend `colour + stars × (1 − alpha)`, so clouds and horizon haze
   sit in front of the stars with no second texture.
3. An eclipse is the Sun's disc minus the Moon's (angles by `atan2(|a×b|, a·b)`;
   `acos(dot)` runs out of float precision at a solar width), the sky dimmed
   toward twilight by the light left (`sun_y()`), a 360° sunset at totality, and
   a corona with equatorial streamers and polar plumes. Clouds are lit by the
   same dimmed Sun, so they darken with it.
4. A lunar eclipse shades the disc by the umbra and penumbra (soft edge, copper
   heart) and takes the moonlight out of the sky, so the stars come back.

`cargo run -p nus-render --example celestial -- <dir> [case]` renders offscreen
captures (night, constellations, every eclipse phase, annular, lunar).

## Quiet extras (settings · EXPERIMENTS, opened by a chord on the Home prompt)

Each is a switch; all are on by default except *nus will wait*.

* **Name a star** — click a star, planet, the Sun or the Moon on the sky (or a
  named star on the Space page); its name, designation, class, distance and
  magnitude settle beside it as plain words in the sky's own layer. 338 stars
  carry IAU names (`scripts/generate-star-names.py`).
* **Turn the sky's clock** — scroll over the sky: a notch is 20 minutes, Shift a
  day, Alt a month. Esc returns to now. `eclipse` at the prompt jumps to the next
  one you can see.
* **Draw the constellations** — hold Ctrl+Shift (after a beat): the figures are
  drawn a stroke at a time, then named.
* **Let the sky drift** — after 45 s untouched the sky runs 60× (an hour a
  minute); any input unwinds it.
* **Almanac at the prompt** — `sky`, `moon`, `tonight`, `eclipse`; `nus sky`,
  `nus moon`, `nus tonight` do the same from any shell. The sky carries one dry
  line when something is on (a countdown to totality, a shower's peak, the Unix
  clock reaching 1,800,000,000 or 2³¹).
* **Exit status manners** — hover a command block's lamp: a dry, accurate word
  for 1, 2, 42, 126, 127, 130, 137, 139, 143 (`manners.rs`).
* **Sky of this build** — press and hold the masthead number on the profile card:
  the sky at the moment of the build's commit (`NUS_BUILD_EPOCH`), over your
  Place (or Greenwich).
* **nus will wait** (off) — while an eclipse is total where you are, a finished
  command's notice (Hatch completion, dock attention, Finish Work Complete) is
  held and delivered afterwards.

An eclipse also turns the view to look at it, thins the *modelled* clouds (never
a real forecast's) and takes the text polarity of a dark sky.

## Data and attribution

* ESA Hipparcos Main Catalogue (I/239) and the new reduction (I/311), via CDS
  VizieR; the IAU Catalog of Star Names (WGSN). Source files are kept in
  `crates/render/src/space-data/names/` and regenerated byte-for-byte by
  `scripts/generate-star-names.py` (`--check` by default).
* Western constellation figures: Stellarium, CC BY-SA (as for the Space page).
* Algorithms: J. Meeus, *Astronomical Algorithms* (2nd ed.); P. Bretagnon &
  G. Francou, VSOP87; E. M. Standish, *Keplerian Elements for Approximate
  Positions of the Major Planets* (JPL); meteor shower peaks from the International
  Meteor Organization's calendar.
