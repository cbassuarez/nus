#!/usr/bin/env python3
"""Run unchanged Speedometer 3.1 workloads locally with a result-collection wrapper."""
import argparse,datetime,functools,importlib.util,json,os,platform,subprocess,tempfile,threading,time,uuid
from http.server import SimpleHTTPRequestHandler,ThreadingHTTPServer
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]
spec=importlib.util.spec_from_file_location('native',ROOT/'scripts/perf-native.py');native=importlib.util.module_from_spec(spec);spec.loader.exec_module(native)
PIN='1386415be8fef2f6b6bbdbe1828872471c5d802a'

def main():
 ap=argparse.ArgumentParser();ap.add_argument('--source',type=Path,required=True);ap.add_argument('--nus',type=Path,required=True);ap.add_argument('--zen',type=Path,required=True);ap.add_argument('--runs',type=int,default=5);ap.add_argument('--out',type=Path,required=True);a=ap.parse_args()
 if a.out.exists():ap.error('choose a new evidence path')
 revision=subprocess.check_output(['git','-C',str(a.source),'rev-parse','HEAD'],text=True).strip()
 if revision!=PIN:raise ValueError('unexpected Speedometer revision')
 if subprocess.check_output(['git','-C',str(a.source),'status','--porcelain'],text=True).strip():raise ValueError('Speedometer source must remain unmodified')
 collected={};failures={};ready={}
 class Handler(SimpleHTTPRequestHandler):
  def __init__(self,*args,**kwargs):super().__init__(*args,directory=str(a.source),**kwargs)
  def do_POST(self):
   token=self.path.rsplit('/',1)[-1]
   try:body=json.loads(self.rfile.read(int(self.headers.get('Content-Length','0'))))
   except Exception:self.send_error(400);return
   if self.path.startswith('/collect/'):collected[token]=body
   elif self.path.startswith('/failed/'):failures[token]=body
   elif self.path.startswith('/ready/'):ready[token]=body
   else:self.send_error(404);return
   self.send_response(200);self.end_headers()
  def do_GET(self):
   parts=self.path.split('/')
   if len(parts)>3 and parts[1]=='r':
    token=parts[2];self.path='/'+('/'.join(parts[3:]) or 'index.html')
    if self.path=='/index.html':
     source=(a.source/'index.html').read_text()
     wrapper='''<script type="module">
import './resources/main.mjs';
const token=TOKEN;const send=(kind,value)=>fetch('/'+kind+'/'+token,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(value)});
const client=globalThis.benchmarkClient;const finish=client.didFinishLastIteration.bind(client);
client.didFinishLastIteration=function(metrics){finish(metrics);send('collect',{valid:document.querySelector('#summary').className==='valid',iterations:this._measuredValuesList,score:this._computeResults(this._measuredValuesList,'score'),userAgent:navigator.userAgent,viewport:[innerWidth,innerHeight],visible:document.visibilityState});};
const error=client.handleError.bind(client);client.handleError=function(e){error(e);send('failed',{error:String(e)});};
await new Promise(r=>document.readyState==='complete'?r():addEventListener('load',r,{once:true}));
await send('ready',{visible:document.visibilityState,viewport:[innerWidth,innerHeight]});
setTimeout(()=>client.start(),2000);
</script>'''.replace('TOKEN',json.dumps(token))
     body=source.replace('</body>',wrapper+'</body>').encode();self.send_response(200);self.send_header('Content-Type','text/html');self.send_header('Content-Length',str(len(body)));self.end_headers();self.wfile.write(body);return
   super().do_GET()
  def log_message(self,*args):pass
 server=ThreadingHTTPServer(('127.0.0.1',0),Handler);threading.Thread(target=server.serve_forever,daemon=True).start()
 apps={'NUS':native.executable_for(a.nus),'Zen':a.zen/'Contents/MacOS/zen'}
 data={'schema':1,'status':'running','recorded_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'platform':platform.platform(),'workload':'Speedometer 3.1, all default suites, 10 iterations','source_revision':revision,'harness_sha256':native.sha256(Path(__file__)),'apps':{n:{'binary_sha256':native.sha256(p)} for n,p in apps.items()},'requested_trials':a.runs,'attempts':[],'limits':'Scores describe web-app responsiveness, not the entire workspace or physical input latency. Fresh profiles per independent trial; 10 internal iterations per score. Foreground benchmark windows, local fixtures, uncontrolled OS background activity.'}
 a.out.parent.mkdir(parents=True,exist_ok=True);logs=a.out.with_suffix('');logs.mkdir()
 def save():native.atomic_write(a.out,json.dumps(data,indent=2,allow_nan=False)+'\n')
 save()
 try:
  for trial in range(a.runs):
   order=['NUS','Zen'] if trial%2==0 else ['Zen','NUS']
   for name in order:
    token=uuid.uuid4().hex;url=f'http://127.0.0.1:{server.server_port}/r/{token}/'
    record={'product':name,'trial':trial+1,'status':'running'};data['attempts'].append(record);save();print(name,trial+1,flush=True)
    with tempfile.TemporaryDirectory(prefix='nus-browser-bench-') as td:
     d=Path(td);profile=d/'profile';profile.mkdir();log=logs/f'{name}-{trial+1}.log'
     if name=='NUS':
      (profile/'onboarded').write_text('skip');(profile/'settings.json').write_text(json.dumps({'behavior':{'splash':'None','keep_alive':'Off','close_asks':False},'motion':{'register':0.5,'reduce':True}}));shot=d/'browser.shot';shot.write_text(f'awaitperf startup_first_present\ntab {url}\nawaitfile {d}/done\n');env=native.run_environment(d,shot);env['NUS_SHOT_SIZE']='1440x1000';command=native.launch_command(apps[name])
     else:
      (profile/'user.js').write_text('user_pref("browser.shell.checkDefaultBrowser", false);\nuser_pref("browser.aboutwelcome.enabled", false);\nuser_pref("zen.welcome-screen.seen", true);\nuser_pref("browser.startup.homepage_override.mstone", "ignore");\n')
      env=os.environ.copy();command=[str(apps[name]),'--new-instance','--profile',str(profile),'--width','1440','--height','1000',url]
     with log.open('w') as f:
      proc=subprocess.Popen(command,cwd=d,env=env,stdout=f,stderr=subprocess.STDOUT,start_new_session=True)
      try:
       deadline=time.monotonic()+300
       while token not in collected and token not in failures:
        if proc.poll() is not None:raise RuntimeError('browser exited before results')
        if time.monotonic()>deadline:raise TimeoutError('Speedometer did not complete within 300s')
        time.sleep(.25)
       if token in failures:raise RuntimeError(str(failures[token]))
       result=collected[token]
       if not result['valid'] or len(result['iterations'])!=10 or result['visible']!='visible' or not ready.get(token):raise ValueError('incomplete, hidden or invalid benchmark')
       record.update(status='complete',result=result,ready=ready[token]);save();print(name,record['trial'],'score',result['score']['mean'],flush=True)
      except Exception as e:record.update(status='failed',error=str(e));data['status']='failed';save();raise
      finally:
       if proc.poll() is None:
        proc.terminate()
        try:proc.wait(timeout=5)
        except subprocess.TimeoutExpired:proc.kill();proc.wait(timeout=5)
  data['status']='complete';save();print(a.out)
 finally:server.shutdown();server.server_close()

if __name__=='__main__':main()
