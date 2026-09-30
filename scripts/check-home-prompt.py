#!/usr/bin/env python3
"""Exercise Home through the native app's keyboard path in a disposable profile."""
import argparse
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import subprocess
import tempfile
import threading

class Page(BaseHTTPRequestHandler):
    def do_GET(self):
        data=b'<title>Prompt destination fixture</title><h1>Opened from the prompt</h1>'
        self.send_response(200)
        self.send_header('Content-Type','text/html')
        self.send_header('Content-Length',str(len(data)))
        self.end_headers()
        self.wfile.write(data)
    def log_message(self, *_):
        pass

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('bundle',type=Path)
    args=parser.parse_args()
    root=Path(tempfile.mkdtemp(prefix='nus-home-prompt-'))
    profile=root/'profile';profile.mkdir()
    (profile/'onboarded').write_text('skip')
    (profile/'settings.json').write_text(json.dumps({'behavior':{'hatch_background':False,'hatch_status':False,'splash':'None','new_window':'Prompt','then':'Prompt','follow_os_theme':False},'motion':{'register':0.5,'reduce':True}}))
    server=ThreadingHTTPServer(('127.0.0.1',0),Page)
    threading.Thread(target=server.serve_forever,daemon=True).start()
    url=f'http://127.0.0.1:{server.server_port}/home'
    script=root/'home.shot'
    script.write_text('\n'.join([
        'wait 1200','homeparitycheck','theme blueprint','homeclear','wait 250','shot home-search',
        f'hometype {url}','key cmd+enter','assertpane web','wait 900',
        'eval document.title','wait 200','assertreply Prompt destination fixture',
        'home','assertpane home','homeclear','hometype settings','key enter','assertpane settings',
        'home','homeclear','hometype where to travel','key cmd+k','assertpromptfirst Search the web',
        'key esc','assertpane home','trafficsize 740 380','homelast','wait 250','homevisible','shot home-results-narrow',
        'trafficsize 1440 900','homeclear','hometype > echo ready','wait 200','shot home-command',
        'homeclear',f'hometype {url}','key enter','assertpane web','wait 700',
        'eval document.title','wait 200','assertreply Prompt destination fixture',
    ])+'\n')
    exe=args.bundle.resolve()/'Contents/MacOS/nus'
    env={k:v for k,v in os.environ.items() if not k.startswith('NUS_')}
    env.update(NUS_SHOT_DIR=str(root),NUS_SHOT=str(script),NUS_SHOT_OUT=str(root/'screens'),NUS_SHOT_SIZE='1440x900')
    try:
        with (root/'run.log').open('w') as log:
            result=subprocess.run([str(exe)],env=env,stdout=log,stderr=subprocess.STDOUT,timeout=90)
    finally:
        server.shutdown();server.server_close()
    output=(root/'run.log').read_text()
    assert result.returncode==0 and 'HOME PARITY PASS' in output and 'panicked' not in output,output[-5000:]
    assert 'shot: unknown' not in output,output[-3000:]
    assert len(list((root/'screens').glob('*.png')))==3
    (root/'result.json').write_text(json.dumps({'passed':True,'binary_sha256':hashlib.sha256(exe.read_bytes()).hexdigest(),'platform':'macOS','checks':['shared routes','new tab and normal URL navigation','new-window intent','Cmd-K draft handoff','selection editing','focused home pane','selected-row visibility']},indent=2)+'\n')
    print(f'PASS: home prompt native regression; evidence {root}')

if __name__=='__main__':
    main()
