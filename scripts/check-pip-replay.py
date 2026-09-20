#!/usr/bin/env python3
"""Native PiP and session-map regression checks, using disposable profiles.

Usage: python3 scripts/check-pip-replay.py [path/to/nus.app]
Requires macOS desktop access. Does not use the user's profile or network.
"""
from pathlib import Path
exec(compile(Path(__file__).with_name('check-settings.py').read_text().split('base=run(')[0], 'check-settings.py', 'exec'))

video=root/'video.html'
video.write_text('''<!doctype html><title>PiP test video</title><style>body{margin:0;background:#14252c}video{width:100vw;height:100vh}canvas{display:none}</style><video autoplay muted playsinline></video><canvas width="960" height="540"></canvas><script>const c=document.querySelector('canvas'),g=c.getContext('2d'),v=document.querySelector('video');let n=0;function paint(){g.fillStyle='#14252c';g.fillRect(0,0,960,540);g.fillStyle='#6ddac6';g.fillRect(60+n%720,340,120,60);g.fillStyle='white';g.font='40px monospace';g.fillText('nus / PiP check',60,100);g.font='22px monospace';g.fillText('Live video · frame '+n++,60,160);requestAnimationFrame(paint)}paint();v.srcObject=c.captureStream(30);v.play();</script>''')
prefs={'behavior':{'hatch_background':False,'hatch_status':False},'motion':{'register':0.5,'reduce':True}}
commands=['newshell','wait 800']
for i in range(8):
    commands.extend([f"shell printf 'history-output-{i}\\n'; for i in {{1..30}}; do printf '  recorded line %s passed\\n' $i; done"+('; false' if i==4 else ''),'wait 450'])
commands.extend(['timeline','wait 300','historycheck','shot map','historymap','wait 100','shot dragged','historysearch history-output-4','wait 100','historyassertmatches 1','shot search','historysearch no-results-fixture','wait 100','historyassertmatches 0','shot empty','historysearch','tl home','wait 100','historysnapshot','wait 100','shot snapshot','historysnapshot','wait 100','historyexport'])
for face,size in [('paper','1200x850'),('ink','480x720')]:
    os.environ['NUS_SHOT_SIZE']=size
    profile=run('history-'+face,'\n'.join(commands),prefs,face=face)
    exports=list((profile/'shares').glob('*.html'));assert len(exports)==1
    html=exports[0].read_text();assert 'session-map' in html and 'autoplay:false,loop:false' in html and '@@' not in html
    script=root/('export-'+face+'.mjs');script.write_text(html.split('<script type="module">',1)[1].split('</script>',1)[0])
    subprocess.run(['node','--check',str(script)],check=True)
os.environ['NUS_SHOT_SIZE']='1200x850'
run('pip',f'tab {video.as_uri()}\nwait 2000\npip\nwait 600\npippoint 200 100\npiptransportcheck\npipcheck\nwait 200\npipedge\nwait 200\nshotpip controls',prefs)
# A second main window replaces the existing PiP. The host's invariant and
# the second window's pipcheck exercise cross-window ownership.
run('pip-two-windows',f'tab {video.as_uri()}\nwait 1800\npip\nwait 600\nnewwindow\nwait 5000',prefs,
    second=f'wait 600\ntab {video.as_uri()}\nwait 1800\npip\nwait 600\npipcheck\nwait 1000')
print('PiP and session-map checks passed.',flush=True)
