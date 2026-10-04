# Reviewing nus

nus is an MIT-licensed native terminal, browser, and editor. The shipping app is
in `spikes/composite`; the root Cargo workspace contains its reusable crates.
The name “spike” is historical and does not identify a separate demo.

## Start here

- [Review overview](https://cbassuarez.com/nus.dev/review/): specifications, architecture, funding scope, and limitations.
- [Published previews](https://github.com/cbassuarez/nus/releases): platform archives, `release.json`, and `SHA256SUMS.txt`.
- [Preview 17 change notes](releases/v0.0.2-preview.17.md): the reviewed candidate’s features, fixes and validation boundaries.
- [Architecture](ARCHITECTURE.md), [dependencies](DEPENDENCIES.md), and [third-party notices](../NOTICE).
- [Security policy](../SECURITY.md) and [privacy and diagnostics](PRIVACY_AND_DIAGNOSTICS.md).

## Reviewed candidate · October 4, 2026

**v0.0.2-preview.17** was published from
`a81adee496f3dfbc6749e2bb3b3f9a966e36acbc`.
Its [release workflow](https://github.com/cbassuarez/nus/actions/runs/37227526637)
completed macOS arm64, Windows x86-64 and Linux x86-64 packaging and publication.
The [review evidence](https://cbassuarez.com/nus.dev/review/releases/#reviewed-release)
names each runner, procedure and remaining limit.

Windows' five executable payloads and installer are Authenticode signed and
verified. macOS remains ad-hoc signed, without Developer ID distribution or
Apple notarization. Linux publishes SHA-256 checksums. Read the signing label
and source revision of the package you actually download; signing identifies
a publisher and a checksum identifies bytes, neither establishes safety.

The release ran packaged browser rendering, timeout, crash, hang, retry and
keyword regressions on all three platforms. Windows installer checks cover
installation, upgrade, native launch, normal uninstall, reinstall with the same
local profile and complete cleanup. Linux's Debian installation, launch and
removal check passed. Root CI, profile/component compatibility and the bounded
PTY ring verification passed at this same source revision. These are scoped,
maintainer-operated automated checks, not an independent security audit or
universal interactive and hardware acceptance. The current local macOS 15.6
walkthrough uses Chromium software-paint upload: the accelerated browser split
capture showed a black strip and clipped content and did not finish its resized
paint wait. The review packet preserves that open observation and original
capture; its cause is not yet established.

Homebrew, winget and the rolling apt repository did not update: their optional
jobs exited successfully without the needed repository credentials. Direct
release packages, including the installer and .deb, were published. The site's
release snapshot was updated by a separate manual dispatch and Pages deployment.
A green optional job does not prove that its integration changed.

“Here” continues across updates, reinstalls and moved app copies without sync.
Normal uninstall keeps its local profile. Deliberate `nus uninstall --everything`
removes the current channel's local profiles, recovery copies, vault keys, logs
and owned retained app packages; Windows also offers **Remove them too**.
Projects, other channels and external sync folders are preserved. Complete
cleanup is irreversible; review the confirmation before proceeding.

Use the release tag to reproduce the reviewed binary. New commits and newer
downloads do not inherit its evidence. The September security audit and native
performance measurements retain their original dates and scope.

## Build and checks

Install stable Rust with rustfmt and clippy. macOS needs Xcode command line
tools. Windows needs Visual Studio 2022's Desktop development with C++ workload;
run the commands below in Git Bash with the MSVC developer environment available.
Linux needs `build-essential pkg-config libwayland-dev libxkbcommon-dev
libgtk-3-dev libasound2-dev libudev-dev libgbm-dev`.

From a Bash shell:

```sh
git clone --recurse-submodules https://github.com/cbassuarez/nus.git
cd nus
# To reproduce the published candidate:
git checkout v0.0.2-preview.17
git submodule update --init --recursive
cargo build --workspace --locked
cargo test --workspace --locked
bash scripts/fetch-cef.sh
source scripts/env.sh
cargo build --locked --manifest-path spikes/composite/Cargo.toml --bins
cargo test --locked --manifest-path spikes/composite/Cargo.toml --bin composite
python3 scripts/test-release.py
```

For a contribution, work from current `main` rather than the release tag, and
also run `cargo fmt --all --check` and
`cargo clippy --workspace --all-targets --locked -- -D warnings`.
The app is outside the root workspace: root checks alone do not compile or test
it. Do not reformat the app's intentionally compact source as part of a fix.
The CEF fetch downloads the version pinned by the vendored bindings; see
[release engineering](RELEASING.md) for packaging and signing.

## Source map

| Area | Location |
| --- | --- |
| Native app, browser integration, chrome, startup | `spikes/composite/src` |
| Terminal parser and state | `crates/vt` |
| GPU rendering | `crates/render` |
| Profile sync | `crates/sync` |
| CLI and language server client | `crates/cli`, `crates/lsp` |
| Pinned CEF bindings and renderer patch | `vendor/cef-rs`, `vendor/wgpu-hal` |
| Build, capture, release checks | `scripts` |
| CI and native release packaging | `.github/workflows` |

## Review boundaries and feedback

Previews may break and have no support schedule. Chrome extensions are not
supported; see [the extension constraints](EXTENSIONS.md). Published performance
measurements describe their recorded revision and machine, not every release.
Read the [security limits](../SECURITY.md#known-limits) before testing private
browsing, assistant context, remote control, or phone access.

The historical September 21, 2026 audit recorded no public issues and one
merged PR; it is not a current tracker count.
An empty issue tracker is not evidence that the app is defect-free. Report
reproducible problems with the version, OS, expected result, actual result, and
redacted evidence. Use private vulnerability reporting for security findings.

`main` is the integration branch. Completed feature branches are deleted after
merging; their commits remain in the merged PR history. The historical
September audit was prepared on `seb/feat-review-readiness` in [PR #2](https://github.com/cbassuarez/nus/pull/2).
No branch protection was configured at audit time; a green workflow is evidence
of checks, not enforced peer review.
