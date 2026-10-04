#!/usr/bin/env python3
"""Exercise keyword validation and persistence in the packaged native UI."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--app', required=True, type=Path)
    parser.add_argument('--out', type=Path)
    args = parser.parse_args()
    directory = (args.out or Path(tempfile.mkdtemp(prefix='nus-keyword-check-'))).resolve()
    if args.out and directory.exists():
        parser.error('choose a new output directory to retain earlier evidence')
    directory.mkdir(parents=True, exist_ok=True)
    profile = directory / 'profile'
    profile.mkdir(exist_ok=True)
    (profile / 'onboarded').write_text('skip')
    (profile / 'settings.json').write_text(json.dumps({'behavior': {
        'splash': 'None', 'then': 'Prompt', 'update_checks': False,
        'hatch_background': False, 'hatch_status': False,
    }, 'motion': {'register': 0.5, 'reduce': True}}))
    source = 'https://example.org/search?q=%s'
    steps = ['wait 1800', f'promptrun keyword rs {source}', 'keywordedit rs',
             'input two words', 'key Enter', 'assertpalette open',
             f'assertkeyword {source} | rs', 'shot invalid-keyword',
             'key cmd+u', 'input ' + 'é' * 33, 'key Enter',
             'assertpalette open', f'assertkeyword {source} | rs',
             'key cmd+u', 'input ' + 'é' * 32, 'key Enter',
             'assertpalette closed', f'assertkeyword {source} | ' + 'é' * 32,
             'keywordedit ' + 'é' * 32, 'key Enter', 'assertpalette closed',
             f'assertkeyword {source} |', 'shot cleared-keyword']
    # The primary modifier is Ctrl away from macOS.
    if os.name == 'nt' or os.sys.platform != 'darwin':
        steps = [step.replace('cmd+u', 'ctrl+u') for step in steps]
    script = directory / 'check.shot'
    script.write_text('\n'.join(steps) + '\n', encoding='utf-8')
    env = dict(os.environ, NUS_SHOT_DIR=str(directory), NUS_SHOT=str(script),
               NUS_SHOT_OUT=str(directory / 'screens'), NUS_MODE='paper',
               NUS_SHOT_SIZE='1200x900')
    env.pop('NUS_SHOT2', None)
    app = args.app.resolve()
    exe = app / 'Contents/MacOS/nus' if app.suffix == '.app' else app
    log = directory / 'run.log'
    print(f'Evidence: {directory}', flush=True)
    with log.open('w', encoding='utf-8') as out:
        result = subprocess.run([str(exe)], cwd=directory, env=env, stdout=out,
                                stderr=subprocess.STDOUT, timeout=90)
    text = log.read_text(encoding='utf-8', errors='replace')
    if result.returncode or 'panicked' in text:
        raise RuntimeError(f'Keyword check failed: {text[-4500:]}')
    print('PASS keyword editor: invalid input preserves the keyword and editor; '
          'Unicode obeys storage limits; empty input clears deliberately.', flush=True)


if __name__ == '__main__':
    main()
