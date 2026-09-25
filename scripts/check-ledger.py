#!/usr/bin/env python3
"""The Ledger end to end: stand-in claude, codex and aider in real shells.

claude reports through the real `nus hook` ($NUS_CLI, $NUS_PANE, the
instance socket) and then waits for one raw keypress; the check clicks
ALLOW in the sidebar and reads what the program received. codex is known
by its command line, aider by an OSC 777 notification.
"""
from pathlib import Path
exec(compile(Path(__file__).with_name('check-settings.py').read_text().split('base=run(')[0], 'check-settings.py', 'exec'))
try:
    run('ledger', 'sidebarwidth 300\nledgerprep\nwait 7000\nshot ledger\nledgercheck\nwait 1500\nledgeranswer\nshot ledger-after', {'behavior':{'splash':'None','update_checks':False}})
finally:
    for marker in root.glob('*/profile/.vault-id'):
        subprocess.run(['/usr/bin/security','delete-generic-password','-s','dev.nus.local-state.v1','-a',marker.read_text()],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
