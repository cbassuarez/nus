#!/usr/bin/env python3
"""Capture native Home legibility cases in disposable profiles.

    python3 scripts/check-home-legibility.py /absolute/nus.app --out /tmp/home-legibility
    python3 scripts/check-home-legibility.py /absolute/nus.app --prepare-only

No build is started. The output directory must be new. These are native state
assertions and reviewable pictures, not an automatic WCAG or performance verdict.
No screenshot pixel is treated as an un-antialiased text/background contrast pair.
"""

import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import platform
import signal
import struct
import subprocess
import tempfile
import time


LOOKS = ("space", "pond", "brain", "memphis", "sky", "line", "plate")
THEMES = {"folio": "paper", "carbon": "ink", "blueprint": "ink"}
QUERY = "reference"
LONG_INPUT = "https://example.invalid/reference/reading-area?contrast=selection-and-caret"
CASES = {
    "wide": {"size": [1440, 900], "top": False, "wide": False,
             "clock": "2026-09-22T19:00:00Z",
             "purpose": "All seven looks in bright, black and saturated palettes; saved rows, scrolled results, selection"},
    "narrow": {"size": [480, 720], "top": True, "wide": True,
               "clock": "2026-09-22T19:00:00Z",
               "purpose": "All seven looks with top/wide prompt, expanded saved results and last-row visibility"},
    "night": {"size": [1440, 900], "top": False, "wide": False,
              "clock": "2026-09-23T07:00:00Z",
              "purpose": "Sky at local night with bright and saturated application palettes"},
    "private": {"size": [480, 720], "top": False, "wide": False,
                "clock": "2026-09-22T19:00:00Z", "private": True,
                "purpose": "Constrained incognito Home, wrapped privacy note and selection in three palettes"},
}
LIMITS = [
    "Native state assertions and screenshot completeness do not certify visual contrast.",
    "Review final composited text, selection, caret, saved details and route controls; do not score antialiased glyph edges.",
    "Reduced-motion stills and a fixed astronomical wall clock do not validate moving-art interference, IME, HDR, GPU time or energy.",
    "Selection is driven through real app keyboard dispatch but the existing shot API has no selection-range assertion.",
    "The PNG '-paper' suffix is the shot transport label, not the current palette; each capture records its asserted appearance.",
    "Private coverage uses the product's constrained plain Home; it does not claim that incognito permits artwork.",
]


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2) + "\n")


def preferences(case):
    saved = [f"https://example.invalid/reference-{index:02d}" for index in range(1, 13)]
    return {
        "theme_mode": "paper",
        "behavior": {
            "hatch_background": False, "hatch_status": False,
            "splash": "None", "then": "Prompt", "new_window": "Prompt",
            "follow_os_theme": False, "remember": False,
            "home_look": "Line", "home_art": "space", "sky_weather": False,
            "place": [40, -105],
            "prompt": {
                "top": case["top"], "wide": case["wide"], "compact": False,
                "home_limit": 12, "search_limit": 12, "hints": True,
                "saved": saved, "saved_preview": True, "saved_library": True,
                "saved_names": {url: f"Reference {index:02d} — readable title and detail"
                                for index, url in enumerate(saved, 1)},
                "sources": [{"source": "Saved", "home": True, "search": True, "count": 12},
                            {"source": "Web", "home": True, "search": True, "count": 3}],
            },
        },
        "motion": {"register": 0.5, "reduce": True},
    }


def shot_plan(name):
    steps = ["wait 1600", "home", "wait 300", "assertpane home", "asserttabs 1"]
    captures = []
    private = name == "private"
    if private:
        steps += ["privatecheck"]

    def capture(look, theme, state, suffix=""):
        key = f"{look}-{theme}-{state}{suffix}"
        mode = THEMES[theme]
        steps.extend([f"theme {theme}", f"homelook {look if look in ('line', 'plate') else 'art ' + look}"])
        # homelook is intentionally transient; this existing command saves it
        # before asserthomeart checks the actual renderer and persisted choice.
        if not private:
            steps.append("startpage prompt")
        steps.extend(["wait 800" if look == "plate" else "wait 350",
                      "assertpane home", f"assertappearance {mode}",
                      "key cmd+a", "key backspace"])
        if look not in ("line", "plate"):
            steps.append(f"asserthomeart {look}")
        if state == "results":
            steps.extend([f"input {QUERY}", "wait 150", "homelast", "wait 200", "homevisible"])
        elif state == "selection":
            steps.extend([f"input {LONG_INPUT}", "key cmd+a", "wait 150"])
        else:
            steps.append("wait 150")
        steps.extend(["assertpane home", "asserttabs 1", f"shot {key}"])
        captures.append({"name": key, "file": key + "-paper.png", "look": look,
                         "theme": theme, "appearance": mode, "state": state,
                         "selection_via_keyboard": state == "selection",
                         "last_row_visibility_asserted": state == "results"})

    if name == "wide":
        for look in LOOKS:
            for theme, state in (("folio", "placeholder"), ("carbon", "results"), ("blueprint", "selection")):
                capture(look, theme, state)
    elif name == "narrow":
        for index, look in enumerate(LOOKS):
            capture(look, tuple(THEMES)[index % len(THEMES)], "results")
        for look in ("space", "sky"):
            capture(look, "blueprint", "selection")
    elif name == "night":
        capture("sky", "folio", "placeholder")
        capture("sky", "blueprint", "selection")
    elif private:
        for theme in THEMES:
            capture("line", theme, "selection" if theme == "blueprint" else "placeholder")
        steps.append("privatecheck")
    else:
        raise ValueError(name)
    return steps, captures


def prepare_case(root, name):
    case = CASES[name]
    directory = root / name
    profile = directory / "profile"
    profile.mkdir(parents=True, exist_ok=False)
    (directory / "tmp").mkdir()
    (profile / "onboarded").write_text("skip")
    prefs = preferences(case)
    write_json(profile / "settings.json", prefs)
    steps, captures = shot_plan(name)
    script = directory / "check.shot"
    script.write_text("\n".join(steps) + "\n")
    return dict(case, name=name, directory=str(directory), script=str(script),
                script_sha256=sha256(script), initial_prefs_sha256=sha256(profile / "settings.json"),
                expected_screenshot_count=len(captures), captures=captures,
                state_assertion_count=sum(step.startswith(("assert", "homevisible", "privatecheck")) for step in steps),
                status="prepared-not-run", screenshots=[])


def screenshot_info(path, size):
    with path.open("rb") as stream:
        header = stream.read(24)
    if len(header) != 24 or header[:8] != b"\x89PNG\r\n\x1a\n" or header[12:16] != b"IHDR":
        raise RuntimeError(f"Invalid PNG header: {path}")
    width, height = struct.unpack(">II", header[16:24])
    scale_x, scale_y = width / size[0], height / size[1]
    if width < 400 or height < 400 or abs(scale_x - scale_y) > 0.02 or not 0.75 <= scale_x <= 4:
        raise RuntimeError(f"Capture does not match requested logical size {size}: {width}x{height}: {path}")
    return {"path": str(path), "width": width, "height": height,
            "scale": round(scale_x, 4), "bytes": path.stat().st_size, "sha256": sha256(path)}


def run_case(executable, result, timeout):
    directory = Path(result["directory"])
    env = {key: value for key, value in os.environ.items() if not key.startswith("NUS_")}
    clock_ms = round(datetime.fromisoformat(result["clock"].replace("Z", "+00:00")).timestamp() * 1000)
    env.update(NUS_SHOT=result["script"], NUS_SHOT_DIR=str(directory),
               NUS_SHOT_OUT=str(directory / "screens"), NUS_SHOT_SIZE="x".join(map(str, result["size"])),
               NUS_CLOCK=str(clock_ms), NUS_MODE="paper", TMPDIR=str(directory / "tmp"),
               TMP=str(directory / "tmp"), TEMP=str(directory / "tmp"))
    command = [str(executable), "-ApplePersistenceIgnoreState", "YES", "-NSQuitAlwaysKeepsWindows", "NO"]
    if result.get("private"):
        command.append("--incognito")
        env["NUS_PRIVATE_LOOK"] = (directory / "profile/settings.json").read_text()
    result.update(command=command, clock_ms=clock_ms, reduced_motion=True,
                  weather_network_enabled=False, visual_review="pending")
    start = time.monotonic()
    try:
        with (directory / "run.log").open("w") as log:
            # A new process group lets a timeout end only this test and its
            # children. Never pkill by application name or touch the user's app.
            process = subprocess.Popen(command, env=env, cwd=directory, stdout=log,
                                       stderr=subprocess.STDOUT, start_new_session=True)
            try:
                result["exit_code"] = process.wait(timeout=timeout)
            except subprocess.TimeoutExpired:
                try:
                    os.killpg(process.pid, signal.SIGTERM)
                    process.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGKILL)
                    process.wait()
                except ProcessLookupError:
                    pass
                raise RuntimeError(f"Native case timed out after {timeout}s; run.log retained")
        output = (directory / "run.log").read_text(errors="replace")
        errors = ("panicked", "Validation Error", "shot: unknown", "shot: no theme named", "shot: key: no ")
        if result["exit_code"] or any(error in output for error in errors):
            raise RuntimeError("Native assertion, unsupported command, process or GPU validation failure; inspect run.log")
        expected = {capture["file"] for capture in result["captures"]}
        actual = {path.name for path in (directory / "screens").glob("*.png")}
        if expected != actual:
            raise RuntimeError(f"Screenshot set mismatch; missing={sorted(expected - actual)}, extra={sorted(actual - expected)}")
        result["screenshots"] = [screenshot_info(directory / "screens" / capture["file"], result["size"])
                                 for capture in result["captures"]]
        if result.get("private"):
            marker = directory / "private-root.txt"
            if not marker.is_file():
                raise RuntimeError("Incognito native assertion did not produce its isolated-root marker")
            private_root = Path(marker.read_text().strip())
            result["private_root"] = str(private_root)
            result["private_root_removed"] = not private_root.exists()
            if not result["private_root_removed"]:
                raise RuntimeError("Incognito process did not remove its temporary profile")
        result["status"] = "captured-native-assertions-passed"
    except (RuntimeError, OSError) as error:
        result.update(status="failed", error=str(error))
    result["elapsed_s"] = round(time.monotonic() - start, 3)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("bundle", type=Path)
    parser.add_argument("--out", type=Path, help="New evidence directory; existing directories are rejected")
    parser.add_argument("--case", nargs="+", choices=CASES, dest="cases", default=list(CASES))
    parser.add_argument("--timeout", type=float, default=90, help="Maximum seconds per native launch")
    parser.add_argument("--prepare-only", action="store_true", help="Write profiles, scripts and a planned manifest without launching")
    args = parser.parse_args()
    executable = args.bundle.resolve() / "Contents/MacOS/nus"
    if not executable.is_file():
        parser.error(f"No native app executable: {executable}")
    if args.timeout <= 0:
        parser.error("--timeout must be positive")
    root = args.out.resolve() if args.out else Path(tempfile.mkdtemp(prefix="nus-home-legibility-"))
    if args.out:
        root.mkdir(parents=True, exist_ok=False)
    manifest = {
        "schema": 1, "prepared_at": datetime.now(timezone.utc).isoformat(),
        "executable": str(executable), "executable_sha256": sha256(executable),
        "harness": str(Path(__file__).resolve()), "harness_sha256": sha256(Path(__file__)),
        "platform": platform.platform(), "machine": platform.machine(),
        "visual_review_required": True, "visual_review": "pending", "limitations": LIMITS,
        "cases": [prepare_case(root, name) for name in dict.fromkeys(args.cases)],
    }
    manifest["expected_screenshot_count"] = sum(case["expected_screenshot_count"] for case in manifest["cases"])
    manifest_path = root / "manifest.json"
    write_json(manifest_path, manifest)
    print(f"Evidence: {root} ({manifest['expected_screenshot_count']} planned screenshots)", flush=True)
    if args.prepare_only:
        print("Prepared only. No native app was launched and no acceptance result is claimed.")
        return
    for case in manifest["cases"]:
        run_case(executable, case, args.timeout)
        write_json(manifest_path, manifest)
        print(f"{case['status']}: {case['name']}", flush=True)
    manifest["completed_at"] = datetime.now(timezone.utc).isoformat()
    manifest["executable_sha256_after"] = sha256(executable)
    manifest["executable_unchanged"] = manifest["executable_sha256_after"] == manifest["executable_sha256"]
    manifest["native_checks_passed"] = all(case["status"] == "captured-native-assertions-passed" for case in manifest["cases"])
    write_json(manifest_path, manifest)
    if not manifest["native_checks_passed"] or not manifest["executable_unchanged"]:
        raise SystemExit(f"Native acceptance incomplete; inspect {manifest_path}")
    print("Native state assertions passed. Visual review is still required; no pixel-contrast or performance claim.")


if __name__ == "__main__":
    main()
