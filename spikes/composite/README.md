# spike 4 — composite

The app skeleton: `nus-render` compositor, terminal + browser panes, the
Broadsheet chrome, focus routing, and the navigation model from
`docs/PRODUCT.md`. Everything keeper-shaped lives in `crates/`; this binary
is glue and will be replaced by `crates/app`.

```
source scripts/env.sh && cd spikes/composite && RUST_LOG=info cargo run
```

## Findings — Windows 11, 2026-09-16

- **Backend choice matters for zero-copy.** `Backends::PRIMARY` picked Vulkan
  on the RTX 5060 and Vulkan on Windows lacks `VK_KHR_external_memory_win32`,
  so CEF paints fell back to CPU copies. `nus-render` now forces DX12 on
  Windows, Metal on macOS, Vulkan on Linux.
- **ConPTY and resize.** winit emits a burst of sizes during window creation
  (58×35 → 29×20 → 30×21 → 58×35); conhost scrambles its buffer if it sees
  them all, so terminal resizes are debounced 80 ms. Separately, `Term::resize`
  had to adopt the xterm rule (drop blank rows below the cursor before pushing
  rows into scrollback) because conhost repaints with absolute CUPs.
- **Navigation works as specified**: Ctrl+Shift chords, go/new/url palette,
  URL-at-prompt hint and open-in-split, MRU cycling, pinned row, multi-select
  close, close confirmation on a running process, reopen-closed.
- Sidebar previews of browser tabs are the same CEF texture drawn small —
  free.

- **Pump once.** `pump()` consumes the change it reports. The loop pumped,
  then `redraw()` pumped again and saw nothing, so browser-only changes
  (first paint, video frames) never redrew until the next input event — it
  looked like slow page loads. Latch the result into `dirty`.
- **One request context.** `request_context_create_context` with default
  settings gives each tab a private in-memory cookie jar and cache. Use the
  global context (per-Space contexts with a cache_path in v1).
- **Popups → stacks.** `on_before_popup` hands the URL to the app instead of
  loading in place; the app opens it as a stack child (or split / new tab by
  rule). `window.open` from CDP needs `userGesture: true` or Chrome blocks it.
- **DX12 swapchain alpha is Opaque.** `get_capabilities().alpha_modes` is
  `[Opaque]` here, so window opacity has no effect on Windows without
  DirectComposition; the OPACITY row says so. Metal/Wayland to be checked.
- **Luau in-process.** mlua (luau, vendored) adds ~1 min to a clean build
  and sandboxes fine: `io`/`os`/`require` are absent in `sandbox(true)`.

- **Windows swapchain sizes lie mid-move** (0 or 32767); clamp to the
  device's max texture dimension or `Surface::configure` panics.
- **UI Automation is the test harness for AccessKit.** `[System.Windows.
  Automation.AutomationElement]::FromHandle` walks the tree and
  `InvokePattern` drives it; no screen reader needed to verify.
- **CDP replies by id.** `Runtime.evaluate` with `returnByValue` and a
  small `replies` queue in `Shared` is enough for extraction (reader mode);
  `window.open` from CDP needs `userGesture`.

- **Texture on a stroke.** A texture instance carries the carapace's
  radius and thickness in `uv.xy`; the shader masks the pattern to the
  rounded stroke and emits light-or-dark speckle so it reads on any ramp.
  The band is thin, so it is drawn at ×3 strength.
- **Native synth beats WebAudio here.** cuelume's recipes are a few
  hundred lines on cpal; cached buffers per cue, one output stream, no
  process. Recipes are data, so rules can name cues.
- **Custom pointers.** `winit::window::CustomCursor::from_rgba` (32×32)
  built from the theme at runtime; `set_cursor_visible(false)` on a key,
  back on the next motion.
- **AccessKit actions bypassed persistence.** `apply_setting` from the
  tree never called `save_prefs`; fixed. Test settings through UIA, not
  only the mouse.

- **Chromium's process singleton.** A second process with the same
  `root_cache_path` prints "Opening in existing browser session" and
  `initialize` returns 0 — so a second window is a second process with
  its own root cache (own cookies). Windows must be in-process for v1.
- **Glyph rotation is a vertex-shader matter.** Kind-1 instances spin
  about their centre by `phase`; no atlas re-raster. Symmetric glyphs
  (plus at 90°, an 8-tooth gear at 45°) look unmoved — pick angles that read.
- **SetCursorPos alone yields no CursorMoved** in winit; hover tests need
  relative `mouse_event` deltas (`$TEMP/hover2.ps1`).

## Not done here (v1)
- Font fallback (symbols, emoji) — `⌘`/`▸`/`↵` are boxes in Plex Mono.
- CEF popup surfaces (`<select>` dropdowns) are not composited.
- Selection / copy / paste, scrollbars, favicons, history, find.
- Space-owned browser profiles (one request context per Space; the spike uses the global one).
- Window transparency on Windows (needs a DirectComposition swapchain).
- Single instance over a named pipe / unix socket (loopback TCP here).
- Windows in one process sharing the Chromium profile (one process per window here).
- Reader mode images (captions only here) and link following.
