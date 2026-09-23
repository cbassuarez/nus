#!/usr/bin/env python3
"""Publication gates: never expose missing, mixed or corrupted packages."""
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import unittest
from unittest.mock import patch

spec=importlib.util.spec_from_file_location('release',Path(__file__).with_name('publish-release.py'))
release=importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)

class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root=Path(self.tmp.name)
        self.tag='v0.0.1-preview.1'
        for target in release.package.TARGETS:
            archive=self.root/f'nus-0.0.1-preview.1-{target}.zip'
            archive.write_bytes(target.encode())
            record={'name':archive.name,'target':target,'version':self.tag,'channel':'preview','sha256':release.package.digest(archive),'size':archive.stat().st_size,'signing':'unsigned'}
            (self.root/f'{target}.json').write_text(json.dumps(record))

    def run_release(self, existing=False, targets=None):
        calls=[]
        def run(args,**kw):
            calls.append(args)
            return subprocess.CompletedProcess(args,0 if existing or args[2]!='view' else 1, '{"isDraft":false}' if existing else '')
        args=['publish','--tag',self.tag,'--revision','a'*40,'--directory',str(self.root)]
        if targets is not None: args.extend(['--targets',targets])
        with patch.object(sys,'argv',args),patch.object(release.subprocess,'run',run):
            release.main()
        return calls

    def test_complete_preview_is_drafted_uploaded_then_published(self):
        calls=self.run_release()
        self.assertEqual([c[2] for c in calls],['view','create','upload','edit'])
        self.assertIn('--draft',calls[1])
        self.assertIn('--prerelease',calls[1])
        self.assertIn('--latest=false',calls[-1])
        manifest=json.loads((self.root/'release.json').read_text())
        self.assertEqual(len(manifest['assets']),3)
        self.assertNotIn('## Changes',(self.root/'notes.md').read_text())

    def test_preview_seven_notes_include_summary_and_revision_pinned_review_link(self):
        self.tag='v0.0.1-preview.7'
        for path in self.root.glob('*.json'):
            record=json.loads(path.read_text());record['version']=self.tag
            path.write_text(json.dumps(record))
        self.run_release()
        notes=(self.root/'notes.md').read_text()
        self.assertIn('## Changes',notes)
        self.assertIn('A first-open Atlas arrival',notes)
        self.assertIn('configured encrypted library sync',notes)
        self.assertIn('https://github.com/cbassuarez/nus/blob/'+'a'*40+'/docs/releases/v0.0.1-preview.7.md',notes)
        self.assertNotIn('](../',notes)

    def test_missing_matrix_member_cannot_publish(self):
        (self.root/'linux-x86_64.json').unlink()
        with self.assertRaises(FileNotFoundError): self.run_release()

    def test_corrupt_archive_cannot_publish(self):
        next(self.root.glob('*.zip')).write_bytes(b'changed')
        with self.assertRaisesRegex(ValueError,'Invalid archive'): self.run_release()

    def test_existing_public_release_cannot_be_overwritten(self):
        with self.assertRaisesRegex(ValueError,'immutable'): self.run_release(existing=True)

    def test_unsigned_stable_cannot_publish(self):
        self.tag='v0.0.1'
        for p in self.root.glob('*.json'):
            v=json.loads(p.read_text());v['version']=self.tag;v['channel']='stable';p.write_text(json.dumps(v))
        with self.assertRaisesRegex(ValueError,'not signed'): self.run_release()

    def test_scoped_preview_publishes_only_declared_platform(self):
        calls=self.run_release(targets='linux-x86_64')
        manifest=json.loads((self.root/'release.json').read_text())
        self.assertEqual([e['target'] for e in manifest['assets']],['linux-x86_64'])
        self.assertIn('Not included in this preview:',(self.root/'notes.md').read_text())
        self.assertEqual(len([a for a in calls[-2] if a.endswith('.zip')]),1)

    def test_stable_cannot_omit_platforms(self):
        self.tag='v0.0.1'
        with self.assertRaisesRegex(ValueError,'every platform'): self.run_release(targets='linux-x86_64')

    def test_unknown_targets_cannot_publish(self):
        for targets in ['', 'linux-arm99', 'linux-x86_64,anything']:
            with self.assertRaisesRegex(ValueError,'Unknown'): self.run_release(targets=targets)

    def test_mixed_channel_cannot_publish(self):
        path=self.root/'linux-x86_64.json'
        record=json.loads(path.read_text());record['channel']='stable'
        path.write_text(json.dumps(record))
        with self.assertRaisesRegex(ValueError,'Mixed'): self.run_release()

    def test_stable_requires_each_platforms_own_signature(self):
        self.tag='v0.0.1'
        signing={'linux-x86_64':'checksum','macos-arm64':'notarized','windows-x86_64':'authenticode'}
        for target in signing:
            path=self.root/f'{target}.json'
            record=json.loads(path.read_text())
            record.update(version=self.tag,channel='stable',signing=signing[target])
            path.write_text(json.dumps(record))
        for target,wrong in [('macos-arm64','authenticode'),('windows-x86_64','notarized')]:
            with self.subTest(target=target):
                path=self.root/f'{target}.json'
                record=json.loads(path.read_text());record['signing']=wrong
                path.write_text(json.dumps(record))
                with self.assertRaisesRegex(ValueError,'not signed'): self.run_release()
                record['signing']=signing[target];path.write_text(json.dumps(record))
        with patch.object(release.support,'evaluate',return_value={'kind':'current','latest':True,'support_until':None}):
            calls=self.run_release()
        self.assertIn('--latest=true',calls[-1])

    def test_invalid_tag_cannot_publish(self):
        self.tag='v0.0.1-preview.invalid'
        with self.assertRaisesRegex(ValueError,'Expected'): self.run_release()

    @unittest.skipIf(sys.platform == 'win32', 'Linux executable permission bits require a POSIX filesystem')
    def test_linux_package_contains_runtime_cli_and_executable_launcher(self):
        # Exercise staging, permissions, archive and manifest together without
        # depending on this test host's CEF distribution or native binaries.
        payloads={
            'vendor/cef/libcef.so':b'cef',
            'vendor/cef/libEGL.so.1':b'egl',
            'vendor/cef/icudtl.dat':b'icu',
            'vendor/cef/locales/en-US.pak':b'locale',
            'vendor/cef/include/cef.h':b'sdk is excluded',
            'spikes/composite/target/release/composite':b'desktop',
            'target/release/nus-hold':b'hold',
            'target/release/nus':b'cli',
            'assets/fonts/OFL-test.txt':b'font license',
            'assets/icons/LICENSE':b'icon license',
            'assets/icon/nus-256.png':b'icon',
            'LICENSE':b'license',
        }
        for name,data in payloads.items():
            path=self.root/name;path.parent.mkdir(parents=True,exist_ok=True);path.write_bytes(data)
            if '/release/' in name: path.chmod(0o755)
        args=['package','--tag',self.tag,'--target','linux-x86_64']
        with patch.object(sys,'argv',args),patch.object(release.package,'ROOT',self.root),patch('builtins.print'):
            release.package.main()
        output=self.root/'dist/release'
        record=json.loads((output/'linux-x86_64.json').read_text())
        archive=output/record['name']
        self.assertEqual(record['sha256'],release.package.digest(archive))
        self.assertEqual(record['signing'],'checksum')
        prefix='nus-0.0.1-preview.1-linux-x86_64/'
        with tarfile.open(archive) as tar:
            names=set(tar.getnames())
            for name in ['nus','nus-desktop','nus-hold','bin/nus','libcef.so','libEGL.so.1','icudtl.dat','locales/en-US.pak','licenses/OFL-test.txt']:
                self.assertIn(prefix+name,names)
            self.assertNotIn(prefix+'include/cef.h',names)
            self.assertEqual(tar.getmember(prefix+'nus').mode & 0o111,0o111)
            launcher=tar.extractfile(prefix+'nus').read().decode()
            self.assertIn('LD_LIBRARY_PATH',launcher)
            self.assertIn('exec "$dir/nus-desktop" "$@"',launcher)

if __name__=='__main__': unittest.main()
