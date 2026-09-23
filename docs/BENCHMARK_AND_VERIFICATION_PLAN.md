# NUS benchmarking and verification plan

Status: proposed implementation plan, September 22, 2026. No new benchmark or proof results are claimed by this document. The accompanying visualization concepts use illustrative data only.

## Objective

Establish how effectively NUS supports a complete working session: correct completion of real tasks, responsiveness while terminal/browser/editor workloads overlap, continuity across interruptions, and the resources required. Establish selected production invariants with Verus. Publish reproducible evidence, including limitations, failures, and unfavorable results.

The unit of comparison is an equivalent completed task and its whole participating environment. Individual terminal, browser, and editor tests explain the result. There is no single weighted NUS score.

## Evidence contract

Maintain three independent forms of evidence:

1. **Proved:** a named executable component satisfies a written specification under listed assumptions. A proof badge never attaches to an entire workflow by association.
2. **Tested:** a declared application configuration passed concrete integration, correctness, or fault-injection checks. State precisely what happened and what was checked.
3. **Measured:** an observable quantity was sampled with declared boundaries, hardware, instrumentation, uncertainty, and failure accounting.

Product claims link to all applicable evidence. Unknown, unsupported, failed, and not measured remain distinct states. A successful proof does not tighten a statistical confidence interval, and fast measurements do not establish correctness outside the tested cases.

## Starting point and constraints

The repository already has Criterion/CodSpeed component suites, native NUS_PERF instrumentation, scripts/perf-native.py with per-run provenance, and historical native reports. Extend these rather than create a second unrelated measurement system.

Current native timing boundaries include application submission/present-call return rather than physical scanout. The bounded collector retains recent samples while some counters cover the lifetime of the measurement epoch. The native harness is Python; verifying a Rust collector would not verify the Python runner, report transform, OS clocks, or milestone placement.

Preserve existing v1/v2 metric boundaries. Changes to units, meaning, timing boundaries, population, or lifecycle create a new metric version. Do not combine historical series across that change or describe a measurement repair as a speedup.

Implementation must preserve unrelated local changes. Record the actual binary hash and build manifest; a checkout revision alone is insufficient, especially with a dirty checkout or an existing packaged bundle. Do not install competitors, alter personal profiles, or begin long hardware experiments as part of the planning phase.

## Phase 0 — freeze the questions and protocol

Deliverables: versioned scenario specification, metric dictionary, fixture manifest, hardware matrix, claim register, and comparison configurations.

For every metric define start event, end event, clock domain, unit, population, sampling policy, aggregation, failure behavior, and allowable claim. Define correctness oracles before collecting comparative timings.

For every scenario define input files, process workload, visible panes, browser viewport, shell configuration, completion condition, timeout, cleanup, and expected retained state. Provide a short human-readable task independent of product-specific gestures.

Choose three primary launch-study workflows: edit and verify, resolve a port conflict, switch projects under load. Add restore/recovery and a composite session next. Predeclare primary endpoints; treat other findings as exploratory. Choose practical regression tolerances from a pilot and freeze them before confirmatory runs.

Gate: another person can tell exactly what success means and reproduce setup without asking the author which UI path to take.

## Phase 1 — make the measurement pipeline auditable

Extend the native harness with a shared result schema and product adapters. Use NUS_SHOT/internal hooks for semantic assertions and diagnostics; comparative end-to-end timings must use equivalent external initiation and observable completion boundaries. An internal shortcut available only in NUS is not a fair simulated-human action.

Separate three execution modes: deterministic engine measurement, scripted interaction, and human task study. Publish these modes explicitly; do not present scripted completion time as human productivity.

Use controlled local fixtures for primary browser/workspace tests. Pin fixture hashes, dependencies, content, and service behavior. Record actual completion of builds and page operations to detect accidental workload reduction. Live-site tests can be supplemental and must not feed a reproducible primary comparison.

Collect bounded raw event traces or explicitly declared sampling/histograms for interactions. Record buffer overflow and dropped-event counts. Preserve independent run IDs; do not pool thousands of correlated frames into thousands of independent experimental repetitions. Measure instrumentation overhead in paired enabled/disabled runs. Capture trace/video-heavy diagnostics separately if their overhead materially changes the workload.

Record:

- OS, hardware, RAM, GPU, display refresh, resolution, scale, power mode, AC/battery, thermal state, background load, and available disk space.
- Application/build/engine versions, binary hashes, launch arguments, profile and fixture IDs, run order, random seed, and full enabled feature configuration.
- Process membership over time, including helpers and processes that detach or reparent. Account for the whole application set and report servers/LSPs/workloads separately.
- Warmups, actual repetitions, timeout/crash/incorrect-output cases, exclusions with reasons, and partial artifacts.
- Timeline clock alignment and uncertainty. Never subtract timestamps from different clock domains without an established mapping.

Startup has separate first-launch onboarding, normal warm launch, workspace restore, and controlled cold-start protocols. A fresh profile is not a cold OS cache. Use an onboarded profile for ordinary launch testing. Do not claim cold results without a documented cache/reset procedure.

Gate: missing completion, duplicate metrics, buffer overflow, changed binaries, deliberate wrong output, and interrupted runs are visible and cannot silently produce a valid result. A deliberately injected slowdown must be detected. Public exports exclude personal paths, shell output, tokens, and private browsing data through an explicit allowlist while retaining private originals.

## Phase 2 — Verus production pilot

Start with the PTY retention ring in crates/pty/src/hold.rs. It is small, directly connected to continuity, and has existing callers and benchmarks.

Specification:

    retained_after = suffix(retained_before ++ appended_bytes, capacity)
    length(retained_after) <= capacity

Specify zero capacity, empty writes, writes larger than capacity, integer arithmetic, allocation failure assumptions, and repeated append behavior. Distinguish logical retained byte length from allocator capacity or whole-process memory. A ring proof does not prove lossless delivery over the entire PTY/OS/socket path.

Use a minimal production module/crate boundary compatible with the existing build. Pin Verus, its Rust toolchain, and dependencies. Verify that the annotated executable implementation is the one linked into NUS. Do not keep an independently maintained proof-only algorithm and assume it represents production.

Make unverified callers satisfy preconditions through ordinary Rust types, private constructors, and executable validation. Do not expose a safe-looking public function whose safety relies on unchecked caller proof obligations. Review the specification and external interface independently when review resources are available.

Maintain an assumption register: verifier/solver/compiler, standard-library specifications, allocator behavior, wrappers, external_body, external function specifications, assumes, and relevant unsafe/FFI boundaries. No new unreviewed assumptions in the verified core. Zero local assumes is not the same as zero trusted dependencies.

CI runs proof checking, production compilation, ordinary tests, property/differential tests, and relevant benchmarks. A proof-breaking implementation mutation must fail verification. Test specification non-vacuity: valid instances exist, public entry preconditions are reachable, and deliberate incorrect suffix behavior is rejected.

Gate: actual production ring verified, callers preserve the contract, proof-breaking mutation rejected, runtime behavior agrees with a simple oracle, and any performance change is measured. If integration costs are disproportionate, preserve the small pilot and prioritize application measurement rather than spreading verification indiscriminately.

## Phase 3 — extend proofs by product value

| Order | Component | Contract | Boundary |
| --- | --- | --- | --- |
| 1 | Incremental holder frame parser | Valid streams yield identical ordered frames under arbitrary chunking; incomplete suffix preserved; checked length arithmetic; specified oversize/truncation behavior | Does not prove socket reliability, authentication, or UTF-8/VT semantics |
| 2 | Editor asynchronous-result acceptance | Apply only to the intended live document and matching revision/generation; stale results have no effect | Actual worker scheduling and OS I/O remain integration concerns |
| 3 | Pane/workspace routing | Bind queued actions to stable identity/generation; focus changes and ID reuse cannot retarget them | Every dispatch route must use the verified decision path |
| 4 | Assistant dispatch authorization | Current target, origin, permission and cancellation state authorize dispatch at a specified linearization point | Already-issued browser effects cannot be assumed retractable; navigation/dispatch races require integration design |
| 5 | Measurement accumulator/state machine | Exact counts, defined retention window, percentile ranks, epoch isolation, unique milestones, checked arithmetic | Clock fidelity, event placement and Python reporting are separate |
| 6 | Selected grid/scroll operations | Valid geometry, retained-history bounds, mapping consistency, and explicitly specified character behavior | Do not equate a local bounds proof with complete terminal conformance |

Write each contract before changing its implementation. Prefer proofs of executable transitions. If using a state-machine model, prove/refine the relationship to executable transitions; tests alone do not turn model invariants into production theorems. Liveness claims need explicit scheduling/fairness assumptions. Proving stale results cannot apply does not prove fresh results eventually arrive.

The parser spec must define length limits consistently across blocking and incremental paths; that is a design decision to resolve before claiming equivalent behavior. Preserve replay's intentional bounded history semantics. Terminal byte retention alone does not establish that an arbitrary truncated stream reconstructs complete historical screen state.

Gate per component: reviewed contract, production linkage, assumptions, proof run tied to source/toolchain hashes, mutation check, boundary tests, and performance check. No whole-application verification claim.

## Phase 4 — benchmark the unified environment

| Scenario | Workload | Correctness oracle | Primary measurements |
| --- | --- | --- | --- |
| Start and restore | Saved project, shells, editor, docs, local preview | Declared layout/files/URLs/cwd restored; separately identify live-process survival | OS launch to usable state, peak and settled resources |
| Edit and verify | Start fixture server, navigate, locate seeded defect, edit, reload, exercise page | Known test passes; correct project and page revision | Correct completion time, errors, manual handoffs in human study |
| Port conflict | Two fixture projects contend for one port | Intended owner identified; conflict resolved; unrelated service survives | Completion time, wrong-target actions, interruptions |
| Project switch | Two or more busy workspaces, interrupted task | Correct target/revision/cwd; required state retained | Resume time, wrong-context actions, switch latency |
| Mixed workload | Fixed build/output plus typing, browser interaction, search and LSP activity | Output integrity, completed workload, expected typed text and page state | Interaction tails, visible stalls, throughput, resources |
| Recover and continue | App restart and isolated renderer fault, tested separately | Expected processes survive; required state restored; unaffected pane remains usable | Recovery time, data/state loss, blast radius |

Composite session: versioned 15–20 minute script combining representative phases. Record phase transitions, a screen recording in diagnostic runs, and resources on a shared time axis. Do not use a narrated promotional recording as the timing source.

Mixed-load matrix: terminal alone; terminal plus idle page; active browser; build/output; editor search/LSP; multiple workspaces. Add stress levels beyond ordinary use but identify them as stress tests. Hold offered load constant and verify completed work/backlog so dropping, suppressing, or deferring output cannot masquerade as speed.

Scale at 1, 4, 8, and 16 workspaces with identical declared contents. Separate visible, occluded, inactive, minimized, and sleeping behavior. Report retained state and scheduling policies. Add repeated open/close cycles and multi-hour soak tests; distinguish bounded caches/plateaus from sustained growth.

Gate: all primary scenarios have machine-checkable outcomes, correct failed-run accounting, and pilot evidence of reproducibility. Faults are injected only into disposable test instances, never personal sessions.

## Phase 5 — compare fairly and measure uncertainty

First platform: one fixed Mac, comparing NUS, cmux, VS Code with integrated terminal/browser, and Ghostty + Chrome + a declared editor. Verify actual released versions and capabilities when executing; record them. Use Warp only for matching terminal/assistant scenarios. Unsupported tasks are not zero scores; use an explicitly labeled composite configuration or report the task unsupported.

Allow efficient normal workflows in each product. Get configurations/task paths reviewed by experienced users where possible. Provide shipping-default and matched-feature tracks, keeping them separate. Match content, viewport, terminal geometry, scrollback, font size, output delivery, shell configuration, and work completed. Disclose different engines, extensions, LSP modes, sleeping policies, accessibility or feature limitations. A product comparison includes those differences; attributing causality to integration requires a controlled experiment.

Report total participating environment resources and components. Use appropriate platform footprint/private/PSS counters with clear definitions; RSS sums double-count shared pages. Report GPU resources separately where available. Do not merge unlike memory measures across OSes. Energy requires a declared physical or platform measurement method, idle baseline, fixed screen brightness/radio state, and uncertainty; CPU time alone is not energy. Treat battery-runtime experiments as separate long runs.

Pilot for noise and costs. A starting point is 10 exploratory independent runs, followed by approximately 30 measured launches or sessions per primary configuration; revise the confirmatory sample size from pilot precision/power before freezing the protocol. Collect thousands of interaction events across independent sessions where feasible, but publish p99 only with adequate tail support and uncertainty. Thirty session repetitions alone do not establish an accurate p99.

Randomize/interleave product order in blocks, with controlled warmups and thermal recovery. Report medians and distributions, paired effects and confidence intervals at the independent run/session level. Use bootstrap or another justified estimator with dependence respected. For human trials, the participant is the unit of independence. Report completion/failure rates with appropriate uncertainty and successful-run timings separately; timeouts are censored outcomes, not silently discarded observations. Publish all prespecified endpoints and relevant negative findings; account for multiple comparisons where making inferential claims.

For input-to-photon validation, use calibrated external input generation and a photodiode or high-speed camera. Declare display position, scanout/refresh, input device/polling, camera resolution and synchronization uncertainty. Use OS/compositor traces as a scalable complement, not an automatic substitute for photons. Keep 60 Hz and 120 Hz results separate.

Hardware expansion: representative lower-spec integrated-GPU machine, then native Windows and Linux. Report hardware/OS cells independently. Preserve Wayland/X11 distinction where tested. Do not certify a hardware class from one high-end Mac.

Gate: independently repeatable results, no unexplained instrumentation bias, stable artifact provenance, adequate uncertainty reporting, and published failure accounting.

## Phase 6 — productivity and assistant studies

Run a small usability pilot first. Then conduct a counterbalanced within-participant study with equivalent task variants, proficiency assessment, training, and predeclared completion oracles. Size the confirmatory study from pilot variability and the minimum practically meaningful effect. Record successful completion, errors/recovery, resume-after-interruption time, and perceived workload. Application switches/copy-paste counts are explanatory variables, not productivity scores. Consent and minimal recording of participant data are required for the study.

Separate assistant layers:

1. Deterministic scripted tool/context requests: latency, payloads, correct context, authorization and cancellation.
2. Live-model trials: fixed model/version where possible, prompts, budgets, permissions, task set, and repeated runs; success, cost and completion time.

If models/tool capabilities cannot be matched, publish an end-to-end product comparison with those differences rather than attribute results solely to NUS. Never treat recorded model output as a live reasoning measurement.

Gate: productivity claims come from the human study; assistant claims carry success/cost and model configuration, with uncertainty and unsuccessful trials retained.

## Phase 7 — publication and continuous protection

Versioned public bundle: benchmark source, fixture generator and hashes, scenario definitions, sanitized raw results, analysis code, build manifests, proof sources/toolchain, assumption register, selected recordings, and a report generated from those files. Keep archival reports immutable; correction notices link old and new versions. Provide an independent reproduction recipe and invite replication without making replication a prerequisite for all internal progress.

CI layers:

- Per change: correctness, Verus for covered components, schema/analysis tests, deterministic component benchmarks.
- Dedicated reference-machine cadence: native primary scenarios and resource checks; select cadence based on runtime and hardware availability.
- Release candidate: broader workload matrix, recovery, soak, hardware coverage, signed/packaged artifact measurements, and publication export validation.

Initially keep performance checks informational. Establish baseline variance and demonstrate sensitivity to a known slowdown before enforcing tolerances. Gate correctness/proof failures immediately once adopted; calibrate performance gates to practical effects and noise. Each native baseline is tied to a specific hardware/software configuration.

## Visualization directions

All three share one result schema and evidence links. The preview data is synthetic and uses an unnamed reference rather than invented competitor rankings. Proof rows in previews are planned, not verified.

1. **Broadsheet report:** recommended public entry. NUS typography, rules and one signal color; common-scale comparisons with exact values, intervals, sample counts, and visible method disclosure. Expandable evidence per result. Best for fast comprehension and sharing.
2. **Session timeline:** recommended deep view. Aligned workload lanes and measured traces, synchronized phase cursor, active-workspace state, and selected-event detail. Distinguish illustrative activity from recorded telemetry. Best for explaining why latency/resources change during a whole session.
3. **Evidence ledger:** recommended technical appendix. Metric/invariant rows, explicit measured/tested/proved/planned states, source and artifact links, assumptions, and reproducibility information. Best for audits and engineers investigating a claim.

No composite score, selective truncation, unlabeled logarithmic axis, proof-confidence percentage, or cherry-picked winning run. Show units, direction, sample sizes, uncertainty definitions, configuration, failures and missingness. Use identical scales for like measures; distinct scales for unlike units. Proof coverage uses named obligations and boundaries, not a misleading percent of application correctness.

Final visualization implementation must support keyboard/touch, light/dark, narrow screens, accessible value tables, static export, offline inspection, and linked raw data. Validate charts against the result schema, including empty/failed/partial runs and extreme values. Raw evidence must stay available when a chart cannot render.

## Delivery order and first milestone

1. Freeze scenario/metric/claim contracts and the shared result schema.
2. Strengthen runner provenance, event tracing, correctness oracles and failure accounting.
3. Complete the production ring proof pilot and its CI gate.
4. Implement the three primary workflows and mixed-load measurements; collect NUS-only pilot data.
5. Add first-platform comparison adapters and counterbalanced runs.
6. Publish Broadsheet report with session/evidence drill-downs in the chosen design.
7. Expand proofs, recovery/soak tests, hardware and human studies as independent workstreams with the gates above.

The first reviewable release is a complete, reproducible slice: one linked production proof, three correct native workflows, traceable result files, and a report honestly distinguishing measured, tested, proved, failed and unknown. It need not wait for whole-application verification, every competitor, or a human study. Broader claims wait for the corresponding evidence.

## References

- Existing repository contracts: PERFORMANCE.md, PERFORMANCE_BUDGETS.md, RESOURCE_BUDGETS.md, ARCHITECTURE.md, DESIGN.md.
- Verus: https://github.com/verus-lang/verus
- Assumptions and trusted components: https://verus-lang.github.io/verus/guide/tcb.html
- Safe API guidance: https://github.com/verus-lang/verus/blob/main/CONTRIBUTING.md
- cmux: https://www.cmux.dev/
- VS Code integrated browser: https://code.visualstudio.com/docs/debugtest/integrated-browser
- Warp: https://docs.warp.dev/
- Speedometer: https://www.browserbench.org/Speedometer3.1/
- MotionMark: https://browserbench.org/MotionMark/about.html
