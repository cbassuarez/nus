# Reviewing nus

nus is an MIT-licensed native terminal, browser, and editor. The shipping app is
in `spikes/composite`; the root Cargo workspace contains its reusable crates.
The name “spike” is historical and does not identify a separate demo.

## Start here

- [Review overview](https://cbassuarez.com/nus.dev/review/): specifications, architecture, funding scope, and limitations.
- [Published previews](https://github.com/cbassuarez/nus/releases): platform archives, `release.json`, and `SHA256SUMS.txt`.
- [Preview 7 change notes](releases/v0.0.1-preview.7.md): the scope of this candidate.
- [Architecture](ARCHITECTURE.md), [dependencies](DEPENDENCIES.md), and [third-party notices](../NOTICE).
- [Security policy](../SECURITY.md) and [privacy and diagnostics](PRIVACY_AND_DIAGNOSTICS.md).

**The current preview is unsigned for trusted distribution.** macOS uses an
ad-hoc signature, not Developer ID signing or notarization; Windows does not
have an Authenticode signature. Signing identities and distribution validation
are part of the funding request. A checksum verifies archive integrity, not the
publisher's identity. Follow the platform-specific instructions in the release
notes rather than disabling system protections globally.

Preview 7 was published from `ec36cbeb10a891752af29f833fb797e484d3b446`.
Its [release workflow](https://github.com/cbassuarez/nus/actions/runs/35626434391)
completed macOS arm64, Windows x86-64, and Linux x86-64 packaging and publication.
This is build and packaging evidence, not an independent security audit or a
claim that every interactive feature has been tested on every platform.
Subsequent documentation and maintenance commits on `main` do not change that
published binary. Use its tag when reproducing release behavior.

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
git checkout v0.0.1-preview.7
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

There were no public issues and one merged PR at the September 21, 2026 audit.
An empty issue tracker is not evidence that the app is defect-free. Report
reproducible problems with the version, OS, expected result, actual result, and
redacted evidence. Use private vulnerability reporting for security findings.

`main` is the integration branch. Completed feature branches are deleted after
merging; their commits remain in the merged PR history. This audit was prepared
on `seb/feat-review-readiness` in [PR #2](https://github.com/cbassuarez/nus/pull/2).
No branch protection was configured at audit time; a green workflow is evidence
of checks, not enforced peer review.
