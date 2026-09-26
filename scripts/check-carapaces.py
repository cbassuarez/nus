#!/usr/bin/env python3
"""Native material settings, activity, grain, and relaunch checks.

Run with the bundled Python (Pillow) and a freshly built isolated .app.
Settings use the app's real hit targets and click handlers. Screenshots are
GPU readbacks; deterministic source fixtures supplement a real shell job.
"""
import copy
import json
import os
import re
from pathlib import Path
import subprocess
import sys
import tempfile

from PIL import Image, ImageChops


bundle = Path(sys.argv[1] if len(sys.argv) > 1 else '/tmp/nus-carapace-review.app').resolve()
root = Path(sys.argv[2]).resolve() if len(sys.argv) > 2 else Path(tempfile.mkdtemp(prefix='nus-carapaces-'))
root.mkdir(parents=True, exist_ok=True)
print(f'Evidence: {root}', flush=True)
materials = ['Plain', 'InkPool', 'Enamel', 'Interference', 'SingleSeam', 'OpenCorners', 'Overprint', 'EdgeLight']
sources = ['Work', 'Loading', 'Completion', 'Attention', 'Typing', 'Media']
runs = {}


def run(name, steps, prefs=None, *, face='paper', width=1200, height=900, reuse=False):
    directory = root / name
    profile = directory / 'profile'
    profile.mkdir(parents=True, exist_ok=True)
    if not reuse:
        assert not (profile / 'settings.json').exists(), f'Refusing to overwrite existing profile: {profile}'
        (profile / 'onboarded').write_text('skip')
        (profile / 'settings.json').write_text(json.dumps(prefs or {
            'window_rect': [70, 70, width, height],
            'motion': {'register': 0.5, 'reduce': False},
            'behavior': {'splash': 'None', 'update_checks': False, 'then': 'Prompt'},
        }))
    else:
        assert (profile / 'settings.json').exists(), f'No profile to relaunch: {profile}'
    count = runs.get(name, 0) + 1
    runs[name] = count
    script = directory / f'check-{count}.shot'
    script.write_text(f'wait 1600\nwindow {width} {height}\nwait 200\n' + steps + '\n')
    env = dict(os.environ, NUS_SHOT_DIR=str(directory), NUS_SHOT=str(script),
               NUS_SHOT_OUT=str(directory / 'screens'), NUS_MODE=face, NUS_SHOT_SIZE=f'{width}x{height}')
    env.pop('NUS_SHOT2', None)
    log_path = directory / f'run-{count}.log'
    with log_path.open('w') as output:
        result = subprocess.run([str(bundle / 'Contents/MacOS/nus')], env=env,
                                stdout=output, stderr=subprocess.STDOUT, timeout=150)
    log = log_path.read_text()
    assert result.returncode == 0 and 'panicked' not in log, f'{name}: {log_path}\n{log[-6000:]}'
    for line in steps.splitlines():
        if line.startswith('shot '):
            path = directory / 'screens' / f'{line.split()[1]}-{face}.png'
            with Image.open(path) as image:
                assert image.width > 100 and image.height > 100, f'Empty screenshot: {path}'
                size = re.search(re.escape(str(path)) + r' \((\d+)×(\d+) px at ([\d.]+)×\)', log)
                assert size and image.width == round(width * float(size[3])), f'Wrong logical capture width: {path}'
    print('PASS', name, 'relaunch' if reuse else '', flush=True)
    return json.loads((profile / 'settings.json').read_text())


def click(hit, *, selected=True):
    # Opening the tab resets its scroll before seeking downward. Every
    # selection is routed through the same mouse handler as a user click.
    steps = ['looktab 1', 'wait 150', f'settingseek {hit}', f'settingclick {hit}', 'wait 150']
    if selected:
        steps.append(f'assertchoice {hit}')
    return '\n'.join(steps) + '\n'


def source_click(source, enabled):
    # A source chip's hit is its NEXT value, so its selected state has the
    # opposite bool after enabling it. Disabled values are checked in saved
    # preferences and by the quiet-source run below.
    steps = click(f'ReactTo({source}, {str(enabled).lower()})', selected=False)
    if enabled:
        steps += f'assertchoice ReactTo({source}, false)\n'
    return steps


def same_edge(name, a, b, face='paper'):
    directory = root / name / 'screens'
    with Image.open(directory / f'{a}-{face}.png') as first, Image.open(directory / f'{b}-{face}.png') as second:
        first, second = first.convert('RGB'), second.convert('RGB')
        assert first.size == second.size
        w, h = first.size
        # Ignore pane content/caret clocks; inspect only the outer material.
        n = max(8, round(w / 1200 * 12))
        strips = [(0, 0, w, n), (0, h - n, w, h), (0, 0, n, h), (w - n, 0, w, h)]
        return all(ImageChops.difference(first.crop(r), second.crop(r)).getbbox() is None for r in strips)


try:
    base = run('bootstrap', click('Material(InkPool)') + click('Reaction(Expressive)')
               + click('TexKind(Grain)') + click('TexOn(Carapace)')
               + 'home\ncarapacefixture idle\nwait 100\ncarapacefixture typing\nwait 100\ncarapacestate quiet\n'
                 'carapacefixture media\nwait 100\ncarapacestate quiet\nshot defaults')
    assert base['surface']['react_to'] == {
        'work': True, 'loading': True, 'completion': True, 'attention': True, 'typing': False, 'media': False,
    }, 'Typing and media must remain opt-in on a new profile'
    base['surface'].update(texture_kind='Grain', texture_on='Carapace', texture=0.08, texture_motion=False,
                           shell='Stroke', shell_width=12.0, shell_radius=14.0)
    base['motion']['reduce'] = False
    base['behavior'].update(splash='None', update_checks=False, then='Prompt')

    steps = click('Shell(Band)') + 'shot band\n' + click('Shell(Stroke)')
    steps += ''.join(click(f'Material({material})') for material in materials)
    steps += ''.join(click(f'Reaction({reaction})') for reaction in ['Still', 'Subtle', 'Expressive'])
    for source in sources:
        if source not in ['Typing', 'Media']:
            steps += source_click(source, False)
        steps += source_click(source, True)
        if source in ['Typing', 'Media']:
            steps += source_click(source, False) + source_click(source, True)
    steps += click('Material(InkPool)') + click('TexKind(Grain)') + 'shot selected'
    saved = run('native-clicks', steps, base)
    assert saved['surface']['material'] == 'InkPool'
    assert saved['surface']['reaction'] == 'Expressive'
    assert saved['surface']['texture_kind'] == 'Grain' and saved['surface']['texture'] > 0
    assert all(saved['surface']['react_to'][source.lower()] for source in sources)
    checks = ['looktab 1', 'wait 200', 'assertchoice Material(InkPool)', 'assertchoice Reaction(Expressive)',
              'assertchoice TexKind(Grain)', 'assertchoice TexOn(Carapace)']
    checks += [f'assertchoice ReactTo({source}, false)' for source in sources]
    checks += ['shot persisted', 'home', 'carapacefixture idle', 'wait 100', 'carapacestate quiet']
    relaunched = run('native-clicks', '\n'.join(checks), reuse=True)
    for field in ['material', 'reaction', 'react_to', 'texture_kind', 'texture_on', 'texture']:
        assert relaunched['surface'][field] == saved['surface'][field], f'Relaunch lost {field}'

    enabled = copy.deepcopy(saved)
    steps = 'home\ncarapacefixture idle\nwait 100\ncarapacestate quiet\n'
    for source in ['work', 'output', 'typing', 'completion', 'loading', 'media']:
        steps += f'carapacefixture {source}\nwait 180\ncarapacestate active\n'
        if source == 'loading':
            steps += 'carapacestate progress:0.42\n'
        steps += f'shot {source}\ncarapacefixture idle\nwait 4500\ncarapacestate quiet\n'
    steps += ('carapacefixture work\nwait 700\nshot active-a\nwait 500\nshot active-b\n'
              'carapacefixture idle\nwait 4000\ncarapacefixture attention\nwait 1800\n'
              'carapacestate held\nshot held-a\nwait 500\nshot held-b\n')
    run('sources-enabled', steps, enabled)
    assert not same_edge('sources-enabled', 'active-a', 'active-b'), 'Active ink pool did not visibly move'
    assert same_edge('sources-enabled', 'held-a', 'held-b'), 'Held attention kept moving after settling'

    disabled = copy.deepcopy(enabled)
    disabled['surface']['react_to'] = {source.lower(): False for source in sources}
    steps = 'home\ncarapacefixture idle\nwait 100\n'
    for source in ['work', 'output', 'typing', 'completion', 'attention', 'loading', 'media']:
        steps += f'carapacefixture {source}\nwait 180\ncarapacestate quiet\n'
    run('sources-disabled', steps + 'shot quiet', disabled)

    still = copy.deepcopy(enabled)
    still['surface']['reaction'] = 'Still'
    run('still', 'home\ncarapacefixture idle\nwait 100\ncarapacefixture work\nwait 500\n'
        'carapacestate quiet\nshot still-a\nwait 500\nshot still-b', still)
    assert same_edge('still', 'still-a', 'still-b'), 'Still changed the material over time'
    reduced = copy.deepcopy(enabled)
    reduced['motion']['reduce'] = True
    run('reduced', 'home\ncarapacefixture idle\nwait 100\ncarapacefixture work\nwait 500\n'
        'carapacestate reduced\nshot reduced-a\nwait 500\ncarapacestate reduced\nshot reduced-b', reduced)
    assert same_edge('reduced', 'reduced-a', 'reduced-b'), 'Reduced motion changed the material over time'

    real = copy.deepcopy(base)
    real['surface']['react_to']['attention'] = False
    run('real-shell', 'newshell\nwait 1200\nassertpane term\nkey q\nwait 250\ncarapacestate quiet\n'
        'key backspace\nline echo quiet\nwait 300\ncarapacestate quiet\nshot typing-off\nerase 10\n'
        'shell sleep 5\nwait 3100\n'
        'carapacestate active\nshot shell-working\nwait 6500\ncarapacestate quiet\nshot shell-settled', real)

    # The same seven materials and grain render at both sizes and faces.
    for width in [1200, 480]:
        for face in ['paper', 'ink']:
            steps = ''
            for material in materials:
                steps += click(f'Material({material})') + 'settingsbounds\n'
                steps += f'shot settings-{material.lower()}\nhome\nwait 200\nshot frame-{material.lower()}\n'
            steps += click('Reaction(Still)') + 'settingseek ReactTo(Media, false)\nwait 150\nsettingsbounds\nshot activity-controls\n'
            steps += click('TexKind(Grain)') + 'settingsbounds\nshot grain-controls\n'
            steps += click('Shell(Stroke)') + 'settingsbounds\nshot frame-controls\n'
            run(f'visual-{width}-{face}', steps, still, face=face, width=width)

    steps = click('TexKind(None)') + 'home\nwait 300\nshot grain-off\n'
    steps += click('TexKind(Grain)') + 'home\nwait 300\nshot grain-on\n'
    grain = run('grain-coexistence', steps, still)
    assert grain['surface']['material'] == 'InkPool' and grain['surface']['texture_kind'] == 'Grain'
    assert not same_edge('grain-coexistence', 'grain-off', 'grain-on'), 'Grain did not affect the material edge'
finally:
    # Remove only keychain entries minted by these disposable profiles.
    for marker in root.glob('*/profile/.vault-id'):
        subprocess.run(['/usr/bin/security', 'delete-generic-password', '-s',
                        'dev.nus.local-state.v1', '-a', marker.read_text()],
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False)

print('Carapaces: native clicks, persistence, sources, reduced motion, real shell, grain and visual captures passed.', flush=True)
