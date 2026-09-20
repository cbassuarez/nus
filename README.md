# nus

*terminus*: the endpoint. the last terminal emulator, multiplexer, and browser you’ll install.

## What this is, and isn't

nus is my terminal and my browser. I built it for one user (me!) the way I
want it, and I use it every day. I made it for personal and professional work, because none of the popular cross-platform tools integrated with the popular cross-platform browsers. I leave it FOSS as someone else might get some use out of this.

That means:

- **I decide the design.** The product decisions live in [docs/PRODUCT.md](docs/PRODUCT.md)
  and [docs/DESIGN.md](docs/DESIGN.md); each "settled" pass there is a decision
  already made. A change that contradicts them is out of scope, however good.
- **Feature requests are welcome.** Describe the problem you want to solve;
  requests inform development without promising implementation. Check whether
  a setting, `rules.luau`, a theme or a layout already covers it.
- **I read bug reports and small fixes.** A crash with a reproduction, a
  platform build fix, a typo, a wrong doc: very much welcome. I’ll try to stamp out bugs as quickly as they come in. See
  [CONTRIBUTING.md](CONTRIBUTING.md) before opening anything, and email contact@cbassuarez.com for secured/responsible disclosure of vulnerabilities. There are limited funds available (I am one person, funding this by themselves, though I am awaiting extra funding to establish an actual program).
- **Pre-alpha, no releases, no support.** It builds; it is not packaged. Until
  there are releases, expect breakage and expect no answer on a schedule.

## What it does

- **Terminal:** its own VT core (`crates/vt`), GPU-rendered, Kitty keyboard
  protocol, Kitty/iTerm2/Sixel images, OSC 133 blocks with lamps, folds and a
  share page, OSC 9;4 progress in the sidebar and taskbar, mouse reporting,
  DECRQSS/XTGETTCAP/XTVERSION.
- **Browser:** Chromium (CEF) rendered offscreen and composited by us. Native DevTools,
  a reader, per-site rules, ad blocking in the request handler,
  userscripts, containers, our own picture-in-picture. **No Chrome
  extensions** (coming soon) — windowless CEF cannot host them; [docs/EXTENSIONS.md](docs/EXTENSIONS.md)
  explains the wall and the routes through it.
- **Incognito:** ⇧⌘N on macOS or Ctrl+Shift+N on Windows/Linux opens a private
  browser window with temporary cookies and storage, no saved browsing history
  or session restore, and downloads kept on disk. See [privacy and diagnostics](docs/PRIVACY_AND_DIAGNOSTICS.md).
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
- **A local profile, not an account:** a name, a face, the day it began,
  in a file in a folder on your machine. No server, no telemetry, nothing
  sent; the card that sets it up says so.
- **Sync without an account:** the profile on more than one device, sealed
  with a key you copy, carried by a folder you already sync or a private git
  remote; last writer wins ([docs/SYNC.md](docs/SYNC.md)).
- **Targets:** Windows 11 (daily), macOS (Apple silicon) and Linux (Wayland +
  X11).

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
