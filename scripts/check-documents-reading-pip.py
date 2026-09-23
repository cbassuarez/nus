#!/usr/bin/env python3
"""Native document, reading-form, arrival and PiP focus regressions.
Uses disposable profiles and local fixtures; no browsing data is touched.
Build with scripts/bundle-mac.sh --debug, then pass the bundle path.
"""
from pathlib import Path
exec(compile(Path(__file__).with_name('check-settings.py').read_text().split('base=run(')[0], 'check-settings.py', 'exec'))
run_case=run
def run(name,*args,**kwargs):
 only=os.environ.get('NUS_CHECK_ONLY','')
 if only and not any(name.startswith(prefix) for prefix in only.split(',')):return None
 return run_case(name,*args,**kwargs)
prefs={'behavior':{'splash':'None','hatch_background':False,'hatch_status':False,'follow_os_theme':False},'motion':{'register':0.5,'reduce':True},'window_rect':[90,90,1200,900]}
(root/'images').mkdir()
(root/'images/picture.svg').write_text('<svg xmlns="http://www.w3.org/2000/svg" width="160" height="90"><rect width="160" height="90" fill="#377b91"/><circle cx="80" cy="45" r="24" fill="#f8df8c"/></svg>')
md=root/'A document.md'
md.write_text('# A place to read\n\nA **formatted** document with *emphasis*, [a linked file](next.md), and a local image.\n\n![A local illustration](images/picture.svg)\n\n## Details\n\n- [x] Local files\n- [ ] Remote images\n\n| Name | Value |\n| --- | --- |\n| One | 1 |\n\n```rust\nfn main() { println!("hello"); }\n```\n\n<script>window.bad=1</script>\n')
(root/'next.md').write_text('# Linked document\nRelative navigation worked.')
(root/'data.json').write_text('{"project":"nus","enabled":true,"items":[{"name":"first","count":4},{"name":"<script>not executable</script>"}]}')
(root/'data.csv').write_text('Name,Note,Count\nAda,"A quoted, multiline\nnote",7\nBen,Plain,2\n')
(root/'notes.txt').write_text('Plain text stays literal.\n<script>not executable</script>\n')

def reply(js,expected): return f'eval {js}\nwait 300\nassertreply {expected}'
run('file-tree',f'document {md}\nwait 1000\nasserttabs 1\ndocument {md}\nasserttabs 1\n'+reply("document.querySelector('article h1')?.textContent",'A place to read'),prefs)
run('markdown',f'tab {md.as_uri()}\nwait 1800\n'+reply("JSON.stringify({heading:document.querySelector('article h1')?.textContent,image:document.querySelector('article img')?.naturalWidth,bad:window.bad||0,code:!!document.querySelector('pre code')})",'"image":160,"bad":0,"code":true')+'\nshot markdown\n'+reply("document.querySelector('a[href=\"next.md\"]').click(); 'clicked'",'clicked')+'\nwait 1000\n'+reply("document.querySelector('article h1')?.textContent",'Linked document'),prefs)
for name,js,expected in [('data.json',"document.querySelectorAll('article details').length",'5'),('data.csv',"JSON.stringify([...document.querySelectorAll('tbody td')].map(x=>x.textContent))",'A quoted, multiline'),('notes.txt',"document.querySelector('article pre')?.textContent",'<script>not executable</script>')]:
 run('viewer-'+name,f'tab {(root/name).as_uri()}\nwait 1200\n'+reply(js,expected)+f'\nshot {name}',prefs)
run('viewer-settings','settingsat 18\nwait 200\nsettingsbounds\nshot viewers\nsettingseek Viewer(Theme(Ink))\nsettingclick Viewer(Theme(Ink))\nwait 100\nassertchoice Viewer(Theme(Ink))\ntab '+md.as_uri()+'\nwait 1200\n'+reply("getComputedStyle(document.documentElement).backgroundColor",'rgb(29, 32, 37)')+'\nshot override',prefs)
run('viewer-disabled',f'tab {md.as_uri()}\nwait 1200\n'+reply("document.querySelector('article')===null",'true'),{'behavior':{**prefs['behavior'],'viewers':{'markdown':False}},'motion':prefs['motion']})
for width in [1200,480]:
 os.environ['NUS_SHOT_SIZE']=f'{width}x850'
 p=run(f'reading-form-{width}','library\nwait 500\nlibraryclick Add\nwait 100\ninput A note\nkey Tab\nkey Tab\ninput First paragraph\nkey Enter\ninput Second paragraph\nreadingbounds\nshot draft\nkey cmd+enter\nwait 400\nreadingassert 2 4\nreadingcontext A note\nwait 100\nshot context\ninput edit\nkey Enter\nwait 100\nkey cmd+u\ninput Updated note\nkey cmd+enter\nwait 400\nreadingopen Updated note\nwait 200\nreadingbounds\nshot saved-note\nreadingback\nlibraryclick Add\nwait 100\ninput Discard me\nkey Escape\nreadingassert 2 4',prefs)
 if p is None:continue
 entries=[json.loads(f.read_text()) for f in (p/'library').glob('*.json')]
 assert len(entries)==2 and any(e['title']=='Updated note' and e['user_notes']=='First paragraph\nSecond paragraph' for e in entries)
os.environ['NUS_SHOT_SIZE']='1200x850'
video=root/'video.html'
video.write_text('''<video autoplay muted playsinline style="width:100%;height:100%"></video><canvas width="960" height="540" hidden></canvas><script>const c=document.querySelector('canvas'),g=c.getContext('2d'),v=document.querySelector('video');let n=0;function draw(){g.fillStyle='#173344';g.fillRect(0,0,960,540);g.fillStyle='#eee';g.font='40px monospace';g.fillText('PiP frame '+n++,80,120);requestAnimationFrame(draw)}draw();v.srcObject=c.captureStream(30);v.play();</script>''')
run('pip-keep',f'tab {video.as_uri()}\nwait 2000\npipfocus off\nwait 700\npipassert open\npipfocus on\nwait 200\npipassert open\nshotpip away\npipclose\npipassert closed',prefs)
closeprefs=copy.deepcopy(prefs);closeprefs['behavior']['pip_policy']={'focus_app':True}
run('pip-return',f'tab {video.as_uri()}\nwait 1800\npipfocus off\nwait 700\npipassert open\npipfocus on\npipassert closed',closeprefs)
for face in ['paper','ink']:
 arrival=copy.deepcopy(prefs);arrival['behavior']['splash']='Draw';arrival['motion']['reduce']=False
 run('arrival-'+face,'wait 1000\narrival 0.45\nshot arrival-mark\narrival 1.25\nshot arrival-full\nkey Enter\nwait 100\nassertpane welcome\nshot arrival-complete',arrival,onboarding=True,face=face)
print('Document, reading, PiP and arrival checks passed.',flush=True)
