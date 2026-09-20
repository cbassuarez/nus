#!/usr/bin/env python3
"""Modular drawer: native controls, real PTY work and a real Chromium transfer.

Uses temporary profiles. Does not inject OS input or alter the user's settings.
"""
from pathlib import Path
exec(compile(Path('scripts/check-settings.py').read_text().split('base=run(')[0], 'check-settings.py', 'exec'))
import http.server
import threading
import time

profile = run('empty', 'startpage prompt\nmenudrawer\nwait 500\nmenuassert visible\nassertnoshells\nmenushot empty\nmenuclick Close\nmenuassert hidden\nmenutray\nwait 300\nmenuassert visible\nmenutray\nwait 200\nmenuassert hidden\nassertnoshells')
prefs = json.loads((profile / 'settings.json').read_text())
prefs['window_rect'] = [80, 80, 1600, 1100]
prefs['behavior']['splash'] = 'None'
prefs['motion']['reduce'] = True
prefs['sidebar_pinned'] = True

def choose(hit):
    return f'settingsscroll 0\nwait 100\nsettingseek {hit}\nwait 100\nsettingclick {hit}\nwait 250\nassertchoice {hit}\nsettingsbounds\n'

steps = 'settingsat 17\nwait 100\nshot settings-top\n'
for hit in ['MenuSignal(Count)', 'MenuSignal(Text)', 'MenuSignal(Dot)',
            'MenuDensity(Work, Expanded)', 'MenuDensity(Downloads, Compact)',
            'MenuDensity(Shortcuts, Hidden)', 'MenuNames(false)', 'MenuRecent(false)',
            'MenuEnabled(false)']:
    steps += choose(hit)
steps += 'settingseek MenuMove(Downloads, false)\nwait 100\nsettingclick MenuMove(Downloads, false)\nwait 200\nshot settings-order\nmenudrawer\nwait 300\nmenuassert visible\nassertnoshells\nmenushot configured\nmenuclick Customize\nwait 200\nassertpane settings\n'
configured = run('configuration', steps, prefs)
saved = json.loads((configured / 'settings.json').read_text())
config = saved['behavior']['menu_drawer']
assert config['sections'] == [dict(module='Downloads', density='Compact'), dict(module='Work', density='Expanded'), dict(module='Shortcuts', density='Hidden')]
assert not config['names'] and not config['recent'] and not config['enabled']
run('relaunch', 'settingsat 17\nwait 100\nassertchoice MenuDensity(Work, Expanded)\nassertchoice MenuNames(false)\nassertchoice MenuEnabled(false)\nmenudrawer\nwait 300\nmenuassert visible\nassertnoshells', saved)

all_hidden = copy.deepcopy(saved)
for section in all_hidden['behavior']['menu_drawer']['sections']:
    section['density'] = 'Hidden'
run('hidden-sections', 'menudrawer\nwait 300\nmenuassert visible\nmenushot empty-custom\nmenuclick Customize\nwait 200\nassertpane settings\nassertnoshells', all_hidden)

class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *args): pass
    def do_GET(self):
        if self.path == '/':
            body = b'<title>Drawer transfer check</title><p>Local fixture</p>'
            self.send_response(200)
            self.send_header('Content-Type', 'text/html')
        else:
            body = b'NUS DRAWER CHECK\n' * 100000
            self.send_response(200)
            self.send_header('Content-Type', 'application/octet-stream')
            self.send_header('Content-Disposition', 'attachment; filename="Private report.pdf"')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        try:
            for i in range(0, len(body), 8192):
                self.wfile.write(body[i:i+8192]); self.wfile.flush()
                if self.path != '/': time.sleep(.035)
        except (BrokenPipeError, ConnectionResetError): pass

server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
threading.Thread(target=server.serve_forever, daemon=True).start()
url = f'http://127.0.0.1:{server.server_port}'
steps = f'''newshell
wait 800
hatchname Private project
shell sleep 45
wait 400
tab {url}/
wait 1200
download {url}/slow
wait 700
downloadassert active
menudrawer
wait 400
menuassert visible
menushot mixed-live
menuclick Download(Pause
wait 300
downloadassert paused
menushot paused
menuclick Download(Resume
wait 300
menuclick Work(
wait 300
assertpane term
settingsat 17
wait 100
'''
steps += choose('MenuNames(false)')
steps += '''menudrawer
wait 300
menuprivacy Private
menushot private
theme nord
appearance ink
wait 500
menushot nord-ink
menuclick Downloads
wait 300
assertpane downloads
wait 9500
downloadassert complete
menudrawer
wait 300
menushot finished
menuclick Close
menuassert hidden
ctrlc
'''
try:
    live = run('live-work-download', steps, prefs)
    records = json.loads((live / 'downloads.json').read_text())
    assert len(records) == 1 and records[0]['done']
    assert Path(records[0]['path']).read_bytes().startswith(b'NUS DRAWER CHECK')
finally:
    server.shutdown()

for width in [80, 248, 320]:
    run(f'footer-{width}', f'sidebarwidth {width}\nwait 200\nsidebarcheck\nmenufooter\nwait 300\nmenuassert visible\nassertnoshells\nmenushot footer-{width}', prefs)
print('Menu drawer checks passed.', flush=True)
