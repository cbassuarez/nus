#!/usr/bin/env python3
"""Make a signed flat apt repository for one channel, ready to upload as release assets.

    apt-repo.py --out <folder> --key <fingerprint> nus-preview_0.0.2~preview.9_amd64.deb

A flat repository is one folder: the .deb, Packages(.gz), Release, InRelease
and Release.gpg, with no dists/ or pool/. That fits GitHub Releases, which has
no subfolders: the release workflow uploads the folder to the rolling release
`apt-<channel>`, and each .deb's sources entry points apt there
(package-linux.py apt_source). apt follows GitHub's download redirects. The
repository carries the newest version only, so apt always installs the latest.
Beside it go the public keyring and <package>.sources, byte-identical to the
files the .deb installs, so setting the repository up by hand first and then
installing the package raises no configuration-file prompt.
Signing uses gpg's default home (or GNUPGHOME) and the given key.
"""
import argparse
import email.utils
import gzip
import hashlib
import importlib.util
from pathlib import Path
import shutil
import subprocess

_spec = importlib.util.spec_from_file_location('package_linux', Path(__file__).with_name('package-linux.py'))
linux = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(linux)

def fields(deb):
    out = subprocess.run(['dpkg-deb', '-f', str(deb)], check=True, capture_output=True, text=True).stdout
    control, last = {}, None
    for line in out.splitlines():
        if line[:1] in (' ', '\t') and last: control[last] += '\n' + line
        elif ':' in line:
            last, value = line.split(':', 1)
            control[last] = value.strip()
    return control

def build(debs, out, key):
    out.mkdir(parents=True, exist_ok=True)
    stanzas, packages = [], set()
    for deb in debs:
        control = fields(deb)
        if control.get('Architecture') != 'amd64' or control.get('Package') not in ('nus', 'nus-preview'):
            raise ValueError(f'{deb} is not a nus amd64 package')
        packages.add(control['Package'])
        shutil.copy2(deb, out/deb.name)
        data = deb.read_bytes()
        control.update({'Filename': f'./{deb.name}', 'Size': str(len(data)), 'MD5sum': hashlib.md5(data).hexdigest(),
                        'SHA1': hashlib.sha1(data).hexdigest(), 'SHA256': hashlib.sha256(data).hexdigest()})
        stanzas.append('\n'.join(f'{k}: {v}' for k, v in control.items()))
    if len(packages) != 1: raise ValueError('A channel repository holds exactly one package')
    channel = 'preview' if packages == {'nus-preview'} else 'release'
    text = '\n\n'.join(stanzas) + '\n'
    (out/'Packages').write_text(text)
    with gzip.GzipFile(out/'Packages.gz', 'wb', mtime=0) as z: z.write(text.encode())
    release = ['Origin: nus', 'Label: nus', f'Suite: {channel}', 'Architectures: amd64',
               f'Description: nus {channel} channel', f'Date: {email.utils.formatdate(usegmt=True)}']
    for field, algorithm in [('MD5Sum', 'md5'), ('SHA256', 'sha256')]:
        release.append(f'{field}:')
        for index in ['Packages', 'Packages.gz']:
            data = (out/index).read_bytes()
            release.append(f' {hashlib.new(algorithm, data).hexdigest()} {len(data):>16} {index}')
    (out/'Release').write_text('\n'.join(release) + '\n')
    gpg = ['gpg', '--batch', '--yes', '--pinentry-mode', 'loopback', '--local-user', key, '--digest-algo', 'SHA256']
    subprocess.run(gpg + ['--clearsign', '--output', str(out/'InRelease'), str(out/'Release')], check=True)
    subprocess.run(gpg + ['--armor', '--detach-sign', '--output', str(out/'Release.gpg'), str(out/'Release')], check=True)
    public = subprocess.run(['gpg', '--batch', '--export', key], check=True, capture_output=True).stdout
    if not public: raise ValueError(f'No public key for {key}')
    (out/'nus-archive-keyring.gpg').write_bytes(public)
    (out/f'{packages.pop()}.sources').write_text(linux.apt_source(channel))
    return channel

def main():
    p = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    p.add_argument('debs', nargs='+', type=Path)
    p.add_argument('--out', type=Path, required=True)
    p.add_argument('--key', required=True, help='fingerprint of the signing key')
    args = p.parse_args()
    print(build(args.debs, args.out, args.key))

if __name__ == '__main__': main()
