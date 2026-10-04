# Releasing nus

The Release workflow builds three native packages: Apple Silicon Macs,
Windows x86-64, and Linux x86-64. Intel Macs are not a target. Native runners fetch the CEF version pinned by
the submodule, test both workspaces, build the app and CLI, package the complete
runtime, and exercise packaged HTTP browsing, JavaScript, rendered pixels and
native timeout/crash/hang recovery before publication.

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
`AZURE_SUBSCRIPTION_ID`) and signs exactly `nus.exe` (the CEF bootstrap), `nus.dll` (the application),
`chrome_elf.dll`, `nus-hold.exe` and `bin/nus.exe` with RFC 3161 timestamps. Other bundled CEF,
Widevine and MSVC binaries are never signed as ours. `chrome_elf.dll` is the one
exception because CEF requires it: a signed bootstrap exits at launch unless
`chrome_elf.dll` and the client DLL are signed with its own certificate.
`--build-installer` runs
`verify-windows-release.ps1`, which requires a valid, timestamped signature on
all five, then compiles `scripts/windows-installer.iss` with the runner's Inno
Setup into `nus-<version>-windows-x86_64-setup.exe`; that is signed the same
way. `--finalize-staged` verifies all six signatures (one signer) and only
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

The Windows installer is the Windows download: nus.dev offers only it, and the
release notes tell people to run it. The portable ZIP stays published because
it is not a download so much as a payload: the in-app updater, `install.ps1` and
the winget manifest all unpack it, and `publish-release.py` keeps it as the
first asset for its target. Do not drop it or rename it without changing those.

The installer is per-user and never asks for elevation. Each channel
installs to one fixed folder, `%LOCALAPPDATA%\Programs\nus\<release|preview>`,
and records it in `%LOCALAPPDATA%\nus\installs\<channel>\installed-location`.
nus treats that folder as a single installation, so installer upgrades, in-app
updates (which swap the folder in place and refresh the version shown in
Installed apps) and reinstalls keep one profile; a portable ZIP elsewhere is
still its own installation but shares that channel's profile by default. An upgrade replaces the whole folder, the
uninstaller lives outside it in `%LOCALAPPDATA%\nus\uninstall\<channel>`, and
uninstalling keeps profiles unless the person chooses *Remove them too* (a
silent uninstall keeps them unless `/REMOVELOCALDATA` is supplied).

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
when no nus holds it. A busy profile or failed move stops launch and preserves
the original; it never silently creates a duplicate local profile. This also
applies to local “Here” profiles without sync. One
copy uses the profile at a time (the profile lock), and a newer version saves a
recovery generation before upgrading it. A copy can keep a profile of its own
instead (Settings · Updates · Profile, listed in `installs/<channel>/separate`);
Welcome shows once per version per channel. Legacy `nus/profile` data is offered for
explicit settings import and is not overwritten. Browsing data is not imported.
Source checkouts continue to use their local `profile` directory.

`nus uninstall` keeps the local profile for a reinstall. `nus uninstall
--everything` explicitly removes this installation and its channel's profiles,
recovery copies, retained app packages owned by this installation, vault credentials
and logs; add `--yes` for unattended cleanup. Successful in-app updates remove
their download and staging folders.
Windows' **Remove them too** choice uses the same cleanup through the signed
CLI, and silent uninstall can request it with `/REMOVELOCALDATA`. A busy profile
blocks cleanup. Other channels, project files and external sync destinations
are preserved; a vault key still referenced by another local profile is kept.

Linux releases carry two forms of the same payload. CEF's libraries are
stripped first (libcef.so ships with full debug info: about 1.4 GB, 270 MB
stripped). `nus-desktop` finds libcef.so through an `$ORIGIN` runpath, so the
`./nus` launcher sets no `LD_LIBRARY_PATH` for shells to inherit, and it
resolves its own folder through symlinks.

- **`.deb`** (`scripts/package-linux.py`, recorded as `packages` on the Linux
  asset, like the Windows installer). Installs the channel to `/opt/nus` or
  `/opt/nus-preview` with `/usr/bin/nus` (an `update-alternatives` link, so both
  channels can be installed; Current wins), a system desktop entry, hicolor
  icons and AppStream metadata. Its postinst writes an AppArmor profile granting
  `userns` to that executable where the system has AppArmor abi 4.0 (Ubuntu
  24.04+ restricts unprivileged user namespaces otherwise), and `chrome-sandbox`
  is setuid root as the fallback where user namespaces are off. Its
  `nus-package.json` says `"managed": "deb"`: the in-app updater only reports
  new versions and leaves installing them to apt. The release workflow installs,
  runs and removes the package on the runner. `--archive <tar.gz>` repackages a
  published release.
- **tar.gz**. `./install-desktop.sh` copies the folder to
  `${XDG_DATA_HOME:-~/.local/share}/nus/app/<channel>`, links
  `~/.local/bin/nus` (the copy installed last answers), registers the desktop entry (replacing one whose
  copy was deleted) and checks the sandbox. Where AppArmor blocks it, it prints
  `sudo sh <installed>/install-desktop.sh --allow-sandbox`, which writes a
  profile for that one copy. `--uninstall` removes the copy and keeps settings.
  In-app updates work for this copy as before.

**Install commands.** The site serves the one-liners
(`cbassuarez.com/nus.dev/install.sh` and `install.ps1`, in the nus.dev
repository). Each checks downloads against the release's `SHA256SUMS.txt`:
on Debian and Ubuntu it installs the .deb with apt, on other Linux the archive
with `install-desktop.sh`, on Windows the signed installer silently, and on
macOS nus.app. Package names and the `nus@preview` / `.Preview` identifiers
below must stay in step with them and with the download page.

**The apt repository** lives in GitHub Releases: after publishing, the release
workflow builds a signed flat repository (`scripts/apt-repo.py`: the .deb,
Packages, Release, InRelease) and uploads it to the rolling release
`apt-preview` or `apt-release`, deleting the previous .deb. apt follows GitHub's
download redirects, so nothing else needs hosting. The tags have no leading
`v`, so the in-app updater ignores them. One-time setup:

1. Create a signing key used only for this:
   `gpg --quick-gen-key 'nus packages <contact@cbassuarez.com>' ed25519 sign never`.
2. Store the private key (`gpg --armor --export-secret-keys <fingerprint>`) as
   the secret `NUS_APT_SIGNING_KEY`, and the public key
   (`gpg --armor --export <fingerprint>`) as the variable `NUS_APT_PUBLIC_KEY`.

From the next release on, each .deb ships
`/etc/apt/sources.list.d/<package>.sources` and the keyring, so installing it
once subscribes the machine to its channel. Packages built before then install
and run, but do not update themselves. Without the secret the step is skipped.

**Homebrew and winget.** After publishing, the release workflow renders the
release for each package manager from its verified `release.json`
(`scripts/package-managers.py`):

- **Homebrew**: `Casks/nus.rb` or `Casks/nus@preview.rb`, committed to the tap
  `github.com/cbassuarez/homebrew-tap` (`brew install cbassuarez/tap/nus@preview`;
  `brew install` finds casks without `--cask`). The casks conflict, since both
  install `nus.app`; each links the `nus` command. A cask, not a formula: Homebrew
  installs GUI apps only as casks, and homebrew-core formulae must build from
  source with no prebuilt payloads. For a bare `brew install nus`, the cask
  joins `homebrew/cask`, which takes notarized, stable, notable apps: submit
  `Casks/nus.rb` once the first stable release is notarized; its autobump then
  replaces the tap for that channel. `auto_updates` leaves updating to
  the app. Ad-hoc signed builds get a postflight that clears Homebrew's
  quarantine flag, without which macOS reports the app as damaged; notarized
  builds keep it. Setup: the tap `cbassuarez/homebrew-tap` exists;
  add a fine-grained token with Contents: write on it as the secret
  `HOMEBREW_TAP_TOKEN`.
- **winget**: manifests for `cbassuarez.nus` / `cbassuarez.nus.Preview`
  (the Inno installer, per-user scope, `nus` on PATH), submitted to
  `microsoft/winget-pkgs` with `wingetcreate`. Microsoft's validation and
  moderators merge them; the first submission of each identifier takes longest.
  Setup: a classic token with `public_repo` scope, from the account whose fork
  the pull requests come from, as the secret `WINGET_TOKEN`.

Without their secrets, both jobs skip. Neither can change a published release.

nus checks Chromium's sandbox before starting CEF and shows a native error if
it is unavailable. The probe maps its uid in the new namespace, because Ubuntu's
restriction lets `unshare` itself succeed. nus never silently adds `--no-sandbox`.

`python scripts/check-browser.py --app <package> --out <new-directory>` requires
Pillow and a usable desktop GPU. It keeps an isolated profile, Chromium log,
screenshots and JSON results. Use `--software` on macOS to exercise the BGRA
upload path used on Windows/Linux; macOS defaults to shared GPU textures.
Windows/Linux currently trade shared-texture zero-copy performance for reliable
BGRA upload, keeping Chromium's GPU compositing enabled.

Transient workflow artifacts expire in three days. Published releases are
versioned downloads and are retained deliberately. There is no per-commit binary
publication, recurring capture, or unbounded automatic nightly archive.

Workflow runners: [GitHub's runner reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners).
Mac distribution requirements: [Apple notarization](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution).
