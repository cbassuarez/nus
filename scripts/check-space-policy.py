#!/usr/bin/env python3
"""Compile/run the actual dependency-free Space motion module, no CEF needed."""
from pathlib import Path
import shutil
import subprocess
import tempfile
ROOT=Path(__file__).resolve().parents[1]

def main():
    compiler=shutil.which('rustc')
    if not compiler:
        raise SystemExit('rustc is required; no native policy tests were executed.')
    with tempfile.TemporaryDirectory(prefix='nus-space-policy-') as tmp:
        binary=Path(tmp)/('motion.exe' if __import__('os').name=='nt' else 'motion')
        subprocess.run([compiler,'--edition=2021','--test',str(ROOT/'crates/render/src/space_motion.rs'),'-o',str(binary)],check=True,timeout=120)
        subprocess.run([str(binary),'--nocapture'],check=True,timeout=30)
if __name__=='__main__':main()
