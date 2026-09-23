# First arrival

The first-time opening lasts 14.6 seconds. One transparent native window carries
both the launch card and the final workspace, so there is no second window or
visible full-window rectangle waiting behind the card.

- 0–1.25 s: the card opens from a fine line; the background stars are points.
- 1.25–4.4 s: forward acceleration lengthens their trails.
- 4.4–6.4 s: warp brakes reduce outward speed and shorten trails to points.
  A separate constellation forms the authored italic n; the background stars
  never converge on the letter or reverse toward the center.
- 6.4–8.6 s: the solid n holds while a signal-colored star orbits it.
- 8.6–9.7 s: the star leaves the disappearing card and reaches the window edge.
- 9.7–12.75 s: it inks the window perimeter, respecting the existing corner radius,
  then travels along the upper edge.
- 12.75–14.6 s: the actual workspace is revealed beneath a moving ink edge.

Enter, Space, Escape or a click skip immediately. Reduced motion and Still show
a brief static mark; None bypasses the sequence. Completion/skip writes the
existing arrival-seen marker. New secondary windows do not run first arrival.
The normal interface, shell shape, spacing and colors are preserved afterward.

The expensive star choreography is baked offline at 120 Hz into a 4.45 MB
compressed vector bank. It expands to 10.87 MB before the first visible frame.
Playback interpolates adjacent samples, without particle simulation, random
generation, glyph rasterization, per-frame image decoding or disk reads. A 512px
prebaked mark and the cached workspace scene are released with the splash.
Background maintenance is deferred and unfocused 50 ms pacing is bypassed during
first arrival. The clock stays tied to elapsed time, so delayed frames do not
queue up or lengthen the ceremony. Geometry adapts to the current window size.

Regenerate assets with
`cargo run --manifest-path spikes/composite/Cargo.toml --example hyperdrive-bake`.
The baked sample regression verifies quantized positions against the authoring
curves. Runtime drawing still requires CPU/GPU scheduling; no desktop application
can guarantee uninterrupted presentation during an OS stall or exhausted GPU.
No image downloads, movie playback, added sound, or persistent animation timer.

`cargo test --manifest-path spikes/composite/Cargo.toml --bin composite hyperdrive::tests`
checks forward-only braking, separate star populations, perimeter continuity and
phase bounds. `scripts/check-hyperdrive.py <bundle>` captures native Paper/Ink
frames, checks fresh/seen profiles, skip/reduced/disabled modes, and exercises the
native green button followed by fullscreen exit. PNGs captured during arrival
retain transparency; ordinary page/document captures keep their opaque output.

Native validation is on macOS 15.6. Windows/Linux compositors and macOS 26 visual
appearance require their own runs. A compositor without transparent surfaces
cannot reproduce the desktop-visible handoff.
