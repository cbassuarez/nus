#!/usr/bin/env python3
"""Real PTYs, native Hatch controls and rendered evidence; isolated profiles.

Usage: python3 scripts/check-hatch.py /path/to/nus.app
No input is injected into other applications. Commands only run in test shells.
"""
from pathlib import Path
exec(compile(Path('scripts/check-settings.py').read_text().split("base=run(")[0], 'check-settings.py', 'exec'))

profile=run('empty', 'hatchsize 40\nwait 500\nhatchwork\nwait 500\nassertnoshells\nhatchassertvisible true\nhatchshot empty')
prefs=json.loads((profile/'settings.json').read_text())
prefs['behavior'].update(splash='None',hatch_autohide=False,hatch_status=True,hatch_background=True,hatch_spaces='One')
prefs['motion']['reduce']=True
prefs['window_rect']=[80,80,1200,900]
steps=r'''newshell
wait 800
hatchname Failed build
shell sleep 1; printf 'HATCH_FAILURE\n'; false
wait 1800
hatchassertstatus Failed
newshell
wait 800
hatchname Finished tests
shell sleep 3; printf 'HATCH_SUCCESS\n'
wait 3800
hatchassertstatus Finished
newshell
wait 800
hatchname Running task
shell sleep 30
wait 500
hatchassertstatus Running
newshell
wait 800
hatchname Agent attention
shell sleep 0.5; printf '\a'; sleep 30
home
wait 1000
hatchassertstatus Attention
hatchwork
wait 500
hatchshot work
hatchcheck
wait 400
hatchshot live-terminal
hatchassertnotch
hatchclick work
wait 200
hatchclick pin
wait 200
hatchshot pinned-work
hatchhide
wait 200
hatchassertvisible false
wait 250
hatchbadgeshot compact-island
hatchshow
wait 200
hatchassertvisible true
hatchsize 60
wait 400
asserthatchsize 60
hatchshot modal
hatchbackground
wait 300
hatchassertbackground
hatchassertvisible false
hatchwork
wait 300
hatchassertvisible true
hatchshot background-work'''
run('real-work',steps,prefs)
run('narrow-ink',r"""newshell
wait 800
hatchname Long named project session
shell sleep 30
wait 300
hatchwork
wait 400
hatchnarrow
wait 400
hatchassertnarrow
hatchshot narrow-work
hatchopen Long named project session
wait 400
hatchnarrow
wait 400
hatchassertnarrow
hatchshot narrow-terminal
hatchoptionscheck
wait 400
hatchassertshade
hatchshot dim-modal
hatchhide
wait 400
hatchassertquiet
newshell
wait 800
shell sleep 3; false
wait 600
hatchbackground
wait 3300
hatchassertnotice
wait 6200
hatchassertquiet""",prefs,face='ink')
run('cross-space',r"""newshell
wait 800
hatchname Primary session
shell sleep 30
newwindow
wait 3500
hatchwork
wait 300
hatchopen Second session
wait 300
hatchtoggle
wait 300
hatchtoggle
wait 1100
hatchopen Primary session
wait 400
hatchassertsession Primary session
hatchassertvisible true
hatchshot returned-primary
wait 2000""",prefs,second=r"""wait 1200
newshell
wait 800
hatchname Second session
shell sleep 30
wait 2700
hatchassertsession Second session
hatchassertvisible true
hatchshot selected-secondary
wait 2000
hatchassertvisible false
wait 8000""")
print('Hatch native checks passed.',flush=True)
