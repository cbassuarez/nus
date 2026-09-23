#!/usr/bin/env python3
"""Exercise Mercury through Settings in isolated native profiles.

Run with the bundled Python (Pillow) and an isolated, freshly built .app.
Screenshots come from the app's actual GPU renderer, not a mockup.
"""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

from PIL import Image, ImageChops

bundle = Path(sys.argv[1] if len(sys.argv) > 1 else '/tmp/nus-mercury-review.app')
root = Path(sys.argv[2]) if len(sys.argv) > 2 else Path(tempfile.mkdtemp(prefix='nus-mercury-art-'))
root = root.resolve()
root.mkdir(parents=True, exist_ok=True)
print(f'Evidence: {root}', flush=True)
only = set(filter(None, os.environ.get('NUS_MERCURY_ONLY', '').split(',')))


def run(name, steps, *, reduced=False, width=1100, height=900, onboarding=False, reuse=False):
    directory = root / name
    profile = directory / 'profile'
    if only and name not in only:
        assert profile.is_dir(), f'No previous evidence to reuse for {name}'
        return profile
    profile.mkdir(parents=True, exist_ok=True)
    if not reuse:
        for marker in ['mercury.json', 'mercury-closed.json', 'onboarded', 'me.json']:
            (profile / marker).unlink(missing_ok=True)
        if not onboarding:
            (profile / 'onboarded').write_text('skip')
            (profile / 'me.json').write_text(json.dumps({'name': 'Mercury Review', 'face': 'Initial', 'created': '2026-09-22'}))
        (profile / 'settings.json').write_text(json.dumps({
            'window_rect': [70, 70, width, height], 'motion': {'register': 0.5, 'reduce': reduced},
            'behavior': {'splash': 'None', 'update_checks': False},
        }))
    script = directory / 'check.shot'
    script.write_text(f'wait 1400\nwindow {width} {height}\nwait 200\n' + steps + '\n')
    env = dict(os.environ, NUS_SHOT_DIR=str(directory), NUS_SHOT=str(script),
               NUS_SHOT_OUT=str(directory / 'screens'), NUS_MODE='paper',
               NUS_DOCK_TRACE=str(directory / 'dock'))
    env.pop('NUS_SHOT2', None)
    with (directory / 'run.log').open('w') as output:
        result = subprocess.run([str(bundle / 'Contents/MacOS/nus')], env=env,
                                stdout=output, stderr=subprocess.STDOUT, timeout=240)
    log = (directory / 'run.log').read_text()
    assert result.returncode == 0 and 'panicked' not in log, log[-5000:]
    print('PASS', name, 'relaunch' if reuse else '', flush=True)
    return profile


def same(name, a, b):
    directory = root / name / 'screens'
    a = Image.open(directory / f'{a}-paper.png').convert('RGB')
    b = Image.open(directory / f'{b}-paper.png').convert('RGB')
    return ImageChops.difference(a, b).getbbox() is None


open_settings = 'looktab 5\nwait 250\nsettingseek Mercury\nwait 180\n'
claim = open_settings + 'shot settings-claim\nwait 850\nshot settings-liquid-next\nsettingclick Mercury\n'

try:
    navigation = run('navigation', '''looktab 0
wait 200
settingseek LookTab(5)
settingclick LookTab(5)
wait 200
settingsbounds
shot app-icon-tab
settingseek Mercury
wait 100
shot app-icon-card''')
    assert not (navigation / 'mercury.json').exists(), 'Navigating to App icon claimed the award'
    profile = run('animated', claim + '''wait 900
shot arrival
wait 1700
shot earned
mercurypresentationcheck
wait 950
shot liquid-next
hover 65 72
wait 85
hover 105 92
wait 85
hover 140 70
wait 85
shot hover-trail
wait 2500
shot hover-cleared
key escape
wait 150
mercurystate closed
shot settings-earned
mercurycheck
''' + f'mercuryexport {root / "icons"}\n' + '''settingclick Mercury
wait 250
mercurypresentationcheck
mercurycontinue
mercurystate closed
wait 4200''')
    switching = root / 'animated' / 'dock'
    assert len(list(switching.glob('*mercury-shell-base.tiff'))) >= 2, 'Claim and replay must each run a shell over the current icon'
    assert len(list(switching.glob('*mercury-intro-0.tiff'))) >= 2, 'Claim/replay skipped the construction loop'
    assert not same('animated', 'earned', 'liquid-next'), 'Liquid motion did not change the rendered image'
    assert not same('animated', 'settings-claim', 'settings-liquid-next'), 'Settings Mercury card is static'
    a = Image.open(root / 'animated/screens/earned-paper.png').convert('RGB').crop((380, 150, 730, 450))
    b = Image.open(root / 'animated/screens/liquid-next-paper.png').convert('RGB').crop((380, 150, 730, 450))
    from PIL import ImageStat
    assert sum(ImageStat.Stat(ImageChops.difference(a, b)).mean) / 3 > 1, 'Metal movement is imperceptible'
    def dark_pixels(name):
        im = Image.open(root / f'animated/screens/{name}-paper.png').convert('RGB').crop((70, 85, 260, 215))
        return sum(max(p) < 180 for p in im.get_flattened_data())
    assert dark_pixels('hover-trail') > dark_pixels('hover-cleared') + 100, 'Dither trail did not appear and dissolve'
    receipt = (profile / 'mercury.json').read_bytes()
    run('animated', 'mercurycheck\n' + open_settings + 'wait 5000\nshot settings-relaunch', reuse=True)
    assert (profile / 'mercury.json').read_bytes() == receipt
    dock = root / 'animated' / 'dock'
    assert not list(dock.glob('*mercury-liquid*.tiff')), 'Dock must not animate after construction'
    settled = sorted(dock.glob('*mercury-still.tiff'))
    assert len(settled) >= 3, 'Claim, replay and relaunch did not settle'
    def native_image(path):
        png = path.with_suffix('.png')
        subprocess.run(['/usr/bin/sips', '-s', 'format', 'png', str(path), '--out', str(png)], check=True, stdout=subprocess.DEVNULL)
        return Image.open(png).convert('RGBA')
    first = native_image(settled[0])
    assert all(not ImageChops.difference(first, native_image(p)).getbbox() for p in settled[1:]), 'Settled Dock artwork changed'
    bases = sorted(dock.glob('*mercury-shell-base.tiff'))
    assert len(bases) >= 3, 'Claim, replay and launch must each retain the current icon as a shell base'
    first_base = native_image(bases[0])
    first_overlay = native_image(sorted(dock.glob('*mercury-intro-0.tiff'))[0])
    if first_base.size != first_overlay.size:
        first_base = first_base.resize(first_overlay.size, Image.Resampling.LANCZOS)
    solid = [i for i, a in enumerate(first_base.getchannel('A').get_flattened_data()) if a > 245]
    overlay_alpha = list(first_overlay.getchannel('A').get_flattened_data())
    assert solid and sum(overlay_alpha[i] > 240 for i in solid) / len(solid) > 0.98, 'Incoming shell erased the underlying icon'


    run('reduced', claim + '''wait 100
mercurypresentationcheck
shot reduced-a
wait 700
shot reduced-b
key enter
mercurystate closed''', reduced=True)
    assert same('reduced', 'reduced-a', 'reduced-b'), 'Reduced motion must be completely still'

    run('narrow', claim + '''wait 2400
mercurypresentationcheck
shot narrow-modal
key escape
wait 150
mercurystate closed
settingsbounds
shot narrow-settings''', width=480, height=640)

    run('short', claim + '''wait 2400
mercurypresentationcheck
shot short-modal
key escape
mercurystate closed''', width=800, height=420)

    run('onboarding', '''mercurystate absent
shot onboarding
closeprofile
welcome
wait 100
mercurystate absent
shot welcome''', onboarding=True)
    assert not (root / 'onboarding' / 'profile' / 'mercury.json').exists()
    if os.environ.get('NUS_MERCURY_RECORD') == '1':
        run('film', open_settings + '''record mercury-reveal 8
at 0.0 settingclick Mercury
at 3.0 hover 65 72
at 3.15 hover 105 92
at 3.3 hover 140 70
at 3.45 hover 180 95
mercurypresentationcheck
key enter
mercurystate closed''', width=1100, height=900)
finally:
    # The Dock tracer also captures unrelated launch frames and the bundle's
    # multi-resolution icon. Retain only Mercury readback and event metadata.
    for image in root.glob('*/dock/*.tiff'):
        if 'mercury' not in image.name:
            image.unlink()
    for marker in root.glob('*/profile/.vault-id'):
        subprocess.run(['/usr/bin/security', 'delete-generic-password', '-s',
                        'dev.nus.local-state.v1', '-a', marker.read_text()],
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False)

print('Mercury: Settings claim/replay, native Dock, animation, reduced motion, bounds, persistence and onboarding isolation passed.', flush=True)
