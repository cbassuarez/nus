#!/usr/bin/env python3
"""Build the Debian package from a staged Linux payload (or a released archive).

The package installs one channel to /opt/<package>, owned by root, and gives it
what an extracted archive cannot have: the `nus` command on PATH (an
alternative, so both channels can be installed; Current wins), a system desktop
entry and icons, an AppArmor profile that lets Chromium create its sandbox on
Ubuntu 24.04+, a setuid sandbox helper where user namespaces are off, and,
given the repository's public key, updates through apt from the flat
repository the release workflow keeps in the `apt-<channel>` GitHub release. nus-package.json says
"managed": "deb", so the in-app updater leaves the copy to apt.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tarfile
import tempfile
import time

MAINTAINER = 'Sebastian Suarez Solis <contact@cbassuarez.com>'
HOMEPAGE = 'https://cbassuarez.com/nus.dev/'
REPOSITORY = 'https://github.com/cbassuarez/nus'
ICON_SIZES = [16, 24, 32, 48, 64, 128, 256, 512]
# From the bundle's ELF dependencies and what nus loads at runtime (Wayland,
# xkbcommon-x11, GTK file dialogs, xdg-mime/xdg-settings, curl for updates).
# t64 names first (Ubuntu 24.04+, Debian 13), then their older names.
DEPENDS = [
    'libc6 (>= 2.35)', 'libstdc++6', 'libgcc-s1',
    'libasound2t64 | libasound2', 'libatk1.0-0t64 | libatk1.0-0', 'libatk-bridge2.0-0t64 | libatk-bridge2.0-0',
    'libatspi2.0-0t64 | libatspi2.0-0', 'libcairo2', 'libcups2t64 | libcups2', 'libdbus-1-3', 'libdrm2', 'libexpat1',
    'libgbm1', 'libglib2.0-0t64 | libglib2.0-0', 'libgtk-3-0t64 | libgtk-3-0', 'libnspr4', 'libnss3', 'libpango-1.0-0',
    'libudev1', 'libwayland-client0', 'libx11-6', 'libxcb1', 'libxcomposite1', 'libxdamage1', 'libxext6', 'libxfixes3',
    'libxkbcommon0', 'libxkbcommon-x11-0', 'libxrandr2', 'ca-certificates', 'curl', 'xdg-utils',
]
RECOMMENDS = ['mesa-vulkan-drivers', 'fonts-liberation']

def channel(tag):
    return 'release' if '-preview.' not in tag else 'preview'

def package_name(tag):
    return 'nus' if channel(tag) == 'release' else 'nus-preview'

def app_id(tag):
    return 'dev.nus.app' if channel(tag) == 'release' else 'dev.nus.app.preview'

def deb_version(tag):
    """v0.0.2-preview.9 -> 0.0.2~preview.9, which sorts before 0.0.2."""
    m = re.fullmatch(r'v(\d+\.\d+\.\d+)(?:-(preview\.\d+))?', tag)
    if not m: raise ValueError('Expected vX.Y.Z or vX.Y.Z-preview.N')
    return m[1] + (f'~{m[2]}' if m[2] else '')

def desktop_entry(tag):
    """Must agree with default_browser/linux.rs desktop(): Type, Exec, MimeType
    and X-Nus-Owner, in that order, are what registration compares."""
    root = f'/opt/{package_name(tag)}'
    owner = hashlib.sha256(root.encode()).hexdigest()
    title = 'nus' if channel(tag) == 'release' else 'nus Preview'
    return (f'[Desktop Entry]\nType=Application\nName={title}\nGenericName=Terminal and Web Browser\n'
            f'Comment=A terminal and browser\nExec="{root}/nus" --open-external -- %U\nIcon={app_id(tag)}\n'
            'Terminal=false\nStartupNotify=true\nCategories=Development;TerminalEmulator;WebBrowser;\n'
            'Keywords=terminal;shell;browser;web;\nMimeType=x-scheme-handler/http;x-scheme-handler/https;\n'
            f'StartupWMClass=nus\nX-Nus-Owner={owner}\n')

def metainfo(tag):
    name = 'nus' if channel(tag) == 'release' else 'nus Preview'
    return f'''<?xml version="1.0" encoding="UTF-8"?>
<component type="desktop-application">
  <id>{app_id(tag)}</id>
  <metadata_license>MIT</metadata_license>
  <project_license>MIT</project_license>
  <name>{name}</name>
  <summary>A terminal that is also a browser</summary>
  <description>
    <p>nus is a terminal, web browser and workspace in one application.</p>
  </description>
  <launchable type="desktop-id">{app_id(tag)}.desktop</launchable>
  <url type="homepage">{HOMEPAGE}</url>
  <url type="vcs-browser">https://github.com/cbassuarez/nus</url>
  <developer id="com.cbassuarez"><name>Sebastian Suarez Solis</name></developer>
  <content_rating type="oars-1.1"/>
  <releases><release version="{deb_version(tag)}" date="{time.strftime('%Y-%m-%d', time.gmtime())}"/></releases>
</component>
'''

def apparmor_profile(name, executable):
    """Ubuntu 24.04+ denies unprivileged user namespaces to unconfined
    programs that have no profile saying otherwise. Chromium's sandbox needs
    them. This grants exactly that to the installed executable, nothing else."""
    return f'''# Allows nus's Chromium sandbox to create user namespaces. Installed by {name}.
abi <abi/4.0>,
include <tunables/global>

profile {name} {executable} flags=(unconfined) {{
  userns,

  include if exists <local/{name}>
}}
'''

# Both channels provide /usr/bin/nus; with both installed, Current wins.
ALTERNATIVE_PRIORITY = {'nus': 100, 'nus-preview': 50}

def maintainer_scripts(name):
    profile = apparmor_profile(name, f'/opt/{name}/nus-desktop')
    alternative = f'/opt/{name}/bin/nus'
    # The profile is written only where the parser knows abi 4.0 and userns;
    # older systems never restricted user namespaces and need no profile.
    postinst = f'''#!/bin/sh
set -e
if [ "$1" = configure ]; then
    update-alternatives --install /usr/bin/nus nus {alternative} {ALTERNATIVE_PRIORITY[name]}
    if [ -f /etc/apparmor.d/abi/4.0 ]; then
        cat > /etc/apparmor.d/{name} <<'EOF'
{profile}EOF
        if [ -d /sys/kernel/security/apparmor ] && command -v apparmor_parser >/dev/null 2>&1; then
            apparmor_parser -r -T -W /etc/apparmor.d/{name} || echo "{name}: could not load its AppArmor profile; Chromium's sandbox may be unavailable." >&2
        fi
    fi
fi
exit 0
'''
    prerm = f'''#!/bin/sh
set -e
if [ "$1" = remove ] || [ "$1" = deconfigure ]; then
    update-alternatives --remove nus {alternative}
fi
exit 0
'''
    postrm = f'''#!/bin/sh
set -e
if [ "$1" = remove ] || [ "$1" = purge ]; then
    if [ -f /etc/apparmor.d/{name} ]; then
        if [ -d /sys/kernel/security/apparmor ] && command -v apparmor_parser >/dev/null 2>&1; then
            apparmor_parser -R /etc/apparmor.d/{name} >/dev/null 2>&1 || true
        fi
        rm -f /etc/apparmor.d/{name}
    fi
fi
exit 0
'''
    return postinst, prerm, postrm

def apt_repository(channel):
    """A flat repository (scripts/apt-repo.py) kept as a rolling release."""
    return f'{REPOSITORY}/releases/download/apt-{channel}/'

def apt_source(channel):
    """The package's conffile; apt-repo.py publishes the same bytes for manual setup."""
    return (f'Types: deb\nURIs: {apt_repository(channel)}\nSuites: ./\nArchitectures: amd64\n'
            'Signed-By: /usr/share/keyrings/nus-archive-keyring.gpg\n')

def is_elf(path):
    with path.open('rb') as f: return f.read(4) == b'\x7fELF'

def build_deb(stage, tag, out, keyring=None):
    """Package `stage` (the same folder the tar.gz carries) as a .deb in `out`."""
    name, version = package_name(tag), deb_version(tag)
    if not (stage/'nus-desktop').is_file() or not (stage/'nus-package.json').is_file():
        raise FileNotFoundError(f'{stage} is not a staged nus Linux payload')
    out.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='nus-deb-') as tmp:
        root = Path(tmp)/'root'
        opt = root/'opt'/name
        shutil.copytree(stage, opt, symlinks=True)
        # The archive's user installer and instructions don't apply here.
        for extra in ['install-desktop.sh', 'README.txt']:
            (opt/extra).unlink(missing_ok=True)
        record = json.loads((opt/'nus-package.json').read_text())
        record['managed'] = 'deb'
        (opt/'nus-package.json').write_text(json.dumps(record))
        usr = root/'usr'
        apps = usr/'share/applications'
        apps.mkdir(parents=True)
        (apps/f'{app_id(tag)}.desktop').write_text(desktop_entry(tag))
        (usr/'share/metainfo').mkdir(parents=True)
        (usr/'share/metainfo'/f'{app_id(tag)}.metainfo.xml').write_text(metainfo(tag))
        icons = sorted((opt/'icons').glob('nus-*.png')) if (opt/'icons').is_dir() else []
        for icon in icons or [opt/'nus.png']:
            size = icon.stem.removeprefix('nus-') if icons else '256'
            target = usr/'share/icons/hicolor'/f'{size}x{size}'/'apps'
            target.mkdir(parents=True, exist_ok=True)
            shutil.copy2(icon, target/f'{app_id(tag)}.png')
        doc = usr/'share/doc'/name
        doc.mkdir(parents=True)
        shutil.copy2(opt/'LICENSE', doc/'copyright')
        conffiles = []
        if keyring:
            sources = root/'etc/apt/sources.list.d'
            sources.mkdir(parents=True)
            (sources/f'{name}.sources').write_text(apt_source(channel(tag)))
            conffiles.append(f'/etc/apt/sources.list.d/{name}.sources')
            (usr/'share/keyrings').mkdir(parents=True)
            shutil.copy2(keyring, usr/'share/keyrings/nus-archive-keyring.gpg')
        for path in [root, *root.rglob('*')]:
            if not path.is_symlink(): path.chmod(0o755 if path.is_dir() or os.access(path, os.X_OK) else 0o644)
        # Fallback where user namespaces are off entirely (Chromium prefers them).
        if (opt/'chrome-sandbox').is_file(): (opt/'chrome-sandbox').chmod(0o4755)
        control = root/'DEBIAN'
        control.mkdir()
        size = sum(p.stat().st_size for p in root.rglob('*') if p.is_file() and not p.is_symlink()) // 1024
        title = 'nus' if channel(tag) == 'release' else 'nus Preview'
        (control/'control').write_text(
            f'Package: {name}\nVersion: {version}\nArchitecture: amd64\nMaintainer: {MAINTAINER}\n'
            f'Installed-Size: {size}\nDepends: {", ".join(DEPENDS)}\nRecommends: {", ".join(RECOMMENDS)}\n'
            f'Section: web\nPriority: optional\nHomepage: {HOMEPAGE}\n'
            f'Description: {title}: a terminal that is also a browser\n'
            ' nus is a terminal, web browser and workspace in one application.\n'
            ' This package installs the ' + channel(tag) + ' channel to /opt/' + name + '.\n')
        postinst, prerm, postrm = maintainer_scripts(name)
        for script, text in [('postinst', postinst), ('prerm', prerm), ('postrm', postrm)]:
            (control/script).write_text(text)
            (control/script).chmod(0o755)
        if conffiles: (control/'conffiles').write_text(''.join(f'{c}\n' for c in conffiles))
        # The file is named for the tag: GitHub rewrites `~` in asset names, so a
        # name with the Debian version would not be the name it serves.
        deb = out/f'{name}_{tag[1:]}_amd64.deb'
        # Root ownership without root: dpkg-deb records root:root itself.
        subprocess.run(['dpkg-deb', '--root-owner-group', '-Zxz', '--build', str(root), str(deb)], check=True, stdout=subprocess.DEVNULL)
    return deb

def main():
    p = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    source = p.add_mutually_exclusive_group(required=True)
    source.add_argument('--stage', type=Path, help='a staged payload folder (dist/nus-<version>-linux-x86_64)')
    source.add_argument('--archive', type=Path, help='a released nus-<version>-linux-x86_64.tar.gz')
    p.add_argument('--tag', help='vX.Y.Z[-preview.N]; read from nus-package.json when omitted')
    p.add_argument('--out', type=Path, default=Path('dist/release'))
    p.add_argument('--keyring', type=Path, default=os.environ.get('NUS_APT_KEYRING'), help='public keyring (binary .gpg) of the apt repository; adds it as an update source')
    args = p.parse_args()
    with tempfile.TemporaryDirectory(prefix='nus-archive-') as tmp:
        stage = args.stage
        if args.archive:
            with tarfile.open(args.archive) as t: t.extractall(tmp, filter='tar')
            [stage] = [d for d in Path(tmp).iterdir() if d.is_dir()]
        tag = args.tag or json.loads((stage/'nus-package.json').read_text())['version']
        print(build_deb(stage, tag, args.out, args.keyring))

if __name__ == '__main__': main()
