# Preview 3 audit — 2026-09-29

Reviewed commit `d99a069eb4562331c4035803d6250484183e999c` and release run [36642557009](https://github.com/cbassuarez/nus/actions/runs/36642557009). The corrections below are local and uncommitted. The tagged candidate does not include them.

## Findings corrected locally

- **High: Linux release validation could not start the application.** The runner installed `libxkbcommon-dev` but not the separate X11 runtime, `libxkbcommon-x11-0`. The new packaged-browser check panicked in `xkbcommon-dl` before CEF startup. Added the runtime to CI/release dependencies and the package's desktop prerequisites. This does not establish that Linux browsing passes; the corrected workflow still needs to run.
- **High: Retry after browser creation failure discarded pane geometry.** Replacing a failed `WebPane` produced its initial 1×1 page until another layout event. Retry now lays out the replacement immediately.
- **Medium: displayed error/overlay and action dispatch could disagree.** Native error drawing was added in the last commit, but dispatch still preferred an underlying interstitial. Drawing, keyboard selection and actions now select the same overlay first, including overlays above the HTML index. A regression test covers precedence.
- **Medium: inactive error tabs could consume clicks.** The overlay click handler iterated every tab, including stale page rectangles. Native error screens increased the cases affected. Hit testing now uses only the active tab.
- **Medium: WebKit visibility did not account for native error transcripts.** It checked only overlays and could cover an error drawn by nus. It now uses the same transcript selection as the renderer.

## Requested UI corrections

- Toast geometry uses the focused browser viewport, excluding its footer and DevTools. A visible toast reserves a narrow bottom strip from the native WebKit view, without resizing the underlying webpage. This makes its pixels and click targets accessible above WebKit.
- Automatic PiP requires audible playback; muted, zero-volume and known audio-free previews are excluded. Manual PiP remains available for these videos.
- A WebKit page moved into PiP leaves a centered “Playing in picture in picture” notice in its original pane. Clicking that pane returns the video and closes PiP.
- PiP resizing preserves the corner corresponding to the window center’s quadrant in the current monitor’s usable area. Buttons, wheel/pinch, edge drags, native size corrections and stream aspect changes use this anchor. Growth is limited at the fixed corner; portrait minimum size is proportional to its aspect.
- PiP no longer inherits the main window's decorative corner treatment. Its optional top stripe and controls remain.

## Other CI results

- Root CI fails `cargo fmt --all --check` in `crates/pty/src/discover.rs` and `crates/render/src/{gpu,grid,scene,text}.rs`. None of those files changed in the audited commit. These existing formatting differences are left outside this patch.
- The deterministic benchmark workflow compiled and ran its benchmarks, then its CodSpeed upload failed with HTTP 401. It needs valid service authorization; benchmark success must not be inferred from that failed run.
- Verified invariants and profile/component compatibility workflows passed.
- The standalone toast wording check reports one existing issue in `shells.rs`: the literal filename in “Could Not Create shells.json”. It is outside this patch.

## Validation and limits

- Native build passes; full native suite: 575 passed, one ignored.
- JavaScript transport/selection tests pass, including silent preview exclusion.
- Packaging tests: 25 passed; release support tests: three passed; workflow YAML parses.
- Live packaged browsing check passes after the recovery fixes: HTTP, JavaScript, actual page pixels, navigation timeout, renderer crash/hang, and keyboard retries. Evidence: `/tmp/nus-audit-browser-final/results.json`.
- Updated WebKit native fixture passes for top-level and same-origin nested video: silent automatic PiP exclusion, manual PiP, transport controls, reopen/close, toast viewport bounds, and source-pane notice captures.
- GPU captures were visually inspected for the notice, toast placement and PiP chrome. These captures do not include WebKit's separate native video layer. Desktop capture through the computer-use service timed out. Real streaming-service playback, DRM and native OS input delivery are not newly verified.
- macOS packaged release browsing and recovery checks passed. Linux failed before CEF as described above; Windows release compilation was still running at the last audit check. Publication is blocked by the Linux failure.

### PiP follow-up validation

The corner geometry unit tests and PiP control tests pass (nine tests). The native aspect fixture passes across all four quadrants, exercising wheel/pinch, edge drag, native resize, changing portrait/landscape aspect and reused PiP windows. Evidence: `/var/folders/68/_bgz9y8x01l2_tf4889_jhdw0000gn/T/nus-settings-check-nitmt_6g`. Native OS validation here is macOS; Wayland still delegates interactive window placement to its compositor.

Clicking the source-pane PiP notice also passes the native WebKit fixture for both top-level and nested same-origin video: PiP closes, the native view returns, and playback position is retained. Evidence: `/var/folders/68/_bgz9y8x01l2_tf4889_jhdw0000gn/T/nus-webkit-pip-check-k1e88a3f`. Input is through the app handlers, not OS event delivery.

### Rapid PiP skips

PiP skip clicks/keys now accumulate an intended destination, coalescing a burst after 100 ms of quiet and limiting continuous repeats to one seek per 250 ms. CEF/HTML media and WebKit's service adapter share this queue. It retains playing/paused intent, restores a seek-induced pause once per seek, and invalidates delayed work on explicit pause, scrub, frame-step, trusted page interaction or source/player replacement. Reaching the actual end of a video can still end playback normally.

`node scripts/check-pip-skip.cjs` passes for deliberately lagging and pausing HTML/service players, including cancellation and source replacement. Existing transport tests pass. Native fixtures pass forty rapid skip presses while playing and paused in WebKit (top-level and same-origin nested video) and CEF (generated seekable WebM). These checks use local media and app input handlers; they do not establish behavior for every protected streaming service. Evidence: `/tmp/nus-skip-native.log` and `/tmp/nus-cef-skip-native.log`. The selected swept-arrow icon design is now applied to the floating PiP controls (see below).

### Selected PiP controls

Floating PiP now uses mirrored swept arrows with the configured skip value underneath the arrowhead, its baseline aligned with the opposite tail. Values through 120 seconds fit without touching the tail. Transport centres, hit targets, keyboard/accessibility routing and rapid-skip commands are unchanged.

Hover brightens the icon itself with a diffuse glyph-shaped glow extending 5.5 logical pixels and an 18% scale increase; the pointer-hover rectangle is removed. Keyboard focus retains an explicit outline. Glow and scaling reuse the icon atlas entry rather than rasterizing another SVG or allocating a blur surface.

Native macOS build and JavaScript transport/rapid-skip checks pass. Native captures were inspected at normal and small PiP sizes for 10- and 120-second values, including skip-arrow and close-button hover. The local CEF fixture still preserves playing/paused state during rapid button bursts. Evidence: `/tmp/nus-swept-native.log` and `/var/folders/68/_bgz9y8x01l2_tf4889_jhdw0000gn/T/nus-settings-check-9a19man0`. This validates app-handler input and GPU captures on macOS, not Windows/Linux desktop rendering or protected-service playback.

### Home motion follow-up

The battery artwork policy could stop requesting frames until a caret/input event arrived. It now schedules a 30 Hz deadline from the frame start, included in the event loop's wakeup calculation. macOS battery polling uses IOKit instead of synchronously spawning `pmset`. Artwork command storage is reused, and polygon transforms avoid duplicate vectors. The allocation and measurement details are in [the home motion review](../performance/2026-09-29-home-motion.md); these changes remain uncommitted.
