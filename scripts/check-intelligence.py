#!/usr/bin/env python3
"""The intelligence atom: the ring, the dial and the nucleus set real CLI
flags, in Settings and in an assistant's launch review. Isolated profiles."""
import os
from pathlib import Path
import subprocess
import sys
import tempfile

bundle = Path(sys.argv[1] if len(sys.argv) > 1 else 'dist/nus.app').resolve()
root = Path(tempfile.mkdtemp(prefix='nus-intelligence-check-'))
print(f'Evidence: {root}', flush=True)


def run(name, steps, face='paper'):
    directory = root / name
    profile = directory / 'profile'
    profile.mkdir(parents=True)
    (profile / 'onboarded').write_text('skip')
    script = directory / 'check.shot'
    script.write_text('wait 1600\n' + steps + '\n')
    env = dict(os.environ, NUS_SHOT_DIR=str(directory), NUS_SHOT=str(script), NUS_SHOT_OUT=str(directory / 'screens'), NUS_MODE=face)
    env.pop('NUS_SHOT2', None)
    log = directory / 'run.log'
    with log.open('w') as out:
        result = subprocess.run([str(bundle / 'Contents/MacOS/nus')], env=env, stdout=out, stderr=subprocess.STDOUT, timeout=90)
    text = log.read_text()
    if result.returncode or 'panicked' in text:
        raise RuntimeError(f'{name}: {log}\n{text[-3500:]}')
    print('PASS', name, flush=True)


run('settings', '\n'.join([
    'settingsat 9', 'wait 400', 'assertintel 3 auto', 'shot settings-deep',
    "assertcommand 0 '--model' 'opus' '--effort' 'high'",
    "assertcommand 1 'model_reasoning_effort=high'",
    'intelturn 150', 'wait 400', 'assertintel 1', 'shot settings-quick',
    'inteltap right', 'wait 400', 'assertintel 2',
    "assertcommand 0 '--model' 'sonnet' '--effort' 'medium'",
    'intelspin -45', 'wait 400', 'assertintel 4', 'shot settings-max',
    "assertcommand 0 '--model' 'opus' '--effort' 'max'",
]))
run('review', '\n'.join([
    'assistantdraft 0 why did the lease test fail', 'wait 600', 'shot review-deep',
    'intelspin 90', 'wait 400', 'assertintel 2',
    'intelnucleus', 'wait 400', 'assertintel 2 opus',
    "assertcommand 0 '--model' 'opus' '--effort' 'medium'",
    'intelnucleus', 'intelnucleus', 'wait 600', 'assertintel 2 haiku',
    "assertcommand 0 '--model' 'haiku' --",
    'shot review-haiku', 'intelnucleus', 'wait 200', 'assertintel 2 auto',
    'inteltap left', 'inteltap left', 'wait 600', 'assertintel 0', 'shot review-instant',
    'reviewbounds',
]))
run('review-ink', 'assistantdraft 1 review the diff\nwait 800\nshot review-codex-ink', face='ink')
print('Intelligence checks passed.', flush=True)
