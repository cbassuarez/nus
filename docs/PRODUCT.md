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
