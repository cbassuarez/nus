#!/usr/bin/env python3
"""Finish Work, natively: real shells, the real OS assertion, isolated profiles.

Usage: python3 scripts/check-finish-work.py /path/to/nus.app

A command typed and entered by the user can be protected; the native wake
assertion exists exactly while protected work runs, a second user command
joins, and everything is released when the last one finishes. Work written
into a shell by something other than the user never qualifies. Desktops
never show the control. macOS only (pmset reports the assertion).
"""
import json, os, subprocess, sys, tempfile
from pathlib import Path

bundle = Path(sys.argv[1] if len(sys.argv) > 1 else 'dist/nus.app').resolve()
root = Path(tempfile.mkdtemp(prefix='nus-finish-work-'))
print(f'Evidence: {root}', flush=True)


def run(name, steps, portable='1'):
    directory = root / name
    profile = directory / 'profile'
    profile.mkdir(parents=True)
    (profile / 'onboarded').write_text('skip')
    (profile / 'settings.json').write_text(json.dumps({'schema': 2, 'behavior': {'splash': 'None', 'then': 'Prompt'}, 'motion': {'reduce': True}}))
    script = directory / 'check.shot'
    script.write_text('wait 1500\n' + steps + '\n')
    env = dict(os.environ, NUS_SHOT_DIR=str(directory), NUS_SHOT=str(script), NUS_SHOT_OUT=str(directory / 'screens'), NUS_PORTABLE=portable, NUS_MODE='paper')
    log = directory / 'run.log'
    with log.open('w') as out:
        result = subprocess.run([str(bundle / 'Contents/MacOS/nus')], env=env, stdout=out, stderr=subprocess.STDOUT, timeout=120)
    text = log.read_text()
    if result.returncode or 'panicked' in text:
        raise RuntimeError(f'{name}: {log}\n{text[-3500:]}')
    print('PASS', name, flush=True)


run('user-work', '''newshell
wait 3000
assertfinishshown yes
assertfinish unavailable
assertwake off
line sleep 12
key Enter
wait 1500
assertfinish ready 1
finishwork
wait 400
assertfinish holding 1
assertwake on
shot holding
newshell
wait 3000
line sleep 2
key Enter
wait 1500
assertfinish holding 2
wait 11000
assertfinish unavailable
assertwake off
shot released''')

run('not-the-user', '''newshell
wait 1800
shell sleep 4
wait 900
assertfinish unavailable
finishwork
wait 400
assertfinish unavailable
assertwake off''')

run('desktop', '''wait 300
assertfinishshown no''', portable='0')

print('Finish Work checks passed.', flush=True)
