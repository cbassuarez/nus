#!/usr/bin/env python3
"""Native reading/reveal/sky checks in disposable profiles; no user profile touched."""
import copy,json,os,subprocess,sys,tempfile
from pathlib import Path
bundle=Path(sys.argv[1] if len(sys.argv)>1 else 'dist/nus.app').resolve()
root=Path(tempfile.mkdtemp(prefix='nus-reading-arrival-'));print('Evidence:',root,flush=True)
def run(name,steps,prefs=None,fresh=False,clock=None):
 d=root/name;p=d/'profile';p.mkdir(parents=True,exist_ok=True)
 if prefs is not None:(p/'settings.json').write_text(json.dumps(prefs))
 if not fresh:(p/'onboarded').write_text('skip')
 shot=d/'check.shot';shot.write_text('wait 2800\n'+steps+'\n')
 env=dict(os.environ,NUS_SHOT_DIR=str(d),NUS_SHOT=str(shot),NUS_SHOT_OUT=str(d/'screens'))
 for k in ['NUS_SHOT2','NUS_MODE','NUS_CLOCK']:env.pop(k,None)
 if clock:env['NUS_CLOCK']=str(clock)
 with (d/'run.log').open('w') as f:r=subprocess.run([str(bundle/'Contents/MacOS/nus')],env=env,stdout=f,stderr=subprocess.STDOUT,timeout=90)
 log=(d/'run.log').read_text()
 if r.returncode or 'panicked' in log:raise RuntimeError(f'{name}: {d}/run.log\n{log[-3000:]}')
 print('PASS',name,flush=True);return p
p=run('arrival','assertpane home\nassertprofile open\nshot profile\ncloseprofile\nassertpane welcome\nshot welcome\narrival 0.15\nshot arrival-mark\narrival 0.9\nshot arrival-warp\nwait 1800\nkey Escape\nwelcomedismiss\nassertpane home',fresh=True)
assert (p/'arrival-seen').exists()
base=json.loads((p/'settings.json').read_text());base['behavior'].update(splash='None',then='Prompt',atlas='Planet',window_start='Last');base['window_rect']=[80,80,1400,1050]
article=root/'article.html';article.write_text('<!doctype html><meta charset="utf-8"><title>A place to return to</title><article><h1>A place to return to</h1><p>By a reader</p>'+''.join(f'<h2>Chapter {i}</h2><p>'+('Reading should give a thought room to breathe. A page worth saving deserves a quiet place to return to, wherever your next day begins. '*7)+'</p>' for i in range(1,20))+'<pre>let thought = "keep this";\n  return thought;</pre></article>')
p=run('reading',f'tab {article.as_uri()}\nwait 1300\nsavereading\nwait 1800\nlibrary\nwait 350\nreadingassert 2 1000\nshot library\nreadingopen A place\nwait 250\nreadingbounds\nshot reader\nlibraryclick More\nwait 100\nshot reading-options\nkey End\nkey Enter\nwait 150\nshot reading-remove-confirmation\nkey Enter\nwait 150\nreadingbounds\nreadingscroll 1800\nwait 1000\nreadingprogress 0.05\nshot reader-resume\nreadingback',base)
entries=[json.loads(path.read_text()) for path in (p/'library').glob('*.json')]
assert len(entries)==2
starter=[e for e in entries if e['source']=='https://cbassuarez.com/nus.dev/']
assert len(starter)==1 and starter[0]['snapshot'] is None and not starter[0]['deleted']
assert (p/'library/.defaults-v1').read_text()=='complete\n'
e=next(e for e in entries if e['source']==article.as_uri())
assert not e['deleted'] and not e['archived'] and not e['finished'], 'Keep must not mutate the item'
assert e['snapshot'] and (p/'library/objects'/f"{e['snapshot']}.article").is_file()
article.unlink()
run('reading','library\nwait 400\nreadingopen A place\nwait 200\nreadingassert 2 1000\nreadingprogress 0.05\nshot offline-resume\nkey cmd+f\ntype breathe\nkey Enter\nshot reader-find\nkey Escape\nkey cmd+plus\nwait 150\nreadingbounds\nshot reader-larger\nkey Escape\nreadingbounds\nshot library-return')
for face in ['paper','ink']:
 prefs=copy.deepcopy(base);prefs['theme_mode']=face;prefs['behavior']['follow_os_theme']=False
 for art,place,clock in [('sky',[35.2,-106.6],1718982000000),('sky',[35.2,-106.6],1718949600000),('space',[-33.9,151.2],1718982000000)]:
  prefs['behavior'].update(home_look='Art',home_art=art,place=place)
  run(f'{art}-{face}-{clock}','home\nwait 250\nasserthomeart '+art+'\nshot background',prefs,clock=clock)
print('Native reading, reveal and sky checks passed.',flush=True)
