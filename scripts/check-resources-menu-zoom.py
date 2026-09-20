#!/usr/bin/env python3
"""Native menu, zoom and browser teardown regressions in disposable profiles."""
from pathlib import Path
exec(compile(Path('scripts/check-settings.py').read_text().split('base=run(')[0], 'settings-harness', 'exec'))

base=run('preferences','settingscheck\nhome\nassertpane home')
prefs=json.loads((base/'settings.json').read_text())
prefs['behavior'].update(splash='None',home_look='Line',new_window='Prompt',then='Prompt',keep_alive='Off',close_asks=False)
prefs['motion']['reduce']=True
prefs['window_rect']=[60,60,1100,800]
# The menu steps ask AppKit to perform the real NSMenuItem action. They then
# wait for its native callback to reach the event loop, rather than calling run().
run('native-menus-and-ui-zoom','''home
assertbrowsers 0
key cmd+plus
assertzoom 110
key cmd+-
assertzoom 100
appmenu ZoomIn
wait 120
assertzoom 110
appmenu ZoomReset
wait 120
assertzoom 100
appmenu Settings
wait 180
assertpane settings
key cmd+plus
assertzoom 110
shot settings-zoom
key cmd+0
assertzoom 100
appmenu Downloads
wait 180
assertpane downloads
appmenu Home
wait 180
assertpane home
key f10
wait 100
shot application-menu
key escape
appmenu NewShell
wait 600
assertpane term
key cmd+plus
assertzoom 110
appmenu ZoomIn
wait 100
assertzoom 125
key cmd+-
assertzoom 110
key cmd+0
assertzoom 100
shell printf 'zoom-shell-intact\\n'
wait 200
appmenu CloseTab
wait 200
assertpane home''',prefs)

# Locally generated browser fixture: exercise a focused frame and verify edits.
page=root/'editing.html'
page.write_text('<!doctype html><title>Resource check</title><textarea id="field">copy from the web frame</textarea><script>field.focus()</script>')
from http.server import ThreadingHTTPServer, SimpleHTTPRequestHandler
from functools import partial
from threading import Thread
server=ThreadingHTTPServer(('127.0.0.1',0),partial(SimpleHTTPRequestHandler,directory=str(root)))
Thread(target=server.serve_forever,daemon=True).start()
page_url=f'http://127.0.0.1:{server.server_port}/editing.html'
run('web-menu-and-zoom',f'''home
tab {page_url}
wait 700
assertbrowsers 1
key cmd+plus
assertzoom 110
appmenu ZoomIn
wait 100
assertzoom 125
appmenu ZoomOut
wait 100
assertzoom 110
appmenu ZoomReset
wait 100
assertzoom 100
eval document.getElementById('field').select(); 'selected'
wait 150
appmenu Copy
wait 150
eval document.getElementById('field').value='';document.getElementById('field').focus(); 'cleared'
wait 100
appmenu Paste
wait 150
eval document.getElementById('field').value
wait 150
assertreply copy from the web frame
appmenu Reload
wait 400
assertbrowsers 1
appmenu CloseTab
wait 400
assertbrowsers 0
tab {page_url}
wait 500
appmenu ZoomIn
wait 100
assertzoom 110
appmenu CloseTab
wait 200
tab {page_url}
wait 500
assertzoom 110
key cmd+0
assertzoom 100
appmenu CloseTab
wait 200
assertbrowsers 0''',prefs)

steps=['home','assertbrowsers 0','resourcestats baseline']
for i in range(24):
    steps += [f'tab {page_url}', 'wait 130','assertbrowsers 1','appmenu CloseTab','wait 150','assertbrowsers 0']
    if i in (3,11,23): steps += [f'resourcestats cycle-{i+1}']
steps += ['shot after-tab-churn']
run('browser-lifecycle','\n'.join(steps),prefs)
for line in (root/'browser-lifecycle/run.log').read_text().splitlines():
    if line.startswith('RESOURCE_STATS '): print(line,flush=True)
editor=root/'zoom-fixture.txt';editor.write_text('Zoom preserves editor text and cursor.\n')
run('editor-and-hatch',f'''home
openfile {editor}
wait 300
key cmd+plus
assertzoom 110
appmenu ZoomIn
wait 100
assertzoom 125
key cmd+0
assertzoom 100
shot editor-zoom-reset
newshell
wait 500
hatchsize 40
wait 500
hatchterminal
wait 150
hatchkey cmd+plus
asserthatchzoom 110
hatchkey cmd+-
asserthatchzoom 100
hatchkey cmd+0
asserthatchzoom 100
hatchhide''',prefs)
small=copy.deepcopy(prefs);small['window_rect']=[80,80,480,780]
run('narrow-zoom','''home
key cmd+plus
key cmd+plus
key cmd+plus
key cmd+plus
key cmd+plus
assertzoom 200
assertchrome
shot home-200
key cmd+0
assertzoom 100
key f10
wait 100
shot menu-narrow''',small,face='ink')
server.shutdown()
print('Native menus, focused zoom and browser teardown checks passed.',flush=True)
