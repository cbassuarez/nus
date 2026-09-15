# Dependencies: bundle / fork / depend / reference

## Bundle (fetched by `scripts/fetch-cef.sh`, never committed)
- **CEF** binary distribution, version pinned by the `vendor/cef-rs` submodule
  commit (currently CEF 152 / Chromium 152). Windows
  x64, macOS arm64, Linux x64. Bump monthly with Chromium.
- **Fonts:** JetBrains Mono (OFL) as default, Symbols Nerd Font for icons.

## Fork (git submodule under `vendor/`, we carry patches)
- `vendor/cef-rs` — tauri-apps/cef-rs. Expect patches around OSR shared
  textures and Chrome-runtime windowless mode.

## Depend (crates.io)
wgpu, winit, raw-window-handle, vte, portable-pty, unicode-width, swash,
rustybuzz, fontdb, adblock (Brave), mlua (luau), rusqlite (Arc import),
keyring, notify-rust, windows (notifications).

## Reference (read, don't vendor)
- Ghostty — terminal state, Kitty keyboard/graphics, shaping.
- Alacritty — `vte` integration, grid.
- WezTerm — ConPTY, SSH profile UX.
- Brave — `adblock` engine integration.
- Zed / gpui — scene graph and text system design.
- Arc — `~/Library/Application Support/Arc/StorableSidebar.json` (Spaces),
  Chromium `Bookmarks` and `History` under Arc's User Data (import).
