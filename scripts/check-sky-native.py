#!/usr/bin/env python3
"""Capture six native Sky cases in disposable profiles with retained provenance.

    python3 scripts/check-sky-native.py /absolute/nus.app --out /tmp/sky-check
    python3 scripts/check-sky-native.py /absolute/nus.app --case sunrise night

Requires an optimized macOS bundle containing c:atmosphere. No build is started.
The output directory must be new. Fixed wall clocks and reduced motion make the
visual cases repeatable; these runs are not animation, GPU, or energy benchmarks.
Overcast/rain override only their isolated profile's sky.luau. They exercise the
native primitive, not provider parsing or real-world weather accuracy. Weather
network consent is disabled in every case. Screenshots still need visual review.
For separate real-clock motion samples, use scripts/perf-home.py --art sky.
"""

import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import platform
import struct
import subprocess
import tempfile
import time


CASES = {
    "illustrated": {"clock": "2026-09-22T19:00:00Z", "place": None, "logical_size": [480, 720],
                    "purpose": "Built-in, no location; narrow paper, ink, and typed prompt"},
    "sunrise": {"clock": "2026-09-22T13:00:00Z", "place": [40, -105],
                "purpose": "Built-in, Colorado near sunrise; sun roughly 1.5 degrees high"},
    "sunset": {"clock": "2026-09-23T00:55:00Z", "place": [40, -105],
               "purpose": "Built-in, Colorado near sunset; sun roughly 0.5 degrees below horizon"},
    "night": {"clock": "2026-09-23T07:00:00Z", "place": [40, -105],
              "purpose": "Built-in local night, paper and ink; sun roughly 50 degrees below horizon"},
    "overcast": {"clock": "2026-09-22T19:00:00Z", "place": None,
                 "purpose": "Explicit native atmosphere fixture, unbroken layered cloud",
                 "fixture": {"low": 0.92, "middle": 0.88, "high": 0.65,
                             "stratus": 0.95, "precipitation": 0, "base": 0.8, "haze": 0.65}},
    "rain": {"clock": "2026-09-22T19:00:00Z", "place": None,
             "purpose": "Explicit native atmosphere fixture, dense low cloud and rain",
             "fixture": {"low": 0.98, "middle": 0.88, "high": 0.76,
                         "stratus": 0.9, "precipitation": 8, "base": 0.5, "haze": 0.9}},
}


def sha256(path):
    h = hashlib.sha256()
    with path.open("rb") as file:
        for block in iter(lambda: file.read(1024 * 1024), b""):
            h.update(block)
    return h.hexdigest()


def fixture_source(values):
    weather = ", ".join(f"{key}={value}" for key, value in values.items())
    return """-- name: native Sky acceptance fixture
-- says: explicit renderer parameters, not observed weather
function draw(c)
  c:backdrop("light")
  c:atmosphere({sun={0.48,0.42,-0.77}, moon={0,-1,0}, moon_light=0,
    wind_low={4,2}, wind_middle={9,-1}, wind_high={15,5}, seed=42,
    bearing=3.14159265, elevation=0.45, fov=0.9, prompt_light=0.14,
    """ + weather + """})
end
"""


def screenshot_info(path):
    with path.open("rb") as file:
        header = file.read(24)
    if len(header) != 24 or header[:8] != b"\x89PNG\r\n\x1a\n" or header[12:16] != b"IHDR":
        raise RuntimeError(f"Invalid screenshot: {path}")
    width, height = struct.unpack(">II", header[16:24])
    if width < 400 or height < 400:
        raise RuntimeError(f"Unexpected screenshot size: {path}: {width}x{height}")
    return {"path": str(path), "width": width, "height": height,
            "bytes": path.stat().st_size, "sha256": sha256(path)}


def run_case(executable, root, name, case, timeout):
    directory = root / name
    profile = directory / "profile"
    profile.mkdir(parents=True)  # Never reuse a profile, even after a failed run.
    (profile / "onboarded").write_text("skip")
    prefs = {
        "behavior": {"hatch_background": False, "hatch_status": False,
                     "splash": "None", "then": "Prompt", "home_look": "Art",
                     "home_art": "sky", "place": case["place"],
                     "sky_weather": False, "follow_os_theme": False},
        "theme_mode": "paper", "motion": {"register": 0.5, "reduce": True},
    }
    (profile / "settings.json").write_text(json.dumps(prefs, indent=2))
    fixture = case.get("fixture")
    if fixture:
        art = profile / "art"
        art.mkdir()
        (art / "sky.luau").write_text(fixture_source(fixture))
    steps = ["wait 1600", "home", "wait 300", "assertappearance paper", "asserthomeart sky",
             "assertplace " + ("set" if case["place"] else "unset"),
             "asserttabs 1", "shot sky"]
    expected = 1
    if name in ("illustrated", "night"):
        steps += ["appearance ink", "wait 200", "assertappearance ink", "asserthomeart sky", "shot sky-ink"]
        expected += 1
    if name == "illustrated":
        steps += ["hometype A quiet place to think",
                  "wait 150", "asserthomeart sky", "shot narrow-typed"]
        expected += 1
    shot = directory / "check.shot"
    shot.write_text("\n".join(steps) + "\n")
    (directory / "tmp").mkdir()
    env = {key: value for key, value in os.environ.items() if not key.startswith("NUS_")}
    clock_ms = round(datetime.fromisoformat(case["clock"].replace("Z", "+00:00")).timestamp() * 1000)
    logical_size = case.get("logical_size", [1440, 900])
    env.update(NUS_SHOT=str(shot), NUS_SHOT_DIR=str(directory),
               NUS_SHOT_OUT=str(directory / "screens"), NUS_SHOT_SIZE="x".join(map(str, logical_size)),
               NUS_CLOCK=str(clock_ms), NUS_MODE="paper", TMPDIR=str(directory / "tmp"),
               TMP=str(directory / "tmp"), TEMP=str(directory / "tmp"))
    result = dict(case, clock_ms=clock_ms, directory=str(directory), reduced_motion=True,
                  logical_size=logical_size,
                  weather_network_enabled=False, route="fixture" if fixture else "built-in")
    if fixture:
        result["fixture_sha256"] = sha256(profile / "art/sky.luau")
    started = time.monotonic()
    try:
        with (directory / "run.log").open("w") as log:
            process = subprocess.run(
                [str(executable), "-ApplePersistenceIgnoreState", "YES", "-NSQuitAlwaysKeepsWindows", "NO"],
                env=env, stdout=log, stderr=subprocess.STDOUT, timeout=timeout)
        result["exit_code"] = process.returncode
        output = (directory / "run.log").read_text(errors="replace")
        if process.returncode or "panicked" in output or "Validation Error" in output:
            raise RuntimeError("Native assertion, process, or GPU validation failure; inspect run.log")
        screens = sorted((directory / "screens").glob("*.png"))
        if len(screens) != expected:
            raise RuntimeError(f"Expected {expected} screenshots, found {len(screens)}")
        result["screenshots"] = [screenshot_info(path) for path in screens]
        for screen in result["screenshots"]:
            # An ignored native resize must not be reported as narrow coverage.
            if abs(screen["width"] / screen["height"] - logical_size[0] / logical_size[1]) > 0.01:
                raise RuntimeError(f"Screenshot aspect ratio differs from requested {logical_size}: {screen['path']}")
        result["status"] = "captured-native-assertions-passed"
    except (RuntimeError, OSError, subprocess.TimeoutExpired) as error:
        result.update(status="failed", error=str(error))
    result["elapsed_s"] = round(time.monotonic() - started, 3)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("bundle", type=Path)
    parser.add_argument("--out", type=Path, help="New evidence directory; an existing directory is rejected")
    parser.add_argument("--case", nargs="+", choices=CASES, dest="cases", default=list(CASES))
    parser.add_argument("--timeout", type=float, default=45, help="Maximum seconds per native case (default: 45)")
    args = parser.parse_args()
    executable = args.bundle.resolve() / "Contents/MacOS/nus"
    if not executable.is_file():
        parser.error(f"No native app executable: {executable}")
    if args.timeout <= 0:
        parser.error("--timeout must be positive")
    if args.out:
        root = args.out.resolve()
        root.mkdir(parents=True, exist_ok=False)
    else:
        root = Path(tempfile.mkdtemp(prefix="nus-sky-native-"))
    try:
        power = subprocess.run(["pmset", "-g", "batt"], capture_output=True, text=True, timeout=5).stdout
    except (OSError, subprocess.TimeoutExpired) as error:
        power = f"Unavailable: {error}"
    manifest = {"schema": 1, "captured_at": datetime.now(timezone.utc).isoformat(),
                "executable": str(executable), "executable_sha256": sha256(executable),
                "platform": platform.platform(), "machine": platform.machine(), "power": power,
                "logical_size": [1440, 900], "visual_review_required": True,
                "measurement_limits": "Fixed wall-clock/reduced-motion visual checks. Not animation, GPU time, or battery evidence. Fixture weather is illustrative.",
                "cases": []}
    manifest_path = root / "manifest.json"
    manifest_path.write_text(json.dumps(manifest, indent=2))
    print(f"Evidence: {root}", flush=True)
    for name in dict.fromkeys(args.cases):
        result = run_case(executable, root, name, CASES[name], args.timeout)
        manifest["cases"].append(result)
        manifest_path.write_text(json.dumps(manifest, indent=2))
        print(f"{result['status']}: {name}", flush=True)
    manifest["executable_sha256_after"] = sha256(executable)
    manifest["executable_unchanged"] = manifest["executable_sha256_after"] == manifest["executable_sha256"]
    manifest_path.write_text(json.dumps(manifest, indent=2))
    failed = any(case["status"] == "failed" for case in manifest["cases"])
    if failed or not manifest["executable_unchanged"]:
        raise SystemExit(f"Native Sky check incomplete; inspect {manifest_path}")
    print("Native assertions passed; review the captured images for visual acceptance.", flush=True)


if __name__ == "__main__":
    main()
