#!/usr/bin/env python3
"""Run isolated native nus scenarios; retain validated samples, logs, JSON and HTML.

Timing comes from NUS_PERF, not process wall clock. A fresh nus profile is NOT a
cold OS/filesystem cache. Run this against a rebuilt, packaged native app.
"""
from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import html
import json
import math
import os
from pathlib import Path
import platform
import signal
import statistics
import subprocess
import sys
import tempfile
from typing import Any
import uuid
import contextlib
import importlib.util

_workflow_spec = importlib.util.spec_from_file_location('benchmark_workflows', Path(__file__).with_name('benchmark_workflows.py'))
benchmark_workflows = importlib.util.module_from_spec(_workflow_spec)
_workflow_spec.loader.exec_module(benchmark_workflows)

SCHEMA = 2
# NSArgumentDomain overrides are volatile (this process only). Do not use
# `defaults write`, delete ~/Library/Saved Application State, or click dialogs.
MACOS_LAUNCH_ARGUMENTS = (
    "-ApplePersistenceIgnoreState", "YES",
    "-NSQuitAlwaysKeepsWindows", "NO",
)
STARTUP_METRICS = (
    "startup_private_ready", "startup_dock_ready", "startup_cef_ready",
    "startup_event_loop_ready", "startup_window_created", "startup_app_ready",
    "startup_first_present",
)


def percentile(values: list[float], q: float) -> float | None:
    if not 0.0 <= q <= 1.0:
        raise ValueError("percentile must be between zero and one")
    if not values:
        return None
    ordered = sorted(values)
    return ordered[max(0, math.ceil(len(ordered) * q) - 1)]


def median_abs_deviation(values: list[float]) -> float | None:
    if not values:
        return None
    med = statistics.median(values)
    return statistics.median(abs(x - med) for x in values)


def summary(values: list[float]) -> dict[str, Any]:
    if any(not math.isfinite(v) or v < 0 for v in values):
        raise ValueError("samples must be finite and nonnegative")
    if not values:
        return dict(count=0, median=None, p95=None, min=None, max=None, mad=None)
    return dict(count=len(values), median=statistics.median(values),
                p95=percentile(values, .95), min=min(values), max=max(values),
                mad=median_abs_deviation(values))


def executable_for(path: Path) -> Path:
    path = path.expanduser().resolve()
    return path / "Contents" / "MacOS" / "nus" if path.suffix == ".app" else path


def launch_command(executable: Path, *, version: bool = False) -> list[str]:
    # --version must be first: main() handles it before initializing AppKit/CEF.
    command = [str(executable)] + (["--version"] if version else [])
    if sys.platform == "darwin":
        command.extend(MACOS_LAUNCH_ARGUMENTS)
    return command


def run_environment(directory: Path, shot: Path) -> dict[str, str]:
    # Inherited shot scripts, fake clocks, or artificial startup delays must not
    # silently turn into a measurement of another workload.
    env = {key: value for key, value in os.environ.items() if not key.startswith("NUS_")}
    temporary = directory / "tmp"
    temporary.mkdir()
    env.update(NUS_PERF="1", NUS_SHOT=str(shot), NUS_SHOT_DIR=str(directory),
               NUS_SHOT_OUT=str(directory / "screens"), NUS_SHOT_SIZE="1440x900",
               TMPDIR=str(temporary) + os.sep, TMP=str(temporary), TEMP=str(temporary))
    return env


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def version_of(executable: Path) -> str | None:
    try:
        result = subprocess.run(launch_command(executable, version=True), capture_output=True,
                                text=True, timeout=5)
        return result.stdout.strip() if result.returncode == 0 else None
    except (OSError, subprocess.SubprocessError):
        return None


def revision() -> str | None:
    try:
        return subprocess.check_output(["git", "rev-parse", "HEAD"], text=True,
                                       stderr=subprocess.DEVNULL, timeout=5).strip()
    except (OSError, subprocess.SubprocessError):
        return None


def parse_records(output: str, prefix: str) -> list[tuple[str, dict[str, Any]]]:
    records = []
    for line in output.splitlines():
        # Accept only the actual protocol line, never an echoed/logged command.
        if not line.startswith(prefix + " "):
            continue
        label, separator, payload = line[len(prefix) + 1:].partition(" ")
        if not label or not separator:
            raise ValueError(f"malformed {prefix} record")
        try:
            value = json.loads(payload)
        except json.JSONDecodeError as error:
            raise ValueError(f"invalid {prefix} JSON for {label}") from error
        if not isinstance(value, dict):
            raise ValueError(f"{prefix} {label} must contain a JSON object")
        records.append((label, value))
    return records


def editor_metric(size: int) -> str:
    # Mirrors Buffer::presented() in spikes/composite/src/editor.rs.
    if size >= 100 * 1024 * 1024:
        return "file_100m_open_submit"
    if size >= 10 * 1024 * 1024:
        return "file_10m_open_submit"
    return "file_open_submit"


def validate_metrics(metrics: dict[str, Any], scenario: str, editor_bytes: int) -> None:
    if not metrics:
        raise ValueError("empty PERF record; rebuild the native app with NUS_PERF support")
    for name, stat in metrics.items():
        if not isinstance(stat, dict) or type(stat.get("count")) is not int or stat["count"] < 1:
            raise ValueError(f"invalid sample count for {name}")
        values = [stat.get(field) for field in ("p50_ms", "p95_ms", "p99_ms", "max_ms")]
        if any(type(v) not in (int, float) or not math.isfinite(v) or v < 0 for v in values):
            raise ValueError(f"invalid timing samples for {name}")
        if values != sorted(values):
            raise ValueError(f"inconsistent percentiles for {name}")
    startup_marks = tuple("startup_cef_deferred" if name == "startup_cef_ready" and "startup_cef_deferred" in metrics else name for name in STARTUP_METRICS)
    required = (*startup_marks, "main_to_first_submit") if scenario == "startup" else (benchmark_workflows.METRICS[scenario],) if scenario in benchmark_workflows.METRICS else (editor_metric(editor_bytes),)
    for name in required:
        stat = metrics.get(name)
        if stat is None or stat["count"] != 1 or stat["p50_ms"] <= 0:
            raise ValueError(f"missing/invalid one-shot metric {name}; rebuild the native bundle")
    if scenario == "startup":
        marks = [metrics[name]["p50_ms"] for name in startup_marks]
        if marks != sorted(marks):
            raise ValueError("startup milestones are not chronological")
        if metrics["startup_first_present"] != metrics["main_to_first_submit"]:
            raise ValueError("startup alias disagrees with first-present measurement")


def sample_from_output(output: str, scenario: str, editor_bytes: int = 10 * 1024 * 1024) -> dict[str, Any]:
    label = scenario if scenario in benchmark_workflows.METRICS else "startup" if scenario == "startup" else "editor"
    perf = parse_records(output, "PERF")
    if len(perf) != 1 or perf[0][0] != label:
        raise ValueError(f"expected exactly one PERF {label} record; got {[x[0] for x in perf]}")
    metrics = perf[0][1]
    validate_metrics(metrics, scenario, editor_bytes)
    memory = parse_records(output, "MEMORY")
    if len(memory) != 1 or memory[0][0] != label:
        raise ValueError(f"expected exactly one MEMORY {label} record")
    rss = memory[0][1]
    if type(rss.get("available")) is not bool:
        raise ValueError("MEMORY must say whether RSS is available")
    if rss["available"]:
        for key in ("main_rss_kib", "tree_rss_kib"):
            value = rss.get(key)
            if type(value) not in (int, float) or not math.isfinite(value) or value < 0:
                raise ValueError(f"invalid {key}")
    return {"metrics": metrics, "memory": rss}


def make_editor_file(path: Path, size: int) -> None:
    line = b"0123456789abcdef deterministic nus editor performance fixture\n"
    block = line * 4096
    with path.open("wb") as stream:
        remaining = size
        while remaining:
            part = block[:remaining]
            stream.write(part)
            remaining -= len(part)


def scenario_script(name: str, directory: Path, editor_bytes: int) -> str:
    if name in benchmark_workflows.METRICS:
        return benchmark_workflows.scenario_script(name, directory)
    ready = "awaitperf startup_first_present\nassertpresented\nasserttabs 1\n"
    if name == "startup":
        # Never reset startup samples or time a human dismissing a modal.
        return ready + "perfstats startup\nmemory startup\n"
    if name == "editor-open":
        target = directory / "editor-perf.txt"
        make_editor_file(target, editor_bytes)
        return (ready + f"perfreset\nopenfile {target}\nawaitperf {editor_metric(editor_bytes)}\n"
                f"asserteditorready {editor_bytes}\nperfstats editor\nmemory editor\n")
    raise ValueError(f"unknown scenario: {name}")


def terminate_run(process: subprocess.Popen[Any]) -> None:
    """Stop only the launched run, never other nus instances by name."""
    if os.name == "posix":
        for sig in (signal.SIGTERM, signal.SIGKILL):
            try:
                os.killpg(process.pid, sig)
            except ProcessLookupError:
                pass
            if sig == signal.SIGTERM:
                try:
                    process.wait(timeout=2)
                except subprocess.TimeoutExpired:
                    pass
        process.wait(timeout=5)
    else:
        try:
            subprocess.run(["taskkill", "/PID", str(process.pid), "/T", "/F"],
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=5)
        except (OSError, subprocess.SubprocessError):
            process.kill()
        if process.poll() is None:
            process.kill()
        process.wait(timeout=5)


def run_once(executable: Path, scenario: str, editor_bytes: int, timeout: float,
             log_path: Path) -> dict[str, Any]:
    with tempfile.TemporaryDirectory(prefix="nus-perf-") as td, contextlib.ExitStack() as cleanup:
        directory = Path(td).resolve()
        profile = directory / "profile"
        profile.mkdir()
        (profile / "onboarded").write_text("skip", encoding="utf-8")
        shot = directory / "perf.shot"
        shot.write_text(scenario_script(scenario, directory, editor_bytes), encoding="utf-8")
        if scenario in benchmark_workflows.METRICS:
            cleanup.callback(benchmark_workflows.cleanup, directory)
            (profile / 'settings.json').write_text(json.dumps({'behavior': {'splash': 'None', 'keep_alive': 'Off', 'close_asks': False, 'ports_probe': False, 'ports_show_docker': False}, 'motion': {'register':0.5,'reduce': True}}))
        env = run_environment(directory, shot)
        log_path.parent.mkdir(parents=True, exist_ok=True)
        # A file also prevents inherited helper stdout handles from keeping
        # communicate() blocked after the main process exits.
        with log_path.open("wb") as output:
            output.write(("HARNESS command: " + json.dumps(launch_command(executable)) + "\n").encode("utf-8"))
            output.flush()
            process = subprocess.Popen(launch_command(executable), cwd=directory, env=env,
                                       stdout=output, stderr=subprocess.STDOUT,
                                       start_new_session=(os.name == "posix"))
            try:
                returncode = process.wait(timeout=timeout)
            except subprocess.TimeoutExpired as error:
                terminate_run(process)
                raise RuntimeError(f"native run timed out after {timeout:g}s; log: {log_path}") from error
            except KeyboardInterrupt:
                terminate_run(process)
                raise
        text = log_path.read_text(encoding="utf-8", errors="replace")
        if returncode != 0 or "panicked" in text:
            raise RuntimeError(f"native run failed ({returncode}); log: {log_path}\n{text[-6000:]}")
        try:
            sample = sample_from_output(text, scenario, editor_bytes)
            if scenario in benchmark_workflows.METRICS:
                sample['workflow_validation'] = benchmark_workflows.validate(scenario, directory)
        except ValueError as error:
            raise RuntimeError(f"{error}; log: {log_path}\n{text[-3000:]}") from error
        return {**sample, "log_file": str(log_path)}


def aggregate(runs: list[dict[str, Any]]) -> tuple[dict[str, Any], dict[str, Any]]:
    metrics = {}
    for name in sorted({name for run in runs for name in run["metrics"]}):
        raw = [float(run["metrics"][name]["p50_ms"]) for run in runs if name in run["metrics"]]
        metrics[name] = {**summary(raw), "unit": "ms", "raw": raw,
                         "missing_runs": len(runs) - len(raw),
                         "aggregation": "across-process p50_ms (not pooled event percentiles)"}
    memory = {}
    for name in ("main_rss_kib", "tree_rss_kib"):
        raw = [float(run["memory"][name]) for run in runs if run["memory"].get("available")]
        memory[name] = {**summary(raw), "unit": "KiB", "raw": raw,
                        "missing_runs": len(runs) - len(raw)}
    return metrics, memory


def fmt(value: Any, digits: int = 2) -> str:
    return "—" if value is None else f"{float(value):.{digits}f}"


def sparkline(values: list[float]) -> str:
    # Each plot uses its own complete zero-to-maximum range. Never clip outliers.
    scale = max(values, default=1) or 1
    points = [(8 + i * 384 / max(1, len(values) - 1), 44 - 38 * value / scale)
              for i, value in enumerate(values)]
    line = " ".join(f"{x:.2f},{y:.2f}" for x, y in points)
    dots = "".join(f'<circle cx="{x:.2f}" cy="{y:.2f}" r="2"><title>run {i+1}: {v:.6f} ms</title></circle>'
                   for i, ((x, y), v) in enumerate(zip(points, values)))
    return (f'<svg viewBox="0 0 400 50" role="img" aria-label="Per-run p50 values, zero to {scale:.6f} milliseconds">'
            f'<polyline points="{line}"/>{dots}</svg>')


def html_report(data: dict[str, Any]) -> str:
    metrics = data["summary"]["metrics"]
    max_metric = max((m["median"] or 0 for m in metrics.values()), default=1) or 1
    rows = []
    for name, stat in metrics.items():
        width = 100 * (stat["median"] or 0) / max_metric
        rows.append(f'''<section><div class="head"><strong>{html.escape(name)}</strong>
<span>{fmt(stat['median'])} ms median · {fmt(stat['p95'])} ms empirical p95 · MAD {fmt(stat['mad'])}</span></div>
<div class="bar"><i style="width:{width:.3f}%"></i></div>{sparkline(stat['raw'])}
<small>n={stat['count']} · missing runs={stat['missing_runs']} · range {fmt(stat['min'])}–{fmt(stat['max'])} ms</small>
<details><summary>Every per-run value (ms)</summary><pre>{html.escape(json.dumps(stat['raw']))}</pre></details></section>''')
    memory = "".join(f'<section><strong>{html.escape(name)}</strong><p>{fmt(stat["median"], 0)} KiB median · n={stat["count"]}</p></section>'
                     for name, stat in data["summary"]["memory"].items() if stat["count"])
    failure = f'<pre>{html.escape(data.get("error", ""))}</pre>' if data.get("error") else ""
    return f'''<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>nus performance · {html.escape(data['scenario'])}</title><style>
:root{{color-scheme:light dark;font:15px/1.5 system-ui,sans-serif}}body{{max-width:1000px;margin:40px auto;padding:0 24px}}h1{{margin-bottom:4px}}
section{{border:1px solid;border-radius:10px;margin:12px 0;padding:16px}}.head{{display:flex;justify-content:space-between;gap:16px;flex-wrap:wrap}}
.bar{{height:7px;margin:12px 0;background:color-mix(in srgb,currentColor 12%,transparent)}}.bar i{{display:block;height:100%;background:currentColor}}
svg{{height:55px;width:100%}}polyline{{fill:none;stroke:currentColor;stroke-width:1.5}}circle{{fill:currentColor}}pre{{white-space:pre-wrap;overflow-wrap:anywhere}}small{{opacity:.8}}
</style><h1>nus performance</h1><p><strong>Status: {html.escape(data['status'])}</strong> · {html.escape(data['scenario'])} · {len(data['runs'])}/{data['metadata']['runs']} measured runs</p>{failure}
<p>Startup milestones are cumulative from main() and must not be added together. First-present means the app returned from its present path, not measured display scanout.</p>
<p>Recurring metrics summarize <strong>per-process p50s</strong>; their p95 here is not event-level frame/input p95. With fewer than 20 launches, nearest-rank p95 is the observed maximum, not a reliable tail estimate.</p>
<h2>Timings</h2>{''.join(rows) or '<p>No valid measured samples yet.</p>'}
<p>Bars compare medians within this report. Each trace has its own zero-to-max scale, includes every retained sample and does not clip outliers.</p>
<h2>Memory</h2>{memory or '<p>RSS unavailable.</p>'}<p>Summed RSS includes shared pages more than once; it is not private physical memory.</p>
<h2>Provenance</h2><pre>{html.escape(json.dumps(data['metadata'], indent=2))}</pre>
<p>The adjacent JSON retains raw process summaries, warmups and failure status. Per-event samples are not exported by NUS_PERF. Logs are retained separately.</p></html>'''


def atomic_write(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    # Same-directory temporary file keeps replacement on the same filesystem.
    with tempfile.NamedTemporaryFile("w", encoding="utf-8", dir=path.parent, delete=False) as stream:
        temporary = Path(stream.name)
        try:
            stream.write(text)
        except BaseException:
            temporary.unlink(missing_ok=True)
            raise
    try:
        os.replace(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)


def write_report(path: Path, data: dict[str, Any]) -> None:
    metrics, memory = aggregate(data["runs"])
    data["summary"] = {"metrics": metrics, "memory": memory}
    atomic_write(path, json.dumps(data, indent=2, sort_keys=True, allow_nan=False) + "\n")
    atomic_write(path.with_suffix(".html"), html_report(data))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--app", required=True, type=Path, help="native executable or macOS .app bundle")
    parser.add_argument("--scenario", choices=["startup", "editor-open", *benchmark_workflows.METRICS], default="startup")
    parser.add_argument("--runs", type=int, default=10, help="measured launches (minimum 5)")
    parser.add_argument("--warmups", type=int, default=1)
    parser.add_argument("--timeout", type=float, default=90.0)
    parser.add_argument("--editor-bytes", type=int, default=10 * 1024 * 1024)
    parser.add_argument("--out", type=Path, default=Path("perf-native.json"))
    args = parser.parse_args()
    if args.runs < 5 or args.warmups < 0:
        parser.error("--runs must be at least 5 and --warmups must be nonnegative")
    if not math.isfinite(args.timeout) or args.timeout <= 0 or args.editor_bytes <= 0:
        parser.error("--timeout must be finite and positive; --editor-bytes must be positive")
    if args.out.suffix.lower() != ".json":
        parser.error("--out must end in .json (the companion file ends in .html)")
    executable = executable_for(args.app)
    if not executable.is_file() or not os.access(executable, os.X_OK):
        parser.error(f"not an executable: {executable}")
    output = args.out.expanduser().resolve()
    run_id = uuid.uuid4().hex[:12]
    logs = output.parent / (output.stem + "-logs") / run_id
    logs.mkdir(parents=True)
    data: dict[str, Any] = {
        "schema": SCHEMA, "status": "running", "scenario": args.scenario,
        "runs": [], "warmup_runs": [], "attempts": [],
        "metadata": {
            "generated_at_utc": dt.datetime.now(dt.timezone.utc).isoformat(),
            "source_revision": revision(), "nus_version": version_of(executable),
            "executable": str(executable), "executable_sha256": sha256(executable),
            "platform": platform.platform(), "python": sys.version.split()[0],
            "runs": args.runs, "warmups": args.warmups, "timeout_seconds": args.timeout,
            "editor_bytes": args.editor_bytes if args.scenario == "editor-open" else None,
            "launch_arguments": launch_command(executable)[1:],
            "macos_resume_policy": "ignore-existing-state" if sys.platform == "darwin" else "not-applicable",
            "profile_policy": "fresh onboarded nus profile and temp directory for each launch",
            "cache_policy": "OS/filesystem caches not evicted; not a cold-disk benchmark",
            "measurement": "NUS_PERF in-process summaries; no subprocess elapsed-time substitution",
            "percentiles": "nearest-rank across process p50s; p95 equals max for n < 20",
            "logs": str(logs),
            "harness_hashes": {p.name: sha256(p) for p in (Path(__file__), Path(__file__).with_name('benchmark_workflows.py'), Path(__file__).with_name('benchmark_fixture.py'))},
            "study_class": "local scripted pilot; uncontrolled background activity",
            "workflow_scope": "application-owned actions paced by the event loop; not human input or photon latency",

        },
    }
    write_report(output, data)
    for i in range(args.warmups + args.runs):
        kind = "warmup" if i < args.warmups else "sample"
        log = logs / f"{i + 1:03d}-{kind}.log"
        print(f"{kind} {i + 1}/{args.warmups + args.runs} · {log}", flush=True)
        attempt: dict[str, Any] = {"kind": kind, "log_file": str(log)}
        data["attempts"].append(attempt)
        try:
            sample = run_once(executable, args.scenario, args.editor_bytes, args.timeout, log)
        except (OSError, RuntimeError, KeyboardInterrupt) as error:
            message = str(error) or "interrupted"
            attempt.update(status="failed", error=message)
            data.update(status="failed", error=message)
            write_report(output, data)
            print(f"FAILED: {message}\nPartial report: {output}", file=sys.stderr)
            return 130 if isinstance(error, KeyboardInterrupt) else 1
        attempt["status"] = "valid"
        data["warmup_runs" if kind == "warmup" else "runs"].append(sample)
        write_report(output, data)
    if sha256(executable) != data["metadata"]["executable_sha256"]:
        data.update(status="failed", error="native executable changed during measurement")
        write_report(output, data)
        print(data["error"], file=sys.stderr)
        return 1
    data["status"] = "complete"
    write_report(output, data)
    print(f"JSON: {output}\nHTML: {output.with_suffix('.html')}")
    for name, stat in data["summary"]["metrics"].items():
        print(f"{name:32} median {fmt(stat['median'])} ms  p95 {fmt(stat['p95'])} ms  MAD {fmt(stat['mad'])}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
