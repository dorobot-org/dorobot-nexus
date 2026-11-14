#!/usr/bin/env python3
"""Filesystem/UI bridge to the authenticated agent. GUI heartbeat owns the lease."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.request
from agent import atomic,pack


class Client:
    def __init__(self,url,token):
        from urllib.parse import urlsplit
        parsed=urlsplit(url)
        if parsed.scheme not in ('http','https') or parsed.username or parsed.password or parsed.query or parsed.fragment:
            raise ValueError('Use an HTTP(S) agent endpoint without credentials in the URL')
        if parsed.scheme=='http' and parsed.hostname not in ('localhost','127.0.0.1','::1'):
            raise ValueError('Remote agents need HTTPS or an SSH tunnel to localhost')
        self.url=url.rstrip('/');self.token=token;self.owner=None;self.seq=0;self.lock=threading.Lock()
        # Never send a robot credential to an environment proxy or redirect target.
        class NoRedirect(urllib.request.HTTPRedirectHandler):
            def redirect_request(self,*args,**kwargs):return None
        self.http=urllib.request.build_opener(urllib.request.ProxyHandler({}),NoRedirect)
    def call(self,path,data=None):
        request=urllib.request.Request(self.url+path,data=data,headers={'Authorization':'Bearer '+self.token,'Content-Type':'application/json'})
        try:
            with self.http.open(request,timeout=15 if path=='/bundle' else 4) as reply:return json.loads(reply.read(4*1024*1024))
        except urllib.error.HTTPError as error:
            raise RuntimeError(error.read(65536).decode()) from error
    def command(self,op,**kwargs):
        with self.lock:
            self.seq+=1
            result=self.call('/command',json.dumps(dict(op=op,owner=self.owner,sequence=self.seq,**kwargs)).encode())
            if op=='attach':self.owner=result.pop('owner')
            return result


def bridge(args):
    args.session.mkdir(parents=True,exist_ok=True)
    cfg=json.loads((args.session/'config.json').read_text())
    client=Client(cfg['url'],Path(cfg['token_file']).read_text().strip())
    current=dict(state='connecting',ack=0,error=None,agent=None)
    pending=None;seen=set();closed=threading.Event();last_heartbeat=0.
    publish_lock=threading.Lock()
    def publish():
        with publish_lock:atomic(args.session/'status.json',current)
    def heartbeats():
        nonlocal last_heartbeat
        while not closed.wait(.2):
            try:
                file=args.session/'heartbeat'
                if time.time()-file.stat().st_mtime>1.2:
                    client.command('stop');closed.set();return
                if client.owner:
                    reply=client.command('heartbeat')
                    current['agent']=reply;last_heartbeat=time.monotonic()
            except Exception as error:
                current['error']='Connection lost: '+str(error)
                current['state']='disconnected'
                # Do not invent a stop acknowledgement. The target watchdog owns lease expiry.
            publish()
    try:
        current['agent']=client.command('attach');current['state']='connected';publish()
        thread=threading.Thread(target=heartbeats,daemon=True);thread.start()
        while not closed.is_set():
            for file in sorted(args.session.glob('command-*.json')):
                if file.name in seen:continue
                seen.add(file.name);request=json.loads(file.read_text());seq=request['id'];op=request['op']
                try:
                    current['error']=None
                    if op=='disconnect':
                        current['agent']=client.command('stop');closed.set()
                    elif op=='prepare':
                        if current['agent']['state']=='armed':raise ValueError('Stop before preparing another checkpoint')
                        current['state']='exporting';publish()
                        folder=args.session/f'policy-{seq}'
                        cmd=[sys.executable,str(args.root/'tools/training/export_checkpoint.py'),'--checkpoint',request['checkpoint'],
                             '--output',str(folder),'--robot',cfg['robot'],'--studio-root',str(args.root)]
                        with (args.session/f'export-{seq}.log').open('w') as log:
                            pending=subprocess.Popen(cmd,stdout=log,stderr=subprocess.STDOUT)
                            deadline=time.monotonic()+120
                            while pending.poll() is None:
                                if closed.wait(.1) or time.monotonic()>deadline:
                                    pending.terminate();pending.wait(timeout=5);raise RuntimeError('Export cancelled or timed out')
                            if pending.returncode:raise RuntimeError(f'Export failed; see {args.session}/export-{seq}.log')
                        current['agent']=client.call('/bundle',pack(folder));pending=None
                    else:
                        current['agent']=client.command(op,**{k:v for k,v in request.items() if k not in ('op','id')})
                    current['state']='connected' if not closed.is_set() else 'closed'
                except Exception as error:current['error']=str(error);current['state']='connected'
                current['ack']=seq;publish()
            time.sleep(.05)
    except Exception as error:current['error']=str(error);current['state']='failed'
    finally:
        closed.set()
        if pending and pending.poll() is None:pending.terminate()
        try:current['agent']=client.command('stop')
        except Exception as error:current['stop_error']=str(error)
        publish()


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--session',type=Path,required=True);p.add_argument('--root',type=Path,default=Path(__file__).resolve().parents[2]);bridge(p.parse_args())
