#!/usr/bin/env python3
"""Publish retained before/after observations without private fixture paths."""
import argparse, hashlib, json, math, statistics
from pathlib import Path

def sha(path):return hashlib.sha256(path.read_bytes()).hexdigest()
def finite(values):
    if not values or any(type(v) not in (int,float) or not math.isfinite(v) or v<0 for v in values):raise ValueError('invalid observations')
    return {'median':statistics.median(values),'min':min(values),'max':max(values),'count':len(values),'observations':[{'run':i+1,'value':v} for i,v in enumerate(values)]}
def main():
    ap=argparse.ArgumentParser();ap.add_argument('--evidence',type=Path,required=True);ap.add_argument('--out',type=Path,required=True);a=ap.parse_args()
    sources={}
    def read(name):
        p=a.evidence/name;d=json.loads(p.read_text());sources[name]=sha(p);return d
    before=read('before-startup.json');after=read('startup.json');memory=read('home-memory-v1.json')
    baseline=before['metadata']['executable_sha256'];binary=after['metadata']['executable_sha256']
    def native(d,metric,expected):
        if d['status']!='complete' or d['metadata']['executable_sha256']!=expected or len(d['runs'])!=d['metadata']['runs'] or len(d['warmup_runs'])!=1 or any(x['status']!='valid' for x in d['attempts']):raise ValueError('incomplete or mixed native evidence')
        return finite([r['metrics'][metric]['p50_ms'] for r in d['runs']])
    out={'schema':1,'recorded_at':memory['recorded_at'],'hardware':memory['hardware'],'binary_sha256':binary,'baseline_binary_sha256':baseline,'startup_ms':{'before':native(before,'startup_first_present',baseline),'after':native(after,'startup_first_present',binary)},'file10_ms':{'before':native(read('before-editor10.json'),'file_10m_open_submit',baseline),'after':native(read('home-file-10m-v1.json'),'file_10m_open_submit',binary)}}
    for mode in ['idle','mercury-idle']:
        values={}
        for label,expected in [('before',baseline),('after',binary)]:
            d=read(f'{label}-{mode}/results.json')
            if d['binary_sha256']!=expected or d['mode']!=mode or len(d['runs'])!=3:raise ValueError('mixed or incomplete idle experiment')
            values[label]={'tree_rss_mib':finite([r['memory']['idle']['tree_rss_kib']/1024 for r in d['runs']]),'main_rss_mib':finite([r['memory']['idle']['main_rss_kib']/1024 for r in d['runs']]),'ui_turns_per_fixture':finite([r['perf']['idle']['ui_turn_work']['count'] for r in d['runs']])}
        out[mode.replace('-','_')]=values
    life=read('lifecycle/results.json')
    if life['binary_sha256']!=binary or life['mode']!='lifecycle' or len(life['runs'])!=1 or 'complete' not in life['runs'][0]['perf']:raise ValueError('incomplete lifecycle evidence')
    out['sleep_wake']={'trials':1,'states':life['runs'][0]['memory'],'browser_initialization_ms':life['runs'][0]['perf']['complete']['browser_initialization'],'method':'Five local pages. Plain page disposed; forms, media and Back/Forward history protected. Recreated page cookie and scroll checked; three further sleep/wake cycles retain five live browsers. This is a functional fixture, not a population estimate.'}
    out['dock_asset']=read('dock-bank.json')
    out['cache_policy']={'disposable_browser_retention_bytes':256*1024*1024,'enforcement':'After graceful shutdown, oldest complete HTTP/code/GPU cache stores first, across containers. Not a hard live-session limit or a cap on the whole profile.','protected':'Cookies, IndexedDB, service-worker storage, user projects/downloads and recovery generations excluded.','replay_retention_bytes':128*1024*1024,'replay_enforcement':'Active bytes count toward retention. Active recordings are preserved and can exceed this budget while per-window limits apply.'}
    out['boundaries']=['Release builds on one Apple M4 Pro; fresh profiles; background activity, thermals and OS caches uncontrolled.','Startup ends at the present-call return, not display scanout. Framework loading and browser initialization move to first browser use.','File-open values end at loaded-content submission. Before/after summaries here use medians; homepage keeps p95 over 20 opens and maximum over five 100 MiB opens.','Idle fixtures wait ten seconds after settling. UI loop turns are not kernel wakeups, CPU usage or energy.','RSS includes shared-page double counting. Mercury-idle compares the full app after claiming and returning home; it does not isolate the Dock change.','Binary identities include uncommitted work. These are local optimized builds, not measurements of every published download.']
    out['source_hashes']=sources;out['harness_sha256']=sha(Path(__file__))
    a.out.parent.mkdir(parents=True,exist_ok=True);a.out.write_text(json.dumps(out,indent=2,allow_nan=False)+'\n');print(a.out)
if __name__=='__main__':main()
