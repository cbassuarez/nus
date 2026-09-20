#!/usr/bin/env python3
"""Native privacy/security regressions. Only a loopback HTTP fixture is visited."""
import http.server
import json
import os
from pathlib import Path
import socket
import sqlite3
import subprocess
import sys
import tempfile
import threading
import time

bundle = Path(sys.argv[1] if len(sys.argv) > 1 else 'dist/nus.app').resolve()
exe = bundle/'Contents/MacOS/nus' if sys.platform == 'darwin' else bundle
root = Path(tempfile.mkdtemp(prefix='nus-privacy-check-'))
print(f'Evidence: {root}', flush=True)

class Fixture(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path == '/download':
            body = b'nus private download retained\n'
            self.send_response(200)
            self.send_header('Content-Disposition', 'attachment; filename="nus-private-check.txt"')
            self.send_header('Content-Type', 'application/octet-stream')
        else:
            body = b'<!doctype html><title>Local privacy check</title><h1>Local privacy check</h1>'
            self.send_response(200)
            self.send_header('Content-Type', 'text/html')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)
    def log_message(self, *_): pass

server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Fixture)
threading.Thread(target=server.serve_forever, daemon=True).start()
base = f'http://127.0.0.1:{server.server_port}'

def wire(port, text):
    with socket.create_connection(('127.0.0.1', port), timeout=2) as s:
        s.sendall(text.encode())
        s.shutdown(socket.SHUT_WR)
        try: return s.makefile('rb').readline(1024*1024).decode()
        except ConnectionResetError: return ''

def run(name, steps, private=False, directory=None, probe=False, second=None):
    directory = directory or root/name
    directory.mkdir(exist_ok=True)
    profile = directory/'profile'
    profile.mkdir(exist_ok=True)
    (profile/'onboarded').write_text('skip')
    if not (profile/'settings.json').exists():
        (profile/'settings.json').write_text(json.dumps({'schema':2, 'behavior':{'splash':'None','then':'Prompt','hatch_background':False}}))
    if not (profile/'sites.json').exists():
        (profile/'sites.json').write_text(json.dumps({'127.0.0.1':{'perms':{'camera':True}}}))
    script = root/(name+'.shot')
    # Leave time for the unauthenticated idle-socket probe to expire.
    script.write_text(f'wait {8000 if probe else 1600}\nkeychaincheck\n'+steps+'\n')
    env = dict(os.environ, NUS_SHOT=str(script), NUS_SHOT_DIR=str(directory), NUS_SHOT_OUT=str(root/name), NUS_MODE='paper')
    env.pop('NUS_SHOT2', None)
    if second is not None:
        second_script = root/(name+'-second.shot')
        second_script.write_text('wait 500\n'+second+'\n')
        env['NUS_SHOT2'] = str(second_script)
    env.pop('NUS_REMOTE_DEBUGGING_PORT', None)
    log = root/(name+'.log')
    with log.open('w') as output:
        process = subprocess.Popen([str(exe)]+(['--incognito'] if private else []), cwd=directory, env=env, stdout=output, stderr=subprocess.STDOUT)
        try:
            if probe:
                until = time.monotonic()+15
                while not (profile/'instance').exists() and process.poll() is None and time.monotonic() < until: time.sleep(.05)
                port, token = (profile/'instance').read_text().splitlines()[:2]
                port = int(port)
                assert len(token) == 64
                if os.name != 'nt': assert (profile/'instance').stat().st_mode & 0o777 == 0o600
                assert wire(port, 'raise\n') == ''
                denied = json.loads(wire(port, json.dumps({'token':'wrong','cmd':'ls'})+'\n'))
                assert denied['ok'] is False
                accepted = json.loads(wire(port, json.dumps({'token':token,'cmd':'ls'})+'\n'))
                assert accepted['ok'] is True
                cli = Path(__file__).resolve().parents[1]/'target/debug/nus'
                assert cli.is_file(), 'Build nus-cli before running this check'
                cli_result = subprocess.run([str(cli),'ls','--json'], env=dict(env, NUS_INSTANCE=str(profile/'instance')), capture_output=True, text=True, timeout=5, check=True)
                assert isinstance(json.loads(cli_result.stdout)['tabs'], list)
                # Large, unauthenticated input must close without a command.
                try: assert wire(port, 'x'*(1024*1024+1)+'\n') == ''
                except (BrokenPipeError, ConnectionResetError): pass
                with socket.create_connection(('127.0.0.1', port), timeout=7) as idle:
                    idle.sendall(b'{')
                    assert idle.recv(1) == b'', 'Unauthenticated idle input did not expire'
                assert process.poll() is None, 'Idle socket closed only because the app exited'
                # Query this process's listeners, without opening any unrelated port.
                if sys.platform == 'darwin':
                    sockets = subprocess.run(['lsof','-nP','-a','-p',str(process.pid),'-iTCP','-sTCP:LISTEN'], capture_output=True, text=True).stdout
                    assert ':9229 ' not in sockets, sockets
            process.wait(timeout=70)
        except BaseException:
            process.kill(); process.wait(); raise
    text = log.read_text()
    assert process.returncode == 0 and 'panicked' not in text, text[-5000:]
    if private:
        temporary = Path((directory/'private-root.txt').read_text())
        assert not temporary.exists(), f'Private root was not removed: {temporary}'
    print('PASS', name, flush=True)
    return directory

try:
    regular = run('regular', f'''supportcheck
shot reporting
tab {base}/normal
wait 1100
permissioncheck {base}
shot origin-permissions
eval document.cookie='regular=kept; path=/; max-age=3600'; localStorage.setItem('normal','kept'); 'stored'
wait 200
assertreply stored
devtools
wait 1200
assertdevtools open
asserturl {base}/normal
devtools
wait 400
assertdevtools closed
wait 2000''', probe=True)
    if sys.platform == 'darwin':
        # Inspect only this fixture's cookie database, never the user's.
        databases = list((regular/'profile').rglob('Cookies'))
        assert databases, 'Missing fixture cookie database'
        with sqlite3.connect(str(databases[0])) as database:
            value, encrypted = database.execute("SELECT value, encrypted_value FROM cookies WHERE name='regular'").fetchone()
        assert not value and encrypted, 'Fixture cookie was not encrypted on disk'
    private = run('private', f'''assertnoshells
assertpane home
privatecheck
shot incognito
tab {base}/private-canary
wait 1000
eval JSON.stringify([document.cookie,localStorage.getItem('normal'),localStorage.getItem('secret')])
wait 200
assertreply ["",null,null]
eval document.cookie='private=yes; path=/'; localStorage.setItem('secret','private-canary'); 'stored'
wait 200
assertreply stored
tab {base}/private-second
wait 1000
eval document.cookie+'|'+localStorage.getItem('secret')
wait 200
assertreply private=yes|private-canary
download {base}/download
wait 1500
privatecheck
devtools
wait 1000
assertdevtools open
devtools
wait 400
assertdevtools closed
privatecheck''', private=True)
    kept = private/'downloads/nus-private-check.txt'
    assert kept.read_bytes() == b'nus private download retained\n'
    if sys.platform == 'darwin':
        # A retained download can also retain where it came from. macOS stores
        # that in extended attributes, outside the private session and outside
        # anything the app deletes, so the private screen has to admit it.
        attributes = subprocess.run(['xattr', str(kept)], capture_output=True, text=True).stdout.split()
        provenance = [a for a in attributes if a in ('com.apple.quarantine', 'com.apple.metadata:kMDItemWhereFroms')]
        recorded = ''
        for attribute in provenance:
            recorded += subprocess.run(['xattr','-p',attribute,str(kept)], capture_output=True, text=True).stdout
        print(f'    download provenance: {provenance or "none"}', flush=True)
        # Either the origin is not recorded, or PRIVACY_AND_DIAGNOSTICS.md and
        # the private screen say that it is. They do; this asserts they must.
        if str(server.server_port) in recorded or 'quarantine' in ' '.join(provenance):
            note = (Path(__file__).resolve().parents[1]/'docs/PRIVACY_AND_DIAGNOSTICS.md').read_text().lower()
            assert 'where they came from' in note, 'Downloads keep their origin, but the privacy note does not say so'
    run('private-reopened', f'''privatecheck
tab {base}/reopened
wait 1000
eval JSON.stringify([document.cookie,localStorage.getItem('secret')])
wait 200
assertreply ["",null]
privatecheck''', private=True)
    modifier = 'cmd' if sys.platform == 'darwin' else 'ctrl'
    run('private-windows', f'''privatecheck
tab {base}/private-canary
wait 1000
eval document.cookie='shared=private; path=/'; 'stored'
wait 200
assertreply stored
key {modifier}+shift+n
wait 500
assertwindows 2
wait 3500
assertwindows 1
privatecheck
key {modifier}+shift+w
wait 500
assertwindows 0''', private=True, second=f'''privatecheck
assertwindows 2
tab {base}/second-window
wait 800
eval document.cookie
wait 200
assertreply shared=private
key {modifier}+w
wait 100
assertpane home
key {modifier}+w
wait 500
assertwindows 0''')
    run('regular-reopened', f'''tab {base}/normal
wait 1000
eval document.cookie+'|'+localStorage.getItem('normal')+'|'+localStorage.getItem('secret')
wait 200
assertreply regular=kept|kept|null''', directory=regular)
    for file in (regular/'profile').rglob('*'):
        if file.is_file() and file.stat().st_size < 16*1024*1024:
            assert b'private-canary' not in file.read_bytes(), f'Private data in regular profile: {file}'
    print('Privacy, local control, reporting, and DevTools checks passed.', flush=True)
finally:
    server.shutdown()
