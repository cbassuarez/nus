# nus

*terminus* — a terminal emulator that is also a browser. Personal software,
MIT-licensed so the license question never comes up.

- **Terminal:** own VT core, GPU-rendered, Kitty keyboard/graphics.
- **Browser:** Chromium (CEF) rendered offscreen and composited by us. Real
  extensions, real devtools, our own PiP.
- **One window model:** Spaces → tabs → splits. A terminal tab and a browser
  tab are peers. No multiplexer.
- **Config:** sandboxed Luau.
- **Targets:** Windows 11, macOS (Apple silicon), Linux (Wayland + X11).

Status: pre-alpha, nothing runs yet. See [docs/SPIKES.md](docs/SPIKES.md) for
what's being de-risked first and [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)
for the decisions already made.

## Building

Requires stable Rust (`rustup`), plus:

- Windows: VS 2022 with the *Desktop development with C++* workload.
- macOS: Xcode command line tools.
- Linux: `build-essential pkg-config libwayland-dev libxkbcommon-dev libgtk-3-dev`.

```
scripts/fetch-cef.sh      # exports the CEF binary matching vendor/cef-rs into vendor/cef
export CEF_PATH=$PWD/vendor/cef   # see vendor/cef-rs/README.md for per-OS library paths
cargo build
```
