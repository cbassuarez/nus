#!/usr/bin/env python3
"""Validate native reports and export an allowlisted public benchmark catalog.

No measurements are run. Summaries are recomputed from retained run records.
Failed/partial attempts stay visible. Private paths and logs never enter output.
"""
from __future__ import annotations
import argparse
import importlib.util
import json
import math
from pathlib import Path
import re

spec = importlib.util.spec_from_file_location('perf_native', Path(__file__).with_name('perf-native.py'))
native = importlib.util.module_from_spec(spec)
spec.loader.exec_module(native)


def number(value):
    if type(value) not in (int, float) or not math.isfinite(value) or value < 0:
        raise ValueError('invalid numeric observation')
    return value


def export_report(data, source_hash):
    if data.get('schema') != 2 or data.get('status') not in ('complete', 'failed', 'running'):
        raise ValueError('expected native schema 2 and explicit status')
    meta = data['metadata']
    scenario = data['scenario']
    workflow = scenario in native.benchmark_workflows.METRICS
    if scenario not in ('startup', 'editor-open') and not workflow:
        raise ValueError('unsupported measurement boundary')
    for key in ('runs', 'warmups'):
        if type(meta[key]) is not int or meta[key] < (5 if key == 'runs' else 0):
            raise ValueError('invalid requested run counts')
    if not re.fullmatch(r'[0-9a-f]{64}', meta.get('executable_sha256', '')):
        raise ValueError('missing binary identity')
    # Only these simple descriptors are public, never arbitrary source strings.
    platform = meta.get('platform', '')
    if not re.fullmatch(r'[A-Za-z0-9_.+ -]{1,160}', platform):
        raise ValueError('invalid platform descriptor')
    version = meta.get('nus_version')
    if version is not None and not re.fullmatch(r'nus [0-9A-Za-z.()+ -]{1,100}', version):
        raise ValueError('invalid binary version')
    generated = meta.get('generated_at_utc', '')
    if not re.fullmatch(r'[0-9T:.+Z-]{10,40}', generated):
        raise ValueError('invalid timestamp')
    runs, warmups, attempts = data['runs'], data['warmup_runs'], data['attempts']
    if len(runs) > meta['runs'] or len(warmups) > meta['warmups'] or sum(a.get('kind') == 'sample' for a in attempts) > meta['runs'] or sum(a.get('kind') == 'warmup' for a in attempts) > meta['warmups']:
        raise ValueError('excess retained runs')
    if any(a.get('kind') not in ('warmup', 'sample') or a.get('status') not in ('valid', 'failed') for a in attempts):
        raise ValueError('invalid attempt')
    for kind, retained in [('sample', runs), ('warmup', warmups)]:
        if sum(a['kind'] == kind and a['status'] == 'valid' for a in attempts) != len(retained):
            raise ValueError('attempt/retained sample mismatch')
    failures = sum(a['status'] == 'failed' for a in attempts)
    if data['status'] == 'complete' and (failures or len(runs) != meta['runs'] or len(warmups) != meta['warmups']):
        raise ValueError('incomplete report marked complete')
    size = meta.get('editor_bytes') if scenario == 'editor-open' else None
    if scenario == 'editor-open' and (type(size) is not int or size <= 0):
        raise ValueError('invalid editor fixture size')
    metric = native.benchmark_workflows.METRICS[scenario] if workflow else 'startup_first_present' if scenario == 'startup' else native.editor_metric(size)
    if workflow:
        hashes=meta.get('harness_hashes', {})
        if set(hashes) != {'perf-native.py','benchmark_workflows.py','benchmark_fixture.py'} or any(not re.fullmatch(r'[0-9a-f]{64}', v) for v in hashes.values()):
            raise ValueError('missing workflow fixture identity')
    for run in runs + warmups:
        native.validate_metrics(run['metrics'], scenario, size or 0)
        if workflow and run.get('workflow_validation') != {'schema':1,'scenario':scenario,'passed':True}:
            raise ValueError('missing workflow correctness evidence')
        memory = run['memory']
        if type(memory.get('available')) is not bool:
            raise ValueError('memory availability must be explicit')
        if memory['available']:
            for field in ('main_rss_kib', 'tree_rss_kib'):
                number(memory[field])
    labels={'edit-verify':'Edit, save and verify in browser', 'port-conflict':'Recover an occupied local port', 'project-switch':'Switch project buffers with PTY output'}
    boundaries={'edit-verify':'Prepared server → page open, file edit and save → browser DOM confirms saved bytes', 'port-conflict':'Contender launch → real conflict, owner jump, stop and restart → both services confirmed in browser', 'project-switch':'Dirty project A → edit/save project B → return/save A → browser confirms A; output counter advances during interval'}
    observations = [{'run': i + 1, 'value': number(r['metrics'][metric]['p50_ms'])} for i, r in enumerate(runs)]
    memory = [{'run': i + 1, 'value': number(r['memory']['tree_rss_kib']) / 1024} for i, r in enumerate(runs) if r['memory']['available']]
    return {
        'id': source_hash[:16], 'source_sha256': source_hash,
        'status': data['status'], 'product': 'NUS', 'scenario': scenario,
        'recorded_at': generated, 'platform': platform, 'version': version,
        'binary_sha256': meta['executable_sha256'],
        'requested_runs': meta['runs'], 'completed_runs': len(runs),
        'warmup_runs': len(warmups), 'failed_attempts': failures,
        'unattempted_runs': meta['runs'] - sum(a['kind'] == 'sample' for a in attempts),
        'execution': 'Local scripted pilot; application-owned actions include event-loop scheduling and oracle overhead. Not human task timing or input-to-photon latency.' if workflow else 'Application-owned scenario; not human task timing',
        'harness_hashes': {k:v for k,v in meta.get('harness_hashes', {}).items() if k in ('perf-native.py','benchmark_workflows.py','benchmark_fixture.py') and isinstance(v,str) and re.fullmatch(r'[0-9a-f]{64}',v)},
        'cache_policy': 'Fresh onboarded profiles; OS/filesystem caches not evicted',
        'hardware': 'Hardware model, power and thermal state not recorded by this source',
        'metrics': [
            {'id': metric + ('-' + str(size) if size else ''),
             'label': labels[scenario] if workflow else 'Main entry to first presentation call' if scenario == 'startup' else f'Open {size / 1048576:g} MiB text file',
             'unit': 'ms', 'direction': 'lower',
             'boundary': boundaries[scenario] if workflow else 'Rust main entry → application present-call return; not display scanout' if scenario == 'startup' else 'File open request → loaded content frame submission; not display scanout',
             'observations': observations},
            {'id': 'tree_rss_mib', 'label': 'Whole process tree RSS', 'unit': 'MiB', 'direction': 'lower',
             'boundary': 'Point-in-time summed resident pages, including shared-page double counting; not private footprint',
             'observations': memory},
        ],
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('inputs', nargs='+', type=Path)
    parser.add_argument('--out', required=True, type=Path)
    args = parser.parse_args()
    records = [export_report(json.loads(p.read_text()), native.sha256(p)) for p in args.inputs]
    if len({r['id'] for r in records}) != len(records):
        raise ValueError('duplicate source report')
    native.atomic_write(args.out, json.dumps({'schema': 1, 'records': records}, indent=2, allow_nan=False) + '\n')
    print(args.out)


if __name__ == '__main__':
    main()
