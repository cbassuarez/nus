#!/usr/bin/env python3
"""Isolated native checks for command rerun and occupied-split tunnel actions."""
from pathlib import Path
exec(compile(Path(__file__).with_name('check-settings.py').read_text().split('base=run(')[0], 'check-settings.py', 'exec'))
os.environ['NUS_PORTS_FIXTURE']='1'
try:
    run('port-actions', 'portactionscheck\nwait 200', {'behavior':{'splash':'None','update_checks':False}})
finally:
    for marker in root.glob('*/profile/.vault-id'):
        subprocess.run(['/usr/bin/security','delete-generic-password','-s','dev.nus.local-state.v1','-a',marker.read_text()],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
