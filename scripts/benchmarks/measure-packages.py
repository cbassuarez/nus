#!/usr/bin/env python3
"""Measure offline app bundles and authenticated release archives; never execute archives."""
import argparse,datetime,hashlib,json,plistlib,stat,subprocess,tarfile,zipfile
from pathlib import Path

def sha(path):
 h=hashlib.sha256()
 with path.open('rb') as f:
  for chunk in iter(lambda:f.read(1024*1024),b''):h.update(chunk)
 return h.hexdigest()

def archive_bytes(path):
 if path.name.endswith('.zip'):
  with zipfile.ZipFile(path) as z:
   return sum(v.file_size for v in z.infolist() if not v.is_dir() and not stat.S_ISLNK(v.external_attr>>16))
 with tarfile.open(path,'r:gz') as t:return sum(v.size for v in t if v.isfile())

def bundle(name,path):
 d=plistlib.loads((path/'Contents/Info.plist').read_bytes());seen=set();logical=0
 for file in path.rglob('*'):
  if file.is_file() and not file.is_symlink():
   s=file.stat();key=(s.st_dev,s.st_ino)
   if key not in seen:logical+=s.st_size;seen.add(key)
 exe=path/'Contents/MacOS'/d['CFBundleExecutable']
 return {'product':name,'version':d.get('CFBundleShortVersionString'),'build':d.get('CFBundleVersion'),'logical_bytes':logical,'allocated_kib':int(subprocess.check_output(['du','-sk',str(path)],text=True).split()[0]),'executable_sha256':sha(exe)}

def main():
 p=argparse.ArgumentParser();p.add_argument('--release-metadata',type=Path,required=True);p.add_argument('--archives',type=Path,required=True);p.add_argument('--app',action='append',required=True,help='Product=/absolute/path/Application.app');p.add_argument('--published-mac',type=Path,required=True);p.add_argument('--out',type=Path,required=True);a=p.parse_args()
 if a.out.exists():p.error('choose a new result path')
 metadata=json.loads(a.release_metadata.read_text());release=metadata[0] if isinstance(metadata,list) else metadata;packages=[]
 for asset in release['assets']:
  if not asset['name'].endswith(('.zip','.tar.gz')):continue
  path=a.archives/asset['name'];digest=sha(path)
  if asset.get('digest')!='sha256:'+digest:raise ValueError('archive does not match published digest: '+path.name)
  packages.append({'name':path.name,'download_bytes':path.stat().st_size,'unpacked_file_bytes':archive_bytes(path),'sha256':digest,'url':asset['browser_download_url']})
 apps=[bundle(name,Path(path)) for name,path in (value.split('=',1) for value in a.app)]
 published=bundle('NUS published macOS',a.published_mac)
 out={'schema':1,'recorded_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'release':release['tag_name'],'packages':packages,'applications':apps,'published_mac':published,'harness_sha256':sha(Path(__file__)),'release_metadata_sha256':sha(a.release_metadata),'method':'Application bundle regular-file bytes, unique inode once; symlinks excluded. Allocated KiB from du -sk on APFS. Archive unpacked sizes are file payload bytes excluding symlinks, not native Windows/Linux disk allocation.'}
 a.out.parent.mkdir(parents=True,exist_ok=True);a.out.write_text(json.dumps(out,indent=2)+'\n');print(a.out)
if __name__=='__main__':main()
