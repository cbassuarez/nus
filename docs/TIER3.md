# Tier three — the nus-only opportunities

Audit written 2026-09-17, before building. For each: what nus has today
(measured in the code, not remembered), what the best in class does, what
to build and in what order, and where the settings and rules hooks go.
Nothing here is settled design; it is the research the design pass reads.

## 0. Ground rules that hold across all eight

- **Rules first, settings second.** Every item gets a `rules.luau` hook
  before it gets a settings row; the settings row is the hook's most common
  answer with a name. (The `ports()` hook is the pattern: one function, a
  table in, a table out.)
- **Use the real thing.** tree-sitter, ropey, neoscroll's curves, Neovide's
  caret, lsp-types. For this tier: **kitty's remote-control protocol shape**,
  **WezTerm's session format ideas**, **Warp's block semantics via OSC 133**,
  **the standard OSCs (9;4, 10/11, 52, 133, 777)**. No hand-rolled
  protocols where one exists.
- **Broadsheet.** Rules, lamps, caps labels; no chrome that isn't a rule or
  a chip.

## 1. Blocks without a custom shell

**Today (measured):** OSC 133 marks land as `Mark { kind, row }` in
`nus_vt::Term`; `term.marks`, `command_text(m)`, `output_text(m)`,
`block_at(line)`; `termui.rs` draws a rule between commands, chips (COPY ·
RUN AGAIN) on hover, triple-click+Ctrl selects a block's output, the ask
panel sends the last block. **Not there:** folding, exit-code lamps per
block, block navigation keys, share-as-page, block search, block-aware
scrolling, "select this block" as a first-class object.

**Best in class:** Warp's blocks — each command+output is an object with a
lamp (exit code), collapsible, navigable (Ctrl+↑/↓), shareable as a link;
"bookmark this block"; block filter. Ghostty/kitty/WezTerm have the marks
and jump-to-prompt but no object.

**Build:**
- `Block` view over marks: `{cmd, start, end, exit, started, ended, cwd}` —
  exit code needs OSC 133;D with the code (bash/zsh/fish/pwsh integration
  scripts already emit it; confirm PowerShell's does).
- **Lamp** in the gutter at each command row: green/red/dim; running =
  breathing signal. **Fold**: Ctrl+Shift+←/→ on the block, or click the
  lamp; folded = one ruled line `<cmd> · 412 lines · 3.2s · ✓`.
- **Navigate**: Ctrl+↑/↓ between blocks (kitty-style jump to prompt
  exists as marks; add the selection state). **Select**: Ctrl+A twice
  = block, thrice = all.
- **Share as a page**: block → a `nus://block/<id>` page rendered by us
  (reader pane style: command as a heading, output as pre, cwd/time/exit
  as a caps dateline) with COPY AS MARKDOWN · SAVE HTML · OPEN BESIDE.
  Later: paste to a gist via `gh`.
- **Block search**: `/` in a shell filters blocks by command text, like
  the board's filter.
- Settings — TERMINAL · BLOCKS: LAMPS (on/off), FOLD LONG OUTPUT (never ·
  over N lines), RULES BETWEEN COMMANDS (existing). Rules: `on_block(b)` →
  `{fold, tint, name, notify}` (a failing `cargo build` tints red and
  notifies).

**Order:** lamps + exit codes → fold → navigate/select → share page → search.

## 2. Assistant with tabs AND shells as context

**Today (measured):** `ask.rs` sends profile, OS, cwd, the last command
and the tail of its output. Backends: claude/codex/copilot/ollama/curl.
**Not there:** the page's text, other tabs, the focused *block* (only the
last), the editor buffer, skills, memory.

**Best in class:** Dia — "chat with your tabs" reads open tabs' content,
has skills (saved prompts with triggers) and memory (a running profile).
Warp AI reads the last block. Nobody reads tabs *and* the shell.

**Build:**
- **Context picker** in the ask panel, chips: `THIS SHELL · LAST BLOCK ·
  SELECTED BLOCK · THIS PAGE · ALL TABS · EDITOR BUFFER`, defaulting to
  shell + focused block + the split's page. Page text = the reader's
  extraction (we have `reader.rs`); tabs = titles + URLs + reader text on
  demand (cost shown as a token estimate).
- **Skills = rules**: `skills = { explain = { prompt = "...", context =
  {"block"} , key = "E" } }` in `rules.luau`; they appear as chips and
  palette rows (`ask explain`). Ship six: explain this error, write the
  command, summarize this page, compare these tabs, write a commit
  message from the diff, what's on this port.
- **Memory**: `profile/memory.md`, appended by an explicit REMEMBER on a
  turn, read into every prompt; a settings page shows and edits it. No
  automatic memory.
- **Actions**: a fenced block's INSERT/RUN exist; add OPEN (URLs in the
  answer), and for pages: HIGHLIGHT (the answer's quote, found in the page).
- Settings — ASSISTANTS: DEFAULT CONTEXT (chips), MEMORY (on/off, edit),
  BACKEND per Space. Rules: `skills`, `on_ask(ctx)` to add context.

**Order:** picker + page text → skills → memory → actions.

## 3. Title-bar progress (OSC 9;4)

**Today (measured):** `Event::Progress(state, pct)` parsed; `t.progress`
stored; drawn as a bar in the pane. **Not there:** in the sidebar row, the
strip crumb, the taskbar.

**Best in class:** Rio 0.4 renders it in the title/tab; Windows Terminal
puts it on the taskbar button (ITaskbarList3) and the tab; ConEmu did both.

**Build:** small.
- Sidebar row: a 2px signal line under the row title, width = pct; state 2
  (error) red, 3 (indeterminate) marquee, 4 (warning) gold. Strip crumb:
  the same under the tab crumb.
- Taskbar: Windows `ITaskbarList3::SetProgressValue/State` on the main
  HWND (windows-sys); macOS dock badge; Linux `unity://` progress via
  D-Bus is dead — skip.
- Rules: `on_progress(p)` → sound/notify at 100%.
- Setting: TERMINAL · PROGRESS: PANE · SIDEBAR · TASKBAR (each on/off).

**Order:** sidebar + crumb → taskbar → rules.

## 4. Remote control — a socket and a `nus` CLI

**Today (measured):** the instance listener: a loopback TCP port in
`profile/instance`, accepts URLs / `file://` / `raise`, one line each.
**Not there:** a protocol, a CLI, replies, authentication.

**Best in class:** kitty's remote control — `kitten @ launch|send-text|
set-colors|ls|focus-tab|…` over a Unix socket, JSON commands with replies,
`--to` to pick an instance, a password option; WezTerm `wezterm cli
spawn|split-pane|list|send-text`. Both let scripts drive the window;
kitty's is also what its own kittens use.

**Build:**
- **Protocol:** JSON lines over the existing loopback port (Windows) or a
  Unix socket in `profile/` (others), one request → one reply
  `{ok, result|error}`. A per-launch token in `profile/instance` beside the
  port; the CLI reads it, the app requires it. Commands, kitty's names
  where they fit: `ls` (spaces/tabs/panes as JSON), `open <url>`,
  `edit <file>`, `launch --profile --cwd`, `split`, `send-text --tab`,
  `focus --tab`, `theme <name>`, `look <preset>`, `set-colors`, `ports`,
  `hatch toggle|hoist|land`, `block last --json`, `ask "<q>"`.
- **CLI:** a `nus` binary (a thin Rust client, same repo, `crates/cli`),
  also the thing `nus <file>` at a prompt already pretends to be; the
  shell integration can alias `nus` to it.
- **Rules can call it**: `nus.run("split")` from Luau, so a rule can drive
  the window (auto-open a port in a split is already a rule; this makes
  every command reachable).
- Settings — REMOTE CONTROL: on/off, token shown, ALLOW FROM (this user ·
  anyone on loopback).

**Order:** protocol + `ls`/`open`/`launch`/`send-text` → CLI → the rest
of the verbs → Luau bridge.

## 5. Sessions and workspaces as files

**Today (measured):** `profile/session.json` (tabs, panes, pins, stacks,
names, colours, containers, split widths, the hatch tab); RESTORE on the
atlas. **Not there:** named sessions, a file you can write by hand, per-
project layouts.

**Best in class:** kitty `--session file` (a tiny DSL: `new_tab`, `layout`,
`launch --cwd`), WezTerm workspaces + `wezterm cli` to build them, tmuxinator
/ tmuxp YAML, Zellij layouts (KDL, the richest).

**Build:**
- **Format: Luau**, because rules already are and it lets a layout compute
  (`cwd = env.HOME .. "/nus"`). A `.nus.luau` returns a table:
  `{ space = "nus", tabs = { { shell = "pwsh", cwd = "...", run = "cargo watch" },
  { page = "http://localhost:5173", beside = 1 }, { edit = "src/main.rs" } },
  hatch = { shell = "pwsh" } }`.
- **Open**: `nus open layout.nus.luau`, a `.nus.luau` in a project root
  offered on `cd` (a toast: "this folder has a layout · open it"), the
  atlas lists saved ones. **Save**: SAVE THIS LAYOUT in the palette writes
  the current window as one.
- Rules: `on_open_layout(l)` to tweak; `layouts` table for named ones.
- Setting — STARTUP · THEN: RESTORE · LAYOUT <file>.

**Order:** format + open → save → cd toast → atlas.

## 6. Shell integration over SSH

**Today (measured):** integration scripts are injected for local shells
(`-NoExit -Command` for pwsh, rc snippets for bash/zsh/fish); `Profile::ssh`
exists (a profile that runs `ssh host`), but the remote shell has no marks,
cwd, or progress.

**Best in class:** kitty's ssh kitten — copies the integration (and
optionally terminfo, a shell rc) to the remote over the same connection,
then execs the shell with it; `kitten ssh host`. Warp "warpify" — a
one-liner you paste on the remote that bootstraps its integration.

**Build:**
- `nus ssh <host>` (the CLI, item 4) = `ssh -t host 'sh -c "$(cat)"' <
  bootstrap.sh` where bootstrap writes the integration to
  `~/.cache/nus/` on the remote and execs the login shell with it sourced;
  pwsh remote gets the `-NoExit -Command` form. A `terminfo` entry for
  `nus` goes along the same way (the conformance list wants it anyway).
- The ssh profile uses it; the sidebar row shows the host.
- Setting — TERMINAL · SSH: BRING INTEGRATION (on/off), BRING TERMINFO.

**Order:** bootstrap script → the profile uses it → terminfo.

## 7. Themes hot-swap from the shell

**Today (measured):** OSC 10/11/12 set/query work in `nus_vt` (`set_color`,
`dynamic_color_sequence`, `ColorsChanged`), but they change the *palette*
of that terminal only; the look studio (presets, ink/paper, ramp) is app-
level and doesn't listen.

**Best in class:** kitty `kitten themes` + `set-colors` change the whole
window from the shell; OSC 10/11 are the standard; iTerm2 has its own
1337 SetColors; the "dark mode follows the OS" ask (OSC 777?, WezTerm's
`window:get_appearance`) is the other half.

**Build:**
- OSC 10/11 on the *focused* shell → offer, not force: a chip on the pane
  "the shell set a colour · APPLY TO THE LOOK", unless the setting says
  always. Applying = the look studio's INK/PAPER + accent updated to the
  nearest preset or a custom one; reverts with the shell's reset.
- `nus theme <name>` / `nus look <preset>` via remote control (item 4) —
  the honest hot-swap.
- OS appearance: follow the system's dark/light (winit's `theme()`), a
  setting LOOK · FOLLOW THE OS.
- Rules: `on_theme(t)` when the look changes (already `on_event`?), and
  `theme_from_shell(c)` to map a colour to a preset.

**Order:** remote-control verb → follow the OS → OSC 10/11 chip.

## 8. Automatic tab management and semantic grouping

**Today (measured):** stacks (parent/child), folders (GITHUB/PORTS/FILES/
rules/plain), pins, archive/sleep by age, tiles; rules `new_tab` colours.
**Not there:** automatic grouping, "clean up", suggested groups, dedupe,
auto-archive by rules.

**Best in class:** Arc's auto-archive (tabs die after N hours unless
pinned — nus has it), Chrome's Tab Organizer (LLM-named groups), Dia's
tab grouping, Firefox's Tab Groups + "suggest groups" (local ML by title/
URL), Sidekick's domain grouping. Vivaldi stacks. OneTab's "send all to a
list".

**Build:**
- **Signals**: host, path prefix, title tokens, opener (stack), the shell
  cwd that opened it, time-of-open cluster, the tab's Space. All local.
- **Grouping engine**: `group(tab) -> name` in rules first: a default rule
  that groups by host with a per-project override (`if cwd:find("nus")
  then return "nus" end`). Then an automatic pass (TIDY in the palette /
  every N minutes if on): cluster by host + title tokens, propose groups as
  a sheet — `GROUP "docs.rs" · 4 tabs · [MAKE STACK] [ARCHIVE] [SKIP]` —
  never silently.
- **Semantic** grouping through the assistant (item 2): optional, "name
  these groups" sends titles only, returns names.
- **Dedupe**: same URL in two tabs → a chip on the newer one "already open
  in 03 · SWITCH · KEEP BOTH".
- Settings — TABS · TIDY: SUGGEST GROUPS (off · hourly · daily), GROUP BY
  (host · rule), ARCHIVE AFTER (existing), DEDUPE (on/off), NAME GROUPS
  WITH THE ASSISTANT (on/off). Rules: `group(tab)`, `on_tidy(groups)`.

**Order:** dedupe → `group()` rule + host grouping → the TIDY sheet →
semantic names.

## Sequencing across the tier

By value per week, honest:

1. **Blocks** (1) — the crossover feature; a week for lamps/fold/navigate,
   a second for share-as-page.
2. **Remote control + CLI** (4) — unlocks 5, 6, 7 and the Luau bridge.
3. **Progress** (3) — two days; makes shells feel alive in the sidebar.
4. **Assistant context** (2) — the picker and page text are a week; skills
   ride on rules.
5. **Sessions as files** (5) — a week once 4 exists.
6. **SSH** (6) — a few days once 4 exists.
7. **Tab tidy** (8) — dedupe is a day; the sheet is a week.
8. **Theme hot-swap** (7) — a few days, mostly 4's verb.

## Settled 2026-09-17 (seb, via the question tool)

- Blocks fold by **clicking the lamp or Ctrl+Shift+←**; folded = one ruled
  line `cmd · N lines · time · lamp`.
- Share a block **beside the shell** as a reader-style page (`nus://block/<id>`):
  command as heading, output as pre, caps dateline; copy-as-markdown, save
  HTML, gist via `gh` — as icons.
- Assistant default context: **shell + focused block + the split's page**;
  chips (icons) add ALL TABS / EDITOR.
- Tidy **suggests only, never acts**; dedupe chips are the one automatic thing.
- The CLI is a **separate `nus` binary** in `crates/cli`.
- Layouts are **Luau** (`.nus.luau`).
- OSC 10/11 from a shell: a **setting** — CHIP (default: the pane changes,
  a chip offers APPLY TO THE LOOK) · ALWAYS · PANE ONLY.
- **General rule for the pass: icons instead of labels wherever an icon is
  unambiguous.** Chips carry an icon first; words only where an icon would
  need explaining.
