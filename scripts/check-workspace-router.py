#!/usr/bin/env python3
"""Pointer navigation, shared routing, real CLI fixtures and typography in native nus.

Uses an isolated profile and explicitly simulated CLI executables: no model request,
login, or user's provider configuration is changed by these checks.
"""
from pathlib import Path
exec(compile(Path('scripts/check-settings.py').read_text().split("base=run(")[0], 'check-settings.py', 'exec'))

_run = run
def run(name, steps, prefs=None, **kwargs):
    profile = _run(name, steps, prefs, **kwargs)
    # Screenshot writes can fail independently of the native action assertions.
    # Never report a visual pass if the evidence was not actually saved.
    face = kwargs.get('face', 'paper')
    for step in steps.splitlines():
        if step.startswith('shot '):
            shot = profile.parent/'screens'/f"{step[5:]}-{face}.png"
            assert shot.is_file() and shot.stat().st_size > 1000, f'Missing screenshot: {shot}'
    return profile

profile=run('bootstrap','startpage prompt\nassertpane home')
prefs=json.loads((profile/'settings.json').read_text())
prefs['behavior']['splash']='None'
prefs['motion']['reduce']=True
prefs['window_rect']=[80,80,2560,1600]
prefs['sidebar_pinned']=False
prefs['behavior']['home_look']='Art'
prefs['behavior']['home_art']='memphis'
fixtures=root/'fixtures'
fixtures.mkdir()
providers=[]
for name in ['claude','codex','ollama']:
    path=fixtures/name
    # argv arrives through an actual shell and PTY in the native app.
    path.write_text('''#!/usr/bin/env python3
import json, pathlib, sys, os
name=pathlib.Path(sys.argv[0]).name
args=sys.argv[1:]
with pathlib.Path(sys.argv[0]).with_suffix('.calls.jsonl').open('a') as log: log.write(json.dumps(args)+'\\n')
if '--version' in args: print(name+' fixture 1.0')
elif args[:2]==['auth','status']: print('{"loggedIn":true}')
elif args[:2]==['login','status']: print('Signed in (fixture)')
elif args[:2]==['mcp','get']: print('nus fixture registration')
elif args==['list']: print('NAME ID SIZE MODIFIED\\ntest-model:latest abc 1GB today')
else:
    pathlib.Path(sys.argv[0]).with_suffix('.received.json').write_text(json.dumps({'argv':args,'cwd':os.getcwd()}))
    print('SIMULATED ASSISTANT: prompt received as a literal argument')
''')
    path.chmod(0o755)
    providers.append({'executable':str(path),'model':'test-model:latest' if name=='ollama' else ''})
prefs['behavior']['assistants']={'providers':providers}
prefs['behavior']['ask_backend']='claude'

steps=['welcome','wait 100','settingsat 0','wait 100']
for section in range(18):
    steps += [f'settingclick Section({section})','wait 100',f'assertsection {section}','assertprofile closed','settingsbounds']
steps += ['home','wait 100','homelook art memphis','homeclear','hometype home','homeenter','assertpane home','assertprofile closed','promptcheck','homeclear','wait 100','shot home']
run('pointer-navigation','\n'.join(steps),prefs)

steps=['settingsat 9','wait 3500','shot assistants','assertconnections','settingclick Workspace(AssistantTab(1))','wait 100','shot work','settingclick Workspace(AssistantTab(2))','wait 100','shot context']
run('connections','\n'.join(steps),prefs)

# Change through pointer controls and verify after a separate launch.
steps=['settingsat 16','wait 100','settingclick Workspace(PromptPreset(Web))','wait 100','assertchoice Workspace(PromptPreset(Web))','shot web-preset','palette go espresso','assertpromptfirst Search the web','close','settingsat 16','wait 100','settingclick Workspace(PromptPreset(Assistants))','wait 100','palette go review this change','assertpromptfirst Review prompt for Claude','enter','wait 100','assertpromptfirst Start Claude','shot review','close']
saved=run('routing','\n'.join(steps),prefs)
run('routing-relaunch','palette go explain this\nassertpromptfirst Review prompt for Claude',json.loads((saved/'settings.json').read_text()))

custom=run('custom-sources', 'settingsat 16\nwait 100\nsettingseek Workspace(Layout(0))\nsettingclick Workspace(Layout(0))\nwait 100\nsettingclick Workspace(Layout(1))\nwait 100\nsettingseek Workspace(Source(Shell, 0))\nsettingclick Workspace(Source(Shell, 0))\nwait 100\nsettingseek Workspace(Count(Shell, 1))\nsettingclick Workspace(Count(Shell, 1))\nwait 100\nsettingseek Workspace(Move(Shell, -1))\nsettingclick Workspace(Move(Shell, -1))\nwait 100\nsettingsbounds\nshot custom-sources', prefs)
custom_prefs=json.loads((custom/'settings.json').read_text())
config=custom_prefs['behavior']['prompt']
assert config['wide'] and config['compact']
shell=next(s for s in config['sources'] if s['source']=='Shell')
assert shell['home'] and shell['count']==4
run('custom-sources-relaunch','settingsat 16\nwait 100\nsettingseek Workspace(Source(Shell, 0))\nassertchoice Workspace(Source(Shell, 0))',custom_prefs)

literal="Explain what's in $(touch /tmp/nus-router-unsafe); keep `ticks` literal"
for i,name in enumerate(['claude','codex','ollama']):
    run('launch-'+name,f'assistantdraft {i} {literal}\nwait 100\nshot review\nenter\nwait 1300\nassertpane term\nshot terminal',prefs)
    result=json.loads((fixtures/(name+'.received.json')).read_text())
    assert result['argv'][-1]==literal, result
    assert result['argv'][-2]=='--', result
    assert result['cwd']!=str(root/('launch-'+name)), result
assert not Path('/tmp/nus-router-unsafe').exists(), 'shell expanded a prompt'

steps=['settingsat 15','wait 100','settingclick Workspace(Pairing(1))','wait 100','shot fonts','settingseek Workspace(TypeSize(1, 1))','settingclick Workspace(TypeSize(1, 1))','wait 100','settingseek Workspace(TypeLine(1, 1))','settingclick Workspace(TypeLine(1, 1))','wait 100','settingseek Workspace(Tracking(1))','settingclick Workspace(Tracking(1))','wait 100','home','homeenter','wait 500','assertfonts','shot terminal']
font_profile=run('typography','\n'.join(steps),prefs)
fp=json.loads((font_profile/'settings.json').read_text())
assert fp['behavior']['typography']['terminal_size']==14
assert fp['behavior']['typography']['terminal_line']>1
run('typography-relaunch','home\nhomeenter\nwait 500\nassertfonts',fp)

for width,height,face in [(1000,1300,'paper'),(700,1300,'ink'),(1800,900,'paper')]:
    p=copy.deepcopy(prefs);p['window_rect']=[80,80,width,height]
    steps=[]
    for section in [9,15,16]:
        steps += [f'settingsat {section}','wait 150','settingsbounds',f'shot page-{section}','settingsscroll 500','wait 100','settingsbounds',f'shot page-{section}-scroll']
    run(f'narrow-{width}-{face}','\n'.join(steps),p,face=face)
p=copy.deepcopy(prefs);p['window_rect']=[80,80,700,1300]
long_prompt="Explain this path and quoted arguments without changing files. "*6
run('narrow-review', f'assistantdraft 1 {long_prompt}\nwait 100\nreviewbounds scrollable\nshot review-top\nreviewscroll 900\nwait 100\nreviewbounds\nshot review-scroll\nclose',p,face='ink')
print('Workspace router native checks passed.',flush=True)
