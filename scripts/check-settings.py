#!/usr/bin/env python3
"""Native settings and onboarding regressions, using isolated profiles."""
import copy
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

bundle = Path(sys.argv[1] if len(sys.argv)>1 else 'dist/nus.app').resolve()
root = Path(tempfile.mkdtemp(prefix='nus-settings-check-'))
print(f'Evidence: {root}', flush=True)

def run(name, steps, prefs=None, onboarding=False, face='paper', second=None, download_history=None):
    directory=root/name
    profile=directory/'profile'
    profile.mkdir(parents=True)
    if not onboarding: (profile/'onboarded').write_text('skip')
    if prefs: (profile/'settings.json').write_text(json.dumps(prefs))
    if download_history is not None: (profile/'downloads.json').write_text(json.dumps(download_history))
    script=directory/'check.shot'
    script.write_text('wait 1600\n'+steps+'\n')
    env=dict(os.environ,NUS_SHOT_DIR=str(directory),NUS_SHOT=str(script),NUS_SHOT_OUT=str(directory/'screens'))
    if face: env['NUS_MODE']=face
    else: env.pop('NUS_MODE',None)
    env.pop('NUS_SHOT2',None)
    if second:
        script2=directory/'second.shot'; script2.write_text(second+'\n'); env['NUS_SHOT2']=str(script2)
    log=directory/'run.log'
    with log.open('w') as out:
        result=subprocess.run([str(bundle/'Contents/MacOS/nus')],env=env,stdout=out,stderr=subprocess.STDOUT,timeout=90)
    text=log.read_text()
    if result.returncode or 'panicked' in text: raise RuntimeError(f'{name}: {log}\n{text[-3500:]}')
    print('PASS',name,flush=True)
    return profile

base=run('bindings','settingscheck\nphonecheck\nwait 100\nsettingsat 3\nwait 100\nshot bindings')
prefs=json.loads((base/'settings.json').read_text())
prefs['window_rect']=[70,70,1440,1000]
prefs['behavior']['splash']='None'
prefs['motion']['reduce']=True
run('onboarding','asserttabs 1\nassertnoshells\nassertpane welcome\nassertprofile open\nshot profile-modal\ncloseprofile\nwait 100\nshot welcome\nwelcomeclick Prompt\nassertpane home\nasserttabs 2\nwelcome\nwait 100\nwelcomeclick Settings(2)\nassertpane settings',prefs,True)
run('onboarding-dismiss','asserttabs 1\ncloseprofile\nwelcomedismiss\nassertpane home\nasserttabs 1',prefs,True)
steps=[]
for section,first,second in [(3,'HdrStyle(Rail)','HdrStyle(Bar)'),(4,'OpenedBy(Front)','OpenedBy(Behind)'),(6,'ClickToSource(false)','ClickToSource(true)'),(7,'PortsGrouping(Process)','PortsGrouping(Origin)'),(8,'HatchLook(Card)','HatchLook(Sheet)')]:
    steps += [f'settingsat {section}','wait 100',f'settingclick {first}','wait 100',f'assertchoice {first}',f'settingclick {second}','wait 100',f'assertchoice {second}']
run('hatch-height','hatchsize 30\nwait 700\nasserthatchsize 30\nhatchsize 60\nwait 700\nasserthatchsize 60',prefs)
run('native-clicks','\n'.join(steps)+'\nsettingsat 3\nwait 100\nsettingseek Slider(Grace,\nsliderdrag 0.8\nwait 100\nshot slider-drag',prefs)
run('shared-windows','settingsat 4\nwait 100\nnewwindow\nwait 2200\nassertchoice OpenedBy(Front)\nsettingclick OpenedBy(Behind)\nwait 6000',prefs,second='wait 1200\nsettingsat 4\nwait 100\nsettingclick OpenedBy(Front)\nwait 2500\nassertchoice OpenedBy(Behind)\nshot shared-secondary')
manual=run('manual-theme','appearance ink\nassertappearance ink',prefs,face=None)
run('manual-theme-relaunch','assertappearance ink\nassertnoshells',json.loads((manual/'settings.json').read_text()),face=None)
for width,face in [(2560,'paper'),(1440,'paper'),(800,'paper'),(480,'ink')]:
    visual=copy.deepcopy(prefs);visual['window_rect']=[80,80,width,1100]
    steps=['welcome','wait 100','shot welcome','settingsat 3']
    for section in range(1,15):
        steps += [f'settingsat {section}','wait 80',f'shot page-{section:02}', 'settingsscroll 660','wait 80',f'shot page-{section:02}-scroll']
    run(f'visual-{width}-{face}','\n'.join(steps),visual,face=face)
print('Settings and onboarding checks passed.',flush=True)
