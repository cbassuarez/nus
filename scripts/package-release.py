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
# The signing scripts sign only with the complete set; the record must say the same.
MAC_SIGNING = ['MACOS_CERTIFICATE', 'MACOS_CERTIFICATE_PASSWORD', 'MACOS_SIGN_IDENTITY', 'APPLE_API_KEY', 'APPLE_API_KEY_ID', 'APPLE_API_ISSUER']
# Windows is signed between invocations: --stage-only leaves the payload here;
# Artifact Signing signs these executables in place; --build-installer verifies
# them and compiles the installer from them; Artifact Signing signs that; then
# --finalize-staged verifies everything and packages. Nothing else is signed.
WINDOWS_STAGE = 'dist/windows-stage'
WINDOWS_INSTALLER = 'dist/windows-installer'
WINDOWS_SIGNED = ['nus.exe', 'nus-hold.exe', 'bin/nus.exe']
WINDOWS_INSTRUCTIONS = 'Extract the entire folder, then open nus.exe. Keep its DLLs and locales together.\nThe shell CLI is bin/nus.exe. Settings live in %LOCALAPPDATA%/nus/installs/<channel>/<installation>/profile.\n'
configured = lambda names: all(os.environ.get(n) for n in names)

def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()

def iscc():
    """Inno Setup's compiler, preinstalled on GitHub's Windows runners."""
    found = os.environ.get('ISCC') or shutil.which('iscc')
    path = Path(found) if found else Path(os.environ.get('ProgramFiles(x86)', r'C:\Program Files (x86)'))/'Inno Setup 6/ISCC.exe'
    if not path.is_file(): raise FileNotFoundError('Inno Setup 6 (ISCC.exe) is required to build the Windows installer')
    return path

def describe(stage, args, stable, signing, instructions):
    (stage/'README.txt').write_text(f'nus {args.tag}\n\n{instructions}\nChannel: {"stable" if stable else "preview"}\nSigning: {signing}\nhttps://cbassuarez.com/nus.dev/download/\n')
    (stage/'nus-package.json').write_text(json.dumps({'version':args.tag,'target':args.target,'signing':signing}))

def finish(stage, name, out, args, stable, signing, instructions):
    """Describe the staged folder, then archive it under its release name."""
    describe(stage, args, stable, signing, instructions)
    if args.target.startswith('windows'):
        archive = out/f'{name}.zip'
        with zipfile.ZipFile(archive,'w',zipfile.ZIP_DEFLATED,compresslevel=6) as z:
            for path in sorted(stage.rglob('*')):
                if path.is_file(): z.write(path, Path(name)/path.relative_to(stage))
    else:
        archive = out/f'{name}.tar.gz'
        with tarfile.open(archive,'w:gz') as t: t.add(stage,arcname=name)
    return archive

def main():
    p = argparse.ArgumentParser()
    p.add_argument('--target', required=True, choices=sorted(TARGETS))
    p.add_argument('--tag', required=True)
    mode = p.add_mutually_exclusive_group()
    mode.add_argument('--stage-only', action='store_true', help='Windows: stage the unsigned payload for signing; do not archive')
    mode.add_argument('--build-installer', action='store_true', help='Windows: verify the signed stage, then compile its installer')
    mode.add_argument('--finalize-staged', action='store_true', help='Windows: verify the signed stage and installer, then archive them')
    mode.add_argument('--unsigned-preview', action='store_true', help='Windows: local preview package without signing')
    args = p.parse_args()
    if not re.fullmatch(r'v\d+\.\d+\.\d+(?:-preview\.\d+)?', args.tag):
        p.error('Expected vX.Y.Z or vX.Y.Z-preview.N')
    stable = '-preview.' not in args.tag
    windows = args.target.startswith('windows')
    if windows and not (args.stage_only or args.build_installer or args.finalize_staged or args.unsigned_preview):
        p.error('Windows packages are staged, signed, then finalized: pass --stage-only, --build-installer, --finalize-staged or --unsigned-preview')
    if not windows and (args.stage_only or args.build_installer or args.finalize_staged or args.unsigned_preview):
        p.error('Staging and signing modes apply only to Windows')
    if args.unsigned_preview and stable:
        p.error('Stable Windows releases must be signed')
    out = ROOT / 'dist/release'
    out.mkdir(parents=True, exist_ok=True)
    name = f'nus-{args.tag[1:]}-{args.target}'
    build = ROOT / 'spikes/composite/target/release'
    if args.target.startswith('macos'):
        app = ROOT / 'dist/nus.app'
        env = dict(os.environ, NUS_BUNDLE_OUT=str(app), NUS_BUNDLE_SKIP_BUILD='1')
        subprocess.run(['bash', str(ROOT/'scripts/bundle-mac.sh')], env=env, check=True)
        subprocess.run(['bash', str(ROOT/'scripts/sign-mac-release.sh'), str(app), 'stable' if stable else 'preview'], check=True)
        signing = 'notarized' if configured(MAC_SIGNING) else 'ad-hoc'
        archive = out / f'{name}.zip'
        subprocess.run(['ditto', '-c', '-k', '--sequesterRsrc', '--keepParent', str(app), str(archive)], check=True)
    elif args.build_installer or args.finalize_staged:
        stage = ROOT / WINDOWS_STAGE
        setup = ROOT / WINDOWS_INSTALLER / f'{name}-setup.exe'
        missing = [exe for exe in WINDOWS_SIGNED if not (stage/exe).is_file()]
        if args.finalize_staged and not setup.is_file(): missing.append(setup.name)
        if missing: raise FileNotFoundError(f'Staged Windows payload is incomplete: {", ".join(missing)}')
        # The record may only say authenticode once every signature has been checked.
        verify = ['pwsh','-NoProfile','-File',str(ROOT/'scripts/verify-windows-release.ps1'),'-Directory',str(stage)]
        subprocess.run(verify + (['-Installer',str(setup)] if args.finalize_staged else []), check=True)
        signing = 'authenticode'
        describe(stage, args, stable, signing, WINDOWS_INSTRUCTIONS)
        if args.build_installer:
            if setup.exists(): setup.unlink()
            subprocess.run([str(iscc()), '/Q', f'/DVersion={args.tag[1:]}', f'/DNumericVersion={args.tag[1:].split("-")[0]}',
                            f'/DChannel={"release" if stable else "preview"}', f'/DSourceDir={stage}', f'/DOutputDir={setup.parent}',
                            f'/DOutputName={setup.stem}', str(ROOT/'scripts/windows-installer.iss')], check=True)
            print(f'Built {setup}; sign it, then run --finalize-staged.')
            return
        archive = finish(stage, name, out, args, stable, signing, WINDOWS_INSTRUCTIONS)
        shutil.copy2(setup, out/setup.name)
        installer = {'name':setup.name,'sha256':digest(out/setup.name),'size':(out/setup.name).stat().st_size}
        (out/f'{setup.name}.sha256').write_text(f'{installer["sha256"]}  {setup.name}\n')
    else:
        stage = ROOT / (WINDOWS_STAGE if windows else f'dist/{name}')
        if stage.exists(): shutil.rmtree(stage)
        stage.mkdir(parents=True)
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
        if windows:
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
            signing, instructions = 'unsigned', WINDOWS_INSTRUCTIONS
        else:
            shutil.copy2(build/'composite', stage/'nus-desktop')
            shutil.copy2(ROOT/'target/release/nus-hold', stage/'nus-hold')
            shutil.copy2(ROOT/'target/release/nus', stage/'bin/nus')
            (stage/'nus').write_text('#!/bin/sh\nset -eu\ndir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)\nexport LD_LIBRARY_PATH="$dir${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"\nexec "$dir/nus-desktop" "$@"\n')
            (stage/'nus').chmod(0o755)
            shutil.copy2(ROOT/'assets/icon/nus-256.png', stage/'nus.png')
            (stage/'nus.desktop').write_text('[Desktop Entry]\nType=Application\nName=nus\nComment=A terminal and browser in one workspace\nExec=nus\nIcon=nus\nTerminal=false\nCategories=Development;TerminalEmulator;WebBrowser;\n')
            signing = 'checksum'
            instructions = 'Extract the entire folder and run ./nus. The shell CLI is bin/nus.\nRequires an x86-64 Linux desktop, glibc 2.35+, Vulkan, GTK 3, ALSA and NSS.\nSettings live in ${XDG_DATA_HOME:-$HOME/.local/share}/nus/installs/<channel>/<installation>/profile.\nTo add a desktop entry, copy nus.desktop to ~/.local/share/applications,\nset Exec and Icon to the absolute extracted paths, and keep the folder in place.\n'
        shutil.copy2(ROOT/'LICENSE', stage/'LICENSE')
        if args.stage_only:
            print(f'Staged unsigned Windows payload at {stage}; sign {", ".join(WINDOWS_SIGNED)}, then run --finalize-staged.')
            return
        archive = finish(stage, name, out, args, stable, signing, instructions)
    record = {'name':archive.name,'target':args.target,'version':args.tag,'channel':'stable' if stable else 'preview','sha256':digest(archive),'size':archive.stat().st_size,'signing':signing}
    if args.finalize_staged: record['installer'] = installer
    (out/f'{args.target}.json').write_text(json.dumps(record,indent=2)+'\n')
    (out/f'{archive.name}.sha256').write_text(f'{record["sha256"]}  {archive.name}\n')
    print(json.dumps(record))

if __name__ == '__main__': main()
