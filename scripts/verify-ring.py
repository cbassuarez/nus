#!/usr/bin/env python3
"""Check the shipped ring with pinned Verus and a deliberately broken mutation."""
import argparse
import datetime
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
VERSION = '0.2026.09.20.aef82ed'
FLAGS = ['--crate-type', 'lib', '--no-cheating', '--output-json', '--num-threads', '1']


def verify(executable, source):
    p = subprocess.run([str(executable), *FLAGS, str(source)], capture_output=True, text=True, timeout=120)
    try:
        data = json.loads(p.stdout)
        result = data['verification-results']
    except (ValueError, KeyError) as error:
        raise RuntimeError('Verifier failed before producing a proof result:\n' + p.stderr) from error
    if data['verus']['version'] != VERSION:
        raise RuntimeError('Unexpected Verus version; update the pin deliberately')
    return p.returncode, result, data['verus']


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--verus', required=True, type=Path)
    parser.add_argument('--out', type=Path, default=ROOT/'target/verification/ring.json')
    args = parser.parse_args()
    source = ROOT/'crates/pty/src/ring.rs'
    text = source.read_text()
    # Public compatibility re-export must still refer to the checked source.
    if 'pub use crate::ring::Ring;' not in (ROOT/'crates/pty/src/hold.rs').read_text():
        raise RuntimeError('Production holder no longer uses the checked ring')
    if 'mod ring;' not in (ROOT/'crates/pty/src/lib.rs').read_text():
        raise RuntimeError('Checked source is not in the production crate')
    code, result, version = verify(args.verus.resolve(), source)
    if code or not result['success'] or result['verified'] != 3 or result['errors'] != 0 or not result['is-verifying-entire-crate']:
        raise RuntimeError('Production proof did not fully pass: ' + json.dumps(result))
    marker = 'self.buf.extend_from_slice(bytes);'
    if text.count(marker) != 1:
        raise RuntimeError('Mutation anchor changed; review sensitivity test')
    with tempfile.TemporaryDirectory(prefix='nus-ring-mutation-') as directory:
        broken = Path(directory)/'ring.rs'
        broken.write_text(text.replace(marker, 'self.buf.clear();'))
        code, mutation, _ = verify(args.verus.resolve(), broken)
    if code == 0 or mutation['success'] or mutation['errors'] < 1 or mutation['encountered-vir-error']:
        raise RuntimeError('Expected a genuine proof failure for dropped input')
    files = ['crates/pty/src/ring.rs','crates/pty/src/hold.rs','crates/pty/src/lib.rs','crates/pty/Cargo.toml','Cargo.lock']
    hashes = {p:hashlib.sha256((ROOT/p).read_bytes()).hexdigest() for p in files}
    record = {'schema':1,'status':'verified','recorded_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),
              'scope':'Ring::new, Ring::push, Ring::bytes; exact bounded suffix and capacity preservation',
              'verus':version,'flags':FLAGS,'verification':result,'mutation_rejected':True,
              'source_hashes':hashes,
              'assumptions':['Verus, SMT solver and Rust compiler correctness','Pinned vstd contracts for Vec and slice operations','Successful allocation; no claim about allocator failure or physical footprint','Private ring fields; state created by new and mutated only by push'],
              'excludes':['PTY and socket delivery','Replay screen reconstruction','Whole-application correctness','Timing and performance']}
    args.out.parent.mkdir(parents=True,exist_ok=True)
    args.out.write_text(json.dumps(record,indent=2)+'\n')
    print('3 executable methods verified; dropped-input mutation rejected.')
    print(args.out)


if __name__ == '__main__':
    main()
