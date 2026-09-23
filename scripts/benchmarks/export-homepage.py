#!/usr/bin/env python3
"""Publish only validated observations behind the existing five homepage figures."""
import argparse,hashlib,json,math,statistics
from pathlib import Path

def load(path):
 d=json.loads(path.read_text())
 if d.get('status')!='complete':raise ValueError(f'incomplete source: {path.name}')
 return d

def main():
 p=argparse.ArgumentParser();p.add_argument('--perf',type=Path,required=True);p.add_argument('--site',type=Path,required=True);a=p.parse_args();dest=a.site/'assets/benchmarks';dest.mkdir(exist_ok=True)
 files={'file10':'home-file-10m-v1.json','file100':'home-file-100m-v1.json','memory':'home-memory-v1.json','packages':'package-evidence-v2.json'}
 data={k:(json.loads((a.perf/v).read_text()) if k=='packages' else load(a.perf/v)) for k,v in files.items()}
 out={'schema':1,'recorded_at':data['memory']['recorded_at'],'hardware':data['memory']['hardware'],'runtime_binary_sha256':data['memory']['binary_sha256'],'runtime_version':data['memory']['version'],'package_release':data['packages']['release'],'source_hashes':{v:hashlib.sha256((a.perf/v).read_bytes()).hexdigest() for v in files.values()},'figures':{},'package_sizes':{}}
 runtime_apps=[x for x in data['packages']['applications'] if x['product']=='NUS']
 if len(runtime_apps)!=1 or runtime_apps[0]['executable_sha256']!=out['runtime_binary_sha256']:raise ValueError('mixed package/runtime binaries')
 for key,metric,count,stat in [('file10','file_10m_open_submit',20,'p95'),('file100','file_100m_open_submit',5,'max')]:
  d=data[key]
  if d['metadata']['executable_sha256']!=out['runtime_binary_sha256']:raise ValueError('mixed runtime binaries')
  if len(d['runs'])!=count or len(d['warmup_runs'])!=1 or len(d['attempts'])!=count+1 or any(x['status']!='valid' for x in d['attempts']):raise ValueError('wrong run accounting')
  values=[]
  for run in d['runs']:
   m=run['metrics'][metric]
   if m['count']!=1 or m['p50_ms']!=m['max_ms']:raise ValueError('not a single open')
   v=m['p50_ms']
   if type(v) not in (int,float) or not math.isfinite(v) or v<=0:raise ValueError('invalid observation')
   values.append(v)
  value=sorted(values)[math.ceil(.95*count)-1] if stat=='p95' else max(values)
  out['figures'][key]={'value':value,'unit':'ms','statistic':stat,'count':count,'observations':[{'run':i+1,'value':v} for i,v in enumerate(values)],'median':statistics.median(values),'min':min(values),'max':max(values),'boundary':'File open request to first loaded-content submission; independent launches, one excluded warmup; OS caches uncontrolled; not display scanout.'}
 for key,case in [('tabs','tabs'),('windows','windows')]:
  trials=data['memory'][case]
  if len(trials)!=5 or data['memory']['requested_trials']!=5 or any(r['status']!='complete' for r in trials):raise ValueError('incomplete memory trials')
  obs=[{'run':f"{r['trial']}:{v['to']}",'value':v['mib']} for r in trials for v in r['increments']]
  values=[x['value'] for x in obs]
  if len(values)!=(35 if key=='tabs' else 15) or any(type(x) not in (int,float) or not math.isfinite(x) for x in values):raise ValueError('invalid memory delta')
  out['figures'][key]={'value':statistics.median(values),'unit':'MiB','min':min(values),'max':max(values),'count':len(values),'trials':len(trials),'observations':obs,'states':trials,'boundary':'Additional process-tree RSS, tabs 2–8; first-tab initialization excluded.' if key=='tabs' else 'Observed parent-process RSS change, windows 2–4; not total GPU memory. Negative deltas can reflect concurrent page reclamation, not negative window allocation.'}
 packages=data['packages'];out['packages']=packages
 allocated=packages['published_mac']['allocated_kib']
 out['package_sizes']['mac']={'value':allocated/1024,'unit':'MiB','statistic':'allocated bundle size on APFS','allocated_kib':allocated}
 for platform,suffix in [('mac','macos-arm64.zip'),('windows','windows-x86_64.zip'),('linux','linux-x86_64.tar.gz')]:
  item=next(v for v in packages['packages'] if v['name'].endswith(suffix));out['package_sizes'].setdefault(platform,{}).update(download_mib=item['download_bytes']/1048576,unpacked_mib=item['unpacked_file_bytes']/1048576,sha256=item['sha256'])
 (dest/'homepage.json').write_text(json.dumps(out,indent=2,allow_nan=False)+'\n')
 print(dest/'homepage.json')

if __name__=='__main__':main()
