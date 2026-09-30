#!/usr/bin/env python3
"""Native cache invalidation, artwork reclamation and prompt-action regression."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('bundle', type=Path)
    args = parser.parse_args()
    root = Path(tempfile.mkdtemp(prefix='nus-heap-ui-'))
    profile = root / 'profile'
    profile.mkdir()
    (profile / 'onboarded').write_text('skip')
    (profile / 'settings.json').write_text(json.dumps({
        'behavior': {'hatch_background': False, 'hatch_status': False, 'splash': 'None',
                     'new_window': 'Prompt', 'then': 'Prompt'},
        'motion': {'register': 0.5, 'reduce': False},
    }))
    script = root / 'check.shot'
    script.write_text('\n'.join([
        'wait 1600', 'home', 'settings', 'home', 'wait 100', 'promptcheck', 'promptcachecheck',
        'savedfixture', 'savedcheck', 'homeclear', 'hometype settings', 'wait 200',
        'homeenter', 'assertpane settings', 'home', 'homelook art memphis',
        'wait 2200', 'shot memphis-before-pressure', 'perfreset', 'memorypressure critical',
        'wait 600', 'perfstats recovered', 'shot memphis-after-pressure',
        'homeclear', 'hometype ? allocation check', 'wait 200', 'shot prompt',
        'homeclear', 'trafficsize 820 600',
        'hometype ? NEW TAB · café e\u0301 👩‍💻 漢字 — a long workspace label that must fit without changing its Unicode text',
        'wait 200', 'shot narrow-unicode',
        'trafficsize 1440 900', 'homeclear', 'settingsat 2', 'wait 300', 'settingsbounds',
        'home', 'promptcachecheck', 'wait 200', 'shot home',
    ]) + '\n')
    env = {k:v for k,v in os.environ.items() if not k.startswith('NUS_')}
    env.update(NUS_SHOT_DIR=str(root), NUS_SHOT=str(script), NUS_SHOT_OUT=str(root/'screens'),
               NUS_SHOT_SIZE='1440x900', NUS_MODE='paper', NUS_PERF='1')
    with (root/'run.log').open('w') as log:
        result = subprocess.run([str(args.bundle.resolve()/'Contents/MacOS/nus')],
                                env=env, stdout=log, stderr=subprocess.STDOUT, timeout=60)
    output = (root/'run.log').read_text()
    assert result.returncode == 0 and 'panicked' not in output, output[-4000:]
    recovered = next(json.loads(line.split(' ',2)[2]) for line in output.splitlines() if line.startswith('PERF recovered '))
    assert recovered['art_script']['count'] > 2, recovered
    print(f'PASS: native heap UI regression; evidence {root}')


if __name__ == '__main__':
    main()
