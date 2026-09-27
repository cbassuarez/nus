# Streaming and picture in picture

On macOS, Netflix, Prime Video, Hulu and the other hosts listed in
`spikes/composite/src/webkit.rs` open in the system's WKWebView. Protected
playback uses the macOS media stack; PiP moves that live native view beneath
NUS's controls. It does not copy decrypted frames through Chromium or the GPU
capture path. This implementation adds no paid SDK, hosted service or new CEF
build. A user's streaming subscription and the provider's playback rules still
apply.

Apple documents encrypted premium playback in macOS WKWebView in
[What's new in WKWebView](https://developer.apple.com/videos/play/wwdc2022/10049/).
That establishes platform capability, not a guarantee for every streaming
service. On September 26, 2026, the user confirmed Netflix playback in NUS's
WebKit route and reported Prime's generic **Video Unavailable** error. Hulu
playback has not been verified. Prime's
[linked help page](https://www.primevideo.com/help?nodeId=GU85HKX66NVFNQ9Y)
covers many different failures and is not a specific DRM diagnosis.

## Implementation

- A directly opened streaming address goes to WebKit before Chromium can
  redirect it to an unsupported-browser page. Late Chromium callbacks cannot
  replace the native address or loading state. A failed cookie query times out
  into WebKit's own sign-in flow instead of leaving a blank tab forever.
- WebKit sign-ins follow NUS profile and container boundaries. macOS 14 and
  newer support named persistent stores; older systems use isolated memory
  stores. Private sessions stay in memory. Existing native cookies take
  precedence over stale Chromium cookies during handoff. Moving from the old
  shared WebKit jar may require signing in again.
- PiP's native view stays below the transparent controls after reparenting
  and resizing. Mouse input passes to NUS; keyboard focus returns to NUS's
  window view. The first click is accepted even if PiP is inactive.
- Faded controls retain their hit geometry. Keyboard and accessibility focus
  follow the controls actually present. Seek, pause/play and mute use the
  selected player, including accessible same-origin child frames. Netflix
  seeks use its player API rather than assigning `currentTime` beneath it.
- Cross-origin player frames cannot be inspected through same-origin DOM
  traversal. NUS does not guess their crop or relay arbitrary page messages
  as control commands.

## Diagnose a failed native stream

With the failing page selected in the rebuilt app, run:

```sh
nus page info --json
```

`engine` distinguishes WebKit from Chromium. `media_diagnostic` includes the
HTML media error category, ready state and network state. It contains no
license data or cookie values. An empty diagnostic means no media state has
been observed yet; `media error: none` does not establish successful playback.

For a deeper local investigation, start the app with
`NUS_MEDIA_DIAGNOSTICS=1`, then reproduce the failure. The same command also
reports bounded key-system access requests and whether those requests were
granted or denied. This opt-in observes `requestMediaKeySystemAccess` without
changing its arguments or returned Promise. It is off by default because
wrapping a page API changes its function identity. An EME access grant is not
a license grant or proof of a decoded frame.

Do not use **Widevine installed** as a streaming acceptance test.
[Google's Widevine overview](https://developers.google.com/widevine/drm/overview)
states that Widevine has no fees but requires an agreement. A compatible CDM,
codecs and the provider's acceptance of the client are separate requirements.
The optional system-codec CEF build does not satisfy those requirements by
itself and is unnecessary for the macOS WebKit route.

## Verification

The focused Rust checks cover native host routing, cookie identity and store
isolation, delayed navigation callbacks, frame report routing and PiP control
selection. `node scripts/check-video-transport.cjs` exercises media transport,
same-origin frame geometry, failures and diagnostics in deterministic fixtures.

`python3 scripts/check-webkit-pip.py /absolute/path/to/nus.app` exercises the
native route against a temporary local video and disposable profile. It is a
WebKit/PiP regression check, not authenticated Netflix, Prime or Hulu playback
validation. Its app-level input calls do not establish OS-level first-click
delivery; that also needs a desktop interaction check.
