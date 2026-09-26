#!/usr/bin/env python3
"""Collect existing nus performance data for review; never run benchmarks.

Run from the nus repository after measurement processes have finished.
The archive is for private review, not direct website publication: native JSON
may contain local paths. File timestamps and checkout metadata are collection
context, NOT proof that this checkout produced every benchmark in the archive.
Python 3.9+; standard library only. No source files are modified.
"""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
import tempfile
import zipfile

DATA_ROOTS = ("target/criterion", "target/perf")
SOURCE_FILES = (
    "Cargo.toml", "Cargo.lock", "rust-toolchain", "rust-toolchain.toml",
    "README.md", "docs/DESIGN.md", "docs/PERFORMANCE.md",
    "scripts/perf-native.py", "scripts/tests/test_perf_native.py",
    "crates/vt/src/term.rs", "crates/vt/src/grid.rs",
    "spikes/composite/src/perf.rs",
)
SOURCE_GLOBS = (
    "crates/*/Cargo.toml", "crates/*/benches/**/*.rs",
    "crates/vt/tests/perf_fixtures.rs",
)


def command(root: Path, *args: str) -> str | None:
    try:
        completed = subprocess.run(args, cwd=root, capture_output=True,
                                   text=True, timeout=15, check=False)
        return completed.stdout.strip() if completed.returncode == 0 else None
    except (OSError, subprocess.TimeoutExpired):
        return None


def safe_file(root: Path, path: Path) -> bool:
    try:
        relative = path.relative_to(root)
        if not path.is_file():
            return False
        current = root
        for part in relative.parts:
            current = current / part
            if current.is_symlink():
                return False
        return True
    except (OSError, ValueError):
        return False


def selected_files(root: Path) -> tuple[list[Path], set[Path]]:
    data = set()
    for name in DATA_ROOTS:
        directory = root / name
        if directory.is_dir() and not directory.is_symlink():
            for path in directory.rglob("*"):
                if path.suffix.lower() in {".json", ".csv"} and safe_file(root, path):
                    data.add(path)
    selected = set(data)
    selected.update(root / name for name in SOURCE_FILES
                    if safe_file(root, root / name))
    for pattern in SOURCE_GLOBS:
        selected.update(path for path in root.glob(pattern) if safe_file(root, path))
    return sorted(selected), data


def signature(path: Path) -> tuple[int, int, int, int]:
    info = path.stat()
    return info.st_dev, info.st_ino, info.st_size, info.st_mtime_ns


def collect(root: Path, destination: Path, max_mib: int = 128) -> dict:
    root = root.resolve(strict=True)
    if not (root / "Cargo.toml").is_file() or not (root / "crates/vt").is_dir():
        raise ValueError("Run in the nus repository, or supply --repo /path/to/nus.")
    paths, data = selected_files(root)
    if not data:
        raise ValueError("No result JSON/CSV found in target/criterion or target/perf. "
                         "Benchmark test-mode does not collect performance samples.")
    before = {path: signature(path) for path in paths}
    if sum(item[2] for item in before.values()) > max_mib * 1024 * 1024:
        raise ValueError(f"Selected files exceed {max_mib} MiB; inspect them before "
                         "raising --max-mib.")
    metadata = {
        "bundle_schema": 1,
        "collected_at_utc": datetime.now(timezone.utc).isoformat(),
        "purpose": "private review input; not a validated publication dataset",
        "provenance_note": "Checkout/tool versions are observed at collection time, "
                           "not asserted to be the provenance of existing results. "
                           "No benchmark run is started or certified by this collector.",
        "publication_note": "Allowlist public fields; remove local paths/log references "
                            "from website exports. Preserve raw originals privately.",
        "checkout_at_collection": {
            "head": command(root, "git", "rev-parse", "HEAD"),
            "tracked_status": command(root, "git", "status", "--porcelain", "--untracked-files=no"),
        },
        "tools_at_collection": {"rustc": command(root, "rustc", "-Vv"),
                                "cargo": command(root, "cargo", "-V"),
                                "python": platform.python_version()},
        "machine_at_collection": {"os": platform.system(), "release": platform.release(),
                                  "machine": platform.machine(), "processor": platform.processor()},
        "warnings": [], "files": [],
    }
    if sys.platform == "darwin":
        metadata["machine_at_collection"].update({
            "macos": command(root, "sw_vers", "-productVersion"),
            "model": command(root, "sysctl", "-n", "hw.model"),
            "cpu": command(root, "sysctl", "-n", "machdep.cpu.brand_string"),
            "memory_bytes": command(root, "sysctl", "-n", "hw.memsize"),
        })
    destination = destination.absolute()
    if destination.exists():
        raise ValueError(f"Output already exists: {destination.name}; choose another --out.")
    destination.parent.mkdir(parents=True, exist_ok=True)
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(dir=destination.parent, suffix=".zip", delete=False) as handle:
            temporary = Path(handle.name)
        with zipfile.ZipFile(temporary, "w", zipfile.ZIP_DEFLATED, compresslevel=6) as archive:
            for path in paths:
                content = path.read_bytes()
                if signature(path) != before[path]:
                    raise RuntimeError(f"File changed while collecting: {path.relative_to(root)}. "
                                       "Collect only after measurements finish.")
                name = path.relative_to(root).as_posix()
                if path in data and path.suffix.lower() == ".json":
                    try:
                        parsed = json.loads(content)
                    except (ValueError, UnicodeError) as error:
                        raise ValueError(f"Incomplete/invalid result JSON: {name}: {error}") from error
                    if isinstance(parsed, dict) and parsed.get("status") in {"running", "failed"}:
                        metadata["warnings"].append(f"{name}: status={parsed['status']}; not publishable")
                archive.writestr(name, content)
                metadata["files"].append({"path": name, "bytes": len(content),
                                          "mtime_ns": before[path][3],
                                          "sha256": hashlib.sha256(content).hexdigest(),
                                          "kind": "result" if path in data else "source"})
            if selected_files(root)[0] != paths or any(signature(p) != before[p] for p in paths):
                raise RuntimeError("Inputs changed during collection. Collect after runs finish.")
            archive.writestr("BUNDLE-MANIFEST.json", json.dumps(metadata, indent=2) + "\n")
        # Exclusive creation protects an existing archive even if another process
        # created the requested output after our initial existence check.
        with temporary.open("rb") as source, destination.open("xb") as target:
            import shutil
            shutil.copyfileobj(source, target)
        return metadata
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", type=Path, default=Path.cwd())
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S%fZ")
    parser.add_argument("--out", type=Path, default=Path(f"nus-perf-results-{stamp}.zip"))
    parser.add_argument("--max-mib", type=int, default=128)
    args = parser.parse_args()
    if args.max_mib < 1:
        parser.error("--max-mib must be positive")
    try:
        manifest = collect(args.repo, args.out, args.max_mib)
    except (OSError, ValueError, RuntimeError, zipfile.BadZipFile) as error:
        print(f"ERROR: {error}", file=sys.stderr)
        return 1
    results = sum(item["kind"] == "result" for item in manifest["files"])
    print(f"ZIP: {args.out.absolute()}\nIncluded: {results} result files; "
          f"{len(manifest['files']) - results} source/context files")
    print("Raw review input only: old, failed, or pre-fix results are not certified by inclusion.")
    for warning in manifest["warnings"]:
        print(f"WARNING: {warning}")
    print("Local paths may appear in JSON. Review before sharing; do not publish this ZIP unchanged.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
