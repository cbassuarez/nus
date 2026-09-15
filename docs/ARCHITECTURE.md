# Architecture

Decisions made 2026-09-15. Each is a commitment, not a suggestion; change the
doc when you change your mind.

## Host

Rust workspace, one binary per OS. `winit` owns the window, `wgpu` owns the
surface, **we own the compositor**. Everything on screen — chrome, terminal
grids, browser tabs, PiP — is a layer in our scene graph. No platform child
windows, no webview for UI. This is what makes Wayland, PiP, tab previews and
splits uniform across OSes.

## Crates

| crate      | owns |
|------------|------|
| `vt`       | VT parser (`vte`), grid, scrollback, modes, Kitty keyboard, Kitty/Sixel graphics |
| `pty`      | shell profiles; spawns local shells, WSL distros, `ssh <host>` via `portable-pty` |
| `render`   | wgpu compositor, retained scene graph, glyph atlas (swash + rustybuzz), external texture layers |
| `ui`       | sidebar, Spaces, tabs, splits, command bar, previews, attention state |
| `browser`  | CEF host (offscreen, Chrome runtime), profiles, ad-block (`adblock`), extensions, Arc import |
| `config`   | Luau sandbox, curated API, keybinds, themes, hooks |
| `app`      | wiring, event loop, notifications |

## Terminal

Own core. `vte` for the state machine; grid/scrollback/modes ours. Reference
implementations to read, not copy: Ghostty (state design, Kitty protocols),
Alacritty (`vte` usage), WezTerm (ConPTY quirks).

## Browser

CEF, offscreen rendering, Chrome runtime. Each tab is a `CefBrowser` whose
paint output lands in a wgpu texture — shared texture where the platform allows
(D3D11 on Windows, IOSurface on macOS), CPU upload otherwise. Profiles are CEF
request contexts; a Space can pin a profile.

Ad-blocking is ours: Brave's `adblock` engine hooked into CEF's resource
request handler, so it works with zero extensions installed.

**No Chrome extensions in v1** (decided 2026-09-15 after spike 2). Windowless
browsers are Alloy style, and libcef does not attach the tab model or the
extension request proxy to Alloy WebContents: extensions load and run but
cannot see, filter, or inject into our tabs. The things extensions are used
for are built natively instead:

- ad/tracker blocking: `adblock` in the resource request handler (above)
- userscripts / userstyles: our own, injected per browser through CEF's
  DevTools protocol (`Page.addScriptToEvaluateOnNewDocument`). Covers
  Vimium-style keys, dark mode, Arc "Boosts"
- password manager: `op` (1Password) / `bw` (Bitwarden) CLIs behind a
  `nus-browser` credential provider; form detection and fill via injected JS
- profiles: CEF request contexts, one per Space if configured

Escape hatch, post-v1: build our own libcef with Alloy-style tabs wired into
the extension system. Chromium-scale build on three OSes, monthly. Tracked as
a future phase, not a v1 dependency.

PiP is ours: a tab's texture drawn into a small always-on-top window. Not
Chromium's PiP.

## Chrome UI

Hand-rolled retained scene graph on wgpu. Rects, rounded rects, text runs,
texture quads, clip, scroll. No general-purpose toolkit; the UI surface area
(sidebar, tabs, command bar, settings) doesn't justify one, and nothing else
composites external textures without friction.

## Config

Luau via `mlua`. Sandboxed: no `io`, `os`, `require` of arbitrary files, no
FFI. The config gets a curated `nus` table: keybinds, theme, profiles, and
hooks such as `on_output(tab, line)` — which is how the built-in
"localhost URL → open browser split" feature is implemented, so users can
replace it.

## Security posture

- HTML is never used for UI. The only web content is the user's browsing.
- Config is a sandboxed VM with no host access.
- Terminal → browser communication is typed messages in-process; the browser
  never gets a PTY handle, the terminal never gets a `CefBrowser`.
- CEF is pinned and bumped on Chromium's 4-week cadence. A stale CEF is a stale
  browser.

## Platform notes

- Windows: ConPTY; pwsh default profile; WSL distros enumerated as profiles.
- macOS: IOSurface shared textures; notarized builds.
- Linux: Wayland first, X11 works. Verify CEF boots under Wayland-only
  sessions (no XWayland) — SPIKES.md #1.
