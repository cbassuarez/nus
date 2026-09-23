#!/usr/bin/env python3
"""Collect Speedometer repetitions from one existing Arc test tab; never launch or quit Arc."""
import argparse,datetime,hashlib,importlib.util,json,os,platform,subprocess,threading,time,uuid
from http.server import SimpleHTTPRequestHandler,ThreadingHTTPServer
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]
spec=importlib.util.spec_from_file_location('native',ROOT/'scripts/perf-native.py');native=importlib.util.module_from_spec(spec);spec.loader.exec_module(native)
PIN='1386415be8fef2f6b6bbdbe1828872471c5d802a'
def main():
 ap=argparse.ArgumentParser();ap.add_argument('--source',type=Path,required=True);ap.add_argument('--arc-pid',type=int,required=True);ap.add_argument('--runs',type=int,default=5);ap.add_argument('--out',type=Path,required=True);a=ap.parse_args()
 if a.out.exists():ap.error('choose a new result path')
 exe=Path('/Applications/Arc.app/Contents/MacOS/Arc')
 owner=subprocess.check_output(['ps','-p',str(a.arc_pid),'-o','uid=,comm='],text=True).strip().split(None,1)
 if owner!=[str(os.getuid()),str(exe)]:raise ValueError('PID must identify the existing Arc process owned by this user')
 if subprocess.check_output(['git','-C',str(a.source),'rev-parse','HEAD'],text=True).strip()!=PIN or subprocess.check_output(['git','-C',str(a.source),'status','--porcelain'],text=True).strip():raise ValueError('Speedometer checkout must be the unchanged pin')
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
 data={'schema':1,'status':'running','recorded_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'platform':platform.platform(),'source_revision':PIN,'requested_trials':a.runs,'apps':{'Arc':{'binary_sha256':native.sha256(exe)}},'harness_sha256':native.sha256(Path(__file__)),'sampling_unit':'Repeated benchmark runs within one existing Arc process','session_reused':True,'workload':'Speedometer 3.1, all default suites, 10 internal iterations per repetition','limits':'User-prepared empty Arc account, existing macOS account and app process. Account changes do not establish a new filesystem profile. Browser storage and caches retained; no app restart. Repetitions share session state and are not independent process trials. Follow-up batch, not paired with the earlier NUS/Zen runs; background activity, thermal and power state uncontrolled. No causal speedup or confidence interval claim.','attempts':[]}
 a.out.parent.mkdir(parents=True,exist_ok=True);control=a.out.with_suffix('.control.json')
 def save():native.atomic_write(a.out,json.dumps(data,indent=2,allow_nan=False)+'\n')
 def state(phase,**kw):native.atomic_write(control,json.dumps({'phase':phase,**kw})+'\n')
 save()
 try:
  for trial in range(1,a.runs+1):
   token=uuid.uuid4().hex;url=f'http://127.0.0.1:{server.server_port}/r/{token}/';record={'product':'Arc','trial':trial,'status':'running'};data['attempts'].append(record);save();state('open',trial=trial,url=url);print('OPEN',trial,url,flush=True)
   deadline=time.monotonic()+600
   try:
    while token not in collected and token not in failures:
     os.kill(a.arc_pid,0)
     if time.monotonic()>deadline:raise TimeoutError('benchmark did not complete')
     time.sleep(.25)
    if token in failures:raise ValueError(str(failures[token]))
    result=collected[token]
    if not result['valid'] or not result['score']['isValid'] or len(result['iterations'])!=10 or result['visible']!='visible' or not ready.get(token):raise ValueError('invalid or hidden benchmark')
    record.update(status='complete',result=result,ready=ready[token]);save();print('COMPLETE',trial,result['score']['mean'],flush=True)
   except BaseException as exc:record.update(status='failed',error=str(exc));raise
  data['status']='complete';save();state('complete');print(a.out,flush=True)
 except BaseException as exc:data.update(status='failed',error=str(exc));save();state('failed');raise
 finally:server.shutdown();server.server_close()
if __name__=='__main__':main()
