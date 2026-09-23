#!/usr/bin/env python3
"""Opening changes: real native windows with isolated profiles; no user data."""
import copy
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

bundle = Path(sys.argv[1] if len(sys.argv) > 1 else 'dist/nus.app').resolve()
root = Path(tempfile.mkdtemp(prefix='nus-opening-check-'))
print(f'Evidence: {root}', flush=True)

def run(name, steps, prefs=None, fresh=False, previous=None):
    directory = root / name
    profile = directory / 'profile'
    profile.mkdir(parents=True, exist_ok=True)
    if prefs is not None:
        (profile / 'settings.json').write_text(json.dumps(prefs))
    if not fresh:
        (profile / 'onboarded').write_text('skip')
    if previous:
        (profile / 'previous-install').write_text(str(previous))
        (profile / 'onboarding-pending').write_text('1')
    script = directory / 'check.shot'
    script.write_text('wait 2700\n' + steps + '\n')
    env = dict(os.environ, NUS_SHOT_DIR=str(directory), NUS_SHOT=str(script), NUS_SHOT_OUT=str(directory / 'screens'))
    env.pop('NUS_SHOT2', None)
    env.pop('NUS_MODE', None)
    log = directory / 'run.log'
    with log.open('w') as out:
        result = subprocess.run([str(bundle / 'Contents/MacOS/nus')], env=env, stdout=out, stderr=subprocess.STDOUT, timeout=90)
    text = log.read_text()
    if result.returncode or 'panicked' in text:
        raise RuntimeError(f'{name}: {log}\n{text[-3500:]}')
    print('PASS', name, flush=True)
    return profile

first = run('first', 'assertpane welcome\nasserttabs 1\ncloseprofile\nwelcomebounds\nshot first-install', fresh=True)
assert (first / 'onboarding-pending').exists()
run('first', 'assertpane welcome\ncloseprofile\nwelcomedismiss\nassertpane home', fresh=True)
assert not (first / 'onboarding-pending').exists()
run('first', 'assertpane home\nasserttabs 1\nassertnoshells', fresh=True)
base = json.loads((first / 'settings.json').read_text())
base['behavior'].update(splash='None', then='Prompt', atlas='Planet', window_start='Last')
base['sidebar_pinned'] = True
base['window_rect'] = [80, 80, 2000, 1500]

old = root / 'old-profile'
old.mkdir()
old_prefs = copy.deepcopy(base)
old_prefs['sidebar']['pin_display'] = 'Preview'
(old / 'settings.json').write_text(json.dumps(old_prefs))
returned = run('return-defaults', 'assertpane welcome\nassertprofile open\ncloseprofile\nwelcomebounds\nshot redownload\nwelcomedismiss\nassertpane home', base, fresh=True, previous=old)
assert json.loads((returned / 'settings.json').read_text())['sidebar']['pin_display'] == 'Icon'
run('return-defaults', 'assertpane home\nasserttabs 1', fresh=True)
imported = run('return-import', 'assertpane welcome\nassertprofile open\ncloseprofile\nwelcomeclick ImportSettings\nwait 200\nwelcomedismiss', base, fresh=True, previous=old)
assert json.loads((imported / 'settings.json').read_text())['sidebar']['pin_display'] == 'Preview'

narrow = copy.deepcopy(base)
narrow['window_rect'] = [80, 80, 860, 1500]
run('return-narrow', 'assertpane welcome\nassertprofile open\ncloseprofile\nwelcomebounds\nshot narrow-top\nwelcomescroll 360\nwait 200\nwelcomebounds\nshot narrow-pins', narrow, fresh=True, previous=old)

palette = copy.deepcopy(base)
palette['behavior']['then'] = 'Palette'
run('palette', 'assertpalette open\nasserttabs 1\nkey Escape\nassertpalette closed\nkey cmd+t\nassertpalette open\nasserttabs 1\nkey Escape\nkey cmd+k\nassertpalette open\nasserttabs 1\nshot palette\nkey Escape\nsettingsat 2\nwait 200\nsettingseek Then(Palette)\nwait 100\nsettingclick Then(Palette)\nassertchoice Then(Palette)', palette)

for mode in ['Icon', 'Preview']:
    prefs = copy.deepcopy(base)
    prefs['sidebar']['pin_display'] = mode
    prefs['surface']['shell_radius'] = 12.0
    run('pins-' + mode, 'sidebarwidth 280\nwait 150\npinsbounds\npindrag 0 1\nwait 200\npinsassert Reading list|Welcome|Downloads|Ports\nshot tiles\npinclick Open(1)\nwait 200\nassertpane welcome\nsidebarwidth 80\nwait 200\npinsbounds\nshot compact', prefs)

page = root / 'preview.html'
page.write_text('<title>Preview check</title><body style="background:#1d5bdb;color:white;font:40px sans-serif">Live preview<script>setInterval(()=>document.body.style.background = document.body.style.background === "rgb(29, 91, 219)" ? "#af2862" : "#1d5bdb",500)</script>')
web = copy.deepcopy(base)
web['sidebar']['pin_display'] = 'Preview'
web['pinned_tabs'].append(dict(id='qa-web', title='Live preview', target={'Page': {'url': page.as_uri(), 'container': 'PERSONAL'}}))
run('web-preview', 'sidebarwidth 280\nwait 150\nasserttabs 1\npinclick Open(4)\nwait 1800\nassertpane web\npinsbounds\nshot web-preview\npinclick Open(0)\nwait 600\nshot background-preview', web)
for face in ['paper', 'ink']:
    settings = copy.deepcopy(base)
    settings['behavior']['follow_os_theme'] = False
    settings['theme_mode'] = face
    settings['motion']['reduce'] = False
    run('settings-' + face, 'settingsat 3\nwait 200\nsettingseek PinDisplay(Preview)\nwait 100\nsettingclick PinDisplay(Preview)\nwait 180\nassertchoice PinDisplay(Preview)\nsettinghover PinDisplay(Icon)\nwait 250\nshot pin-display-hover\nsettingsscroll 0\nwait 200\nsettinghover HdrStyle(Rail)\nwait 250\nshot hover-before\nsettingsscroll 5\nwait 250\nshot hover-scrolled\nsettingsat 2\nwait 200\nsettingseek Then(Palette)\nwait 150\nsettingclick Then(Palette)\nwait 150\nassertchoice Then(Palette)\nshot start-new-tab', settings)
print('Opening checks passed. Review screenshots for visual acceptance.', flush=True)
