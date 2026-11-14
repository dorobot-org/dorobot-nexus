#!/usr/bin/env python3
"""Authenticated transport calls this stdlib-only, persistent training supervisor."""
import argparse
import base64
import fcntl
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import signal
import subprocess
import sys
import threading
import time

VERSION=1
TERMINAL={'Completed','Failed','Stopped'}

def atomic(path,value):
    path=Path(path);temp=path.with_name(path.name+f'.{os.getpid()}.tmp')
    with temp.open('w') as f:
        json.dump(value,f,allow_nan=False);f.write('\n');f.flush();os.fsync(f.fileno())
    temp.replace(path)

def digest(data):return hashlib.sha256(data).hexdigest()
def local(root,name):
    relative=PurePosixPath(name)
    if relative.is_absolute() or not relative.parts or any(p in ('..','.') for p in relative.parts) or '\\' in name:
        raise ValueError('Non-local job path')
    p=Path(root)
    for part in relative.parts:
        p=p/part
        if p.is_symlink():raise ValueError('Linked job files are not accessible')
    return p

def owned(root,token):
    root=Path(root)
    if not root.is_absolute() or root.is_symlink():raise ValueError('Job root must be an absolute owned directory')
    spec=json.loads((root/'.job/spec.json').read_text())
    if spec['version']!=VERSION or spec['token']!=token:raise ValueError('Job identity mismatch')
    return spec

def alive(root):
    with (root/'.job/lock').open('a') as f:
        try:fcntl.flock(f,fcntl.LOCK_EX|fcntl.LOCK_NB)
        except BlockingIOError:return True
        return False

def status(root,token):
    root=Path(root);owned(root,token)
    p=root/'.job/state.json'
    state=json.loads(p.read_text()) if p.exists() else dict(state='Unknown',error='Job status is missing; worker termination is unproven')
    if state['state'] not in TERMINAL and state['state']!='Queued' and not alive(root):
        state.update(state='Unknown',error='Supervisor is unavailable. Worker termination is unproven; automatic relaunch is blocked.')
    return {**state,"version":VERSION,"token":token}

def prepare(req):
    root=Path(req['root']);spec=req['spec']
    if not root.is_absolute() or root.is_symlink():raise ValueError('Invalid job directory')
    marker=root/'.job/spec.json'
    if marker.exists():
        current=owned(root,spec['token'])
        if current!=spec:raise ValueError('Existing job has another immutable specification')
        return status(root,spec['token'])
    root.mkdir(parents=True,exist_ok=True)
    for name,entry in req.get('files',{}).items():
        p=local(root,name);data=base64.b64decode(entry['data'],validate=True)
        if digest(data)!=entry['sha256']:raise ValueError('Input file checksum differs')
        if p.exists():
            if not p.is_file() or digest(p.read_bytes())!=entry['sha256']:raise ValueError('Input collision; existing data preserved')
        else:
            p.parent.mkdir(parents=True,exist_ok=True)
            with p.open('xb') as f:f.write(data)
    (root/'.job').mkdir(exist_ok=True)
    atomic(marker,spec)
    atomic(root/'.job/state.json',dict(state='Queued',updated=time.time(),token=spec['token']))
    return status(root,spec['token'])

def start(root,token):
    root=Path(root);spec=owned(root,token);current=status(root,token)
    if current['state']!='Queued':return current
    # The lock inside supervise is authoritative when concurrent starts race.
    with (root/'.job/supervisor.log').open('ab') as log:
        child=subprocess.Popen([sys.executable,str(Path(__file__).resolve()),'supervise',str(root),token],
                         stdin=subprocess.DEVNULL,stdout=log,stderr=log,start_new_session=True,close_fds=True)
    # Reap when the client stays alive; client exit does not own the supervisor.
    threading.Thread(target=child.wait,daemon=True).start()
    for _ in range(100):
        current=status(root,token)
        if current['state']!='Queued':return current
        time.sleep(.02)
    return current

def control(root,token,action,request_id):
    root=Path(root);owned(root,token)
    if action not in ('pause','resume','stop'):raise ValueError('Unknown job command')
    state=status(root,token)
    if state['state'] in TERMINAL:return state
    if state['state']=='Unknown':raise ValueError(state['error'])
    if state['state']=='Queued':
        if action!='stop':raise ValueError('Job has not started')
        # Queued cancellation and start both take the same supervisor lock.
        with (root/'.job/lock').open('a') as lock:
            fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
            atomic(root/'.job/state.json',dict(state='Stopped',updated=time.time(),reason='Cancelled before launch'))
        return status(root,token)
    atomic(root/'.job/control.json',dict(action=action,id=request_id,token=token))
    return state

def supervise(root,token):
    root=Path(root);spec=owned(root,token)
    lock=(root/'.job/lock').open('a')
    try:fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
    except BlockingIOError:return
    state=json.loads((root/'.job/state.json').read_text())
    if state['state']!='Queued':return
    state=dict(state='Starting',updated=time.time(),supervisor_pid=os.getpid(),launches=1,command=spec['command'],token=token)
    def publish(**values):
        state.update(values,updated=time.time());atomic(root/'.job/state.json',state)
    publish()
    try:
        if spec.get('input_error'):raise ValueError(spec['input_error'])
        for name,expected in spec.get('inputs',{}).items():
            if digest(local(root,name).read_bytes())!=expected:raise ValueError('Captured input changed: '+name)
        for name,expected in spec.get('source_checks',{}).items():
            if digest(Path(name).read_bytes())!=expected:raise ValueError('Trainer source changed after enqueue: '+name)
        with (root/'job-events.jsonl').open('xb') as output,(root/'job-stderr.log').open('xb') as errors:
            env=os.environ.copy();env.update(spec.get('env',{}));env['NEXUS_SUPERVISED']='1'
            worker=subprocess.Popen(spec['command'],cwd=spec['cwd'],env=env,stdin=subprocess.DEVNULL,stdout=output,stderr=errors,start_new_session=True)
            publish(state='Running',worker_pid=worker.pid,started=time.time())
            handled=None;stop_at=None
            while True:
                control_path=root/'.job/control.json'
                if control_path.exists():
                    command=json.loads(control_path.read_text())
                    if command['token']!=token:raise ValueError('Control identity mismatch')
                    if command['id']!=handled:
                        handled=command['id'];action=command['action']
                        try:
                            if action=='stop':
                                os.killpg(worker.pid,signal.SIGCONT);os.killpg(worker.pid,signal.SIGTERM)
                                stop_at=time.monotonic();publish(state='Stopping',ack=handled)
                            elif stop_at is None:
                                os.killpg(worker.pid,signal.SIGSTOP if action=='pause' else signal.SIGCONT)
                        except ProcessLookupError:
                            # Exit can race a control request. waitpid below owns the result.
                            pass
                flags=os.WNOHANG|os.WUNTRACED|getattr(os,'WCONTINUED',0)
                pid,code=os.waitpid(worker.pid,flags)
                if pid:
                    if os.WIFEXITED(code) or os.WIFSIGNALED(code):
                        worker.returncode=os.waitstatus_to_exitcode(code)
                        # A trainer must not leave data-loader or launcher children behind.
                        try:os.killpg(worker.pid,signal.SIGKILL)
                        except ProcessLookupError:pass
                        publish(state='Stopped' if stop_at is not None else 'Completed' if worker.returncode==0 else 'Failed',returncode=worker.returncode,finished=time.time(),ack=handled,
                                error=None if worker.returncode==0 or stop_at is not None else 'Worker failed; see job-stderr.log and job-events.jsonl')
                        break
                    if os.WIFSTOPPED(code):publish(state='Paused',ack=handled)
                    elif hasattr(os,'WIFCONTINUED') and os.WIFCONTINUED(code):publish(state='Stopping' if stop_at else 'Running',ack=handled)
                if stop_at is not None and time.monotonic()-stop_at>5:
                    try:os.killpg(worker.pid,signal.SIGKILL)
                    except ProcessLookupError:pass
                time.sleep(.05)
    except Exception as e:
        if 'worker' in locals() and worker.returncode is None:
            try:os.killpg(worker.pid,signal.SIGKILL)
            except ProcessLookupError:pass
            worker.wait()
        publish(state='Failed',error=str(e),finished=time.time())
    finally:lock.close()

def inventory(root,token):
    root=Path(root);owned(root,token);out={}
    for p in root.rglob('*'):
        relative=p.relative_to(root).as_posix()
        if relative.startswith('.job/') or relative.startswith('inputs/') or relative in ('job-request.json','job-worker.json','run.json'):continue
        if p.is_symlink():continue
        if p.is_file():
            try:local(root,relative)
            except ValueError:continue
            m=p.stat();out[relative]=[m.st_size,m.st_mtime_ns]
    return out

def rpc(req):
    op=req['op']
    if op=='prepare':return prepare(req)
    root=Path(req['root']);token=req['token']
    if op=='probe':
        if not root.is_absolute() or root.is_symlink():raise ValueError('Invalid job directory')
        return status(root,token) if (root/'.job/spec.json').exists() else None
    if op=='start':return start(root,token)
    if op=='status':return status(root,token)
    if op=='control':return control(root,token,req['action'],req['request_id'])
    if op=='inventory':return inventory(root,token)
    if op=='file':
        owned(root,token);path=local(root,req['path'])
        if not path.is_file():raise ValueError('Output is not a regular file')
        with path.open('rb') as f:
            before=os.fstat(f.fileno());stamp=[before.st_size,before.st_mtime_ns]
            if 'stamp' in req and req['stamp']!=stamp:raise ValueError('Output changed during transfer')
            f.seek(req.get('offset',0));data=f.read(min(req.get('limit',4*1024*1024),4*1024*1024))
            after=os.fstat(f.fileno());current=path.stat()
            if (before.st_ino,before.st_size,before.st_mtime_ns)!=(after.st_ino,after.st_size,after.st_mtime_ns) or (before.st_ino,before.st_size,before.st_mtime_ns)!=(current.st_ino,current.st_size,current.st_mtime_ns):
                raise ValueError('Output changed during transfer')
        return dict(data=base64.b64encode(data).decode(),sha256=digest(data),stamp=stamp)
    raise ValueError('Unknown job operation')

def main():
    if len(sys.argv)>1 and sys.argv[1]=='supervise':supervise(Path(sys.argv[2]),sys.argv[3]);return
    try:print(json.dumps(dict(ok=True,result=rpc(json.load(sys.stdin))),allow_nan=False))
    except Exception as e:print(json.dumps(dict(ok=False,error=str(e))))
if __name__=='__main__':main()
