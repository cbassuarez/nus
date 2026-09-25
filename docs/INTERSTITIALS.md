# Interstitials

What nus shows when a site can't load, or when something happens to a
page that's already open. Direction chosen 2026-09-25: **Transcript**. The page
reads as a shell log: what nus tried (`» open …`), what happened, then
the next commands. A 3px rule on the left marks severity: signal red for
danger, ink for a problem, nothing at rest. ↵ always runs the safe
command. Going ahead anyway is a dim command at the end of the list and
is never the default.

Code: `spikes/composite/src/interstitial.rs` (model, detection, page
script), `interstitial_ui.rs` (commands, native overlays).

## In place of the site (HTML)

The page is built node by node over Chromium's error document, or over a
blank one. The address bar keeps the address you asked for. Chrome's own
error pages enforce Trusted Types, so no HTML strings are used (no
`document.write` or `innerHTML`). Commands come back through the
`nusInterstitial` CDP binding, with a token that only that document has.

| page | when |
|---|---|
| Connection not private | any `NET::ERR_CERT_*`. nus cancels the certificate request, so Chrome's interstitial never shows. `proceed` allows the host for this session. |
| Clock | `ERR_CERT_DATE_INVALID` while the clock reads earlier than this build's commit, or more than 3 years after it |
| Dangerous site | host is on `profile/dangerous.txt`, which you maintain (nus has no Safe Browsing). Refused before any request is made. |
| Crashed / out of memory | `on_render_process_terminated`. macOS reports V8 running out of memory as an ordinary crash (exit code 5), so there it gets the crash page. |
| Send form again | `ERR_CACHE_MISS` |
| Wi-Fi sign-in | a network error, then an HTTP probe to `captive.apple.com` gets an answer other than `Success` |
| Can't reach | every other load error: DNS, refused, timeout, offline, blocked by content blocking |

## Over the page (native overlays)

These are drawn by nus. The page stays alive underneath them.

| overlay | when |
|---|---|
| Not responding | nus sends each visible page a trivial `Runtime.evaluate` every 3s. If there's no answer for 10s, this shows. Chromium's own unresponsive callback also triggers it. `stop` crashes the renderer from its I/O thread. |
| Waking | a sleeping tab is shown and hasn't painted within 300ms |
| Mic / camera | macOS has denied or restricted nus (`AVCaptureDevice` authorization) |
| File blocked | a program disguised as a document (`invoice.pdf.exe`), or `.scr` `.pif` `.vbe` `.jse`. The download is never written. `keep` allows that URL for this session. |

## Dev addresses

`nus://interstitials` lists everything. Clicking a row, or typing its
command at the `»` prompt, opens it.

- `nus://interstitial/<cert|malware|clock|oom|crash|permission|file|resubmit|sleep|portal|unreachable|hung>`: the page with sample facts
- `nus://crash`: crashes this page's renderer (CDP `Page.crash`)
- `nus://oom`: allocates until V8 aborts
- `nus://hang`: loops the main thread; the watch catches it in about 10s
- `nus://block-download`: downloads a blob named `invoice.pdf.exe`
