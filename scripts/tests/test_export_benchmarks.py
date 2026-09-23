import copy
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('export_benchmarks', Path(__file__).parents[1]/'export-benchmarks.py')
exporter = importlib.util.module_from_spec(spec)
spec.loader.exec_module(exporter)


def report():
    metric = dict(count=1,p50_ms=20,p95_ms=20,p99_ms=20,max_ms=20)
    stats = {k:dict(metric) for k in (*exporter.native.STARTUP_METRICS,'main_to_first_submit')}
    run = dict(metrics=stats,memory=dict(available=True,main_rss_kib=100,tree_rss_kib=200),log_file='/private/token')
    return dict(schema=2,status='complete',scenario='startup',runs=[copy.deepcopy(run) for _ in range(5)],warmup_runs=[],
                attempts=[dict(kind='sample',status='valid',log_file='/private/token') for _ in range(5)],
                metadata=dict(runs=5,warmups=0,executable_sha256='a'*64,platform='macOS-arm64',nus_version='nus 0.0.1 (abc)',generated_at_utc='2026-09-23T00:00:00Z',executable='/Users/private',logs='/secret'),
                summary={'forged':'never exported'})


class ExportTests(unittest.TestCase):
    def test_export_is_allowlisted_and_derived(self):
        result = exporter.export_report(report(),'b'*64)
        self.assertNotIn('/private',str(result));self.assertNotIn('/Users',str(result));self.assertNotIn('forged',str(result))
        self.assertEqual(result['metrics'][0]['observations'],[dict(run=i+1,value=20) for i in range(5)])

    def test_workflow_requires_correctness_and_fixture_identity(self):
        d=report();d['scenario']='edit-verify'
        d['metadata']['harness_hashes']={k:'c'*64 for k in ('perf-native.py','benchmark_workflows.py','benchmark_fixture.py')}
        for run in d['runs']:
            run['metrics']={'workflow_edit_verify_v1':dict(count=1,p50_ms=20,p95_ms=20,p99_ms=20,max_ms=20)}
            run['workflow_validation']={'schema':1,'scenario':'edit-verify','passed':True}
        self.assertEqual(exporter.export_report(d,'b'*64)['metrics'][0]['label'],'Edit, save and verify in browser')
        del d['runs'][0]['workflow_validation']
        with self.assertRaisesRegex(ValueError,'correctness'):exporter.export_report(d,'b'*64)

    def test_extra_attempts_and_private_metadata_rejected_or_excluded(self):
        d=report();d['metadata']['harness_hashes']={'/secret':'token'}
        self.assertEqual(exporter.export_report(d,'b'*64)['harness_hashes'],{})
        d['attempts'].append(dict(kind='sample',status='failed'))
        with self.assertRaises(ValueError):exporter.export_report(d,'b'*64)

    def test_incomplete_cannot_be_complete(self):
        d=report();d['runs'].pop()
        with self.assertRaises(ValueError):exporter.export_report(d,'b'*64)

    def test_invalid_samples_rejected(self):
        for value in (float('nan'),float('inf'),-1,True):
            d=report();d['runs'][0]['metrics']['startup_first_present']['p50_ms']=value
            with self.assertRaises(ValueError):exporter.export_report(d,'b'*64)

    def test_failed_attempts_retained_without_private_error(self):
        d=report();d['status']='failed';d['runs'].pop();d['attempts'][-1].update(status='failed',error='/secret/password')
        out=exporter.export_report(d,'b'*64)
        self.assertEqual(out['failed_attempts'],1);self.assertEqual(out['completed_runs'],4)
        self.assertNotIn('password',str(out))


if __name__=='__main__':unittest.main()
