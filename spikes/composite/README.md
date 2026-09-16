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

## Not done here (v1)
- Font fallback (symbols, emoji) — `⌘`/`▸`/`↵` are boxes in Plex Mono.
- CEF popup surfaces (`<select>` dropdowns) are not composited.
- DevTools: needs a second OSR browser; F12 only logs.
- Selection / copy / paste, scrollbars, favicons, history, find.
- Space-owned browser profiles (one request context per Space).
