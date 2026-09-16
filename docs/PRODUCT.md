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
