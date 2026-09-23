# Native workflow pilots and public report

The first implementation of the benchmark plan is a local, scripted pilot.
It exercises the real release application, PTYs, editor, embedded browser and
Ports board. It does not estimate human productivity or physical input latency.

## Run

Build an isolated macOS bundle (requires the project's fetched CEF):

```sh
NUS_BUNDLE_OUT=/tmp/nus-benchmarks.app scripts/bundle-mac.sh
python3 scripts/perf-native.py --app /tmp/nus-benchmarks.app \
  --scenario edit-verify --runs 10 --warmups 1 \
  --out target/perf/edit-verify-v1.json
python3 scripts/perf-native.py --app /tmp/nus-benchmarks.app \
  --scenario port-conflict --runs 10 --warmups 1 \
  --out target/perf/port-conflict-v1.json
python3 scripts/perf-native.py --app /tmp/nus-benchmarks.app \
  --scenario project-switch --runs 10 --warmups 1 \
  --out target/perf/project-switch-v1.json
```

Use a quiet machine, finish compilation first, fix power/display configuration,
and record the environment for publishable comparisons. Current pilots do not
control thermal state, background activity, refresh rate or OS caches. They
must not be described as a universal NUS baseline. No competitor results or
speedup claims are inferred from them.

Every launch uses a new onboarded profile, disposable project files and local
fixture servers launched from real NUS shells. Fixture ports are allocated at
runtime; a port-allocation race fails the run rather than being silently retried.
Servers have a token-bound shutdown endpoint, a watchdog and harness cleanup.
Python must be installed on the benchmark host. The fixture's launch cost is
part of the port-recovery interval, so keep Python versions fixed for comparisons.

## Endpoints and oracles

| Scenario | Timed interval | Required outcome |
| --- | --- | --- |
| `edit-verify` | Prepared server to page open, edit, save, and DOM confirmation | Exact saved bytes and matching browser title/body |
| `port-conflict` | Contender launch to completed recovery and browser checks | Real address conflict; Ports board associates listener PID with correct tab; old owner stops; new owner binds; unrelated service still responds |
| `project-switch` | Dirty buffer A to edit/save B, restore/save A, and browser confirmation | Both files exact; correct buffer identity after switching; background PTY output counter increases inside timed interval |

Project switching means two project buffers in one window. It does not yet
measure cross-Space restoration or a multi-window workspace. The background
fixture emits roughly 1 KiB every 10 ms; the achieved rate is scheduler-dependent.
The counter establishes that output advanced, not that every byte was rendered.

The NUS_SHOT driver dispatches one application-owned action per event-loop turn
and waits on semantic readiness. Timings include that pacing and correctness
checks. Startup is excluded from workflow intervals. The endpoints are DOM/file
and ownership checks, not display scanout or physical key injection. The shell,
HTTP, browser and editor paths are real; there are no synthetic Ports rows.

## Evidence and publication

JSON schema 2 retains each attempt, warmup, run and failure. A failure stops the
experiment and writes a partial report; logs remain beside the JSON. Never
substitute aggregate values for observations, discard slow runs, or quietly
rerun a failed measured trial into the same report. Use a new output path for
a corrected protocol and keep the old failed report as calibration history.

The exporter accepts only known boundaries, positive run accounting, finite
observations, executable identity, and (for workflows) passing oracles plus
harness/fixture hashes. It strips local paths and raw logs:

```sh
python3 scripts/export-benchmarks.py \
  target/perf/edit-verify-v1.json \
  target/perf/port-conflict-v1.json \
  target/perf/project-switch-v1.json \
  --out /path/to/nus-site/assets/benchmarks/catalog.json
```

Each chart retains all observations and starts at zero, with its own explicit
scale. The Broadsheet reports median, observed range and an exact binomial
order-statistic interval for the population median where sample size permits.
At least 95% coverage assumes independent identically distributed runs; it is
not a p95 latency bound or a guarantee across hardware. Process-tree RSS sums
shared pages and is not private memory footprint.

The public site is static HTML with native details/tables; it works without
JavaScript and through the existing Swup navigation. Build and test from
`nus-site` with `node scripts/build.mjs` and `npm test`.

Formal proof evidence is a separate source-level claim. See `VERIFICATION.md`.
The site build checks that the downloaded ring source matches the proof record;
it does not verify machine code or attach a new proof to historical binaries.
The complete research program—controlled multi-machine studies, competitor
adapters, physical latency and further invariants—is in
`BENCHMARK_AND_VERIFICATION_PLAN.md`.
