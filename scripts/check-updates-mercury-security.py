#!/usr/bin/env python3
"""Isolated native checks: no provider calls, downloads or installation swaps."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

bundle = Path(sys.argv[1] if len(sys.argv) > 1 else '/tmp/nus-feature-review.app').resolve()
root = Path(tempfile.mkdtemp(prefix='nus-mercury-security-'))
print(f'Evidence: {root}', flush=True)

def run(name, steps, reduced=False, reuse=False):
    directory = root / name
    profile = directory / 'profile'
    profile.mkdir(parents=True, exist_ok=True)
    if not reuse:
        (profile / 'onboarded').write_text('skip')
        (profile / 'me.json').write_text(json.dumps({'name': 'Review', 'face': 'Initial', 'created': '2026-09-22'}))
        (profile / 'settings.json').write_text(json.dumps({
            'window_rect': [70, 70, 1100, 900], 'motion': {'reduce': reduced},
            'behavior': {'splash': 'None', 'update_checks': False},
        }))
        (profile / 'memory.md').write_text('token=synthetic-migration-canary')
    script = directory / 'check.shot'
    script.write_text('wait 1400\n' + steps + '\n')
    env = dict(os.environ, NUS_SHOT_DIR=str(directory), NUS_SHOT=str(script),
               NUS_SHOT_OUT=str(directory / 'screens'), NUS_MODE='paper')
    env.pop('NUS_SHOT2', None)
    log = directory / ('relaunch.log' if reuse else 'run.log')
    with log.open('w') as output:
        result = subprocess.run([str(bundle / 'Contents/MacOS/nus')], env=env,
                                stdout=output, stderr=subprocess.STDOUT, timeout=90)
    assert result.returncode == 0, log.read_text()[-4000:]
    assert 'panicked' not in log.read_text(), log
    assert (profile / 'memory.md').read_bytes().startswith(b'NUSENC01')
    assert b'synthetic' not in (profile / 'memory.md').read_bytes()
    assert (profile / '.vault-format').read_text() == '1'
    print('PASS', name, 'relaunch' if reuse else '', flush=True)
    return profile

try:
    profile = run('animated', 'vaultcheck\nheldvaultcheck\nmercuryclaim\nwait 1200\nshot mercury-arrival\nwait 2600\nshot mercury-earned\nmercurycheck\ncloseprofile\nmecard\nwait 500\nshot profile\ncloseprofile\nsettingsat 14\nwait 100\nshot updates\nupdateready\nwait 150\nshot update-ready-header\nupdateheaderclick\nwait 150\nsettingseek UpdateConfirm\nupdatewarningcheck\nwait 200\nshot interruption-warning')
    claim = json.loads((profile / 'mercury.json').read_text())
    run('animated', 'mercurycheck\nmecard\nwait 500\nshot profile-relaunch', reuse=True)
    assert json.loads((profile / 'mercury.json').read_text()) == claim
    run('reduced', 'mercuryclaim\nwait 100\nshot mercury-reduced\nmercurycheck', reduced=True)
finally:
    # These credentials belong only to this script's disposable profiles.
    for marker in root.glob('*/profile/.vault-id'):
        subprocess.run(['/usr/bin/security', 'delete-generic-password', '-s',
                        'dev.nus.local-state.v1', '-a', marker.read_text()],
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True)
print('Native claim persistence, reduced motion, update warning and vault checks passed.', flush=True)
