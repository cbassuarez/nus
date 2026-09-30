#!/usr/bin/env python3
"""Native home motion probe: real clocks, isolated profile, retained raw evidence.

Use an optimized macOS bundle. Frame intervals describe app submissions, not
display scanout; power source and background machine load affect the result.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('bundle', type=Path)
    parser.add_argument('--art', nargs='+', choices=['memphis', 'pond', 'space', 'sky'], default=['memphis', 'pond', 'space', 'sky'])
    parser.add_argument('--seconds', type=float, default=10)
    parser.add_argument('--out', type=Path)
    args = parser.parse_args()
    if not 1 <= args.seconds <= 60:
        parser.error('--seconds must be between 1 and 60')
    executable = args.bundle.resolve() / 'Contents/MacOS/nus'
    root = args.out or Path(tempfile.mkdtemp(prefix='nus-home-perf-'))
    root.mkdir(parents=True, exist_ok=True)
    profile = root / 'profile'
    profile.mkdir()  # Never overwrite/reuse a user's profile or an earlier run.
    (profile / 'onboarded').write_text('skip')
    (profile / 'settings.json').write_text(json.dumps({
        'behavior': {'hatch_background': False, 'hatch_status': False, 'splash': 'None'},
        'motion': {'register': 0.5, 'reduce': False},
    }))
    steps = ['wait 1600', 'home']
    for art in args.art:
        steps += ['perfreset', f'homelook art {art}', 'wait 2200', f'perfstats {art}-entrance',
                  'perfreset', f'wait {round(args.seconds * 1000)}', f'perfstats {art}', f'shot home-{art}']
    script = root / 'motion.shot'
    script.write_text('\n'.join(steps) + '\n')
    env = {k: v for k, v in os.environ.items() if not k.startswith('NUS_')}
    env.update(NUS_PERF='1', NUS_SHOT=str(script.resolve()), NUS_SHOT_DIR=str(root.resolve()),
               NUS_SHOT_OUT=str((root / 'screens').resolve()), NUS_SHOT_SIZE='1440x900')
    metadata = {'platform': platform.platform(), 'machine': platform.machine(),
                'executable': str(executable), 'sha256': hashlib.sha256(executable.read_bytes()).hexdigest(),
                'art': args.art, 'seconds_per_art': args.seconds,
                'power': subprocess.run(['pmset', '-g', 'batt'], capture_output=True, text=True).stdout}
    (root / 'provenance.json').write_text(json.dumps(metadata, indent=2))
    print(f'Evidence: {root.resolve()}', flush=True)
    with (root / 'run.log').open('w') as log:
        result = subprocess.run([str(executable), '-ApplePersistenceIgnoreState', 'YES', '-NSQuitAlwaysKeepsWindows', 'NO'],
                                env=env, stdout=log, stderr=subprocess.STDOUT,
                                timeout=30 + len(args.art) * (args.seconds + 4))
    output = (root / 'run.log').read_text()
    if result.returncode or 'panicked' in output:
        raise RuntimeError(f'Native probe failed; see {root / "run.log"}')
    metrics = {}
    for line in output.splitlines():
        if line.startswith('PERF '):
            _, label, value = line.split(' ', 2)
            metrics[label] = json.loads(value)
    for art in args.art:
        if metrics.get(art, {}).get('art_script', {}).get('count', 0) < 2:
            raise RuntimeError(f'{art} did not animate; retain and inspect the raw run')
    (root / 'metrics.json').write_text(json.dumps(metrics, indent=2))
    for art, sample in metrics.items():
        spacing = sample['frame_interval']
        print(f'{art}: frames={sample["frame_build_submit"]["count"]}, '
              f'p95={spacing["p95_ms"]:.2f} ms, max={spacing["max_ms"]:.2f} ms')


if __name__ == '__main__':
    main()
