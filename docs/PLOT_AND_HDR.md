# Plot and Radiance

`page_signal.rs` owns parsed local-address classification and Plot geometry.
The page keeps its content dimensions and pointer behavior. Four solid brackets
anchor a quiet, stationary dashed perimeter. Compact address/status versions
keep only the brackets. Local pages use these bounds alone, without an overlaid
colored loading strip. The marks have a narrow contrasting keyline so they
remain visible over light and dark page content. Each pane owns its own bounds.

Radiance is an instanced strip in the existing quad renderer, not a separate
animated web view. Its full-width fill follows reported loading progress. The
small tip on non-local pages emits above SDR white; the resting Plot perimeter does not. Completed
loads fade out and stop requesting animation frames. There is no fictional
trickle progress. Existing Rule, Comet and Carapace choices remain available.

HDR uses a separate RGBA16Float pipeline with ExtendedSrgbLinear surfaces when
advertised by wgpu. UI textures are decoded to linear light, Windows SDR white
is normalized against scRGB's 80-nit reference, and regular PNG captures use the
original SDR pipeline. Surface capability and current display headroom are
separate facts. Unsupported native surfaces use SDR, and the OS handles the
available headroom of a supported extended surface.

Run `scripts/check-plot-hdr.py <bundle>` on macOS for native state/geometry and
shader-pixel checks. The script uses disposable profiles and a loopback fixture
server. `hdrpixels` renders the production loading shader into RGBA16Float and
checks that its tip exceeds 2× SDR white while a white reference remains 1×.
An SDR screenshot cannot verify display luminance.
