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

## Holder

`nus-hold` (`crates/hold`) is one process per held shell: it spawns the
shell through `nus_pty::Pty` — the same pty code the app uses — keeps a ring
of the last 4 MB it has read, and serves the pty's bytes both ways over a
loopback socket to one client at a time. `profile/hold/<id>.json` names it:
port, a per-launch token, the holder's pid and the shell's. The frames are
`[tag][len u32][payload]` (`nus_pty::hold`): the client sends bytes,
resizes and kill; the holder sends a greeting, the ring once, then output
and the exit code. The app is a client; the VT core stays in the app.
Detach is closing the socket; attach is opening it and taking the ring,
which the core replays before going live. The holder outlives the app on
purpose — `ClosePseudoConsole` terminates the client, so whoever owns the
pseudoconsole decides whether `claude` survives a restart — and is started
with its own process group and no console. It exits when its child does, or
on kill. One thing learned in spike 5: ConPTY's conhost asks the terminal
where the cursor is (DSR 6) and draws nothing until it hears back, so the
holder answers when no client is attached and keeps the ask out of the
ring.

## Assistant bridge

Eyes and hands are the instance protocol (`remote.rs`) behind `nus mcp`,
an MCP server on stdio in `crates/cli` that maps tools to verbs. A verb
whose answer waits on the page — a CDP reply, a capture on the next draw —
is parked in `App::deferred` and answered when it comes; hands are parked
in the pane's band until you answer. Reading a page is the reader pipeline
(CDP `Runtime.evaluate` with `returnByValue`) and, for pixels, the tab's
texture through the render crate's `snapshot` — the OS screen is never
read. Console and network are CDP events kept per page in `Shared::log`.
Hands are CDP `Input.dispatchMouseEvent` / `Input.insertText` /
`Input.dispatchKeyEvent` and the tab's own `load`; each is checked against
the HANDS setting and the allowed hosts before it runs. User input on that
pane while a hand waits cancels it and answers the tool *taken over*. The
terminal never gets a `CefBrowser` and the browser never gets a pty handle:
the bridge is typed messages, as before.

## Checkpoints and replay

A checkpoint is taken at each block boundary for the tab the block is in:
the cast segment since the last checkpoint (bytes and timings), the page
beside as `Page.captureSnapshot` (MHTML) plus URL and scroll, its pixels as
the texture read back (WebP; skipped when the DOM hash is unchanged), and
each editor buffer as a diff against the last checkpoint. The store is
`profile/replay/<session>/cast.jsonl` (asciinema v2 plus a `checkpoint`
event pointing at blobs in `blobs/`), pruned by age. Scrubbing replays the
cast into a scratch `Term` up to *t* — the same core, so the picture is
exact — and swaps the page pane for the still. Sharing bundles the tab's cast, its stills and the wasm renderer the site
already uses (`nus_vt_wasm`, carried in the app's assets) into one HTML file
— module syntax stripped, the wasm and the cast inline — because a
`file://` page can fetch nothing; so a replay needs no server and no nus.

## The loop

Click-to-source on localhost pages resolves the clicked element to a file
and line in this order: a source map for the element's stylesheet or script
(fetched through CDP, never re-requested by us), a framework's debug marker
on the fibre/instance (React, Vue, Svelte), then the served file's path
under the project. The editor pane opens it; the page's tab row lights when
the dev server reloads, and the reload state rides the lamp of the block
that started the server.
