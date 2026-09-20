# Performance budgets and measurement boundaries

The reference class is a four-core machine with 8 GB RAM, integrated graphics
and an SSD on each supported OS. This is a hardware class, not a claim that
three machines of that class have been tested. The September 20 measurements
available here come from an Apple M4 Pro with 14 CPU cores and 48 GiB RAM.
They cannot certify the lower-spec targets or Windows/Linux runtime behavior.

## Budgets

| Area | Target and scope |
| --- | --- |
| Distribution | Minimize installed release size without removing required browser resources, languages, media support or signing |
| Idle tabs | Under 150 MB additional memory for a simple idle browser tab; report the whole process tree separately |
| Additional windows | Under 25 MB per empty additional main window; report window size, scale and memory method |
| Projects | Linear retained state and bounded shared work; measure representative projects and language servers separately from blank tabs |
| File opening | Under 100 ms from open request to first content-frame submission for a 10 MiB text file |
| Large files | Load, search and highlight 100 MiB or larger documents outside the input thread |
| Syntax | Under 2 ms per screen view, with language, viewport and context size declared |
| Background search | No missed display frames while typing; CPU frame-submit measurements alone cannot certify this |
| Startup | First useful interactive frame before the first Dock bounce completes; use OS launch and presentation timestamps |
| Git checkout | Under 50 ms from a completed checkout to synchronized editor/tree state |
| Diagnostics | Under 15 ms for internal diagnostics; measure generation and presentation, not only coordinate conversion |
| Extensions | Measure serialization CPU and bytes per request before making a near-zero overhead claim |
| Key to screen | Verify the complete current app through display presentation; the historical 2.8 ms spike is not that measurement |

## Implemented changes

- The app has its own release profile because it is a separate Cargo workspace.
  Optimized code and thin LTO remain enabled; shipping debug information and
  symbols are removed. The bundle script also supports an isolated output path.
- Main windows share a GPU device, immutable pipelines and a system font
  database. Font bytes are shared, fallback fonts are selected lazily, and
  immutable macOS system fonts can be mapped without copying the complete
  Apple Color Emoji collection. Weak caches release their data with the last
  owner. Each window retains its own glyph atlas and drawing buffers.
- Window registries use one shared snapshot, replacing a separate copy of all
  windows in every window. Comparison and retained registry storage are linear.
- The event loop waits for input or its next service deadline instead of
  spinning. A focused Home cursor redraws on blink changes rather than on every
  frame. Background animation respects reduced motion.
- A process-wide pool of two workers with a 16-job queue handles cancellable
  file loading, search, highlighting and directory listing. A full queue never
  waits on the input thread. Stale work is cancelled and stale answers ignored.
- Text files load directly into ropes without a second full-file string.
  Rendering handles visible columns even for a single 100 MiB line. Searches
  use a streaming linear algorithm and cap retained matches at 10,000; the UI
  identifies truncated results and refuses a partial Replace All.
- Editor changes use revisions, not whole-file hashes. Language-server snapshots
  are cheap rope clones; text conversion, minimal change calculation and JSON
  serialization happen on the writer thread. Incremental servers receive changed
  ranges; full-sync servers retain their protocol behavior.
- Diagnostic positions use rope UTF-16 indexing. Unchanged documents do not
  resend on caret movement. Existing resource retention limits are documented
  in [RESOURCE_BUDGETS.md](RESOURCE_BUDGETS.md).

## Reproduce

Build a release app with `NUS_BUNDLE_OUT=/tmp/nus-performance/nus.app
bash scripts/bundle-mac.sh`, then run
`python3 scripts/check-performance.py /tmp/nus-performance/nus.app` on macOS.
The native harness uses disposable profiles and local fixtures, verifies editing
and Undo, and saves a `performance.json` report with hardware details. It removes
its large generated documents even when a check fails. Filesystem caches are not
controlled. Do not call these disk-cold measurements.

The opt-in `NUS_PERF=1` counters retain at most 2,048 recent samples per category
in memory. They do not write recurring logs. Native test steps explicitly export
their results. `main_to_first_submit` begins at Rust main entry;
`file_open_submit` ends after the loaded content's GPU submission;
`frame_build_submit` includes swapchain acquisition/present-call waiting.
None measures display scanout or photons. RSS process-tree sums count shared
pages in each process and are not private physical footprint.

The ignored release test `editor_work::tests::release_measurements` separately
reports file-to-rope loading, a warm Rust grammar on 16 KiB of source, and UTF-16
diagnostic position mapping. These are components, not end-to-end app results.

## September 20 results

The [raw report](performance/2026-09-20-m4-pro.json) records the release app at
1,100 × 800 logical pixels, 2× display scale, reduced motion, and a static Home
background. All cases in the reported run reached an explicit completion marker.

| Measurement | Result | Interpretation |
| --- | --- | --- |
| Installed macOS bundle | 355.2 MiB versus 429.3 MiB existing bundle | 17.3% smaller; main executable 33.7 MiB; required CEF resources retained |
| 10 MiB plaintext, open to content submission | p50 16.4 ms; p95 28.9 ms; max 30.1 ms, 20 opens | Meets the 100 ms target on this machine, with warm/uncontrolled filesystem caches |
| 100 MiB plaintext, open to content submission | p50 58.9 ms; max 63.0 ms, five opens | Load and ordinary edit/Undo passed; does not certify every operation on large files |
| 100 MiB single-line document | End and typing passed; longest of five sampled frame submissions 13.0 ms | Visible-column rendering avoids shaping the whole line |
| Typing during 100 MiB background search | 123 submitted frames; p95 8.43 ms; max 8.66 ms; none above 16.67 ms | CPU submission evidence, not proof of zero display-frame drops |
| Rust highlighting, warm 16 KiB context | p95 2.72 ms | Does **not** meet the 2 ms syntax target |
| 100 diagnostic coordinate conversions | p95 0.027 ms | Mapping only; does not measure diagnostic generation or LSP response time |
| Rust main entry to first submission | 309–346 ms, three processes | Not OS launch-to-photon latency, true cold launch, or a measured Dock-bounce comparison |
| Idle Home, whole process tree | 382–388 MiB RSS sum | Does **not** meet a 150 MB whole-app budget |
| First idle browser tab | About 208 MiB added to process-tree RSS | Includes browser initialization; above 150 MB |
| Further idle browser tabs, through eight | About 101–102 MiB per added tab | Approximately linear in this fixture, below 150 MB per further tab |
| Additional empty windows, through four | 1.5–19.0 MiB per step in parent RSS | Under 25 MB in this run; GPU allocations/private physical footprint require separate profiling |

Closing all eight browser tabs returned the live CEF browser count to zero.
Process-tree RSS remained about 124 MiB above the pre-browser baseline after
800 ms, including a retained helper and browser caches. Browser closure alone
does not imply immediate return of every allocated page to the OS.

Earlier runs under active compilation and only about 1 GiB of free disk space
were substantially slower: 1.8–2.3 seconds to first submission, with a 10 MiB
load exceeding the test's 250 ms readiness deadline. Completed build artifacts
were removed before retrying. This variability is part of the evidence; the
successful run is not a guarantee under host resource pressure. An intermediate
run exited before its 10 MiB script finished, so the harness now rejects early
successful process exits rather than accepting an incomplete measurement.

## Regression verification

- The release app test suite passed: 154 tests, with the timing probe intentionally
  ignored by the normal suite. TLS integration tests require local socket access.
- The root workspace suite passed, including font ownership/cache checks, rope
  change correctness, the fake language-server lifecycle, and real PTY output
  backpressure without loss.
- Native macOS menus, focused zoom, Hatch and 24 browser open/close cycles passed
  using the optimized release artifact. A visual check covered the narrow dark
  menu and a 100 MiB single-line editor at its final column.
- Native PiP/replay checks passed in light/dark and narrow layouts, including
  two main windows sharing the GPU and exported session history.
- The changed render, language-server and PTY crates compiled for Windows and
  Linux. This is not a native GUI or full application check on those platforms.
- The isolated app passed ad-hoc code-signing verification with deep and strict
  checks. No notarization or distribution-signing claim follows from that check.

## Remaining boundaries

The current large-file highlighter uses bounded surrounding context rather than
a persistent incremental parse tree. Grammar state that begins outside that
context can be incomplete. Language servers are explicitly paused above 8 MiB;
the editor identifies that state. Save, whole-document replacement, large
selection copy and some bulk editing operations still need asynchronous paths.

Directory refresh is asynchronous but currently polled at 500 ms while visible.
That does not meet the 50 ms checkout synchronization target or cover external
buffer reload. Extension serialization, full language-server diagnostic latency,
physical key-to-screen latency, display frame misses, true disk-cold launch,
multi-project scaling and lower-spec Windows/Linux runs remain unverified.
