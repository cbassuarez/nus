#!/usr/bin/env python3
"""Package already-built native binaries. Never fetch or rebuild at this stage."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tarfile
import zipfile

ROOT = Path(__file__).resolve().parents[1]
TARGETS = {'macos-arm64', 'windows-x86_64', 'linux-x86_64'}

def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()

def main():
    p = argparse.ArgumentParser()
    p.add_argument('--target', required=True, choices=sorted(TARGETS))
    p.add_argument('--tag', required=True)
    args = p.parse_args()
    if not re.fullmatch(r'v\d+\.\d+\.\d+(?:-preview\.\d+)?', args.tag):
        p.error('Expected vX.Y.Z or vX.Y.Z-preview.N')
    stable = '-preview.' not in args.tag
    out = ROOT / 'dist/release'
    out.mkdir(parents=True, exist_ok=True)
    name = f'nus-{args.tag[1:]}-{args.target}'
    build = ROOT / 'spikes/composite/target/release'
    if args.target.startswith('macos'):
        app = ROOT / 'dist/nus.app'
        env = dict(os.environ, NUS_BUNDLE_OUT=str(app), NUS_BUNDLE_SKIP_BUILD='1')
        subprocess.run(['bash', str(ROOT/'scripts/bundle-mac.sh')], env=env, check=True)
        subprocess.run(['bash', str(ROOT/'scripts/sign-mac-release.sh'), str(app), 'stable' if stable else 'preview'], check=True)
        signing = 'notarized' if os.environ.get('MACOS_CERTIFICATE') else 'ad-hoc'
        archive = out / f'{name}.zip'
        subprocess.run(['ditto', '-c', '-k', '--sequesterRsrc', '--keepParent', str(app), str(archive)], check=True)
    else:
        stage = ROOT / 'dist' / name
        if stage.exists(): shutil.rmtree(stage)
        stage.mkdir()
        cef = ROOT / 'vendor/cef'
        # Keep all runtime payloads and all locales; exclude SDK/build inputs.
        for path in cef.iterdir():
            if path.is_file() and (path.suffix.lower() in {'.dll','.so','.pak','.bin','.dat','.json','.html'} or '.so.' in path.name or path.name == 'chrome-sandbox'):
                shutil.copy2(path, stage/path.name)
        for folder in ['locales', 'swiftshader', 'WidevineCdm']:
            if (cef/folder).is_dir(): shutil.copytree(cef/folder, stage/folder)
        (stage/'bin').mkdir()
        licenses=stage/'licenses'
        licenses.mkdir()
        for notice in (ROOT/'assets/fonts').iterdir():
            if notice.name.startswith(('OFL-', 'License-')): shutil.copy2(notice, licenses/notice.name)
        shutil.copy2(ROOT/'assets/icons/LICENSE',licenses/'icons.txt')
        if args.target.startswith('windows'):
            shutil.copy2(build/'composite.exe', stage/'nus.exe')
            shutil.copy2(ROOT/'target/release/nus-hold.exe', stage/'nus-hold.exe')
            shutil.copy2(ROOT/'target/release/nus.exe', stage/'bin/nus.exe')
            # The MSVC runtime is required on clean machines, not only runners.
            # The CRT folder is named for the toolset (VC143, VC145...), so find
            # the one that actually carries vcruntime140.dll.
            redists = [d for d in (Path(os.environ['VCToolsRedistDir'])/'x64').glob('Microsoft.VC*.CRT') if (d/'vcruntime140.dll').exists()]
            if not redists: raise RuntimeError('MSVC runtime missing under VCToolsRedistDir')
            for dll in sorted(redists)[-1].glob('*.dll'): shutil.copy2(dll, stage/dll.name)
            if not (stage/'vcruntime140.dll').exists(): raise RuntimeError('MSVC runtime missing')
            subprocess.run(['pwsh','-NoProfile','-File',str(ROOT/'scripts/sign-windows-release.ps1'),str(stage),'stable' if stable else 'preview'], check=True)
            signing = 'authenticode' if os.environ.get('WINDOWS_CERTIFICATE') else 'unsigned'
            instructions = 'Extract the entire folder, then open nus.exe. Keep its DLLs and locales together.\nThe shell CLI is bin/nus.exe. Settings live in %LOCALAPPDATA%/nus/profile.\n'
        else:
            shutil.copy2(build/'composite', stage/'nus-desktop')
            shutil.copy2(ROOT/'target/release/nus-hold', stage/'nus-hold')
            shutil.copy2(ROOT/'target/release/nus', stage/'bin/nus')
            (stage/'nus').write_text('#!/bin/sh\nset -eu\ndir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)\nexport LD_LIBRARY_PATH="$dir${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"\nexec "$dir/nus-desktop" "$@"\n')
            (stage/'nus').chmod(0o755)
            shutil.copy2(ROOT/'assets/icon/nus-256.png', stage/'nus.png')
            (stage/'nus.desktop').write_text('[Desktop Entry]\nType=Application\nName=nus\nComment=A terminal and browser in one workspace\nExec=nus\nIcon=nus\nTerminal=false\nCategories=Development;TerminalEmulator;WebBrowser;\n')
            signing = 'checksum'
            instructions = 'Extract the entire folder and run ./nus. The shell CLI is bin/nus.\nRequires an x86-64 Linux desktop, glibc 2.35+, Vulkan, GTK 3, ALSA and NSS.\nSettings live in ${XDG_DATA_HOME:-$HOME/.local/share}/nus/profile.\nTo add a desktop entry, copy nus.desktop to ~/.local/share/applications,\nset Exec and Icon to the absolute extracted paths, and keep the folder in place.\n'
        shutil.copy2(ROOT/'LICENSE', stage/'LICENSE')
        (stage/'README.txt').write_text(f'nus {args.tag}\n\n{instructions}\nChannel: {"stable" if stable else "preview"}\nSigning: {signing}\nhttps://nus.dev/download/\n')
        (stage/'nus-package.json').write_text(json.dumps({'version':args.tag,'target':args.target,'signing':signing}))
        if args.target.startswith('windows'):
            archive = out/f'{name}.zip'
            with zipfile.ZipFile(archive,'w',zipfile.ZIP_DEFLATED,compresslevel=6) as z:
                for path in sorted(stage.rglob('*')):
                    if path.is_file(): z.write(path, path.relative_to(stage.parent))
        else:
            archive = out/f'{name}.tar.gz'
            with tarfile.open(archive,'w:gz') as t: t.add(stage,arcname=name)
    record = {'name':archive.name,'target':args.target,'version':args.tag,'channel':'stable' if stable else 'preview','sha256':digest(archive),'size':archive.stat().st_size,'signing':signing}
    (out/f'{args.target}.json').write_text(json.dumps(record,indent=2)+'\n')
    (out/f'{archive.name}.sha256').write_text(f'{record["sha256"]}  {archive.name}\n')
    print(json.dumps(record))

if __name__ == '__main__': main()
