#!/usr/bin/env python3
"""Preview 9 native regressions; isolated profiles, real CEF and optional downloads."""
import http.server,json,os,socket,subprocess,sys,tempfile,threading
from pathlib import Path
bundle=Path(sys.argv[1] if len(sys.argv)>1 else '/tmp/nus-preview9.app').resolve()
root=Path(tempfile.mkdtemp(prefix='nus-preview9-check-'));print('Evidence:',root,flush=True)
class Page(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        body=b'<title>Local fixture</title><body style="font:20px system-ui;background:white;color:black"><h1>Plot boundary</h1><p>Only the dashed bounds remain.</p>'
        if self.path=='/subframe':body+=f'<iframe src="http://127.0.0.1:{dead}/missing"></iframe>'.encode()
        if self.path=='/redirect':self.send_response(302);self.send_header('Location',f'http://127.0.0.1:{dead}/redirected');self.end_headers();return
        self.send_response(200);self.send_header('Content-Type','text/html');self.send_header('Content-Length',str(len(body)));self.end_headers();self.wfile.write(body)
    def log_message(self,*args):pass
with socket.socket() as s:s.bind(('127.0.0.1',0));dead=s.getsockname()[1]
server=http.server.ThreadingHTTPServer(('127.0.0.1',0),Page);threading.Thread(target=server.serve_forever,daemon=True).start();port=server.server_port
prefs={'theme_mode':'paper','behavior':{'splash':'None','then':'Prompt','update_checks':False,'follow_os_theme':False},'load_bar':{'style':'Rule','color':'Signal','thickness':3.0,'chase':18.0}}
def run(name,steps,env=None,timeout=100):
 d=root/name;p=d/'profile';p.mkdir(parents=True,exist_ok=True)
 if not (p/'settings.json').exists():(p/'settings.json').write_text(json.dumps(prefs))
 (p/'onboarded').write_text('skip');shot=d/'check.shot';shot.write_text('wait 2000\n'+steps+'\n')
 e=dict(os.environ,NUS_SHOT_DIR=str(d),NUS_SHOT=str(shot),NUS_SHOT_OUT=str(d/'screens'),NUS_SHOT_SIZE='1200x900',NUS_MODE='paper');e.pop('NUS_SHOT2',None);e.update(env or {})
 with (d/'run.log').open('w')as out:r=subprocess.run([str(bundle/'Contents/MacOS/nus')],env=e,stdout=out,stderr=subprocess.STDOUT,timeout=timeout)
 text=(d/'run.log').read_text()
 if r.returncode or 'panicked' in text:raise RuntimeError(f'{name}: {text[-4500:]}')
 print('PASS',name,flush=True)
try:
 run('navigation',f'''tab http://127.0.0.1:{port}/
wait 2200
asserturl http://127.0.0.1:{port}/
assertloadidle
shot local-bounds
url http://127.0.0.1:{dead}/missing
wait 2500
asserturl http://127.0.0.1:{dead}/missing
assertloadidle
shot failed-url
url http://127.0.0.1:{port}/subframe
wait 2200
asserturl http://127.0.0.1:{port}/subframe
url http://127.0.0.1:{port}/redirect
wait 2500
asserturl http://127.0.0.1:{dead}/redirected
assertloadidle
noticefixture Visible outside the editor
asserttoast Visible outside the editor
shot toast'''+''.join(f'\nurl http://127.0.0.1:{port}/\nwait 700\nasserturl http://127.0.0.1:{port}/\nurl http://127.0.0.1:{port}/redirect\nwait 2000\nasserturl http://127.0.0.1:{dead}/redirected\nassertloadidle' for _ in range(5)))
 run('icons','''sidebarhoverprobe
iconsettings
wait 500
shot icon-choices
iconchoose Plex
wait 600
asserticon Plex
shot icon-selected''')
 run('icons','asserticon Plex\niconsettings\nwait 300\niconchoose Newsreader\nwait 300\nasserticon Newsreader')
 if '--downloads' in sys.argv:
  # An isolated PATH exercises installation even if these tools exist globally.
  bin=root/'fixture-bin';bin.mkdir();
  for name in ['node','npm']:
   found=subprocess.check_output(['/usr/bin/which',name],text=True).strip();(bin/name).symlink_to(found)
  shell=root/'fixture-shell';shell.write_text('#!/bin/sh\nprintf %s '+str(bin)+':/usr/bin:/bin:/usr/sbin:/sbin\n');shell.chmod(0o755)
  profile=root/'downloads/profile';profile.mkdir(parents=True)
  (profile/'bundles.json').write_text(json.dumps([{'id':'fixture-missing-manager','name':'Missing manager fixture','kind':'lsp','about':'Test missing prerequisite','command':['nus-missing-manager']}]))
  run('downloads','''welcome
wait 500
bundle bash-language-server
asserttoast Installing bash-language-server
awaitbundle bash-language-server
assertbundle bash-language-server
asserttoast Installed bash-language-server
shot installed-bash
bundle taplo
awaitbundle taplo
assertbundle taplo
assertbundle bash-language-server
asserttoast Installed taplo
bundle taplo
asserttoast Removed taplo
assertbundle bash-language-server
bundle fixture-missing-manager
wait 1200
asserttoast Could Not Install fixture-missing-manager
shot installation-error
''',{'SHELL':str(shell),'PATH':str(bin)+':/usr/bin:/bin:/usr/sbin:/sbin'},timeout=180)
  tool=root/'downloads/profile/tools/bash-language-server/bin/bash-language-server'
  env=dict(os.environ,PATH=str(bin)+':/usr/bin:/bin:/usr/sbin:/sbin');version=subprocess.check_output([str(tool),'--version'],env=env,text=True,timeout=20);print('Bash runtime:',version.strip(),flush=True)
finally:server.shutdown()
