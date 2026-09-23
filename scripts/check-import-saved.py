#!/usr/bin/env python3
"""Native import reel and saved-command integration, in disposable profiles."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

bundle = Path(sys.argv[1] if len(sys.argv) > 1 else '/tmp/nus-import-saved.app')
root = Path(tempfile.mkdtemp(prefix='nus-import-saved-'))
print(f'Evidence: {root}', flush=True)
steps = '''wait 1100
savedfixture
savedcheck
promptcheck
settingscheck
home
wait 300
shot saved-home
palette go
wait 300
shot saved-palette
close
settingsat 19
wait 300
shot saved-settings
settingseek Workspace(SavedEdit(0
wait 200
shot saved-collection
mecard import
reelat 0
wait 400
shot import-sentence
reelat 1.95
wait 100
shot import-turn
importsource
wait 200
shot import-sources
importfixture
wait 200
shot import-review
importapplycheck
importfixture
importapplycheck
closeprofile
savedinsertprobe
wait 2000
savedinsertcheck
'''
try:
    for face, width, reduced in [('paper', 1100, False), ('ink', 760, True)]:
        work = root / face
        profile = work / 'profile'
        profile.mkdir(parents=True)
        (profile / 'onboarded').write_text('skip')
        (profile / 'me.json').write_text(json.dumps({'name': 'Review', 'face': 'Initial', 'created': '2026-09-23'}))
        (profile / 'settings.json').write_text(json.dumps({'window_rect': [50, 50, width, 960], 'motion': {'reduce': reduced}, 'behavior': {'splash': 'None', 'home_look': 'Line', 'update_checks': False}}))
        shot = work / 'check.shot'
        shot.write_text(steps)
        env = dict(os.environ, NUS_SHOT_DIR=str(work), NUS_SHOT=str(shot), NUS_SHOT_OUT=str(work / 'screens'), NUS_MODE=face)
        env.pop('NUS_SHOT2', None)
        with (work / 'run.log').open('w') as log:
            result = subprocess.run([str(bundle / 'Contents/MacOS/nus')], env=env, stdout=log, stderr=subprocess.STDOUT, timeout=100)
        assert result.returncode == 0, (work / 'run.log').read_text()[-6000:]
        folders = json.loads((profile / 'folders.json').read_text())
        assert len(next(f for f in folders if f['name'] == 'Imported from Arc')['items']) == 2
        prefs = json.loads((profile / 'settings.json').read_text())['behavior']['prompt']
        assert prefs['saved_names']['> cargo test --workspace'] == 'Test the workspace'
        assert not prefs['saved_run']
        print(f'PASS {face}: settings, source controls, named matching, import review and idempotent apply', flush=True)
finally:
    for marker in root.glob('*/profile/.vault-id'):
        subprocess.run(['/usr/bin/security', 'delete-generic-password', '-s', 'dev.nus.local-state.v1', '-a', marker.read_text()], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
