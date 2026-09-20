# Releasing nus

The Release workflow builds three native packages: Apple Silicon Macs,
Windows x86-64, and Linux x86-64. Intel Macs are not a target. Native runners fetch the CEF version pinned by
the submodule, test both workspaces, build the app and CLI, package the complete
runtime, and check that the packaged executable can start its loader.

## Channels

- Preview tags are `vX.Y.Z-preview.N`; GitHub marks them prereleases.
- Stable tags are `vX.Y.Z`; they become GitHub's latest release and require
  notarized Mac packages and signed Windows packages.
- `X.Y.Z` must match both Cargo package versions. Increment the preview number
  for a new candidate; published releases are never overwritten.
- A manual run defaults to build-only. Its artifacts expire after three days.
  Enable Publish or push a version tag to publish a complete passing matrix.
  Build-only Mac packages are ad-hoc signed even when signing secrets exist;
  that keeps credential access limited to publication runs.
- A manual preview can select Linux, Windows or macOS independently. All selected
  jobs must pass; its notes explicitly list omitted platforms. This lets Linux
  previews ship while Apple's account or signing is pending. Stable still requires
  the complete three-platform matrix. Published previews remain immutable.

The publication job verifies every archive's size and SHA-256, creates a draft,
uploads all three packages, `SHA256SUMS.txt` and `release.json`, then publishes.
A failed upload remains a draft. The downloads site reads only published releases
and only exposes assets matching this package contract. Missing channels and
failed API requests never become invented download links.

## Signing configuration

Repository Actions secrets (never commit the values):

| macOS | Meaning |
| --- | --- |
| `MACOS_CERTIFICATE` | Base64 PKCS#12 of the selected Developer ID Application identity |
| `MACOS_CERTIFICATE_PASSWORD` | Random password protecting that export |
| `MACOS_SIGN_IDENTITY` | Developer ID identity or certificate fingerprint |
| `APPLE_API_KEY` | App Store Connect API private key in P8 format |
| `APPLE_API_KEY_ID` | Key ID |
| `APPLE_API_ISSUER` | Issuer ID |

Each runner imports the identity into a temporary keychain, signs nested code
inside out with the hardened runtime, submits to Apple's notary service, staples
the ticket, and runs strict signature and Gatekeeper checks. Temporary keychains
and credential files are removed on exit. The Developer ID team alone cannot
authenticate to notarization; the API credential is also needed.

Windows uses `WINDOWS_CERTIFICATE` (base64 PFX) and
`WINDOWS_CERTIFICATE_PASSWORD`. The Windows SDK signs the GUI and CLI with
SHA-256, timestamps them and verifies Authenticode before packaging. An Apple
certificate cannot sign Windows applications.

Preview packaging can be unsigned/ad-hoc when signing is not configured. Its
metadata and the website must identify that accurately. Native install, media,
focus, accessibility and clean-machine tests remain release review gates; a
successful loader check is not a complete desktop acceptance test.

## Packages and storage

Mac ZIPs contain `nus.app`; Windows ZIPs include the redistributable runtime,
all CEF resources and `bin/nus.exe`; Linux tarballs include `./nus`, the desktop
binary, CEF, locales and `bin/nus`. Windows/Linux packages keep user profiles in
the platform's user-data directory, never alongside an installed executable.
Linux packages target glibc 2.35+ and need the desktop libraries listed in their
README. They preserve Chromium's sandbox; they do not silently add `--no-sandbox`.

Transient workflow artifacts expire in three days. Published releases are
versioned downloads and are retained deliberately. There is no per-commit binary
publication, recurring capture, or unbounded automatic nightly archive.

Workflow runners: [GitHub's runner reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners).
Mac distribution requirements: [Apple notarization](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution).
