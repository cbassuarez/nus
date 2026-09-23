import importlib.util
from pathlib import Path
import tempfile
import unittest

spec=importlib.util.spec_from_file_location('workflows',Path(__file__).parents[1]/'benchmark_workflows.py')
w=importlib.util.module_from_spec(spec);spec.loader.exec_module(w)

class WorkflowTests(unittest.TestCase):
    def test_wrong_saved_bytes_fail_the_oracle(self):
        with tempfile.TemporaryDirectory() as td:
            p=Path(td);(p/'A.txt').write_text('before')
            with self.assertRaisesRegex(ValueError,'wrong saved'):w.validate('edit-verify',p)
            (p/'A.txt').write_text('beforex')
            self.assertTrue(w.validate('edit-verify',p)['passed'])

    def test_project_switch_checks_both_buffers_and_load(self):
        with tempfile.TemporaryDirectory() as td:
            p=Path(td);(p/'A.txt').write_text('beforex');(p/'B.txt').write_text('wrong');(p/'A.load').write_text('100')
            with self.assertRaisesRegex(ValueError,'project B'):w.validate('project-switch',p)
            (p/'B.txt').write_text('beforey');(p/'A.load').write_text('0')
            with self.assertRaisesRegex(ValueError,'load'):w.validate('project-switch',p)

    def test_port_recovery_requires_real_conflict_and_old_owner_stop(self):
        with tempfile.TemporaryDirectory() as td:
            p=Path(td)
            for name in ('B.pid','U.pid','B.conflict'):(p/name).write_text('fixture')
            with self.assertRaisesRegex(ValueError,'A.stopped'):w.validate('port-conflict',p)
            (p/'A.stopped').write_text('stopped');self.assertTrue(w.validate('port-conflict',p)['passed'])

if __name__=='__main__':unittest.main()
