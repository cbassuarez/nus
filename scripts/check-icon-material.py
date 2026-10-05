#!/usr/bin/env python3
"""Native settings selection/persistence and actual Dock pixel readback."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

app, reference = Path(sys.argv[1]).resolve(), Path(sys.argv[2]).resolve()
root = Path(tempfile.mkdtemp(prefix="nus-icon-material-"))
directory = root / "state"
profile = directory / "profile"
profile.mkdir(parents=True)
(profile / "onboarded").write_text("skip")
(profile / "settings.json").write_text(json.dumps({"behavior": {"splash": "None", "then": "Prompt", "update_checks": False, "follow_os_theme": False}, "motion": {"register": 0.5, "reduce": True}, "theme_mode": "paper", "window_rect": [80, 80, 1200, 900]}))
faces = ["Plex", "Silkscreen", "Plex Italic", "Bungee", "Rubik Mono", "Newsreader"]
slugs = ["plex", "silkscreen", "plex-italic", "bungee", "rubik", "newsreader"]


def run(name, steps):
    evidence = root / name
    evidence.mkdir()
    shot = evidence / "check.shot"
    shot.write_text("wait 1800\n" + steps + "\n")
    env = dict(os.environ, NUS_SHOT=str(shot), NUS_SHOT_DIR=str(directory), NUS_SHOT_OUT=str(evidence / "screens"), NUS_DOCK_TRACE=str(evidence / "dock"), NUS_SHOT_INTERACTIVE="1", NUS_MODE="paper")
    env.pop("NUS_SHOT2", None)
    with (evidence / "run.log").open("w") as log:
        result = subprocess.run([str(app / "Contents/MacOS/nus")], env=env, stdout=log, stderr=subprocess.STDOUT, timeout=90)
    text = (evidence / "run.log").read_text()
    assert result.returncode == 0 and "panicked" not in text, text[-4000:]
    return evidence


steps = ["looktab 5", "wait 700", "settingsscroll 300", "wait 200", "shot regular-choices"]
for face, slug in zip(faces, slugs):
    steps += [f"iconchoose {face}", "wait 450", f"asserticon {face}", f"shot chosen-{slug}"]
for family in ("nord", "dracula", "broadsheet"):
    steps += [f"theme {family}", "wait 1100", f"shot signal-{family}"]
steps += ["iconchoose Plex", "wait 500", "asserticon Plex"]
evidence = run("selection", "\n".join(steps))
rows = [json.loads(line) for line in (evidence / "dock/events.jsonl").read_text().splitlines()]
frames = [row for row in rows if row["event"] == "frame"]
seen = {row["face"] for row in frames}
assert set(faces) <= seen, seen
colors = {tuple(round(c * 255) for c in row["signal"][:3]) for row in frames}
assert {(200, 16, 46), (136, 192, 208), (189, 147, 249)} <= colors, colors
comparisons = []
image_pairs = []
for face, slug in zip(faces, slugs):
    row = next(row for row in frames if row["face"] == face and tuple(round(c * 255) for c in row["signal"][:3]) == (200, 16, 46))
    path = evidence / "dock" / f"{row['ms']}-{faces.index(face)}.tiff"
    image_pairs += [str(path), str(reference / f"{slug}-256.png")]
# AppKit's TIFF includes the monitor profile. Normalize both images through
# AppKit to sRGB; comparing untagged PNG bytes to display-space RGB is invalid.
decoded = subprocess.run(["swift", "-module-cache-path", str(app.parent / "swift-cache"), str(Path(__file__).with_suffix(".swift")), *image_pairs], check=True, capture_output=True, text=True)
for face, result in zip(faces, json.loads(decoded.stdout)):
    assert result["opaque_rgb_mean_error"] < 1 and result["opaque_rgb_p95_error"] <= 2 and result["alpha_max_error"] <= 1, (face, result)
    comparisons.append({"face": face, **result})
assert json.loads((profile / "settings.json").read_text())["behavior"]["app_icon"] == "Plex"
relaunch = run("relaunch", "asserticon Plex\nlooktab 5\nwait 800\nsettingsscroll 300\nwait 200\nshot persisted-plex\nappearance ink\nwait 700\nasserticon Plex\nshot regular-ink")
assert not (profile / "mercury.json").exists(), "Regular icon selection claimed Mercury"
summary = {"passed": True, "faces": faces, "colors": sorted(colors), "readback": comparisons, "persistence": "Plex retained on relaunch and appearance change", "evidence": str(root)}
(root / "validation.json").write_text(json.dumps(summary, indent=2) + "\n")
print(json.dumps(summary, indent=2), flush=True)
