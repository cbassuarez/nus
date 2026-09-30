#!/usr/bin/env python3
"""Native theme switching, persistence and renderer-polarity regression.

Runs an isolated app/profile. Never opens or modifies dist/nus.app's profile.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('bundle', type=Path)
    args = parser.parse_args()
    root = Path(tempfile.mkdtemp(prefix='nus-themes-'))
    profile = root / 'profile'
    profile.mkdir()
    (profile / 'onboarded').write_text('skip')
    (profile / 'settings.json').write_text(json.dumps({
        'behavior': {'hatch_background': False, 'hatch_status': False, 'splash': 'None',
                     'new_window': 'Prompt', 'then': 'Prompt', 'follow_os_theme': False,
                     'home_art': 'memphis', 'home_look': 'Art', 'shell_tint': 'Random'},
        'motion': {'register': 0.5, 'reduce': True},
    }))
    fixture = root / 'notes.rs'
    fixture.write_text('// A place to settle into.\nfn begin() {\n    let place = "home";\n    println!("ready: {}", place);\n}\n')
    steps = ['wait 1500', 'home', 'appearancecheck', 'homelook art memphis', 'wait 300']
    themes = ['blueprint', 'canopy', 'carbon', 'citron', 'folio', 'indigo', 'iris', 'lagoon', 'ledger', 'vermilion']
    for theme in themes:
        steps += [f'theme {theme}', 'wait 120', f'shot home-{theme}']
    steps += ['theme carbon', 'newshell', 'wait 1000',
              "shell printf '\\033[32mReady\\033[0m  \\033[33mAttention\\033[0m  \\033[31mError\\033[0m\\n'",
              f'themeeditor {fixture}', 'wait 600', 'asserteditorready']
    for theme in ['carbon', 'indigo', 'folio', 'ledger']:
        steps += [f'theme {theme}', 'wait 200', f'shot work-{theme}']
    steps += ['theme blueprint', 'looktab 0', 'wait 300', 'settingsbounds', 'shot theme-picker',
              'looktab 3', 'wait 300', 'settingsbounds', 'shot theme-behavior',
              'trafficsize 900 680', 'wait 300', 'settingsbounds', 'shot theme-behavior-narrow']
    shot = root / 'check.shot'
    shot.write_text('\n'.join(steps) + '\n')
    exe = args.bundle.resolve() / 'Contents/MacOS/nus'
    env = {k: v for k, v in os.environ.items() if not k.startswith('NUS_')}
    env.update(NUS_SHOT_DIR=str(root), NUS_SHOT=str(shot), NUS_SHOT_OUT=str(root/'screens'),
               NUS_SHOT_SIZE='1440x900')
    with (root/'run.log').open('w') as log:
        result = subprocess.run([str(exe)], env=env, stdout=log, stderr=subprocess.STDOUT, timeout=90)
    output = (root/'run.log').read_text()
    assert result.returncode == 0 and 'APPEARANCE CHECK PASS' in output and 'panicked' not in output, output[-5000:]
    assert 'unknown' not in '\n'.join(line for line in output.splitlines() if 'shot:' in line), output[-3000:]
    captures = list((root/'screens').glob('*.png'))
    assert len(captures) == 17, [p.name for p in captures]
    evidence = {'passed': True, 'binary_sha256': hashlib.sha256(exe.read_bytes()).hexdigest(),
                'themes': themes, 'captures': [p.name for p in captures]}
    (root/'result.json').write_text(json.dumps(evidence, indent=2)+'\n')
    print(f'PASS: native theme regression; evidence {root}')


if __name__ == '__main__':
    main()
