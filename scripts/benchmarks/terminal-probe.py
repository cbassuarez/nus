#!/usr/bin/env python3
"""Common PTY write-to-parser-reply boundary. Not display presentation latency."""
import argparse,fcntl,hashlib,json,os,re,select,struct,termios,time,tty
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('directory',type=Path);p.add_argument('--warmup',type=int,default=1);a=p.parse_args();d=a.directory
saved=termios.tcgetattr(0);tty.setraw(0)
def write(data):
 while data:
  n=os.write(1,data);data=data[n:]
def reply():
 buf=b'';deadline=time.monotonic()+20
 while time.monotonic()<deadline:
  if select.select([0],[],[],.1)[0]:
   buf+=os.read(0,4096)
   m=re.search(rb'\x1b\[(\d+);(\d+)R',buf)
   if m:return [int(m[1]),int(m[2])]
 raise TimeoutError('no terminal cursor reply')
try:
 rows,cols,_,_=struct.unpack('HHHH',fcntl.ioctl(0,termios.TIOCGWINSZ,b'\0'*8))
 (d/'ready.json').write_text(json.dumps({'rows':rows,'cols':cols,'pid':os.getpid()}))
 deadline=time.monotonic()+40
 while not (d/'go').exists():
  if time.monotonic()>deadline:raise TimeoutError('no harness start')
  time.sleep(.01)
 line=b'0123456789 abcdefghijklmnopqrstuvwxyz ABCDEFGHIJKLMNOPQRSTUVWXYZ\r\n'
 cases={'plain':line*8000,'ansi':(b'\x1b[32m'+line+b'\x1b[0m')*8000}
 results=[]
 for name,data in cases.items():
  for warm in range(a.warmup+1):
   write(b'\x1b[2J\x1b[H\x1b[6n');assert reply()==[1,1]
   start=time.perf_counter_ns();write(data+b'\x1b[H'+b'NUS-BENCH-END'+b'\x1b[6n');position=reply();elapsed=(time.perf_counter_ns()-start)/1e6
   if position!=[1,14]:raise ValueError('wrong final parser state '+str(position))
   if warm==a.warmup:results.append({'case':name,'bytes':len(data),'sha256':hashlib.sha256(data).hexdigest(),'elapsed_ms':elapsed,'mib_per_second':len(data)/1048576/(elapsed/1000),'final_cursor':position})
 (d/'result.json').write_text(json.dumps({'geometry':[cols,rows],'samples':results}))
except BaseException as e:
 (d/'error.txt').write_text(str(e));raise
finally:termios.tcsetattr(0,termios.TCSANOW,saved)
