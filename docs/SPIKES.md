# Spikes

Throwaway binaries under `spikes/`, each its own crate, excluded from the
workspace. A spike is done when its README records the answer. Order matters:
each one can kill or reshape the design, cheapest-to-kill first.

## 1. `cef-osr` — CEF offscreen → wgpu texture, all three OSes
**Windows done 2026-09-15: shared texture via D3D11→D3D12, 144 fps.**
macOS and Linux runs still TODO — see `spikes/cef-osr/README.md`.

Boot CEF (Chrome runtime, windowless), load a page, get paint callbacks into a
wgpu texture, draw it in a winit window.
- Shared texture path works on Windows (D3D11) and macOS (IOSurface)?
- Linux: shared texture or CPU upload? Frame cost at 4K?
- Boots under a Wayland-only session?
- Which `cef-rs` version/commit; what needed patching.

## 2. `cef-ext` — extensions + native messaging under OSR
**Done 2026-09-15 on Windows — answer: no.** See `spikes/cef-osr/README.md`.
Extensions run but cannot reach Alloy-style tabs. Design amended in
ARCHITECTURE.md: native replacements in v1, own libcef build later.

On top of #1: load uBlock Origin Lite and the 1Password extension. Log in.
- MV3 extensions load and run in Chrome runtime + windowless?
- Extension popups/options pages render in OSR?
- Native messaging host reachable from the extension?
If this fails, "daily-driver browser" is redefined before any UI is written.

## 3. `vt-render` — own VT core + glyph atlas
**Windows done 2026-09-15: vim + ligatures correct, 2.77 ms mean key-event→present-call in the isolated spike,
<2 ms full-screen frames in release.** Core lives in `crates/vt` and
`crates/pty`; findings in `spikes/vt-render/README.md`. macOS/Linux TODO.

`vte` state machine, grid, PTY via `portable-pty`, swash/rustybuzz atlas.
Run `vim`, `lazygit`, `claude` at 120 fps with ligatures on all three OSes.
- Input latency (key → pixel) measured, not guessed.
- ConPTY resize/scrollback behaviour on Windows.
- Kitty keyboard protocol negotiated correctly.

## 4. `composite` — #1 + #3 in one window with a sidebar
**Windows done 2026-09-16.** Compositor in `crates/render`; navigation model
implemented; findings in `spikes/composite/README.md`. Next: move the glue into
`crates/app`/`ui`/`browser` and delete the spikes.

The skeleton of the real app. Two layers, a hand-rolled sidebar, keyboard focus
routing between terminal and browser. When this works, the code moves into
`crates/` and the spikes are deleted.

## Look and feel (before spike 4)
Direction "Broadsheet" chosen 2026-09-16 — see `docs/DESIGN.md` and the
canvas sources in `design/`. Spike 4 renders the chrome from those tokens.

## 5. `hold` — a pty that outlives the app
**Windows done 2026-09-18** in `crates/hold` and `nus_pty::hold`, with an
end-to-end test (`crates/pty/tests/hold.rs`): spawn through the holder,
detach, attach with the ring replayed, live, kill, file gone. Found: conhost
sends DSR 6 at startup and draws nothing until answered — the holder
answers when nobody is attached. Loopback + token instead of a named pipe.
Unix and job objects still to do.

The holder process from ARCHITECTURE.md, Windows first.
- Does a ConPTY owned by a child survive the parent's exit and crash? Job
  object placement so the app's death takes nothing.
- Reattach: open the pipe, replay a 4 MB ring into `nus_vt::Term`, go live.
  Latency, and does the screen match a shell that was never detached
  (cursor, modes, alt screen)?
- `claude` and `codex` specifically: an interactive session held for ten
  minutes with nobody attached, then resumed — do they notice?
- Unix: `openpty` + a supervisor; SIGHUP handling; the same pipe protocol.

## 6. `checkpoint` — what a block boundary costs
**Built 2026-09-18** as `replay.rs`: a cast per tab, a marker and a still
per block, the timeline over a scratch `Term`, before/after as a pixel
diff, share as one inline HTML file. Costs not yet measured over a working
day; MHTML and editor buffers not yet in a checkpoint.

Take the checkpoint from PRODUCT.md on every block for a working day.
- `Page.captureSnapshot` size and time on real pages (docs.rs, a Vite app,
  GitHub); texture readback time at 1600×1000 (we have `snapshot`).
- Storage per day at KEEP 1 DAY with pixels skipped on unchanged DOM hash.
- Scrub latency: replaying an 8-hour cast to an arbitrary *t*; where the
  index points go.
- Before/after: a pixel diff and a DOM diff that read as a list, on a
  real change (a CSS edit under HMR).

## 7. `hands` — CDP input on an offscreen browser, as blocks
**Built 2026-09-18** as `hands.rs` and `nus mcp`: scroll, type, navigate
and click through CDP on the OSR browser work as sent; the band, the chips,
take-over and *allow on this host* work; the MCP handshake was exercised
with a hand-rolled client (initialize, tools/list, tools/call). Not yet
tried against Claude Code or Codex themselves.

- `Input.dispatchMouseEvent` / `dispatchKeyEvent` on an OSR browser: do
  clicks land where the pane's coordinates say, at every scale factor?
- A hand as a block: open, lamp, fold; user input cancels it and the tool
  returns *taken over* — the timing of that race.
- `nus mcp` handshake with Claude Code and Codex: registration, the tool
  list, a page read and a click round-trip, and what each does with a
  refused action.
- Policy: `confirm` bands over the page while the assistant waits; ALLOW ON
  THIS HOST persists to `sites.json`.
