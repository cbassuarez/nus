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
import zipfile
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
        self.with_installer()
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

    def windows_payloads(self):
        redist=self.root/'redist'
        payloads={
            'vendor/cef/libcef.dll':b'cef',
            'vendor/cef/icudtl.dat':b'icu',
            'vendor/cef/locales/en-US.pak':b'locale',
            'spikes/composite/target/release/composite.exe':b'desktop',
            'target/release/nus-hold.exe':b'hold',
            'target/release/nus.exe':b'cli',
            'redist/x64/Microsoft.VC145.CRT/vcruntime140.dll':b'crt',
            'assets/fonts/OFL-test.txt':b'font license',
            'assets/icons/LICENSE':b'icon license',
            'LICENSE':b'license',
            'inno/ISCC.exe':b'compiler',
        }
        for name,data in payloads.items():
            path=self.root/name;path.parent.mkdir(parents=True,exist_ok=True);path.write_bytes(data)
        return {'VCToolsRedistDir':str(redist),'ISCC':str(self.root/'inno/ISCC.exe')}

    def package(self,*mode,tag=None,run=None):
        args=['package','--tag',tag or self.tag,'--target','windows-x86_64',*mode]
        calls=[]
        def verify(cmd,**kw):
            calls.append(cmd)
            if cmd[0].endswith('ISCC.exe'):
                # Stand in for Inno Setup: an installer built from the stage.
                define=lambda key: next(a.split('=',1)[1] for a in cmd if a.startswith(f'/D{key}='))
                self.assertTrue((Path(define('SourceDir'))/'README.txt').is_file())
                Path(define('OutputDir')).mkdir(parents=True,exist_ok=True)
                (Path(define('OutputDir'))/(define('OutputName')+'.exe')).write_bytes(b'setup '+define('Channel').encode())
            return (run or (lambda: subprocess.CompletedProcess(cmd,0)))()
        with patch.object(sys,'argv',args),patch.object(release.package,'ROOT',self.root),patch.object(release.package.subprocess,'run',verify),patch('builtins.print'):
            release.package.main()
        return calls

    def test_windows_stage_is_unsigned_payload_without_release_record(self):
        with patch.dict('os.environ',self.windows_payloads()):
            calls=self.package('--stage-only')
        stage=self.root/'dist/windows-stage'
        for name in ['nus.exe','nus-hold.exe','bin/nus.exe','libcef.dll','vcruntime140.dll','locales/en-US.pak','LICENSE']:
            self.assertTrue((stage/name).is_file(),name)
        # Metadata that claims a signature must not exist before signing.
        self.assertFalse((stage/'README.txt').exists())
        self.assertFalse((stage/'nus-package.json').exists())
        self.assertEqual(list((self.root/'dist/release').iterdir()),[])
        self.assertEqual(calls,[])

    def test_windows_installer_is_built_only_from_verified_executables(self):
        with patch.dict('os.environ',self.windows_payloads()):
            self.package('--stage-only')
            calls=self.package('--build-installer')
        self.assertTrue(calls[0][3].endswith('verify-windows-release.ps1'))
        self.assertNotIn('-Installer',calls[0])
        self.assertTrue(calls[1][0].endswith('ISCC.exe'))
        self.assertIn('/DChannel=preview',calls[1])
        self.assertIn('/DNumericVersion=0.0.1',calls[1])
        self.assertTrue((self.root/'dist/windows-installer/nus-0.0.1-preview.1-windows-x86_64-setup.exe').is_file())
        self.assertEqual(list((self.root/'dist/release').iterdir()),[])

    def test_windows_finalize_verifies_before_recording_authenticode(self):
        with patch.dict('os.environ',self.windows_payloads()):
            self.package('--stage-only')
            self.package('--build-installer')
            calls=self.package('--finalize-staged')
        self.assertEqual(len(calls),1)
        self.assertTrue(calls[0][3].endswith('verify-windows-release.ps1'))
        self.assertTrue(calls[0][-1].endswith('-setup.exe'))
        output=self.root/'dist/release'
        record=json.loads((output/'windows-x86_64.json').read_text())
        self.assertEqual(record['signing'],'authenticode')
        archive=output/record['name']
        self.assertEqual(record['sha256'],release.package.digest(archive))
        self.assertIn(record['sha256'],(output/f'{archive.name}.sha256').read_text())
        setup=output/record['installer']['name']
        self.assertEqual(setup.name,'nus-0.0.1-preview.1-windows-x86_64-setup.exe')
        self.assertEqual(record['installer']['sha256'],release.package.digest(setup))
        self.assertIn(record['installer']['sha256'],(output/f'{setup.name}.sha256').read_text())
        prefix='nus-0.0.1-preview.1-windows-x86_64/'
        with zipfile.ZipFile(archive) as z:
            names=set(z.namelist())
            for name in ['nus.exe','nus-hold.exe','bin/nus.exe','libcef.dll','README.txt','nus-package.json']:
                self.assertIn(prefix+name,names)
            self.assertIn('Signing: authenticode',z.read(prefix+'README.txt').decode())
            self.assertEqual(json.loads(z.read(prefix+'nus-package.json'))['signing'],'authenticode')

    def test_windows_finalize_requires_the_signed_installer(self):
        with patch.dict('os.environ',self.windows_payloads()):
            self.package('--stage-only')
        with self.assertRaisesRegex(FileNotFoundError,'setup.exe'): self.package('--finalize-staged')

    def with_installer(self):
        path=self.root/'windows-x86_64.json'
        record=json.loads(path.read_text())
        setup=self.root/'nus-0.0.1-preview.1-windows-x86_64-setup.exe'
        setup.write_bytes(b'setup')
        record.update(signing='authenticode',installer={'name':setup.name,'sha256':release.package.digest(setup),'size':setup.stat().st_size})
        path.write_text(json.dumps(record))
        return setup

    def test_installer_is_published_beside_its_portable_package(self):
        setup=self.with_installer()
        calls=self.run_release()
        self.assertIn(str(setup),calls[-2])
        self.assertIn(setup.name,(self.root/'SHA256SUMS.txt').read_text())
        manifest=json.loads((self.root/'release.json').read_text())
        # The updater takes the first asset for its target: it must stay the ZIP.
        windows=[e for e in manifest['assets'] if e['target']=='windows-x86_64']
        self.assertEqual([e['name'] for e in windows],['nus-0.0.1-preview.1-windows-x86_64.zip'])
        self.assertEqual(windows[0]['installer']['name'],setup.name)

    def test_corrupt_installer_cannot_publish(self):
        self.with_installer().write_bytes(b'changed')
        with self.assertRaisesRegex(ValueError,'Invalid installer'): self.run_release()

    def test_stable_windows_requires_installer(self):
        self.tag='v0.0.1'
        signing={'linux-x86_64':'checksum','macos-arm64':'notarized','windows-x86_64':'authenticode'}
        for target in signing:
            path=self.root/f'{target}.json'
            record=json.loads(path.read_text())
            record.update(version=self.tag,channel='stable',signing=signing[target])
            path.write_text(json.dumps(record))
        with self.assertRaisesRegex(ValueError,'requires its installer'): self.run_release()

    def test_windows_failed_verification_writes_no_release(self):
        with patch.dict('os.environ',self.windows_payloads()):
            self.package('--stage-only')
            self.package('--build-installer')
        def fail(): raise subprocess.CalledProcessError(1,'pwsh')
        with self.assertRaises(subprocess.CalledProcessError): self.package('--finalize-staged',run=fail)
        self.assertEqual(list((self.root/'dist/release').iterdir()),[])

    def test_windows_finalize_requires_staged_executables(self):
        with self.assertRaisesRegex(FileNotFoundError,'incomplete'): self.package('--finalize-staged')

    def test_windows_packaging_mode_is_explicit_and_stable_is_never_unsigned(self):
        with patch('sys.stderr'):
            with self.assertRaises(SystemExit): self.package()
            with self.assertRaises(SystemExit): self.package('--unsigned-preview',tag='v0.0.1')

    def test_windows_unsigned_preview_says_so(self):
        with patch.dict('os.environ',self.windows_payloads()):
            calls=self.package('--unsigned-preview')
        self.assertEqual(calls,[])
        record=json.loads((self.root/'dist/release/windows-x86_64.json').read_text())
        self.assertEqual(record['signing'],'unsigned')

if __name__=='__main__': unittest.main()
