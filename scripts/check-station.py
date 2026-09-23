#!/usr/bin/env python3
"""Station native rendering, independent row cascades, and port details."""
from pathlib import Path
exec(compile(Path(__file__).with_name('check-settings.py').read_text().split('base=run(')[0], 'check-settings.py', 'exec'))
os.environ['NUS_PORTS_FIXTURE']='1'
try:
    for face,width,reduced in [('paper',1200,False),('ink',760,True)]:
        prefs={'window_rect':[60,60,width,960],'motion':{'reduce':reduced},'behavior':{'splash':'None','update_checks':False}}
        run('station-'+face,'''mecard import
reelat 0
wait 300
shot station-import
reelat 2.05
wait 100
shot station-import-turn
closeprofile
boardfixture
board
wait 2200
boardbounds
shot station-ports
stationchange
wait 230
stationcheck
shot station-ports-turn
wait 1800
boarddetail
wait 100
boardbounds
shot station-details
''',prefs,face=face)
finally:
    for marker in root.glob('*/profile/.vault-id'):
        subprocess.run(['/usr/bin/security','delete-generic-password','-s','dev.nus.local-state.v1','-a',marker.read_text()],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
