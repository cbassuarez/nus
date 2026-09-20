#!/usr/bin/env python3
"""Real CEF transfers and native geometry checks, confined to temporary profiles."""
from pathlib import Path
exec(compile(Path('scripts/check-settings.py').read_text().split('base=run(')[0], 'check-settings.py', 'exec'))
import http.server
import threading
import time

class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self,*args): pass
    def do_GET(self):
        if self.path=='/' or self.path.startswith('/index'):
            body=b'<title>Quarterly Research Report</title><h1>Download test</h1><p>Local transfer fixture.</p>'
            self.send_response(200);self.send_header('Content-Type','text/html');self.send_header('Content-Length',str(len(body)));self.end_headers();self.wfile.write(body);return
        slow=self.path.startswith('/slow')
        body=b'NUS LOCAL DOWNLOAD TEST\n'*(48000 if slow else 128)
        name='archive.tar.gz' if self.path.startswith('/archive') else 'document.pdf'
        self.send_response(200);self.send_header('Content-Type','application/octet-stream');self.send_header('Content-Disposition',f'attachment; filename="{name}"');self.send_header('Content-Length',str(len(body)));self.end_headers()
        try:
            for i in range(0,len(body),8192):
                self.wfile.write(body[i:i+8192]);self.wfile.flush()
                if slow: time.sleep(.04)
        except (BrokenPipeError,ConnectionResetError): pass
server=http.server.ThreadingHTTPServer(('127.0.0.1',0),Handler)
threading.Thread(target=server.serve_forever,daemon=True).start()
url=f'http://127.0.0.1:{server.server_port}'
profile=run('bootstrap','startpage prompt')
prefs=json.loads((profile/'settings.json').read_text());prefs['behavior']['splash']='None';prefs['motion']['reduce']=True;prefs['window_rect']=[80,80,2880,1800];prefs['sidebar_pinned']=True
base=f'tab {url}/\nwait 1400\n'
profile=run('transfer',base+f'download {url}/slow\nwait 800\ndownloadassert active\ndownloads modal\nwait 200\nshot progress\ndownloadclick Pause\nwait 600\ndownloadassert paused\nshot paused\ndownloadclick Resume\nwait 6500\ndownloadassert complete\ndownloadassert document.pdf\ndownloadclick Open Downloads page\nwait 200\nassertpane downloads\nshot complete',prefs)
records=json.loads((profile/'downloads.json').read_text());assert len(records)==1 and records[0]['done']
assert Path(records[0]['path']).read_bytes().startswith(b'NUS LOCAL DOWNLOAD TEST')
run('history','downloads page\nwait 200\ndownloadassert complete\ndownloadassert document.pdf\nshot history',prefs,download_history=records)
# Naming and collision reservation use real concurrent Chromium downloads.
run('automatic',base+f'downloadmode all\ndownload {url}/small\ndownload {url}/small\nwait 1800\ndownloadassert complete\ndownloadassert Quarterly Research Report.pdf\ndownloadassert Quarterly Research Report (2).pdf\ndownloadassert unique\ndownloads page\nwait 150\nshot named',prefs)
run('selective',base+f'downloadmode selective\ndownload {url}/archive\ndownload {url}/small\nwait 1800\ndownloadassert archive.tar.gz\ndownloadassert Quarterly Research Report.pdf\ndownloads page\nwait 150\nshot selective',prefs)
run('cancel',base+f'download {url}/slow\nwait 700\ndownloadact cancel\nwait 500\ndownloadassert cancelled\ndownloads page\nwait 150\nshot cancelled',prefs)
steps=['sidebarwidth 280','wait 300','sidebardrag 80','wait 300','sidebarcheck','shot small','footerdrag','wait 100','sidebarcheck','shot taller-footer','smallsidetype icons','wait 150','shot icons','smallsidetype preview','wait 150','shot previews','sidebardrag 320','wait 250','sidebarcheck','shot wide','downloadhover','wait 350','shot hover']
run('sidebar',base+'\n'.join(steps),prefs)
folder=root/'tree-fixture';folder.mkdir();(folder/'notes.md').write_text('Fixture');(folder/'images').mkdir()
run('files',f'sidebarwidth 80\nfiles\nbind {folder}\nwait 300\nsidebarcheck\nshot tree',prefs)
right=copy.deepcopy(prefs);right['sidebar']['side']='Right'
run('right-sidebar','sidebarwidth 280\nwait 250\nsidebardrag 80\nwait 250\nsidebarcheck\nshot right\nfooter\nwait 250\nshot right-themes',right)
run('grid',f'newshell\nurl {url}/\nwait 1600\ngridcheck\nshot split',prefs)
for width in [960,640,480]:
    p=copy.deepcopy(prefs);p['window_rect']=[80,80,width,1100]
    run(f'narrow-{width}','sidebarwidth 80\nhover 1 200\nwait 250\nsidebarcheck\ndownloads modal\nwait 200\nshot modal',p)
server.shutdown()
print('Download and sidebar checks passed.',flush=True)
