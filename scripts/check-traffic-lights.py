#!/usr/bin/env python3
"""Check real AppKit hit testing and unchanged shell geometry in disposable profiles.

Run after building/bundling the composite app. These checks use NSView hitTest,
not the GPU screenshot's old synthetic traffic-light rectangles. Actual hover,
Option-click and window-manager menus still need a macOS UI check.
"""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

bundle = Path(sys.argv[1] if len(sys.argv) > 1 else "dist/nus.app").resolve()
root = Path(tempfile.mkdtemp(prefix="nus-traffic-check-"))
print(f"Evidence: {root}", flush=True)


def run(name, steps, prefs=None, size="1200x800"):
    directory = root / name
    profile = directory / "profile"
    profile.mkdir(parents=True)
    (profile / "onboarded").write_text("skip")
    if prefs:
        (profile / "settings.json").write_text(json.dumps(prefs))
    script = directory / "check.shot"
    script.write_text("wait 1600\nhover 600 400\nwait 300\n" + steps + "\n")
    env = dict(os.environ, NUS_SHOT_DIR=str(directory), NUS_SHOT=str(script),
               NUS_SHOT_OUT=str(directory / "screens"), NUS_SHOT_SIZE=size,
               NUS_MODE="paper")
    env.pop("NUS_SHOT2", None)
    log = directory / "run.log"
    with log.open("w") as out:
        result = subprocess.run([str(bundle / "Contents/MacOS/nus")], env=env,
                                stdout=out, stderr=subprocess.STDOUT, timeout=60)
    text = log.read_text()
    if result.returncode or "panicked" in text or "TRAFFIC CHECK PASSED" not in text:
        raise RuntimeError(f"{name}: inspect {log}\n{text[-4000:]}")
    print(f"PASS {name}", flush=True)
    return json.loads((profile / "settings.json").read_text())


base = run("wide", "trafficcheck\nshot wide")
base["behavior"].update(splash="None", hatch_background=False, hatch_status=False)
base["motion"]["reduce"] = True
run("narrow", "trafficcheck\nshot narrow", base, "700x650")
run("resize-theme-fullscreen", """trafficcheck
trafficsize 700 650
wait 1000
trafficcheck
appearance ink
wait 300
trafficcheck
trafficfullscreen
wait 2000
trafficcheck
trafficfullscreen
wait 2000
trafficcheck
trafficsize 1200 800
wait 1000
trafficcheck""", base)
run("hidden-header", """key ctrl+shift+F11
wait 300
trafficcheck
key ctrl+shift+F11
compact
hover 500 400
wait 1400
trafficcheck
hover 100 20
wait 300
trafficcheck
compact
wait 300
trafficcheck""", base)
base["surface"].update(shell="Stroke", shell_width=12.0, shell_radius=24.0)
run("custom-shell", "trafficcheck\nshot custom-shell", base)
print("Native traffic-light checks passed.", flush=True)
