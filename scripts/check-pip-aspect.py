#!/usr/bin/env python3
"""Native stream-aspect checks using a local canvas stream and disposable profile."""
from pathlib import Path
exec(compile(Path(__file__).with_name('check-settings.py').read_text().split('base=run(')[0], 'check-settings.py', 'exec'))

video=root/'aspect.html'
video.write_text('''<!doctype html><title>PiP aspect test</title>
<style>body{margin:0;background:#802050}video{width:100vw;height:100vh;object-fit:contain}canvas{display:none}</style>
<video autoplay muted playsinline></video><canvas></canvas><script>
const c=document.querySelector('canvas'),g=c.getContext('2d'),v=document.querySelector('video');
function source(w,h){c.width=w;c.height=h;const old=v.srcObject;v.srcObject=c.captureStream(30);old?.getTracks().forEach(t=>t.stop());v.play();}
function paint(){g.fillStyle='#14252c';g.fillRect(0,0,c.width,c.height);g.strokeStyle='#6ddac6';g.lineWidth=8;g.strokeRect(8,8,c.width-16,c.height-16);g.beginPath();g.arc(c.width/2,c.height/2,Math.min(c.width,c.height)/3,0,Math.PI*2);g.stroke();requestAnimationFrame(paint)}
source(360,640);paint();</script>''')
prefs={'behavior':{'hatch_background':False,'hatch_status':False},'motion':{'reduce':True}}
os.environ['NUS_SHOT_SIZE']='1200x850'
steps=[f'tab {video.as_uri()}','wait 1800','pip','wait 500','pipaspect 0.5625','shotpip portrait',
       "eval v.style.objectFit='cover'",'wait 300','pipaspect 0.5625','shotpip portrait-cover',
       "eval v.style.objectFit='contain'",'wait 300',
       'pipnativesize 350 400','wait 400','pipaspect 0.5625','pipcheck','wait 200','pipaspect 0.5625']
for width,height in [(640,480),(960,540),(840,360)]:
    ratio=width/height
    steps += [f'eval source({width},{height})','wait 700',f'pipaspect {ratio}',
              'pipedge','wait 300',f'pipaspect {ratio}',
              'pipnativesize 500 400','wait 300',f'pipaspect {ratio}',f'shotpip aspect-{width}-{height}']
steps += ['pipclose','eval source(360,640)','wait 600','pip','wait 500','pipaspect 0.5625']
run('pip-stream-aspect','\n'.join(steps),prefs)
run('pip-reused-window',f'tab {video.as_uri()}\nwait 1800\npip\nwait 500\npipaspect 0.5625\nnewwindow\nwait 6000',prefs,
    second=f'wait 500\ntab {video.as_uri()}\nwait 1800\neval source(640,480)\nwait 700\npip\nwait 500\npipaspect {4/3}')
print('PiP stream aspect checks passed.',flush=True)
