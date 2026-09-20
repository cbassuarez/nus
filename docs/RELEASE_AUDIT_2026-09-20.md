# Security and release audit — 2026-09-20

**Verdict: the approved privacy and security changes pass the macOS checks, but
this checkout is not yet cleared for a public cross-platform release.** The
remaining gates below must be resolved against one fixed release candidate.

This review covered incognito isolation and persistence, browser debugging,
macOS cookie protection, local/phone control boundaries, issue reporting,
dependency advisories and build checks. It was not an independent penetration
test of Chromium, the GPU stack, every renderer bridge or all terminal protocols.
UI and core changes were limited to the behavior explicitly approved in this
task. Concurrent editor, resource-budget, native-menu and zoom work was preserved.

## Findings addressed

| Finding | Change and evidence |
| --- | --- |
| **High: regular browsing enabled Chromium's test keychain.** | Ordinary launches now use macOS Keychain. The bypass requires an explicitly isolated screenshot fixture. With `NUS_TEST_REAL_KEYCHAIN=1`, the native check confirms that the mock switch is absent, the fixture cookie is encrypted on disk, and it survives reopening. No user profile was opened or migrated. Older mock-keychain cookies may require signing in again. |
| **High: an unauthenticated debugging endpoint opened on every launch.** | No default debugging listener. External debugging requires `NUS_REMOTE_DEBUGGING_PORT`, binds to loopback, and is disabled in incognito. Built-in DevTools uses CEF's internal connection in a native tools window. Normal and private lifecycle checks pass without the debug port. |
| **High: predictable tokens and weak credential-file handling protected powerful remote commands.** | Tokens now use 32 bytes of OS randomness. Credential files are atomically replaced with owner-only access; Unix mode and symlink replacement are tested. Windows has an explicit protected owner DACL, pending Windows runtime verification. |
| **High: launch handoff accepted unauthenticated raw URL/raise input.** | Handoff now uses authenticated JSON and an acknowledgment. Raw input and incorrect tokens are rejected. The existing `nus ls --json` CLI still succeeds against the isolated app. |
| **Medium: unauthenticated socket input and concurrency were unbounded.** | CLI requests are capped at 1 MiB, 256 requests per connection, and a shared 32-connection limit. Reads/writes have five-second socket timeouts and absolute request deadlines. Phone requests cap headers at 16 KiB and bodies at 64 KiB. Oversized and idle unauthenticated CLI input is rejected in the native check. An individual blocking read can extend the total deadline by up to its socket timeout. |
| **High: an empty global CEF cache setting did not reliably make browsing private.** | Native testing exposed a persistent global context. Private tabs now require an explicit in-memory context and refuse to open if CEF reports a persistent cache path. Private tabs/windows share that context only inside their private process. |
| **High: saved device permissions crossed origin boundaries.** | The old hostname helper removed schemes, ports and `www.`, and collapsed IPv6 addresses to `[`. Browser permission callbacks used that value to auto-accept saved grants. With approval, grants now use a standards-based URL parser and the full canonical web origin in a separate permission store. Legacy host grants require a new decision. Unit regressions cover ports, schemes, subdomains, IPv6, default-port normalization and combined requests; native checks exercise saved decisions without accessing hardware. |
| **Privacy: private activity could reach normal persistence or control paths.** | A separate process and temporary root isolate private browsing. History, restore, replay, settings/site/download-history writes, terminal/assistant features, CLI and phone control are disabled there. A recursive disk-canary scan, normal/private cookie isolation, private-session reopening, and temporary-root cleanup all pass. Completed downloaded files survive cleanup. |
| **Behavior: the final private tab did not close its window.** | Standard browser close/new-tab/address shortcuts work in private windows. Last-tab closure closes the private window. The native check exercises ⇧⌘N, shared temporary cookies across windows, ⌘W and ⇧⌘W. |
| **Reporting: no useful, privacy-conscious in-app issue entry points.** | Help, the command menu, and Settings open bug/feature drafts for `cbassuarez/nus`. Only build/version/OS information is attached automatically. The controls are present and visually checked; no issue was submitted and no live website was visited in the native harness. |
| **Release checks omitted the shipping application.** | CI now fetches pinned CEF and builds/tests `spikes/composite` in addition to the root workspace. Its lockfile is included for `--locked` checks. The app package version matches the current 0.0.1 bundle version. Remote CI has not been run here. |
| **Test reliability: profile fixtures could share a directory.** | The store tests used elapsed nanoseconds from a newly created clock value as a unique suffix. Parallel tests collided and read each other's data. Fixtures now use separate temporary directories with automatic cleanup. |

## Open release gates

1. **The shared checkout needs clean CI.** The app and its helper build
   successfully. The final app test run passed 141 tests with one explicitly
   ignored release timing probe; the workspace passed 76. Formatting and
   strict Clippy still fail. Clippy reports a complex type in `crates/lsp`,
   needless borrows in render text code, `manual_div_ceil` in
   `crates/render/src/gpu.rs` and `too_many_arguments` in
   `crates/render/src/scene.rs`. Editor/highlighter compilation issues encountered
   during concurrent work are resolved in the successful final build. Freeze
   a candidate before the final CI run; this audit was performed in a shared,
   changing checkout.
2. **Run native checks on Windows and Linux.** This session verified macOS Apple
   Silicon. Windows ACLs, Ctrl-based shortcuts, downloads, private shutdown,
   native DevTools and Linux desktop behavior still need runtime evidence.
3. **Test the actual signed distribution.** The local review bundle is ad hoc
   signed. Developer ID signing, notarization/Gatekeeper, clean-machine install,
   helpers and production Keychain prompts still need a packaged-candidate run.
   Restoring Keychain did not include migration of real users' older cookies.
4. **Finish interactive DevTools testing.** Automated CEF open/close assertions
   pass, with the inspected page preserved. The macOS event loop logs
   `tried to run event handler, but no handler was set` around native tool-window
   operations. That does not prove a user-visible failure, but keyboard focus,
   window closing and repeated use should be checked in the final package.
5. **Reconcile the pinned Chromium version with upstream security releases.**
   The tested bundle reports Chromium 152.0.7977.83. RustSec does not audit CEF
   binaries, Chromium CVEs, vendored GPU code or font assets. This pass did not
   establish that the bundled engine contains every current upstream fix.
6. **Publish the issue templates with the release.** The new field IDs must be
   on GitHub's default branch before version/OS prefill can be validated there.
   No templates, issues or other content were published during this check.

## Remaining security and maintenance decisions

- **Phone control remains unencrypted.** It is opt-in, but serves HTTP on all
  interfaces and carries a bearer token in URLs/forms. A network observer can
  capture that token and invoke privileged actions while the feature is enabled.
  Randomness and input bounds do not solve transport interception. Encrypted
  transport/pairing or a narrower product policy requires a separate approved
  core/UI change before describing this feature as secure remote access.

  **Closed after this audit.** The phone is served over TLS with a certificate
  made in memory per session; a plaintext request is not answered, and the
  token no longer crosses the network in the clear. The certificate is
  self-signed, so the first connection is trust-on-first-use: SYNC · THE PHONE
  shows its SHA-256 for comparison against what the phone displays, and nus
  pins that exact certificate when the phone's page is opened in a tab here.
  Passive interception is closed; an active attacker on the network who is
  accepted at the phone's trust prompt is not. Separately, requests arriving
  from the phone now carry `Origin::Phone` and are limited to `front` and
  `hands-answer` — the rest of remote control is unreachable from the network.
  See `phone.rs`, `remote.rs` and the tests beside them.
- **Two font dependencies are unmaintained.** Both lockfiles have zero known
  vulnerability matches in the refreshed RustSec database (1,251 advisories),
  with maintenance warnings for `rustybuzz 0.20.1` and `ttf-parser 0.25.1`.
  These are not confirmed exploitable vulnerabilities. Rustybuzz's maintainer
  recommends HarfRust, and RustSec lists Skrifa as a parser alternative.
  A migration touches text shaping/layout and needs approval plus visual font
  regressions. Sources: [Rustybuzz maintenance status](https://github.com/harfbuzz/rustybuzz),
  [ttf-parser advisory](https://rustsec.org/advisories/RUSTSEC-2026-0192).

## Evidence and reproduction

The full successful native suite used a disposable copy of the macOS bundle
and loopback-only HTTP pages. It checked:

- normal encrypted cookie and local-storage persistence across reopening;
- no normal cookies/storage in private tabs, sharing only inside a private session;
- no private cookies/storage on a new private launch;
- no private browsing canary in profile files, no saved history/session files;
- private window creation/closure, temporary-root deletion and retained downloads;
- native DevTools availability with external debugging disabled;
- issue controls and a persistent Incognito label;
- valid CLI requests, wrong-token/raw-input rejection, oversized input and idle expiry.

The final successful suite is at `/tmp`'s macOS temporary equivalent under
`nus-privacy-check-pedi2vg4`.
That tested executable's SHA-256 is
`86c7a8773c68d1fddb458807db44fe3570f0f71b267b6972a2ef32d8aed23e49`.
The build reported base revision `966fee9`; the working tree includes uncommitted
changes, so that revision alone is not an identifier for the tested source.

The final safeguard resolves the private download destination before changing
to the disposable root, including when HOME/USERPROFILE is absent. The native
check confirms that destination is outside the disposable root. The permission
panel was also visually inspected with a synthetic saved grant; no camera or
microphone was activated.

Reproduction instructions and the user-visible privacy contract are in
[Privacy and diagnostics](PRIVACY_AND_DIAGNOSTICS.md). The harness is
`scripts/check-privacy-security.py`. Rust unit checks use the environment from
`scripts/env.sh`; the app's lockfile is `spikes/composite/Cargo.lock`.
