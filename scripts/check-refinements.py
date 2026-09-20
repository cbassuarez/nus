#!/usr/bin/env python3
"""Native regression checks for settings search, fonts, footer and welcome.

Uses the same temporary-profile runner as check-settings.py.
"""
from pathlib import Path
exec(compile(Path('scripts/check-settings.py').read_text().split("base=run(")[0], 'check-settings.py', 'exec'))

profile=run('bootstrap','assertplace unset\nstartpage prompt')
prefs=json.loads((profile/'settings.json').read_text())
prefs['behavior']['splash']='None'
prefs['motion']['reduce']=True
prefs['window_rect']=[80,80,1440,1100]
run('search','searchcheck\npalette settings locatoin\nassertsearch location\nshot search\nenter\nwait 200\nassertrevealed\nshot location\nassertplace unset\npalette place 35.2, -106.6\nenter\nassertplace set\npalette place\nenter\nassertplace unset\npalette settings font wieght\nassertsearch weight\nenter\nwait 200\nassertrevealed\nshot weight',prefs)
font_profile=run('fonts','looktab 3\nwait 100\nfontcheck\npalette settings interface font\nenter\nwait 100\nsettingclick UiFont(Areal)\nwait 100\nassertchoice UiFont(Areal)\nsettingclick UiWeight(Medium)\nwait 100\nassertchoice UiWeight(Medium)\nshot areal',prefs)
run('fonts-relaunch','looktab 3\nassertchoice UiFont(Areal)\nassertchoice UiWeight(Medium)',json.loads((font_profile/'settings.json').read_text()))
run('footer','sidebar\nhover 1 250\nwait 250\nfooter\nwait 300\nfootercheck\nshot nine-slots\nfooteradd\nwait 100\nfooterscroll\nwait 100\nshot more-themes',prefs)
for width,face in [(1440,'paper'),(800,'paper'),(480,'ink')]:
    p=copy.deepcopy(prefs);p['window_rect']=[80,80,width,1100]
    run(f'welcome-{width}','closeprofile\nwait 100\nshot welcome\nwelcomescroll 600\nwait 100\nshot guide',p,True,face=face)
run('rounded','radius 32\nsettingsat 3\nwait 200\nshot corners',prefs)
print('Refinement checks passed.',flush=True)
