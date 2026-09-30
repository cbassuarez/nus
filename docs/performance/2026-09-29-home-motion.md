# Home animation pacing and allocation review

## Reproduced cause

The battery artwork policy returned `self.frames % 2 == 0` from
`art_wants_frame()`. A false result removed the request for the next frame;
it did not schedule a frame at half the refresh rate. On an otherwise quiet
home page the animation waited for another event, commonly the caret's next
change. The optimized macOS battery baseline reproduced gaps around 660 ms
across Memphis, pond, space and sky. This was a scheduling failure, not evidence
of a video-frame buffer overflow or general heap exhaustion.

The replacement sets a deadline 1/30 second after the current frame starts.
The outer event loop includes that deadline in its wakeup calculation, and
the normal tick marks the window dirty when due. Beginning the interval before
artwork drawing prevents its CPU cost from being added to the frame period.
Each build clears obsolete requests; reduced-motion, inactive or removed
artwork does not keep the timer alive. Plugged-in artwork keeps its existing
continuous redraw policy.

macOS power detection now uses an IOKit snapshot, with the existing five-second
cache. It previously launched and waited for `pmset` on the UI thread. The
large measured stalls are explained by the missing frame requests; the power
query change removes a separate avoidable synchronous subprocess.

## Allocation changes implemented

- Artwork command vectors return to their owning Lua canvas after rendering,
  retaining capacity for the next frame. The existing command count/byte limits
  remain enforced. A regression test verifies capacity reuse and absence of
  stale commands.
- Polygon coordinates are transformed in their existing vector. Quads use a
  four-element stack array. Rendering no longer allocates a second vector for
  each of these commands.
- Home, settings artwork previews and welcome artwork all return their command
  buffers. Scene and GPU instance buffers already retain capacity in existing code.

These changes reduce identified allocation work. They do not establish a
measured reduction in total process memory or total allocations per second.
Preserving vector capacity is intentional; Rust documents that clearing a
vector retains its allocation. [Rust Vec documentation](https://doc.rust-lang.org/stable/std/vec/struct.Vec.html#method.clear)

## Next heap optimization priorities

These recommendations were subsequently implemented and extended in the
[heap follow-up](2026-09-29-heap.md), with allocation counts and native validation.

1. **Measure transient allocations with stacks.** Use Instruments Allocations
   during Memphis motion, prompt typing, artwork selection and video playback.
   Track allocation count/bytes per second separately from retained bytes and
   physical footprint; correlate spikes with frame gaps. Apple documents that
   Allocations reports both the allocations and the responsible code.
   [Apple memory profiling guidance](https://developer.apple.com/documentation/xcode/gathering-information-about-memory-use)
2. **Avoid allocation on text-cache hits.** `FontSystem::shape` constructs an
   owned string before looking in its cache. `measure` has similar owned-key
   work. A borrowed lookup or appropriately bounded interned key can remove
   that churn while retaining current cache limits. Preserve font fallback,
   text, size and tracking identity.
3. **Reuse geometry storage and Lua tables.** `points_of` still creates one
   vector per polygon; Memphis also creates local contour and transformed-point
   tables repeatedly. Consider a bounded vertex arena plus command ranges,
   and reuse/cache invariant Lua contours. Keep the animated squiggle, parallax,
   entrance timing and pointer response intact. Release idle scratch storage
   under memory pressure rather than retaining every historical maximum.
4. **Reduce prompt snapshots and cache-key formatting.** Home regenerates a
   debug-formatted key containing prompt configuration and tab titles, then
   clones cached rows into the pane and again for drawing. Revision-based
   invalidation and borrowed/shared snapshots are candidates; input edits,
   reordered tabs and changed actions must still invalidate immediately.
5. **Reuse software-video bindings with their textures.** The Windows/Linux
   software paint path retains textures but creates a new bind group per
   paint. Cache the binding for the same texture generation and rebuild on
   resize. Measure on those platforms before attributing video stutter to it.

Keep the current allocator until a trace shows meaningful time inside it.
Reducing allocations in hot paths and preserving bounded reuse has clearer
code-level opportunities here than a global allocator change.

## Native results

Optimized macOS 15.6 / arm64 runs on battery, with ten seconds requested per
ongoing-art phase. This is one accepted baseline and one final run per artwork,
not a statistical confidence interval. The existing battery policy targets
30 Hz; additional caret/input events may produce shorter frame intervals.

| Artwork | Before p95 gap | After p95 gap | Before largest gap | After largest gap |
| --- | ---: | ---: | ---: | ---: |
| Memphis | 372.46 ms | 34.89 ms | 661.73 ms | 35.18 ms |
| Pond | 369.95 ms | 35.04 ms | 662.27 ms | 35.48 ms |
| Space | 654.40 ms | 34.67 ms | 660.10 ms | 35.01 ms |
| Sky | 368.00 ms | 34.97 ms | 660.70 ms | 35.81 ms |

Memphis ongoing frame-build/submission p95 fell from 12.25 ms to 3.28 ms.
Its final entrance phase had a 34.83 ms p95 interval and one 56.96 ms maximum;
ongoing motion's maximum was 35.18 ms. The change therefore removes the repeated
long pauses measured here, without claiming that every startup frame hits its
budget or that all video playback is fixed.

Full metrics and executable hashes are retained in
[the evidence JSON](2026-09-29-home-motion.json). Raw logs and native captures
remain at the evidence directories recorded there. The Memphis baseline used
the same 2.2-second warmup as the final probe; the earlier pond/space/sky baseline
used 1.5 seconds. The table compares ongoing motion after those warmups. The
final native Memphis capture was inspected and renders the existing artwork.

Validation: optimized composite build passed; the focused artwork test command
passed all 10 selected tests, including command-buffer reuse and artwork resource
limits. `scripts/perf-home.py` completed all four artworks, and Python syntax and
`git diff --check` passed. Windows/Linux native presentation and an Instruments
allocation trace were not run. Changes remain uncommitted.

## Measurement boundaries

`scripts/perf-home.py` uses an isolated profile, real clocks, enabled motion and
`NUS_SHOT_SIZE=1440x900`. It records both entrance and ongoing motion, executable
hashes, power-source information and raw logs. Timings come from optimized builds
with the same opt-in instrumentation on both sides. No synthetic CPU load was
introduced in the accepted comparison runs. A first Memphis baseline overlapped
briefly with compilation, so it is retained separately and excluded.

`frame_interval` is spacing between app frame starts; `frame_build_submit` is
CPU work through submission. Neither is display scanout or a GPU timing query.
The 16.67 ms threshold in the shared metrics is a 60 Hz reference, not a failure
threshold for the 30 Hz battery policy. These runs do not establish Windows/Linux
presentation performance, protected streaming-service smoothness, or behavior
under every workload.
