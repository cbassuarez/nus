#!/usr/bin/env python3
"""Check actual native frames against usable display bounds, using throwaway profiles."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

bundle = Path(sys.argv[1] if len(sys.argv) > 1 else 'dist/nus.app').resolve()
root = Path(tempfile.mkdtemp(prefix='nus-window-placement-'))
print(f'Evidence: {root}', flush=True)

def run(name, rect=None, centered=False, cascade=False, onboarding=False):
    directory = root / name
    profile = directory / 'profile'
    profile.mkdir(parents=True)
    if not onboarding:
        (profile / 'onboarded').write_text('skip')
    prefs = {'behavior': {'splash': 'None', 'window_start': 'Centered' if centered else 'Last'}}
    if rect is not None:
        prefs['window_rect'] = rect
    (profile / 'settings.json').write_text(json.dumps(prefs))
    script = directory / 'check.shot'
    steps = 'wait 1600\nwindowboundscheck\nshot placed\n'
    if cascade:
        steps += 'windowedge\nwait 200\nnewwindow\nwait 6000\n'
    script.write_text(steps)
    env = dict(os.environ, NUS_SHOT_DIR=str(directory), NUS_SHOT=str(script),
               NUS_SHOT_OUT=str(directory / 'screens'), NUS_SOFTWARE_PAINT='1')
    env.pop('NUS_SHOT_SIZE', None)
    env.pop('NUS_SHOT2', None)
    if cascade:
        second = directory / 'second.shot'
        second.write_text('wait 1600\nwindowboundscheck\nshot cascade\nwait 1000\n')
        env['NUS_SHOT2'] = str(second)
    log = directory / 'run.log'
    with log.open('w') as out:
        result = subprocess.run([str(bundle / 'Contents/MacOS/nus')], env=env,
                                stdout=out, stderr=subprocess.STDOUT, timeout=90)
    text = log.read_text()
    if result.returncode or 'panicked' in text or text.count('WINDOW BOUNDS:') < (2 if cascade else 1):
        raise RuntimeError(f'{name}: {log}\n{text[-4000:]}')
    print('PASS', name, flush=True)

run('first-open', onboarding=True)
run('valid-restore', [80, 80, 900, 700])
run('partial-overflow', [1100, 600, 1400, 1000])
run('oversized-restore', [100, 100, 5000, 4000])
run('disconnected-display', [-8000, -4000, 1500, 1000])
run('explicit-centered', [8000, 4000, 5000, 4000], centered=True)
run('cascade-at-desktop-edge', [80, 80, 900, 700], cascade=True)
print('Native window placement checks passed.', flush=True)
