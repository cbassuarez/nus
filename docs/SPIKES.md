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
**Windows done 2026-09-15: vim + ligatures correct, 2.8 ms key→present,
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
