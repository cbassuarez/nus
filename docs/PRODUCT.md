# Product decisions

Settled 2026-09-16 in the design pass. These are the behaviours spike 4 and
the v1 crates implement; change the doc when a decision changes.

## Windows and Spaces
- **A Space is a window.** Switching Space (⌘⌥1–9) raises that window. The
  Space row at the top of the sidebar is a window switcher; the tab list is
  the current Space's only. Quick terminal and PiP are the only other windows.
- A Space owns: its signal color, a default shell profile + cwd for new
  terminal tabs, and a browser profile (CEF request context / cookie jar).
  Config overrides per Space are not v1.
- Splits: a tab is one pane or a left|right pair. No vertical splits, no
  nesting. One tab = one sidebar preview.
- Chromeless by default: 6px signal band + 30px top strip. Sidebar on ⌘⇧S.

## Quick terminal
- ⌥⌘T. Global hotkey by default (summons over any app); `quick.global =
  false` keeps it inside nus. Drops from the top edge of the active display.

## Attention ("waiting")
- Signals: BEL, OSC 9 / OSC 777 notifications, and shell integration
  (OSC 133 prompt marks → "command finished"). nus installs the shell hook
  for pwsh, bash, zsh, fish. No output-idle heuristics.
- Shown in the sidebar (filled label in the Space's signal color) and in the
  top strip summary; OS notification when the window is unfocused.

## Session restore
- On launch: Spaces (windows), tabs, splits, each terminal's profile + cwd,
  each browser tab's URL and scroll, **and each terminal's scrollback
  snapshot** as read-only history above the fresh prompt.

## Theme
- Follows the OS (paper when light, ink when dark), switches live.
  `theme = "paper" | "ink"` pins one.

## Browser
- New tab opens ⌘K; there is no new-tab page. URL or search from the palette.
- Search engine configurable (`browser.search`), default Google.
- Third-party cookies blocked by default, per-site exceptions in config.
- Downloads: silent to ~/Downloads, ruled toast with open / reveal.
- Password manager: 1Password via the `op` CLI (biometric unlock), our own
  form detection and fill. Bitwarden later.
- Import: bookmarks/history/Spaces from Chromium-family browsers (Arc, Chrome,
  Edge, Brave) — Chromium profile format, one-shot.

## Profiles
- SSH: every `Host` in ~/.ssh/config becomes an `ssh:<name>` profile.
- WSL: `wsl -l` distros become `wsl:<name>` profiles on Windows.
- "Open in nus" entry in Explorer / Finder / file managers.

## Ops
- Self-update from GitHub Releases, offered in the top strip. No telemetry.
  Crashes write a local log only.

## Navigation (settled 2026-09-16, second pass)

**Modifier.** App chords are ⌘ on macOS and **Ctrl+Shift** on Windows/Linux, so
they never reach the shell (Ctrl+T/K/L/W/D/R are shell keys). Ctrl+1–9 is the
one plain-Ctrl chord: tab by position (shells don't use it).

| chord (Win/Linux · mac) | action |
|---|---|
| Ctrl+Shift+T · ⌘T | new tab → palette in *new* mode |
| Ctrl+Shift+K · ⌘K | palette in *go* mode |
| Ctrl+Shift+L · ⌘L | palette in *url* mode (browser pane) |
| Ctrl+Shift+W · ⌘W | close tab (closes every selected tab; asks first if a foreground process is running) |
| Ctrl+Shift+Z · ⌘Z | reopen last closed tab (profile + cwd, or URL) |
| Ctrl+Shift+D · ⌘D | toggle the browser split |
| Ctrl+Shift+S · ⌘⇧S | sidebar |
| Ctrl+1–9 · ⌘1–9 | tab N |
| Ctrl+` · ⌃` | cycle tabs most-recently-used (Ctrl+Tab is left to the OS) |
| Ctrl+PgUp/PgDn · ⌘⇧[ ] | previous / next tab in order |
| Ctrl+Enter · ⌘↵ | open the detected localhost URL in the split; +Shift → new tab |

**New tab** opens the palette (*new*): profile rows for a terminal, or type a
URL / search terms for a browser tab. Empty Enter → default shell.

**URL at a shell prompt.** If the entire line typed at a fresh prompt is a URL
(scheme, `localhost[:port]`, or `host.tld[/path]` with a known TLD) and you
press Enter, nus clears the line and opens the page in the split beside the
terminal (Ctrl+Shift+Enter: new tab). A ruled hint appears as you type
(`↵ opens in browser · Ctrl+↵ runs in shell`). Any editing key (arrows,
history, Ctrl+…) disqualifies the line until the next Enter. Bare words never
trigger. This is the browser-session-from-a-terminal feature.

**Browser panes** mirror Chrome while focused: Ctrl+L url, Ctrl+R / F5
reload, Alt+←/→ back/forward, Ctrl+plus/minus/0 zoom, Ctrl+F find, F12
devtools. App chords stay Ctrl+Shift.

**Palette.** *go*: tabs → actions → open-URL / search rows. URL vs search is
auto-detected and both rows are always offered. *new*: profiles → browser
row. *url*: navigate the browser pane.

**Sidebar.** A pinned row on top (durable tabs: your shell, the dev server),
then one list in creation order. Ctrl+click toggles selection, Shift+click
selects a range; close acts on the selection. Pin/unpin via the palette or
the tab's row menu.

## Sidebar and settings (settled 2026-09-16, third pass)

- **Reveal:** hidden by default; hovering the 6px hot edge slides it over the
  content; it hides ~300 ms after the mouse leaves. Ctrl+Shift+S pins it.
- **Rows:** one line per tab — number, kind glyph, title, cwd/host. The 52px
  live preview expands only under the hovered row and under any tab that is
  *waiting*. Pinned row on top (compact cells).
- **Footer:** Space identity and controls — Space color + name, browser
  identity (cookie jar) and default shell profile for new tabs, assistant
  router status (which of claude / codex / ollama / chatgpt are wired, and the
  default), gear → settings tab, `+ new tab`.
- **Settings:** Ctrl+, opens a native settings tab (Broadsheet styled):
  fonts, theme, Space profiles, browser (search engine, cookies, downloads),
  assistants (router), keys. Every edit writes `~/.config/nus/init.luau`; the
  file is the source of truth and hot-reloads.
- **Assistants:** the palette offers *search*, *ask chatgpt*, *ask claude*
  (web, `?q=`) and *ask <tool> in this shell* for local CLIs on PATH
  (`claude "…"`, `codex "…"`, `ollama run <model> "…"`), which types the
  command into the focused terminal. The router (which tool, which model,
  args vs stdin) is a config table.

## Arc parity targets (settled 2026-09-16, fourth pass)

**Shell (carapace).** The Space color as a physical shell around the window.
Styles: `band` (today's 6px top band), `stroke` (all four edges), `gradient`
(signal → ink along the stroke), `aurora` (slowly animated gradient). A global
`radius` rounds the window corners and the stroke together (0 by default —
Broadsheet — but yours to turn). Config: `nus.shell = { style, width, radius }`.

**PiP.** Ours, in the compositor. Trigger: automatically when a playing video's
tab leaves view (tab/Space switch, window blur); returning to the tab pulls it
back; manual toggle too. Source: the tab's own texture cropped to the video's
rect, tracked by an injected script that also scrolls it into view — no second
decode, no DRM issue. Transport keys act on the element through CDP, so sites
can't hide them: ←/→ ±10 s, Space/K play-pause, J/L ±10, ,/. frame-step, ↑/↓
volume, M mute — only while the PiP window is focused. Bezier easing on every
move/resize, wheel-over scales around the cursor, corner snapping.

**Local sites.** Detected: localhost, 127.0.0.1, `*.local`, RFC-1918. Safety
tape (diagonal signal + ink hazard stroke) around the page and in the URL
field. Dev suite, all v1: ports list in the palette (listening ports with
process names → open with tape on), DevTools as a windowless CEF pane,
responsive presets (390/768/1440) + screenshot from our texture, reload on
directory change + console mirrored into the terminal split.

**Script channel.** One mechanism under PiP, theme reach, the JSON viewer and
the dev suite: scripts injected per tab via CEF's DevTools protocol
(`Page.addScriptToEvaluateOnNewDocument`, `Runtime.addBinding`), results back
through a DevTools message observer. This is also the userscript system.

## Header, stacks, settings, onboarding (settled 2026-09-16, fifth pass)

**Icons.** Phosphor (MIT), regular weight for chrome, bold for the active
state, rasterized from SVG into the glyph atlas. Text labels in the chrome
give way to icons; caps labels remain for words that are content (tab titles,
section names).

**Header.** Platform-native controls (traffic lights left on macOS; — ▢ ✕
right elsewhere), drag anywhere else. Left: wordmark, then a clickable crumb
(Space → Space switcher, tab → palette, cwd → open here). When a browser pane
is focused the crumb becomes an editable URL field. Right: a status cluster —
waiting count, PiP, assistant, local ports — as icons with counts.

**Stacks.** A tab that spawns another (terminal → URL beside it, page → link,
agent → tab) nests it one level under the parent. A stack collapses to one
row ("parent · +3") unless it is active or holds a waiting tab. Closing the
parent closes the stack (asks first). A stack is one ⌘-number; ⌘⇧[ ] walk
inside it. Terminal → URL opens the split beside the terminal; a link from a
page opens in the same stack, unfocused.

**Settings, v1 scope.** Terminal: scrollback, cursor, copy-on-select, bell →
OS notification when unfocused. Tabs: new-tab position, ⌘W inside a stack,
auto-collapse. Browser: per-site zoom, downloads directory, third-party
cookie exceptions. Assistants: router table (default tool, model per tool,
args vs stdin, web fallbacks).

**Onboarding.** First launch opens a real shell with a ruled panel beside
it: five things to try (⌘K, ⌘T + URL, a URL at the prompt, hover the edge,
⌥⌘T), each ticked off as you do it. No wizard, no modal.

## Surface, rules, sidebar rules (settled 2026-09-16, sixth pass)

**Texture stays on the carapace.** Grain goes on the band or stroke, never
on content and never on the PiP video.

**Surface picker.** Beyond the fixed Broadsheet tokens, the surface is the
user's: a signal colour (carapace, Space square, ticks, progress), an
optional base the paper is tinted toward (with a tint amount), texture
strength, window opacity (terminal panes show the desktop through; the
chrome stays paper), carapace style, width and corner radius. Every control
is native chrome — swatches, chips, sliders — and the window is its own
live preview.

**Rules.** `rules.luau` (sandboxed Luau; `~/.config/nus/rules.luau` in v1)
decides what a new tab or Space looks like. `new_tab(ctx)` gets kind,
index, profile, Space, signal, theme and — when the tab joins a stack — the
parent's colours, and returns `{ bg, signal }`. The default rule gives each
terminal its own hue and keeps a stack in the parent's family. The RULES
section of settings shows the file, reloads it, resets it, opens it in the
editor. Helpers: `hue`, `mix`, `hsl`.

**Sidebar rules.** Side: left or right. Reveal: from the screen edge (a
flick from the desktop works) or only when the pointer travels from inside
the window. Grace: how long it stays after the pointer leaves. Fullscreen:
hover, hidden, or pinned. F11 toggles fullscreen.

**Tab behaviour.** Links a page opens go to the stack, the split, or a new
tab. A URL at a prompt opens beside or in a new tab. Closing asks when a
process is running, or never. Default shell profile.

**Onboarding, as built.** The fifth chord in the panel is Ctrl+` (last tab)
until the quick terminal exists. Ticks persist across launches; SKIP THE
TOUR is remembered.

## Motion, small screens, 2027 niceties (settled 2026-09-16, seventh pass)

**Motion as ink.** One easing (ease-out cubic), base durations of 80–220ms,
and a register slider from snappy (0.45×) to cinematic (2.2×); reduce
motion follows the OS or is forced. Things slide and rules extend; nothing
scales, bounces or blurs. Sidebar slides, rows grow, stacks unfold, the
active tint travels, the palette rises, bands drop, the crumb fades, the
loading bar travels. Content switches instantly.

**Loading bar.** Chases real progress with a trickle, fades on arrival.
Styles: rule, comet (bright head, fading tail), carapace (fills the band
across the window). Colour: signal, the tab's own, or ink. Weight and
chase are sliders.

**Width, not mode.** Wide ≥1200 logical px: everything. Standard 900–1200:
sidebar hover-only. Narrow <900: no split (the focused pane takes the
content), one status icon, palette to the window, settings as a tile grid
that drills into a section under a back crumb. No manual kiosk/touch
switch.

**Reader mode.** Ctrl+Alt+R (⌘⌥R, as Firefox) or the book: the article, extracted in the
page, set here in Newsreader on our paper — 640px measure, 19/28 body,
rules not boxes, signal bullets, mono code. The page keeps living
underneath. Word count in the tools row; the text feeds "ask about this
page".

**Boosts.** `on_page(ctx)` in `rules.luau` returns `{ css, js }` per site.

**Little nus.** Links from other apps open in a small floating window with
the band and one page; Esc closes, Ctrl+Shift+O keeps it as a tab. One
instance: later launches hand their URLs over. MAKE DEFAULT in settings
registers nus as a browser (Windows now; bundle / .desktop for the
others).

**Accessibility.** The chrome is an AccessKit tree from day one: every hit
target is a node with a spoken label and an action, panes carry their
text, the palette is a list box. Screen readers and UI automation see the
same thing the mouse does.

**Deferred from this pass** (still planned): content blocking (adblock
crate), sleeping tabs, OS media controls, find in page, favicons, ruled
context menus, downloads, history/autocomplete, session restore, site
permission bands, print to PDF, snap layouts, `<select>` popups.

## Look & feel pages, sound, startup (settled 2026-09-16, eighth pass)

**Icon.** A Newsreader Italic *n* with an open, tapered band in orbit
(NASA-meatball stroke: start and end, heavier at the belly, tilted −24°).
Ink *n*, signal band; the live icon follows the current theme and signal,
the bundled one ships Broadsheet. The splash is the same drawing, nothing
else, and fades once the first tab has painted (debounced, min hold from
settings). Restore / recent live in the **atlas** modal (header planet
icon), never on the splash.

**Header.** Wordmark once. A crumb of favicon + *Title · host*, click to
edit the address. DevTools panels (console, network, elements) are icons.

**Sidebar.** 32px rows, favicon or profile icon, ⌘-numerals ≤ 9, hover ×,
waiting dot, children under a rule. Footer is one row: avatar (drop a
`profile/avatar.png`), +, history, downloads, settings.

**Surface.** Ramps, not a colour: 2–4 stops, angle, loop, aurora drift and
breath. Signal, tint, opacity (on window / chrome / panes — Windows DX12 is
opaque, the row says so). Textures: grain, stipple, stitch, linen,
halftone, scale, still or animated, placed on the **carapace** (masked to
its rounded stroke, two-tone so it reads on any ramp, ×3 strength for a
thin band), the chrome, or the panes — never on content or video. Shell:
band or frame, width, radius. Presets (Broadsheet, Midnight, Ledger,
Darkroom) plus `profile/surfaces/*.json`; SAVE PRESET writes one.

**Theme.** Paper / ink / page tokens per mode, ANSI 16 with a picker,
families (Solarized, Gruvbox, Nord, from-signal), contrast grade and
saturation, import from Ghostty / Windows Terminal / VS Code / base16
files in `profile/themes/`. Broadsheet's own tokens are not editable —
edits layer over them and RESET returns.

**Rules.** `profile/rules.luau`, sandboxed. `new_tab(ctx)` and
`new_space(ctx)` return a look (`bg`, `signal`); `on_page(ctx)` returns
boosts; `on_event(ev)` returns a cue name (or `false` to silence). Five
starters, live status, syntax-coloured preview, reload on save.

**Sound.** cuelume's seventeen recipes ported to a native synth on cpal
(tone + noise layers, exponential envelopes, glide, detune, biquad,
shimmer), cached per cue, mixed on one stream. Fourteen events, each
mapped to a cue or silence, previewable; master volume; rules can override
per event.

**Startup.** Window: remembered / maximised / fullscreen. Splash: icon,
icon + sound, none; hold. Then: last session / atlas / new tab / nothing.
Atlas: on demand, on launch, persistent. Outside links: little nus or a
tab. Start on login; make default browser.

**Cursor.** Shape (the shell's, block, beam, underline), hollow or hidden
when unfocused, blink never / after 2s idle / always with a period,
colour ink / signal / the tab's own, motion jump / glide / comet on the
motion register, beam weight, pointer over the chrome (system, ink arrow,
signal dot), hide the pointer while typing.

**Width rule for all of it.** Every page is the same ruled two-column
form; under 900px it becomes tiles that drill in. Settings changed by
assistive tech persist like clicks.

## Windows, the sidebar header, hover (settled 2026-09-16, ninth pass)

**Windows, not Spaces.** A window owns its tabs; everyone knows what a
window is. Auto-named from the git root it was launched in, else the
dominant host, else *nus*; duplicates number themselves; F2 (or the
list) renames through the palette; the name shows in the sidebar, the
list, the OS title bar and Alt-Tab. Switching is the OS's job plus the
header's list; new window is Ctrl N. (Spike: one process per window,
each with its own Chromium cache — the process singleton forbids sharing
one; v1 moves windows in-process so they share the profile.)

**Header styles, named for what they do.** BAR: the window's name and
NEW TAB in one ruled row, a split caret for the kinds. RAIL: every
window as its square along the sidebar's edge, the name above the tabs.
Bools for the variants: masthead title (Newsreader) or caps, dateline
(where · tabs · ports), NEW TAB in the header and/or as the next ruled
row (a ghost plus where the tab will appear), window cell with or
without the name, kinds caret, rail always or on hover, press flash.

**NEW TAB** is the button hit most: click for a shell in the default
profile (rules colour it, the band flashes through the signal); hold
240ms, right-click, or the caret fans the kinds out (profiles as their
squares, page).

**Hover.** Every icon button gets a soft rounded ink overlay under the
pointer; the important ones move on the motion register — pop, spin,
bob, swing. No icon is ever a bare glyph you have to guess at.

**Look chip.** Paper · ink · signal in the footer, fanned on hover;
opens the look pages. Settings are grouped LOOK · FEEL · WORK · SYSTEM.

## The look studio (settled 2026-09-16, tenth pass)

**One roof.** Appearance, surface, theme and cursor are one section,
LOOK: a live **proof** of the window at the top (carapace with its ramp
and texture, chrome, sidebar, a shell with coloured runs and the cursor,
a page), a tab strip beneath — PRESETS · SURFACE · TOKENS · TYPE &
MOTION · CURSOR — and the rows of the tab you're on. What you change,
you see change in the proof and in the window around it.

**Neobrutal tokens.** Presets are cards faced with their own ramp, the
signal as a chip, the name in Newsreader; a hard offset shadow, a 2px
ink outline, the current one ringed. Tokens are big tiles with name and
hex under them; ANSI as a row of small tiles. Tiles lift on hover.

**A real picker.** Pick a tile and hue / saturation / lightness sliders
appear with a tray of candidates (the signal's family, the swatches, the
mode's papers or inks). Any token can be any colour; the tray is a
shortcut, not a limit.

**Icon and text.** The icon is embedded in the exe and set on every
window from the first frame — never the default exe glyph. Sidebar text
fits, or marquees when its row is active or hovered, or clips under a
fade; never an ellipsis.

## The polish pass (settled 2026-09-16, eleventh pass)

Researched against Rio, Ghostty, kitty, WezTerm, Warp, Dia, Arc, Zen,
Vivaldi and Orion; tier one built in full.

**Windows in one process.** One `App` per OS window, one Chromium
profile, a registry every window sees; the rail and the window list are
real; links from other launches land in the window last used.

**Shell integration, auto-injected.** OSC 133 prompt marks, OSC 7 cwd
and OSC 9;4 progress read in nus-vt at the point they occur; bundled
scripts for PowerShell (-NoExit -EncodedCommand), bash (--rcfile), zsh
(ZDOTDIR), fish (-C), cmd (PROMPT); nushell has it built in. AUTO / OFF
under TERMINAL. It buys: a bar cursor at the prompt, new shells where
the focused one is, the window's name and dateline from the cwd, jump
between prompts, copy the last output, paste with a band when it's
risky, DONE / FAILED badges and a ring when a long command ends
elsewhere, × codes on failed lines, progress as the loading bar, no
close-confirm at a prompt, recent commands in the palette to run again.

**Blocks.** Each command is a block: a hairline where it starts, a
gutter rule on hover (signal when it failed), chips to copy its output
or run it again, Ctrl+triple-click selects its output, the scrollbar
ticks are the prompts.

**The terminal's interaction layer.** Selection with semantic zones,
copy and paste (bracketed, protected), click-to-move at a prompt, a thin
scrollbar that appears on hover or when scrolled, find in scrollback as
a band, hints mode (labels over URLs, paths and hashes), an unfocused
split washes with paper, a cols × rows card while resizing.

**Fonts and images.** Missing glyphs come from system fallbacks
(symbols, emoji as masks, other scripts) without the app knowing; Kitty
graphics and iTerm2 inline images draw in the shell; the Kitty keyboard
protocol was already in.

**The command line, lit and predicted.** Tokens colour as you type;
the history entry that continues your line ghosts after the caret and
Right / End accepts it. Terminal-side, nothing to install; history per
profile in profile/history.

**Browser table stakes.** Find in page, downloads (to ~/Downloads, a
list in the footer, click reveals), permission asks as a band, `<select>`
popups composited, content blocking (built-in hosts plus
profile/blocklist.txt), history-ranked address palette, idle pages sleep
after 30 minutes and archive after 12 hours.

**The welcome page.** A full tab, ruled, with a live START HERE
checklist and TRY buttons that do the thing; F1, the palette, settings.

**Still to do before bundling:** `<select>` popups need a live test on a
real form; downloads and permission bands were built to the CEF
contract but not exercised by hand; colour emoji want an RGBA atlas;
Sixel is not decoded; command output folding; hints with custom regex.

## Tier two (settled 2026-09-16, twelfth pass)

The ten from the research report, in the order asked, all built.

**Stock themes.** Thirteen originals and seven ports, each a theme
across paper, ink, page, sixteen ANSI colours, the surface, cursor,
loading bar, sounds and a tab-colour wish for the rules; a contrast
audit is a test.

**The tab tree.** Stacks nest as deep as they go; a caret folds, Ctrl+
Shift+- folds all; a row drags onto another to nest under it, or
between rows to move.

**Name, icon, colour per tab.** Right-click a row: RENAME (F2, through
the palette; Shift+F2 now names the window), ICON (an emoji or any
short string in the favicon's place), a row of swatches (the user's
tint beats the rules' and survives a theme), PIN, CLOSE, TILE, SAVE TO
FOLDER. All of it in the session.

**Tiles.** Ctrl+click rows, Ctrl+Shift+D: two side by side, three as an
L, four as a grid. A tiling belongs to its tabs and shows whenever one
is active; dividers drag; Ctrl+Alt+arrows walk, with Shift swap; a
closed tile re-tiles the rest.

**Peek.** Alt+click a link: it floats over the page behind a scrim.
Esc or a click outside closes it; Ctrl+Enter keeps it in the stack. A
peek is a tab the sidebar doesn't list, so every pane path serves it.

**Compact.** Ctrl+Shift+B: a 48px column of icons, the top strip hidden
until the pointer reaches the top edge, the hovered row's name beside
the column.

**Chains, and settings in the palette.** A `chains` table in
rules.luau names lists of palette commands ("open <url>", "run <cmd>",
"tile", anything the palette takes); every settings row is a palette
row that opens settings there.

**Folders.** Under the tabs: GITHUB (open pull requests involving you,
from `gh`, off the main thread), PORTS (what's listening), lists from
rules.luau (`folders`, static or a function), and your own (SAVE TO
FOLDER; a saved page never archives). Items open in place or go to the
tab that has them.

**The site panel.** The gear at the end of a URL row, per host: zoom
(remembered), autoplay, JavaScript, cookies (block or clear), boosts,
blocking, and the permissions the site was given — band answers are
remembered and answered without asking next time, each forgettable.

**Focus.** Ctrl+Shift+F11: the page or shell alone in the window.

**Containers.** Named cookie jars, one CEF request context each under
profile/container-<name>; a window's container colours its square and
names its title, new pages open in it, a new window inherits it; a
page can be reopened in another; the palette makes new ones.

**Learned on the way.** A CEF request context with its own cache must
be a direct child of the root cache path, and its profile initialises
a few pumps later — the first browser in it waits for
`on_request_context_initialized`. Sidebar hits resolve topmost-last so
a menu over the folders wins. `layout()` now schedules the shells'
resize itself (a split terminal used to keep its old size). SendKeys
cannot deliver Ctrl+Alt+arrow chords; keybd_event with the arrow's
scancode and the extended flag can.

## Icons, status, ask (settled 2026-09-17, thirteenth pass)

**Site icons.** A page without a favicon gets a letter tile — the
host's first letter on a square in one of the signal's family — never
the generic globe; a favicon can't linger from the previous site.

**What's in the tab.** The row names the focused pane and badges the
split's other pane's kind at the icon's corner; the title follows. A
shell that exits leaves: the other pane takes the tab, a lone shell's
tab closes.

**The status lamp.** LIVE was a word; it's a lamp now — signal while
loading, ink when live, hazard stripes when local, hollow while asleep
— with BROWSER · STATUS choosing lamp, lamp + word, word, or none.

**Ask.** Ctrl+Shift+? beside a shell: one line in, a few command blocks
out, each with INSERT (at the prompt), RUN, COPY. The shell, folder and
the last command's output go along. Not a chat; the last turns scroll.
Backends are what the machine has: claude -p, codex exec, the Copilot
CLI if installed, ollama, or the Anthropic API through curl with
ANTHROPIC_API_KEY; NUS_ASK_CMD for anything else. (Ctrl+Shift+A was
first; a global hotkey on the machine swallowed it before winit saw it.)

**The sidebar drew at 10 fps.** fit() trimmed a character at a time and
measured every step; it bisects now and widths are cached. 83 fps in a
debug build.

## Panes, tools, scrolling, conformance (settled 2026-09-17, fourteenth pass)

**Split panes.** Nothing shows until the pointer nears a pane's corner;
then MOVE (drag onto a sidebar row, or NEW TAB), SWAP, SOLO, TO A TAB,
CLOSE bloom out of it on a proximity field and fade as it leaves —
never persistent. The rule between the panes lights as you near it and
drags; the width is the tab's. TABS · PANE CONTROLS: NEAR or NEVER.

**The caret is Neovide's.** A port of its cursor renderer: four
critically damped springs, the leading corners fast and the trailing
ones slow, drawn as one quad. TRAIL is Neovide's trail_size.

**Syntax is tree-sitter.** Bash and PowerShell grammars ride in the
binary for the prompt line and the ask panel's blocks; any other grammar
loads from profile/grammars/<name>/ (the library `tree-sitter build`
makes, plus highlights.scm). LSP waits for an editor pane; the crates
are lsp-types and lsp-server when it comes.

**Optional tools.** The welcome page's OPTIONAL TOOLS section is the
first boot's question: language servers, grammars, assistant CLIs,
each fetched only on GET (curl, then unzip/gunzip into profile/), each
a folder you can delete. assets/bundles.json is the list;
profile/bundles.json adds to it. Grammar bundles wait on the release
pipeline that builds them.

**Scrolling.** The shell scrolls on neoscroll's curves — a line at a
time on an eased clock, more ticks extending the trip; Shift+PgUp/
PgDn and Ctrl+Shift+Home/End ride the same curve. Pages keep
Chromium's smooth scrolling, switchable.

**Conformance.** TERM=xterm-256color, COLORTERM=truecolor,
TERM_PROGRAM=nus; DA1/DA2; XTVERSION answers `nus <version>`;
XTGETTCAP answers TN, RGB/Tc, colors, setrgbf/b, Ms, Ss/Se, Smulx and
refuses the rest; DA3; DECXCPR; XTWINOPS 11/13/14/16/18/19/22/23;
DECRQSS for SGR, DECSTBM, DECSCUSR, DECSCL, DECSCA; OSC 8, 52, 133, 7,
9;4, 1337; the kitty keyboard protocol; synchronized output (2026);
bracketed paste; focus events. Sixel (DA1 claims 4; XTSMGRAPHICS
answers colours and geometry) lands as an image placement like Kitty and
iTerm2 pictures. Mouse reporting: 1000/1002/1003 with X10, UTF-8 (1005),
SGR (1006) and SGR-pixel (1016) encodings; Shift keeps the click for
selection; the wheel reports when the application has the mouse, is
arrow keys under alternate scroll (1007), and scrolls us otherwise. Next
for adoption: a terminfo entry (`nus`) shipped and TERM=nus once tools
know it, vttest/esctest runs, and a `nus` CLI (open a URL or a file,
`nus ask`).

## LSP, the editor, the ports board (settled 2026-09-17, fifteenth pass)

**LSP is real.** `crates/lsp` is the client: a server per language spawned
on demand over stdio (JSON-RPC, lsp-types), initialize/shutdown, document
sync, hover, completion, diagnostics, definition, formatting, all off the
main thread. Servers come from the welcome page's OPTIONAL TOOLS (bundles):
rust-analyzer, PowerShell Editor Services, bash-language-server,
typescript-language-server, pyright, and more as bundles.json grows.

**The editor pane is a full editor.** Open a file from the prompt
(`nus <file>`), a click on a path, or the URL row. Multi-buffer with a
FILES folder in the sidebar (the cwd tree), find/replace, format-on-save,
tree-sitter colour, LSP hover/completion/diagnostics/go-to-definition.
The buffer is ropey; nothing hand-rolled where a crate is the real thing.

**The prompt line has LSP too**, configurable: TERMINAL · PROMPT LSP:
QUIET (default: dotted underline on a diagnostic, hover for the message,
completions ride the ghost prediction and Tab accepts) · MENU (a small
completion list under the caret) · OFF. bash-language-server or PowerShell
Editor Services by shell.

**The ports board** (Ctrl+Shift+P; the status-cluster icon; palette
`ports`; the PORTS folder header) is a centred overlay sheet, ~70% wide,
in the departures-board manner: monospace ruled rows that split-flap in
and out as ports arrive and depart (reduced-motion: fade), a lamp per row
(UP · EXPOSED · DYING). EXPAND or Ctrl+Enter makes it a full page tab;
Esc closes. Grouped by origin by default — MINE (ports your nus shells
started; we own the pty tree) · OTHERS (user processes) · SYSTEM
(privileged, dim) · CONNECTIONS (established in/outbound, one row per
process with a count and top remote hosts) · DOCKER/WSL when present —
or by port, or by process. A row knows process, pid, command line, cwd,
owning tab, bound interface (0.0.0.0 gets the exposed lamp and a warning
stripe), uptime, protocol (TCP/UDP), and an HTTP probe (status, title,
framework) when probing is on. Enter/click opens the detail in place with
the action strip: OPEN (tab/split/peek), COPY URL, JUMP TO SHELL, KILL
(graceful, DYING lamp, force after 3 s; no confirm for yours, confirm for
system), RUN AGAIN (its block command), TUNNEL (cloudflared or ngrok in
the owning tab's split, the public URL on the row), WATCH (a toast when
it comes up or goes), and an inline rename on the name cell (persisted
by process+port in profile/ports.json). A new port: the status icon
lights and a line rides beside it for 6 s — "5173 · vite is up · O to
open" — never stealing focus.

**Settings — PORTS:** GROUPING (origin · port · process); OPEN IN (tab ·
split · peek); POLL WHILE OPEN (1 s · 5 s · 10 s); NEW-PORT TOAST (on ·
off); SHOW SYSTEM · UDP · CONNECTIONS · DOCKER/WSL (each on/off); ASK
BEFORE KILL (system · always · never); PROBE (on · off — it sends a
request to your server); TUNNEL (cloudflared · ngrok); HIDDEN PROCESSES
(a list, seeded with the system set). **Rules:** `ports` in rules.luau —
by port or process: name, tint, auto-open in a split when it appears,
auto-tunnel, hide, watch. Rules for power users; the settings for those
who can't be bothered.

## The hatch (settled 2026-09-17, sixteenth pass)

**The quick terminal is the hatch.** A second, borderless, always-on-top
window of the Space, summoned by a global hotkey — Ctrl+` by default;
Win+` or Ctrl+Shift+Space — that works whether or not nus is in front,
onto the monitor under the pointer (or the foreground window's, or the
primary). What it shows is a real tab of the Space that lives up there
instead of in the sidebar: HOIST the tab you're on (Ctrl+Shift+↑) and it
goes up, whatever was up comes down; LAND (Ctrl+Shift+↓) brings the
hatch's tab down as a normal tab, focused. A hatch with nothing in it
opens a fresh shell in the active shell's cwd. Blocks, marks, ports, ask,
the prompt LSP, a split — all of it works up there, because it is the
same pane machinery.

**Two looks, sheet default.** The SHEET: 960 wide from the top edge of
the monitor, no top border, 2px edges, 40% tall (SIZE: 30–60%), the
Space's band as a lip along its foot that you drag to resize; the
masthead reads *quick* · the Space's square · the tab's title, with
LAND · PIN · ESC on the right; the foot carries the ports toast or the
hotkey's status. The CARD: 70% × 60% centred, the carapace as a 22px
frame around the content (the one surface texture belongs on), a short
signal mark in the frame's foot; drag the frame to move, the corner to
resize. Both ride in on the motion register (the sheet drops, the card
settles) and hide the same way. AUTOHIDE (default on) hides it when it
loses focus unless pinned; Esc hides. SPACES: FOLLOW (one hatch per
Space; the hotkey goes to the Space you were last in) or ONE FOR ALL.
Where the OS won't give a global hotkey (macOS until the Accessibility
permission lands; Linux until the portal), the same chord works inside
nus and the settings say so.

**Settings — HATCH:** LOOK · HOTKEY · SIZE · MONITOR · AUTOHIDE · SPACES.

## Blocks, remote control (settled 2026-09-17, seventeenth pass)

**Blocks.** Every command is a block over its OSC 133 marks. A lamp on the
prompt's row in the gutter: green ran, red failed, dim unknown, breathing
signal while it runs. Click the lamp (or Ctrl+Shift+←/→) to fold the output
to one ruled line — cmd · N lines · lamp. Ctrl+↑/↓ walk the blocks; Ctrl+A
twice selects a block's output, thrice everything; Ctrl+Shift+/ filters
by command. Hover a block: share · run again · copy. Share writes a
Broadsheet page (command as heading, output as pre, caps dateline) and
opens it beside the shell; the palette offers copy-as-markdown and a gist
through gh. TERMINAL · BLOCKS: LAMPS, FOLD NEVER / OVER 50 / OVER 200.
Rules: `on_block(b)` → `{ fold, notify }`.

**Remote control.** A JSON-lines protocol on the instance port with a
per-launch token (both in `profile/instance`): ls · open · edit · launch ·
split · send-text · focus · close · theme · look · ports · hatch · block ·
ask · raise · version. The `nus` command (`crates/cli`) speaks it, and a
bare `nus <file>` or `nus <url>` opens it. Rules call `nus.run(cmd, args)`;
the app answers after the hook returns.

## Tier three, the rest (settled 2026-09-17, eighteenth pass)

**Progress.** OSC 9;4 runs as a 2px line under the tab's sidebar row and
the strip crumb (red on error, gold on warning, a marquee while
indeterminate) and on the Windows taskbar button. TERMINAL · PROGRESS.
Rules: `on_progress({ state, tab })`.

**The assistant sees tabs and shells.** Chips above the field, as icons —
shell · block · page · tabs · editor · memory — lit when they go along;
shell + the block in focus + the page beside are the default (ASSISTANTS ·
GOES ALONG). The page's text comes through the reader. Skills are rules
(`skills = { name = { prompt, context } }`): text chips in the panel and
`ask <name>` in the palette. Memory is `profile/memory.md`: the book icon
on an answer keeps its first line; ASSISTANTS shows and forgets it. Insert
· run · copy on an answer are icons with tooltips.

**Layouts as Luau.** A `.nus.luau` returns `{ space, tabs = { { shell, cwd,
run } | { page, beside } | { edit } … }, hatch }` and can compute (`env.HOME`,
`env.here`). Open from the palette's layout rows, the atlas, `nus open
<file>`, or the chip a shell's cwd offers when one is there; SAVE THIS
WINDOW AS A LAYOUT writes `profile/layouts/<name>.nus.luau`. STARTUP ·
THEN · A LAYOUT. Rules: `on_open_layout(l)`.

**SSH.** An ssh profile carries the integration in its remote command:
the bootstrap writes nus's bash/zsh scripts to `~/.cache/nus` on the far
side over the same connection and execs the login shell with them. Nothing
installed there. TERMINAL · SSH. `nus ssh <host> [--split]`.

**Tidy.** Suggestions only. A page open elsewhere gets a band on the newer
tab: switch there, or keep both. `group(tab)` in rules names a group
(default: host, or project folder). TIDY — the palette, or hourly/daily
(TABS · TIDY) — proposes groups as a sheet with make-a-stack · archive ·
skip per group; nothing moves until you tap. Rules: `on_tidy(groups)`.

**Shell colours.** OSC 10/11 from a shell changes the pane; TERMINAL ·
SHELL COLOURS: OFFER (default — a palette icon on the pane applies them to
the look: ink or paper by the background, the accent from a coloured
foreground) · ALWAYS · PANE ONLY. `nus theme <name>` and `nus look
ink|paper --signal` are the hot-swap from a script.


## Recovery, the assistant's other half, replay, the loop (settled 2026-09-17, nineteenth pass; built 2026-09-18)

Three flagships and the small things that make them true. Everything here
rides on what exists: blocks over OSC 133, session restore, the ports board,
the instance-port protocol, containers, rules.luau, and the compositor owning
both textures. Built in the composite spike the day after; the deltas from
the plan are in the text below.

**Cut off.** Session restore already lays the scrollback down as read-only
history. A block with a start mark and no finish mark is the command the
restart killed: a ruled line under the snapshot with one icon chip on it,
chosen by the command, its tooltip saying what it will do (*cut off at 14:02
· resume claude*). `claude …` → RESUME, which types
`claude --continue` (that cwd's latest conversation), or `claude --resume
<id>` when the newest `~/.claude/projects/<cwd>/*.jsonl` postdates the block.
`codex …` → RESUME, `codex resume --last` or `codex resume <id>` from
`~/.codex/sessions/`. A server or watcher → RUN AGAIN. `ssh` → RECONNECT.
TERMINAL · CUT OFF: CHIP (default) · RUN AGAIN · OFF. Rules: `on_cutoff(b)
→ { label, cmd }`; `assistants.<name>.resume` teaches it aider, opencode,
whatever comes next. A held shell never shows it — nothing was cut.

**Held.** A shell's pty lives in `nus-hold` (`crates/hold`), a small child
that owns the pseudoconsole — on Windows `ClosePseudoConsole` kills the
client, so the owner has to outlive the app — keeps a 4 MB ring of output,
and serves the pty over a loopback socket named in `profile/hold/<id>.json`
(port and a per-holder token). nus attaches. nus quitting, crashing or
updating leaves the holder and its process running; the next launch
reattaches and the VT core replays the ring, so the screen is what it would
have been — colours, blocks and all. TERMINAL · KEEP ALIVE: ON (default —
every shell is held; at quit an idle prompt is let go, a running command is
kept) · OFF. A shell cannot be moved into a holder after the fact, so the
choice is at spawn, not at quit. The sidebar row of a held-but-unattached shell carries a
*held* icon (tooltip: *held · claude running*); the atlas carries the same
icon with a count. Close on a running
process: CLOSE · DETACH · CANCEL, and reopen-closed reattaches. Moving a tab
to another Space is a reattach. The ports board names the held process by
its block's command. `nus hold ls · attach <id> · kill <id>`. A holder whose
child exits with nothing attached exits too. SSH profiles hold the client.

**Eyes.** `nus mcp` (in `crates/cli`) is an MCP server on stdio over the
instance protocol: `claude mcp add nus -- nus mcp`, the same for codex.
Tools: `nus_tabs` · `nus_page_info` · `nus_page_text` (the reader) ·
`nus_page_dom` · `nus_page_console` · `nus_page_network` ·
`nus_page_screenshot` (the pane's texture — the NUS_SHOT path, never the
OS) · `nus_page_open` · `nus_block` · `nus_ports` · `nus_log` · `nus_hold`.
The assistant in the shell reads the page beside it as you see it, logged
in as you. Console and network are kept per page from the moment it opens
(`Runtime` and `Network` enabled on creation), capped. Not yet: the eye on
the row, and a switch to turn eyes off.

**Hands.** `nus_page_click` (a selector, or x and y) · `nus_page_type` ·
`nus_page_scroll` · `nus_page_navigate`, by CDP on the pane. Every hand
leaves an icon chip under the URL row for a minute — click, keyboard,
caret, globe; ink when it ran, signal when it was denied or taken over —
with the words in its tooltip, and a `hand` line in the page's log. ASK
draws a band over the page: *claude wants to scroll down · ALLOW · DENY ·
ALLOW ON THIS HOST*; y/enter, n/esc, h answer it, and any other key or a
click on the page takes over: the tool is told *taken over by the user*.
ASSISTANTS · HANDS: ASK (default) · ALWAYS · NEVER · CONFIRM SUBMIT (a
submit, an Enter or a navigation asks even on an allowed host) · the
allowed hosts, forgettable. The assistant's name comes from its
environment (`CLAUDECODE`, `CODEX_*`). Not yet: the rules-file policy,
the assistant's own Space, and a hand as a block in the journal.

**Ports that remember.** `ports.json` keeps each port's last owner and the
block that started it. On restore, *was listening*: 5173 · vite · `npm run
dev` in `~/proj` — START AGAIN opens a shell there and runs it. TERMINAL ·
PORTS · REMEMBER: ON · OFF. Rules: `on_port_missing(p)`.

**Transcripts.** OPEN TRANSCRIPT on an assistant block: the session's jsonl
as a Broadsheet page beside the shell through the reader — turns, tool
calls folded, search, copy as markdown — following live while it runs.

**Journal.** One line per finished block in `profile/journal/<cwd>.jsonl`:
command, cwd, start, duration, exit, tab. `nus log`, the palette's `log`
rows, a page per folder, and the chip a cwd offers — *14 commands here this
week*. TERMINAL · JOURNAL: ON · OFF; KEEP 30 DAYS.

**Attention, named.** An assistant block is *working* (breathing lamp) or
*waiting for you* (filled signal, taskbar): the existing waiting state,
with the assistant's name in the row icon's tooltip — *claude · waiting for
you*. Stateful icons with tooltips, not captions: that is the rule for every
row, chip and status in this pass.

**Replay.** Ctrl+Shift+H opens a continuous, searchable history document
with a scrollable session map at the right, following Visual Studio's map
scrollbar direction. The map uses the same output lines as the document:
command boundaries are detents, failed commands have stronger ticks, and the
viewport shows exactly which portion is visible. Drag the viewport to browse;
click elsewhere in the map or use the arrows to step between commands. Wheel
and trackpad movement is continuous, with a small snap near command boundaries
at gesture end. Home/End select the first/last matching command. Search covers
commands, folders, status and output. Copy command/output never executes it.
Escape clears a search or exits the snapshot before returning to the live
shell; the live process continues while history owns keyboard input.

Every shell pane records its own asciinema v2 stream under
`profile/replay/<session>/`, including resize events and UTF-8 split across PTY
reads. Command-end markers include command, output, exit status, folder and
duration. Existing left-pane casts keep their names; old markers reconstruct
output from the recording. The terminal snapshot is optional; B retains the
page still's before/after/diff view. `nus share` and Playback / export create
one self-contained HTML file with the searchable document and session map,
copy controls, transcript/cast downloads, and optional paused playback with
speed and idle skipping. Recording retention remains 7 days, 1 day, or off.

**Picture in picture.** At most one PiP belongs to the nus process, including
across main windows; another source replaces the existing one. Video ownership
uses stable tab IDs. Controls offer play/pause, ten-second seek for seekable
video, mute, return, close, and smaller/larger sizes. Tab and Shift+Tab reach
controls; arrows, Space, M and +/- also work, with AccessKit actions for screen
readers. Live streams omit seeking. Hover reveals controls without taking
focus. Drag any edge or corner to resize with the aspect ratio preserved;
scroll and pinch amounts scale proportionally, without corner snapping or a
competing resize animation. The renderer checks the actual drawable size.

PiP uses a floating window. macOS uses NSFloatingWindowLevel, stays visible on
deactivation, joins Spaces as a fullscreen auxiliary, and reads NSScreen's
visibleFrame for Dock/menu-bar exclusion. Windows reads the monitor work area;
X11 intersects the current desktop's EWMH work area with the monitor. Wayland
placement and stacking remain compositor controlled, with native move/resize
requests. Native validation on Windows, Linux, mixed-scale monitors and physical
trackpad gestures is still required; macOS checks cover the local display.

**The loop.** On a localhost page, Alt+Shift+click an element → the editor
pane at its source: framework markers first (React `_debugSource`, Svelte
`__svelte_meta`, Vue `__file`, an inspector attribute), then the served
file under the folder the server runs from (the ports board's owner, else
the shell beside, else the focused cwd). BROWSER · CLICK TO SOURCE. A
`file:line` in a failing block already opens the editor. Not yet: source
maps, and the reload riding the block's lamp.

**Order.** As planned, and done in that order on 2026-09-18: cut off, ports
that remember, the journal, attention named; held; eyes, hands; replay; the
loop. What each one still lacks is named above, in place.

## Tokens for the caret and the selection, program colours, what comes first (settled 2026-09-17, twentieth pass)

**Caret and selection are tokens.** A theme carries, per face, a caret
colour and a selection colour — the ink unless it says — beside paper,
ink and page on LOOK · TOKENS, with a tray of the ink, the signal and
the brights and a FOLLOW THE INK chip to let go. The shell's selection
wash, the editor's, and both carets draw from them; the cursor rule's
INK became THE THEME'S CARET (old prefs still read), SIGNAL and THE
TAB'S OWN sit over it, and a program's OSC 12 still wins. The wash is
always 22%; the token is the colour.

**Program colours.** claude, codex and every TUI bring colours picked
against someone else's background, and land unreadable on ours.
TERMINAL · PROGRAM COLOURS: AS THEY COME · 3:1 · **4.5:1 AA** (default) ·
7:1 AAA — any text that can't be read against its paper is walked
toward white or black, the way it leans, until it reads (WCAG 2; VS
Code's terminal does the same at 4.5). TRUECOLOUR: **AS SENT** · THE
THEME'S SIXTEEN — the second snaps truecolour and the 256 to the nearest
of ours in Oklab, so a program wears the theme. Per program, in
rules.luau: `program(p)` (name, cmd, cwd, theme, ink, paper, signal)
returns `contrast`, `snap`, `ansi` (sixteen of its own), `remap` (a
colour it hardcodes → hex or ink/paper/signal/dim). The running program
is read from the pane's blocks (the first real word: past env, sudo,
npx, paths, .exe); answers are cached per program and face until the
rules reload or the theme changes. The default rules carry claude's
remap as a comment: its own colours, graded, is the default.

**Terminal first, or browser first.** STARTUP · FIRST: **TERMINAL** ·
BROWSER, and a START HERE row on the welcome page that flips it.
Terminal first is what nus has been: NEW TAB and an empty Ctrl+T open
the default shell, the palette leads with shells, links from outside
arrive in the little window. Browser first: NEW TAB and an empty Ctrl+T
open the atlas, the palette leads with the address and history, the
kinds fan out with PAGE on top, links from outside open as tabs here,
THEN is the last page. Picking one sets THEN and LINKS FROM OUTSIDE
once; after that they're yours.

**The profile.** You, on this machine: `profile/me.json` — a name, a face
(the initial in the signal, an emoji, or `profile/avatar.png`), the day it
began — and `profile/sync/device`, this machine's name, which is the one
thing that never syncs. A file in a folder is the whole account: no server
behind it, nothing counted, nothing sent, and the card says so in words.
**The card** rises from the avatar in the footer (a signal dot sits on the
avatar until the profile exists; the tooltip is *name · 12 days with nus*).
The first time it walks you through — *hello* (what this is and isn't),
name, face, this device, sync or not — and ends on *day 1, everything here
stays here*; someone who was here before the card gets the day their
profile really began (the oldest of its files). After that the card is the
profile at a glance: the face, the name, *on <device> · local*, a
calendar badge with the day count (the words in its tooltip), rows for
NAME · FACE · DEVICE · SYNC · PRIVATE (each opens its edit in place, Esc
returns), MORE for the full page, CLOSE. The full page is SETTINGS ·
PROFILE: the same rows, SINCE, the sync status with a way to SYNC
SETTINGS, PRIVATE spelled out (what the folder holds, what leaves it —
nothing, unless sync, and then sealed), OPEN THE PROFILE FOLDER, START
OVER. The welcome page's START HERE leads with it. Sync's file list is
now the real one (`settings.json`, not `prefs.json`; plus `me.json`,
containers, the blocklist, the avatar, themes and surfaces).


## Links in the shell, launch, the prompt (settled and built 2026-09-18, twenty-first pass)

**Links in the shell.** A URL in the grid — one the hint scanner finds, or
an OSC 8 hyperlink — underlines under the pointer, with the host in the
tooltip, and a plain click opens it where LINKS says pages go (a stack,
the split, a new tab). Before it opens, a band on the pane asks — *github.com
· open? · enter · esc cancels · d never ask again* — and D stops the asking
for good. TERMINAL · CLICK LINKS: ASK (default) · OPEN · HINTS ONLY (the old
way: Ctrl+Shift+O labels, nothing on click). A drag that starts on a link
still selects; a Shift- or Ctrl-click is never a link.

**Launch.** STARTUP · THEN grew: RESTORE LAST SESSION · A NEW SHELL · THE
LAST PAGE · THE PROMPT · HOME PAGE · <host> · A LAYOUT. Restore brings back
every window, not just the first: the file carries the other windows'
sessions under `windows`, each comes back as a window with its tabs, its
stacks and its container, and the shell a window is born with goes once the
session's tabs are in and it has reached its prompt. LAUNCH TABS: SET FROM
THIS WINDOW saves what is open as the layout named *launch* and points
THEN at it; CLEAR forgets it; `set this window as the launch tabs` in the
palette does the same. REMEMBER: TABS AND WINDOWS (default) · NOTHING —
nothing writes no session, and the atlas keeps only recents. The home page
is set from the palette: `home <url>`; it opens as the whole window.

**The prompt.** THE PROMPT is nus's home: a terminal with no PTY behind it.
One line, centred, a caret in signal and nothing else; the wordmark small
where the pane begins; one dim line at the foot. A URL and Enter: the pane
becomes that page. A command: the pane becomes a shell running it. Enter on
nothing: a shell. As you type, the palette's rows come up beneath the line
— tabs, recent pages and shells, layouts, history, held shells, settings —
and ↓ moves onto them; nothing is picked until you do. `home` in the
palette brings it back as a tab.

**Three directions for the home, of which the prompt is the first.**
*The prompt* (built): the app's whole thesis in one line — type, and it is
a page or a shell. *The board*: the day's front page in Broadsheet — what is
listening (ports), what ran (the journal), what is still running (held),
last time's tabs — every line a row you can open, the caret at the top; the
atlas and the ports board folded into one surface, for the person who
comes back to a machine mid-work. *The plate*: the icon's orbit drawn large
and alone, the band completing one orbit as the app boots, the ring's
stops the last places you were (a shell, a page, a folder) and the caret
appearing only when the orbit closes — for the person who wants the
launch to be a moment. The three share one rule: no captions, no buttons,
one caret, and whatever is typed goes somewhere real.

**Settled (2026-09-18): the prompt stays, the plate stays, the prompt is
the default.** THEN defaults to THE PROMPT, so the window's first tab is
the line (the birth shell is not split with a page any more when THEN
replaces the tab). The board is folded away; its rows are the prompt's rows.
The plate became a way the prompt looks rather than a home of its own:
STARTUP · HOME · THE LINE (default) · THE PLATE. Under the plate the app's
own icon — the wordmark's n with the band in orbit, the taskbar's geometry
exactly, from the same field — sits at half the pane's height above the
line, and your last places are stops on its band: a square where the band
runs clear of the n, a label beside it, at most four — shells still running
in a holder, the folders the last session's shells were in (then the
journal's), dev servers listening, the last pages. ↓ moves onto the stops
as it does onto rows; click one and you are there; type and the palette's
rows come up beneath the line as on the line alone. The band draws itself
in the first time (the motion register stretches it), and the line and the
stops come up once it closes. *The first second*: with a splash on, the
splash draws the icon where the plate keeps it and at the plate's size,
and hands over with the icon in place — only the paper over the chrome
fades; the prompt is the first tab under the same pixels. Nothing moves.
`plate.rs`; the icon's field is sampled once per size (`IconField`) so a
frame at any progress is one cheap pass, which the splash uses too.

**Applied, checked.** Passes seventeen and eighteen are in the code (every
row, hook and verb they name); nineteen is built with its gaps named in
place; twenty (theming) merged from its branch. This pass adds `links.rs`,
`home.rs` and `plate.rs`, and touches session, settings, the splash and
the palette. Still to come for the home: the pond (koi over the line, a
real page with real fish), sketched on the canvas, not built.

## Art behind the prompt, the page's menu, the settings, the third way (built 2026-09-19, twenty-second pass)

**Art.** STARTUP · HOME · THE LINE · THE PLATE · ART: an art is one Luau
file that draws behind the line, every frame, on a canvas that is the
pane. `c.prompt` is the line's box and `c.rows` how far the rows beneath
it reach; the art keeps clear of them. It sees the pointer, what is typed,
the taps on the paper, the tokens (paper, ink, signal, dim, tint, the six
promo signals), the clock, the place, and the kernel's processes; it draws
with rects, circles, ovals, lines, curves, quads, polygons, blobs and text.
Four ship, as files too: *the pond* (four koi from a bending spine, a real
width profile, translucent fins, patches that follow the bend; they keep
off the line, gather when you type, shy from the pointer, and a tap
ripples), *memphis* (the promo's kit — squiggle, zigzag, stripes, dots,
solids with the 2px line and the 8×8 shadow, the sticker), *space* (the
sky over this machine now, zenith at the centre, north up, east left as it
is lying back: the bright northern catalogue by magnitude, the figures in
hairlines, the names in small caps, sidereal time from the clock and the
place, meteors on a tap; the sky fades around the line), *the brain* (every
process as a row of dim type in three trees — the system, nus's own, the
desktop — hairlines parent to child, a pulse down the connector when one
wakes, the kernel's counters along the top; hover a row and its ancestry
lights, type and the names come forward). The picker on the STARTUP page
runs each art live in a card at a pane's size and shrinks it in (type too
small to read is greeked); ADD YOUR OWN writes the blank into
`profile/art/` and opens it in the editor — save and it redraws; ASK FOR
ONE sends the canvas doc to the assistant and the ```luau block it
answers with lands in the picker; OPEN THE FOLDER; PLACE sets lat, lon for
the sky (unset until the user chooses it; no clock or location inference). A file in `profile/art/`
named like a built-in replaces it. The renderer grew a polygon for this:
any simple polygon filled from the signed distance to its outline, one
anti-aliased edge and no seams inside however translucent.

**Tabs opened by others, the copy chord, the page's menu.** TABS · OPENED
BY OTHERS · BEHIND · WITH A TOAST (default) · IN FRONT: a tab a shell, an
assistant's hands, a rule or another app opens lands behind, and a slip
rises at the foot — *opened behind · host · click to go*. Ctrl+Shift+C on
a page copies its url with a toast (the shell keeps its own chord). The
page's right-click is nus's own menu — media first (SAVE VIDEO AS, SAVE
IMAGE AS, COPY THE ADDRESS, PICTURE IN PICTURE), then the link, then the
page (back · forward · reload · copy · view source) — and a download icon
in the page's strip when there is media on the page; saving is Chromium's
save-as, direct media only, into ~/Downloads with the footer's download row.

**Settings, every page.** Prose wraps; chips and buttons wrap when the
column is narrow; long pages read in parts under small captions; SOUND's
events are a speaker toggle, the cue's name (click to hear it) and a
caret; KEYS shows chords as keycaps; the nav has every section (KEYS,
SYNC, PROFILE and UPDATES under YOU) and tightens when the window is
short.

**The third way.** The profile lives one of three ways, chosen on the
card (the avatar in the footer, or PROFILE · HOW IT LIVES): *here* — a
folder on this machine, nothing leaves; *a folder you sync* — iCloud,
OneDrive, Dropbox, Syncthing, a stick, sealed with a key you copy; *a
private repo on a forge* — GitHub (sign in from the card with a code, when
nus has a GitHub app id, or paste a token with repo scope), Forgejo, Gitea
or GitLab (the instance and a token). nus finds or makes `nus-profile`,
private, and the git carrier does the rest; the token stays in
`profile/sync/forge.token`, goes to the forge as a header per git command,
and is never in a url, in git's config or on the carrier. The key step
makes the word on the first device (COPY) or takes it on the next. Same
sealing, same last-writer-wins. See docs/SYNC.md.

## The window as a workspace: FILES, and a new window of its own (settled and built 2026-09-19, twenty-third pass)

**A window may be a folder's.** Bound to a folder it is that folder's
window: the folder's name in the sidebar's head and the strip, the tree
of it under FILES, new shells born in it (a shell already inside it keeps
its own place), its own slot in the session (`folder`). Unbound, a window
follows the focused shell's folder. Binding is a click on the tree's head
(the pin) or a folder picked in a fresh window; `Action::Workspace`
does it from the palette. The other windows' folders are offered wherever
folders are.

**FILES.** The sidebar's second page — the folder in the footer, or
Ctrl+Shift+E — takes the list from under the header to the footer: the
folder's name pinned or following at the top with an up-caret, then the
tree: folders first, then files, dot-files and the heavy folders
(node_modules, target, .git, dist, build, .venv…) dim and closed until
asked. A click on a folder opens it; a click on a file opens it in nus's
editor beside the shell as the preview, which the next click replaces
unless it was edited; a double-click keeps it. Files that aren't text go
to the OS. The tree re-reads its folders every few seconds while shown.
`files.rs`; `SidePage`; `Tree`.

**A new window is a surface of its own.** Never a copy of the one that
asked: no session restore, its own name and number, and what it comes up
as is STARTUP · A NEW WINDOW: THE PROMPT · FOLDERS IN ITS ROWS (default) —
the line, empty, its rows the folders it could be (the other windows',
the last sessions' shells', the journal's; a typed path first), pick one
or press Enter on a path and the window becomes that folder's, a shell
born there where the prompt was and FILES on its tree; A SHELL IN THIS
WINDOW'S FOLDER — a shell born in the asking window's folder; AS LAUNCH —
the splash and THEN as the first window, but never the session. The
photograph driver runs a second script (`-Script2`, `NUS_SHOT2`) in
windows the first one opens.

## The review against the future (begun 2026-09-19)

> "We have a very bad tendency to base our plans for computers on the
> equipment we have in house and the things we're doing now, and totally
> fail to review them in the light of the equipment that will be
> available and the things that we will be doing." — Grace Hopper

nus is exposed on both halves. This section is the standing review: an
assumptions ledger every pass adds a line to, the items that never leave
the checklist, and the programme of work that came out of the first
review. The rule from here: **every pass ends with an ASSUMED line** —
the screen, the input, the engine, the network, the model's latency, and
*who is typing* — so the blind spot is a list rather than a mood.

### The ledger, passes nineteen to twenty-three

- **Screen**: one laptop, 1280×800 at 1.25×; "narrow" is 900px; tiles at
  phone width were drawn but never held. Never an ultra-wide, never a
  phone driving a shell.
- **Input**: a mouse. Hover carries the footer verbs, the tree's head, the
  toasts' affordance and every tooltip. No touch, no pen, no voice.
- **Engine**: CEF is *the* browser; `BrowserTab` is nearly an interface
  but nothing has ever been its second implementation. The PTY is local.
- **Platform**: Windows. `procs.rs` read the kernel only on Windows (now:
  `ps` elsewhere); the login item is a Startup-folder shortcut; the build
  wants Ninja on PATH; paths were being turned to backslashes (fixed).
- **Place**: the sky faced south (now: the equator); the star catalogue is
  northern; location was once guessed from the clock's zone (now: explicitly chosen, or unset).
- **Assistants**: `backends()` was a roll call of 2026's CLIs (now: a
  declared list comes first); the panel assumes a cloud round-trip.
- **Power**: the arts redrew at 60fps whether or not anyone looked (now:
  still when unfocused, every other frame on battery); the debug build
  pegged a core.
- **Who is typing**: a person, at one shell, supervising an assistant in a
  panel beside it. The blocks, journal, replay, cut-off, hands and held
  shells are the primitives of a *supervised agent*; the tabs above them
  are a person's.
- **What a workspace is**: a folder. Sync is last-writer-wins per file (now:
  `memory.md` merges as a union).
- **Output**: a grid of monospace cells; a command's output is a stream
  that blocks only begin to treat as a thing.

### Standing items

1. **Three machines, not one**: the laptop in hand; a phone with only
   `nus` remote control and the little window; a cloud sandbox with no
   local files. A feature that makes no sense on two of the three is the
   review.
2. **The agent-first read**: for every feature, "if an agent is the one
   using this, what does it need?" — structured output, a permission, a
   replay. Done means those three, next to the photographs.
3. **Two of everything at the seams**: a second browser behind
   `BrowserTab`, a second PTY kind (a sandbox), a second model backend
   (local, ambient). Not for the product — to keep the interfaces honest.
4. **A power budget and a touch pass** next to "no clipping" and "both
   faces": `power.rs` is the budget; the touch pass is the next review.

### The programme

Done in this pass (no UI): `power.rs` (the arts' budget: still when the
window is not looked at, every other frame on battery — Windows, Linux
and macOS know their battery); `procs.rs` samples through `ps` where the
kernel is not Windows; `profile/assistants.json` declares assistants
ahead of the roll call (`{"name","command"}`, the prompt on stdin, the
answer on stdout); `memory.md` merges as the union of lines in sync;
the sky faces the equator; typed paths stay as typed.

Asked, not assumed (UI): the touch model; where the agents' front page
lives; what a structured block is first; where an ambient local model
acts; what a workspace is beyond a folder; the phone's nus. The answers
and what they became are recorded under the passes that build them.

Planned, with what each needs: a second browser engine behind
`BrowserTab` (WebView2 on Windows, or a headless one for agents: the
seam is the shared-texture paint, the context menu, downloads and the
DevTools bridge); a second PTY kind (an ssh or sandbox session that
holds, journals and replays like a local one: `nus_pty::Profile` grows a
`where`); a model backend that streams (the panel shows words as they
come, which the ambient uses too).

## The review's first pass at the UI (settled and built 2026-09-19, twenty-fourth pass)

Asked, answered, built:

- **Touch: long-press is hover.** A finger that lands moves the pointer,
  so everything that follows the pointer follows the finger; held a
  beat (350ms) it is a hover — the tooltip comes, nothing acts; lifted
  quickly it is a tap, a click; moved past the slop it drags. Once a
  touch has been seen, hit targets grow toward 44px — the sidebar, the
  settings, the card, the prompt's rows. The mouse is untouched.
  `touch.rs`.
- **The agents' front page lives in the prompt's rows.** WHILE YOU WERE
  AWAY — five minutes unfocused, or since the last session was saved —
  what failed, what ran long, what still runs (here and held), who asks
  for hands; each row opens the shell it names; acting or Esc dismisses.
  `news.rs`; `journal::since`.
- **A diff in a block is a diff.** Chips on every `@@` line: STAGE and
  REVERT for `git diff`, UNSTAGE for `--cached`, APPLY for any other
  diff (an agent's patch, `cat x.patch`); one hunk through `git apply`
  in the shell's folder; the outcome a toast, the output untouched.
  `diffs.rs`.
- **The ask panel gets a LOCAL mode.** The head names the backend and
  cycles the ones on the machine on a click — LOCAL · <name> for a
  declared assistant or ollama; ASSISTANTS · ASK WITH sets it. Answers
  stream line by line, so a local model reads as instant.
- **A workspace is a folder, local or remote.** Settled: the unit stays a
  folder; it may be on an ssh host or in a sandbox, read over the shell's
  own connection. The local half is FILES; the remote half waits on the
  second PTY kind (the seam is in the ledger).
- **The phone's nus is a page the window serves.** SYNC · THE PHONE: this
  window on the LAN — the front page (what ran and failed while you were
  away, what is listening, hands to allow or deny, the tabs) and a line
  to ask; a token in the address, https with a certificate made for that
  session alone, this network only; the address in `profile/phone` and on
  the clipboard when it turns on, with the certificate's fingerprint shown
  beside the switch because the phone will ask about it once. Everything it
  knows comes through the instance port's request channel, and the phone's
  door allows only `front` and `hands-answer` — asked for anything else, it
  refuses rather than trusting the caller to have sent the right verb.
  `phone.rs`.
- **The power budget stays a policy**, not a control.

ASSUMED: one laptop and one phone on one network; a mouse with a touch
screen imagined, not held; git on PATH; the shell's own colours for a
diff's + and −; an answer that fits one screen of the phone.

## Startup settings (revised 2026-09-19)

Startup and New tab share four destinations: **Prompt palette**, **Home page**,
**Custom layout**, and **The last page**. Cmd+T on macOS (Ctrl+Shift+T elsewhere),
the header button, and the sidebar button all use this selection. A custom
layout adds its saved tabs and panes without replacing existing work. A missing
layout or absent recent page falls back to the prompt. The home address and
saved layout are editable from Startup. Terminal/browser preference controls
palette ordering and external links without changing the selected start page.

**Home background** has a **Minimal** preview row (Prompt only, Logo & prompt),
followed by **Art** previews. Artwork actions have captions: Add your own,
Ask for one, Open folder. Preview clipping and click targets stay inside the
scrolling content; selected previews have a check mark as well as a signal shadow.
The separate launch animation controls Draws in / Still / None and its duration.
The selected background applies to all Home prompts and persists across launches.
Sky and Space include visible illustrated skies without a location. An optional,
explicit location enables the local sun and star chart; it is never inferred.
The preview cards render the same artwork, with legible stars at thumbnail size.
Reduced motion shows a settled composition and freezes the sky shader's clouds.

**New window behavior**, labeled Cmd+N / Ctrl+N, offers Prompt palette,
Shell in the current window's folder, or Same as launch, each with a preview.
Saved sessions remain accessible through the atlas. Existing legacy session
restore and shell startup preferences remain readable.

Native regression checks: build the macOS bundle, then run
`python3 scripts/check-startup.py dist/nus.app`. The checks use temporary profiles
and local pages, assert destinations and tab counts, exercise all new-window
modes, and capture narrow/wide settings in paper and ink for visual review.

## Settings and onboarding (revised 2026-09-19)

Sidebar, Tabs, Terminal, Browser, Ports, Hatch, Assistants, Rules, Sync and
Profile use captioned visual choices and actions. Independent switches have
separate On / Off cards. Action buttons do not masquerade as selected values.
Keys remain readable keycaps; unavailable features are described as unavailable.
Terminal settings are grouped by shells, command editing, clipboard/scrolling,
history/replay, and colours/progress. Sliders support dragging and accessibility
value changes. Browser smooth scrolling explicitly requires a restart; replay
recording and phone access change immediately. Disabling phone access revokes
its URL and closes the listener.

Preferences merge only fields changed by a window, so a stale window saving its
size cannot undo another window's settings. Open windows adopt shared changes.
Manual light/dark appearance and the selected preset name survive relaunch.

First launch opens exactly one welcome tab, with profile setup over it. Its
links open the existing profile editor, prompt, theme studio, Startup, Settings
and keyboard guide. A page-based launch never creates a temporary held shell.
Existing saved launch choices are preserved. Onboarding uses the existing
`assets/art/memphis.luau` composition and renderer, profile face, theme card,
startup previews and Phosphor icons. Memphis entry settles after three seconds;
pointer interaction and entry respect Reduce Motion, including the macOS setting.
The welcome introduction appears once. Optional tour progress remains saved,
but unfinished exercises do not replace the selected start page on relaunch.
Remembering a session does not reopen its tabs automatically; a selected custom
layout opens only its own saved tabs.

Run `python3 scripts/check-settings.py dist/nus.app` for isolated native checks
of selection/persistence, sliders, shared windows, phone shutdown, theme
relaunch, profile onboarding and narrow/wide screenshots. Run the Startup
regressions as well when changing initial pane creation.


## Settings discovery and window polish (2026-09-19)

The footer theme picker has three columns and three visible rows. Its viewport
clips previews and click targets; additional chosen themes scroll below those
nine slots. Look → Presets → Footer theme slots lets users add or remove stock,
ported and saved themes, or restore the curated default nine.

Every settings page exposes Search settings (Cmd+F on macOS, Ctrl+F elsewhere).
The search index is built from the rendered controls, option captions and nearby
explanations, including all five Look tabs. It ranks exact labels, then prefixes,
feature synonyms and single-character typos/transpositions; all query words must
match. Results identify their section and navigate to the actual row with a
brief highlight. An unmatched query says so instead of returning random rows.

Location is optional and explicitly configured in Startup. Clearing it removes
coordinates from the art environment. Sky and star artwork asks for a location
until one is chosen. Neither the time zone nor any network lookup infers it.

The compositor clips the entire window to the carapace radius, including chrome
and web content, and clears the outside corners to transparent. macOS traffic
lights sit beside the wordmark, without adding a title bar. They are composited
in the same frame as the shell and call the normal window actions.

Onboarding distributes the existing Memphis Luau vector pieces around the
introduction and guide sections. They are individual polygons, lines and circles,
not a raster image or a miniature composition. Hover and tap reactions respect
Reduce Motion; the profile setup remains part of first launch.

Look → Type & Motion has independent interface and terminal family/weight choices.
Bundled families: IBM Plex Mono, Victor Mono, JetBrains Mono, ABC Areal, ABC Areal
Semi Mono and ABC Areal Mono. Terminal choices include only fixed-width families.
Regular (400), Medium (500) and Bold (700) load the corresponding supplied files;
preferences persist across launches and windows. Interface emphasis uses the
family's real Bold face. Areal files and licensing terms came from the supplied
ABCAreal.zip; upstream Victor Mono and JetBrains Mono licenses are retained under
assets/fonts. No system font installation is required.

Run scripts/check-refinements.py against the built macOS app for search ranking
and reveal, location clearing, font selection/persistence, footer containment and
scrolling, onboarding and rounded-corner screenshots.


## Hatch: ongoing work (revised 2026-09-19)

This direction replaces the sixteenth-pass Hatch interaction above. Hatch opens
an overview of real terminal work across all open Spaces. Rows show running,
finished, failed, needs attention, or needs input, with the session name, Space,
folder, and exit status. Selecting a row opens the original pane and PTY; it
does not re-run its command. Work and Terminal switch views within the same
surface. + Shell explicitly creates a shell. The summon shortcut resumes the
last view and session across Spaces in the default One for all mode; opening
the overview by itself creates no shell. Existing explicit Follow preferences
remain available for people who want the focused Space to choose Hatch. Existing Hatch
tabs and the HOIST/LAND commands remain valid.

Dropdown unfolds from the actual camera housing on notched Macs. AppKit
`NSScreen.safeAreaInsets` and both auxiliary top areas determine the housing
height, width, and center; `visibleFrame` is only the non-notched fallback.
A passive black island joins the hardware cutout at the top of `screen.frame`;
its side indicators remain outside the camera exclusion. The dropdown sits at
status-window level and reveals downward from the housing without moving text
through the cutout. Bundle safe-area compatibility mode is explicitly off.
On other displays it unfolds from the top center. Modal floats in the center. Presentation and height changes preserve
session identity. Pin prevents dismissal on focus loss; Hide and the summon
shortcut always dismiss. Escape reaches the terminal in Terminal view and
dismisses the overview in Work view. Arrow keys and Tab select work, Enter
opens it, Ctrl+N creates a shell, and Ctrl+P pins from Work. Ctrl+Shift+O returns
to Work, Ctrl+Shift+Down expands the session into nus, and Ctrl+Shift+Up pins.

Hatch settings add Compact work status (on), Keep nus in background (on),
Completion notices (off), and Dim behind modal (off). Closing a main window
with background enabled hides its surface but retains its live session owner.
Quit nus deliberately exits all windows and processes. macOS and Windows have
a native NSStatusItem/menu or tray entry for Work, Show/hide, Open nus window, and Quit. The optional
compact top-edge status is clickable. Optional six-second completion notices
use that surface and open their specific session when clicked. Neither status
changes nor completion notices activate a window. Status can be disabled
independently of notices. This is an in-app notice, not notification-center
integration.

Desktop identity uses the existing orbit geometry with a pure-white n, a dark
contrast edge and shadow, and the lower letter clipped beneath the orbit.
The macOS Dock and menu bar follow the last-focused window's signal colour;
completion notices use the same mark. The Dock loops the promo's Plex,
Silkscreen, Plex Italic, Bungee, Rubik Mono and Newsreader sequence while
launch is pending. Embedded clipped frames install before shell discovery,
CEF and window creation; native launch completion or the first rendered
window settles on Newsreader, whichever is observed first.
Launch does not request an additional attention bounce. macOS controls the
physical bounce timing; AppKit exposes no per-bounce timing callback. An
informational attention request loops the lettering three times (about 1.8
seconds), then cancels the request and settles on Newsreader. Focus and Reduce
Motion stop it immediately. Bursts coalesce; there is no repeated request or
focus stealing. Completion and new explicit input/attention signals follow the
existing optional Completion notices setting. Restored snapshots stay quiet. Quit cancels any remaining icon animation and
restores AppKit's packaged default before window and browser teardown, through
both the native Quit callback and normal app cleanup. Native
checks compare that restored image and the post-exit system icon against the
clipped artwork; the legacy protruding letter is a negative comparison.

The signed macOS app and CEF helper bundles include the same multi-resolution
icon for Stage Manager, Finder and system notification identity. Packaging
names that resource by its content and refreshes Launch Services registration
so new artwork replaces the prior bundle icon reference. Those system
surfaces use the packaged red orbit; live theme changes and font animation are
limited to surfaces the app controls. Runtime Finder custom-icon metadata is
deliberately avoided because it fails strict code-signature verification.
Linux keeps the themed X11 window icon and publishes a matching per-user
desktop-entry icon for Wayland launchers; launcher refresh timing belongs to
the desktop environment. Windows flashes the taskbar and X11 raises urgency;
both use the same bounded font loop through runtime window icons. Wayland uses
the compositor attention request where supported, with the static desktop icon:
winit 0.30 cannot set a Wayland window icon. No animation writes to application
resources or rewrites desktop entries per frame. Windows/Linux behavior still
needs native desktop validation; compile checks do not establish launcher behavior.

Status evidence comes from existing OSC 133 shell marks, exit codes, progress,
and explicit attention signals. A bell during work means Needs attention;
only an actual nus confirmation means Needs input. Agent processes use those
same signals. Silence is never interpreted as a request for input. Unsupported
shell integration cannot supply running/completion status. Already restored
history never triggers a completion notice. Updates reuse cached command data
and do not rescan terminal scrollback on every status poll.

CLI additions preserve existing commands:

```text
nus hatch work
nus hatch list
nus hatch open --window WINDOW_ID --tab-id TAB_ID [--right]
nus hatch quit
```

The list provides stable window/tab/pane targets; closed or stale targets are
rejected. `toggle`, `show`, `hide`, `hoist`, and `land` retain their spelling.
No directory change or command execution occurs on summon or selection.

Platform integration: Windows retains RegisterHotKey and adds tray access and
foreground-window restoration. macOS uses registered Carbon shortcuts without
Accessibility permission, restores the previous application, and joins Spaces
with fullscreen-auxiliary window behavior. Linux X11 uses global-hotkey; the
compositor controls final focus/placement. Wayland users must bind
`nus hatch toggle` through their desktop shortcut settings; global positioning
and focus restoration are not promised. Linux uses a StatusNotifierItem tray
where the desktop provides a host, with the in-app drawer as a fallback. Launch at
login on macOS/Linux, dedicated per-session global shortcuts, and richer agent-specific
request-for-input protocols remain future work.

Validation: `python3 scripts/check-hatch.py /path/to/nus.app` uses isolated
profiles and real PTYs to check status, handoff identity, presentation changes,
pinning/dismissal, background lifetime, cross-Space selection, passive notices,
and narrow Paper/Ink rendering. Native fullscreen desktops, mixed-scale
monitors, Windows/Linux behavior, and p95 warm keyboard readiness under 100 ms
require reference-hardware validation; no latency claim follows from animation
duration or these functional tests.

## Downloads and resizable chrome (2026-09-19)

The footer Downloads icon shows active status; hovering reveals filenames and
progress without leaving the page. Clicking opens the Ledger modal: quiet
monospaced rows, thin separators, a signal-colored progress line only for active
transfers, and compact pause/resume, cancel, retry, open, source and reveal
controls. All downloads opens a dedicated tab using the same renderer, spacing,
typography and actions; Cmd+J
on macOS or Ctrl+J elsewhere and the command palette open that page directly.
Completed, cancelled and interrupted transfers remain in local downloads.json.
Clear finished removes history entries and leaves downloaded files in place.
Transfers interrupted by quitting are labelled honestly on the next launch.
The full page searches filenames, original names, sites, types and statuses.
Cmd/Ctrl+F focuses search; Tab walks controls; Page Up/Down and Home/End browse
history. Escape clears search or dismisses the modal. Search supports Unicode,
clipboard and keyboard editing, and exposes its value to accessibility. Narrow
rows move controls below the details. Retry retains the original browser container and its cookie jar; the source
action returns to the recorded originating page in that container when available.

Browser settings → File naming defaults to Off, preserving supplied filenames.
All downloads uses the originating page's title with the original extension.
Selective (beta) permits readable names for documents, images and media but
preserves technical, unknown, versioned and checksum-like names. Missing or
generic titles retain the original filename. Names are sanitized and concurrent
or existing filename collisions receive a numeric suffix, without replacing files.

Drag the sidebar's inner edge to resize it, or the footer's top rule to change
its row height. Settings → Sidebar exposes both dimensions. Below 105 logical
pixels, the tab list and file tree show icons with title/path tooltips; choose
kind icons, favicons, or web previews in Settings. Footer actions wrap into
rows; Downloads and Settings remain reachable in the narrow column. Dimensions
and the small-sidebar style persist and update other windows through preferences.

Pane headers share a 32px logical height, and browser/editor footers share 32px.
Layout edges are rounded once in physical pixels; hidden DevTools reserves no
separator. Traffic lights are 12px (10px in narrow windows), with hover glyphs
and full-height click targets. Initial shell rendering happens before the main
window becomes visible.

Run scripts/check-downloads-sidebar.py for local CEF transfers, naming and
collision handling, pause/resume/cancel/retry, search, history clearing without
file deletion, Ledger in wide/narrow Paper/Ink, sidebar drags and screenshots,
plus terminal/browser pane-boundary checks. It uses temporary profiles and saves
all test downloads there, leaving the user's Downloads folder untouched.

## Assistants, typography, and the shared prompt (2026-09-19)

Settings → Assistants is a CLI connection and session hub for Claude, Codex and
Ollama. Connections checks installed executables, CLI versions, sign-in presence
and Ollama service/model availability without sending a model prompt. It does
not claim that a paid request, a selected model, or every tool is working merely
because an account is present. Checks are asynchronous, bounded, and omit account
output. Explicit executable overrides fail visibly; a selected missing provider
is not silently replaced by another provider. macOS app launch includes the
bundled nus command and ~/.local/bin as well as the login shell's PATH.

Work opens editable task, planning and review prompts and resumes actual terminal
sessions. Every assistant draft shows its command and working folder before
starting a persistent shell session. Long commands wrap and the review scrolls
inside narrow windows. Prompt text is quoted as a literal argument;
Ollama requires a chosen model. These sessions retain the CLI's own instructions,
account and permission controls and can move into Hatch. Context configures nus
Ask attachments and nus browser-tool access separately. Advanced provides custom
executable paths, diagnostics, instruction-file editing, and staged MCP setup
commands. MCP registration is checked, but is not advertised as proof of a
successful tool call. Registration commands wait in a terminal for review and
Enter. The bundled CLI provides `nus mcp`; nus never rewrites provider accounts.
The existing one-shot Ask uses configured executable/model settings, surfaces
process errors, and limits runtime. It is separate from interactive CLI sessions.

Settings → Prompt offers Shell, Web, Assistants, Mixed and Minimal starting
points. Home and the Go palette share routing and ordered sources: saved
shortcuts, projects, sessions, shell history, browser history, assistants,
attention items, layouts, app actions and settings. Each source has independent
idle/search visibility and a result limit. Users can reorder sources, adjust
width, density and vertical placement, hide route hints, set total result limits,
and save/remove shortcuts. Presets preserve saved shortcuts. Sources with no
results take no space. Suggestions briefly cache filesystem-backed data so
animated Home backgrounds do not repeatedly scan history and project folders.

The default Enter destination is configurable. Explicit `> command`, `? search`,
`@claude prompt`, `@codex prompt` and `@ollama prompt` remain available regardless
of hidden sources. Assistant routes always open a review. A bare `home` opens the
prompt and its selected background; `home URL` preserves the existing startup
website setting. Startup distinguishes Home/prompt from Website. Background
Welcome hit targets cannot intercept settings clicks. All settings content,
including navigation actions inside a page, is clipped to the visible region.

Settings → Fonts has curated pairings and a live specimen, with independent UI,
terminal and editor families, weights and sizes; terminal/editor line spacing;
and terminal column spacing. It supports bundled and installed fonts, restricts
terminal/editor choices to monospaced faces, persists changes and updates live
terminal grid metrics. Settings controls wrap in narrow windows and support Tab,
Shift+Tab, Enter/Space and Page Up/Down. Reduced motion samples a completed Home
art composition instead of leaving artwork at an empty entrance frame.

Validation: `scripts/check-workspace-router.py /path/to/nus.app` uses isolated
profiles and explicitly simulated assistant executables. Native pointer checks
cover all 18 settings sections with Welcome in a background tab, shared routing,
real PTY launches with literal prompts, preference persistence, terminal font
metrics and narrow Paper/Ink layouts. Every requested screenshot must be saved
for the test to pass. Unit checks cover presets, source normalization, routing,
quoting, explicit executable failure and bundled font contracts. These checks do
not prove live cloud model responses, provider MCP tool calls, or Windows/Linux
runtime behavior; those require separate platform and account validation.

## Signal icons and a modular menu drawer (2026-09-19)

Signal is the menu-bar/tray entry point. A single drawer combines compact work
rows with expanded Desk cards; these are independent section choices, not
mutually exclusive modes. Settings → Desktop → Menu & Tray controls the icon
(dot, count, or short status), section order, and Hidden/Compact/Expanded for
Work, Downloads, and Quick actions. The default is compact Work, expanded
Downloads, and quick-action tiles. Names and finished items can each be hidden.
Choices persist and synchronize across nus windows. The footer always opens
the same drawer, including when the native icon is disabled or unavailable.

Work rows target the original window, tab, and pane. Opening the drawer creates
no terminal. Downloads use the real Chromium queue, with live progress and
Pause/Resume/Show controls in expanded cards. Quick actions open the configured
new tab, a terminal, a window, or search. Open nus, Customize, and Quit stay in
the footer. Escape, the close button, a second icon click, or clicking outside
dismisses the drawer. Keyboard navigation scrolls focused actions into view;
the drawer exposes its buttons to platform accessibility.

The existing white n and authored orbit remain the icon, with a still activity,
attention, or count badge. Its orbit follows the theme. macOS can place count
or status text beside the icon; Windows and Linux use badges and tooltips.
Linux's StatusNotifierItem runs off the UI thread, waits for a tray host, and
retries an unavailable session bus. Wayland focus and placement remain under
the compositor's control. Right-click exposes a small native fallback menu.

Validation: `scripts/check-menu-drawer.py /path/to/nus.app` exercises isolated
profiles, native setting controls and footer hit targets, preference reload,
mixed sections, hidden names, real PTY selection, a real local Chromium
transfer, and pause/resume. Unit checks cover config normalization, status
classification, privacy, display bounds, and Linux ARGB icon transport. Native
Windows/Linux tray hosts and mixed-scale monitor behavior need platform QA.

## Closing is terminating (2026-09-19)

Closing a tab, a shell, a split or a whole stack ends what it was running.
Not just the shell: **the whole process tree under it**. Killing the shell
alone is not enough — the pty closing sends SIGHUP to the *foreground*
process group, so a command still in front dies with it, but anything
backgrounded, daemonised or deaf to SIGHUP survives, reparented to init and
still holding its port. nus leaves none of those behind.

- `nus_pty::ports` reads the process table once, walks it leaves-first, and
  signals: SIGTERM / `taskkill` first, a 120 ms breath only when there were
  children, then outright. `Pty::kill` does this, and `Drop` stands in for it
  when a pane is simply dropped, so no path can orphan a shell.
- Closing a stack reaps in one pass: one reading of the table, one grace
  period, however many tabs.
- A **held** shell is the holder's. Closing its tab ends it on purpose; the
  holder runs the same reap its side. Detach (`D` on the close band) and quit
  still let a working held shell go on living — that is what holding is for.
- At quit: `release_idle_held` lets idle held shells go and keeps the working
  ones; every local shell is then reaped.

**Asking first** is SETTINGS · TERMINAL · CLOSING (ASK WHEN BUSY, the default; NEVER ASK), and it asks about real work
only: a shell sitting at its prompt with nothing under it closes without a
word; a shell running something names it and asks, in either half of a split.
Off, closing is silent — but it still terminates. There is no setting that
leaves strays.

**The ×.** Every closable thing carries one on hover: sidebar rows (expanded
and the 48px compact column) and the ports board's rows, where it stops that
process and everything under it. The board's kill keeps its own three-way
`ports.kill_confirm` (always / never / system only).

## Route keys, and discoverability as a quality bar (2026-09-19)

The foot of Home is four marks, not a sentence: a shell, a page, an
assistant, the rows. The one Enter would take is lit in the signal with a
rule under it; a fifth mark appears when the line names a folder. The pointer
names each one, and a click puts its prefix on the line — so the row teaches
the typing instead of describing it. `prompt.hints` (SETTINGS · PROMPT ·
LAYOUT · ROUTE KEYS) turns it off for a bare line.

Discoverability — what is bundled, what commands exist, what a route does —
is held to a **premium quality bar, not a paywall**. nus is free, forever;
nothing here is ever gated behind a purchase. The bar is: show it as a mark
before a sentence, show it where the thing is used, and let acting on it be
one click.

## The face is a file you pick (2026-09-19)

A profile picture is browsed for, on every platform — NSOpenPanel, the common
item dialog, the desktop portal — from the profile card and from SETTINGS ·
PROFILE. Whatever you pick (PNG, JPEG, WebP, GIF, BMP, TIFF, ICO) is squared
off from the middle at 256px and written as `profile/avatar.png`; the file you
picked stays where it is. Dropping a PNG in the folder by hand still works.

The panel goes up from the main thread — macOS allows no other, and `rfd`
panics under winit's pump-events loop if asked from one — and is modal while
it is up, like every other app's open dialog. nus does not park a thread on
it: the answer is asked for a frame at a time from `tick`, so there is no
join and no ordering between the panel closing and the next frame.
