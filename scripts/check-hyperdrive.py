#!/usr/bin/env python3
"""Native first-arrival checks, using disposable profiles and rendered frames."""
import copy
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

bundle = Path(sys.argv[1] if len(sys.argv)>1 else 'dist/nus.app').resolve()
root = Path(tempfile.mkdtemp(prefix='nus-hyperdrive-check-'))
print(f'Evidence: {root}',flush=True)
def run(name,steps,prefs=None,size='1200x800',onboarded=False):
    d=root/name;p=d/'profile';p.mkdir(parents=True,exist_ok=True)
    if prefs is not None:(p/'settings.json').write_text(json.dumps(prefs))
    if onboarded:(p/'onboarded').write_text('skip')
    shot=d/'check.shot';shot.write_text(steps+'\n')
    env=dict(os.environ,NUS_SHOT_DIR=str(d),NUS_SHOT=str(shot),NUS_SHOT_OUT=str(d/'screens'),NUS_SHOT_SIZE=size)
    for key in ['NUS_SHOT2','NUS_CLOCK','NUS_MODE']:env.pop(key,None)
    with (d/'run.log').open('w') as f:
        result=subprocess.run([str(bundle/'Contents/MacOS/nus')],env=env,stdout=f,stderr=subprocess.STDOUT,timeout=65)
    log=(d/'run.log').read_text()
    assert result.returncode==0 and 'panicked' not in log and 'ARRIVAL CHECK PASSED' in log, (d,log[-3500:])
    print(f'PASS {name}',flush=True)
    return p

p=run('first-launch','wait 100\narrivalcheck active\nkey Escape\nwait 100\narrivalcheck done\nassertpane welcome\nappearance paper\nshot unchanged')
assert (p/'arrival-seen').exists()
base=json.loads((p/'settings.json').read_text())
base['motion']['reduce']=False
base['behavior'].update(splash='Draw',hatch_background=False,hatch_status=False,follow_os_theme=False)
steps='wait 100\nkey Escape\nwait 100\nappearance paper\nshot baseline\n'
for t in [0.1,1.1,3.4,4.8,5.8,6.5,7.8,9.2,10.5,12.3,13.4]:
    steps+=f'arrival {t}\narrivalcheck active\nshot phase-{t}\n'
steps+='arrival 14.7\nwait 100\narrivalcheck done\nassertpane welcome\ntrafficcheck\nshot completed\n'
run('paper',steps,base)
ink=copy.deepcopy(base);ink['theme_mode']='ink'
run('ink-narrow',steps.replace('appearance paper','appearance ink'),ink,'700x650')
still=copy.deepcopy(base);still['motion']['reduce']=True
run('reduced','wait 700\narrivalcheck done\nassertpane welcome\nshot reduced',still)
none=copy.deepcopy(base);none['behavior']['splash']='None'
run('disabled','wait 300\narrivalcheck done\nassertpane welcome',none)
run('seen-profile','wait 300\narrivalcheck done',base,onboarded=True)
run('native-green','wait 3000\narrivalcheck done\ntrafficpress 2\nwait 2500\ntrafficstate fullscreen\ntrafficcheck\ntrafficfullscreen\nwait 2500\ntrafficstate windowed\ntrafficcheck',base,onboarded=True)
print('First-arrival and native button checks passed.',flush=True)
