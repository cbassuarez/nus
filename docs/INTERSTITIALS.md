# Interstitials

What nus shows when a site can't load, or when something happens to a
page that's already open. Direction chosen 2026-09-25: **Transcript**; refined
2026-10-01 to **Trace**. The page reads as a shell log: what nus did
(`» open …`, `» watch …`, `» page …`), a headline that says what is wrong in
words, then a **trace** — one ruled row per step nus can vouch for, with how
long it took and ✓ / ✕ / … / — — and the next commands. The step that failed
carries the 3px rule: signal red for danger, ink for a problem. Pages without
a trace (clock, dangerous site, resend form, Wi-Fi, dialogs) keep the rule
down the left. ↵ always runs the safe command. Going ahead anyway is a dim
command at the end of the list and is never the default. A new transcript
always starts with its default command highlighted.

Traces never invent facts. A load error's trace comes from Chromium's error
(which step it names: name, connect, secure, request, answer) and the time
from asking to the error; a local name shows what this computer resolves it
to. A page that stops answering shows the server's answer and load time from
the page's own network log, and where its script is stuck from one brief
`Debugger.pause` (resumed at once; given up after 2s). A crash shows how long
the page was open, the exit reason in words (`SIGSEGV (11)`: a bad memory
access), pages that ended within 2s of it (one process), and this site's
earlier ends this session.

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
| Crashed / out of memory | `on_render_process_terminated`. macOS reports V8 running out of memory as an ordinary crash (exit code 5), so there it gets the crash page. `retry-all` reloads every page that ended with it; `details` copies the trace for a bug report. Stopping a page shows *Stopping the page* until Chromium reports the renderer gone; a reload asked meanwhile waits for that (at most 5s). If Chromium refuses to end it, the not-responding page comes back and says so. |
| Send form again | `ERR_CACHE_MISS` |
| Wi-Fi sign-in | a network error, then the system's own plain-HTTP check (macOS `captive.apple.com`, Windows `msftconnecttest.com`, Linux `nmcheck.gnome.org`) answers with a redirect, or a page in place of its usual text. A proxy's or filter's refusal (403, 407, 5xx) is not a sign-in. |
| Can't reach | every other load error: DNS, refused, timeout, offline, blocked by content blocking, an error status with no page (`ERR_HTTP_RESPONSE_CODE_FAILURE`: the server *did* answer). A refused local port also says what last served it (ports that remember), offers `start` (its saved command in a new shell) and `watch` (load the page when the port answers). |

A load Chromium drops (`ERR_ABORTED`: a 204, *Stay on this page*, Stop, a
download) is not an error: no page, the address returns to the page still
shown, and its 30s deadline is disarmed. Clock pages need a build date; a
build made outside git never blames the clock.

## Over the page (native overlays)

These are drawn by nus. The page stays alive underneath them.

| overlay | when |
|---|---|
| Still waiting | a load with no answer and no error after 30s. Nothing is stopped: slow servers, big uploads and long reports keep going, and the overlay goes when the page arrives. ↵ keeps waiting (another 30s), `retry` asks again, `stop` (Esc) stops the load. |
| Not responding | nus sends each visible page a trivial `Runtime.evaluate` every 3s. If there's no answer for 10s, this shows; any answer, even a failed evaluation, counts as alive. Chromium's own unresponsive callback also triggers it. `stop` stops a pending load, then crashes the renderer from its I/O thread (CDP `Page.crash`, which Chromium refuses during a navigation). |
| Waking | a sleeping tab is shown and hasn't painted within 300ms |
| Mic / camera | macOS has denied or restricted nus (`AVCaptureDevice` authorization) |
| File blocked | a program disguised as a document (`invoice.pdf.exe`), or `.scr` `.pif` `.vbe` `.jse`. The download is never written. `keep` allows that URL for this session. |

## Dev addresses

`nus://interstitials` lists everything. Clicking a row, or typing its
command at the `»` prompt, opens it.

- `nus://interstitial/<cert|malware|clock|oom|crash|permission|file|resubmit|sleep|portal|unreachable|slow|hung>`: the page with sample facts
- `nus://crash`: crashes this page's renderer (CDP `Page.crash`)
- `nus://oom`: allocates until V8 aborts
- `nus://hang`: loops the main thread; the watch catches it in about 10s
- `nus://block-download`: downloads a blob named `invoice.pdf.exe`
