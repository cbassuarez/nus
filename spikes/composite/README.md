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

## Not done here (v1)
- Font fallback (symbols, emoji) — `⌘`/`▸`/`↵` are boxes in Plex Mono.
- CEF popup surfaces (`<select>` dropdowns) are not composited.
- Selection / copy / paste, scrollbars, favicons, history, find.
- Space-owned browser profiles (one request context per Space; the spike uses the global one).
- Window transparency on Windows (needs a DirectComposition swapchain).
- Single instance over a named pipe / unix socket (loopback TCP here).
- Reader mode images (captions only here) and link following.
