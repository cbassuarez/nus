import importlib.util
from pathlib import Path
import unittest
from unittest.mock import patch

spec=importlib.util.spec_from_file_location('workspace',Path(__file__).parents[1]/'benchmarks/measure-workspace.py')
workspace=importlib.util.module_from_spec(spec);spec.loader.exec_module(workspace)

class AccountingTests(unittest.TestCase):
    def test_pid_union_counts_overlapping_roots_once_and_excludes_other_apps(self):
        table='10 1 1024\n11 10 2048\n12 11 4096\n90 1 999999\n'
        with patch.object(workspace.subprocess,'check_output',return_value=table):
            result=workspace.rss([10,11])
        self.assertEqual(result,{'rss_mib':7.0,'processes':3,'root_count':2})

    def test_a_closed_browser_cannot_be_reported_as_zero_memory(self):
        with patch.object(workspace.subprocess,'check_output',return_value='10 1 1024\n'):
            with self.assertRaisesRegex(ValueError,'root process exited'):
                workspace.rss([10,20])

if __name__=='__main__':unittest.main()
