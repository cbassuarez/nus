#!/usr/bin/env python3
"""Native lifecycle guards and production-loop idle measurements, isolated profiles."""
import argparse, importlib.util, json, os, subprocess, threading
from pathlib import Path
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('native', ROOT / 'scripts/perf-native.py')
native = importlib.util.module_from_spec(spec); spec.loader.exec_module(native)

def main():
    ap=argparse.ArgumentParser();ap.add_argument('--app',type=Path,required=True);ap.add_argument('--out',type=Path,required=True);ap.add_argument('--mode',choices=['lifecycle','idle','mercury-idle'],default='lifecycle');ap.add_argument('--runs',type=int,default=1);a=ap.parse_args()
    if a.out.exists():ap.error('retain previous evidence; choose a new output directory')
    a.out.mkdir(parents=True)
    exe=native.executable_for(a.app)
    class Handler(BaseHTTPRequestHandler):
        def do_GET(self):
            case=self.path.split('?')[0].strip('/')
            extras={'form':'<input id="draft">','media':'<audio></audio>'}.get(case,'')
            body=f'<!doctype html><title>Fixture {case}</title><p>{case}</p>{extras}<div style="height:4000px"></div>'.encode()
            self.send_response(200);self.send_header('Content-Length',str(len(body)));self.end_headers();self.wfile.write(body)
        def log_message(self,*args):pass
    server=ThreadingHTTPServer(('127.0.0.1',0),Handler);threading.Thread(target=server.serve_forever,daemon=True).start()
    url=f'http://127.0.0.1:{server.server_port}'
    results=[]
    try:
        for trial in range(a.runs):
            d=(a.out/str(trial+1)).resolve();p=d/'profile';p.mkdir(parents=True);(p/'onboarded').write_text('skip')
            (p/'me.json').write_text(json.dumps({'name':'Performance fixture','face':'Initial','created':'2026-09-22'}))
            (p/'settings.json').write_text(json.dumps({'behavior':{'splash':'None','home_look':'Line','new_window':'Prompt','then':'Prompt','keep_alive':'Off','close_asks':False,'update_checks':False,'sleep_after_min':1,'archive_after_h':0},'motion':{'register':0.5,'reduce':a.mode!='mercury-idle'},'window_rect':[60,60,1100,900]}))
            steps=['awaitperf startup_first_present','assertpresented','home','wait 1000']
            if a.mode=='lifecycle':
                steps+=['assertbrowsers 0','perfstats before-browser',f'tab {url}/plain','awaitpage Fixture plain','eval document.cookie="fixture=kept";scrollTo(0,800)','wait 400',f'tab {url}/form','awaitpage Fixture form','eval document.querySelector("input").value="keep my draft"',f'tab {url}/media','awaitpage Fixture media',f'tab {url}/history','awaitpage Fixture history','eval history.pushState({},"","#two")',f'tab {url}/active','awaitpage Fixture active','wait 800','assertbrowsers 5','memory awake','ageinactivetabs 120','wait 1500','assertsleep 1 true','assertsleep 2 false','assertsleep 3 false','assertsleep 4 false','assertsleep 5 false','assertbrowsers 4','memory sleeping','activatetab 1','awaitpage Fixture plain','wait 300','assertsleep 1 false','assertbrowsers 5','eval JSON.stringify({cookie:document.cookie.includes("fixture=kept"),scroll:Math.abs(scrollY-800)<2})','awaitreply {"cookie":true,"scroll":true}','activatetab 2','eval document.querySelector("input").value','awaitreply keep my draft']
                for n in range(3):
                    steps+=['activatetab 5','ageinactivetabs 120','wait 800','assertbrowsers 4','activatetab 1','awaitpage Fixture plain','wait 300','assertbrowsers 5',f'memory rewake-{n}']
                steps+=['perfstats complete']
            else:
                if a.mode=='mercury-idle':steps+=['looktab 5','wait 250','settingseek Mercury','wait 180','settingclick Mercury','wait 9000','key enter','home','wait 1000']
                steps+=['perfreset','wait 10000','perfstats idle','memory idle']
            shot=d/'run.shot';shot.write_text('\n'.join(steps)+'\n');env=native.run_environment(d,shot);env['NUS_SHOT_SIZE']='1100x900'
            if a.mode=='mercury-idle':env['NUS_DOCK_TRACE']=str(d/'dock')
            log=d/'run.log'
            with log.open('w') as f:
                proc=subprocess.Popen(native.launch_command(exe),cwd=d,env=env,stdout=f,stderr=subprocess.STDOUT,start_new_session=True)
                try:code=proc.wait(timeout=120)
                except BaseException:native.terminate_run(proc);raise
            text=log.read_text();assert code==0 and 'panicked' not in text,text[-3500:]
            perf=native.parse_records(text,'PERF');memory=native.parse_records(text,'MEMORY')
            assert perf and perf[-1][0]==('complete' if a.mode=='lifecycle' else 'idle'),'incomplete native run'
            if a.mode=='lifecycle':assert 'startup_cef_ready' not in perf[0][1], 'browser engine initialized before first use'
            results.append({'trial':trial+1,'perf':dict(perf),'memory':dict(memory),'log_sha256':native.sha256(log)})
            print('PASS',a.mode,trial+1,flush=True)
        assert native.sha256(exe), 'binary disappeared'
        (a.out/'results.json').write_text(json.dumps({'schema':1,'mode':a.mode,'binary_sha256':native.sha256(exe),'runs':results,'limits':'Local isolated fixtures. RSS includes shared pages. Idle count is native UI loop turns, not kernel wakeups.'},indent=2)+'\n')
    finally:server.shutdown();server.server_close()

if __name__=='__main__':main()
