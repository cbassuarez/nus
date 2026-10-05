#!/usr/bin/env python3
"""Native header actions and settings state, with disposable local profiles."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

bundle = Path(sys.argv[1] if len(sys.argv) > 1 else 'dist/nus.app').resolve()
root = Path(tempfile.mkdtemp(prefix='nus-header-check-'))
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

run('settings-state', '''asserttabs 1
assertpane home
assertheader
headerclick settings
wait 100
assertpane settings
asserttabs 1
shot settings
key x
headerclick settings
wait 100
assertpane home
asserttabs 1
headerclick settings
wait 100
key esc
assertpane home
settingsat 4
wait 100
key cmd+f
wait 100
key esc
assertpane settings
key cmd+w
assertpane home
newshell
wait 500
line echo unchanged
headerclick settings
wait 100
key x
key cmd+w
assertpane term
assertcmd echo unchanged
asserttabs 2
headerclick settings
wait 100
newshell
wait 100
assertpane term
asserttabs 3''')

run('assistant-note', '''newshell
wait 700
assertheader
headerclick assistant
wait 100
assertask open
shot assistant
headerclick assistant
wait 100
assertask closed
url about:blank
wait 300
focus page
headerclick assistant
wait 100
assertask open
asserttabs 2
headerclick assistant
wait 100
assertask closed
headerclick note
wait 200
assertnote
asserttabs 3
shot note
home
wait 100
headerclick assistant
wait 200
assertask open
headerclick assistant
wait 100
assertask closed''')

for width, face in [(480, 'ink'), (800, 'paper')]:
    run(f'narrow-{width}', '''shot header
assertheader
headerclick settings
wait 100
assertheader
assertpane settings
shot settings
headerclick settings
wait 100
assertpane home''', width, face)

print('Header checks passed.', flush=True)
