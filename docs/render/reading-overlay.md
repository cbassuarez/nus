# Home artwork reading overlay

This is the transparency follow-up to the native Space patch. It changes only
Home's reading material, its artwork palette selection, and targeted tests.
It does not change Space rendering expressions, assets, motion, or controls.
It also closes a missing `}` after `visibility()` in the preceding Space patch;
that one-line delimiter correction prevents an unclosed-function parse error.
Startup,
browser recovery/default handling, settings previews, and native error-screen
papers are not modified.

## Appearance

- Artwork prompt/results: 0.10 alpha (10% opacity).
- Artwork footer: 0.03 alpha (3% opacity), using a 0.30 multiplier.
- Dark artwork uses black; Light artwork uses white. Theme-following artwork
  uses its resolved local-paper tint at the same 10%/3% opacities.
- Existing physical core geometry and 56/20 logical-pixel outward feathers stay.
- Text, caret, focus and opaque reverse selection are NOT multiplied by the
  veil alpha. Their existing entrance animation remains unchanged.
- There is one material draw: color.a * max(mask0*weight0, mask1*weight1).
  The overlap is not two source-over layers. Footer pixels overlapped by the
  reading-area feather may inherit that stronger feather, but never sum them.

The kind-23 HDR branch that replaced the requested tint/alpha with opaque RGB
paper has been removed. `fs_main` still converts/scales RGB for extended-linear
surfaces, while preserving alpha. Identical alpha does not imply identical
encoded pixel brightness across SDR and HDR blend spaces.

## Compatibility and unchanged behavior

`Scene::reading_field` and the three-argument `Scene::reading_fields` remain
callable. The old optional linear-surface argument is accepted but no longer
changes color or opacity. Use an explicitly opaque input color for a solid
surface; plain Home already does this and retains its local-paper treatment.

`reading_fields_weighted` uses the reserved y component of the third vec2 point
record for each region's weight. The 64-byte Instance, points stride (three
vec2s per region), existing bind groups, pipelines and draw kind stay the same.
The Scene writer and quad shader must be rebuilt together. Zero or invalid
regions append nothing; weights greater than one are clamped; the existing
clip/layer is retained. A zero-strength footer does not expand draw bounds.

`Palette::for_art` changes only the veil opacity after resolving the established
foreground palette. Its backdrop bounds describe intended background polarity,
not a measured guarantee of text contrast over every arbitrary image/HDR pixel.
The existing strong-surface Palette::new tests remain meaningful for that API;
new tests explicitly show that the faint artwork variant cannot make the same
universal contrast claim. No increased opacity or copied CSS mask is hidden in
another pass to restore that guarantee.

## Checks

Read-only source-contract and independent scalar-oracle checks:

```sh
python3 scripts/check-reading-overlay.py
```

Native Scene, production WGSL/Naga, and Home palette checks:

```sh
source scripts/env.sh
cargo test --locked --release -p nus-render reading
cargo test --locked --release --manifest-path spikes/composite/Cargo.toml \
  --bin composite home_contrast::tests
cargo check --locked --manifest-path spikes/composite/Cargo.toml --bins --lib
```

Explicit real-GPU test, using production Scene instances, quad.wgsl, and the
same binding/immediates/blending layout (no CEF or window needed):

```sh
cargo test --locked --release -p nus-render --test reading_overlay \
  reading_overlay_sdr_and_hdr_gpu_readback -- --ignored --nocapture
```

It checks RGBA8 SDR and RGBA16F linear output at white scales 1 and 2, over
opaque white and transparent backgrounds. Samples cover core, footer, overlap,
feather, clipped feather, untouched pixels, a legacy Some(paper) caller, and an
unrelated opaque solid. No adapter is a failure, not a passing GPU test. The test
is ignored by default because CI may have no compatible adapter.

Finally rebuild the actual application bundle and view Home on the real target
display. Inspect Space/Earth and Darkroom, Sky in light and dark states, another
artwork and plain Home, wide/narrow/HiDPI, text selection, and a hidden footer.
An SDR offscreen screenshot alone cannot test the previous live-HDR regression.
The native Space source checks should still pass; do not modify its renderer or
its overlay-independent edge shading to make these tests pass.

## Verification boundary

The generation environment can apply/check/reverse patches and run Python
contracts/oracles. It has no cargo/rustc or wgpu bindings. The shipped Rust/Naga
and real-GPU tests are not claimed to have run until target evidence is supplied.
