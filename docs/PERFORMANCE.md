# Performance

nus has two deliberately different performance layers. They answer different questions and should not be collapsed into one number.

| Layer | Tool | Question |
|---|---|---|
| Deterministic library work | Criterion locally, CodSpeed in CI | Did this change make CPU work more expensive? |
| Native application behavior | `NUS_PERF` + `scripts/perf-native.py` | Did the real application get slower on this platform? |

A benchmark protects a performance-sensitive behavior, not merely a function that happens to be measurable.

## Deterministic benchmarks

The Rust benches use `codspeed-criterion-compat` under the crate name `criterion`. Outside `cargo codspeed` it is ordinary Criterion, so local and CI runs execute the same benchmark source.

```sh
cargo bench -p nus-vt
cargo bench -p nus-render
cargo bench -p nus-pty
cargo bench -p nus-sync
```

Criterion's local HTML report is written under `target/criterion/report/index.html`. It includes distributions and comparison plots; use it when optimizing locally rather than treating a single terminal number as ground truth.

The CI suite uses CodSpeed simulation. It is intentionally limited to deterministic CPU paths. It does not claim to measure GPU presentation, shell startup, filesystem latency, host font discovery, network access, or CEF page loading.

### Catalog

- `vt/advance_v2/*`: VT parsing and terminal-state application for plain, ANSI-heavy, cursor-heavy, TUI, Unicode and sustained-scroll streams. Whole-buffer and realistic chunked delivery are separate cases.
- `vt/search/*`: terminal history search over a populated 10k-line scrollback.
- `vt/resize_v2/*`: populated-grid resize/reflow boundaries.
- `vt/scroll_v2/*`: direct grid scrolling at representative row counts.
- `render/shape/*`: bundled-font shaping, including the implementation's 512/513-byte cache boundary.
- `render/measure/*`: cached text measurement.
- `render/glyph/*`: cold and hot glyph raster/cache paths using bundled IBM Plex Mono only.
- `render/scene/*`: CPU-side scene construction and layer finalization. No GPU adapter is created.
- `render/policy/*`: contrast grading and palette-nearest math.
- `pty/frame_encode/*`, `pty/frame_decode/*`: complete in-memory holder frame codec lifecycles.
- `pty/frame_parse_v2/*`: whole-buffer and fragmented parsing, with decoded payload disposal included. Fragmented delivery also includes copying arriving chunks into the pending buffer.
- `pty/ring_v2/*`: holder output ring fill and steady-state wrapping.
- `sync/union_lines/*`, `sync/key/*`, `sync/open/*`: deterministic merge, key codec and decrypt paths.

`nus_sync::seal()` is intentionally absent from the CodSpeed suite because it obtains a fresh nonce from OS randomness. Mixing that syscall into a simulated CPU benchmark would make the result less trustworthy, not more complete.

## Benchmark discipline

Names are stable and use `subsystem/operation/scenario`. Renaming a published benchmark breaks its historical line and should be treated as a schema change.

Fixture generation, assertions, logging, filesystem setup, randomness, host-font discovery, network access and sleeps stay outside deterministic timed regions unless one of those operations is explicitly the subject of the benchmark. Mutable state uses batched setup so samples begin from a known state. Outputs are observed with `black_box` where optimization could otherwise erase work.

### Timing-boundary repair and baseline migration

The original scroll benchmark passed an exclusive bottom bound to an inclusive
`Grid::scroll_up(top, bottom, ...)` API. Full-screen scrolling must end at
`rows - 1`, not `rows`. The adapter used by the benchmark and ordinary fixture
tests now encodes that contract; production grid behavior is unchanged.

The original by-value batched closures also destroyed their Term, Grid, Scene,
FontSystem or Ring inside the measured operation. That is inappropriate for a
benchmark intended to isolate ingestion, resize, layer closure or cache misses.
The corrected suites use `iter_batched_ref`: setup and setup-state destruction
are outside the timed region. Large states use `BatchSize::PerIteration` to bound
live memory; small-operation batches are capped at 16 states. Per-iteration
clock overhead is not suitable for nanosecond operations; `Scene::finish()` uses
the bounded multi-state batch instead.

Changed metrics deliberately receive new IDs:

```text
vt/advance_v2/*
vt/resize_v2/*
vt/scroll_v2/*
render/glyph/cold_ascii_v2
render/scene/terminal_*_v2
render/scene/finish_v2/*
pty/frame_parse_v2/*
pty/ring_v2/*
```

Do not present a v1-to-v2 reduction as an application speedup. These are new
measurement baselines. Preserve previous artifacts for auditing; do not combine
the two ID families in a regression comparison. Unchanged benchmarks retain
their original IDs.

The exclusion is specific to setup-state teardown, not a blanket exclusion of
allocation or freeing. Allocations made by the measured operation remain timed.
Search, codec, shaping and decrypt `iter` cases retain their existing result
lifecycle boundary. Parser v2 observes decoded payloads with `black_box` and
includes their disposal in both whole and fragmented delivery. Input-buffer
teardown is excluded. `Scene::finish()` measures closing a populated layer, not
freeing all instances or rendering/presenting a GPU frame.

Generated terminal fixtures are validated once before timing. They are preferred over captured sessions because they are deterministic, reviewable and free of user data. Add a sanitized captured trace only when it protects behavior that cannot be expressed by a generated fixture.

## Native performance

Set `NUS_PERF=1` to enable bounded, in-memory instrumentation. With the variable unset, timing scopes do not take timestamps and no samples are retained.

Startup metrics are one-shot elapsed times from process `main()`:

- `startup_private_ready`
- `startup_dock_ready`
- `startup_cef_deferred` (native startup; CEF has not initialized yet)
- `startup_event_loop_ready`
- `startup_window_created`
- `startup_app_ready`
- `startup_first_present`

`startup_cef_ready` is emitted separately on first browser use. `browser_initialization` records that one-time engine initialization cost; it must not be described as eliminated. The harness also accepts the former eager-startup milestone when measuring older builds.

`main_to_first_submit` remains as the legacy alias for `startup_first_present` so existing scripts keep working.

Recurring runtime metrics currently include `frame_build_submit`, `input_handler`, `ui_turn_work`, and editor open-to-present timings. Runtime aggregation belongs to the main UI/event-loop thread; background work is measured from request until its result becomes observable there.

### Native harness

The harness launches a real app with a fresh profile for every sample, uses the existing `NUS_SHOT` protocol for semantic assertions, retains every raw run, and emits both JSON and a self-contained HTML visualization.

```sh
python3 scripts/perf-native.py \
  --app dist/nus.app \
  --scenario startup \
  --runs 10 \
  --out target/perf/startup.json

python3 scripts/perf-native.py \
  --app dist/nus.app \
  --scenario editor-open \
  --editor-bytes 10485760 \
  --runs 10 \
  --out target/perf/editor-10m.json
```

**Rebuild and repackage the native bundle before running the new harness.**
Root-workspace checks or rebuilding the composite executable alone do not update
an existing `dist/nus.app`. This harness requires the new `awaitperf` shot command
in the executable inside that bundle. Use the existing release/bundle build path.

On macOS every measured launch receives these volatile command-line defaults:

```text
-ApplePersistenceIgnoreState YES
-NSQuitAlwaysKeepsWindows NO
```

The intent is a fresh launch with no restoration/recovery prompt, rather than
measuring the delay while a person chooses Reopen or Don't Reopen. The harness
never writes global defaults, deletes saved-window state or automates clicking
another application. These flags are confined to its child command. macOS system
dialog behavior still needs verification on the macOS version being measured;
a blocked run must time out and fail, never contribute a timing sample.

Each launch has a fresh onboarded nus profile, isolated working directory and
per-run temporary directory. Inherited `NUS_*` test hooks are removed before the
harness sets its own values. The shell, OS and filesystem caches are not made
hermetic by this isolation. A fresh profile is **not** a cold-disk or cold-OS-cache
benchmark; that distinction is recorded in provenance.

`awaitperf startup_first_present` keeps the event loop running until the semantic
milestone exists. It does not reset startup samples, sleep for a guessed duration
or count a first-frame assertion as a readiness wait. Editor scenarios wait for
the correct size-specific `file_open_submit`, `file_10m_open_submit` or
`file_100m_open_submit` metric. The parent process enforces the timeout. Existing
startup milestones measure cumulative elapsed time from `main()`; do not add
them together. First-present here means return from the app's present path, not
hardware display-scanout latency.

Schema 2 output includes `running` / `failed` / `complete` status, source checkout
revision separately from binary version and executable SHA-256, exact launch
arguments, requested run counts, warmups, all validated per-process summaries,
attempts and log paths. The executable hash is checked again at completion.
Source HEAD is not assumed to identify the binary. Each attempt retains a log
under `<output-stem>-logs/<run-id>/`. Failed attempts retain partial JSON/HTML
and cause a nonzero exit. Missing, duplicate, malformed or wrongly labelled
records, non-finite/negative timings, missing first-present and inconsistent
startup aliases are rejected rather than silently skipped.

The HTML plots every retained process value without clipping outliers, labels
sample counts and missing runs, shows failure status and permits raw-value
inspection. Each sparkline has its own zero-to-maximum scale; the bars compare
medians within the report. No script, stylesheet or asset is fetched externally.

Median/p95/min/max/MAD aggregate **per-process p50 values**. They are not pooled
frame/input event percentiles. In particular, ten launches give a nearest-rank
empirical p95 equal to the maximum, not a reliable tail-latency estimate. NUS_PERF
exports bounded in-process summaries, not every raw event timestamp; the JSON
preserves those summaries without pretending to contain unsampled event data.
The harness refuses fewer than five measured launches.

Subprocess wall time is deliberately not substituted for the app's semantic
milestone. The timeout is only a failure boundary. Closing another nus instance,
clicking through a dialog, or reusing a different native executable mid-run must
not be part of collecting a valid baseline.

RSS tree totals are an upper estimate because shared resident pages can appear in more than one process. The output says so; do not present tree RSS as private physical memory.

## Continuous benchmarking

`.github/workflows/benchmark.yml` is separate from correctness CI and release packaging. It checks out recursive submodules, builds only the root-workspace benchmarks, and does not fetch or package CEF. CodSpeed results are initially informational: establish several stable baselines and verify an intentional temporary slowdown is detected before making performance a merge gate.

Before sampling, the benchmark workflow runs Python harness tests, the ordinary
VT fixture/scroll regression tests and Criterion's test mode for all four bench
executables. Compilation alone would not catch the original scroll panic.

```sh
python3 -m unittest discover -s scripts/tests -p 'test_perf_native.py' -v
cargo test --locked -p nus-vt --test perf_fixtures
cargo bench --locked -p nus-vt --bench terminal -- --test
cargo bench --locked -p nus-render --bench render -- --test
cargo bench --locked -p nus-pty --bench hold -- --test
cargo bench --locked -p nus-sync --bench sync -- --test
```

The Python subprocess tests use a fake executable and synthetic records. Passing
them establishes harness behavior, not nus performance or macOS dialog behavior.
The VT tests call the same scroll adapter as the benchmark, including 1-row
windows, multiple scroll counts, retained-history bounds and chunked fixture
state equivalence.

A useful sensitivity check is a temporary PR that performs an extra pass over every VT input byte or shapes each render line twice. Verify that CodSpeed identifies the regression, then close/revert the PR without merging it.

## Adding a benchmark

Before adding one, answer all four questions:

1. What operation is measured?
2. What representative input does it use?
3. What setup is explicitly excluded from the timed region?
4. Which user-visible regression would this catch?

If the fourth answer is unclear, profiling or a unit test is probably a better tool.

## Regression triage

1. Confirm the CodSpeed comparison or reproduce locally with Criterion.
2. Check whether the changed benchmark is deterministic and semantically unchanged.
3. Use CodSpeed's differential profile or a local profiler to locate the hot path.
4. Compare the corresponding native metric when the regression can affect user experience.
5. Optimize or document the intentional tradeoff; do not tune the fixture to hide the change.

## Non-goals

The deterministic suite does not benchmark hosted-runner GPU time, live websites, arbitrary shell startup, host font discovery, or sleeps. Native measurements may cover those areas only when there is a semantic readiness boundary and a controlled environment.


### Lightweight native path

CEF initializes on first browser/request-context use. On macOS the framework
itself is also loaded lazily; the initialization metric includes that cost. The event loop uses CEF's
requested pump deadlines and worker completion wakes; idle maintenance waits at
most 50 ms, while actively animating scenes retain the existing 2 ms pacing.
Visible browsers also impose their own external BeginFrame deadline. An
unchanged previous paint must not let native idle maintenance delay the next
browser animation frame. CEF's message-pump requests alone do not provide this
external frame clock.
The benchmark harness uses this production scheduling path too.

Eligible sleeping tabs close their CEF browser and release imported paint
textures; existing sidebar/replay previews are retained. Wake recreates the browser in the same container and restores scroll.
The configured sleep timeout still applies. Pinned/current tabs, forms or edits,
media/capture, permission prompts, readers, agent requests, open DevTools and
Back/Forward history prevent disposal. A currently open PiP/little window also
prevents this pass from disposing tabs.

Shaped text shares immutable glyph arrays and uses bounded second-chance
replacement (1,024 runs / 8,192 glyphs). Width measurements use 4,096 entries.
Cache pressure evicts cold entries instead of clearing all hot text.
Unchanged browser media/scroll reports are deduplicated before crossing IPC.
The edited-document suspension guard reports once and removes its input
listener. Subsequent typing or bulk form updates must not rescan the document
for media when the sticky protection state is already set.
Journal updates authenticate existing ciphertext once and replace it atomically,
avoiding the former second decryption without changing the encrypted format.

After Chromium shuts down, disposable HTTP/code/GPU caches across the profile
and container profiles share a 256 MiB retention budget. Oldest cache stores are
removed whole until under budget. This is an exit-time budget, not a hard live
limit or a cap on the entire profile. Cookies, IndexedDB, service-worker data,
projects, downloads and recovery generations are excluded. Active replay bytes
now count toward the existing 128 MiB retained replay budget; live recordings
are never deleted and can exceed it while their per-window limit applies.

The Dock uses 84 compressed construction frames, decodes one at a time, and
settles on static Mercury artwork with its timer invalidated. The claim scene
and Settings preview keep their GPU material motion.

`python3 scripts/check-lightweight.py --app /path/to/nus.app --out /new/evidence`
checks native suspension, cookie/scroll restoration, guarded pages and repeated
wake-up. `--mode idle` and `--mode mercury-idle` measure ten-second loop activity
and RSS. These are UI loop turns, not kernel wakeups; process-tree RSS counts
shared pages more than once. `scripts/check-mercury.py` verifies the claim scene
still flows while the Dock construction resolves to identical still artwork.
