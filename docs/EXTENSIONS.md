# Chrome extensions

Written 2026-09-17. The one major feature nus does not offer, and why, and
the route to offering it.

## The wall, precisely

CEF's own rule (its maintainer, verbatim on the forum): "Extensions are only
supported with Chrome style windows that show some portion of the Chrome UI
(like the Chrome toolbar)." Windowless rendering is Alloy style, always.
Spike 2 measured what that means in practice (`spikes/cef-osr/README.md`):
an MV3 extension loads and its service worker runs — uBlock Origin Lite
initialised 161 dynamic rules and matched them in a test — but our tabs
are not in the tab model (`chrome.tabs.query({})` → `[]`), declarativeNetRequest
is not attached to their URLLoaderFactory (blocked URLs return 200), content
scripts have nothing to attach to, `chrome-extension://` pages are
`ERR_BLOCKED_BY_CLIENT`. The extension system is wired to `TabHelpers` and
the extension web-request proxy that libcef creates only for Chrome-style
`WebContents`. Nothing to configure around; the pieces are absent.

Sources: the CEF forum thread on extension queries, the Alloy-bootstrap
removal issue (#3685: "The Chrome extension API is supported with Chrome
style browsers/windows only"), and spike 2.

## What people actually use extensions for

Ranked by what shows up in "why I can't switch browsers" threads:

1. Content blocking (uBlock Origin, Privacy Badger) — **built**: `adblock`
   in the request handler with EasyList; per-site off in the site panel.
2. Password managers (1Password, Bitwarden) — **planned**: `op`/`bw` CLIs
   behind a credential provider, fill via injected JS.
3. Dark mode / reader / userstyles (Dark Reader, Stylus) — **built**: the
   reader pane, per-site JS/CSS boosts through `Page.addScriptToEvaluateOnNewDocument`.
4. Vim keys / navigation (Vimium) — **partly built**: hints mode; the rest
   is a boost away.
5. Tab tools (OneTab, Tab Groups) — **built natively**: the sidebar, folders,
   stacks, tiles, archive.
6. Dev tools (React DevTools, Vue, Redux, Wappalyzer, JSON viewer) — the
   real hole. React DevTools has a standalone (`react-devtools` on npm,
   connects over a websocket); the rest need the extension runtime.
7. Everything else: Grammarly, Honey, Pocket, Notion clipper, Loom, Momentum,
   wallets (MetaMask). Long tail; wallets are the loud one.

So the *need* is mostly met natively except for framework devtools and the
long tail. The *expectation* — "does it run my extensions?" — is not, and it
is a question every reviewer will ask.

## Four routes

### A. Native equivalents (the v1 position, continued)

Keep building what extensions do. Adds a **boosts gallery**: a curated,
signed set of injected scripts (Vimium keys, dark mode, JSON viewer, a
Wappalyzer-style stack sniff, a React/Vue "is this a framework app" badge)
shipped as bundles, installable from the welcome page, and a userscript
manager (Greasemonkey `==UserScript==` headers, `@match`) so the Violentmonkey
corpus works as-is. Password fill through `op`/`bw`.

- Effort: weeks, all in Rust/JS we already own.
- Covers: 1–5 fully, 6 partly (no React DevTools panel), 7 not at all.
- Risk: none technical. The reviewer answer is "no, but here's what it has".

### B. The standalone-devtools bridge

For item 6 specifically: React DevTools, Vue Devtools and Redux DevTools all
ship standalone Electron/Node apps that connect over a websocket to a script
injected into the page. We can inject that script (we already inject) and
host the standalone's UI in a pane (it's a web page). Result: framework
devtools as a nus pane, no extension runtime. Wappalyzer's detection
heuristics are open (JSON) and run as a boost.

- Effort: a week per framework; the injection is the same for all three.
- Covers: the loudest part of 6.
- Risk: the standalone apps lag the extensions by a version now and then.

### C. A hidden Chrome-style window, composited by us

Create tabs as **Chrome-style** browsers in a real native window that is
kept off-screen (or 1×1, or on a hidden desktop), and get their pixels not
through OSR but through the DevTools protocol's `Page.startScreencast` /
`HeadlessExperimental.beginFrame`, or on Windows through `DwmRegisterThumbnail`
/ `PrintWindow` of the native HWND into a D3D texture we import into wgpu.
Extensions see real tabs; we still draw the chrome ourselves.

- Effort: a spike (a week) to learn whether the Chrome-style window can be
  created hidden without CEF insisting on its toolbar, and whether a capture
  path reaches 60fps with zero copies. Then a month to make it the tab path.
- Covers: everything, in principle — the tab is a real Chrome tab.
- Risk: high. Input routing (we'd forward events into the native window),
  popups and permission bubbles appear as Chrome's own UI, the "some portion
  of the Chrome UI" condition may be load-bearing (the toolbar hosts the
  action icons — an extension without a visible action may still work, per
  the maintainer's "case-by-case"). Capture latency on macOS/Linux is the
  unknown. This is the route that could answer "yes" to the reviewer.

### D. Our own libcef

Patch libcef to create `TabHelpers` and the extension request proxy for
Alloy-style `WebContents` (it's a handful of `AttachTabHelpers` calls and one
proxy registration, in `libcef/browser/alloy/…`). Then OSR tabs are extension
targets.

- Effort: a Chromium build per OS per CEF release — hours of compute
  monthly, a CI machine with 100 GB and a day, and the patch to maintain.
  The patch itself is small; the pipeline is the cost.
- Covers: everything, with our rendering path untouched.
- Risk: the patch rots with each release; popups and extension pages still
  need a place to render (they'd be OSR too — fine, we composite them).
  This is what Vivaldi-style embedders do; it's the "real" answer.

## The recommendation

**A now, B next, C as a one-week spike this quarter, D when there's a
release pipeline to hang it on.**

A closes the practical gap for nearly everyone and is pure nus. B turns the
one real hole (framework devtools) into a feature nobody else has — React
DevTools *beside your shell*. C is worth a spike because if the hidden
Chrome-style window can be captured cheaply, it's the fastest path to an
honest "yes"; the spike answers three yes/no questions and stops. D is the
right long-term shape and also what makes a `nus` build reproducible for
releases anyway — the moment there is a release pipeline building CEF, the
patch is a day's work on top.

What to say in the meantime, on the welcome page and the site: *"nus doesn't
run Chrome extensions. Blocking, dark mode, reader, vim keys, tab tools and
password fill are built in; userscripts run as-is; React and Vue devtools
open as panes."*

## Spike C, concretely

Three questions, one branch, one week:

1. Can a Chrome-style browser (`runtime_style: CHROME`, `window_info` with
   a parent HWND that is a hidden `WS_POPUP`) be created without CEF
   showing a toolbar, and does uBlock Lite's DNR then block in it?
2. Can its frames be captured at 60fps: try `Page.startScreencast` (JPEG,
   probably too slow), then `DwmRegisterThumbnail` (composited, zero-copy
   but only into another visible HWND), then `PrintWindow` into a DIB →
   D3D11 texture → wgpu import (one copy).
3. Does forwarding input (`SendInput` to the hidden HWND, or CDP
   `Input.dispatchMouseEvent`) feel native?

Two yes and one "good enough" → plan the month. Any no → C is dead and D is
the route.
