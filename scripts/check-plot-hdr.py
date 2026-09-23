#!/usr/bin/env python3
"""Native Plot/radiance checks. Screenshots are SDR geometry evidence, not HDR proof."""
import copy, http.server, json, os, subprocess, sys, tempfile, threading
from pathlib import Path
bundle=Path(sys.argv[1] if len(sys.argv)>1 else 'dist/nus.app').resolve()
root=Path(tempfile.mkdtemp(prefix='nus-plot-check-'))
print('Evidence:',root,flush=True)
class Page(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        dark=self.path.startswith('/dark')
        color='#171c1a' if dark else '#ffffff'
        ink='#e6eae7' if dark else '#28352d'
        body=(f'<title>Local workspace</title><style>body{{background:{color};color:{ink};font:18px system-ui;margin:48px}}h1{{font-size:30px}}</style><h1>Local workspace</h1><p>The page remains interactive inside its bounds.</p><input aria-label="Project name" placeholder="Project name">').encode()
        self.send_response(200);self.send_header('Content-Type','text/html');self.send_header('Content-Length',str(len(body)));self.end_headers();self.wfile.write(body)
    def log_message(self,*args): pass
server=http.server.ThreadingHTTPServer(('127.0.0.1',0),Page)
threading.Thread(target=server.serve_forever,daemon=True).start()
port=server.server_port
try:
    for face,width in [('paper',1200),('ink',1200),('paper',480)]:
        d=root/f'{face}-{width}';p=d/'profile';p.mkdir(parents=True)
        (p/'onboarded').write_text('skip')
        prefs={'theme_mode':face,'behavior':{'splash':'None','then':'Prompt','update_checks':False,'follow_os_theme':False},'load_bar':{'style':'Radiance','color':'Signal','thickness':2.0,'chase':18.0}}
        (p/'settings.json').write_text(json.dumps(prefs))
        shot=d/'check.shot'
        shot.write_text(f'''wait 1800
assertprofile closed
tab http://127.0.0.1:{port}/{'dark' if face=='ink' else 'light'}
wait 1600
assertpane web
asserthdr enabled
hdrpixels
loadbarfixture 0.58
wait 350
assertloadbar 0.58
shot loading
wait 600
assertloadbar 0.58
loadbarfixture 1
wait 1600
assertloadbar 0
shot completed
wait 300
assertloadbar 0
''')
        env=dict(os.environ,NUS_SHOT_DIR=str(d),NUS_SHOT=str(shot),NUS_SHOT_OUT=str(d/'screens'),NUS_SHOT_SIZE=f'{width}x800',NUS_MODE=face,RUST_LOG='info')
        env.pop('NUS_SHOT2',None)
        with (d/'run.log').open('w') as log: result=subprocess.run([str(bundle/'Contents/MacOS/nus')],env=env,stdout=log,stderr=subprocess.STDOUT,timeout=45)
        text=(d/'run.log').read_text()
        if result.returncode or 'panicked' in text: raise RuntimeError(text[-3500:])
        print('PASS',face,width,flush=True)
finally: server.shutdown()
print('Plot geometry, HDR surface selection, progress hold, completion and idle checks passed.',flush=True)
