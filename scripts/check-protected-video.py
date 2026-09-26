#!/usr/bin/env python3
"""Protected video stays off disk: `nus page screenshot` refuses a page whose
video has MediaKeys (EME, here ClearKey, built into Chromium), keeps refusing
after the video goes and the address changes in place, and captures again
once a new document loads. Only a loopback HTTP fixture is visited."""
import http.server
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import time

bundle = Path(sys.argv[1] if len(sys.argv) > 1 else 'dist/nus.app').resolve()
exe = bundle/'Contents/MacOS/nus' if sys.platform == 'darwin' else bundle
cli = bundle/'Contents/Resources/bin/nus'
if not cli.is_file(): cli = Path(__file__).resolve().parents[1]/'target/debug/nus'
assert cli.is_file(), 'Build nus-cli (or the bundle) before running this check'
root = Path(tempfile.mkdtemp(prefix='nus-protected-video-check-'))
print(f'Evidence: {root}', flush=True)

# Keys on a video, then (2.5 s) the video goes and the address changes in
# place, then (5 s) a new document. The title says which stage is showing.
KEYED = b'''<!doctype html><title>loading</title><video muted></video><script>
const v=document.querySelector('video');
navigator.requestMediaKeySystemAccess('org.w3.clearkey',[{initDataTypes:['keyids'],videoCapabilities:[{contentType:'video/webm; codecs="vp8"'}]}])
  .then(a=>a.createMediaKeys()).then(k=>v.setMediaKeys(k)).then(()=>{
    document.title='keyed';
    setTimeout(()=>{v.remove();history.pushState({},'','/keyed#spa');document.title='spa';},2500);
    setTimeout(()=>{location.href='/clear';},5000);
  }).catch(e=>{document.title='eme failed: '+e.name+' '+e.message;});
</script>'''
CLEAR = b'<!doctype html><title>clear</title><h1>Nothing protected here</h1>'

class Fixture(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        body = KEYED if self.path.startswith('/keyed') else CLEAR
        self.send_response(200)
        self.send_header('Content-Type', 'text/html')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)
    def log_message(self, *_): pass

server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Fixture)
threading.Thread(target=server.serve_forever, daemon=True).start()
base = f'http://127.0.0.1:{server.server_port}'

directory = root/'protected'
profile = directory/'profile'
profile.mkdir(parents=True)
(profile/'onboarded').write_text('skip')
(profile/'settings.json').write_text(json.dumps({'schema':2, 'behavior':{'splash':'None','then':'Prompt','hatch_background':False}}))
script = root/'protected.shot'
script.write_text(f'wait 1600\ntab {base}/keyed\nwait 30000\n')
env = dict(os.environ, NUS_SHOT=str(script), NUS_SHOT_DIR=str(directory), NUS_SHOT_OUT=str(root), NUS_MODE='paper')
env.pop('NUS_SHOT2', None)
cli_env = dict(env, NUS_INSTANCE=str(profile/'instance'))

def nus(*args):
    r = subprocess.run([str(cli), *args, '--json'], env=cli_env, capture_output=True, text=True, timeout=10)
    return r.returncode, r.stdout, r.stderr.strip()

def title_is(want, within):
    until, seen = time.monotonic()+within, None
    while time.monotonic() < until:
        code, out, _ = nus('page', 'info')
        if code == 0:
            seen = json.loads(out).get('title')
            if seen == want: return
            assert not str(seen).startswith('eme failed'), seen
        time.sleep(.1)
    raise AssertionError(f'page never showed {want!r} (last {seen!r})')

def refused():
    code, out, err = nus('page', 'screenshot')
    if code == 0: return False, out
    assert 'protected video' in err, f'refused for another reason: {err}'
    return True, err

log = root/'protected.log'
with log.open('w') as output:
    process = subprocess.Popen([str(exe)], cwd=directory, env=env, stdout=output, stderr=subprocess.STDOUT)
    try:
        until = time.monotonic()+15
        while not (profile/'instance').exists() and process.poll() is None and time.monotonic() < until: time.sleep(.05)
        assert (profile/'instance').exists(), 'no instance file: remote control never came up'

        title_is('keyed', 15)
        # The report runs every 100 ms; give the flag a moment to arrive.
        until, answer = time.monotonic()+2, None
        while time.monotonic() < until:
            is_refused, answer = refused()
            if is_refused: break
            time.sleep(.1)
        assert is_refused, f'screenshot of a keyed page was taken: {answer}'
        print('PASS keyed: screenshot refused', flush=True)

        title_is('spa', 6); time.sleep(.4)
        is_refused, answer = refused()
        assert is_refused, f'the flag let go within the same document: {answer}'
        print('PASS same document: still refused after the video left and the address changed in place', flush=True)

        title_is('clear', 8); time.sleep(.4)
        is_refused, answer = refused()
        assert not is_refused, answer
        shot = Path(json.loads(answer)['path'])
        assert shot.is_file() and shot.stat().st_size > 0, shot
        print(f'PASS new document: captured again ({shot.name})', flush=True)
        process.wait(timeout=60)
    except BaseException:
        process.kill(); process.wait(); raise
text = log.read_text()
assert process.returncode == 0 and 'panicked' not in text, text[-5000:]
print('Protected video checks passed.', flush=True)
