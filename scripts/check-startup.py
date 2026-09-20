#!/usr/bin/env python3
"""Native startup regressions. Run after scripts/bundle-mac.sh --debug.

Usage: python3 scripts/check-startup.py [path/to/nus.app]
Uses temporary profiles, local HTML, real windows, and the app's screenshot driver.
Screenshots and logs remain in the printed temporary directory for visual review.
"""
import copy
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

bundle = Path(sys.argv[1] if len(sys.argv) > 1 else "dist/nus.app").resolve()
exe = bundle / "Contents/MacOS/nus"
root = Path(tempfile.mkdtemp(prefix="nus-startup-check-"))
print(f"Evidence: {root}", flush=True)
page = root / "home.html"
page.write_text("<title>Startup home check</title><h1>Home page</h1><p>The selected startup address opened.</p>")
url = page.as_uri()


def run(name, steps, prefs=None, second=None, face="paper"):
    directory = root / name
    profile = directory / "profile"
    (profile / "layouts").mkdir(parents=True)
    (profile / "onboarded").write_text("skip")
    (profile / "layouts/qa.nus.luau").write_text(
        'return {tabs={{page=' + json.dumps(url) + '},{page="about:blank"}}}')
    if prefs is not None:
        (profile / "settings.json").write_text(json.dumps(prefs))
    script = directory / "check.shot"
    script.write_text("wait 2500\n" + steps + "\n")
    env = dict(os.environ, NUS_SHOT_DIR=str(directory), NUS_SHOT=str(script),
               NUS_SHOT_OUT=str(directory / "screens"), NUS_MODE=face)
    env.pop("NUS_SHOT2", None)
    if second:
        script2 = directory / "second.shot"
        script2.write_text("wait 2500\n" + second + "\n")
        env["NUS_SHOT2"] = str(script2)
    log = directory / "run.log"
    with log.open("w") as output:
        result = subprocess.run([str(exe)], env=env, stdout=output, stderr=subprocess.STDOUT, timeout=60)
    text = log.read_text()
    if result.returncode or "panicked" in text or (second and f"shot: {second.splitlines()[-1]}" not in text):
        raise RuntimeError(f"{name} failed; inspect {log}\n{text[-2500:]}")
    print(f"PASS {name}", flush=True)
    return profile


profile = run("bootstrap", "assertpane home\nasserttabs 1\nstartpage prompt")
base = json.loads((profile / "settings.json").read_text())
base["behavior"].update(splash="None", atlas="Planet", home_url=url, window_start="Last")
base["window_rect"] = [100, 100, 2880, 1800]

# Startup and new-tab paths: no temporary shell left behind, no existing tabs lost.
for mode, kind, count in [("Prompt", "home", 1), ("HomePage", "web", 1),
                          ("LastPage", "home", 1), ("Layout", "web", 2)]:
    prefs = copy.deepcopy(base)
    prefs["behavior"].update(then=mode, then_layout="qa")
    steps = f"assertpane {kind}\nasserttabs {count}\nnewtab\nwait 500\nassertpane {kind}\nasserttabs {count * 2}"
    if mode == "HomePage":
        steps += f"\nasserturl {url}"
    run("destination-" + mode, steps, prefs)

run("live-changes", f"""startpage home {url}
newtab
wait 800
assertpane web
asserturl {url}
asserttabs 2
startpage last
newtab
wait 800
assertpane web
asserturl {url}
asserttabs 3
startpage layout missing
newtab
assertpane home
asserttabs 4
startpage prompt
newtab
assertpane home
asserttabs 5""", base)

for mode, kind in [("prompt", "home"), ("shell", "term"), ("launch", "web")]:
    run("new-window-" + mode, f"newwindowlook {mode}\nstartpage home {url}\nnewwindow\nwait 6000",
        base, second=f"assertpane {kind}\nasserttabs 1")

legacy = copy.deepcopy(base)
legacy["behavior"].update(then="Restore", new_window="Launch")
profile = run("new-window-preserves-restore", "newwindow\nwait 6000", legacy,
              second="assertpane home\nasserttabs 1")
assert json.loads((profile / "settings.json").read_text())["behavior"]["then"] == "Restore"

for width, face in [(1440, "paper"), (800, "paper"), (480, "paper"), (1440, "ink")]:
    prefs = copy.deepcopy(base)
    prefs["window_rect"] = [100, 100, width * 2, 1800]
    steps = "settingsat 2\nwait 200\nshot top"
    for scroll in [750, 1250, 1650, 2050, 2600]:
        steps += f"\nsettingsscroll {scroll}\nwait 200\nshot scroll-{scroll}"
    run(f"visual-{width}-{face}", steps, prefs, face=face)
print("All native startup checks passed. Review screenshots for visual acceptance.")
