# spike 3 — vt-render

Real PTY (`nus-pty`) → `nus-vt` → rustybuzz shaping → swash rasterization
into an R8 atlas → one instanced-quad wgpu pipeline. `lig.rs` is a ligature
test file. Ctrl+Shift+S toggles a continuous-redraw stress mode.

```
cd spikes/vt-render && RUST_LOG=info cargo run --release
```

## Findings — Windows 11, 2026-09-15, Intel RaptorLake iGPU (Vulkan), 144 Hz

Font found: Cascadia Code @ 13pt × 1.25 DPI = 21.7px → 13×25 px cells.

- **Correctness:** PowerShell prompt, `Get-ChildItem`, colored `Write-Host`,
  and vim (syntax colors, line numbers, `:set nu`, cursor, `~` fill) all
  render correctly. OSC 0 title reaches the window. Window resize with vim
  open re-lays-out through ConPTY with no warnings.
- **Ligatures:** `->` `=>` `!=` `==` `>=` `<=` `&&` `||` `...` `::` shape
  correctly via rustybuzz `calt`; a ligature glyph lands on its first
  cluster's column and the following columns emit nothing.
- **Latency (release):** mean **key-event→present-call 2.77 ms** over 134 keystrokes,
  measured from winit key event to `queue.present()` of the frame containing
  the echo — including ConPTY and PowerShell's own round trip. This historical
  spike result is not a measurement of the current full application or physical
  key-to-screen latency. GPU scheduling, compositor queues, refresh and scanout
  add latency; a present call alone cannot establish an upper bound for photons.
- **Throughput (release):** full-screen vim frame build 1.5–1.8 ms
  (~330 instances); 0.88 ms/frame average while dumping a 10k-line file.
  Debug build: 28 ms/frame full-screen — shaping every row every frame is
  the cost. The real renderer must cache shaped rows by content hash and
  rebuild only damaged rows; this spike deliberately doesn't.
- **ConPTY:** first prompt appears a few rows down (ConPTY's own initial
  cursor placement); resize and scrollback behave. No issues found.
- **Wide chars / non-ASCII:** 漢字 and accented Latin render; vim showed
  bytes for `—`/CJK only because `LANG` wasn't set in the PTY env
  (vim fell back to latin1). Fixed in `nus-pty` by setting `LANG` when unset.
- **Not done here (v1 work):** grapheme clusters / combining marks, font
  fallback (emoji, symbols), bold/italic faces (bold is currently the bright
  palette variant), subpixel positioning, selection, paste, mouse reporting,
  Kitty graphics.

Verdict: own VT core + own atlas renderer is viable on day one; no reason to
reach for an external terminal library.

### macOS / Linux — TODO
Same binary; expect Metal / Vulkan via wgpu. Font candidates include Menlo,
SF Mono, DejaVu Sans Mono, so no font install needed. Record fps and latency.
