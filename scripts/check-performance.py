#!/usr/bin/env python3
"""Release-app measurements on macOS, in disposable profiles.

CPU submission timings are not physical key-to-screen latency. RSS includes
shared pages and is not private physical footprint. Keep the JSON alongside
the machine/OS/display details before comparing runs. No production profile is
opened. The generated large fixtures are removed after successful validation.
"""
from pathlib import Path
import atexit
exec(compile(Path('scripts/check-settings.py').read_text().split('base=run(')[0], 'settings-harness', 'exec'))
os.environ['NUS_PERF']='1'
_harness_run=run
def run(name, steps, *args, **kwargs):
    profile=_harness_run(name, steps+'\nperfstats __complete__', *args, **kwargs)
    log=(profile.parent/'run.log').read_text()
    if not any(line.startswith('PERF __complete__ ') for line in log.splitlines()):
        raise RuntimeError(f'{name}: application exited before completing the measurement script: {profile.parent / "run.log"}')
    return profile
fixtures=[]
def remove_fixtures():
    # Failed assertions must not leave hundreds of megabytes behind either.
    for path in fixtures:
        path.unlink(missing_ok=True)
atexit.register(remove_fixtures)
base=run('preferences','settingscheck\nhome\nassertpane home')
prefs=json.loads((base/'settings.json').read_text())
prefs['behavior'].update(splash='None',home_look='Line',new_window='Prompt',then='Prompt',keep_alive='Off',close_asks=False)
prefs['motion']['reduce']=True
prefs['window_rect']=[60,60,1100,800]

for i in range(3):
    run(f'startup-{i}','home\nwait 300\nperfstats startup\nmemory idle',prefs)

for mib in [10,100]:
    path=root/f'file-{mib}m.txt'; fixtures.append(path)
    chunk=b'A bounded editor fixture with commands and output.\n'*4096
    with path.open('wb') as f:
        left=mib*1024*1024
        while left:
            n=min(left,len(chunk));f.write(chunk[:n]);left-=n
    opens=['home','perfreset']
    for _ in range(19 if mib==10 else 4):
        # Readiness assertion is deliberately separate from the measured
        # latency budget. A slow host should produce a slow result, not erase
        # the report before its memory/window cases run.
        opens += [f'openfile {path}', 'wait 1000', f'asserteditorready {mib*1024*1024}', 'appmenu CloseTab', 'wait 100']
    run(f'file-{mib}m','\n'.join(opens)+f'''
openfile {path}
wait 1400
asserteditorready {mib*1024*1024}
perfstats file-open
editorcursor end
key x
wait 80
asserteditorready {mib*1024*1024+1}
key cmd+z
wait 80
asserteditorready {mib*1024*1024}
shot editor-{mib}m
perfreset
editorfind unique-needle-not-in-this-file
home
'''+ '\n'.join('key x\nwait 3' for _ in range(60)) + '\nperfstats background-search\nmemory file',prefs)

# One huge line must not allocate/shape the entire line or block End/typing.
long=root/'long-line.txt';fixtures.append(long)
with long.open('wb') as f:
    for _ in range(100):f.write(b'x'*(1024*1024))
run('long-line',f'''home
openfile {long}
wait 1500
asserteditorready {100*1024*1024}
perfreset
editorcursor end
key y
wait 50
asserteditorready {100*1024*1024+1}
perfstats long-line-edit
shot long-line''',prefs)

# Browser contents are deliberately identical; this measures nus/browser
# overhead, not arbitrary site workloads.
from http.server import ThreadingHTTPServer, SimpleHTTPRequestHandler
from functools import partial
from threading import Thread
(root/'idle.html').write_text('<!doctype html><title>Idle fixture</title><p>An idle browser tab.</p>')
server=ThreadingHTTPServer(('127.0.0.1',0),partial(SimpleHTTPRequestHandler,directory=str(root)))
Thread(target=server.serve_forever,daemon=True).start()
url=f'http://127.0.0.1:{server.server_port}/idle.html'
steps=['home','wait 300','memory tabs-0']
for i in range(1,9):
    steps += [f'tab {url}', 'wait 500']
    if i in [1,2,4,8]:steps += [f'assertbrowsers {i}',f'memory tabs-{i}']
for i in range(8):steps+=['appmenu CloseTab','wait 200']
steps += ['wait 800','assertbrowsers 0','memory tabs-closed']
run('tabs','\n'.join(steps),prefs)
server.shutdown()
steps=['home','wait 300','memory windows-1']
for i in range(2,5):steps += ['newwindow','wait 1600',f'memory windows-{i}']
run('windows','\n'.join(steps),prefs)

import platform
measurements={}
for log in root.glob('*/run.log'):
    case=[]
    for line in log.read_text().splitlines():
        if line.startswith(('PERF ','MEMORY ')) and not line.startswith('PERF __complete__ '):
            kind,label,value=line.split(' ',2)
            case.append({'kind':kind,'label':label,'value':json.loads(value)})
    if case:measurements[log.parent.name]=case
report={'platform':platform.platform(),'machine':platform.machine(),'window_logical':[1100,800],'measurements':measurements,
        'limits':'Fresh processes; filesystem caches uncontrolled. CPU frame submission is not display presentation; RSS sums count shared pages per process.'}
try:report['hardware']=subprocess.check_output(['sysctl','-n','hw.model','hw.memsize','hw.ncpu','machdep.cpu.brand_string'],text=True).splitlines()
except (OSError,subprocess.CalledProcessError):pass
(root/'performance.json').write_text(json.dumps(report,indent=2))
for path in fixtures:path.unlink()
print(f'Measurements: {root / "performance.json"}',flush=True)
