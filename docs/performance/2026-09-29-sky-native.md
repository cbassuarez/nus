# Native ground-view Sky

The built-in Sky artwork now uses a native, procedural cloud volume viewed from
the ground. Earth/Limb/Darkroom are separate artwork and are not changed here.

## Behavior

- Low clouds have volume, self-shadowing, and sunlit edges. Independent middle
  and high layers move with separate winds. Overcast and rain form a ceiling or
  connected bank; there is no photographic background.
- Place drives the existing dated Sun/Moon ephemeris, including full directional
  vectors and lunar phase. Without Place, Sky uses an illustrated afternoon.
- Local weather is an explicit option beside Place in Start/New Tab. It sends
  rounded chosen coordinates to MET Norway. The background worker respects
  HTTP cache metadata and backoff, retains one bounded forecast in memory, and
  cancels on opt-out/location changes. No location is discovered automatically.
- Weather describes forecast conditions. Exact cloud geometry, cloud genus,
  base height, and unavailable upper winds remain modeled. Failed refreshes
  retain marked stale data; without cached data Sky uses illustrated conditions.
- Forecast changes ease with a 45-second time constant, without reseeding or teleporting the
  clouds. Typing holds the cloud field; reduced motion accepts conditions in a
  single still frame. Local exposure protects the prompt and command rows.
- Saved custom `c:sky` scripts continue using the original primitive. The new
  `c:atmosphere` and `c:weather` contracts are documented in `assets/art/API.md`.

## Rendering budget

The expensive volume is at most 768 × 512 and ordinarily updates twice per
second. A presentation pass reprojects it, moves upper layers, and composites
lighting at up to 24 Hz. Sky requests that same cadence on AC and battery;
combining a 30 Hz caller with a 24 Hz renderer would otherwise halve updates.
Hidden/unfocused and reduced-motion artwork requests no animation frames.
Identical held inputs reuse the existing texture without Sky GPU passes.

The stock 64³ RGBA noise is a deterministic, 1 MiB baked field, checked against
its Rust generator. Custom seeds remain procedural. Stable view IDs preserve
GPU resources across resizing; there are at most four cached views, trimmed
after 60 seconds unused. The per-view texture ceiling is about 7 MiB, excluding
pipeline/driver overhead.

Continuous motion remains enabled. Measurements below are specific to this
machine and workload; they do not establish battery life or a universal GPU
norm. Existing reduced-motion and focus policies still provide still frames.

## Native renderer evidence

Apple M4 Pro, Metal, optimized offscreen example, 1280 × 820 requested size:

| Submit through GPU completion | Samples | Median | P95 |
| --- | ---: | ---: | ---: |
| Cached presentation only | 46 | 0.283 ms | 0.727 ms |
| Volume plus presentation | 6 | 12.524 ms | 13.917 ms |

The CPU encoding/submission median was 0.053 ms. These serialized fence timings
include submission/wait overhead and exclude the rest of the native window.
Presentation GPU timestamp queries were zero/unresolved for 49 of 52 measured
samples; the three nonzero samples may be coalesced, so no GPU-only presentation
distribution or battery-drain claim is made.

Initial renderer construction took 134.65 ms in this run: 133.53 ms creating
pipelines and 0.89 ms uploading the baked noise. Pipeline compilation is a
remaining first-use cost and varies with driver cache state. The earlier run
before baking was not a controlled startup comparison.

Six native captures covered broken cloud, thin cloud, overcast, rain, sunset,
and Moon/night. They were visually inspected. Forty-nine repeated unchanged
calls issued zero Sky GPU passes. Native assertions also checked a held weather
refresh and a wind change at fixed time without displacement. Five focused
renderer tests passed, including WGSL validation and the exact stock-noise bake.

Raw measurements, executable/source hashes, and images are retained at
`/private/tmp/nus-native-sky/`. The example executable SHA-256 was
`9647b0694f6c64a6f69e2f26e2a7f4c7e21fea864d511999b04a83a955c98c2b`.

An isolated public-coordinate weather check at 60° N, 10° E returned HTTP 200,
84 forecast samples, all three cloud fractions, surface wind, and HTTP cache
metadata. It exercised the actual fetch/parser without reading a user profile.

## Native window evidence

The first integrated candidate was signed with executable SHA-256
`67c8e902e44b4b9f3c68602f83af5ece73ec39eebb189f32c16b0c71f8950b2f`.
Its six-case native check passed assertions and captured nine images. Visual
review then identified two fixes: saturated theme colors needed a Sky-specific
foreground contrast guard, and views reaching the horizon needed atmospheric
extinction to hide the volume's finite distance boundary.

A clean 12-second motion run on that candidate recorded 273 actual Sky presents,
with median/p95/max intervals of 42.86/54.61/59.55 ms. Volume intervals were
528.11 ms median and 541.70 ms p95. Sky CPU submission p95 was 0.174 ms and whole
window build/submission p95 was 0.383 ms. Process-tree CPU sampling measured
0.370 CPU seconds over 10.081 wall seconds (3.67% of one core). The initial
1.782-second typing hold was confined to the separate entrance phase.

An earlier run contained an unexpected input event and deliberate hold; it is
retained and is not used as a clean cadence sample. The original pre-change
baseline measured 5.47% of one CPU core, while a paired baseline had a changing
power state. These are short local observations, not a controlled energy study
or a causal percentage-improvement claim. No whole-window GPU-power measurement
was established. Continuous motion is retained on this evidence.

Raw profiles, assertions, executable hashes, screenshots, CPU samples, and both
motion runs are retained under
`/private/tmp/nus-sky-native-candidate-20260930/`; `perf-recheck` is the clean run.
The pre-polish executable is retained as `nus-before-contrast-horizon`.

Final acceptance used SHA-256
`a983153acb05fe282e16fc3f14891b576fa3feb10a691af4e60729dab94da2cb`.
All six cases passed again; the nine images confirmed readable neutral Sky text
under colored chrome and a smooth overcast/rain horizon without the seam.
The final clean 12-second run recorded 281 Sky presents (23.42 Hz), with
median/p95/max intervals of 42.856/43.686/43.997 ms. Volume updates were about
1.95 Hz. Sky CPU submission p95 was 0.584 ms; whole-window build/submission p95
was 1.389 ms. Process-tree sampling measured 0.500 CPU seconds over 9.737 wall
seconds (5.14% of one core). Power had changed to AC/charging, so it is not a
controlled CPU comparison against the earlier battery run. Final raw evidence
is in `visual-final`, `perf-final`, and `cpu-sampling-final.json` under that same
candidate directory. Structured receipts are also retained beside this report.

The final optimized build, 5 renderer tests, 17 artwork tests (2 existing
diagnostics ignored), 12 focused Sky tests, and weather fixture checks passed.
The offscreen timing table above predates the final horizon fade; the final
native-window measurements and captures include it. Battery-life impact and
whole-window GPU power remain unmeasured.

## Reproduce

```sh
cargo test -p nus-render sky::tests --lib
cargo test --offline --locked --manifest-path spikes/composite/Cargo.toml --bin composite art::tests
cargo test --offline --locked --manifest-path spikes/composite/Cargo.toml --bin composite sky
cargo test --offline --locked --manifest-path spikes/composite/Cargo.toml --bin composite weather
cargo run -p nus-render --release --example sky -- /tmp/nus-native-sky
python3 scripts/check-sky-native.py /absolute/nus.app --out /tmp/new-sky-captures
python3 scripts/perf-home.py /absolute/nus.app --art sky --seconds 12 --out /tmp/new-sky-perf
```

The capture helper uses disposable profiles and fixed clocks. Overcast/rain are
explicit renderer fixtures, not fetched weather. Performance runs use real
clocks and record executable hashes. Neither helper changes the user's profile.
`NUS_PERF` build/submit durations are CPU-side wall intervals, not GPU execution.
The offscreen example reports separate GPU queries and submit-to-completion
fences; zero/coalesced query samples are marked unresolved rather than free.
