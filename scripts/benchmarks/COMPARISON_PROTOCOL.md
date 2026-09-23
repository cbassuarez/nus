# Comparison protocol v1

Scope chosen with the user: complete VS Code + browser + terminal stacks,
using Ghostty, Kitty and WezTerm; Zen and Arc are the browser candidates.
NUS-only measurements retain their original endpoints. App-internal times
are never ranked against external-driver elapsed times.

## Matched comparisons

1. Installed application bytes: identical filesystem accounting for NUS and
   the union of the editor/browser/terminal bundles. Download size stays a
   distinct measure. Sizes are deterministic observations, not sampling studies.
2. Prepared-workspace RSS: a local text file, a local page, and a running local
   shell/service in both NUS and each stack. Sum the union of descendant PIDs
   exactly once. Report process count and shared-page double-counting limits.
3. Terminal output: identical byte stream, terminal geometry and protocol
   completion oracle for NUS, Ghostty, Kitty and WezTerm. Completion of parsing
   does not claim physical display completion.
4. Browser responsiveness: prefer the same pinned Speedometer workload across
   engines; disclose versions, launch flags, runs and all failures. Browser
   engine scores do not rank the whole unified workspace.

Use dedicated profiles/configuration and local fixtures for the primary experiment.
No user documents, installed extensions, saved tabs or existing application
processes enter those samples. The explicitly labeled Arc follow-up below uses
a retained test-account session and does not qualify as a matched-isolation result.
Each measured case must pass a semantic correctness check. Preserve failed
attempts; do not interpret unsupported automation or a failed oracle as zero.

Use balanced/cyclic application order and at least five independent trials per
runtime case. Record hardware, OS, versions, executable identity, configuration,
fixture and harness hashes. Warmup is explicitly excluded. Publication must
retain observations and report dispersion and uncertainty. No causal speedup
claim follows from historical, uncontrolled or unmatched runs.

A human task study is separate, with counterbalanced task order, experienced
participants, completion/error criteria and consent. These scripted results
cannot support claims about human time saved or preference.

References: https://browserbench.org/announcements/speedometer3/;
https://code.visualstudio.com/docs/configure/command-line;
https://ghostty.org/docs/config;
https://sw.kovidgoyal.net/kitty/invocation/;
https://wezterm.org/cli/start.html.

## Reproduce this local pilot

Use an otherwise idle Apple-silicon Mac with the listed app versions. Run one
experiment at a time. The collectors create disposable application profiles;
Ghostty uses LaunchServices and default configuration, with only its probe-owned
new process sampled. No existing Arc session is sampled. VS Code uses a minimal
local development extension to confirm the fixture is loaded; all configurations
include the same idle Python terminal helper.

```sh
python3 scripts/benchmarks/measure-memory.py --app /path/to/nus.app --runs 5 --out target/perf/home-memory-v1.json
python3 scripts/perf-native.py --help
python3 scripts/benchmarks/measure-workspace.py --nus /path/to/nus.app --apps /path/to/app-directory --runs 5 --out target/perf/workspace-rss-v2.json
git clone --branch release/3.1 https://github.com/WebKit/Speedometer.git /tmp/Speedometer
git -C /tmp/Speedometer checkout 1386415be8fef2f6b6bbdbe1828872471c5d802a
python3 scripts/benchmarks/measure-browser.py --source /tmp/Speedometer --nus /path/to/nus.app --zen /path/to/Zen.app --runs 5 --out target/perf/browser-speedometer-v2.json
```

The Speedometer test iframe is the unmodified default 800×600 viewport. Native
outer window content dimensions differ and are retained per trial. Each reported
score includes ten internal iterations; the five app launches are the independent
sampling units. First-run behavior is retained, not selectively discarded. There
is no finite distribution-free 95% population-median interval with five trials;
we show every trial and observed range, without claiming a precision bound.

Workspace RSS uses five one-second snapshots after readiness and three seconds
settling. Readiness requires a local browser load beacon, exact editor fixture
length and prefix, and a live terminal helper. Each dot is the median of those
five snapshots. Five independent profiles are run per app combination in rotating
order. These are prepared idle workspaces, not sustained development loads.

Homepage file results retain the original p95/maximum endpoints: 20 independent
10 MiB opens and five independent 100 MiB opens, with one excluded warmup each.
Memory increments are grouped by five profiles; within-profile increments are
correlated. Tab results exclude first-tab initialization. Empty-window results
are parent RSS, not the whole process tree. Historical results used different
builds and launch protocols, so do not infer a controlled before/after speedup.

Published package sizes use authenticated preview.7 archives. Runtime and stack
bundle measurements use the named local binaries. Logical bundle bytes exclude
symlinks and deduplicate inodes; APFS allocated size and archive size are distinct.
The matching accounting is used for every stack member. Universal-binary overhead
is retained as distributed. App executable hashes and package SHA-256 hashes are
in the evidence; versions are not a substitute for exact binary identity.

## Calibration failures retained

- Browser v1: two completed scores, then a process-group cleanup permission error.
  Not published as a completed comparison. v2 uses direct owned-process cleanup.
- Workspace v1: NUS completed; Ghostty never ran the terminal helper when launched
  directly. Not published as a completed comparison. v2 launches Ghostty through
  macOS LaunchServices and attributes its process through the unique helper PID.
- Arc isolation preflight: the requested disposable data directory was ignored.
  No result from that preflight was published. The later user-prepared test-account
  session is documented below as a separate, retained-session follow-up. Signing
  out alone does not establish a controlled profile or cache baseline.

Failed calibration reports and logs are retained under target/perf on the test
host. Public data contains completed experiments, with failures disclosed here.
Terminal-throughput and cross-app edit/build/preview timing are planned, not
completed by the currently published size/RSS/browser experiments.


Package reproduction uses `measure-packages.py --help`: supply the saved official
GitHub release metadata, downloaded archives, the extracted published Mac bundle,
and one `--app Product=/absolute/path/App.app` for each installed application.
The collector verifies every archive digest before counting payload bytes.
`export-homepage.py --perf target/perf --site /path/to/nus-site` validates the
completed source records and generates the homepage registry used by all site
pages. It reads the Mac allocation from measured package evidence, not a manual
number. Preserve earlier output paths rather than overwriting prior experiments.

## Arc follow-up: retained test-account session

The user prepared a new account inside Arc, not a separate macOS login. The
follow-up uses that visibly empty test workspace in the existing macOS session.
Changing an Arc account does not establish a new filesystem profile: caches,
local preferences and background process state may persist. Arc remains open;
no app restart, profile deletion, account change or existing-tab closure is part
of these collectors. Only one benchmark tab is opened and reused through the UI.

`measure-arc-browser.py --source /path/to/Speedometer --arc-pid PID --runs 5
--out target/perf/browser-arc-session-v1.json` passively serves the same pinned
Speedometer suites and collects five repetitions, each containing ten internal
iterations. Navigate the test tab to each URL emitted in the adjacent control
JSON. All repetitions share the same Arc process; they are not independent
process launches. The earlier NUS/Zen figures remain a separate batch, with
fresh profiles and alternating run order. The Arc row is visibly distinguished,
with no profile-matched speedup claim or population-median confidence interval.

For whole-stack RSS, use `measure-workspace.py --nus /path/to/nus.app --apps
/path/to/apps --arc-pid PID --runs 5 --out target/perf/workspace-arc-session-v1.json`.
Open the single emitted local fixture URL in the same test tab. The collector
then creates fresh VS Code/terminal trials in rotating order while passively
including the existing Arc process tree. Arc is never in its cleanup list. The
Arc session has already run Speedometer; prior-workload and retained-cache costs
are included. All three Arc stack rows are a separate follow-up batch, not
profile-matched replications of the earlier NUS/Zen workspace experiment.

## Lightweight build regression check

The first lightweight calibration scored about 29 in Speedometer while the
frozen original still scored 42.5 in a same-host pilot. A visible-browser frame
deadline alone did not recover the score. Per-suite results localized the loss
to input-heavy TodoMVC workloads: the new edited-document suspension guard
rescanned media on every input event. Making that guard report once and detach
recovered 42.4 in the next pilot. These single runs diagnose a regression; they
do not establish a speedup over the original. Final repeated measurements use
the corrected binary and retain the calibration reports separately.

The visible browser still receives external BeginFrames independently of native
idle maintenance. No benchmark-specific browser flags, disabled security
features, workload changes or altered page viewport are used by this fix.

In the final lightweight batch, Zen trial 4 stalled and its window appeared as a
macOS thumbnail. It completed before the 300-second timeout after read-only UI
inspection, which may have changed focus. Its valid 33.57 score is retained,
without replacement. Visibility was checked at readiness and completion, not
continuously. All five completed scores per browser remain available; this batch
must not be used for a tightly controlled causal competitor-speedup claim.
