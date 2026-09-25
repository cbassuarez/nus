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


def run(name, steps, prefs=None, second=None, face="paper", marker="skip", session=None):
    directory = root / name
    profile = directory / "profile"
    (profile / "layouts").mkdir(parents=True, exist_ok=True)
    if marker is not None:
        (profile / "onboarded").write_text(marker)
    if session is not None:
        (profile / "session.json").write_text(json.dumps(session))
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

# Startup and new-tab paths: no temporary shell left behind, no existing tabs
# lost. Home is never duplicated: a new tab goes back to the existing one.
for mode, kind, count in [("Prompt", "home", 1), ("HomePage", "web", 1),
                          ("LastPage", "home", 1), ("Layout", "web", 2)]:
    prefs = copy.deepcopy(base)
    prefs["behavior"].update(then=mode, then_layout="qa")
    after = count if kind == "home" else count * 2
    steps = f"assertpane {kind}\nasserttabs {count}\nnewtab\nwait 500\nassertpane {kind}\nasserttabs {after}"
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
asserttabs 3
startpage prompt
newtab
newtab
newtab
assertpane home
asserttabs 3""", base)

for mode, kind in [("prompt", "home"), ("shell", "term"), ("launch", "web")]:
    run("new-window-" + mode, f"newwindowlook {mode}\nstartpage home {url}\nnewwindow\nwait 6000",
        base, second=f"assertpane {kind}\nasserttabs 1")

legacy = copy.deepcopy(base)
legacy["behavior"].update(then="Restore", new_window="Launch")
profile = run("new-window-preserves-restore", "newwindow\nwait 6000", legacy,
              second="assertpane home\nasserttabs 1")
assert json.loads((profile / "settings.json").read_text())["behavior"]["then"] == "Restore"

# A partial welcome tour must not replace the chosen start page. Saving a
# session does not opt into restoring its tabs; an explicit layout still does.
old_session = {"tabs": [{"left": {"kind": "page", "url": url, "title": "Old page"}} for _ in range(6)],
               "active": 4, "windows": [{"tabs": [{"left": {"kind": "ports"}}]}]}
for mode, kind, count in [("Prompt", "home", 1), ("HomePage", "web", 1), ("Layout", "web", 2)]:
    prefs = copy.deepcopy(base)
    prefs["behavior"].update(then=mode, then_layout="qa", remember=True)
    name = "partial-tour-" + mode
    run(name, f"assertpane {kind}\nasserttabs {count}\nassertnoshells", prefs, marker="11010", session=old_session)
    # Same on-disk profile, including settings/session written by the first run.
    run(name, f"assertpane {kind}\nasserttabs {count}\nassertnoshells", marker=None)

fresh = run("tour-once", "assertpane welcome\nasserttabs 1\nassertprofile open\nshot first-profile\ncloseprofile\nassertprofile closed", base, marker=None)
assert (fresh / "onboarded").read_text() == "00000"
run("tour-once", "assertpane welcome\nasserttabs 1\nassertprofile open\ncloseprofile\nwelcomedismiss", marker=None)
run("tour-once", "assertpane home\nasserttabs 1\nassertprofile closed\nassertnoshells", marker=None)

# Select the actual art cards, then reopen the existing Home tab and relaunch
# the same profile. Neither selection nor relaunch may require a location.
for key, index, face, width in [("sky", 3, "paper", 1440), ("space", 2, "paper", 1440),
                                ("sky", 3, "ink", 800), ("space", 2, "ink", 480)]:
    prefs = copy.deepcopy(base)
    prefs["behavior"].update(then="Prompt", home_look="Line", place=None)
    prefs["motion"]["reduce"] = True
    prefs["window_rect"] = [100, 100, width * 2, 1800]
    name = f"background-{key}-{face}"
    steps = f"""assertpane home
asserttabs 1
settingsat 2
wait 100
settingseek HomeArt({index})
settingclick HomeArt({index})
wait 200
assertchoice HomeArt({index})
hover 400 100
wait 1200
shot art-cards
home
wait 200
asserttabs 2
asserthomeart {key}
assertplace unset
shot home
newtab
wait 100
asserthomeart {key}
asserttabs 3"""
    run(name, steps, prefs, face=face, marker="11010", session=old_session)
    run(name, f"asserttabs 1\nassertnoshells\nasserthomeart {key}\nassertplace unset\nshot home-relaunch", face=face, marker=None)

# Explicit local night uses light text in either chrome theme. No location
# uses the illustrated daytime sky checked above, with dark text.
night = copy.deepcopy(base)
night["behavior"].update(then="Prompt", home_look="Art", home_art="sky", place=[0, 0])
night["motion"]["reduce"] = True
old_clock = os.environ.get("NUS_CLOCK")
try:
    os.environ["NUS_CLOCK"] = "0"  # Midnight UTC at Greenwich.
    run("background-local-night", "asserthomeart sky\nassertplace set\nasserttabs 1\nshot night-paper\nappearance ink\nwait 150\nshot night-ink", night)
finally:
    if old_clock is None: os.environ.pop("NUS_CLOCK", None)
    else: os.environ["NUS_CLOCK"] = old_clock

for width, face in [(1440, "paper"), (800, "paper"), (480, "paper"), (1440, "ink")]:
    prefs = copy.deepcopy(base)
    prefs["window_rect"] = [100, 100, width * 2, 1800]
    steps = "settingsat 2\nwait 200\nshot top"
    for scroll in [750, 1250, 1650, 2050, 2600]:
        steps += f"\nsettingsscroll {scroll}\nwait 200\nshot scroll-{scroll}"
    run(f"visual-{width}-{face}", steps, prefs, face=face)
print("All native startup checks passed. Review screenshots for visual acceptance.")
