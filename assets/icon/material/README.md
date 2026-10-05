# Enamel pebble material fields

These banks implement the approved Blender treatment: pearlescent white enamel,
a polished black curved return and a Space-tinted glass orbit under soft studio
lighting. The n is one continuously curved closed volume with no straight
extruded wall. The backdrop and floor remain transparent in app icons.

Dock framing matches the flat icon's approximately 90% horizontal footprint;
the master keeps front, detail and curved-back inspection cameras.

The six font faces retain the actual bundled outlines and optical sizing.
Each bank stores seven grayscale samples of scene-linear radiance; the renderer
evaluates each RGB channel at the current arbitrary Space color, then applies
the same Khronos PBR Neutral display transform as the Blender master. This is
an approximation of the ray-traced color response between samples. White
reflections, pearl color and dark body are baked into that response.

The app needs no Blender, external asset files or per-frame ray tracing.
`dock_icon::Field` shares lazily decoded 256px fields across native previews,
launch, attention and color changes. Full 1024px fields are decoded temporarily
for larger exports. The retained six fields occupy about 17 MiB. Resizing uses
associated alpha in linear light, preserving clean transparent edges.

Format: `NUSD3D02` contains independently compressed 256px native and 1024px
export levels. Each level is zlib over `NUSD3D01`: size, sample count, radiance
range, seven float32 knots, one byte of alpha per pixel and sample-major RGB
uint16 values encoded as `sqrt(radiance / 64)`. The native level is derived in
associated scene-linear light. Small updates read only that compact level.
`manifest.json` records the master, 256-sample render setup, 0.85px pixel filter
and bank hashes. The regular stroke uses a narrow geometric coating boundary,
with depth-scaled coverage in the procedural master. The baked-master hash is
retained separately if only portable source or notes are updated after baking.

Regenerate from the approved master in the adjacent `nus-promo` project:

```sh
/Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
  ../nus-promo/out/logo-3d/enamel-pebble/nus-regular.blend \
  -P ../nus-promo/blender/bake_dock.py -- \
  --kind regular --kind regular --output assets/icon/material --proof ../nus-promo/out/logo-3d/dock-bake
cargo run --offline -p nus-render --example icon -- assets/icon
cargo run --offline -p nus-render --example dock_material -- output/icon-material
python3 ../nus-promo/blender/check_dock_bake.py \
  ../nus-promo/out/logo-3d/dock-bake output/icon-material
```

The signed package and the startup bank use this same renderer. The small
incognito/redaction mark retains its existing flat coverage and bar.

`scripts/check-icon-material.py <review.app> <runtime-proof-directory>` verifies
all six selections, Space colors and saved selection in an isolated native
profile. Its Swift companion compares actual Dock TIFF readbacks and source
PNGs through AppKit in sRGB, respecting the monitor profile in the native TIFF.

The display equations follow the [Khronos PBR Neutral reference](https://github.com/KhronosGroup/ToneMapping/tree/main/PBR_Neutral).
