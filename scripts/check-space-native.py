#!/usr/bin/env python3
"""Run native Home's Space scenario with an isolated app profile and deadline.

This runs a real desktop program, not a mocked renderer. Run in a disposable
OS account for release QA: application-profile isolation is not an OS sandbox.
No default-browser request, external URL, shell command or OS-input injection
is included in the scenario. Captures are app offscreen captures, not scanout.
"""
from pathlib import Path
import argparse
import hashlib
import json
import os
import signal
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
NAMES = ('space-home-earth', 'space-home-held', 'space-home-darkroom',
         'space-home-narrow', 'space-home-typing')

def sha(path):
    h = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            h.update(block)
    return h.hexdigest()

def stop(child):
    if child.poll() is not None:
        return
    if os.name == 'posix':
        try:
            os.killpg(child.pid, signal.SIGTERM)
        except ProcessLookupError:
            return
    else:
        child.terminate()
    try:
        child.wait(timeout=5)
    except subprocess.TimeoutExpired:
        if os.name == 'posix':
            try:
                os.killpg(child.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
        else:
            child.kill()
        child.wait(timeout=5)

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('executable', type=Path, help='GUI executable, Linux launcher, or macOS .app')
    parser.add_argument('--out', type=Path, help='New directory; must not already exist')
    parser.add_argument('--timeout', type=float, default=120.0)
    args = parser.parse_args()
    if not 5 <= args.timeout <= 600:
        parser.error('--timeout must be between 5 and 600 seconds')
    executable = args.executable.expanduser().resolve()
    if executable.suffix == '.app' and executable.is_dir():
        executable = executable / 'Contents/MacOS/nus'
    if not executable.is_file():
        parser.error('GUI executable not found')
    if executable.parent.name == 'bin':
        parser.error('Pass the GUI launcher, not the shell CLI in bin/')
    out = args.out.expanduser().resolve() if args.out else Path(tempfile.mkdtemp(prefix='nus-space-native-'))
    if args.out:
        out.mkdir(mode=0o700, parents=False, exist_ok=False)
    root = out / 'app-profile'
    (root / 'profile').mkdir(parents=True)
    (root / 'profile/onboarded').write_text('skip')
    (root / 'profile/arrival-seen').write_text('1')
    pictures = out / 'captures'
    pictures.mkdir()
    script = ROOT / 'tests/space/native-home.shot'
    env = os.environ.copy()
    # Remove inherited scripts and debug defaults; this scenario never changes
    # OS defaults and should not accidentally inherit a second-window scenario.
    for key in ('NUS_SHOT2', 'NUS_PRIVATE_LOOK', 'NUS_SHELL', 'NUS_DUMP',
                'NUS_SHOT_SIZE', 'NUS_SHOT_INTERACTIVE'):
        env.pop(key, None)
    env.update(NUS_SHOT=str(script), NUS_SHOT_DIR=str(root),
               NUS_SHOT_OUT=str(pictures), NUS_MODE='ink')
    record = {'status': 'running', 'synthetic': False, 'driver': 'app-handlers',
              'capture_kind': 'app-offscreen', 'executable': str(executable),
              'sha256': sha(executable), 'scenario_sha256': sha(script),
              'not_validated': ['OS hit testing', 'display scanout', 'visual parity', 'system consent']}
    # Bootstrap/launcher hashes alone do not identify the application code.
    for name in ('nus.dll', 'nus-desktop'):
        candidate = executable.parent / name
        if candidate.is_file():
            record[name + '_sha256'] = sha(candidate)
    start = time.monotonic()
    child = None
    print(f'Reports: {out}', flush=True)
    try:
        with (out / 'native-home.log').open('wb') as log:
            child = subprocess.Popen([str(executable)], cwd=root, env=env,
                                     stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT,
                                     start_new_session=os.name == 'posix')
            code = child.wait(timeout=args.timeout)
        if code != 0:
            raise RuntimeError(f'Application exited {code}; see native-home.log')
        missing = [name for name in NAMES if not (pictures / f'{name}-ink.png').is_file()]
        if missing:
            raise RuntimeError('Scenario did not produce all captures: ' + ', '.join(missing))
        record.update(status='passed-app-handler-scenario', captures={
            p.name: sha(p) for p in sorted(pictures.glob('*.png'))})
    except (OSError, RuntimeError, subprocess.TimeoutExpired, KeyboardInterrupt) as error:
        record.update(status='failed', error=type(error).__name__ + ': ' + str(error))
    finally:
        if child is not None:
            stop(child)
        record['elapsed_seconds'] = round(time.monotonic() - start, 3)
        (out / 'validation.json').write_text(json.dumps(record, indent=2) + '\n')
    print(json.dumps(record, indent=2))
    return 0 if record['status'].startswith('passed') else 1

if __name__ == '__main__':
    sys.exit(main())
