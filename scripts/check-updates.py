#!/usr/bin/env python3
"""Quiet Status rendering, disclosure, search and restart review; no update installs."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import struct

bundle = Path(sys.argv[1] if len(sys.argv) > 1 else 'dist/nus.app').resolve()
root = Path(tempfile.mkdtemp(prefix='nus-quiet-updates-'))
print(f'Evidence: {root}', flush=True)

def run(name, width, face):
    directory = root / name
    profile = directory / 'profile'
    profile.mkdir(parents=True)
    (profile / 'onboarded').write_text('skip')
    (profile / 'settings.json').write_text(json.dumps({
        'window_rect': [40, 80, width, 1500],
        'behavior': {'splash': 'None'}}))
    steps = ['wait 1600', 'settingsat 14', 'wait 100']
    for state in ['initial', 'current', 'offline', 'checking', 'installing', 'available']:
        steps += [f'updatefixture {state}', 'wait 100', 'quietupdatescheck',
                  'settingsbounds', f'shot {state}']
    steps += ['settingclick UpdateInstall', 'wait 100', 'updatewarningcheck',
              'wait 100', 'settingsbounds', 'shot restart-review',
              'settingclick UpdateCancel', 'wait 100',
              'settingseek UpdateDetails(0)', 'settingclick UpdateDetails(0)',
              'wait 100', 'assertchoice UpdateDetails(0)', 'shot profile-recovery',
              'settingseek UpdateDetails(1)', 'settingclick UpdateDetails(1)',
              'wait 100', 'assertchoice UpdateDetails(1)',
              'settingseek CopySupportDetails', 'settingsbounds', 'shot support-privacy',
              'settingsscroll 0', 'wait 100',
              'settingseek UpdateDetails(0)', 'settingclick UpdateDetails(0)', 'wait 100',
              'settingseek UpdateDetails(1)', 'settingclick UpdateDetails(1)', 'wait 100',
              'palette settings copy support details', 'wait 100', 'key enter',
              'wait 200', 'assertrevealed', 'settingseek CopySupportDetails',
              'settingsbounds', 'shot search-revealed']
    script = directory / 'check.shot'
    script.write_text('\n'.join(steps) + '\n')
    env = dict(os.environ, NUS_SHOT_DIR=str(directory), NUS_SHOT=str(script),
               NUS_SHOT_OUT=str(directory / 'screens'), NUS_MODE=face,
               NUS_SOFTWARE_PAINT='1')
    env.pop('NUS_SHOT2', None)
    env.pop('NUS_SHOT_SIZE', None)
    log = directory / 'run.log'
    with log.open('w') as out:
        result = subprocess.run([str(bundle / 'Contents/MacOS/nus')], env=env,
                                stdout=out, stderr=subprocess.STDOUT, timeout=90)
    text = log.read_text()
    if result.returncode or 'panicked' in text:
        raise RuntimeError(f'{name}: {log}\n{text[-4000:]}')
    for shot in (directory / 'screens').glob('*.png'):
        actual_width, _ = struct.unpack('>II', shot.read_bytes()[16:24])
        if actual_width != width:
            raise RuntimeError(f'{shot}: expected width {width}, got {actual_width}')
    print('PASS', name, flush=True)

run('paper', 1800, 'paper')
run('narrow-ink', 800, 'ink')
run('compact-ink', 480, 'ink')
print('Quiet Status checks passed.', flush=True)
