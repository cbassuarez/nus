# Resource budgets, application menus and zoom

The September 20 audit fixed browser, font and language-server lifetimes, and
added size budgets to the generated data that previously grew during ordinary
use. These are scoped limits, not a cap on the whole application or a claim
that arbitrary websites cannot consume memory.

## Ownership and memory

- Dropping or sleeping a browser pane explicitly closes its CEF browser. CEF's
  close callback is awaited before shutdown; cached request contexts are released.
- System and fallback font bytes are owned and freed with their font system.
  Width caches retain at most 4,096 short strings; glyph lookup entries are capped.
- Local and attached shells have bounded output queues: 32 chunks of at most
  64 KiB, with at most about 1 MiB drained per pass. Backpressure preserves the
  bytes while keeping a noisy shell from monopolizing the interface.
- Closed editor/prompt documents are released in their language servers. Servers
  with no remaining clients are dropped. Incoming/outgoing queues have 32 slots;
  a server that stops consuming requests is ended instead of blocking typing.
  Diagnostic lines are bounded to 8 KiB; oversized protocol frames are rejected.
- Stale hover, command-diff, generated block-page and closed-tab metadata are
  trimmed. Reopen retains the most recent 50 closed tabs. Closed replay streams
  release their file handles during maintenance.
- Browser inspection retains at most 16 replies of up to 1 MiB and 400 diagnostic
  rows with bounded text. Oversized inspection results return an explicit error.

## Disk

| Generated data | Budget |
| --- | --- |
| Recent replay per shell pane | Two 4 MiB segments |
| Replay and page stills per open window | 32 MiB, shared by its panes |
| Closed replay sessions | 128 MiB total; also expires at the selected retention age |
| Command-history file | 1 MiB; 2,000 recent lines plus at most 127 between compactions |
| All command-history files | 4 MiB; inactive files expire after 90 days |
| Journal file / all journal files | 1 MiB / 8 MiB, with configured age retention |
| Generated block/journal HTML pages | 16 MiB; up to seven days |
| Chromium HTTP disk cache | 64 MiB requested per browser profile |
| Legacy diagnostic logs | Reduced to 512 KiB after exceeding 1 MiB |

Replay rotates while writing and accounts for stills when checkpoints finish.
Other generated-data maintenance runs off the input thread at most once per
minute, so those aggregate budgets are periodic rather than instantaneous.
Active replay directories are excluded from the closed-session sweep. CEF file
logging is disabled; PowerShell Editor Services uses its supported `None` log
level. Explicit diagnostic captures remain user-managed.

Downloads, projects, shared replay exports, installed tools, cookies, passwords,
IndexedDB and other website data are **not** deleted by this cleanup. Chromium
component packages, installed tools and user-owned data are not included in the
cache budget. Never manually delete active browser-profile files to enforce it.

At the audit snapshot the existing user profile occupied approximately 128 MiB;
most was Chromium components (including DRM), rather than replay or history.
It was inspected without clearing or modifying its contents.

## Menus and zoom

macOS uses an NSApplication menu with nus, File, Edit, View, Window and Help,
including Services, Hide and the native Window/Help roles. Edit actions address
the browser's focused frame because its off-screen view is not AppKit's native
text responder. The status icon shares the process-wide event handler.
Windows attaches the existing native menu backend; Windows/Linux also expose an
in-window application menu button and F10. Linux does not add a GTK dependency.

Command on macOS, Control elsewhere, plus/minus/zero targets the focused surface.
Plus, equals and numeric-keypad variants work. Web zoom is 25–500%, persisted by
site, and reset uses the configured default. Terminal/editor zoom is 50–300% and
reset restores their configured fonts. Native page content is 75–200%; window
controls keep their size. Hatch routes keys to its own session. Shift-minus
remains available for folding output.

## Verification

- Workspace tests include a real shell producing 8 MiB while its consumer
  pauses, without lost output; font-byte release; bounded line/frame parsing;
  and a fake language server confirming document release.
- App tests cover rolling replay, total replay/still budgets, history compaction,
  active-session and user-file preservation, zoom key variants, and menu routing.
- `scripts/check-resources-menu-zoom.py` uses disposable profiles and AppKit menu
  actions, checking editing, site zoom persistence, terminals, editors, Hatch,
  narrow dark appearance and 24 browser open/close cycles. Every cycle returns
  to zero CEF browser instances. The optimized release run used 207.4 MiB of parent-process
  RSS after cycle 4 and 209.3 MiB after cycle 24; this excludes helper processes
  and is a short regression check, not a long-duration leak proof.
- Native PiP/session-map checks pass in light/dark and narrow layouts, including
  exported history and ownership across two main windows.
- The changed shell and language-server crates compile for Windows and Linux.
  Native Windows/Linux menus, compositor behavior and longer full-process
  memory/disk soak tests still require those platforms.

Release size, tab/window memory and editor timing measurements, including
unmet targets, are in [PERFORMANCE_BUDGETS.md](PERFORMANCE_BUDGETS.md).

PowerShell's accepted log levels are documented in its
[official bootstrap script](https://github.com/PowerShell/PowerShellEditorServices/blob/main/module/PowerShellEditorServices/Start-EditorServices.ps1).
The HTTP cache switch was also checked against the bundled Chromium 152 binary;
an obsolete media-cache switch was removed rather than claiming a separate limit.

## Preview 9: intermittent pressure hardening

The reported approximately 112 GB was total system usage under severe pressure,
not an attribution to nus. The cause has not been reproduced. This pass closes
specific unbounded paths and adds conservative recovery; it cannot guarantee a
maximum for arbitrary web pages, extensions, agents, or other applications.

- Terminal interception retains at most 64 KiB for ordinary incomplete control
  strings, or 16 MiB for image/clipboard control strings (plus one 16 KiB input chunk).
  Oversized strings are discarded through a terminator, with UTF-8/split-ST
  recovery. Chunked Kitty transmissions are limited to 16 MiB encoded bytes.
- PNG/raw image dimensions are checked before pixel allocation; decoded image
  retention is 64 MiB and 1,024 images per terminal, including sixel. Sixel
  operation storage and image placements are also bounded. Decode intermediates
  are temporary and can exceed the retained-image budget.
- Hyperlinks stop accepting new IDs after 16,384 entries or 4 MiB of URL text;
  existing cell IDs remain valid. Shell marks and image placements have finite
  entry budgets even when a program repeatedly updates one line.
- Each artwork VM has 32 MiB, a 250 ms load budget and 100 ms per invocation.
  Rust drawing commands have an independent 4 MiB/8,192-command budget, with
  bounded polygon input. A failing artwork reports an error and can be reloaded.
- Scene uploads occur only after successful surface acquisition. Failed surface
  draws still submit pending glyph/image writes and poll completion so temporary
  staging buffers can retire during resize/occlusion.
- Every five seconds while the event loop runs, nus samples native macOS memory
  pressure, Windows available physical memory, or Linux `MemAvailable`. On
  warning/critical pressure it clears reproducible CPU layout/icon caches,
  flushes GPU uploads, and sends Chromium a moderate/critical memory-pressure
  notification. Responses are throttled to 30 seconds except escalation.
  This path does not close tabs, clear page state, kill processes, or discard
  terminal/editor buffers. Linux metrics describe the host, not cgroup limits.
- Opt-in `NUS_SHOT memory` samples include macOS `phys_footprint` for the main
  process and child tree, alongside RSS and the current pressure level. Missing
  metrics are null, not zero. RSS can count shared pages multiple times; neither
  metric is total system usage. There is no telemetry upload.

`scripts/check-memory-pressure.py` exercises 64 real CEF open/close cycles with
an animated canvas, resizing and the reclamation path, checks browser closure
and retained form/JavaScript state, and records early/late/peak samples in a
local temporary directory. It deliberately does not exhaust the user's machine.

Local macOS arm64 verification (48 GiB RAM, debug bundle, animated canvas):
64 cycles passed with every browser closed and form/JavaScript state preserved
through reclamation. After warm-up, early/late median RSS was 265.75/269.07 MiB
for the main process and 484.21/489.51 MiB for its tree. Main physical footprint
settled at 230.97 MiB. System pressure stayed Normal; this is a controlled
regression test, not a reproduction of the reported incident. See the
[local sample report](releases/preview9-memory-macos.json).
