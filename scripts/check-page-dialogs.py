#!/usr/bin/env python3
"""A page's questions and a site's sign-in, end to end against a local
server: the sheet hangs from the strip, Enter is held after it appears, the
page gets no keys while it stands, a frame is named for itself, a page can be
told to stop asking, and the password never sits in the cloned page."""
import base64
import http.server
import os
from pathlib import Path
import socketserver
import subprocess
import sys
import tempfile
import threading

bundle = Path(sys.argv[1] if len(sys.argv) > 1 else 'dist/nus.app').resolve()
root = Path(tempfile.mkdtemp(prefix='nus-page-dialogs-'))
print(f'Evidence: {root}', flush=True)

ASK = '''<!doctype html><title>asking</title><input id=k autofocus>
<script>setTimeout(()=>{const w='%s';let r;
if(w==='confirm')r=confirm('nus: your session expired. Sign in again to keep your tabs.');
else if(w==='prompt')r=prompt('What should we call you?','sam');
else r=alert('hello from the page');
document.title='answered:'+r},500)</script>'''
SPAM = '''<!doctype html><title>spam</title><script>let n=0;
function go(){n++;alert('ask number '+n);if(n<6)setTimeout(go,50);else document.title='done:'+n}
setTimeout(go,500)</script>'''


class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def send(self, body, code=200, headers=()):
        data = body.encode()
        self.send_response(code)
        self.send_header('Content-Type', 'text/html; charset=utf-8')
        self.send_header('Content-Length', str(len(data)))
        for k, v in headers:
            self.send_header(k, v)
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        path, _, query = self.path.partition('?')
        if path == '/ask':
            self.send(ASK % query.partition('=')[2])
        elif path == '/spam':
            self.send(SPAM)
        elif path == '/frame':
            self.send(f'<!doctype html><title>frame</title><h1>the page</h1><iframe src="http://localhost:{PORT}/ask?what=alert"></iframe>')
        elif path == '/secret':
            want = 'Basic ' + base64.b64encode(b'sam:hunter2').decode()
            if self.headers.get('Authorization') == want:
                self.send('<!doctype html><title>in</title><p id=w>welcome sam</p>')
            else:
                self.send('<p>no</p>', 401, [('WWW-Authenticate', 'Basic realm="Reports"')])
        else:
            self.send('<p>?</p>', 404)


server = socketserver.ThreadingTCPServer(('127.0.0.1', 0), Handler)
PORT = server.server_address[1]
threading.Thread(target=server.serve_forever, daemon=True).start()
BASE = f'http://127.0.0.1:{PORT}'


def run(name, steps, face='paper'):
    directory = root / name
    profile = directory / 'profile'
    profile.mkdir(parents=True)
    (profile / 'onboarded').write_text('skip')
    script = directory / 'check.shot'
    script.write_text('wait 1600\n' + '\n'.join(steps) + '\n')
    env = dict(os.environ, NUS_SHOT_DIR=str(directory), NUS_SHOT=str(script), NUS_SHOT_OUT=str(directory / 'screens'), NUS_MODE=face)
    env.pop('NUS_SHOT2', None)
    log = directory / 'run.log'
    with log.open('w') as out:
        result = subprocess.run([str(bundle / 'Contents/MacOS/nus')], env=env, stdout=out, stderr=subprocess.STDOUT, timeout=120)
    text = log.read_text()
    if result.returncode or 'panicked' in text:
        raise RuntimeError(f'{name}: {log}\n{text[-3500:]}')
    # The password, plain or in the header, anywhere but the runner's echo
    # of the step that typed it.
    app_lines = '\n'.join(l for l in text.splitlines() if not l.startswith('shot: input '))
    for word in ('hunter2', 'aHVudGVyMg'):
        assert word not in app_lines, f'{name}: the password reached the log'
    print('PASS', name, flush=True)


run('confirm', [
    f'url {BASE}/ask?what=confirm', 'awaitdialog',
    # Held: an Enter already on its way does nothing.
    'assertdialogheld true', 'key enter', 'assertdialog 127.0.0.1',
    # Typing goes nowhere: the page's focused input stays empty.
    'input abc', 'shot confirm', 'wait 650', 'assertdialogheld false',
    'key enter', 'wait 300', 'assertnodialog',
    "eval document.getElementById('k').value+'|'+document.title", 'wait 400', 'assertreply |answered:false',
])
run('prompt', [
    f'url {BASE}/ask?what=prompt', 'awaitdialog', 'wait 650', 'input !', 'shot prompt',
    'key enter', 'wait 300', 'assertnodialog',
    'eval document.title', 'wait 400', 'assertreply answered:sam!',
])
run('frame', [
    f'url {BASE}/frame', 'awaitdialog', 'assertdialog A frame from localhost', 'shot frame',
    'key esc', 'wait 300', 'assertnodialog',
])
run('spam', [
    f'url {BASE}/spam',
    'awaitdialog', 'wait 650', 'key enter', 'wait 150',
    'awaitdialog', 'wait 650', 'key enter', 'wait 150',
    'awaitdialog', 'wait 650', 'shot spam-third', 'dialogclick stop', 'wait 600',
    'assertnodialog', 'eval document.title', 'wait 400', 'assertreply done:6',
])
run('signin', [
    f'url {BASE}/secret', 'awaitdialog', 'assertdialog Sign in to 127.0.0.1', 'wait 650',
    'input sam', 'key tab', 'input hunter2', 'assertsecret 7', 'shot signin',
    'key enter', 'wait 1500', 'assertnodialog',
    "eval document.getElementById('w').textContent", 'wait 400', 'assertreply welcome sam',
])
run('signin-ink', [f'url {BASE}/secret', 'awaitdialog', 'wait 650', 'input sam', 'shot signin-ink'], face='ink')
print('Page dialog checks passed.', flush=True)
