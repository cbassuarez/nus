#!/usr/bin/env python3
"""Assemble a complete, immutable release from verified matrix artifacts."""
import argparse
import json
import re
from pathlib import Path
import subprocess

import importlib.util
spec = importlib.util.spec_from_file_location('package_release', Path(__file__).with_name('package-release.py'))
package = importlib.util.module_from_spec(spec)
spec.loader.exec_module(package)

support_spec = importlib.util.spec_from_file_location('release_support', Path(__file__).with_name('release-support.py'))
support = importlib.util.module_from_spec(support_spec)
support_spec.loader.exec_module(support)

def main():
    p=argparse.ArgumentParser()
    p.add_argument('--tag',required=True)
    p.add_argument('--revision',required=True)
    p.add_argument('--targets',default=','.join(sorted(package.TARGETS)))
    p.add_argument('--directory',type=Path,default=Path('dist/release'))
    p.add_argument('--support-policy',type=Path,default=package.ROOT/'release-support.json')
    args=p.parse_args()
    if not re.fullmatch(r'v\d+\.\d+\.\d+(?:-preview\.\d+)?', args.tag):
        raise ValueError('Expected vX.Y.Z or vX.Y.Z-preview.N')
    channel='preview' if '-preview.' in args.tag else 'stable'
    targets=set(args.targets.split(','))
    if not targets or not targets <= package.TARGETS: raise ValueError('Unknown release targets')
    if '-preview.' not in args.tag and targets != package.TARGETS: raise ValueError('Stable requires every platform')
    entries=[]
    for target in sorted(targets):
        entry=json.loads((args.directory/f'{target}.json').read_text())
        archive=args.directory/entry['name']
        if entry['version'] != args.tag or entry['target'] != target or archive.name != entry['name'] or entry['channel'] != channel:
            raise ValueError('Mixed release artifacts')
        if archive.stat().st_size != entry['size'] or package.digest(archive) != entry['sha256']:
            raise ValueError(f'Invalid archive: {archive.name}')
        required_signing={'macos-arm64':'notarized','windows-x86_64':'authenticode','linux-x86_64':'checksum'}
        if channel == 'stable' and entry['signing'] != required_signing[target]:
            raise ValueError('A stable desktop package is not signed')
        entries.append(entry)
    maintenance=support.evaluate(json.loads(args.support_policy.read_text()),args.tag)
    manifest={'state':'active','maintenance':maintenance,'schema':1,'version':args.tag,'revision':args.revision,'channel':entries[0]['channel'],'assets':entries}
    (args.directory/'release.json').write_text(json.dumps(manifest,indent=2)+'\n')
    (args.directory/'SHA256SUMS.txt').write_text(''.join(f'{e["sha256"]}  {e["name"]}\n' for e in entries))
    notes=args.directory/'notes.md'
    lines=[f'nus {args.tag}', '', 'A terminal, browser and workspace in one application.', '', f'Channel: **{manifest["channel"]}** · Source: `{args.revision}`']
    candidate=package.ROOT/'docs/releases'/f'{args.tag}.md'
    if candidate.is_file():
        # Only the deliberately short summary is copied. The complete document
        # keeps its relative links in GitHub, pinned to the released revision.
        summary=re.search(r'^## Release summary\s*\n(.*?)(?=^## |\Z)',candidate.read_text(),re.M|re.S)
        if summary:
            bullets=[line for line in summary[1].strip().splitlines() if line.startswith('- ')]
            if bullets: lines += ['', '## Changes', '', *bullets]
        url=f'https://github.com/cbassuarez/nus/blob/{args.revision}/docs/releases/{args.tag}.md'
        lines += ['', f'[Candidate review notes, compatibility and known limitations]({url})']
    lines += ['', '## Downloads', '', '| Package | Signing |', '| --- | --- |']
    lines.extend(f'| {e["target"]} | {e["signing"]} |' for e in entries)
    omitted=sorted(package.TARGETS-targets)
    if omitted: lines += ['', 'Not included in this preview: '+', '.join(omitted)+'. These packages remain unavailable until their platform checks and signing setup are complete.']
    lines += ['', 'Extract the complete package before launching. macOS: move nus.app to Applications. Windows: launch nus.exe. Linux: run ./nus; see README.txt for desktop integration and runtime dependencies.', '', 'Preview builds are for early testing. Unsigned Windows previews can show a SmartScreen warning; ad-hoc Mac previews are not notarized. Use the signing column above for this release’s exact status.', '', 'Verify the archive against SHA256SUMS.txt. Release metadata and hashes are also in release.json.', '', 'Downloads and installation: https://cbassuarez.com/nus.dev/download/', 'Changes: https://github.com/cbassuarez/nus/commits/'+args.revision]
    # The public API returns release notes without a second cross-origin asset
    # request. Keep the same verified metadata available to the download page.
    lines += ['', '<!-- nus-release:'+json.dumps(manifest,separators=(',',':'))+' -->']
    notes.write_text('\n'.join(lines)+'\n')
    # Draft first: a failed upload cannot expose a half-published release.
    existing=subprocess.run(['gh','release','view',args.tag,'--json','isDraft'],capture_output=True,text=True)
    if existing.returncode == 0:
        if not json.loads(existing.stdout)['isDraft']: raise ValueError('Published releases are immutable; choose a new preview number')
        subprocess.run(['gh','release','edit',args.tag,'--notes-file',str(notes)],check=True)
    else:
        command=['gh','release','create',args.tag,'--target',args.revision,'--title',f'nus {args.tag}','--draft','--notes-file',str(notes)]
        if manifest['channel']=='preview': command.append('--prerelease')
        subprocess.run(command,check=True)
    assets=[str(args.directory/e['name']) for e in entries]+[str(args.directory/'release.json'),str(args.directory/'SHA256SUMS.txt')]
    subprocess.run(['gh','release','upload',args.tag,*assets,'--clobber'],check=True)
    subprocess.run(['gh','release','edit',args.tag,'--draft=false','--latest='+('true' if maintenance['latest'] else 'false')],check=True)

if __name__=='__main__': main()
