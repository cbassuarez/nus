#!/usr/bin/env python3
"""Check the curved Mercury assets through current Settings and AppKit.

Usage: bundled-python scripts/check-mercury-material.py review.app runtime-dir
The runtime directory contains exact native 256px signal reference PNGs.
"""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

from PIL import Image, ImageChops

app, reference = (Path(arg).resolve() for arg in sys.argv[1:3])
root = Path(tempfile.mkdtemp(prefix="nus-mercury-material-"))


def run(name, steps, *, reduced=False, width=1100, height=900, reuse=False):
    directory = root / name
    profile = directory / "profile"
    profile.mkdir(parents=True, exist_ok=True)
    if not reuse:
        (profile / "onboarded").write_text("skip")
        (profile / "settings.json").write_text(json.dumps({
            "window_rect": [80, 80, width, height], "theme_mode": "paper",
            "motion": {"register": 0.5, "reduce": reduced},
            "behavior": {"splash": "None", "update_checks": False,
                         "follow_os_theme": False},
        }))
    evidence = directory / ("relaunch" if reuse else "first")
    evidence.mkdir()
    shot = evidence / "check.shot"
    shot.write_text(f"wait 1800\nwindow {width} {height}\nwait 250\n" + steps + "\n")
    env = dict(os.environ, NUS_SHOT=str(shot), NUS_SHOT_DIR=str(directory),
               NUS_SHOT_OUT=str(evidence / "screens"),
               NUS_DOCK_TRACE=str(evidence / "dock"), NUS_MODE="paper")
    env.pop("NUS_SHOT2", None)
    with (evidence / "run.log").open("w") as log:
        result = subprocess.run([str(app / "Contents/MacOS/nus")], env=env,
                                stdout=log, stderr=subprocess.STDOUT, timeout=120)
    log = (evidence / "run.log").read_text()
    assert result.returncode == 0 and "panicked" not in log, log[-5000:]
    assert (profile / "mercury.json").is_file(), "Settings did not claim Mercury"
    assert json.loads((profile / "settings.json").read_text())["behavior"]["app_icon"] == "Mercury"
    print("PASS", name, "relaunch" if reuse else "", flush=True)
    return evidence


claim = """looktab 5
wait 300
settingsscroll 300
wait 250
settingseek AppIcon(Mercury)
shot icon-choices
settingclick AppIcon(Mercury)
wait 1000
mercurycheck
mercurypresentationcheck
shot reveal-a
wait 700
shot reveal-b
wait 2100
mercurycontinue
mercurystate closed
wait 500
shot selected-mercury
"""
selection = run("selection", claim + """theme nord
wait 1200
shot signal-nord
theme dracula
wait 1200
shot signal-dracula
theme broadsheet
wait 1200
shot signal-red
""")
rows = [json.loads(line) for line in (selection / "dock/events.jsonl").read_text().splitlines()]
stills = [row for row in rows if row["event"] == "mercury-still"]
assert any(row["event"].startswith("mercury-intro-") for row in rows), "Claim skipped its intro"
assert not any("mercury-liquid" in row["event"] for row in rows), "Dock retained idle animation"
pairs, colors = [], []
for color in ("c8102e", "88c0d0", "bd93f9"):
    expected = tuple(bytes.fromhex(color))
    row = next(row for row in stills if tuple(round(c * 255) for c in row["signal"][:3]) == expected)
    pairs.extend([str(selection / "dock" / f"{row['ms']}-mercury-still.tiff"),
                  str(reference / f"signal-{color}-256.png")])
    colors.append(color)
result = subprocess.run(["swift", "-module-cache-path", str(app.parent / "swift-cache"),
                         str(Path(__file__).with_name("check-icon-material.swift")), *pairs],
                        capture_output=True, text=True)
assert result.returncode == 0, result.stderr[-5000:]
readback = json.loads(result.stdout)
for comparison in readback:
    assert comparison["opaque_rgb_mean_error"] < 1
    assert comparison["opaque_rgb_p95_error"] <= 2
    assert comparison["alpha_max_error"] <= 1

profile = root / "selection/profile"
receipt = (profile / "mercury.json").read_bytes()
run("selection", "asserticon Mercury\nmercurycheck\nlooktab 5\nwait 500\nsettingsscroll 300\nwait 500\nshot persisted-mercury", reuse=True)
assert (profile / "mercury.json").read_bytes() == receipt

reduced = run("reduced", claim, reduced=True)
a = Image.open(reduced / "screens/reveal-a-paper.png").convert("RGB")
b = Image.open(reduced / "screens/reveal-b-paper.png").convert("RGB")
assert ImageChops.difference(a, b).getbbox() is None, "Reduced-motion reveal moved"
for name, width, height in (("narrow", 480, 640), ("short", 800, 420)):
    run(name, claim, reduced=True, width=width, height=height)

report = {"passed": True, "evidence": str(root), "colors": colors,
          "native_readback": readback, "settings": "claimed and selected through App icon tile",
          "persistence": "selection and receipt retained on relaunch",
          "presentation": "normal, reduced motion, narrow and short windows"}
(root / "validation.json").write_text(json.dumps(report, indent=2) + "\n")
print(json.dumps(report, indent=2), flush=True)
