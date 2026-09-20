#!/usr/bin/env python3
"""Native UI checks in disposable profiles; synthetic ports, no port probes."""
from pathlib import Path
exec(compile(Path(__file__).with_name('check-settings.py').read_text().split('base=run(')[0], 'check-settings.py', 'exec'))
os.environ['NUS_PORTS_FIXTURE']='1'
profile=run('bootstrap','sidebarwidth 280\nwait 150\npinsassert Welcome|Downloads|Ports\npinsbounds')
prefs=json.loads((profile/'settings.json').read_text());prefs['motion']['reduce']=True
prefs['behavior'].update(splash='None',ports_probe=False,ports_show_docker=False)
prefs['sidebar_pinned']=True;prefs['window_rect']=[80,80,1800,1400]
p=run('pins','closeprofile\nwelcome\nwait 150\npinsassert Welcome|Downloads|Ports\nassertnoshells\nasserttabs 1\nshot onboarding\nwelcomeclick EditPins\nwait 150\npinclick Edit\nwait 150\npinclick Open(1)\nwait 150\nassertpane downloads\nasserttabs 2\npinclick Open(1)\nasserttabs 2\npinclick Edit\nwait 150\nlivefoldersassert \npinclick Github\nwait 150\nlivefoldersassert GITHUB\npinclick LocalPorts\nwait 150\nlivefoldersassert GITHUB|PORTS\npinclick Github\nwait 150\npinclick LocalPorts\nwait 150\nlivefoldersassert \npinclick Down(0)\nwait 150\npinsassert Downloads|Welcome|Ports\npindrag 0 1\nwait 150\npinsassert Welcome|Downloads|Ports\npindrag 1 0\nwait 150\npinsassert Downloads|Welcome|Ports\npinclick Remove(2)\nwait 150\npinsassert Downloads|Welcome\npinsbounds\nshot edited\npinclick Edit\nwait 150\nkey cmd+w\nwait 150\npinsassert Downloads|Welcome\nasserttabs 1\npinclick Open(0)\nwait 150\nassertpane downloads\nasserttabs 2',prefs,True)
saved=json.loads((p/'settings.json').read_text());assert [x['title'] for x in saved['pinned_tabs']]==['Downloads','Welcome']
run('pins-relaunch','sidebarwidth 280\nwait 150\npinsassert Downloads|Welcome\nassertnoshells\nasserttabs 1\npinclick Open(0)\nwait 150\nassertpane downloads\npinsbounds\nshot restored',saved)
run('onboarding-picks','closeprofile\nwelcome\nwait 150\nwelcomeclick Pins(ToggleDefault(2))\nwait 150\npinsassert Welcome|Downloads\nwelcomeclick Pins(ToggleDefault(2))\nwait 150\npinsassert Welcome|Downloads|Ports\nwelcomeclick EditPins\nwait 150\npinclick Remove(2)\npinsassert Welcome|Downloads',prefs,True)
# Explicit empty choices must stay empty; no eager URL load on launch.
empty=copy.deepcopy(prefs);empty['pinned_tabs']=[]
run('empty-pins','pinsassert \nasserttabs 1\nassertnoshells\npinsbounds\nshot empty',empty)
custom=copy.deepcopy(prefs);custom['pinned_tabs']=[dict(id='offline',title='Not loaded',target={'Page':{'url':'http://127.0.0.1:1/never-open','container':'PERSONAL'}})]
run('lazy-pins','pinsassert Not loaded\nasserttabs 1\nassertnoshells\nassertpane home\npinsbounds',custom)
for width,face in [(900,'paper'),(900,'ink'),(480,'paper')]:
    visual=copy.deepcopy(prefs);visual['window_rect']=[80,80,width*2,680*2]
    run(f'ports-{width}-{face}',f'window {width} 680\nsidebarwidth 80\nhover 400 100\nboardfixture\nboardpage\nhover 450 500\nwait 800\nboardbounds\nshot board\nboarddetail\nwait 150\nboardbounds\nshot detail\nboardscroll 2000\nwait 150\nboardbounds\nshot scrolled\nkey end\nwait 150\nboardselectionvisible\nkey home\nwait 150\nboardselectionvisible\nboard\nwait 150\nboardbounds\nshot overlay',visual,face=face)
many=copy.deepcopy(prefs);many['pinned_tabs']=[dict(id=f'pin-{i}',title=f'Page {i}',target={'Page':{'url':f'http://127.0.0.1:1/never-open-{i}','container':'PERSONAL'}}) for i in range(30)]
run('pins-overflow','sidebarwidth 248\nwait 150\npinsbounds\npinsscroll\nwait 150\npinsbounds\nasserttabs 1\nshot scrolled-pins',many)
run('pins-compact','sidebarwidth 80\nwait 150\npinsbounds\nshot compact\npinclick Edit\nwait 150\npinsbounds\nshot edit-expanded',prefs)
# Private home uses the actual incognito launch path, never opens a website.
for width,face in [(900,'paper'),(480,'ink')]:
    d=root/f'private-{width}-{face}';(d/'profile').mkdir(parents=True)
    private_prefs=copy.deepcopy(prefs);private_prefs['window_rect']=[80,80,width*2,680*2]
    (d/'profile/settings.json').write_text(json.dumps(private_prefs));(d/'profile/onboarded').write_text('skip')
    shot=d/'check.shot';shot.write_text(f'wait 1200\nwindow {width} 680\nwait 200\nassertpane home\npinsassert \nshot fedora\n')
    env=dict(os.environ,NUS_SHOT=str(shot),NUS_SHOT_DIR=str(d),NUS_SHOT_OUT=str(d/'screens'),NUS_MODE=face,NUS_PRIVATE_LOOK=json.dumps(private_prefs))
    with (d/'run.log').open('w') as log:
        result=subprocess.run([str(bundle/'Contents/MacOS/nus'),'--incognito'],env=env,stdout=log,stderr=subprocess.STDOUT,timeout=60)
    assert result.returncode==0,(d/'run.log').read_text()[-2500:]
    print('PASS',d.name,flush=True)
print('Pinned tabs, onboarding, private wordmark, and ports layout checks passed.',flush=True)
