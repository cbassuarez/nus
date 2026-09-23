#!/usr/bin/env python3
"""Marginal tab/window memory with semantic checks and retained per-trial states."""
import argparse,datetime,hashlib,importlib.util,json,os,platform,subprocess,tempfile,threading
from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer
from pathlib import Path

ROOT=Path(__file__).resolve().parents[2]
spec=importlib.util.spec_from_file_location('native',ROOT/'scripts/perf-native.py');native=importlib.util.module_from_spec(spec);spec.loader.exec_module(native)

def hardware():
    return dict(zip(['model','memory_bytes','logical_cpus','cpu'],subprocess.check_output(['sysctl','-n','hw.model','hw.memsize','hw.ncpu','machdep.cpu.brand_string'],text=True).splitlines()))

def run(exe,scenario,trial,logs,url):
    with tempfile.TemporaryDirectory(prefix='nus-memory-') as td:
        d=Path(td);p=d/'profile';p.mkdir();(p/'onboarded').write_text('skip')
        prefs={'behavior':{'splash':'None','home_look':'Line','new_window':'Prompt','then':'Prompt','keep_alive':'Off','close_asks':False},'motion':{'register':0.5,'reduce':True},'window_rect':[60,60,1100,800]}
        (p/'settings.json').write_text(json.dumps(prefs))
        steps=['awaitperf startup_first_present','assertpresented','home','wait 1000']
        if scenario=='tabs':
            steps+=['assertbrowsers 0','memory tabs-0']
            for i in range(1,9):
                steps += [f'tab {url}/{i}',f'awaitpage Idle fixture {i}',f'assertbrowsers {i}','wait 1000',f'memory tabs-{i}']
            for i in range(8):steps+=['appmenu CloseTab','wait 200']
            steps+=['wait 1000','assertbrowsers 0','memory tabs-closed']
        else:
            steps+=['assertwindows 1','memory windows-1']
            for i in range(2,5):steps+=['newwindow','wait 1600',f'assertwindows {i}',f'memory windows-{i}']
        steps+=['perfstats complete']
        shot=d/'memory.shot';shot.write_text('\n'.join(steps)+'\n');env=native.run_environment(d,shot);env['NUS_SHOT_SIZE']='1100x800'
        log=logs/f'{scenario}-{trial:02d}.log'
        with log.open('w') as f:
            proc=subprocess.Popen(native.launch_command(exe),cwd=d,env=env,stdout=f,stderr=subprocess.STDOUT,start_new_session=True)
            try:code=proc.wait(timeout=90)
            except BaseException:native.terminate_run(proc);raise
        text=log.read_text()
        if code or 'panicked' in text:raise RuntimeError(f'native failure: {log}\n{text[-1600:]}')
        complete=native.parse_records(text,'PERF')
        if len(complete)!=1 or complete[0][0]!='complete':raise ValueError('missing completion marker')
        records=native.parse_records(text,'MEMORY')
        expected=[f'tabs-{i}' for i in range(9)]+['tabs-closed'] if scenario=='tabs' else [f'windows-{i}' for i in range(1,5)]
        if [label for label,_ in records]!=expected:raise ValueError('missing or reordered memory stages')
        for _,v in records:
            if not v.get('available') or any(type(v.get(k)) not in (int,float) or v[k]<0 for k in ['tree_rss_kib','main_rss_kib']):raise ValueError('invalid RSS sample')
        states=[{'stage':label,**v} for label,v in records]
        field='tree_rss_kib' if scenario=='tabs' else 'main_rss_kib'
        # First browser initialization is separate from every further tab.
        increments=[{'from':states[i-1]['stage'],'to':states[i]['stage'],'mib':(states[i][field]-states[i-1][field])/1024} for i in (range(2,9) if scenario=='tabs' else range(1,4))]
        return {'trial':trial,'status':'complete','states':states,'increments':increments,'log_sha256':native.sha256(log)}

def main():
    ap=argparse.ArgumentParser();ap.add_argument('--app',type=Path,required=True);ap.add_argument('--runs',type=int,default=5);ap.add_argument('--out',type=Path,required=True);a=ap.parse_args()
    if a.out.exists():ap.error('choose a new output path; retain earlier evidence')
    if a.runs<1:ap.error('runs must be positive')
    exe=native.executable_for(a.app);a.out.parent.mkdir(parents=True,exist_ok=True);logs=a.out.with_suffix('');logs.mkdir()
    class Handler(BaseHTTPRequestHandler):
        def do_GET(self):
            i=self.path.strip('/');body=f'<!doctype html><title>Idle fixture {i}</title><p>An idle browser tab.</p>'.encode();self.send_response(200);self.send_header('Content-Length',str(len(body)));self.send_header('Cache-Control','no-store');self.end_headers();self.wfile.write(body)
        def log_message(self,*args):pass
    server=ThreadingHTTPServer(('127.0.0.1',0),Handler);threading.Thread(target=server.serve_forever,daemon=True).start()
    data={'schema':1,'status':'running','recorded_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'platform':platform.platform(),'hardware':hardware(),'binary_sha256':native.sha256(exe),'version':native.version_of(exe),'harness_sha256':native.sha256(Path(__file__)),'window_logical':[1100,800],'settle_ms':{'tabs':1000,'windows':1600},'requested_trials':a.runs,'tabs':[],'windows':[],'attempts':[],'limits':'Summed tree RSS double-counts shared pages; window deltas use parent RSS. Background activity, power and thermal state uncontrolled. Fixed settling delays do not prove quiescence.'}
    def save():native.atomic_write(a.out,json.dumps(data,indent=2,allow_nan=False)+'\n')
    save()
    try:
        for trial in range(1,a.runs+1):
            for case in ['tabs','windows']:
                attempt={'case':case,'trial':trial,'status':'running'};data['attempts'].append(attempt);save();print(case,trial,flush=True)
                try:data[case].append(run(exe,case,trial,logs,f'http://127.0.0.1:{server.server_port}'))
                except BaseException as e:attempt['status']='failed';data['status']='failed';data['error']=str(e);save();raise
                attempt['status']='complete';save()
        if native.sha256(exe)!=data['binary_sha256']:raise ValueError('binary changed during experiment')
        data['status']='complete';save();print(a.out,flush=True)
    finally:server.shutdown();server.server_close()

if __name__=='__main__':main()
