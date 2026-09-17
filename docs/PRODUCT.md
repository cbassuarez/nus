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

**Reader mode.** Ctrl+Shift+R or the book: the article, extracted in the
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
