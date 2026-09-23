#!/usr/bin/env python3
"""Whole-workspace RSS: same local page, 100 KiB editor file and idle terminal."""
import signal
import argparse,datetime,importlib.util,json,os,platform,shlex,subprocess,sys,tempfile,threading,time,uuid
from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2];HERE=Path(__file__).resolve().parent
spec=importlib.util.spec_from_file_location('native',ROOT/'scripts/perf-native.py');native=importlib.util.module_from_spec(spec);spec.loader.exec_module(native)

def rss(roots):
 rows={}
 for line in subprocess.check_output(['ps','-axo','pid=,ppid=,rss='],text=True).splitlines():
  bits=line.split()
  if len(bits)==3:pid,parent,kib=map(int,bits);rows[pid]=(parent,kib)
 chosen=set(roots)
 while True:
  added={pid for pid,(parent,_) in rows.items() if parent in chosen};before=len(chosen);chosen|=added
  if len(chosen)==before:break
 if not all(pid in rows for pid in roots):raise ValueError('workspace root process exited')
 return {'rss_mib':sum(rows[pid][1] for pid in chosen if pid in rows)/1024,'processes':len(chosen),'root_count':len(roots)}

def stop(proc):
 if proc.poll() is None:
  proc.terminate()
  try:proc.wait(timeout=5)
  except subprocess.TimeoutExpired:proc.kill();proc.wait(timeout=5)

class OwnedProcess:
 def __init__(self,pid):self.pid=pid
 def poll(self):
  try:os.kill(self.pid,0);return None
  except ProcessLookupError:return 0
 def terminate(self):os.kill(self.pid,signal.SIGTERM)
 def kill(self):os.kill(self.pid,signal.SIGKILL)
 def wait(self,timeout):
  deadline=time.monotonic()+timeout
  while self.poll() is None:
   if time.monotonic()>deadline:raise subprocess.TimeoutExpired(str(self.pid),timeout)
   time.sleep(.1)

def ghostty_process(command,d):
 # LaunchServices is required for Ghostty's macOS graphical application.
 # Identify only the new process that owns this trial's unique probe command.
 subprocess.run(['open','-n','-a','/Applications/Ghostty.app','--args',*command[1:]],check=True)
 deadline=time.monotonic()+15
 while not (d/'terminal-ready.json').exists():
  if time.monotonic()>deadline:raise TimeoutError('Ghostty terminal probe did not start')
  time.sleep(.1)
 pid=json.loads((d/'terminal-ready.json').read_text())['pid']
 rows={}
 for line in subprocess.check_output(['ps','-axo','pid=,ppid=,comm='],text=True).splitlines():
  parts=line.split(None,2)
  if len(parts)==3:rows[int(parts[0])]=(int(parts[1]),parts[2])
 while pid in rows:
  parent,exe=rows[pid]
  if exe=='/Applications/Ghostty.app/Contents/MacOS/ghostty':return OwnedProcess(pid)
  pid=parent
 raise RuntimeError('Cannot attribute Ghostty probe to its owning app')

def main():
 ap=argparse.ArgumentParser();ap.add_argument('--arc-pid',type=int,help='Passively include the already running Arc test session; never launch or quit it.');ap.add_argument('--nus',type=Path,required=True);ap.add_argument('--apps',type=Path,default=Path('/tmp/nus-competitors/apps'));ap.add_argument('--runs',type=int,default=5);ap.add_argument('--out',type=Path,required=True);a=ap.parse_args()
 if a.out.exists():ap.error('choose a new result path')
 seen=set()
 class Handler(BaseHTTPRequestHandler):
  def do_GET(self):
   if self.path.startswith('/ready/'):
    seen.add(self.path.split('/')[-1]);body=b'ok'
   else:
    token=self.path.strip('/');body=f'<!doctype html><title>NUS benchmark fixture</title><p>NUS benchmark fixture: local page ready</p><script>addEventListener("load",()=>fetch("/ready/{token}"))</script>'.encode()
   self.send_response(200);self.send_header('Content-Type','text/html');self.send_header('Content-Length',str(len(body)));self.end_headers();self.wfile.write(body)
  def log_message(self,*args):pass
 server=ThreadingHTTPServer(('127.0.0.1',0),Handler);threading.Thread(target=server.serve_forever,daemon=True).start()
 exe=native.executable_for(a.nus);zen=a.apps/'Zen.app/Contents/MacOS/zen';code=a.apps/'Visual Studio Code.app/Contents/MacOS/Code'
 terminals={'Ghostty':Path('/Applications/Ghostty.app/Contents/MacOS/ghostty'),'Kitty':a.apps/'kitty.app/Contents/MacOS/kitty','WezTerm':Path('/Applications/WezTerm.app/Contents/MacOS/wezterm')}
 names=list(terminals) if a.arc_pid else ['NUS',*terminals];apps={'NUS':exe,'Zen':zen,'VS Code':code,**terminals}
 if a.arc_pid:
  arc=Path('/Applications/Arc.app/Contents/MacOS/Arc');owner=subprocess.check_output(['ps','-p',str(a.arc_pid),'-o','uid=,comm='],text=True).strip().split(None,1)
  if owner!=[str(os.getuid()),str(arc)]:raise ValueError('Expected an existing Arc process owned by this user')
  apps.pop('NUS');apps.pop('Zen');apps['Arc']=arc
 data={'schema':1,'status':'running','recorded_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'platform':platform.platform(),'apps':{n:{'binary_sha256':native.sha256(p)} for n,p in apps.items()},'requested_trials':a.runs,'attempts':[],'harness_sha256':native.sha256(Path(__file__)),'fixture':'100 KiB plaintext file + simple local HTML page + idle Python terminal command; editor foreground; no third-party extensions','sampling':'Median of five process-tree RSS snapshots, one second apart, after all three readiness oracles and 3 seconds settling','limits':'Sum of the unique descendant PID union, RSS still double-counts shared pages across processes. Same tasks, NUS one window versus three app windows. VS Code includes a small readiness-only development extension; helper Python process present in every configuration. Fresh profiles/configs; OS caches and background activity uncontrolled.'}
 if a.arc_pid:
  data['session_reused']=True;data['sampling_unit']='Five fresh editor/terminal trials sharing one existing Arc session'
  data['limits']='Arc uses the user-prepared empty account in the existing macOS session; process, browser storage and caches retained, no app restarts. Account change is not filesystem profile isolation. VS Code and terminals use fresh profiles/configs. Follow-up batch, not paired with the earlier NUS/Zen runs. RSS sums unique descendant PIDs, double-counts shared pages; not physical footprint. Editor foreground, three windows. Small readiness extension and common Python helper included. Background activity uncontrolled.'
 a.out.parent.mkdir(parents=True,exist_ok=True);logs=a.out.with_suffix('');logs.mkdir()
 def save():native.atomic_write(a.out,json.dumps(data,indent=2,allow_nan=False)+'\n')
 save()
 try:
  if a.arc_pid:
   arc_token=uuid.uuid4().hex;arc_url=f'http://127.0.0.1:{server.server_port}/{arc_token}'
   native.atomic_write(a.out.with_suffix('.control.json'),json.dumps({'phase':'open','url':arc_url})+'\n');print('OPEN ARC FIXTURE',arc_url,flush=True)
   deadline=time.monotonic()+600
   while arc_token not in seen:
    os.kill(a.arc_pid,0)
    if time.monotonic()>deadline:raise TimeoutError('Arc fixture did not load')
    time.sleep(.1)
  for trial in range(a.runs):
   order=names[trial%len(names):]+names[:trial%len(names)]
   for name in order:
    label='NUS' if name=='NUS' else 'VS Code + '+('Arc' if a.arc_pid else 'Zen')+' + '+name;record={'product':label,'trial':trial+1,'status':'running'};data['attempts'].append(record);save();print(label,trial+1,flush=True)
    with tempfile.TemporaryDirectory(prefix='nus-workspace-') as td:
     d=Path(td);token=uuid.uuid4().hex;url=f'http://127.0.0.1:{server.server_port}/{token}';file=d/'fixture.txt';line='NUS benchmark fixture: same local text in each editor.\n';file.write_text((line*3000)[:102400]);processes=[];handles=[];env=os.environ.copy()
     if a.arc_pid:token=arc_token;url=arc_url
     def launch(command,extra=None):
      h=(logs/f'{name}-{trial+1}-{len(processes)}.log').open('w');handles.append(h);p=subprocess.Popen(command,cwd=d,env=extra or env,stdout=h,stderr=subprocess.STDOUT,start_new_session=True);processes.append(p);return p
     try:
      terminal=[sys.executable,str(HERE/'idle-terminal.py'),str(d)]
      if name=='NUS':
       profile=d/'profile';profile.mkdir();(profile/'onboarded').write_text('skip');(profile/'settings.json').write_text(json.dumps({'behavior':{'splash':'None','keep_alive':'Off','close_asks':False},'motion':{'register':0.5,'reduce':True}}));shot=d/'workspace.shot';shot.write_text('awaitperf startup_first_present\nnewshell\nshell exec '+' '.join(map(shlex.quote,terminal))+f'\nawaitfile {d}/terminal-ready.json\ntab {url}\nawaitpage NUS benchmark fixture\nopenfile {file}\nawaitperf file_open_submit\nasserteditorready 102400\nperfstats ready\nawaitfile {d}/stop\n');launch(native.launch_command(exe),native.run_environment(d,shot))
      else:
       if not a.arc_pid:
        p=d/'zen-profile';p.mkdir();(p/'user.js').write_text('user_pref("browser.shell.checkDefaultBrowser",false);user_pref("browser.aboutwelcome.enabled",false);user_pref("zen.welcome-screen.seen",true);user_pref("browser.startup.homepage_override.mstone","ignore");');launch([str(zen),'--new-instance','--profile',str(p),'--width','1100','--height','800',url])
       if name=='Ghostty':cmd=[str(terminals[name]),'--config-default-files=false','--window-save-state=never','--quit-after-last-window-closed=true','--window-width=100','--window-height=32','-e',*terminal]
       elif name=='Kitty':cmd=[str(terminals[name]),'--config','NONE','--override','initial_window_width=100c','--override','initial_window_height=32c',*terminal]
       else:
        config=d/'wezterm.lua';config.write_text('return {initial_cols=100,initial_rows=32,check_for_updates=false,window_close_confirmation="NeverPrompt"}');cmd=[str(terminals[name]),'--config-file',str(config),'start','--always-new-process','--',*terminal]
       if name=='Ghostty':processes.append(ghostty_process(cmd,d))
       else:launch(cmd)
       user=d/'code-data/User';user.mkdir(parents=True);(user/'settings.json').write_text(json.dumps({'workbench.startupEditor':'none','workbench.enableExperiments':False,'telemetry.telemetryLevel':'off','update.mode':'none','security.workspace.trust.enabled':True}))
       codeenv=env|{'NUS_BENCH_FILE':str(file),'NUS_BENCH_BYTES':'102400','NUS_BENCH_READY':str(d/'editor-ready.json')}
       launch([str(code),'--user-data-dir',str(d/'code-data'),'--extensions-dir',str(d/'code-extensions'),'--extensionDevelopmentPath='+str(HERE/'vscode-probe'),'--new-window','--skip-welcome','--skip-release-notes',str(file)],codeenv)
      deadline=time.monotonic()+45
      while True:
       editor=('PERF ready ' in (logs/f'{name}-{trial+1}-0.log').read_text()) if name=='NUS' else (d/'editor-ready.json').exists()
       if token in seen and (d/'terminal-ready.json').exists() and editor:break
       if any(p.poll() is not None for p in processes):raise RuntimeError('workspace application exited before readiness')
       if time.monotonic()>deadline:raise TimeoutError(f'readiness failed: browser={token in seen}, terminal={(d/"terminal-ready.json").exists()}, editor={editor}')
       time.sleep(.1)
      time.sleep(3);samples=[]
      for i in range(5):samples.append(rss([p.pid for p in processes]+([a.arc_pid] if a.arc_pid else [])));time.sleep(1)
      import statistics
      record.update(status='complete',samples=samples,rss_mib=statistics.median(s['rss_mib'] for s in samples),oracles={'page':True,'editor_bytes':102400,'terminal':True});save();print('RSS',record['rss_mib'],flush=True)
     except Exception as e:record.update(status='failed',error=str(e));data['status']='failed';save();raise
     finally:
      (d/'stop').touch()
      for p in reversed(processes):stop(p)
      for h in handles:h.close()
  data['status']='complete';save();
  if a.arc_pid:native.atomic_write(a.out.with_suffix('.control.json'),json.dumps({'phase':'complete'})+'\n')
  print(a.out)
 finally:server.shutdown();server.server_close()

if __name__=='__main__':main()
