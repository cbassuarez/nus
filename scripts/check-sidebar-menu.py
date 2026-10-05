#!/usr/bin/env python3
"""Native compact-sidebar menu and footer checks, with disposable local profiles."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

bundle = Path(sys.argv[1] if len(sys.argv) > 1 else 'dist/nus.app').resolve()
root = Path(tempfile.mkdtemp(prefix='nus-sidebar-menu-check-'))
print(f'Evidence: {root}', flush=True)

def run(name, steps, width=1280, face='paper'):
    directory = root / name
    profile = directory / 'profile'
    profile.mkdir(parents=True)
    (profile / 'onboarded').write_text('skip')
    (profile / 'settings.json').write_text(json.dumps({
        'window_rect': [70, 70, width, 850],
        'behavior': {'splash': 'None', 'new_window': 'Prompt'},
        'motion': {'register': 0.5, 'reduce': True},
    }))
    script = directory / 'check.shot'
    script.write_text(f'window {width} 850\nwait 1600\n' + steps + '\n')
    env = dict(os.environ, NUS_SHOT_DIR=str(directory), NUS_SHOT=str(script),
               NUS_SHOT_OUT=str(directory / 'screens'), NUS_MODE=face,
               NUS_SOFTWARE_PAINT='1')
    env.pop('NUS_SHOT2', None)
    log = directory / 'run.log'
    with log.open('w') as out:
        result = subprocess.run([str(bundle / 'Contents/MacOS/nus')], env=env,
                                stdout=out, stderr=subprocess.STDOUT, timeout=90)
    text = log.read_text()
    if result.returncode or 'panicked' in text:
        raise RuntimeError(f'{name}: {log}\n{text[-3500:]}')
    print('PASS', name, flush=True)


steps = """sidebarside {side}
sidebarwidth 48
wait 200
sidebarcheck
shot swatch
sideclick Window
wait 200
windowmenucheck
shot window-menu
sideclick Rename
wait 150
windowrenamecheck
key esc
settingsat 14
wait 150
sideclick Window
wait 200
windowmenucheck
sideclick Rename
wait 150
windowrenamecheck
key esc"""

for width, face in [(1280, 'paper'), (480, 'ink')]:
    for side in ['left', 'right']:
        page = 'url about:blank\nwait 600\n' if width == 480 and side == 'left' else ''
        run(f'menu-{width}-{side}', page + steps.format(side=side), width, face)
