# nus

*terminus* — a terminal emulator that is also a browser.

## What this is, and isn't

nus is my terminal and my browser. I built it for one user — me — the way I
want it, and I use it every day. It is open source because open is the right
way to ship software I depend on, not because it is a community project.

That means:

- **I decide the design.** The product truth lives in [docs/PRODUCT.md](docs/PRODUCT.md)
  and [docs/DESIGN.md](docs/DESIGN.md); each "settled" pass there is a decision
  already made. A change that contradicts them is out of scope, however good.
- **I don't take feature requests.** If you want it to work differently, the
  MIT license means you can fork it, and I mean that kindly — go build yours.
  Most "I wish it did X" is already a rule in `rules.luau`, a theme, or a layout
  file; try those first.
- **I read bug reports and small fixes.** A crash with a reproduction, a
  platform build fix, a typo, a wrong doc: welcome. See
  [CONTRIBUTING.md](CONTRIBUTING.md) before opening anything.
- **Pre-alpha, no releases, no support.** It builds; it is not packaged. Until
  there are releases, expect breakage and expect no answer on a schedule.

## What it does

- **Terminal:** its own VT core (`crates/vt`), GPU-rendered, Kitty keyboard
  protocol, Kitty/iTerm2/Sixel images, OSC 133 blocks with lamps, folds and a
  share page, OSC 9;4 progress in the sidebar and taskbar, mouse reporting,
  DECRQSS/XTGETTCAP/XTVERSION.
- **Browser:** Chromium (CEF) rendered offscreen and composited by us. DevTools
  as a pane, a reader, per-site rules, ad blocking in the request handler,
  userscripts, containers, our own picture-in-picture. **No Chrome
  extensions** — windowless CEF cannot host them; [docs/EXTENSIONS.md](docs/EXTENSIONS.md)
  explains the wall and the routes through it.
- **One window model:** Spaces → tabs → splits. A shell and a page are peers;
  a URL typed at a prompt opens beside it. Stacks, folders (GitHub, ports,
  files), tiles, peeks, a compact mode, a quick terminal (the hatch).
- **An editor pane** on ropey with tree-sitter colour and a language-server
  client (`crates/lsp`); the prompt line gets a language server too.
- **The ports board:** what's listening, who owns it (down to the shell that
  started it), and what to do about it.
- **An assistant** that sees the shell, the block in focus and the page beside
  it; skills from rules; a memory file.
- **Remote control:** `nus ls · open · edit · launch · send-text · theme ·
  hatch · block · ask …` from any shell or script (`crates/cli`); rules can
  drive the window too.
- **Layouts as Luau files**, shell integration that rides over ssh, a
  Broadsheet look with a look studio and twenty stock themes.
- **Config:** sandboxed Luau (`rules.luau`) for rules, folders, chains, skills,
  ports, blocks, grouping, layouts.
- **Targets:** Windows 11 (daily), macOS (Apple silicon) and Linux (Wayland +
  X11) build in CI; the OS-specific pieces (global hotkey, taskbar) land per
  platform as they're done.

## Building

Requires stable Rust (`rustup`), plus:

- Windows: VS 2022 with the *Desktop development with C++* workload.
- macOS: Xcode command line tools.
- Linux: `build-essential pkg-config libwayland-dev libxkbcommon-dev libgtk-3-dev`.

```
scripts/fetch-cef.sh              # the CEF binary matching vendor/cef-rs, into vendor/cef
export CEF_PATH=$PWD/vendor/cef   # see vendor/cef-rs/README.md for per-OS library paths
cargo build                       # the workspace crates
cd spikes/composite && cargo build   # the app (still called the composite spike)
```

`spikes/composite` is the app; it moves into `crates/app` when the spike is
done. See [docs/SPIKES.md](docs/SPIKES.md) for what was de-risked and
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the decisions.

## Credits

Phosphor Icons (MIT), IBM Plex Mono (OFL), Newsreader (OFL), the Chromium
Embedded Framework (BSD), tree-sitter and its grammars (MIT), ropey (MIT),
Neovide's cursor renderer and neoscroll's easings (MIT, ported). See
[NOTICE](NOTICE).

## License

MIT. See [LICENSE](LICENSE).
