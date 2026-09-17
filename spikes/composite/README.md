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

- **The exe icon is a resource.** `winresource` in build.rs embeds
  `assets/icon/nus.ico`; `set_window_icon` alone leaves Alt-Tab and
  Explorer on the default glyph until the window exists.
- **HSL in 0..1.** `surface::to_hsl` returns hue as a fraction of a turn;
  the picker multiplies for display only.

- **Intercept before vte.** OSC 133 / 7 / 9;4 / 1337 and the Kitty
  graphics APC are read in `Term::advance` at the byte where they occur,
  then handed on (or, for image payloads, dropped) — vte's `ansi::Handler`
  has no hook for them. Marks live in absolute lines (`Grid::history_total`).
- **PowerShell execution policy.** Dot-sourcing a script from a profile
  is blocked by default; `-NoExit -EncodedCommand <utf16le base64>` runs
  the same script and isn't subject to it. `-replace` with a lone
  backslash is an invalid regex — use `.Replace([char]92, '/')`.
- **Fallback faces behind a RefCell.** `FontSystem::shape` and `measure`
  stay `&self` while loading system faces lazily; fallback ids start at
  0x8000. Symmetric glyphs need asymmetric hover angles.
- **Chromium's process singleton** was the reason windows moved
  in-process; one profile, N `App`s, events routed by window id.
- **Tests as the harness.** nus-vt tests cover marks across chunks,
  OSC 7/9;4, Kitty chunked RGBA with cursor motion, foreign OSCs
  passing through; termui tests cover hint scanning and labels;
  predict tests cover token classes.

- **Container contexts.** `request_context_create_context` with a
  `cache_path` wants a direct child of `root_cache_path` (Chrome's
  profile manager refuses deeper paths with "Cannot create profile"),
  and the context initialises asynchronously: `create_browser_sync`
  against it returns None until `on_request_context_initialized`. The
  spike pumps `do_message_loop_work` until then (36ms in practice).
- **Hit order.** `side_hits` are drawn in order; menus come last, so
  the click resolver walks them in reverse.
- **Layered scrims.** A scrim over the panes must reset the open layer
  (`scene.layer(None)`) first, or it inherits the last pane's clip.

## Not done here (v1)
- Font fallback (symbols, emoji) — `⌘`/`▸`/`↵` are boxes in Plex Mono.
- Colour emoji (RGBA atlas), Sixel, output folding, custom hint regexes.
- Window transparency on Windows (needs a DirectComposition swapchain).
- Single instance over a named pipe / unix socket (loopback TCP here).
- Reader mode images (captions only here) and link following.
