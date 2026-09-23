#!/usr/bin/env python3
"""Native, disposable-profile continuity checks; no downloads/package swaps."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

bundle = Path(sys.argv[1]).resolve()
exe = bundle / 'Contents/MacOS/nus' if bundle.suffix == '.app' else bundle
root = Path(tempfile.mkdtemp(prefix='nus-multiversion-'))
profile = root / 'profile'
profile.mkdir()
print(f'Evidence: {root}', flush=True)
contract = json.loads(subprocess.check_output([str(exe), '--compatibility']))
(profile / 'onboarded').write_text('skip')
(profile / 'settings.json').write_text(json.dumps({
    'schema': 2, 'window_rect': [70, 70, 1100, 900],
    'behavior': {'splash': 'None', 'then': 'Prompt', 'update_checks': False},
}))
(profile / 'me.json').write_text(json.dumps({'name': 'Review', 'face': 'Initial', 'created': '2026-09-23'}))
script = root / 'check.shot'
script.write_text('wait 1000\nsettingsat 14\nwait 200\nshot continuity\n')
env = dict(os.environ, NUS_SHOT=str(script), NUS_SHOT_DIR=str(root),
           NUS_SHOT_OUT=str(root / 'screens'), NUS_MODE='paper')
env.pop('NUS_SHOT2', None)

def run(name, args=(), success=True):
    with (root / f'{name}.log').open('w') as log:
        proc = subprocess.run([str(exe), *args], env=env, stdout=log,
                              stderr=subprocess.STDOUT, timeout=90)
    assert (proc.returncode == 0) == success, (root / f'{name}.log').read_text()[-4000:]
    print('PASS', name, flush=True)

try:
    run('first-start')
    live = json.loads((profile / 'compatibility.json').read_text())
    assert live['healthy'] is True
    assert live['version'] == contract['version']
    (profile / 'continuity-note').write_text('before update')
    generation = root / 'generations/generation-native'
    shutil.copytree(profile, generation / 'profile', ignore=shutil.ignore_patterns(
        'Singleton*', 'instance', 'hold', '.vault-lock'))
    (generation / 'complete.json').write_text(json.dumps(live))
    (profile / 'continuity-note').write_text('work after update')

    for field, value in [('format', 999), ('chromium_major', 999), ('channel', 'preview' if live['channel'] != 'preview' else 'current')]:
        ahead = dict(live); ahead[field] = value
        encoded = json.dumps(ahead)
        (profile / 'compatibility.json').write_text(encoded)
        run(f'refuse-{field}', success=False)
        assert (profile / 'compatibility.json').read_text() == encoded
        assert (profile / 'continuity-note').read_text() == 'work after update'

    ahead = dict(live); ahead['version'] = '999.0.0'
    (profile / 'compatibility.json').write_text(json.dumps(ahead))
    run('refuse-downgrade', success=False)
    run('recover', ['--recover-profile=generation-native'])
    assert (profile / 'continuity-note').read_text() == 'before update'
    assert (root / 'preserved-generation-native/continuity-note').read_text() == 'work after update'
    assert json.loads((profile / 'compatibility.json').read_text())['healthy'] is True
    assert not (root / 'recovery-pending.json').exists()
    assert (generation / 'complete.json').is_file()
finally:
    if sys.platform == 'darwin':
        # Only credentials created for these disposable profiles.
        ids = {p.read_text() for p in root.rglob('.vault-id')}
        for key in ids:
            subprocess.run(['/usr/bin/security', 'delete-generic-password', '-s',
                            'dev.nus.local-state.v1', '-a', key],
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True)
print('Native compatibility and profile recovery checks passed.', flush=True)
