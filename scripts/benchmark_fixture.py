#!/usr/bin/env python3
"""Disposable loopback workload, launched through a real NUS PTY."""
import argparse
import html
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import os
from pathlib import Path
import threading
import time


def main():
    p=argparse.ArgumentParser();p.add_argument('root',type=Path);p.add_argument('name');p.add_argument('port',type=int);p.add_argument('--load',action='store_true');a=p.parse_args()
    root=a.root;token=(root/'token').read_text();value=root/(a.name+'.txt')
    class Handler(BaseHTTPRequestHandler):
        def do_GET(self):
            if self.path == '/stop/'+token:
                self.send_response(200);self.end_headers();threading.Thread(target=server.shutdown,daemon=True).start();return
            text=value.read_text()
            body=f'<!doctype html><title>NUS-BENCH-{a.name}-{html.escape(text)}</title><body data-value="{html.escape(text,quote=True)}"><h1>{html.escape(a.name)}</h1><p>{html.escape(text)}</p></body>'.encode()
            self.send_response(200);self.send_header('Content-Type','text/html');self.send_header('Content-Length',str(len(body)));self.send_header('Cache-Control','no-store');self.end_headers();self.wfile.write(body)
        def log_message(self,*args):pass
    try:server=ThreadingHTTPServer(('127.0.0.1',a.port),Handler)
    except OSError as e:
        import errno
        if e.errno != errno.EADDRINUSE:raise
        (root/(a.name+'.conflict')).write_text('address-in-use');return
    (root/(a.name+'.pid')).write_text(str(os.getpid()))
    done=threading.Event()
    def load():
        seq=0
        while not done.wait(.01):
            print(f'NUS_LOAD {seq:08d} '+('0123456789abcdef'*64),flush=True)
            seq+=1
            if seq%10==0:
                pending=root/(a.name+'.load.tmp');pending.write_text(str(seq));pending.replace(root/(a.name+'.load'))
    if a.load:threading.Thread(target=load,daemon=True).start()
    timer=threading.Timer(60,server.shutdown);timer.daemon=True;timer.start()
    try:server.serve_forever(poll_interval=.05)
    except KeyboardInterrupt:pass
    finally:
        done.set();timer.cancel();server.server_close();(root/(a.name+'.stopped')).write_text('stopped')


if __name__=='__main__':main()
