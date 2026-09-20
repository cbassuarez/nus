#!/usr/bin/env python3
"""Native media and picker regressions, using disposable profiles.

Usage: python3 scripts/check-media-picker.py [path/to/nus.app]
Requires macOS desktop access and ffmpeg. Tests browser-owned transport on a
seekable video with page controls hidden and disablePictureInPicture enabled,
first presentation, surface reuse, UI labels, and a non-blocking picker sheet.
The picker closes with its owner; manual file selection is tested separately.
"""
from pathlib import Path
exec(compile(Path(__file__).with_name('check-settings.py').read_text().split('base=run(')[0], 'check-settings.py', 'exec'))
prefs={'behavior':{'hatch_background':False,'hatch_status':False,'splash':'None','pip_skip_seconds':17},'motion':{'register':0.5,'reduce':True}}
video=root/'restricted-player.html'
subprocess.run(['ffmpeg','-loglevel','error','-f','lavfi','-i','testsrc2=size=640x360:rate=5','-t','70','-c:v','libvpx-vp9','-b:v','90k',str(root/'seekable.webm')],check=True)
video.write_text('''<!doctype html><title>nus transport check</title><style>body{margin:0;background:#14252c}video{width:100vw;height:100vh;object-fit:contain}</style><video src="seekable.webm" muted playsinline preload="auto" disablepictureinpicture controlslist="nodownload noplaybackrate noremoteplayback"></video>''')
os.environ['NUS_SHOT_SIZE']='1200x850'
run('ui-labels','uilabels\nsettingsat 6\nwait 200\nsettingseek Slider(PipSkip\npipskipkeyboard\nshot browser-settings',prefs)
steps=f'foreground\nwait 150\ntab {video.as_uri()}\nwait 1800\nvideostart\nwait 300\nassertvideotime 30\npip\nwait 200\npiptransportcheck\npipkeys right\nwait 300\nassertvideotime 47\npipkeys tab\npipkeys left\nwait 300\nassertvideotime 30\npipretarget\nwait 100\npiptransportcheck\npipkeys right\nwait 300\nassertvideotime 47\nshotpip skip-controls'
run('pip-restricted-player',steps,prefs)
run('picker-open','pickavatar\nwait 500\nassertpicker open\nshot picker-parent\nwait 500\nassertpicker open',prefs)
print('PASS native label audit, restricted player transport, warm retarget, non-blocking picker',flush=True)
