#!/usr/bin/env python3
"""Render a published release for the package managers people install nus with.

    package-managers.py homebrew --manifest dist/release/release.json --out <tap checkout>
    package-managers.py winget   --manifest dist/release/release.json --out <folder>

  macOS     brew install cbassuarez/tap/nus               (cbassuarez/tap/nus@preview)
  Windows   winget install cbassuarez.nus                 (cbassuarez.nus.Preview)
  Linux     apt, from the repository in the apt-<channel> release (apt-repo.py)

Everything comes from release.json, which publish-release.py verified against
the uploaded assets. The release workflow commits the cask to the tap
(github.com/cbassuarez/homebrew-tap) and submits the winget manifests to
microsoft/winget-pkgs.
"""
import argparse
import json
from pathlib import Path

REPOSITORY = 'https://github.com/cbassuarez/nus'
HOMEPAGE = 'https://cbassuarez.com/nus.dev/'
DESCRIPTION = 'Terminal that is also a browser'
# The Inno Setup AppIds in windows-installer.iss; never change them.
WINDOWS_APP_IDS = {'release': '{E991BB0F-52B1-4A72-ABE7-395558E71B60}', 'preview': '{74ED99B0-ACEA-4860-A2AA-86FD0B170CFB}'}

def load(path):
    manifest = json.loads(path.read_text())
    if manifest.get('schema') != 1 or manifest.get('state') != 'active':
        raise ValueError('Not an active release manifest')
    return manifest

def asset(manifest, target):
    return next((a for a in manifest['assets'] if a['target'] == target), None)

def preview(manifest):
    return manifest['channel'] == 'preview'

def homebrew(manifest):
    """One cask per channel: nus, nus@preview. Both install nus.app, so they conflict."""
    mac = asset(manifest, 'macos-arm64')
    if mac is None: return None
    version = manifest['version'].removeprefix('v')
    token = 'nus@preview' if preview(manifest) else 'nus'
    other = 'nus' if preview(manifest) else 'nus@preview'
    pattern = r'^v?(\d+(?:\.\d+)+-preview\.\d+)$' if preview(manifest) else r'^v?(\d+(?:\.\d+)+)$'
    # Gatekeeper refuses an ad-hoc signed app that carries Homebrew's quarantine
    # flag ("damaged"). Notarized builds keep it; only ad-hoc previews drop it.
    quarantine = '' if mac['signing'] == 'notarized' else f'''
  # This build is ad-hoc signed, not notarized: without this, macOS reports it
  # as damaged. Notarized releases keep the quarantine check.
  postflight do
    system_command "/usr/bin/xattr", args: ["-dr", "com.apple.quarantine", "#{{appdir}}/nus.app"]
  end
'''
    return token, f'''cask "{token}" do
  version "{version}"
  sha256 "{mac['sha256']}"

  url "{REPOSITORY}/releases/download/v#{{version}}/nus-#{{version}}-macos-arm64.zip"
  name "nus{' Preview' if preview(manifest) else ''}"
  desc "{DESCRIPTION}"
  homepage "{HOMEPAGE}"

  livecheck do
    url "{REPOSITORY}.git"
    regex(/{pattern}/i)
    strategy :git
  end

  auto_updates true
  conflicts_with cask: "{other}"
  depends_on arch: :arm64
  depends_on macos: ">= :big_sur"

  app "nus.app"
  binary "#{{appdir}}/nus.app/Contents/Resources/bin/nus"
{quarantine}
  zap trash: [
    "~/Library/Application Support/nus",
    "~/Library/Caches/dev.nus.app",
    "~/Library/HTTPStorages/dev.nus.app",
    "~/Library/Preferences/dev.nus.app.plist",
    "~/Library/Saved Application State/dev.nus.app.savedState",
  ]
end
'''

def winget(manifest):
    """The three-file multi-file manifest winget-pkgs expects, for the Inno installer."""
    windows = asset(manifest, 'windows-x86_64')
    if windows is None or 'installer' not in windows or windows['signing'] != 'authenticode': return None
    channel = 'preview' if preview(manifest) else 'release'
    identifier = 'cbassuarez.nus.Preview' if preview(manifest) else 'cbassuarez.nus'
    version = manifest['version'].removeprefix('v')
    head = f'PackageIdentifier: {identifier}\nPackageVersion: {version}\n'
    schema = lambda kind: f'# yaml-language-server: $schema=https://aka.ms/winget-manifest.{kind}.1.9.0.schema.json\n'
    tail = lambda kind: f'ManifestType: {kind}\nManifestVersion: 1.9.0\n'
    installer = windows['installer']
    files = {
        f'{identifier}.yaml': schema('version') + head + 'DefaultLocale: en-US\n' + tail('version'),
        f'{identifier}.installer.yaml': schema('installer') + head + f'''InstallerType: inno
Scope: user
InstallModes:
- interactive
- silent
- silentWithProgress
UpgradeBehavior: install
ProductCode: '{WINDOWS_APP_IDS[channel]}_is1'
Commands:
- nus
Installers:
- Architecture: x64
  InstallerUrl: {REPOSITORY}/releases/download/{manifest['version']}/{installer['name']}
  InstallerSha256: {installer['sha256'].upper()}
''' + tail('installer'),
        f'{identifier}.locale.en-US.yaml': schema('defaultLocale') + head + f'''PackageLocale: en-US
Publisher: nus
PublisherUrl: {HOMEPAGE}
PublisherSupportUrl: {REPOSITORY}/issues
PackageName: nus{' Preview' if preview(manifest) else ''}
PackageUrl: {HOMEPAGE}
License: MIT
LicenseUrl: {REPOSITORY}/blob/{manifest['version']}/LICENSE
ShortDescription: A terminal that is also a browser.
Description: nus is a terminal, web browser and workspace in one application.
Moniker: {'nus-preview' if preview(manifest) else 'nus'}
Tags:
- terminal
- browser
- shell
- developer-tools
ReleaseNotesUrl: {REPOSITORY}/releases/tag/{manifest['version']}
''' + tail('defaultLocale'),
    }
    letter = identifier[0].lower()
    return Path('manifests', letter, *identifier.split('.'), version), files

def main():
    p = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    p.add_argument('manager', choices=['homebrew', 'winget'])
    p.add_argument('--manifest', type=Path, required=True)
    p.add_argument('--out', type=Path, required=True)
    args = p.parse_args()
    manifest = load(args.manifest)
    if args.manager == 'homebrew':
        cask = homebrew(manifest)
        if cask is None: return print('No macOS package in this release')
        token, text = cask
        (args.out/'Casks').mkdir(parents=True, exist_ok=True)
        (args.out/'Casks'/f'{token}.rb').write_text(text)
        print(args.out/'Casks'/f'{token}.rb')
    else:
        rendered = winget(manifest)
        if rendered is None: return print('No signed Windows installer in this release')
        folder, files = rendered
        (args.out/folder).mkdir(parents=True, exist_ok=True)
        for name, text in files.items(): (args.out/folder/name).write_text(text)
        print(args.out/folder)

if __name__ == '__main__': main()
