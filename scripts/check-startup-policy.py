#!/usr/bin/env python3
"""Compile and execute the actual Rust startup-policy tests, without CEF or a GUI.

No OS defaults, profile files, source files, or Git state are changed. A missing
compiler is a failure, not a skipped test reported as successful.
"""
from pathlib import Path
import os
import shutil
import subprocess
import tempfile
import sys


def main() -> int:
    root = Path(__file__).resolve().parents[1]
    compiler = shutil.which('rustc')
    if compiler is None:
        print('rustc is required; no Rust tests were run.', file=sys.stderr)
        return 2
    source = root / 'spikes/composite/src/startup_policy.rs'
    with tempfile.TemporaryDirectory(prefix='nus-startup-policy-') as tmp:
        binary = Path(tmp) / ('policy-tests.exe' if os.name == 'nt' else 'policy-tests')
        subprocess.run([compiler, '--edition=2021', '--test', str(source), '-o', str(binary)], check=True, timeout=120)
        subprocess.run([str(binary)], check=True, timeout=30)
    return 0


if __name__ == '__main__':
    try:
        raise SystemExit(main())
    except (OSError, subprocess.SubprocessError) as exc:
        print(f'Startup-policy verification failed: {exc}', file=sys.stderr)
        raise SystemExit(1)
