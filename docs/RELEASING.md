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
uploads the selected packages, `SHA256SUMS.txt` and `release.json`, then publishes.
Tag-triggered runs select all three platforms; only manual previews can select
a subset. Publication rejects inconsistent version/channel metadata and stable
packages without the required signature for their own platform.
It then sends a `release-published` dispatch to the nus.dev repository, which
refreshes the download page's fallback snapshot; this needs a
`SITE_DISPATCH_TOKEN` secret with write access to nus.dev, and without it the
site refreshes on its own schedule instead. The page itself reads GitHub
Releases live on every load.
A failed upload remains a draft. The downloads site reads only published releases
and only exposes assets matching this package contract. Missing channels and
failed API requests never become invented download links.

## Preparing a candidate

For `v0.0.1-preview.7`, both Cargo package versions remain `0.0.1`; the preview
number belongs to the release tag. Keep the [candidate review notes](releases/v0.0.1-preview.7.md)
with the same commit as the changes. Push that commit, then create and push its
version tag only after local checks have passed. Pushing the tag starts the
three-platform build **and authorizes automatic publication** if every job passes.
A branch push alone does not publish a release.

Watch the Release workflow for that exact tag through its publish job. A local
Mac bundle or a successful build-only run does not prove that the public release
exists. Before sharing the candidate, verify the published revision, platform
assets, archive checksums and signing labels in `release.json`. If a platform
fails, fix and rerun the unpublished candidate or explicitly choose a manual
scoped preview; never describe a partial result as a complete matrix. Once
published, use a new preview number for any change.

The publication script generates installation/signing notes automatically.
The candidate review document supplies the change summary and review boundaries;
it is not itself evidence of a successful workflow or signing operation.

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

Windows is signed by Azure Artifact Signing, with no exportable key. The
Windows matrix job builds and tests, then `package-release.py --stage-only`
leaves the unsigned payload in `dist/windows-stage`. A separate `sign-windows`
job, the only one with `id-token: write`, runs in the `windows-signing`
environment (variables `AZURE_CLIENT_ID`, `AZURE_TENANT_ID`,
`AZURE_SUBSCRIPTION_ID`) and signs exactly `nus.exe`, `nus-hold.exe` and
`bin/nus.exe` with RFC 3161 timestamps; bundled CEF, Widevine and MSVC binaries
are never signed as ours. `--build-installer` runs
`verify-windows-release.ps1`, which requires a valid, timestamped signature on
all three, then compiles `scripts/windows-installer.iss` with the runner's Inno
Setup into `nus-<version>-windows-x86_64-setup.exe`; that is signed the same
way. `--finalize-staged` verifies all four signatures (one signer) and only
then writes `Signing: authenticode`, the ZIP, both hashes and the record. The
job then installs silently, updates over it, launches the installed copy, and uninstalls. Every Windows package from the Release workflow is signed,
including build-only runs; if signing fails there is no Windows package. Local
packaging can make an explicitly unsigned preview with `--unsigned-preview`,
which stable tags refuse. An Apple certificate cannot sign Windows applications.

Mac preview packaging is ad-hoc unless the *complete* signing set is
configured — all six Apple secrets. A
partial set (a certificate without notarization keys, say) signs nothing and
says so in the log; a stable release refuses in that case. The package record
and the website identify the outcome accurately. Native install, media,
focus, accessibility and clean-machine tests remain release review gates; a
successful loader check is not a complete desktop acceptance test.

## Packages and storage

The Windows installer is per-user and never asks for elevation. Each channel
installs to one fixed folder, `%LOCALAPPDATA%\Programs\nus\<release|preview>`,
and records it in `%LOCALAPPDATA%\nus\installs\<channel>\installed-location`.
nus treats that folder as a single installation, so installer upgrades, in-app
updates (which swap the folder in place and refresh the version shown in
Installed apps) and reinstalls keep one profile; a portable ZIP elsewhere is
still its own installation. An upgrade replaces the whole folder, the
uninstaller lives outside it in `%LOCALAPPDATA%\nus\uninstall\<channel>`, and
uninstalling keeps profiles unless the person chooses *Remove them too* (a
silent uninstall always keeps them).

Setup is one Broadsheet sheet (`scripts/windows-installer.iss`; wordmark
bitmaps from `scripts/installer-art.py`): where it installs, then what it adds
to Windows — `path` (`bin` on PATH, on by default), `browser` (listed in
Default apps for http/https, off) and `desktopicon` (off). Those are also the
`/TASKS=` names. An update shows the same sheet with the last choices folded
into one line, and a task left out on an update is taken back out. If nus is
running, Setup asks once, asks its windows to close, then stops what is left,
held shells included; Restart Manager remains the fallback. The release job
installs with `path,browser`, updates with `path` alone, and uninstalls,
checking PATH and the browser registration at each step. The
installer is recorded as `installer` on the Windows asset, not as a second
asset, because updaters take the first asset for their target and must keep
receiving the ZIP. Stable Windows releases require it.

Mac ZIPs contain `nus.app`; Windows ZIPs include the redistributable runtime,
all CEF resources and `bin/nus.exe`, and the Windows installer installs that
same folder; Linux tarballs include `./nus`, the desktop
binary, CEF, locales and `bin/nus`. Packaged apps keep one profile per channel,
`nus/installs/<channel>/shared/profile` in the platform's user-data directory
(Application Support on macOS), never inside an installed executable.
Development and release channels are separate. Every copy of a channel —
rebuilt, redownloaded, moved or updated — opens that shared profile; the first
launch after upgrading moves the channel's most recently used profile into it
when no nus holds it, and otherwise starts it fresh with an import offer. One
copy uses the profile at a time (the profile lock), and a newer version saves a
recovery generation before upgrading it. A copy can keep a profile of its own
instead (Settings · Updates · Profile, listed in `installs/<channel>/separate`);
Welcome shows once per version per channel. Legacy `nus/profile` data is offered for
explicit settings import and is not overwritten. Browsing data is not imported.
Source checkouts continue to use their local `profile` directory.
Linux packages target glibc 2.35+ and need the desktop libraries listed in their
README. They preserve Chromium's sandbox; they do not silently add `--no-sandbox`.

Transient workflow artifacts expire in three days. Published releases are
versioned downloads and are retained deliberately. There is no per-commit binary
publication, recurring capture, or unbounded automatic nightly archive.

Workflow runners: [GitHub's runner reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners).
Mac distribution requirements: [Apple notarization](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution).
