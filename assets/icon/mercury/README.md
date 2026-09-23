# Mercury

`tidal.png` is the selected Tidal Mercury material study with the two orbital beads from Coalescing Mercury. Generated with the built-in image tool from the two approved references in `output/mercury-icon-options/`. The production asset has alpha transparency; the white cyclorama, shadows and star field belong to the claim modal, not the icon.

The native application embeds this artwork. **Settings → Look → App icon**
owns the live preview and Claim/Replay action. Both it and the claim modal use
continuous GPU reflection flow within the original alpha silhouette. Edges,
open counters and the two beads retain their authored shape. Reflections travel
along the stems and orbit; the icon does not wobble or breathe in size. The
claim fade-in is unchanged. The global reduced-motion preference uses still art.

On macOS, claim, replay and launch run the same construction loop over a
snapshot of the currently displayed Dock icon. The previous icon remains
underneath while the new silver shell flows into place; it retires smoothly
only during the final 28% of the loop. Replaying also starts a fresh shell.
A mercury bead feeds a full-scale ink front through the connected
metal of the actual n. Flow branches where strokes meet, follows the orbit,
and collects the two detached beads last. Intermediate forms are successive
states of that construction, not separate blobs or distance-field morph targets.
The native Dock owns bouncing: these frames add no vertical jump or squash.
Other desktop launchers retain the still icon.

`dock-motion.bin` embeds 84 launch and 128 eight-second loop frames at 256px.
These are precomputed and losslessly compressed, so startup does not rasterize
the liquid artwork or read image files. AppKit composites the precomputed
liquid frames over the captured existing icon during the transition. `dock-still.png` is the reduced-motion fallback.
The app restores the signed bundle icon on quit; it never rewrites that bundle.

To regenerate from the same approved alpha-correct 256px icon:

```sh
source scripts/env.sh
cargo run --locked --manifest-path spikes/composite/Cargo.toml --example mercury -- assets/icon/mercury/dock-still.png output/mercury-flow/frames
python3 scripts/pack-mercury-motion.py output/mercury-flow/frames assets/icon/mercury/dock-motion.bin
```

The packer requires Pillow and verifies exact pixel preservation. Native
verification uses `scripts/check-mercury.py` with an isolated signed app bundle.

The modal's procedural stars independently fade into and out of existence.
A bounded pointer history produces ordered Bayer geometry only in the hover
trail, dissolving over 2.3 seconds. The field belongs to the presentation and
never becomes part of the Dock icon. Continue remains the only modal control.

## Final generation prompt

Use case: precise-object-edit. Production desktop app icon PNG with genuine alpha transparency.
Image 1 is the selected Tidal Mercury icon and the edit target. Image 2 supplies ONLY the two small spherical mercury beads just beyond the right tip of its orbital ribbon.
Preserve Image 1's italic n shape EXACTLY, its elegant serif geometry, thickness, flowing liquid surface undulations, mirrored white softbox highlights and deep dark reflections, and the original orbit thickness. Do not adopt Image 2's thicker inflated n or bulbous feet.
Add the two beautiful small highly reflective liquid mercury beads from Image 2, in the gap just above the right tip of Image 1's orbit: larger bead closer to tip, second smaller bead slightly above and right. Match Image 1's lighting and material.
Remove the entire warm gray background AND floor/contact shadow, leaving genuinely transparent alpha, including inside the open counter and spaces around the orbit. Clean antialiased cutout with no pale fringe. The interior of the metal remains fully opaque, including its white highlights. No shadow or floor in the asset, these will be rendered by the app. Fit the complete n, orbit and beads comfortably inside the square with about 7% transparent safe margin on all sides. Single icon only. No text, tile, backdrop, gradient rectangle, watermark or extra objects. Preserve the remarkable realism and liquid-metal quality of image 1.
