#!/usr/bin/env python3
"""Publication gates: never expose missing, mixed or corrupted packages."""
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
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

    def run_release(self, existing=False):
        calls=[]
        def run(args,**kw):
            calls.append(args)
            return subprocess.CompletedProcess(args,0 if existing or args[2]!='view' else 1, '{"isDraft":false}' if existing else '')
        with patch.object(sys,'argv',['publish','--tag',self.tag,'--revision','a'*40,'--directory',str(self.root)]),patch.object(release.subprocess,'run',run):
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

if __name__=='__main__': unittest.main()
