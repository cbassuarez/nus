#!/usr/bin/env python3
"""CEF PiP skip bursts with generated seekable video and an isolated profile."""
from pathlib import Path
import shutil
exec(compile(Path(__file__).with_name('check-settings.py').read_text().split('base=run(')[0], 'check-settings.py', 'exec'))
ffmpeg=shutil.which('ffmpeg')
assert ffmpeg, 'ffmpeg is required for the local video fixture'
movie=root/'skip.webm'
subprocess.run([ffmpeg,'-nostdin','-loglevel','error','-f','lavfi','-i','testsrc2=size=640x360:rate=8',
                '-t','70','-an','-c:v','libvpx','-deadline','realtime','-b:v','300k',str(movie)],check=True)
page=root/'skip.html'
page.write_text('''<!doctype html><title>PiP skip burst fixture</title>
<style>body{margin:0;background:#152629}video{width:100vw;height:100vh}</style>
<video src="skip.webm" muted playsinline preload="auto"></video>
<script>const v=document.querySelector('video');v.addEventListener('loadedmetadata',()=>{v.pause();v.currentTime=30;});</script>''')
prefs={'behavior':{'hatch_background':False,'hatch_status':False,'pip_skip_seconds':1},'motion':{'register':0.5,'reduce':True}}
steps=[f'tab {page.as_uri()}','wait 5000','assertvideotime 30','pip','wait 600','piptransportcheck',
       'pipskipburst','wait 700','assertvideostate paused','assertvideotime 30',
       'pipclick play','wait 300','pipskipburst','wait 900','assertvideostate playing',
       'pipclick play','wait 300','assertvideostate paused','pipskipburst','wait 700','assertvideostate paused',
       'videostart','wait 400','assertvideotime 30','pipclose']
run('pip-skip-bursts','\n'.join(steps),prefs)
print('PASS CEF PiP: rapid actual skip presses preserve playing and paused states.')
