#!/usr/bin/env python3
"""Native lifecycle/pressure regression. Never exhaust the host to induce pressure.

Uses the same reclamation path as OS notifications, then exercises real CEF
creation/destruction and resizing. Measurements are opt-in, local, and contain
no page contents. RSS includes shared pages; macOS phys_footprint is also sampled.
"""
import http.server
import json
import os
from pathlib import Path
import statistics
import subprocess
import sys
import tempfile
import threading

bundle = Path(sys.argv[1] if len(sys.argv) > 1 else '/tmp/nus-preview9.app').resolve()
root = Path(tempfile.mkdtemp(prefix='nus-memory-check-'))
print(f'Evidence: {root}', flush=True)
page = b'''<!doctype html><title>Memory fixture</title><input id="work"><canvas width="800" height="600"></canvas>
<script>window.retained={note:'unsaved work',count:0};const ctx=document.querySelector('canvas').getContext('2d');
function draw(){retained.count++;ctx.fillStyle='hsl('+retained.count%360+' 60% 50%)';ctx.fillRect(0,0,800,600);requestAnimationFrame(draw)}draw();</script>'''
class Page(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_response(200)
        self.send_header('Content-Type', 'text/html')
        self.send_header('Content-Length', str(len(page)))
        self.end_headers()
        self.wfile.write(page)
    def log_message(self, *args): pass

server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Page)
threading.Thread(target=server.serve_forever, daemon=True).start()
url = f'http://127.0.0.1:{server.server_port}/'
profile = root / 'profile'
profile.mkdir()
(profile / 'onboarded').write_text('skip')
(profile / 'settings.json').write_text(json.dumps({'behavior': {'splash': 'None', 'then': 'Prompt', 'update_checks': False}}))
steps = ['wait 2000', 'home', f'tab {url}', 'wait 1500', 'assertbrowsers 1',
         "eval document.getElementById('work').value='Still here'; retained.count>0",
         'wait 150', 'assertreply true', 'memory pressure-before',
         'memorypressure warning', 'wait 500', 'memorypressure critical', 'wait 700',
         'assertbrowsers 1', f'asserturl {url}',
         "eval document.getElementById('work').value + ' / ' + retained.note + ' / ' + (retained.count>0)",
         'wait 150', 'assertreply Still here / unsaved work / true', 'memory pressure-after',
         'appmenu CloseTab', 'wait 700', 'assertbrowsers 0']
for i in range(1, 65):
    steps += [f'tab {url}?cycle={i}', 'wait 400', 'assertbrowsers 1']
    if i % 8 == 0:
        steps += [f'windowsize {1000 + i} {720 + i}', 'memorypressure warning']
    steps += ['appmenu CloseTab', 'wait 300', 'assertbrowsers 0']
    if i % 4 == 0: steps += [f'memory closed-{i}']
steps += ['wait 2500', 'assertbrowsers 0', 'memory settled']
shot = root / 'memory.shot'
shot.write_text('\n'.join(steps) + '\n')
env = dict(os.environ, NUS_SHOT=str(shot), NUS_SHOT_DIR=str(root), NUS_SHOT_OUT=str(root/'screens'), NUS_SHOT_SIZE='1100x800')
env.pop('NUS_SHOT2', None)
try:
    with (root/'run.log').open('w') as out:
        result = subprocess.run([str(bundle/'Contents/MacOS/nus')], env=env, stdout=out, stderr=subprocess.STDOUT, timeout=240)
finally:
    server.shutdown()
log = (root/'run.log').read_text()
if result.returncode or 'panicked' in log:
    raise RuntimeError(log[-5000:])
samples = []
for line in log.splitlines():
    if line.startswith('MEMORY '):
        _, label, data = line.split(' ', 2)
        samples.append({'label': label, **json.loads(data)})
closed = [s for s in samples if s['label'].startswith('closed-')]
assert len(closed) == 16, 'missing lifecycle memory samples'
summary = {}
for key in ['main_rss_kib', 'tree_rss_kib', 'main_footprint_kib', 'tree_footprint_kib']:
    values = [s[key] for s in closed if s.get(key) is not None]
    if len(values) == 16:
        summary[key] = {'early_median': statistics.median(values[2:6]), 'late_median': statistics.median(values[-4:]), 'peak': max(values), 'settled': samples[-1][key]}
        summary[key]['growth'] = summary[key]['late_median'] - summary[key]['early_median']
(root/'memory.json').write_text(json.dumps({'cycles': 64, 'summary': summary, 'samples': samples, 'limits': 'Debug native macOS bundle. Controlled canvas fixture; simulated reclamation, not real memory exhaustion. Does not establish the cause of the reported system-wide spike.'}, indent=2))
print(json.dumps(summary, indent=2), flush=True)
# A gross regression gate, not a claim that arbitrary sites obey this budget.
for key in ['main_footprint_kib', 'tree_footprint_kib']:
    if key in summary: assert summary[key]['growth'] < 64 * 1024, (key, summary[key])
print('PASS: 64 browser lifecycles, resize, pressure reclamation, and retained form/JS state', flush=True)
