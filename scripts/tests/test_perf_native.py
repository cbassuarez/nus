"""Harness tests use a fake executable, not a GPU/CEF app or real measurements."""
from __future__ import annotations

import contextlib
import copy
import importlib.util
import io
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("nus_perf_native", ROOT / "scripts/perf-native.py")
assert spec is not None and spec.loader is not None
perf = importlib.util.module_from_spec(spec)
spec.loader.exec_module(perf)


def metric(value: float) -> dict:
    return dict(count=1, p50_ms=value, p95_ms=value, p99_ms=value, max_ms=value, over_16_67_ms=0)


def startup() -> dict:
    values = {name: metric(i + 1.0) for i, name in enumerate(perf.STARTUP_METRICS)}
    values["main_to_first_submit"] = dict(values["startup_first_present"])
    return values


def output(metrics=None, label="startup") -> str:
    return (f"PERF {label} {json.dumps(startup() if metrics is None else metrics)}\n"
            f'MEMORY {label} {{"available": false}}\n')


class LaunchTests(unittest.TestCase):
    def test_macos_arguments_are_per_launch(self):
        with mock.patch.object(sys, "platform", "darwin"):
            command = perf.launch_command(Path("/test/nus"))
        self.assertEqual(command, ["/test/nus", "-ApplePersistenceIgnoreState", "YES", "-NSQuitAlwaysKeepsWindows", "NO"])

    def test_version_remains_first(self):
        with mock.patch.object(sys, "platform", "darwin"):
            self.assertEqual(perf.launch_command(Path("/test/nus"), version=True)[1], "--version")

    def test_other_platforms_do_not_receive_appkit_arguments(self):
        with mock.patch.object(sys, "platform", "linux"):
            self.assertEqual(perf.launch_command(Path("/test/nus")), ["/test/nus"])

    def test_no_inherited_nus_test_hooks(self):
        with tempfile.TemporaryDirectory() as td, mock.patch.dict(os.environ, {
                "NUS_DOCK_LAUNCH_TEST_MS": "3000", "NUS_SHOT2": "bad.shot", "NUS_MODE": "paper"}):
            root = Path(td)
            env = perf.run_environment(root, root / "run.shot")
            self.assertNotIn("NUS_DOCK_LAUNCH_TEST_MS", env)
            self.assertNotIn("NUS_SHOT2", env)
            self.assertNotIn("NUS_MODE", env)
            self.assertEqual(Path(env["TMPDIR"]), root / "tmp")
            self.assertTrue(Path(env["TMPDIR"]).is_dir())
            self.assertEqual(env["NUS_PERF"], "1")

    def test_startup_waits_without_resetting_or_sleeping(self):
        script = perf.scenario_script("startup", Path("/tmp/test"), 100)
        self.assertTrue(script.startswith("awaitperf startup_first_present\n"))
        self.assertNotIn("perfreset", script)
        self.assertNotIn("\nwait ", script)
        self.assertIn("asserttabs 1", script)

    def test_editor_waits_for_size_specific_metric(self):
        with tempfile.TemporaryDirectory() as td:
            script = perf.scenario_script("editor-open", Path(td), 1024)
            self.assertIn("awaitperf file_open_submit", script)
            self.assertNotIn("wait 5000", script)
            self.assertEqual((Path(td) / "editor-perf.txt").stat().st_size, 1024)
        self.assertEqual(perf.editor_metric(10 * 1024 * 1024), "file_10m_open_submit")
        self.assertEqual(perf.editor_metric(100 * 1024 * 1024), "file_100m_open_submit")
        self.assertEqual(perf.editor_metric(10 * 1024 * 1024 - 1), "file_open_submit")


class ValidationTests(unittest.TestCase):
    def test_accepts_complete_startup(self):
        self.assertEqual(perf.sample_from_output(output(), "startup")["metrics"], startup())

    def test_deferred_browser_has_its_own_required_startup_marker(self):
        values = startup()
        values["startup_cef_deferred"] = values.pop("startup_cef_ready")
        self.assertEqual(perf.sample_from_output(output(values), "startup")["metrics"], values)
        del values["startup_cef_deferred"]
        with self.assertRaises(ValueError):
            perf.sample_from_output(output(values), "startup")

    def test_rejects_empty_metrics(self):
        with self.assertRaises(ValueError):
            perf.sample_from_output(output({}), "startup")

    def test_rejects_missing_first_present(self):
        values = startup()
        del values["startup_first_present"]
        with self.assertRaises(ValueError):
            perf.sample_from_output(output(values), "startup")

    def test_rejects_nan_negative_and_boolean(self):
        for value in (float("nan"), float("inf"), -1, True):
            with self.subTest(value=value):
                values = startup()
                values["startup_first_present"]["p50_ms"] = value
                with self.assertRaises(ValueError):
                    perf.sample_from_output(output(values), "startup")

    def test_rejects_duplicate_and_wrong_scenario_records(self):
        for text in (output() + output(), output(label="other")):
            with self.assertRaises(ValueError):
                perf.sample_from_output(text, "startup")

    def test_rejects_malformed_protocol(self):
        for text in ("PERF startup {\n", "PERF startup []\n", "PERF startup\n"):
            with self.assertRaises(ValueError):
                perf.parse_records(text, "PERF")

    def test_does_not_parse_an_echoed_record(self):
        self.assertEqual(perf.parse_records('log: PERF startup {"fake": 1}\n', "PERF"), [])

    def test_rejects_missing_or_bad_memory(self):
        for tail in ("", 'MEMORY startup {"available": true}\n'):
            text = output().split("MEMORY")[0] + tail
            with self.assertRaises(ValueError):
                perf.sample_from_output(text, "startup")

    def test_rejects_alias_mismatch(self):
        values = startup()
        values["main_to_first_submit"] = metric(99)
        with self.assertRaises(ValueError):
            perf.sample_from_output(output(values), "startup")

    def test_rejects_multiple_startup_observations(self):
        values = startup()
        values["startup_dock_ready"]["count"] = 2
        with self.assertRaises(ValueError):
            perf.sample_from_output(output(values), "startup")

    def test_requires_matching_editor_metric(self):
        text = output({"file_10m_open_submit": metric(42)}, "editor")
        perf.sample_from_output(text, "editor-open", 10 * 1024 * 1024)
        with self.assertRaises(ValueError):
            perf.sample_from_output(text, "editor-open", 100 * 1024 * 1024)


class StatisticsTests(unittest.TestCase):
    def test_nearest_rank_p95_is_max_with_ten_runs(self):
        stat = perf.summary(list(range(1, 11)))
        self.assertEqual(stat["median"], 5.5)
        self.assertEqual(stat["p95"], 10)
        self.assertEqual(stat["mad"], 2.5)

    def test_aggregation_labels_its_sample_unit_and_missing_runs(self):
        first = perf.sample_from_output(output(), "startup")
        second = copy.deepcopy(first)
        first["metrics"]["optional_event"] = metric(50)
        timing, _ = perf.aggregate([first, second])
        self.assertEqual(timing["optional_event"]["missing_runs"], 1)
        self.assertIn("across-process", timing["optional_event"]["aggregation"])

    def test_plot_preserves_the_largest_outlier(self):
        svg = perf.sparkline([1.0, 2.0, 100.0])
        self.assertIn("run 3: 100.000000 ms", svg)
        self.assertIn('cy="6.00"', svg)
        self.assertNotIn("nan", svg.lower())


class ExecutionTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.exe = self.root / "fake-nus"
        self.exe.write_text(
            f"#!{sys.executable}\n"
            "import json, os, sys, time\nfrom pathlib import Path\n"
            "if '--version' in sys.argv:\n print('nus fake (fixture)'); sys.exit(0)\n"
            "assert Path.cwd() == Path(os.environ['NUS_SHOT_DIR'])\n"
            "assert (Path.cwd()/'profile'/'onboarded').is_file()\n"
            "assert Path(os.environ['TMPDIR']).parent == Path.cwd()\n"
            "print('fake native fixture; not actual nus timing', flush=True)\n"
            "if os.environ.get('FAKE_PERF_TIMEOUT'): time.sleep(60)\n"
            f"print({output()!r}, end='', flush=True)\n",
            encoding="utf-8")
        self.exe.chmod(0o755)

    @unittest.skipIf(os.name == "nt", "fake executable uses a POSIX shebang")
    def test_real_child_has_isolated_cwd_and_retained_log(self):
        log = self.root / "result.log"
        sample = perf.run_once(self.exe, "startup", 1024, 5, log)
        self.assertEqual(sample["metrics"], startup())
        self.assertIn("not actual nus timing", log.read_text())
        self.assertFalse((self.root / "profile").exists())

    @unittest.skipIf(os.name == "nt", "fake executable uses a POSIX shebang")
    def test_timeout_returns_failure_and_keeps_log(self):
        log = self.root / "timeout.log"
        with mock.patch.dict(os.environ, FAKE_PERF_TIMEOUT="1"):
            with self.assertRaisesRegex(RuntimeError, "timed out"):
                perf.run_once(self.exe, "startup", 1024, .3, log)
        self.assertTrue(log.is_file())
        self.assertIn("HARNESS command:", log.read_text())

    @unittest.skipIf(os.name == "nt", "fake executable uses a POSIX shebang")
    def test_end_to_end_fake_run_writes_json_and_html(self):
        target = self.root / "results.json"
        argv = ["perf-native.py", "--app", str(self.exe), "--runs", "5", "--out", str(target)]
        with mock.patch.object(sys, "argv", argv), contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(perf.main(), 0)
        data = json.loads(target.read_text())
        self.assertEqual(data["status"], "complete")
        self.assertEqual(len(data["runs"]), 5)
        self.assertEqual(len(data["warmup_runs"]), 1)
        self.assertEqual(len(data["attempts"]), 6)
        self.assertEqual(data["metadata"]["executable_sha256"], perf.sha256(self.exe))
        report = target.with_suffix(".html").read_text()
        self.assertIn("Status: complete", report)
        self.assertIn("not event-level", report)
        for sample in data["runs"]:
            self.assertTrue(Path(sample["log_file"]).is_file())

    def test_partial_failure_is_retained_not_reported_as_success(self):
        target = self.root / "partial.json"
        argv = ["perf-native.py", "--app", str(self.exe), "--runs", "5", "--warmups", "0", "--out", str(target)]
        sample = perf.sample_from_output(output(), "startup")
        with (mock.patch.object(sys, "argv", argv),
              mock.patch.object(perf, "version_of", return_value="fixture"),
              mock.patch.object(perf, "run_once", side_effect=[sample, RuntimeError("<failed>")]),
              contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO())):
            self.assertEqual(perf.main(), 1)
        data = json.loads(target.read_text())
        self.assertEqual(data["status"], "failed")
        self.assertEqual(len(data["runs"]), 1)
        self.assertEqual(data["attempts"][-1]["status"], "failed")
        self.assertIn("&lt;failed&gt;", target.with_suffix(".html").read_text())


if __name__ == "__main__":
    unittest.main()
