# spike 1 — cef-osr

CEF (Chrome runtime, windowless) → shared GPU texture → wgpu quad in a winit
window. Started as a verbatim copy of `vendor/cef-rs/examples/osr`; diverged
only to add instrumentation and remove the example's frame throttle.

## Run

```
scripts/fetch-cef.sh                      # once
source scripts/env.sh                     # CEF_PATH + library path for this OS
cd spikes/cef-osr && cargo run
RUST_LOG=info,cef=debug cargo run         # shows which import path was taken
```

Windows also needs Ninja on PATH for the `cef-dll-sys` cmake step
(VS ships one: `Common7\IDE\CommonExtensions\Microsoft\CMake\Ninja`).

## Findings

### Windows 11 — 2026-09-15 — CEF 152.0.6, cef-rs 152.3.0, wgpu 30, dx12
- **Shared texture path works.** `on_accelerated_paint` delivers a D3D11
  shared handle; `cef::osr_texture_import::d3d11` opens it as a D3D12
  resource and wraps it as a `wgpu::Texture` (`Bgra8Unorm`, zero-copy).
  No Vulkan or CPU fallback was hit.
- **144 fps**, vsync-bound on a 144 Hz display, once the example's
  `sleep(1000/17)` was removed. Present blocks on vsync; CEF paints at its
  own `windowless_frame_rate` (60) driven by `send_external_begin_frame`.
- github.com renders correctly at 800×600 logical, DPI-scaled.
- Noise: `Network service crashed or was terminated, restarting service`
  logs exactly once at startup on every run. Page loads fine afterwards.
  Investigate in spike 2 alongside request contexts / profiles.
- The example recreates sampler + bind-group layout + bind group on every
  paint. The real `render` crate must cache the imported texture by shared
  handle and only rebuild on handle change.
- CEF's external message pump is driven by calling `do_message_loop_work`
  once per winit pump; the proper integration is
  `on_schedule_message_pump_work` → wake the event loop. Do that in the app,
  not here.

### macOS (Apple silicon) — TODO
Run the same; expect IOSurface → Metal import. Record fps and any fallback.

### Linux — TODO (Ubuntu, Wayland session; then X11)
Run the same; expect dmabuf → Vulkan import (`dmabuf.rs`). Record:
- import path taken and fps
- whether CEF boots in a Wayland-only session (`WAYLAND_DISPLAY` set,
  no XWayland) — if not, which switches fix it (`--ozone-platform=wayland`)
- CPU-upload fallback cost at 4K if dmabuf import fails
